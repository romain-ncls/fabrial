# fabrial: working rules for Claude

A mic enhancer meant to replace EasyEffects, starting with one laptop and one
headset. Read `docs/roadmap.md` first: it holds the decisions and the current
phase. `docs/research.md` has the model survey, and `docs/takes/` what each
recording showed.

## The setup it targets

- **Laptop:** `wandersail`, a Core Ultra 5 125U running Arch Linux and
  PipeWire. The whole machine is declared in `~/dotfiles`, so any package or
  service this project needs goes there, never only on the machine.
- **Headset:** Dell Alienware Pro Wireless, USB `413c:a529`. Its mic is 16 kHz
  mono s16 in hardware. PipeWire calls it
  `alsa_input.usb-Dell_Inc_AWPRO_H_Wireless_Game_0000000000000000-00.mono-fallback`.
- **Headset control:** `awpro` (`~/git/awpro`, also Rust) reads and sets the
  headset: `awpro status --json`, `awpro mic noise-cancel on|off`. The mic's
  own noise reduction turns itself back on whenever the headset restarts.
- **Current chain:** EasyEffects with preset `awpro-v3`. It moves every
  recording stream onto its own output, so a recorder that must read the raw
  mic needs `pw-record -P '{ node.dont-move = true }'`. Check where streams
  are connected (`pactl list source-outputs`), don't assume.

## Recordings

- **Never in this repo.** The repo is public, and takes hold the voices of
  people nearby. Takes live in `~/Documents/mic-takes/<date-time-label>/`.
  `.gitignore` blocks audio files, but don't rely on it.
- **Don't name coworkers or the employer** in docs or commits.
- **Every result comes from the scripts:** `scripts/analyze-take.sh <take>`.
  Claude can't listen, so findings come from the numbers and the spectrograms
  (read the PNGs); Romain listens. Write each take's findings in
  `docs/takes/<date>-<label>.md`.

## Code and tools

- **Rust** for the app, and **shell + ffmpeg + gawk** for scripts. No Python,
  not even for one-off helpers.
- **One heavy job at a time** (renders, model runs, builds), under
  `systemd-run --user --scope -p MemoryMax=…`. Parallel heavy jobs have
  frozen this machine before.
- **Claude cannot run sudo:** ask Romain to run anything that needs root.

## Git

- **Commits:** `feat:`, `fix:`, `docs:`, `chore:`, with a scope when it helps.
  They are SSH-signed. Push right after each commit.
- **Main:** ask before committing to it. Work that takes several commits goes
  on a `feat/<name>` branch and is squashed into main when Romain says it's
  done.
- **Staging:** stage files by name, never `git add -A`.
