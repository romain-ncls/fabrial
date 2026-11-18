# Reference chains

The app's first job is to reproduce the chain in use today, then to improve it.
Both chains below are the starting points.

## awpro-v3: the EasyEffects preset in use

File: [`reference/easyeffects/awpro-v3.json`](../reference/easyeffects/awpro-v3.json).
The live settings saved with the 2026-11-17 take match it.

| # | stage | settings |
|---|---|---|
| 1 | DeepFilterNet3 (LADSPA, 48 kHz) | attenuation limit 80 dB (no limit), post-filter beta 0.02, min/max processing thresholds -10 / 30 (ERB) / 20 (DF) dB |
| 2 | expander (LSP), downward | threshold -50 dB, ratio 2:1, attack 15 ms, release 150 ms, knee -6 dB, RMS detection |
| 3 | equalizer (LSP), IIR, 5 bands | high-pass 50 Hz · low shelf 90 Hz +3 dB · bell 425 Hz -2 dB (Q 1) · bell 3.5 kHz +3 dB (Q 0.7) · high shelf 9 kHz +2 dB |
| 4 | compressor (LSP), downward | threshold -20 dB, ratio 4:1, attack 5 ms, release 75 ms, knee -6 dB, **makeup +15 dB**, RMS detection |

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
