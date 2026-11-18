//! A lookahead peak limiter: no sample leaves above the ceiling.
//!
//! For each sample, the gain that would bring it to the ceiling is computed
//! as it enters. The audio is delayed by the lookahead while the gain takes
//! the minimum over that window (so it is already down when a peak leaves),
//! recovers with the release time, and is smoothed by a moving average over
//! the window. At the moment a peak leaves the delay line, every value in
//! that average is at or below the gain the peak needs, so the output never
//! passes the ceiling. Inter-sample peaks can, slightly: keep the ceiling
//! a little under 0 dBFS.

use std::collections::VecDeque;

use serde::Deserialize;

use crate::{Stage, db_to_gain};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct LimiterParams {
    /// The highest level a sample may leave at, in dBFS.
    #[serde(default = "default_ceiling")]
    pub ceiling: f32,
    /// How far ahead peaks are seen, in ms. This is the stage's delay.
    #[serde(default = "default_lookahead")]
    pub lookahead: f32,
    /// Time for the gain to recover by about 63 %, in ms.
    #[serde(default = "default_release")]
    pub release: f32,
}

fn default_ceiling() -> f32 {
    -1.
}
fn default_lookahead() -> f32 {
    1.5
}
fn default_release() -> f32 {
    60.
}

pub struct Limiter {
    ceiling: f32,
    window: usize,
    release: f32,
    /// The audio, delayed by `window - 1` samples.
    delay: VecDeque<f32>,
    /// (index, gain) pairs, increasing gains: the window's minimum is first.
    mins: VecDeque<(u64, f32)>,
    released: f32,
    /// The last `window` released gains, and their sum.
    avg: VecDeque<f32>,
    sum: f64,
    n: u64,
}

impl Limiter {
    pub fn new(p: &LimiterParams, rate: u32) -> Self {
        let window = ((p.lookahead / 1000. * rate as f32).round() as usize).max(1);
        Self {
            ceiling: db_to_gain(p.ceiling),
            release: 1. - (-1. / (p.release / 1000. * rate as f32)).exp(),
            delay: VecDeque::from(vec![0.; window - 1]),
            mins: VecDeque::with_capacity(window + 1),
            released: 1.,
            avg: VecDeque::from(vec![1.; window]),
            sum: window as f64,
            n: 0,
            window,
        }
    }

    fn next(&mut self, x: f32) -> f32 {
        let need = if x.abs() > self.ceiling {
            self.ceiling / x.abs()
        } else {
            1.
        };
        // Sliding minimum over the last `window` samples.
        while self.mins.back().is_some_and(|&(_, g)| g >= need) {
            self.mins.pop_back();
        }
        self.mins.push_back((self.n, need));
        if self.mins[0].0 + self.window as u64 <= self.n {
            self.mins.pop_front();
        }
        let min = self.mins[0].1;
        self.n += 1;
        // Down at once, up with the release time.
        self.released = if min < self.released {
            min
        } else {
            self.released + (min - self.released) * self.release
        };
        // Moving average over the window.
        self.sum += (self.released - self.avg.pop_front().unwrap()) as f64;
        self.avg.push_back(self.released);
        let gain = (self.sum / self.window as f64) as f32;

        self.delay.push_back(x);
        self.delay.pop_front().unwrap() * gain
    }
}

impl Stage for Limiter {
    fn process(&mut self, buf: &mut [f32]) {
        for s in buf {
            *s = self.next(*s);
        }
    }

    fn latency(&self) -> usize {
        self.window - 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limiter(rate: u32) -> Limiter {
        Limiter::new(&toml::from_str("").unwrap(), rate)
    }

    #[test]
    fn nothing_passes_the_ceiling() {
        let mut l = limiter(48_000);
        let ceiling = db_to_gain(-1.);
        // Bursts of a 440 Hz sine at +6 dBFS between quiet stretches, and a
        // single-sample spike.
        let mut x: Vec<f32> = (0..48_000)
            .map(|i| {
                let a = if (i / 4800) % 2 == 1 { 2. } else { 0.1 };
                a * (2. * std::f32::consts::PI * 440. * i as f32 / 48_000.).sin()
            })
            .collect();
        x[30_000] = 8.;
        l.process(&mut x);
        let peak = x.iter().fold(0f32, |m, s| m.max(s.abs()));
        assert!(peak <= ceiling * 1.000_01, "peak {peak} over {ceiling}");
    }

    #[test]
    fn quiet_audio_only_gets_delayed() {
        let mut l = limiter(16_000);
        let x: Vec<f32> = (0..1600).map(|i| 0.5 * (i as f32 * 0.1).sin()).collect();
        let mut y = x.clone();
        l.process(&mut y);
        let d = l.latency();
        for i in d..x.len() {
            assert!((y[i] - x[i - d]).abs() < 1e-6);
        }
    }
}
