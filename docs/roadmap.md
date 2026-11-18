# Roadmap and decisions

## Decisions so far

- **Rust.**
  - Real-time audio can't afford garbage-collector pauses.
  - DeepFilterNet's own runtime is Rust.
  - `awpro`, the headset controller, is Rust too.
- **The first target is one machine and one headset.** That's a Core Ultra 5
  125U laptop on Arch Linux with PipeWire, and a Dell Alienware Pro Wireless
  headset. Cross-platform comes later, but the core stays free of
  Linux-specific code so it remains possible.
- **Recording before and after is part of the app, not a side tool.**
  Iterating means recording raw once, rendering chain versions offline, and
  comparing them on the same audio.
- **The chain is a file:** a versioned description of the stages and their
  settings, so each version can be compared, kept and reverted.
- **No Python in the tooling:** shell, ffmpeg and gawk for scripts, Rust for
  the rest. Every model candidate ships ONNX, so no Python export step is
  needed.
- **PipeWire through pipewire-rs for now, pipewire-native later.**
  - pipewire-rs wraps PipeWire's C library. Building it needs `clang`, to
    read PipeWire's C headers; running fabrial doesn't.
  - [pipewire-native](https://docs.rs/pipewire-native/latest/) is PipeWire's
    own Rust client, meant to become the official Rust API. Version 0.2.0
    (September 2026) can't send or receive audio yet.
  - **Switch when it can carry audio.** All the PipeWire code lives in
    `crates/pipewire`, so the switch only rewrites that crate. The chain
    and the hand-off to its thread (`fabrial_core::live`) don't depend on
    PipeWire.
- **Apps keep picking the headset, as with EasyEffects.** fabrial will move
  the recording streams onto its virtual mic itself. Unlike EasyEffects, it
  will only move the streams that asked for the headset.
- **Recordings and model files are never committed.**

## Phases

### 0. Measure the current setup (now)

- Done: `scripts/record-easyeffects.sh` and `scripts/analyze-take.sh`, and the
  first take.
- Next: more takes. One with the headset's noise reduction on, and one in a
  loud moment.

### 1. Replace EasyEffects on Linux

- **Done: the chain, offline.**
  - `fabrial render` runs a recorded take through a chain file.
  - `chains/awpro-v3.toml` copies EasyEffects' preset, and
    `scripts/compare-render.sh` shows it matches. See
    [the copy](reference-chains.md#the-rust-copy-chainsawpro-v3toml).
  - There's also a limiter, for the planned fixes.
- **Done: the virtual mic.**
  - `fabrial run` runs a chain live and offers the result as a mic named
    "fabrial". `scripts/live-check.sh` shows it sends what the offline
    render sends (see [the copy](reference-chains.md#live)).
  - It moves the apps that record from the headset onto its mic, and puts
    them back on exit. It replaced EasyEffects in a real call on
    2026-11-18.
  - An Arch package (`packaging/arch`) runs it as a user service that
    starts with PipeWire.
  - **Delay:** the chain's thread adds one block (21 ms with PipeWire's
    usual 1024-sample block) on top of the chain's own 20 ms. So fabrial's
    mic lags EasyEffects' by about 21 ms.
  - **CPU:** live, the chain takes 14–19% of a core while you speak, but
    offline only about 4%. The scheduler probably puts its small, regular
    runs on the slow cores. The longest run seen was 15 ms, of the 21 ms a
    block allows.
  - **A gap at startup:** each start has one short gap, then none.
- **Next: the fixes.** Apply the [planned fixes](reference-chains.md) to the
  chain, one at a time, each checked on the same takes.
- **Recorder built in:**
  - the raw mic and every stage's output, sample-aligned, in one file
  - a rolling buffer of the last few minutes, with a shortcut to save it
- **Offline render:** any chain version on any raw take.
- **Analysis in the app:** what `analyze-take.sh` does today, plus DNSMOS
  (Microsoft's quality predictor, itself an ONNX model).
- **Packaging:** declared in `~/dotfiles` as its own module (`pkg: {git, ref,
  path}`), with the service enabled. EasyEffects stays installed, but no
  longer starts.

### 2. Better models

- **Candidates:** DPDFNet-2 and -4 (16 kHz, Apache-2.0), FastEnhancer-B and -T,
  GTCRN. See [research](research.md).
- **Comparison:** same takes, blind A/B, plus measured CPU cost on the 125U.

### 3. Headset integration (through awpro)

- **Read the headset's mic noise reduction** at startup and whenever it
  changes. It turns itself back on when the headset restarts.
- **Suggest turning it off** if the takes show it does more harm than good, or
  turn it off automatically when the user allows it.
- **Groundwork:** this needs awpro's protocol code available as a library
  crate.

### 4. Voices nearby

- **Speaker-steered gate:** enroll the user's voice and hold the gate closed
  when the speech isn't theirs.
- **VoxLock:** benchmark it.
- **Longer route:** train a personalized DeepFilterNet/DPDFNet variant.

### 5. Other headsets and systems

- **Per-headset profiles,** picked by USB ID.
- **macOS:** an AudioServerPlugIn virtual mic.
- **Windows:** a VST2 build for Equalizer APO first, a signed driver only if
  it proves worth it.
- **Light mode:** FastEnhancer-T or GTCRN for weak laptops.
