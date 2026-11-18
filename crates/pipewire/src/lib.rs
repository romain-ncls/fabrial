//! The live path on Linux: two PipeWire streams around the chain.
//!
//! ```text
//! headset mic → [fabrial_input] → chain → [fabrial_source] → apps
//!                   stream                virtual mic (a stream)
//! ```
//!
//! - **`fabrial_input`:** a stream that records the mic. `node.dont-move`
//!   keeps EasyEffects (or any other tool) from moving it elsewhere.
//! - **`fabrial_source`:** a stream PipeWire presents as a mic, named
//!   "fabrial". Apps record from it. Its class must be `Audio/Source`:
//!   WirePlumber 0.5.19 never set up the ports of a stream classed
//!   `Audio/Source/Virtual` (`pw-loopback`'s fail the same way), and
//!   meanwhile it left the input unlinked too.
//!
//! Both streams share a node group, so the same clock (the mic's) drives
//! them, and a link group, so WirePlumber never feeds the virtual mic back
//! into the input. The chain runs between them on `fabrial_core::live`'s
//! worker thread.
//!
//! This uses pipewire-rs, a wrapper around PipeWire's C library. The plan
//! is to move to pipewire-native, PipeWire's own Rust client, once it can
//! carry audio (see docs/roadmap.md). Everything PipeWire-specific stays in
//! this crate so that move only touches it.

use std::cell::{Cell, RefCell};
use std::io::Cursor;
use std::rc::Rc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use fabrial_core::ChainFile;
use fabrial_core::live::{self, Input, Live, Output, Stats};
use pipewire as pw;
use pw::loop_::Signal;
use pw::properties::PropertiesBox;
use pw::spa;
use pw::stream::{StreamBox, StreamFlags, StreamState};
use spa::param::audio::{AudioFormat, AudioInfoRaw};
use spa::pod::serialize::PodSerializer;
use spa::pod::{Object, Pod, Value};

mod mover;

/// The virtual mic's node name, what `pactl` and `pw-record --target` use.
pub const NODE_NAME: &str = "fabrial_source";
/// The name apps show.
pub const DESCRIPTION: &str = "fabrial";

pub struct Options {
    pub chain: ChainFile,
    /// The mic to read, by node name. None: the default mic.
    pub mic: Option<String>,
    /// The least delay the worker gets to process each block, in ms.
    pub margin_ms: f32,
    /// How often to print stats. None: only when something goes wrong.
    pub stats_every: Option<Duration>,
    /// Move apps that record from the mic onto fabrial's mic.
    pub move_apps: bool,
}

/// Runs until Ctrl-C or SIGTERM.
pub fn run(opts: Options) -> Result<()> {
    block_stop_signals();
    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None).context("starting PipeWire's loop")?;
    let context = pw::context::ContextRc::new(&mainloop, None)?;
    let core = context.connect_rc(None).context("connecting to PipeWire")?;
    if already_running(&mainloop, &core) {
        bail!("another fabrial is running: its mic, {NODE_NAME}, exists");
    }
    // Losing PipeWire, or a stream failing, ends the run with an error, so a
    // service manager restarts it.
    let failure = Rc::new(RefCell::new(None::<String>));
    let fail = {
        let (mainloop, failure) = (mainloop.clone(), failure.clone());
        Rc::new(move |why: String| {
            failure.borrow_mut().get_or_insert(why);
            mainloop.quit();
        })
    };
    let lost = Rc::new(Cell::new(false));
    let _core_listener = core
        .add_listener_local()
        .error({
            let (fail, lost) = (fail.clone(), lost.clone());
            move |id, _, _, message| {
                if id == pw::core::PW_ID_CORE {
                    lost.set(true);
                    fail(format!("lost PipeWire: {message}"));
                }
            }
        })
        .register();

    let rate = opts.chain.rate;
    let chain_name = opts.chain.name.clone();
    let margin = (opts.margin_ms / 1000. * rate as f32).round() as usize;
    let (live, input, output) = live::start(opts.chain, margin)?;
    let group = format!("fabrial-{}", std::process::id());
    let own: mover::OwnIds = Rc::new(Cell::new((u32::MAX, u32::MAX)));
    let mover = match opts.move_apps {
        true => Some(Rc::new(mover::Mover::new(&core, own.clone())?)),
        false => None,
    };

    let mut props = stream_props(&group);
    props.insert(*pw::keys::MEDIA_CATEGORY, "Capture");
    props.insert(*pw::keys::MEDIA_ROLE, "Communication");
    props.insert(*pw::keys::NODE_NAME, "fabrial_input");
    props.insert(*pw::keys::NODE_DESCRIPTION, "fabrial (reads the mic)");
    if let Some(mic) = &opts.mic {
        props.insert(*pw::keys::TARGET_OBJECT, mic.as_str());
    }
    let capture = StreamBox::new(&core, "fabrial-input", props)?;
    let _capture_listener = capture
        .add_local_listener_with_user_data(input)
        .state_changed({
            let (own, mover, fail) = (own.clone(), mover.clone(), fail.clone());
            move |stream, _, old, new| {
                report_state("mic input", &old, &new, &*fail);
                own.set((stream.node_id(), own.get().1));
                if let Some(m) = &mover {
                    m.poke();
                }
            }
        })
        .process(|stream, input: &mut Input| {
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            let Some(data) = buffer.datas_mut().first_mut() else {
                return;
            };
            let (offset, size) = (data.chunk().offset() as usize, data.chunk().size() as usize);
            if let Some(bytes) = data.data() {
                let end = (offset + size).min(bytes.len());
                let bytes = &bytes[offset.min(end)..end];
                input.push(
                    bytes
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|b| f32::from_le_bytes(*b)),
                );
            }
        })
        .register()?;

    let mut props = stream_props(&group);
    props.insert(*pw::keys::MEDIA_CLASS, "Audio/Source");
    props.insert(*pw::keys::NODE_NAME, NODE_NAME);
    props.insert(*pw::keys::NODE_DESCRIPTION, DESCRIPTION);
    props.insert(*pw::keys::NODE_VIRTUAL, "true");
    // Never picked as the default mic on its own.
    props.insert(*pw::keys::PRIORITY_SESSION, "0");
    let source = StreamBox::new(&core, "fabrial", props)?;
    let _source_listener = source
        .add_local_listener_with_user_data(output)
        .state_changed({
            let (own, mover, fail) = (own.clone(), mover.clone(), fail.clone());
            move |stream, _, old, new| {
                report_state("virtual mic", &old, &new, &*fail);
                own.set((own.get().0, stream.node_id()));
                if let Some(m) = &mover {
                    m.poke();
                }
            }
        })
        .process(|stream, output: &mut Output| {
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            let requested = buffer.requested() as usize;
            let Some(data) = buffer.datas_mut().first_mut() else {
                return;
            };
            let Some(bytes) = data.data() else {
                return;
            };
            let room = bytes.len() / 4;
            let n = if requested == 0 {
                room
            } else {
                requested.min(room)
            };
            let mut at = 0;
            output.pull(n, |s| {
                bytes[at..at + 4].copy_from_slice(&s.to_le_bytes());
                at += 4;
            });
            let chunk = data.chunk_mut();
            *chunk.offset_mut() = 0;
            *chunk.stride_mut() = 4;
            *chunk.size_mut() = (n * 4) as u32;
        })
        .register()?;

    let format = mono_f32(rate);
    let flags = StreamFlags::AUTOCONNECT | StreamFlags::MAP_BUFFERS | StreamFlags::RT_PROCESS;
    capture.connect(
        spa::utils::Direction::Input,
        None,
        flags,
        &mut [Pod::from_bytes(&format).unwrap()],
    )?;
    source.connect(
        spa::utils::Direction::Output,
        None,
        flags,
        &mut [Pod::from_bytes(&format).unwrap()],
    )?;

    eprintln!(
        "fabrial: virtual mic \"{DESCRIPTION}\" ({NODE_NAME}), reading {}, chain {chain_name} at {rate} Hz",
        opts.mic.as_deref().unwrap_or("the default mic"),
    );
    eprintln!(
        "fabrial: the chain delays by {:.1} ms, plus a buffer for its thread (one block, at least {:.1} ms). Ctrl-C stops.",
        ms(live.chain_latency as u64, rate),
        opts.margin_ms,
    );

    let report = Report::new(live);
    // Without a stats interval, it still looks every 10 s, and only prints
    // when something went wrong.
    let full = opts.stats_every.is_some();
    let every = opts.stats_every.unwrap_or(Duration::from_secs(10));
    let timer = mainloop.loop_().add_timer({
        let report = report.clone();
        move |_| report.print(full)
    });
    let _ = timer.update_timer(Some(every), Some(every));
    let _signals = [Signal::INT, Signal::TERM].map(|sig| {
        let ml = mainloop.clone();
        mainloop.loop_().add_signal_local(sig, move || ml.quit())
    });

    mainloop.run();

    // Without PipeWire there is nothing to put back, and a round trip would
    // never return.
    if !lost.get() {
        if let Some(m) = &mover {
            m.restore();
            roundtrip(&mainloop, &core);
        }
        let _ = capture.disconnect();
        let _ = source.disconnect();
    }
    report.print(true);
    match failure.borrow_mut().take() {
        Some(why) => bail!(why),
        None => Ok(()),
    }
}

/// Blocks SIGINT and SIGTERM before any other thread starts, so every
/// thread inherits the block and only PipeWire's loop receives them (it
/// reads them from a signalfd). Otherwise the kernel can hand one to
/// another thread, where it kills the process before the apps are moved
/// back.
fn block_stop_signals() {
    // SAFETY: plain libc calls on a local, zeroed signal set.
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        libc::sigaddset(&mut set, libc::SIGINT);
        libc::sigaddset(&mut set, libc::SIGTERM);
        libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut());
    }
}

/// Runs the loop until PipeWire has handled everything sent so far.
fn roundtrip(mainloop: &pw::main_loop::MainLoopRc, core: &pw::core::CoreRc) {
    let done = Rc::new(Cell::new(false));
    let Ok(pending) = core.sync(0) else {
        return;
    };
    let _listener = core
        .add_listener_local()
        .done({
            let done = done.clone();
            let mainloop = mainloop.clone();
            move |id, seq| {
                if id == pw::core::PW_ID_CORE && seq == pending {
                    done.set(true);
                    mainloop.quit();
                }
            }
        })
        .register();
    while !done.get() {
        mainloop.run();
    }
}

/// Properties both streams share.
fn stream_props(group: &str) -> PropertiesBox {
    let mut p = PropertiesBox::new();
    p.insert(*pw::keys::MEDIA_TYPE, "Audio");
    p.insert(*pw::keys::APP_NAME, "fabrial");
    // Same group: the same driver (the mic's clock) runs both. Same link
    // group: WirePlumber never feeds the virtual mic back into the input.
    p.insert(*pw::keys::NODE_GROUP, group);
    p.insert(*pw::keys::NODE_LINK_GROUP, group);
    // Neither is moved by other tools (EasyEffects moves every stream).
    p.insert("node.dont-move", "true");
    p
}

/// The one format both streams use: mono 32-bit float at the chain's rate.
/// PipeWire converts the mic to it (the headset sends 16 kHz s16) and
/// converts it to whatever apps ask for.
fn mono_f32(rate: u32) -> Vec<u8> {
    let mut info = AudioInfoRaw::new();
    info.set_format(AudioFormat::F32LE);
    info.set_rate(rate);
    info.set_channels(1);
    let mut position = [0; spa::sys::SPA_AUDIO_MAX_CHANNELS as usize];
    position[0] = spa::sys::SPA_AUDIO_CHANNEL_MONO;
    info.set_position(position);
    let obj = Object {
        type_: spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
        id: spa::param::ParamType::EnumFormat.as_raw(),
        properties: info.into(),
    };
    PodSerializer::serialize(Cursor::new(Vec::new()), &Value::Object(obj))
        .expect("an audio format always serializes")
        .0
        .into_inner()
}

fn report_state(what: &str, old: &StreamState, new: &StreamState, fail: &dyn Fn(String)) {
    match new {
        StreamState::Error(e) => fail(format!("{what}: {e}")),
        _ => eprintln!("fabrial: {what}: {old:?} → {new:?}"),
    }
}

/// Whether a node named like fabrial's mic already exists: two would
/// confuse apps, and both runs would move the same streams.
fn already_running(mainloop: &pw::main_loop::MainLoopRc, core: &pw::core::CoreRc) -> bool {
    let found = Rc::new(Cell::new(false));
    let Ok(registry) = core.get_registry() else {
        return false;
    };
    let _listener = registry
        .add_listener_local()
        .global({
            let found = found.clone();
            move |global| {
                if global.props.and_then(|p| p.get(*pw::keys::NODE_NAME)) == Some(NODE_NAME) {
                    found.set(true);
                }
            }
        })
        .register();
    roundtrip(mainloop, core);
    found.get()
}

fn ms(samples: u64, rate: u32) -> f64 {
    samples as f64 * 1000. / rate as f64
}

/// Prints what happened since the last look.
/// It owns the `Live`: the worker stops when the last copy goes.
#[derive(Clone)]
struct Report {
    live: Rc<Live>,
    last: Rc<RefCell<(Instant, Stats)>>,
    start: Instant,
    /// Whether the mic sent nothing during the last look.
    silent: Rc<Cell<bool>>,
}

impl Report {
    fn new(live: Live) -> Self {
        let now = Instant::now();
        Self {
            live: Rc::new(live),
            last: Rc::new(RefCell::new((now, Stats::default()))),
            start: now,
            silent: Rc::new(Cell::new(false)),
        }
    }

    /// Prints a line: always when `full`, otherwise only when something went
    /// wrong since the last look (gaps, lost audio, the mic going silent).
    fn print(&self, full: bool) {
        let now = Instant::now();
        let s = self.live.stats();
        let (then, prev) = self.last.replace((now, s.clone()));
        let wall = now.duration_since(then).as_secs_f64();
        let rate = self.live.rate;
        let t = now.duration_since(self.start).as_secs();
        let elapsed = match t {
            ..3600 => format!("{}:{:02}", t / 60, t % 60),
            _ => format!("{}:{:02}:{:02}", t / 3600, t / 60 % 60, t % 60),
        };
        let load = (s.busy - prev.busy).as_secs_f64() / wall * 100.;
        let mut line = format!(
            "{elapsed:>5}  chain {load:.1}% of a core (longest run {:.1} ms)  buffer {:.0} ms  block {:.1} ms",
            s.worst.as_secs_f64() * 1000.,
            ms(s.waiting.saturating_sub(s.block), rate),
            ms(s.block, rate),
        );
        let mut problem = false;
        let silent = s.captured == prev.captured;
        if silent {
            line += "  NO AUDIO FROM THE MIC";
            problem |= !self.silent.get();
        } else if self.silent.get() {
            line += "  the mic is back";
            problem = true;
        }
        self.silent.set(silent);
        for (name, now, before) in [
            ("gaps", s.gaps, prev.gaps),
            ("input dropped", s.dropped, prev.dropped),
        ] {
            if now > before {
                line += &format!("  {name} {}", now - before);
                problem = true;
            }
        }
        if s.skipped > prev.skipped {
            line += &format!("  skipped {:.0} ms", ms(s.skipped - prev.skipped, rate));
            problem = true;
        }
        if full || problem {
            eprintln!("{line}");
        }
    }
}
