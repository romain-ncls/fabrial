#!/usr/bin/env bash
# Records the headset mic twice, until Ctrl+C:
#   raw.flac        the mic before EasyEffects (16 kHz mono, as the headset sends it)
#   processed.flac  EasyEffects' output, what the call app receives
# plus EasyEffects' live settings and the headset's state (through awpro).
# Only your mic: not the call's audio.
# Takes go to ~/Documents/mic-takes, never into this repo (they hold other
# people's voices). Analyse one with scripts/analyze-take.sh.
# Usage: scripts/record-easyeffects.sh [label]
set -u
pgrep -x easyeffects >/dev/null || { echo "EasyEffects isn't running: start it first." >&2; exit 1; }
d=~/Documents/mic-takes/$(date +%F-%H%M)${1:+-$1}
mkdir -p "$d"
cp -r ~/.config/easyeffects/db "$d/easyeffects-db"
if command -v awpro >/dev/null && awpro status --json > "$d/headset.json" 2>/dev/null; then
  grep -q '"noise_cancel":true' "$d/headset.json" &&
    echo "Note: the headset's own mic noise reduction is ON (awpro mic noise-cancel off to disable)."
else
  rm -f "$d/headset.json"; echo "Note: headset state not saved (awpro missing or headset unreachable)."
fi
# node.dont-move: EasyEffects moves every recording stream to its own output;
# this keeps the raw one on the headset.
pw-record -P '{ node.dont-move = true }' \
  --target alsa_input.usb-Dell_Inc_AWPRO_H_Wireless_Game_0000000000000000-00.mono-fallback \
  --rate 16000 --channels 1 --format s16 "$d/raw.flac" &
pw-record -P '{ node.dont-move = true }' \
  --target easyeffects_source --rate 48000 --channels 2 --format s24 "$d/processed.flac" &
trap 'kill %1 %2' INT
echo "Recording into $d (Ctrl+C stops)"
wait; wait
echo "Stopped: $(du -sh "$d" | cut -f1) in $d"
