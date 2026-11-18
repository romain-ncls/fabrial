# Research (November 2026)

The survey behind the model and platform choices. Quality numbers are the
authors' own unless stated otherwise. Scores from different papers are not
comparable: a model's score on VoiceBank-DEMAND (VB-D) moves a lot with its
training data. For example, DeepFilterNet3 scores PESQ 3.17 in its own paper
but 2.80 in DPDFNet's table. Our own recorded takes are the benchmark that
decides.

## The constraint: a 16 kHz mic

The headset (Dell Alienware Pro Wireless, USB `413c:a529`) sends its mic as
16 kHz mono, 16-bit, and nothing else (`/proc/asound/<card>/stream0` lists
`Rates: 16000`). A model built for 16 kHz loses nothing. A 48 kHz model
spends most of its work on empty frequencies.

## Denoising models (causal, real-time)

| model | rate | latency | compute | params | quality (source) | weights | license |
|---|---|---|---|---|---|---|---|
| **DPDFNet-2 / 4 / 8** (Ceva, 2025) | 16 kHz (8 kHz; 48 kHz "hr" for -2, -8) | ~40 ms (20 ms window, 2-frame look-ahead) | 1.35 / 2.36 / 4.37 GMAC/s | 2.5 / 2.8 / 3.5 M | Ceva's 12-language low-SNR set: PESQ 3.14–3.20 vs DFN3 2.76; DNS4 blind DNSMOS OVRL 3.10–3.12 vs DFN3 2.96; VB-D PESQ 2.66–2.71 vs DFN3 2.80 but SI-SNR 23.6–24.1 vs 21.2 | PyTorch, ONNX, TFLite; streaming in sherpa-onnx (C API and Rust crate) | Apache-2.0, code and weights |
| **DeepFilterNet3** (the low-latency variant is in use) | 48 kHz | 40 ms (low-latency: 20 ms, no lookahead) | ~0.35 GMAC/s | 2.1 M | VB-D PESQ 3.17 (own paper) | ONNX, run in Rust with tract | MIT/Apache-2.0, weights included |
| **FastEnhancer-T / B / S** (2025) | 16 and 48 kHz | 32 ms | 0.055 / 0.26 / 0.66 GMAC/s | 22 / 92 / 195 k | VB-D PESQ 2.99 / 3.13 / 3.19, same table as GTCRN 2.87 | PyTorch, ONNX | MIT |
| **GTCRN** | 16 kHz | 32 ms | ~0.035 GMAC/s | 24–48 k | VB-D PESQ 2.87 | streaming ONNX, sherpa-onnx | MIT |
| **UL-UNAS** | 16 kHz | 32 ms | 0.035 GMAC/s | 171 k | VB-D PESQ 3.09 | streaming ONNX (2026) | MIT |
| **RNNoise 0.2** | 48 kHz | ~20 ms | tiny | ~0.35 M (unverified) | VB-D PESQ 2.29 (2018 model) | C, weights compiled in | BSD-3 |

Notes:

- **DPDFNet** is DeepFilterNet2 with dual-path RNN blocks in the encoder. It is
  the closest successor to what runs today. Ceva published no CPU real-time
  figures; we have to measure them.
- **DeepFilterNet** has had no new model since 2023.
- **Without released weights:** LiSenNet (MIT code; the original cannot
  stream), FSPEN, ULCNet and Fast-ULCNet, CoFi-Lite and AdaptCRN.
- **Excluded:** Meta's Denoiser is non-commercial and archived.
- **No open models** from Microsoft (beyond NSNet2), NVIDIA or Google.

## Voices nearby: target-speaker extraction

None of the models above removes nearby talkers: they are trained to keep
speech, and babble is speech. What pushes coworkers down today is the
headset's level advantage plus the expander. Options:

- **A gate steered by your voice (to try):** enroll ~10 s of your voice. A
  speaker-embedding model (ECAPA, WeSpeaker) checks every 0.5–1 s whether the
  current speech is yours, and keeps the expander closed when it isn't. It
  reacts slowly, so it should only hold the gate closed; the level-based gate
  stays fast.
- **VoxLock** (Apache-2.0, Hugging Face):
  - Causal target-speaker extraction from a 3 s enrollment: BSRNN, 27 M
    params, 16 kHz, ~24 ms latency, ONNX.
  - Self-reported 4.95% word error rate vs 5.30% for Krisp.
  - Real-time factor 0.69 on 4 threads of an M2 Pro (int8). Probably too slow
    for a Core Ultra 5 125U, but worth one benchmark.
- **pDeepFilterNet2** (2024): DeepFilterNet2 conditioned on a speaker
  embedding. No weights, but the recipe could be retrained on DeepFilterNet's
  code.
- **Papers only, no weights:** TEA-PSE, Personalized PercepNet,
  VoiceFilter-Lite.
- **No usable weights yet:** the REAL-TSE challenge baselines (SLT 2026) and
  WeSep.

## Bandwidth extension (16 → 48 kHz)

- **Opus 1.6** (Dec 2025): ships a small wideband-to-fullband model (~70 MMAC/s,
  BSD), but only inside its decoder. Using it on mic audio means extracting it.
- **The gain is cosmetic:** extending the band doesn't improve intelligibility.
  Later, if at all.

## Existing products

- **Teams:** voice isolation, built from a voice profile on the device. It
  removes voices that don't match yours. Teams only.
- **Zoom:** personalized audio isolation, from a voiceprint. Zoom only.
- **Discord:** noise suppression, but no equivalent of voice isolation found.
- **Google Meet:** its noise cancellation reportedly does not filter other
  people talking.
- **Apple Voice Isolation:** works only in apps built on Apple's voice
  processing; it is not a virtual mic.
- **Windows Studio Effects (Voice Focus):** needs an NPU and the laptop
  maker's support.
- **NVIDIA Broadcast:** needs an RTX GPU.
- **Krisp:** virtual mic with background-voice cancellation, paid.
- **Open source:**
  - werman/noise-suppression-for-voice: RNNoise as a plugin, GPL-3.
  - NoiseTorch: abandoned after a security incident.
  - Cadmus: a GUI for werman's plugin.

So for coworkers on Teams, the built-in isolation already targets nearby
voices. What this app would add for them: the same sound in every app
(Discord included), and the voice-shaping chain.

## A system-wide virtual mic, per OS

- **Linux:** a PipeWire virtual source, as EasyEffects does.
- **macOS:**
  - **How:** a user-space AudioServerPlugIn: a `.driver` bundle in
    `/Library/Audio/Plug-Ins/HAL`, installed with admin rights, then
    `coreaudiod` restarts. A daemon processes the real mic and hands the audio
    to the plug-in.
  - **Signing:** needs a Developer ID ($99/year) and notarization.
  - **Building blocks:** BlackHole is GPL-3; libASPL is MIT C++.
- **Windows:**
  - **A real virtual device is a kernel driver.** Windows loads it only if
    Microsoft signs it: attestation signing needs an EV certificate and a
    verified company account.
  - **VB-Cable:** needs a paid licence for company use.
  - **Cheapest route:** Equalizer APO (GPL) processes the real capture device
    and hosts 64-bit VST2 plugins, so a VST2 build of the chain would run
    there. Users report it can be unreliable on mics.

## Runtimes

- **ONNX Runtime:** through the `ort` crate. It runs every candidate above,
  on Linux, Windows and macOS.
- **tract:** pure Rust. DeepFilterNet's own Rust code uses it.
- **NPU:** for 10 ms frames, handing each frame to the NPU probably costs more
  than computing it on the CPU. Measure before bothering.

## Sources

- DPDFNet:
  - <https://arxiv.org/abs/2512.16420>
  - <https://github.com/ceva-ip/DPDFNet>
  - <https://huggingface.co/Ceva-IP/DPDFNet>
- DeepFilterNet:
  - <https://arxiv.org/abs/2305.08227>
  - <https://github.com/Rikorose/DeepFilterNet>
- FastEnhancer:
  - <https://arxiv.org/abs/2509.21867>
  - <https://github.com/aask1357/fastenhancer>
- GTCRN and UL-UNAS:
  - <https://github.com/Xiaobin-Rong/gtcrn>
  - <https://arxiv.org/abs/2503.00340>
  - <https://github.com/Xiaobin-Rong/ul-unas>
- RNNoise: <https://github.com/xiph/rnnoise>
- sherpa-onnx speech enhancement: <https://k2-fsa.github.io/sherpa/onnx/speech-enhancement/index.html>
- VoxLock: <https://huggingface.co/nphearum/voxlock>
- pDeepFilterNet2: <https://arxiv.org/abs/2404.08022>
- REAL-TSE challenge: <https://real-tse.github.io/challenge/>
- Opus 1.6: <https://opus-codec.org/demo/opus-1.6>
- Teams voice isolation: <https://learn.microsoft.com/en-us/microsoftteams/voice-isolation>
- Windows driver signing: <https://learn.microsoft.com/en-us/windows-hardware/drivers/dashboard/code-signing-reqs>
- VB-Cable licensing: <https://vb-audio.com/Services/licensing.htm>
- macOS plug-ins:
  - libASPL: <https://github.com/gavv/libASPL>
  - BlackHole: <https://github.com/ExistentialAudio/BlackHole>
- Headset mic noise reduction: <https://www.windowscentral.com/accessories/headphones-headsets/alienware-pro-headset-review>
