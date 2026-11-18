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
- **Next: the virtual mic.** A PipeWire virtual source that apps pick like
  any mic, running the same chain live.
- **Then the fixes:** apply the [planned fixes](reference-chains.md) to the
  chain, one at a time, each checked on the same takes.
- **Recorder built in:**
  - the raw mic and every stage's output, sample-aligned, in one file
  - a rolling buffer of the last few minutes, with a shortcut to save it
- **Offline render:** any chain version on any raw take.
- **Analysis in the app:** what `analyze-take.sh` does today, plus DNSMOS
  (Microsoft's quality predictor, itself an ONNX model).
- **Packaging:** declared in `~/dotfiles` (`pkg: {git, ref, path}`), replacing
  `easyeffects` in the media module.

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
