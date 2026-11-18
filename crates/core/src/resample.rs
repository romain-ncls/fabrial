//! Streaming sample-rate conversion, for feeding a recorded take to a chain
//! that runs at another rate (the headset's 16 kHz to DeepFilterNet3's
//! 48 kHz). Live, PipeWire converts instead.

use anyhow::{Result, anyhow};
use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Indexing, Resampler as _};

const CHUNK: usize = 1024;

pub struct Resampler {
    inner: Fft<f32>,
    ratio: f64,
    pending: Vec<f32>,
    chunk_out: Vec<f32>,
    to_trim: usize,
    consumed: usize,
    produced: usize,
}

impl Resampler {
    pub fn new(from: u32, to: u32) -> Result<Self> {
        let inner = Fft::<f32>::new(from as usize, to as usize, CHUNK, 1, FixedSync::Input)
            .map_err(|e| anyhow!("resampler {from} → {to} Hz: {e}"))?;
        Ok(Self {
            to_trim: inner.output_delay(),
            chunk_out: vec![0.; inner.output_frames_max()],
            ratio: to as f64 / from as f64,
            pending: Vec::with_capacity(CHUNK),
            consumed: 0,
            produced: 0,
            inner,
        })
    }

    /// Converts `input`, appending what is ready to `out`. The output lags by
    /// up to one chunk; `finish` returns the rest.
    pub fn push(&mut self, input: &[f32], out: &mut Vec<f32>) {
        self.consumed += input.len();
        let mut rest = input;
        while !rest.is_empty() {
            let take = (CHUNK - self.pending.len()).min(rest.len());
            self.pending.extend_from_slice(&rest[..take]);
            rest = &rest[take..];
            if self.pending.len() == CHUNK {
                self.run(None, out);
                self.pending.clear();
            }
        }
    }

    /// Converts what is left, then the filter's tail, so the whole output is
    /// exactly the input's length times the ratio.
    pub fn finish(&mut self, out: &mut Vec<f32>) {
        let total = (self.consumed as f64 * self.ratio).ceil() as usize;
        let len = self.pending.len();
        self.pending.resize(CHUNK, 0.);
        self.run(Some(len), out);
        while self.produced < total {
            self.run(Some(0), out);
        }
        out.truncate(out.len() - (self.produced - total));
        self.produced = total;
    }

    fn run(&mut self, partial: Option<usize>, out: &mut Vec<f32>) {
        let input = InterleavedSlice::new(&self.pending[..], 1, CHUNK).unwrap();
        let frames = self.chunk_out.len();
        let mut output = InterleavedSlice::new_mut(&mut self.chunk_out[..], 1, frames).unwrap();
        let indexing = Indexing {
            partial_len: partial,
            ..Indexing::default()
        };
        let (_, n) = self
            .inner
            .process_into_buffer(&input, &mut output, Some(&indexing))
            .expect("buffers sized from the resampler's own limits");
        let trim = self.to_trim.min(n);
        self.to_trim -= trim;
        out.extend_from_slice(&self.chunk_out[trim..n]);
        self.produced += n - trim;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn length_and_alignment() {
        // A 1 kHz sine at 16 kHz, converted to 48 kHz in uneven blocks.
        let n = 16_000 + 123;
        let sine =
            |rate: f32, i: usize| (2. * std::f32::consts::PI * 1000. * i as f32 / rate).sin();
        let input: Vec<f32> = (0..n).map(|i| sine(16_000., i)).collect();
        let mut r = Resampler::new(16_000, 48_000).unwrap();
        let mut out = Vec::new();
        for block in input.chunks(333) {
            r.push(block, &mut out);
        }
        r.finish(&mut out);
        assert_eq!(out.len(), n * 3);
        // No delay: away from the edges, the output is the same sine.
        let err = (4800..out.len() - 4800)
            .map(|i| (out[i] - sine(48_000., i)).abs())
            .fold(0f32, f32::max);
        assert!(err < 1e-3, "max error {err}");
    }
}
