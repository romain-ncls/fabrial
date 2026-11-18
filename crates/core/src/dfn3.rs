//! DeepFilterNet3, through DeepFilterNet's own Rust code (libDF and tract),
//! as the LADSPA plugin EasyEffects runs: version 0.5.6 with the model its
//! build embeds by default, the low-latency DeepFilterNet3 (no lookahead,
//! 10 ms of model delay). The standard model looks 2 frames ahead and
//! treats speech differently, so a copy of EasyEffects needs this one.

use std::collections::VecDeque;

use anyhow::{Result, ensure};
use df::tract::{DfParams, DfTract, RuntimeParams};
use ndarray::Array2;
use serde::Deserialize;

use crate::Stage;

/// The LADSPA plugin's controls, under the same names and scales.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Dfn3Params {
    /// The most the noise is lowered, in dB. 100 means no limit.
    #[serde(default = "default_attenuation_limit")]
    pub attenuation_limit: f32,
    /// Strength of the post-filter, which lowers what is left in noisy
    /// frames further. 0 turns it off; the plugin allows up to 0.05.
    #[serde(default)]
    pub post_filter_beta: f32,
    /// Below this local SNR (dB), a frame counts as noise only and is muted.
    #[serde(default = "default_min_threshold")]
    pub min_processing_threshold: f32,
    /// Above this local SNR (dB), the ERB gains are skipped: the frame is
    /// clean enough.
    #[serde(default = "default_max_erb_threshold")]
    pub max_erb_processing_threshold: f32,
    /// Above this local SNR (dB), the deep filter is skipped.
    #[serde(default = "default_max_df_threshold")]
    pub max_df_processing_threshold: f32,
}

fn default_attenuation_limit() -> f32 {
    100.
}
fn default_min_threshold() -> f32 {
    -10.
}
fn default_max_erb_threshold() -> f32 {
    30.
}
fn default_max_df_threshold() -> f32 {
    20.
}

/// DeepFilterNet3 as a stage. The model works on frames of one hop (10 ms
/// at 48 kHz), so the stage queues samples: one hop of buffering, as the
/// LADSPA plugin does, on top of the model's own delay.
pub struct Dfn3 {
    df: DfTract,
    hop: usize,
    model_delay: usize,
    pending: Vec<f32>,
    out: VecDeque<f32>,
    in_frame: Array2<f32>,
    out_frame: Array2<f32>,
}

impl Dfn3 {
    pub const RATE: u32 = 48_000;

    pub fn new(p: &Dfn3Params, rate: u32) -> Result<Self> {
        ensure!(
            rate == Self::RATE,
            "DeepFilterNet3 runs at {} Hz, the chain at {rate} Hz",
            Self::RATE
        );
        let rp = RuntimeParams::default_with_ch(1).with_thresholds(
            p.min_processing_threshold,
            p.max_erb_processing_threshold,
            p.max_df_processing_threshold,
        );
        let mut df = DfTract::new(DfParams::default(), &rp)?;
        ensure!(df.sr == Self::RATE as usize, "the model is not 48 kHz");
        // Same calls as the plugin's worker when its controls change.
        df.set_atten_lim(p.attenuation_limit);
        df.set_pf_beta(p.post_filter_beta);
        let hop = df.hop_size;
        // The output frame is `lookahead` frames old, and the STFT overlap
        // adds one window minus one hop.
        let model_delay = df.fft_size - hop + df.lookahead * hop;
        Ok(Self {
            model_delay,
            pending: Vec::with_capacity(hop),
            out: VecDeque::from(vec![0.; hop]),
            in_frame: Array2::zeros((1, hop)),
            out_frame: Array2::zeros((1, hop)),
            hop,
            df,
        })
    }
}

impl Stage for Dfn3 {
    fn process(&mut self, buf: &mut [f32]) {
        for s in buf.iter_mut() {
            self.pending.push(*s);
            if self.pending.len() == self.hop {
                self.in_frame
                    .as_slice_mut()
                    .unwrap()
                    .copy_from_slice(&self.pending);
                self.pending.clear();
                // libDF only fails on tensor shape mismatches, which the
                // frame sizes above rule out.
                self.df
                    .process(self.in_frame.view(), self.out_frame.view_mut())
                    .expect("DeepFilterNet3 failed on a frame");
                self.out.extend(self.out_frame.iter());
            }
            *s = self.out.pop_front().expect("the queue holds one hop ahead");
        }
    }

    fn latency(&self) -> usize {
        self.hop + self.model_delay
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// With a 0.5 dB attenuation limit the output is mostly the input
    /// through the model's STFT, so the delay can be measured.
    #[test]
    fn latency_matches_the_measured_delay() {
        let p = Dfn3Params {
            attenuation_limit: 0.5,
            ..toml::from_str("").unwrap()
        };
        let mut st = Dfn3::new(&p, 48_000).unwrap();
        // A 200 Hz → 4 kHz chirp over one second, in uneven blocks.
        let n = 48_000;
        let input: Vec<f32> = (0..n)
            .map(|i| {
                let t = i as f32 / 48_000.;
                0.1 * (2. * std::f32::consts::PI * (200. * t + 1900. * t * t)).sin()
            })
            .collect();
        let mut out = input.clone();
        for block in out.chunks_mut(256) {
            st.process(block);
        }
        let corr = |lag: usize| -> f32 { (lag..n).map(|i| out[i] * input[i - lag]).sum() };
        let best = (0..4_800)
            .max_by(|&a, &b| corr(a).total_cmp(&corr(b)))
            .unwrap();
        assert_eq!(best, st.latency(), "measured delay vs declared latency");
    }
}
