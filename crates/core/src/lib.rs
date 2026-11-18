//! fabrial's processing chain: the stages and the chain file, with no audio
//! I/O. The live mic and the offline render run the same code, block by
//! block, so a render of a recorded take is what the live chain would have
//! sent.

pub mod chain;
pub mod dfn3;
pub mod dynamics;
pub mod eq;
pub mod limiter;
pub mod resample;
pub mod stage;

pub use chain::{Chain, ChainFile};
pub use stage::Stage;

/// dB to linear gain.
pub fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.)
}

/// Linear gain to dB.
pub fn gain_to_db(gain: f32) -> f32 {
    20. * gain.log10()
}
