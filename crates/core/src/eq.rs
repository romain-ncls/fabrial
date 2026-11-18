//! Parametric EQ, designed as LSP Plugins' para_equalizer does in IIR mode
//! (lsp-dsp-units 1.0.39 Filter.cpp), so the copy of the EasyEffects chain
//! matches it.
//!
//! Each band is an analog prototype in normalised s (s/ω0), a product of
//! sections (t0 + t1·s + t2·s²)/(b0 + b1·s + b2·s²). Each section then
//! becomes a biquad, by the bilinear transform with prewarping (BT) or by
//! matched-Z (MT). Modes: RLC (classic RLC sections), BWC (Butterworth with
//! a Q-warped damping), LRX (Linkwitz-Riley: two BWC chains at half the
//! gain each). Only the combinations below are ported; others are refused.

use std::f32::consts::PI;

use anyhow::{Result, bail};
use serde::Deserialize;

use crate::{Stage, db_to_gain};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EqParams {
    pub bands: Vec<Band>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Band {
    #[serde(rename = "type")]
    pub kind: BandType,
    pub mode: Mode,
    /// Hz.
    pub frequency: f32,
    /// dB (ignored by passes).
    #[serde(default)]
    pub gain: f32,
    #[serde(default = "default_q")]
    pub q: f32,
    /// 1 for EasyEffects' "x1", 2 for "x2"…
    #[serde(default = "default_slope")]
    pub slope: u32,
}

fn default_q() -> f32 {
    0.7
}
fn default_slope() -> u32 {
    1
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BandType {
    HiPass,
    LoShelf,
    HiShelf,
    Bell,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    RlcBt,
    RlcMt,
    BwcBt,
    BwcMt,
    LrxBt,
    LrxMt,
}

/// One analog section: numerator t, denominator b, lowest power first.
type Section = ([f32; 3], [f32; 3]);

/// A digital biquad, a0 = 1: y = b0·x + b1·x1 + b2·x2 − a1·y1 − a2·y2.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Biquad {
    pub b: [f32; 3],
    pub a: [f32; 2],
}

fn rlc_hipass(q: f32, slope: u32, out: &mut Vec<Section>) {
    let k = 2. / (1. + q);
    let mut i = slope;
    while i >= 2 {
        out.push(([0., 0., 1.], [1., k, 1.]));
        i -= 2;
    }
    if i == 1 {
        out.push(([0., 1., 0.], [1., 1., 0.]));
    }
}

fn rlc_shelf(lo: bool, q: f32, g: f32, slope: u32, out: &mut Vec<Section>) {
    let gain = g.sqrt();
    let fg = (gain.ln() / (slope * 2) as f32).exp();
    let k = 2. / (1. + q);
    for j in 0..slope {
        let m = if j == 0 { gain } else { 1. };
        out.push(if lo {
            ([fg * m, k * m, (1. / fg) * m], [1. / fg, k, fg])
        } else {
            ([(1. / fg) * m, k * m, fg * m], [fg, k, 1. / fg])
        });
    }
}

/// θj, cos θj and kf for the Butterworth-style designs.
fn bwc_terms(j: u32, slope: u32, k: f32) -> (f32, f32) {
    let th = ((2 * j + 1) as f32 * PI / 2.) / (2 * slope) as f32;
    let ts = th.sin();
    let tc = (1. - ts * ts).sqrt();
    (tc, ts * ts + k * k * tc * tc)
}

fn bwc_bell(q: f32, g: f32, slope: u32, out: &mut Vec<Section>) {
    let fg = (g.ln() / (2 * slope) as f32).exp();
    let k = 1. / (1. + q);
    for j in 0..slope {
        let (tc, kf) = bwc_terms(j, slope, k);
        if g >= 1. {
            out.push((
                [1., 2. * k * tc * fg / kf, fg * fg / kf],
                [1., 2. * k * tc / kf, 1. / kf],
            ));
            out.push((
                [1., 2. * k * tc / fg, kf / (fg * fg)],
                [1., 2. * k * tc, kf],
            ));
        } else {
            out.push((
                [1., 2. * k * tc / kf, 1. / kf],
                [1., 2. * k * tc / (fg * kf), 1. / (fg * fg * kf)],
            ));
            out.push(([1., 2. * k * tc, kf], [1., 2. * k * tc * fg, kf * fg * fg]));
        }
    }
}

fn bwc_shelf(hi: bool, q: f32, g: f32, slope: u32, out: &mut Vec<Section>) {
    let gain = g.sqrt();
    let fg = (gain.ln() / (2 * slope) as f32).exp();
    let k = 1. / (1. + q * (1. - (2. - gain - 1. / gain).exp()));
    for j in 0..slope {
        let (tc, kf) = bwc_terms(j, slope, k);
        let m = if j == 0 { gain } else { 1. };
        out.push(if hi {
            (
                [m * kf / fg, m * 2. * k * tc, m * fg],
                [fg, 2. * k * tc, kf / fg],
            )
        } else {
            (
                [m * fg, m * 2. * k * tc, m * kf / fg],
                [kf / fg, 2. * k * tc, fg],
            )
        });
    }
}

fn bilinear(s: &Section, f: f32, fs: f32) -> Biquad {
    let (t, b) = s;
    let kf = 1. / (f * PI / fs).tan();
    let kf2 = kf * kf;
    let (t0, t1, t2) = (t[0], t[1] * kf, t[2] * kf2);
    let (b0, b1, b2) = (b[0], b[1] * kf, b[2] * kf2);
    if b[2] != 0. {
        let n = 1. / (b0 + b1 + b2);
        Biquad {
            b: [(t0 + t1 + t2) * n, 2. * (t0 - t2) * n, (t0 - t1 + t2) * n],
            a: [2. * (b0 - b2) * n, (b0 - b1 + b2) * n],
        }
    } else {
        let n = 1. / (b0 + b1);
        Biquad {
            b: [(t0 + t1) * n, (t0 - t1) * n, 0.],
            a: [(b0 - b1) * n, 0.],
        }
    }
}

/// Matched-Z for one polynomial: its digital form, then its magnitude at
/// f/10 (digital, f64) and at 0.1 (analog, f64), for the normalisation.
fn matched_poly(p: &[f32; 3], f: f32, fs: f32) -> ([f32; 3], f64, f64) {
    let td = (PI * 2.) / fs;
    let d = if p[2] == 0. {
        if p[1] == 0. {
            [p[0], 0., 0.]
        } else {
            let k = p[1] / f;
            let r = -p[0] / k;
            [k, -k * (r * td).exp(), 0.]
        }
    } else {
        let k = p[2];
        let a = 1. / (f * f);
        let b = p[1] / (f * p[2]);
        let c = p[0] / p[2];
        let disc = b * b - 4. * a * c;
        if disc >= 0. {
            let sq = disc.sqrt();
            let (r0, r1) = ((-b - sq) / (2. * a), (-b + sq) / (2. * a));
            [
                k,
                -k * ((r0 * td).exp() + (r1 * td).exp()),
                k * ((r0 + r1) * td).exp(),
            ]
        } else {
            let sq = (-disc).sqrt();
            let (r, im) = (-b / (2. * a), sq / (2. * a));
            [
                k,
                -2. * k * (r * td).exp() * (im * td).cos(),
                k * (2. * r * td).exp(),
            ]
        }
    };
    let w = (PI * 0.2 * f / fs) as f64;
    let (p0, p1, p2) = (d[0] as f64, d[1] as f64, d[2] as f64);
    let re = p0 * (2. * w).cos() + p1 * w.cos() + p2;
    let im = p0 * (2. * w).sin() + p1 * w.sin();
    let digital = (re * re + im * im).sqrt();
    let w = 0.1f64;
    let re = p[0] as f64 - p[2] as f64 * w * w;
    let im = p[1] as f64 * w;
    (d, digital, (re * re + im * im).sqrt())
}

fn matched(s: &Section, f: f32, fs: f32) -> Biquad {
    let (tt, at, it) = matched_poly(&s.0, f, fs);
    let (bb, ab, ib) = matched_poly(&s.1, f, fs);
    let an = (ab * it) / (at * ib);
    let n = 1. / bb[0] as f64;
    let scale = |v: f32| (v as f64 * n * an) as f32;
    Biquad {
        b: [scale(tt[0]), scale(tt[1]), scale(tt[2])],
        a: [(bb[1] as f64 * n) as f32, (bb[2] as f64 * n) as f32],
    }
}

impl Band {
    /// The band's biquads at `rate`.
    pub fn design(&self, rate: u32) -> Result<Vec<Biquad>> {
        let fs = rate as f32;
        let f = self.frequency.min(0.49 * fs);
        let g = db_to_gain(self.gain);
        let n = self.slope.max(1);
        let mut s = Vec::new();
        use BandType::*;
        use Mode::*;
        match (self.kind, self.mode) {
            (HiPass, RlcBt | RlcMt) => rlc_hipass(self.q, n * 2, &mut s),
            (LoShelf | HiShelf, RlcBt | RlcMt) => {
                rlc_shelf(self.kind == LoShelf, self.q, g, n, &mut s)
            }
            (Bell, BwcBt | BwcMt) => bwc_bell(self.q, g, n, &mut s),
            (LoShelf | HiShelf, BwcBt | BwcMt) => {
                bwc_shelf(self.kind == HiShelf, self.q, g, n, &mut s)
            }
            (LoShelf | HiShelf, LrxBt | LrxMt) => {
                for _ in 0..2 {
                    bwc_shelf(self.kind == HiShelf, self.q, g.sqrt(), n * 2, &mut s);
                }
            }
            (k, m) => bail!("EQ band {k:?} in mode {m:?} isn't ported from LSP yet"),
        }
        let bt = matches!(self.mode, RlcBt | BwcBt | LrxBt);
        Ok(s.iter()
            .map(|s| {
                if bt {
                    bilinear(s, f, fs)
                } else {
                    matched(s, f, fs)
                }
            })
            .collect())
    }
}

/// All bands in series, each biquad in transposed direct form II.
pub struct Eq {
    biquads: Vec<Biquad>,
    state: Vec<[f32; 2]>,
}

impl Eq {
    pub fn new(p: &EqParams, rate: u32) -> Result<Self> {
        let mut biquads = Vec::new();
        for b in &p.bands {
            biquads.extend(b.design(rate)?);
        }
        Ok(Self {
            state: vec![[0.; 2]; biquads.len()],
            biquads,
        })
    }
}

impl Stage for Eq {
    fn process(&mut self, buf: &mut [f32]) {
        for (q, d) in self.biquads.iter().zip(&mut self.state) {
            for x in buf.iter_mut() {
                let y = q.b[0] * *x + d[0];
                d[0] = d[1] + q.b[1] * *x - q.a[0] * y;
                d[1] = q.b[2] * *x - q.a[1] * y;
                *x = y;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn band(toml_band: &str) -> Band {
        toml::from_str(toml_band).unwrap()
    }

    fn close(got: &Biquad, b: [f64; 3], a: [f64; 2], tol: f64) -> bool {
        got.b
            .iter()
            .zip(b)
            .chain(got.a.iter().zip(a))
            .all(|(&g, w)| (g as f64 - w).abs() < tol)
    }

    /// awpro-v3's five bands against the coefficients of a double-precision
    /// port of LSP (BT bands) and against the plugin itself (f32 MT band 1).
    #[test]
    fn awpro_v3_bands_match_lsp() {
        let hp = band("type='hi-pass'\nmode='rlc-bt'\nfrequency=50\nq=0.7")
            .design(48_000)
            .unwrap();
        assert_eq!(hp.len(), 1);
        assert!(close(
            &hp[0],
            [0.996154133767, -1.99230826753, 0.996154133767],
            [-1.99228693134, 0.992329603725],
            1e-6
        ));

        let ls = band("type='lo-shelf'\nmode='rlc-mt'\nfrequency=90\ngain=3\nq=0.7")
            .design(48_000)
            .unwrap();
        assert!(
            close(
                &ls[0],
                [1.0014099, -1.9876382, 0.98639232],
                [-1.9872509, 0.98736703],
                2e-7
            ),
            "{:?}",
            ls[0]
        );

        let bell = band("type='bell'\nmode='bwc-bt'\nfrequency=3500\ngain=3\nq=0.7\nslope=2")
            .design(48_000)
            .unwrap();
        assert_eq!(bell.len(), 4);
        assert!(close(
            &bell[0],
            [1.16545272531, -1.82313347892, 0.732889806723],
            [-1.52801139112, 0.603220444239],
            1e-5
        ));
        assert!(close(
            &bell[3],
            [0.856992147528, -1.32844190962, 0.675919657124],
            [-1.59812768394, 0.802597578965],
            1e-5
        ));

        let hs = band("type='hi-shelf'\nmode='lrx-mt'\nfrequency=9000\ngain=2\nq=0.7")
            .design(48_000)
            .unwrap();
        assert_eq!(hs.len(), 4);
        assert!(close(
            &hs[0],
            [1.06186550331, -0.657447035217, 0.124836445453],
            [-0.593209346093, 0.109480742676],
            1e-5
        ));
        assert_eq!(hs[0], hs[2]);
    }
}
