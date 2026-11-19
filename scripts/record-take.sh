#!/usr/bin/env bash
# Records the headset mic twice, until Ctrl+C:
#   raw.flac        the mic before any processing (16 kHz mono, as the
#                   headset sends it)
#   processed.flac  what the call app receives: fabrial's mic, or
#                   EasyEffects' when fabrial isn't running
# plus the chain in use (fabrial's chain.toml, or EasyEffects' settings) and
# the headset's state through awpro: at the start in headset.json, then
# every 10 s in headset.log, so a take shows when the headset changes its
# own noise reduction mid-call.
# Only your mic: not the call's audio.
# Takes go to ~/Documents/mic-takes, never into this repo (they hold other
# people's voices). Analyse one with scripts/analyze-take.sh.
# Usage: scripts/record-take.sh [label]
set -u
mic=alsa_input.usb-Dell_Inc_AWPRO_H_Wireless_Game_0000000000000000-00.mono-fallback
d=~/Documents/mic-takes/$(date +%F-%H%M)${1:+-$1}
if pw-cli ls Node | grep -q 'node.name = "fabrial_source"'; then
  processed=fabrial_source
  mkdir -p "$d"
  # The chain the running fabrial was started with.
  chain=$(ps -o args= -C fabrial | gawk '$2 == "run" && !/--name/ { for (i = 3; i < NF; i++) if ($i == "--chain" || $i == "-c") { print $(i+1); exit } }')
  if [ -f "$chain" ]; then cp "$chain" "$d/chain.toml"; else echo "Note: fabrial's chain file not found, not saved." >&2; fi
elif pgrep -x easyeffects >/dev/null; then
  processed=easyeffects_source
  mkdir -p "$d"
  cp -r ~/.config/easyeffects/db "$d/easyeffects-db"
else
  echo "Neither fabrial nor EasyEffects is running: start one first." >&2; exit 1
fi
headset() { command -v awpro >/dev/null && timeout 5 awpro status --json 2>/dev/null; }
if headset > "$d/headset.json"; then
  grep -q '"noise_cancel":true' "$d/headset.json" &&
    echo "Note: the headset's own mic noise reduction is ON (awpro mic noise-cancel off to disable)."
else
  rm -f "$d/headset.json"; echo "Note: headset state not saved (awpro missing or headset unreachable)."
fi
# node.dont-move: fabrial and EasyEffects move recording streams onto their
# own mic; this keeps the raw one on the headset. The processed one is
# recorded as 24-bit stereo, as before, so the analysis scripts read both
# kinds of take the same way.
pw-record -P '{ node.dont-move = true }' --target "$mic" \
  --rate 16000 --channels 1 --format s16 "$d/raw.flac" &
pw-record -P '{ node.dont-move = true }' --target "$processed" \
  --rate 48000 --channels 2 --format s24 "$d/processed.flac" &
# Seconds since the start, then the headset's state.
( t0=$(date +%s); while sleep 10; do echo "$(( $(date +%s) - t0 )) $(headset || echo unreachable)"; done ) > "$d/headset.log" &
trap 'kill %1 %2 %3' INT
echo "Recording the raw mic and $processed into $d (Ctrl+C stops)"
wait; wait; wait
echo "Stopped: $(du -sh "$d" | cut -f1) in $d"
