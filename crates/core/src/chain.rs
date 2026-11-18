//! The chain file: a TOML description of the stages and their settings, so
//! each version can be compared, kept and reverted.
//!
//! ```toml
//! name = "example"
//! rate = 48000          # the rate every stage runs at
//!
//! [[stage]]
//! type = "deepfilternet3"
//! post-filter-beta = 0.02
//!
//! [[stage]]
//! type = "gain"
//! db = 6
//! ```

use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::Stage;
use crate::dfn3::{Dfn3, Dfn3Params};
use crate::dynamics::{Dynamics, DynamicsParams};
use crate::eq::{Eq, EqParams};
use crate::limiter::{Limiter, LimiterParams};
use crate::stage::Gain;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChainFile {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// The rate every stage runs at. The input is resampled to it.
    pub rate: u32,
    #[serde(rename = "stage", default)]
    pub stages: Vec<StageSpec>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum StageSpec {
    Deepfilternet3(Dfn3Params),
    Expander(DynamicsParams),
    Compressor(DynamicsParams),
    Equalizer(EqParams),
    Gain(GainParams),
    Limiter(LimiterParams),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GainParams {
    pub db: f32,
}

impl StageSpec {
    /// A short name, for taps and reports.
    pub fn kind(&self) -> &'static str {
        match self {
            StageSpec::Deepfilternet3(_) => "deepfilternet3",
            StageSpec::Expander(_) => "expander",
            StageSpec::Compressor(_) => "compressor",
            StageSpec::Equalizer(_) => "equalizer",
            StageSpec::Gain(_) => "gain",
            StageSpec::Limiter(_) => "limiter",
        }
    }

    fn build(&self, rate: u32) -> Result<Box<dyn Stage>> {
        Ok(match self {
            StageSpec::Deepfilternet3(p) => Box::new(Dfn3::new(p, rate)?),
            StageSpec::Expander(p) => Box::new(Dynamics::expander(p, rate)),
            StageSpec::Compressor(p) => Box::new(Dynamics::compressor(p, rate)),
            StageSpec::Equalizer(p) => Box::new(Eq::new(p, rate)?),
            StageSpec::Gain(p) => Box::new(Gain::new(p.db)),
            StageSpec::Limiter(p) => Box::new(Limiter::new(p, rate)),
        })
    }
}

impl ChainFile {
    pub fn load(path: &Path) -> Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }
}

/// A chain ready to process audio.
pub struct Chain {
    pub file: ChainFile,
    stages: Vec<Box<dyn Stage>>,
}

impl Chain {
    pub fn new(file: ChainFile) -> Result<Self> {
        let stages = file
            .stages
            .iter()
            .enumerate()
            .map(|(i, s)| {
                s.build(file.rate)
                    .with_context(|| format!("stage {} ({})", i + 1, s.kind()))
            })
            .collect::<Result<_>>()?;
        Ok(Self { file, stages })
    }

    pub fn load(path: &Path) -> Result<Self> {
        Self::new(ChainFile::load(path)?)
    }

    pub fn rate(&self) -> u32 {
        self.file.rate
    }

    /// Names of the stages' outputs, as `<n>-<type>` (1-based).
    pub fn tap_names(&self) -> Vec<String> {
        self.file
            .stages
            .iter()
            .enumerate()
            .map(|(i, s)| format!("{}-{}", i + 1, s.kind()))
            .collect()
    }

    /// The delay each stage adds, in samples.
    pub fn latencies(&self) -> Vec<usize> {
        self.stages.iter().map(|s| s.latency()).collect()
    }

    /// The whole chain's delay, in samples.
    pub fn latency(&self) -> usize {
        self.latencies().iter().sum()
    }

    pub fn process(&mut self, buf: &mut [f32]) {
        for s in &mut self.stages {
            s.process(buf);
        }
    }

    /// Like `process`, and appends each stage's output to its tap.
    pub fn process_tapped(&mut self, buf: &mut [f32], taps: &mut [Vec<f32>]) {
        for (s, tap) in self.stages.iter_mut().zip(taps) {
            s.process(buf);
            tap.extend_from_slice(buf);
        }
    }
}
