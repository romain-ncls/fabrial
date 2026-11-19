//! Running a chain live, between the system's audio callbacks.
//!
//! The system's audio thread must never wait, or every app's sound
//! stutters. So it only copies samples in and out of two lock-free ring
//! buffers, and a worker thread runs the chain between them. DeepFilterNet
//! can then take a millisecond, or allocate, without holding up the audio.
//!
//! ```text
//! audio thread: Input::push ──→ ring ──→ worker: chain ──→ ring ──→ Output::pull
//! ```
//!
//! The cost is a little delay: the output stays a margin behind, so the
//! worker has time to finish each block before it is due. The margin is at
//! least the size of the block the system asks for. When the worker is late
//! anyway, the output plays silence until the margin is back (a "gap").
//! When nothing reads the output for a while, what piled up is dropped, so
//! the delay doesn't grow. A smaller backlog that lasts a whole second is
//! dropped too.
//!
//! While the mic sends digital silence (muted, say) for over a second, the
//! chain has settled on silence too, so the worker stops running it and
//! passes the silence on. It runs again on the first sample of sound.
//!
//! None of this is tied to an audio system: PipeWire's streams call it on
//! Linux, and other systems' callbacks can later.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::sync::mpsc;
use std::thread::{self, JoinHandle, Thread};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use rtrb::{Consumer, Producer, RingBuffer};

use crate::{Chain, ChainFile};

/// How much each ring holds, in seconds.
const RING_SECONDS: usize = 2;

/// After this many seconds of digital silence in a row, the chain stops
/// running until sound comes back.
const IDLE_AFTER_SECONDS: usize = 1;

/// What the threads count, shared between them. Samples unless noted.
#[derive(Default)]
struct Counters {
    captured: AtomicU64,
    /// Input lost because the worker was too far behind.
    dropped: AtomicU64,
    processed: AtomicU64,
    /// Time the worker spent in the chain, in ns.
    busy_ns: AtomicU64,
    /// Digital silence passed on without running the chain.
    idle: AtomicU64,
    /// The longest single run of the chain since the last `Live::stats`.
    worst_ns: AtomicU64,
    played: AtomicU64,
    /// Times the output ran dry.
    gaps: AtomicU64,
    /// Output dropped to keep the delay down, or because nothing read it.
    skipped: AtomicU64,
    /// Output waiting, at the last read.
    waiting: AtomicU64,
    /// The size of the last read.
    block: AtomicU64,
}

/// A snapshot of the counters. Totals since the start, except `worst`.
#[derive(Debug, Clone, Default)]
pub struct Stats {
    pub captured: u64,
    pub dropped: u64,
    pub processed: u64,
    pub busy: Duration,
    /// The longest single run of the chain since the previous snapshot.
    pub worst: Duration,
    /// Digital silence passed on without running the chain.
    pub idle: u64,
    pub played: u64,
    pub gaps: u64,
    pub skipped: u64,
    /// Processed samples waiting for the output, at its last read.
    pub waiting: u64,
    /// How many samples the output's last read asked for.
    pub block: u64,
}

/// The control side: stats, and stopping the worker (on drop).
pub struct Live {
    pub rate: u32,
    /// The chain's own delay, in samples.
    pub chain_latency: usize,
    counters: Arc<Counters>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

/// The audio thread's side of the input. Never waits or allocates.
pub struct Input {
    tx: Producer<f32>,
    worker: Thread,
    counters: Arc<Counters>,
}

/// The audio thread's side of the output. Never waits or allocates.
pub struct Output {
    rx: Consumer<f32>,
    counters: Arc<Counters>,
    margin: usize,
    filling: bool,
    /// One second of samples: how long a backlog must last to be dropped.
    second: usize,
    /// The least that waited at a read, and the samples read, this second.
    least: usize,
    read: usize,
}

/// Starts the worker thread with `file`'s chain. `margin` is the least
/// delay the output keeps, in samples.
pub fn start(file: ChainFile, margin: usize) -> Result<(Live, Input, Output)> {
    let rate = file.rate;
    let capacity = rate as usize * RING_SECONDS;
    let (in_tx, mut in_rx) = RingBuffer::<f32>::new(capacity);
    let (mut out_tx, out_rx) = RingBuffer::<f32>::new(capacity);
    let counters = Arc::<Counters>::default();
    let stop = Arc::new(AtomicBool::new(false));

    // The chain is built on the worker: DeepFilterNet's model can't move
    // between threads.
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let worker = {
        let counters = counters.clone();
        let stop = stop.clone();
        thread::Builder::new()
            .name("fabrial-chain".into())
            .spawn(move || {
                let mut chain = match Chain::new(file) {
                    Ok(c) => {
                        let _ = ready_tx.send(Ok(c.latency()));
                        c
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                let mut buf = vec![0f32; capacity];
                let idle_after = rate as usize * IDLE_AFTER_SECONDS;
                // Digital silence in a row, in samples.
                let mut zeros = 0usize;
                while !stop.load(Relaxed) {
                    let n = in_rx.slots();
                    if n == 0 {
                        // Input::push wakes it; the timeout is only so it
                        // notices `stop`.
                        thread::park_timeout(Duration::from_millis(50));
                        continue;
                    }
                    let buf = &mut buf[..n];
                    in_rx.pop_entire_slice(buf).expect("slots were counted");
                    let silent = buf.iter().all(|&s| s == 0.);
                    if silent && zeros >= idle_after {
                        // `buf` already holds the silence the chain would send.
                        counters.idle.fetch_add(n as u64, Relaxed);
                    } else {
                        let t = Instant::now();
                        chain.process(buf);
                        let ns = t.elapsed().as_nanos() as u64;
                        counters.busy_ns.fetch_add(ns, Relaxed);
                        counters.worst_ns.fetch_max(ns, Relaxed);
                    }
                    zeros = if silent { zeros.saturating_add(n) } else { 0 };
                    let (_, left) = out_tx.push_partial_slice(buf);
                    counters.skipped.fetch_add(left.len() as u64, Relaxed);
                    // Counted once the output can read them.
                    counters.processed.fetch_add(n as u64, Relaxed);
                }
            })
            .context("starting the chain's thread")?
    };
    let chain_latency = match ready_rx.recv() {
        Ok(r) => r?,
        Err(_) => return Err(anyhow!("the chain's thread stopped while starting")),
    };

    let input = Input {
        tx: in_tx,
        worker: worker.thread().clone(),
        counters: counters.clone(),
    };
    let output = Output {
        rx: out_rx,
        counters: counters.clone(),
        margin,
        filling: true,
        second: rate as usize,
        least: usize::MAX,
        read: 0,
    };
    let live = Live {
        rate,
        chain_latency,
        counters,
        stop,
        worker: Some(worker),
    };
    Ok((live, input, output))
}

impl Live {
    pub fn stats(&self) -> Stats {
        let c = &self.counters;
        Stats {
            captured: c.captured.load(Relaxed),
            dropped: c.dropped.load(Relaxed),
            processed: c.processed.load(Relaxed),
            busy: Duration::from_nanos(c.busy_ns.load(Relaxed)),
            worst: Duration::from_nanos(c.worst_ns.swap(0, Relaxed)),
            idle: c.idle.load(Relaxed),
            played: c.played.load(Relaxed),
            gaps: c.gaps.load(Relaxed),
            skipped: c.skipped.load(Relaxed),
            waiting: c.waiting.load(Relaxed),
            block: c.block.load(Relaxed),
        }
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        self.stop.store(true, Relaxed);
        if let Some(w) = self.worker.take() {
            w.thread().unpark();
            let _ = w.join();
        }
    }
}

impl Input {
    /// Hands captured samples to the chain.
    pub fn push(&mut self, samples: impl ExactSizeIterator<Item = f32>) {
        let n = samples.len();
        let room = self.tx.slots().min(n);
        if let Ok(chunk) = self.tx.write_chunk_uninit(room) {
            chunk.fill_from_iter(samples);
        }
        self.counters.captured.fetch_add(n as u64, Relaxed);
        self.counters.dropped.fetch_add((n - room) as u64, Relaxed);
        self.worker.unpark();
    }
}

impl Output {
    /// Gives the next `n` processed samples to `write`, in order. Plays
    /// silence while the worker is behind.
    pub fn pull(&mut self, n: usize, mut write: impl FnMut(f32)) {
        let margin = self.margin.max(n);
        let mut waiting = self.rx.slots();
        self.counters.waiting.store(waiting as u64, Relaxed);
        self.counters.block.store(n as u64, Relaxed);
        self.counters.played.fetch_add(n as u64, Relaxed);

        if self.filling {
            if waiting < n + margin {
                (0..n).for_each(|_| write(0.));
                return;
            }
            self.filling = false;
        }
        // Normally `margin` to `margin + 2n` wait here. More means nothing
        // read the output for a while: drop the oldest at once.
        if waiting > margin + 3 * n {
            let skip = waiting - (margin + n);
            if let Ok(chunk) = self.rx.read_chunk(skip) {
                chunk.commit_all();
            }
            self.counters.skipped.fetch_add(skip as u64, Relaxed);
            waiting -= skip;
        }
        // A smaller backlog that lasts (after the graph paused, say) is
        // dropped too, once a second at most, so the delay comes back down.
        self.least = self.least.min(waiting);
        self.read += n;
        if self.read >= self.second {
            if self.least > margin + 2 * n {
                let skip = self.least - (margin + n);
                if let Ok(chunk) = self.rx.read_chunk(skip) {
                    chunk.commit_all();
                }
                self.counters.skipped.fetch_add(skip as u64, Relaxed);
                waiting -= skip;
            }
            self.least = usize::MAX;
            self.read = 0;
        }
        let take = waiting.min(n);
        if let Ok(chunk) = self.rx.read_chunk(take) {
            chunk.into_iter().for_each(&mut write);
        }
        if take < n {
            (take..n).for_each(|_| write(0.));
            self.counters.gaps.fetch_add(1, Relaxed);
            self.filling = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eq_chain() -> ChainFile {
        toml::from_str(
            r#"
            name = "test"
            rate = 48000
            [[stage]]
            type = "equalizer"
            bands = [{ type = "bell", mode = "bwc-bt", frequency = 1000, gain = 6, q = 1, slope = 2 }]
            [[stage]]
            type = "limiter"
            "#,
        )
        .unwrap()
    }

    /// Plays the audio thread: pushes blocks, lets the worker run, pulls.
    /// The output must be the offline result, delayed by a fixed amount.
    #[test]
    fn output_is_the_chain_delayed() {
        let x: Vec<f32> = (0..48_000)
            .map(|i| 1.5 * ((i as f32 * 0.031).sin() + 0.3 * (i as f32 * 0.29).sin()))
            .collect();
        let mut offline = x.clone();
        let mut chain = Chain::new(eq_chain()).unwrap();
        chain.process(&mut offline);

        let (live, mut input, mut output) = start(eq_chain(), 480).unwrap();
        let block = 256;
        let mut y = Vec::new();
        for b in x.chunks(block) {
            input.push(b.iter().copied());
            // The worker needs a moment, like it gets between callbacks.
            let t = Instant::now();
            while live.stats().processed < live.stats().captured
                && t.elapsed() < Duration::from_secs(1)
            {
                thread::yield_now();
            }
            output.pull(b.len(), |s| y.push(s));
        }
        let stats = live.stats();
        assert_eq!(stats.gaps, 0);
        assert_eq!(stats.dropped, 0);

        // Find where the output starts, then compare sample by sample.
        let delay = y.iter().position(|&s| s != 0.).unwrap();
        let first = offline.iter().position(|&s| s != 0.).unwrap();
        let shift = delay - first;
        for i in 0..y.len() - shift {
            assert_eq!(y[i + shift], offline[i], "sample {i}");
        }
        // The margin, rounded up to whole blocks.
        assert!(shift >= 480 && shift < 480 + block, "shift {shift}");
    }

    /// Sound, two seconds of digital silence, sound: the chain idles for
    /// part of the silence, and the output still matches it.
    #[test]
    fn digital_silence_idles_the_chain() {
        let sound = |i: usize| 0.3 * (i as f32 * 0.031).sin();
        let x: Vec<f32> = (0..48_000)
            .map(sound)
            .chain(std::iter::repeat_n(0., 96_000))
            .chain((0..48_000).map(sound))
            .collect();
        let mut offline = x.clone();
        Chain::new(eq_chain()).unwrap().process(&mut offline);

        let (live, mut input, mut output) = start(eq_chain(), 0).unwrap();
        let block = 480;
        let mut y = Vec::new();
        for b in x.chunks(block) {
            input.push(b.iter().copied());
            while live.stats().processed < live.stats().captured {
                thread::yield_now();
            }
            output.pull(b.len(), |s| y.push(s));
        }
        let stats = live.stats();
        assert_eq!(stats.gaps, 0);
        // One second of the silence runs the chain, the other is passed on.
        assert_eq!(stats.idle, 48_000);

        let shift = y.iter().position(|&s| s != 0.).unwrap()
            - offline.iter().position(|&s| s != 0.).unwrap();
        for i in 0..y.len() - shift {
            let d = (y[i + shift] - offline[i]).abs();
            assert!(d < 1e-6, "sample {i}: {} vs {}", y[i + shift], offline[i]);
        }
    }

    #[test]
    fn a_late_worker_leaves_a_gap_then_recovers() {
        let (live, mut input, mut output) = start(eq_chain(), 0).unwrap();
        let mut sink = |_| {};
        // Two blocks in: enough to start (a block plus the margin, which is
        // at least a block), and to play twice. The third read finds nothing.
        for _ in 0..2 {
            input.push([0.1f32; 128].into_iter());
        }
        while live.stats().processed < 256 {
            thread::yield_now();
        }
        output.pull(128, &mut sink);
        output.pull(128, &mut sink);
        assert_eq!(live.stats().gaps, 0);
        output.pull(128, &mut sink);
        assert_eq!(live.stats().gaps, 1);
        // The worker catches up: the output fills, then plays again.
        for _ in 0..4 {
            input.push([0.1f32; 128].into_iter());
        }
        while live.stats().processed < 768 {
            thread::yield_now();
        }
        output.pull(128, &mut sink);
        output.pull(128, &mut sink);
        assert_eq!(live.stats().gaps, 1);
    }

    #[test]
    fn a_lasting_backlog_is_dropped() {
        let (live, mut input, mut output) = start(eq_chain(), 0).unwrap();
        let settle = |live: &Live| {
            while live.stats().processed < live.stats().captured {
                thread::yield_now();
            }
        };
        // Three blocks pile up, as when the graph pauses the output: two
        // more than the margin (one block) needs, not enough to drop at once.
        for _ in 0..3 {
            input.push([0.1f32; 128].into_iter());
        }
        // Then a bit over a second of steady blocks.
        for _ in 0..400 {
            input.push([0.1f32; 128].into_iter());
            settle(&live);
            output.pull(128, |_| {});
        }
        let s = live.stats();
        assert_eq!(s.gaps, 0);
        assert_eq!(s.skipped, 256);
        assert_eq!(s.waiting, 256);
    }

    #[test]
    fn what_piles_up_is_dropped() {
        let (live, mut input, mut output) = start(eq_chain(), 0).unwrap();
        for _ in 0..100 {
            input.push([0.1f32; 128].into_iter());
        }
        while live.stats().processed < 12_800 {
            thread::yield_now();
        }
        let mut sink = |_| {};
        output.pull(128, &mut sink);
        let s = live.stats();
        assert_eq!(s.skipped, 12_800 - 256);
        assert_eq!(s.gaps, 0);
    }
}
