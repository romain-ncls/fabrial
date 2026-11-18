# Reference chains

The app's first job is to reproduce the chain in use today, then to improve it.
Both chains below are the starting points.

## awpro-v3: the EasyEffects preset in use

File: [`reference/easyeffects/awpro-v3.json`](../reference/easyeffects/awpro-v3.json).
The live settings saved with the 2026-11-17 take match it.

| # | stage | settings |
|---|---|---|
| 1 | DeepFilterNet3 **low-latency** (LADSPA 0.5.6, 48 kHz) | attenuation limit 80 dB (at most 80 dB of reduction; 100 turns the limit off), post-filter beta 0.02, min/max processing thresholds -10 / 30 (ERB) / 20 (DF) dB |
| 2 | expander (LSP), downward | threshold -50 dB, ratio 2:1, attack 15 ms, release 150 ms, knee -6 dB, RMS detection |
| 3 | equalizer (LSP), IIR, 5 bands | high-pass 50 Hz · low shelf 90 Hz +3 dB · bell 425 Hz -2 dB (Q 1) · bell 3.5 kHz +3 dB (Q 0.7) · high shelf 9 kHz +2 dB |
| 4 | compressor (LSP), downward | threshold -20 dB, ratio 4:1, attack 5 ms, release 75 ms, knee -6 dB, **makeup +15 dB**, RMS detection |

What the settings really do. These come from LSP's source and were checked
against the installed plugins:

- **DeepFilterNet:** Arch's plugin embeds the low-latency DeepFilterNet3
  model, not the standard one.
  - The low-latency model has no lookahead: 10 ms of model delay, plus
    10 ms of buffering in the plugin.
  - When the plugin's worker falls behind, it adds 10 ms more on the fly.
    During the first take, its output drifted 20 ms later over two
    minutes.
  - It outputs digital silence for any 10 ms frame below -70 dB.
- **Dry -80.01 dB is not "off":** EasyEffects only sends 0 at -100 dB, so
  both dynamics stages mix in the unprocessed input at -80 dB.
- **Knee -6 dB:** the curve bends over 12 dB, from 6 dB below the threshold
  to 6 dB above.
- **Sidechain:** the "reactivity" is a 10 ms moving RMS window.
- **Envelope:** attack and release are times to 70.7 % of a step.
- **Release threshold:** it is relative to the threshold. The compressor's
  -40 dB means that, below -60 dBFS, a falling level uses the attack time.
- **Boost threshold and amount:** they do nothing in downward mode.
- **EQ, as built:**
  - The 50 Hz high-pass has an effective Q of 0.85 (LSP uses 2/(1+Q)).
  - The 425 Hz bell dips -2.5 dB at its centre, not -2.
  - The 9 kHz shelf lowers DC by 0.08 dB.
  - The whole EQ comes to +1.5 dB at 50 Hz, +1.8 at 90 Hz, -2.6 at 425 Hz
    and +2.9 at 3.5 kHz.

### The Rust copy: `chains/awpro-v3.toml`

**Each stage against the real LSP plugin:** I fed both the same input,
using the port values EasyEffects sends. The difference over two minutes of
the first take:

| stage | RMS difference | worst sample |
|---|---|---|
| expander | -147 dBFS | -95 dBFS |
| EQ | -104 dBFS | -80 dBFS |
| compressor | -164 dBFS | -124 dBFS |

That is float rounding. LSP evaluates its biquads in a different order, and
the EQ's low bands magnify the rounding.

**The whole chain against what EasyEffects sent during the meeting:** a
render of the raw take, compared with `scripts/compare-render.sh`.

| stretch | correlation | loudness, copy vs EasyEffects | your voice, copy − EasyEffects (median, p10, p90) | digital silence, copy vs EasyEffects |
|---|---|---|---|---|
| 4:00–6:00 | 0.992 | -17.9 / -17.8 LUFS | -0.1, -1.7, +1.4 dB | 58.1 / 57.6 % |
| 44:00–47:00 | 0.996 | -13.1 / -13.7 LUFS | +0.0, -1.4, +2.1 dB | 87.5 / 87.4 % |

What still differs:

- **Peaks:** the copy is float, so it keeps what goes over full scale (up to
  +9.4 dBFS). EasyEffects' 24-bit recording clips there.
- **Quiet sounds** (raw -72 to -51 dBFS) come out 1–5 dB lower in the copy.
  DeepFilterNet's output differs slightly from EasyEffects' run:
  - it cuts 10 ms frames at different points;
  - EasyEffects' plugin delay moved during the take;
  - PipeWire's resampler isn't ours.
  
  Below the expander's threshold, those differences come out doubled.

#### Live

`scripts/live-check.sh` plays a stretch of the take as a fake mic, runs
`fabrial run` on it, records the "fabrial" mic, and compares that with the
offline render of the same stretch:

| stretch | correlation | loudness, live vs offline | your voice, live − offline (median, p10, p90) | digital silence, live vs offline |
|---|---|---|---|---|
| 4:00–5:00 | 0.998 | -16.7 / -16.6 LUFS | -0.0, -1.1, +1.0 dB | 71.7 / 71.9 % |
| 44:00–45:00 | 0.998 | -13.3 / -13.2 LUFS | -0.0, -1.3, +1.1 dB | 82.5 / 82.6 % |

The differences come from the same causes as above: live, PipeWire
converts the 16 kHz mic to 48 kHz, and DeepFilterNet's 10 ms frames start
at a different point.

Known problems (from the [first take](takes/2026-11-17-meeting.md)):

- **No limiter after +15 dB of makeup gain:** the output clips.
- **The 9 kHz high shelf does nothing:** the headset sends 16 kHz audio, so
  nothing above 8 kHz exists.
- **The 90 Hz low shelf works against the 50 Hz high-pass:** it boosts what the
  high-pass should remove.
- **The high-pass comes after the expander:** low rumble reaches the expander's
  detector.
- **DeepFilterNet3 runs at 48 kHz:** PipeWire upsamples the 16 kHz mic, so most
  of the model's work goes to empty frequencies.
- **The 425 Hz cut is deeper than set:** -2.5 dB, because of LSP's
  Butterworth-style bell.

## The "radio voice" chain

From a short video, which was an ad for an app called Patch
(<https://vm.tiktok.com/ZN8aLoMQ8/>). It is the classic broadcast chain, in this
order:

1. **High-pass at about 100 Hz:** below it there is almost no voice, only
   rumble.
2. **Downward expander:** lowers the quiet parts between words (fan, keyboard,
   breathing). It must come before the compressor, or the compressor raises
   them again.
3. **De-esser:** lowers the hiss of S sounds. On a 16 kHz mic the hiss sits
   between 4 and 8 kHz.
4. **EQ boost around 3.5 kHz:** the range that makes words clear.
5. **Compressor:** narrows the gap between quiet and loud words.
6. **Limiter:** a ceiling for laughs and shouts, so they don't distort.

## Differences, and the target for the first native chain

awpro-v3 compared with the radio chain:

- **Extra in v3:** a denoiser.
- **Missing from v3:** the de-esser and the limiter.
- **High-pass:** too low in v3, partly cancelled by the shelf, and placed after
  the expander.

Planned first native chain:

| # | stage | settings |
|---|---|---|
| 1 | high-pass | ~80–100 Hz |
| 2 | denoiser | DeepFilterNet3 first, then the [model candidates](research.md) |
| 3 | expander | as in v3 |
| 4 | de-esser | 4–8 kHz |
| 5 | EQ | without the 9 kHz shelf or the 90 Hz boost |
| 6 | compressor | makeup gain tuned to about -16 LUFS |
| 7 | limiter | -1 dBFS ceiling |

Every change is checked on the same recorded takes.
