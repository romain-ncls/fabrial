//! `fabrial render`: a recorded take through a chain, written aligned with
//! the raw input so both can be compared sample by sample.

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use fabrial_core::Chain;
use fabrial_core::resample::Resampler;

use crate::audio;

#[derive(clap::Args)]
pub struct Args {
    /// A take folder (its raw.flac is used) or an audio file (.flac, .wav).
    input: PathBuf,
    /// The chain file.
    #[arg(short, long)]
    chain: PathBuf,
    /// Output folder. Default: renders/<chain name>[-<from>-<to>] in the take.
    #[arg(short, long)]
    out: Option<PathBuf>,
    /// Start of the stretch to render (seconds, m:ss or h:mm:ss).
    #[arg(long, value_parser = crate::parse_time)]
    from: Option<f64>,
    /// End of the stretch to render.
    #[arg(long, value_parser = crate::parse_time)]
    to: Option<f64>,
    /// Also write taps.wav: the input, then each stage's output, one channel
    /// each, aligned.
    #[arg(long)]
    taps: bool,
}

const BLOCK: usize = 1024;

pub fn run(a: Args) -> Result<()> {
    let input = if a.input.is_dir() {
        a.input.join("raw.flac")
    } else {
        a.input.clone()
    };
    let take = if a.input.is_dir() {
        a.input.clone()
    } else {
        a.input.parent().map(Path::to_path_buf).unwrap_or_default()
    };
    let mut chain = Chain::load(&a.chain)?;
    let name = chain.file.name.clone();
    let rate = chain.rate();

    let rec = audio::read(&input)?;
    let len = rec.samples.len();
    let at = |t: Option<f64>, default: usize| {
        t.map_or(default, |t| ((t * rec.rate as f64) as usize).min(len))
    };
    let (from, to) = (at(a.from, 0), at(a.to, len));
    if from >= to {
        bail!("nothing to render between {from} and {to}");
    }
    let src = &rec.samples[from..to];

    let out_dir = a.out.clone().unwrap_or_else(|| {
        let mut n = name.clone();
        if a.from.is_some() || a.to.is_some() {
            write!(
                n,
                "-{}-{}",
                dir_stamp(from, rec.rate),
                dir_stamp(to, rec.rate)
            )
            .unwrap();
        }
        crate::renders_dir(&take).join(n)
    });
    std::fs::create_dir_all(&out_dir).with_context(|| format!("creating {}", out_dir.display()))?;
    std::fs::copy(&a.chain, out_dir.join("chain.toml"))?;

    // Channels: the input at the chain's rate, then each stage's output.
    // Each is delayed by the stages before it, so it skips that much.
    let names = chain.tap_names();
    let mut delays = vec![0];
    for l in chain.latencies() {
        delays.push(delays.last().unwrap() + l);
    }
    let total_delay = *delays.last().unwrap();
    let frames = (src.len() as f64 * rate as f64 / rec.rate as f64).ceil() as usize;
    let mut out = audio::Writer::create(&out_dir.join("out.wav"), rate, 1, frames)?;
    let mut taps = if a.taps {
        Some(audio::Writer::create(
            &out_dir.join("taps.wav"),
            rate,
            delays.len(),
            frames,
        )?)
    } else {
        None
    };
    let mut align = Aligner::new(&delays, frames);

    let t0 = Instant::now();
    let mut resampler = if rec.rate != rate {
        Some(Resampler::new(rec.rate, rate)?)
    } else {
        None
    };
    let mut tap_bufs = vec![Vec::new(); names.len()];
    let mut block = Vec::with_capacity(BLOCK * 4);
    let mut step = |block: &mut Vec<f32>, align: &mut Aligner| -> Result<()> {
        align.push(0, block);
        tap_bufs.iter_mut().for_each(Vec::clear);
        chain.process_tapped(block, &mut tap_bufs);
        for (c, t) in tap_bufs.iter().enumerate() {
            align.push(c + 1, t);
        }
        align.drain(&mut out, taps.as_mut())
    };
    for chunk in src.chunks(BLOCK) {
        block.clear();
        match &mut resampler {
            Some(r) => r.push(chunk, &mut block),
            None => block.extend_from_slice(chunk),
        }
        step(&mut block, &mut align)?;
    }
    block.clear();
    if let Some(r) = &mut resampler {
        r.finish(&mut block);
    }
    step(&mut block, &mut align)?;
    // Push the chain's delay out with silence.
    let mut left = total_delay;
    while left > 0 {
        let n = left.min(BLOCK);
        block.clear();
        block.resize(n, 0.);
        step(&mut block, &mut align)?;
        left -= n;
    }
    out.finish()?;
    if let Some(t) = taps {
        t.finish()?;
    }
    let took = t0.elapsed().as_secs_f64();
    let audio_s = frames as f64 / rate as f64;

    let mut report = String::new();
    writeln!(report, "input     {}", input.display())?;
    writeln!(
        report,
        "stretch   {} to {} ({:.1} s at {} Hz)",
        stamp(from, rec.rate),
        stamp(to, rec.rate),
        src.len() as f64 / rec.rate as f64,
        rec.rate
    )?;
    writeln!(
        report,
        "start     {:.6} s into the input",
        from as f64 / rec.rate as f64
    )?;
    writeln!(
        report,
        "chain     {} ({}), at {rate} Hz",
        name,
        a.chain.display()
    )?;
    for (n, l) in names.iter().zip(chain.latencies()) {
        writeln!(report, "  {n:<20} latency {:>5.1} ms", ms(l, rate))?;
    }
    writeln!(
        report,
        "  {:<20} latency {:>5.1} ms (removed in the files)",
        "total",
        ms(total_delay, rate)
    )?;
    writeln!(
        report,
        "files     out.wav{}",
        if a.taps {
            ", taps.wav (input, then each stage)"
        } else {
            ""
        }
    )?;
    writeln!(
        report,
        "speed     {took:.1} s for {audio_s:.1} s of audio ({:.0}x real time)",
        audio_s / took
    )?;
    std::fs::write(out_dir.join("render.txt"), &report)?;
    print!("{report}");
    println!("→ {}", out_dir.display());
    Ok(())
}

/// Lines up the channels: each drops its first `delay` samples, and frames
/// are written once every channel has them.
struct Aligner {
    skip: Vec<usize>,
    queues: Vec<VecDeque<f32>>,
    left: usize,
}

impl Aligner {
    fn new(delays: &[usize], frames: usize) -> Self {
        Self {
            skip: delays.to_vec(),
            queues: vec![VecDeque::new(); delays.len()],
            left: frames,
        }
    }

    fn push(&mut self, c: usize, samples: &[f32]) {
        let skip = self.skip[c].min(samples.len());
        self.skip[c] -= skip;
        self.queues[c].extend(&samples[skip..]);
    }

    /// Writes the frames all channels have: the last channel to `out`, all
    /// of them to `taps`.
    fn drain(&mut self, out: &mut audio::Writer, taps: Option<&mut audio::Writer>) -> Result<()> {
        let n = self
            .queues
            .iter()
            .map(VecDeque::len)
            .min()
            .unwrap()
            .min(self.left);
        let q = &self.queues;
        let last = q.len() - 1;
        out.write(n, |_, i| q[last][i])?;
        if let Some(t) = taps {
            t.write(n, |c, i| q[c][i])?;
        }
        for q in &mut self.queues {
            q.drain(..n);
        }
        self.left -= n;
        Ok(())
    }
}

fn ms(samples: usize, rate: u32) -> f64 {
    samples as f64 * 1000. / rate as f64
}

/// A sample position as h:mm:ss.d or m:ss.d.
fn stamp(pos: usize, rate: u32) -> String {
    let t = pos as f64 / rate as f64;
    let (h, m, s) = ((t / 3600.) as u64, (t % 3600. / 60.) as u64, t % 60.);
    if h > 0 {
        format!("{h}:{m:02}:{s:04.1}")
    } else {
        format!("{m}:{s:04.1}")
    }
}

/// A sample position for a folder name: 2m05 or 1h02m05.
fn dir_stamp(pos: usize, rate: u32) -> String {
    let t = (pos as f64 / rate as f64) as u64;
    let (h, m, s) = (t / 3600, t % 3600 / 60, t % 60);
    if h > 0 {
        format!("{h}h{m:02}m{s:02}")
    } else {
        format!("{m}m{s:02}")
    }
}
