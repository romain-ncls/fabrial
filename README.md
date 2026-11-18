# fabrial

A microphone enhancer for calls: it removes the noise around you, shapes your
voice, and offers the result as a virtual microphone that any app can use.

**Status: early.** Today this repo holds the research, and a recording and
analysis toolkit around the current setup (EasyEffects). The Rust app comes
next: see [the roadmap](docs/roadmap.md).

## Goals

- Replace EasyEffects for one headset (Dell Alienware Pro Wireless) on Linux
  with PipeWire: first the same chain, then better models.
- Record before and after on every change, so versions can be compared on the
  same audio, by ear and by numbers.
- Later: other headsets, Windows and macOS, and a light mode for weaker laptops.

## Scripts

- `scripts/record-easyeffects.sh [label]` records the headset's raw mic and
  EasyEffects' output side by side, until Ctrl+C, into
  `~/Documents/mic-takes/<date>-<label>/`.
- `scripts/analyze-take.sh <take-dir>` compares the two: levels, clipping,
  loudness, how each input level is treated, what got through between your
  words, and spectrograms.

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
