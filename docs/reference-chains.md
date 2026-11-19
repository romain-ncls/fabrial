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

### What each stage does

`scripts/stage-report.sh` measures how much each stage changes the level,
10 ms by 10 ms, from a render made with `--taps`. On the two stretches of
the first take:

| stage | on your voice | on quieter sounds | in short |
|---|---|---|---|
| DeepFilterNet | no change | 1–7 dB lower between -72 and -42 dBFS raw; turns 61 % (4:00–6:00) and 88 % (44:00–47:00) of the time into digital silence | does the cleanup |
| expander | no change | no change, wherever any sound is left | does next to nothing: DeepFilterNet already silences what it would lower (see below) |
| EQ | ±1 dB | +0.2 to +0.9 dB | changes the tone, barely the level; to judge by ear |
| compressor | +15 dB | +15 dB | a fixed volume boost: it lowers your voice by more than 1 dB 0.2 % of the time at 4:00–6:00 and 9.8 % at 44:00–47:00, never by more than 6 dB |

Your voice sits mostly between -40 and -20 dBFS, under the compressor's
-20 dB threshold, so the compressor does little more than add its +15 dB
of makeup gain:

- **It clips:** peaks pass full scale in 5.4 % of your voice at 4:00–6:00
  and 28.5 % at 44:00–47:00 (true peaks +6.5 and +9.4 dBTP).
- **It doesn't even out your level:** you were 5.7 dB louder at
  44:00–47:00 than at 4:00–6:00 (-26.8 vs -32.5 LUFS raw), and the output
  is still 4.8 dB louder (-13.1 vs -17.9 LUFS).
- **It raises everything else by 15 dB too:** the clicks and the nearby
  voices that DeepFilterNet lets through.

How much boost the voice needs: on 2026-11-18, with EasyEffects off and no
boost at all, a coworker found the voice "a bit quiet", though less than
they remembered. In Discord's Studio mode, Discord adds no gain control of
its own, so people hear what fabrial sends. Teams' gain control can't be
turned off.

#### The expander as a gate under the voice

The expander is meant as a gentle gate: it should lower anything quieter
than the voice, as a safety net behind DeepFilterNet, and as the only
cleanup when DeepFilterNet is off to save CPU. It matters most for nearby
voices: they are speech, so DeepFilterNet keeps them.

4:00–6:00 and 44:00–47:00 hold almost nothing between the words (-84 dBFS
raw). 51:00–57:00 has nearby voices. In it:

- **Your voice vs the rest, raw:** the quietest quarter of your speech sits
  below -42 to -49 dBFS, the quietest tenth below -56 to -65. Sound at
  least 1 s away from your voice has a median of -67 dBFS, 90 % of it is
  below -56, and the loudest 10 ms reach -40. The two overlap at the
  bottom of your voice.
- **After DeepFilterNet:** 97 % of the time between your words becomes
  digital silence. What's left peaks at -44 dBFS. The expander only lowers
  the average of that time by 1.9 dB, and leaves the loudest unchanged.
- **Without DeepFilterNet** (`v3-no-dfn`): the expander lowers the faint
  sounds a lot (18 dB at -72 to -66 dBFS raw) but the louder ones, the ones
  you can hear, barely or not at all (5 dB at -60 to -54, none above -54).
  The average between your words only drops by 3 dB, and the loudest
  stays at -40 dBFS, then -25 dBFS after the +15 dB.
- **CPU:** without DeepFilterNet, the chain renders at 342× real time,
  against 11× with it.

So the safety net is needed, but its -50 dB threshold and 2:1 ratio miss
the sounds it's there for. A level gate can't catch the nearby voices that
are as loud as the quiet parts of your voice: that needs to know whose
voice it is ([phase 4](roadmap.md#4-voices-nearby)).

#### The edges of your phrases

A gate must never cut the quiet last word of a sentence, nor the first
sound of one. `stage-report.sh` measures both: the windows with sound up to
0.6 s after your voice before a pause of at least 0.5 s, and up to 0.3 s
before it after one.

- **Ends: DeepFilterNet cuts, not the expander.** DeepFilterNet lowers 15 to
  26 % of those windows by more than 6 dB. They are the fading tail of the
  last syllable: about -62 dBFS raw, some 20 dB under your voice, 50 to
  120 ms after it. Removing them makes phrases stop abruptly. Its
  attenuation limit (80 dB, so it may remove them entirely) is the setting
  to look at. The expander, with its 150 ms release, leaves ends alone.
- **Starts: the expander cuts.** In the two clean stretches, the sound just
  before your voice is the start of the word itself, at about -49 dBFS,
  within 50 ms of it. Before a phrase, the expander is closed and takes its
  15 ms attack to open, so it lowers those first 10 to 20 ms by about
  16 dB: the first sound of each phrase loses its edge.

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
