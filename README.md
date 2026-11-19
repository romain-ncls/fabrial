# fabrial

A microphone enhancer for calls: it removes the noise around you, shapes your
voice, and offers the result as a virtual microphone that any app can use.

**Status: early.** The Rust chain copies the current setup (EasyEffects). It
runs offline on recorded takes, or live as a virtual microphone that apps
are moved onto. Using it in real calls comes next. See
[the roadmap](docs/roadmap.md).

## Goals

- Replace EasyEffects for one headset (Dell Alienware Pro Wireless) on Linux
  with PipeWire: first the same chain, then better models.
- Record before and after on every change, so versions can be compared on the
  same audio, by ear and by numbers.
- Later: other headsets, Windows and macOS, and a light mode for weaker laptops.

## Chains

A chain is a TOML file: the rate it runs at, then its stages in order
(`chains/` holds the ones in use). Render a recorded take through one:

```sh
cargo build --release
target/release/fabrial render <take-dir> --chain chains/<name>.toml \
  [--from 4:00 --to 6:00] [--taps]
```

It writes `<take>/renders/<chain>[-<from>-<to>]/`:

- `out.wav`: what the virtual microphone would have sent.
- `taps.wav` (with `--taps`): the input, then each stage's output, one
  channel each.
- `render.txt`: the stretch, each stage's delay, and the speed.

Every file has the chain's delay removed, so it lines up with the raw input
sample for sample.

## Live

```sh
target/release/fabrial run --chain chains/<name>.toml [--mic <node name>]
```

It reads the default mic and offers the chain's output as a mic named
"fabrial" (`fabrial_source` in PipeWire), until Ctrl+C. Every 10 seconds it
prints the CPU the chain takes, the buffer it adds, and any gaps.

- **`--mic`** reads that mic and no other: if it's missing, fabrial stops
  with an error instead of reading the default one.
- **`--name`** changes the name, `fabrial_check` for example, so a second
  run (a test) can go next to the one in use. It must read another mic.
- **A muted mic** sends digital silence. After a second of it, the chain
  stops running until sound comes back, so a muted mic costs almost no CPU.

Apps keep picking the headset. Whenever an app records from the mic fabrial
reads, fabrial moves it onto its own mic, and puts it back on exit. It
leaves alone the streams that refuse to move (recorders such as
`scripts/record-easyeffects.sh`), level meters, and streams on other mics.
`--no-move` turns this off.

Next to EasyEffects: the apps that WirePlumber already sends to EasyEffects'
mic stay there. Quit EasyEffects, and they come back to the headset, then
to fabrial. WirePlumber remembers each app's mic, so they also go straight
to fabrial the next time it runs, and to the headset when it doesn't.

Building it needs `clang` and PipeWire's headers (Arch's `libpipewire`, which
`pipewire` pulls in).

### As a service

`packaging/arch/PKGBUILD` builds an Arch package from the checkout it sits
in. The package holds the binary, the chains (in `/usr/share/fabrial/chains/`)
and a user service, `fabrial.service`, which starts and stops with PipeWire
and restarts after a failure:

```sh
systemctl --user enable --now fabrial
journalctl --user -u fabrial    # the mic it reads, the apps it moves, problems
```

As a service, it only logs a line when something goes wrong: gaps, lost
audio, the mic going silent. To use another chain, override `ExecStart`
with `systemctl --user edit fabrial`.

## Scripts

- `scripts/record-easyeffects.sh [label]` records the headset's raw mic and
  EasyEffects' output side by side, until Ctrl+C, into
  `~/Documents/mic-takes/<date>-<label>/`.
- `scripts/analyze-take.sh <take-dir>` compares the two: levels, clipping,
  loudness, how each input level is treated, what got through between your
  words, and spectrograms.
- `scripts/compare-render.sh <take-dir> <render-dir>` compares a render with
  what EasyEffects sent for the same stretch, sorted by raw level, with
  spectrograms of the raw input, the render and EasyEffects.
- `scripts/stage-report.sh <render-dir>` shows what each stage of the chain
  does, from a render made with `--taps`: how much it changes the level, by
  raw input level and on your voice, where peaks pass full scale, and the
  loudness after each stage.
- `scripts/live-check.sh <take-dir> [chain] [from] [seconds]` plays a stretch
  of a take as a fake mic, runs `fabrial run` on it, and compares what the
  "fabrial" mic sent with the offline render. It doesn't touch the real mic,
  and runs next to a fabrial in use.

Recordings never go into this repo: they hold the voices of people nearby.

## Docs

- [Roadmap and decisions](docs/roadmap.md)
- [Research: models, products, virtual microphones](docs/research.md)
- [Reference chains](docs/reference-chains.md): the EasyEffects preset in use
  and the "radio voice" chain it is compared with
- [Takes](docs/takes/): what each recording showed

## The name

In Brandon Sanderson's *Stormlight Archive*, a fabrial is a gemstone device
built around a captured spren; its casing decides what the spren does. Here
the casing is the effects chain, and the spren is whichever model you load.

## License

MIT
