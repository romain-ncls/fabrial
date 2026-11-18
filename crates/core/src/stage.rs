/// One step of the chain: mono audio in, mono audio out, at the chain's rate.
/// Not `Send`: tract's model state uses `Rc`, so a chain is built on the thread
/// that runs it.
pub trait Stage {
    /// Processes `buf` in place. Blocks can be of any length, so the stage
    /// keeps whatever state it needs between calls.
    fn process(&mut self, buf: &mut [f32]);

    /// The delay the stage adds, in samples.
    fn latency(&self) -> usize {
        0
    }
}

/// A fixed gain, in dB.
pub struct Gain {
    gain: f32,
}

impl Gain {
    pub fn new(db: f32) -> Self {
        Self {
            gain: crate::db_to_gain(db),
        }
    }
}

impl Stage for Gain {
    fn process(&mut self, buf: &mut [f32]) {
        for s in buf {
            *s *= self.gain;
        }
    }
}
