//! Reading takes and writing renders.

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use anyhow::{Context, Result, bail};

/// A recording, mixed down to mono.
pub struct Mono {
    pub rate: u32,
    pub samples: Vec<f32>,
}

/// Reads a FLAC or WAV file. Several channels are averaged.
pub fn read(path: &Path) -> Result<Mono> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default();
    match ext {
        "flac" => read_flac(path),
        "wav" => read_wav(path),
        _ => bail!("{}: only .flac and .wav are read", path.display()),
    }
    .with_context(|| format!("reading {}", path.display()))
}

fn read_flac(path: &Path) -> Result<Mono> {
    let mut r = claxon::FlacReader::open(path)?;
    let info = r.streaminfo();
    let scale = 1. / (1u64 << (info.bits_per_sample - 1)) as f32;
    let ch = info.channels as usize;
    let mut samples = Vec::with_capacity(info.samples.unwrap_or(0) as usize);
    let mut sum = 0.;
    for (i, s) in r.samples().enumerate() {
        sum += s? as f32 * scale;
        if (i + 1) % ch == 0 {
            samples.push(sum / ch as f32);
            sum = 0.;
        }
    }
    Ok(Mono {
        rate: info.sample_rate,
        samples,
    })
}

fn read_wav(path: &Path) -> Result<Mono> {
    let mut r = hound::WavReader::open(path)?;
    let spec = r.spec();
    let ch = spec.channels as usize;
    let interleaved: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => r.samples::<f32>().collect::<Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let scale = 1. / (1u64 << (spec.bits_per_sample - 1)) as f32;
            r.samples::<i32>()
                .map(|s| s.map(|s| s as f32 * scale))
                .collect::<Result<_, _>>()?
        }
    };
    let samples = interleaved
        .chunks_exact(ch)
        .map(|f| f.iter().sum::<f32>() / ch as f32)
        .collect();
    Ok(Mono {
        rate: spec.sample_rate,
        samples,
    })
}

/// A 32-bit float WAV file, written as it goes. Float keeps what goes over
/// full scale, so clipping shows in the analysis instead of being hidden.
pub struct Writer {
    inner: hound::WavWriter<BufWriter<File>>,
    channels: usize,
}

impl Writer {
    pub fn create(path: &Path, rate: u32, channels: usize, frames: usize) -> Result<Self> {
        // WAV sizes are 32-bit.
        let bytes = frames as u64 * channels as u64 * 4;
        if bytes >= u32::MAX as u64 {
            bail!(
                "{}: {:.1} GB is over WAV's 4 GB; render a shorter stretch (--from/--to)",
                path.display(),
                bytes as f64 / 1e9
            );
        }
        let spec = hound::WavSpec {
            channels: channels as u16,
            sample_rate: rate,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let inner = hound::WavWriter::create(path, spec)
            .with_context(|| format!("creating {}", path.display()))?;
        Ok(Self { inner, channels })
    }

    /// Writes `frames` frames, taking channel `c` of frame `i` from `get(c, i)`.
    pub fn write(&mut self, frames: usize, get: impl Fn(usize, usize) -> f32) -> Result<()> {
        for i in 0..frames {
            for c in 0..self.channels {
                self.inner.write_sample(get(c, i))?;
            }
        }
        Ok(())
    }

    pub fn finish(self) -> Result<()> {
        Ok(self.inner.finalize()?)
    }
}
