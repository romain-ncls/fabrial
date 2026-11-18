# fabrial

A microphone enhancer for calls: it removes the noise around you, shapes your
voice, and offers the result as a virtual microphone that any app can use.

**Status: early.** The Rust chain runs offline on recorded takes, to check it
against the current setup (EasyEffects); the live microphone comes next. See
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
