//! Downward expander and compressor, computed as LSP Plugins does
//! (lsp-dsp-units 1.0.39, the version in lsp-plugins 1.2.37), so the copy
//! of the EasyEffects chain matches it to float precision. All in f32, as
//! LSP computes.
//!
//! Per sample:
//! 1. Sidechain: RMS of the input over a boxcar window ("reactivity").
//! 2. Envelope: a follower on that level, with the attack time when it
//!    rises and the release time when it falls.
//! 3. Gain: the curve at the envelope, with a quadratic knee in the log
//!    domain around the threshold.
//! 4. Output: input × gain × makeup × wet, plus input × dry.

use serde::Deserialize;

use crate::{Stage, db_to_gain};

/// The settings, under the names and units EasyEffects shows.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct DynamicsParams {
    /// dBFS.
    pub threshold: f32,
    pub ratio: f32,
    /// ms.
    pub attack: f32,
    /// ms.
    pub release: f32,
    /// dB, at most 0: the knee spans this far on each side of the threshold.
    #[serde(default = "default_knee")]
    pub knee: f32,
    /// dB, on the processed signal only.
    #[serde(default)]
    pub makeup: f32,
    /// dB, relative to the threshold: while the envelope falls below it,
    /// the attack time is used instead of the release time. Absent: off.
    #[serde(default)]
    pub release_threshold: Option<f32>,
    /// dB of the unprocessed input mixed into the output. Absent: none.
    /// EasyEffects' lowest setting, -80.01, still sends -80 dB.
    #[serde(default)]
    pub dry: Option<f32>,
    /// dB of the processed signal in the output.
    #[serde(default)]
    pub wet: f32,
    /// ms: the sidechain's RMS window.
    #[serde(default = "default_reactivity")]
    pub reactivity: f32,
}

fn default_knee() -> f32 {
    -6.
}
fn default_reactivity() -> f32 {
    10.
}

#[derive(Clone, Copy)]
enum Kind {
    Compressor,
    Expander,
}

/// LSP's interpolation::hermite_quadratic: p(x) with p(x0) = y0,
/// p'(x0) = k0 and p'(x1) = k1.
fn hermite_quadratic(x0: f32, y0: f32, k0: f32, x1: f32, k1: f32) -> [f32; 3] {
    let a = (k0 - k1) * 0.5 / (x0 - x1);
    let b = k0 - 2. * a * x0;
    let c = y0 - (a * x0 + b) * x0;
    [a, b, c]
}

pub struct Dynamics {
    kind: Kind,
    // Sidechain: |x| history over the window, and the sum of squares.
    window: usize,
    history: Vec<f32>,
    pos: usize,
    sum: f32,
    since_refresh: usize,
    // Envelope.
    env: f32,
    tau_attack: f32,
    tau_release: f32,
    release_level: f32,
    // Curve.
    start: f32,
    end: f32,
    knee: [f32; 3],
    tilt: [f32; 2],
    floor: f32,
    // Output.
    wet: f32,
    dry: f32,
}

/// LSP recomputes the sidechain's running sum from its history this often,
/// to drop the float drift.
const REFRESH: usize = 8192;

impl Dynamics {
    pub fn compressor(p: &DynamicsParams, rate: u32) -> Self {
        Self::new(p, rate, Kind::Compressor)
    }

    pub fn expander(p: &DynamicsParams, rate: u32) -> Self {
        Self::new(p, rate, Kind::Expander)
    }

    fn new(p: &DynamicsParams, rate: u32, kind: Kind) -> Self {
        let fs = rate as f32;
        let window = (((p.reactivity * 0.001) * fs) as usize).max(1);
        let tau = |ms: f32| 1. - ((1. - 0.5f32.sqrt()).ln() / (ms * 0.001 * fs)).exp();
        let th = db_to_gain(p.threshold);
        let kn = db_to_gain(p.knee.min(0.));
        let (start, end) = (th * kn, th / kn);
        let r = p.ratio;
        let (knee, tilt, floor) = match kind {
            Kind::Compressor => {
                let t0 = 1. / r - 1.;
                let tilt = [t0, (1. - 1. / r) * th.ln()];
                (
                    hermite_quadratic(start.ln(), 0., 0., end.ln(), t0),
                    tilt,
                    0.,
                )
            }
            Kind::Expander => {
                let t0 = r - 1.;
                let tilt = [t0, (1. - r) * th.ln()];
                // Below this envelope the gain is 0 (LSP's GAIN_AMP_M_140_DB).
                let floor = ((1e-7f32.ln() - tilt[1]) / t0.max(0.001)).exp().max(1e-7);
                (
                    hermite_quadratic(end.ln(), 0., 0., start.ln(), t0),
                    tilt,
                    floor,
                )
            }
        };
        Self {
            kind,
            window,
            history: vec![0.; window],
            pos: 0,
            sum: 0.,
            since_refresh: 0,
            env: 0.,
            tau_attack: tau(p.attack),
            tau_release: tau(p.release),
            release_level: p.release_threshold.map_or(0., db_to_gain) * th,
            start,
            end,
            knee,
            tilt,
            floor,
            wet: db_to_gain(p.wet) * db_to_gain(p.makeup),
            dry: p.dry.map_or(0., db_to_gain),
        }
    }

    fn sidechain(&mut self, x: f32) -> f32 {
        let s = x.abs();
        let old = std::mem::replace(&mut self.history[self.pos], s);
        self.pos = (self.pos + 1) % self.window;
        self.sum += s * s - old * old;
        self.since_refresh += 1;
        if self.since_refresh == REFRESH {
            self.since_refresh = 0;
            self.sum = self.history.iter().map(|v| v * v).sum();
        }
        if self.sum > 0. {
            (self.sum / self.window as f32).sqrt()
        } else {
            0.
        }
    }

    fn gain(&self, e: f32) -> f32 {
        let [a, b, c] = self.knee;
        let [t0, t1] = self.tilt;
        match self.kind {
            Kind::Compressor => {
                if e <= self.start {
                    1.
                } else {
                    let lx = e.ln();
                    if e >= self.end {
                        (t0 * lx + t1).exp()
                    } else {
                        ((a * lx + b) * lx + c).exp()
                    }
                }
            }
            Kind::Expander => {
                if e < self.floor {
                    0.
                } else if e < self.end {
                    let lx = e.ln();
                    if e <= self.start {
                        (t0 * lx + t1).exp()
                    } else {
                        ((a * lx + b) * lx + c).exp()
                    }
                } else {
                    1.
                }
            }
        }
    }
}

impl Stage for Dynamics {
    fn process(&mut self, buf: &mut [f32]) {
        for x in buf {
            let level = self.sidechain(*x);
            let d = level - self.env;
            let tau = if d < 0. && self.env > self.release_level {
                self.tau_release
            } else {
                self.tau_attack
            };
            self.env += tau * d;
            let g = self.gain(self.env);
            *x = *x * g * self.wet + *x * self.dry;
        }
    }
}
