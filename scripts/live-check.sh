#!/usr/bin/env bash
# Checks the live path against the offline render, on a recorded take and
# without touching the real mic:
#   1. plays a stretch of the take's raw.flac as a temporary fake mic;
#   2. runs `fabrial run` on that fake mic and records the "fabrial" mic;
#   3. renders the same stretch offline (`fabrial render`);
#   4. lines both up and compares them, 10 ms window by 10 ms window.
# Nothing is linked to a speaker; the script stops if anything is.
# Writes <take>/renders/live-check-<chain>-<from>/: live.wav, fabrial.log,
# the offline render and report.txt.
# Usage: scripts/live-check.sh <take-dir> [chain, chains/awpro-v3.toml] [from, 4:00] [seconds, 60]
# Needs a release build (cargo build --release) and no other fabrial running.
set -euo pipefail
take=${1:?usage: live-check.sh <take-dir> [chain] [from] [seconds]}
chain=${2:-chains/awpro-v3.toml}
from=${3:-4:00}
secs=${4:-60}
here=$(cd "$(dirname "$0")/.." && pwd)
if [ -f "$chain" ]; then chain=$(realpath "$chain"); else chain="$here/$chain"; fi
[ -f "$chain" ] || { echo "no chain file: $chain" >&2; exit 1; }
. "$here/scripts/lib.sh"
fabrial=$here/target/release/fabrial
[ -x "$fabrial" ] || { echo "build first: cargo build --release" >&2; exit 1; }
if pw-cli ls Node | grep -q 'node.name = "fabrial_source"'; then
  echo "another fabrial is running: stop it first" >&2; exit 1
fi

from_s=$(gawk -v t="$from" 'BEGIN{n=split(t,p,":"); s=0; for(i=1;i<=n;i++) s=s*60+p[i]; print s}')
name=$(gawk -F'"' '/^name *=/{print $2; exit}' "$chain")
out="$take/renders/live-check-$name-${from//:/m}"
mkdir -p "$out"
units=(fabrial-check-run fabrial-check-rec fabrial-check-play)
# One by one: systemctl refuses a whole list when one unit has already gone.
cleanup() { for u in "${units[@]}"; do systemctl --user stop "$u" 2>/dev/null || true; done; }
trap cleanup EXIT

echo "cutting ${secs} s from $from…"
ffmpeg -v error -y -ss "$from_s" -t "$secs" -i "$take/raw.flac" -c:a pcm_s16le "$out/raw.wav"

echo "live…"
t0=$(date +%s)
cd "$here"
systemd-run --user --collect --unit=fabrial-check-run -q -p MemoryMax=2G --working-directory="$here" \
  "$fabrial" run --chain "$chain" --mic fabrial_check_mic --stats 10
sleep 1
systemd-run --user --collect --unit=fabrial-check-rec -q -p MemoryMax=1G \
  pw-record --target fabrial_source -P '{ node.dont-move = true }' --rate 48000 --channels 1 --format f32 "$out/live.wav"
sleep 1
systemd-run --user --collect --unit=fabrial-check-play -q -p MemoryMax=1G \
  pw-play -P '{ media.class = "Audio/Source", node.name = "fabrial_check_mic", node.description = "fabrial check mic", node.dont-move = true, priority.session = 0 }' "$out/raw.wav"
sleep 2
leak=$(pw-link -l | gawk '/^(fabrial|pw-play|pw-record)/{o=1; next} /^[^ ]/{o=0} o && /\|->/ && /alsa_output/')
[ -z "$leak" ] || { echo "something reached a speaker, stopping: $leak" >&2; exit 1; }
timeout $((secs + 30)) bash -c 'while systemctl --user is-active -q fabrial-check-play; do sleep 1; done'
sleep 2
cleanup
journalctl --user -u fabrial-check-run --since "@$t0" --no-pager -o cat | grep -E '^fabrial:|^ *[0-9]+:[0-9]{2} ' > "$out/fabrial.log" || true

echo "offline…"
"$fabrial" render "$out/raw.wav" --chain "$chain" --out "$out/render" > /dev/null

echo "comparing…"
envelope "$out/raw.wav" "" 160 "$out/raw.env"
envelope "$out/render/out.wav" "" 480 "$out/render.env"
envelope "$out/live.wav" "" 480 "$out/live.env"
read -r rI _ rTP < <(loud "$out/render/out.wav")
read -r lI _ lTP < <(loud "$out/live.wav")
{
  echo "Take: $take, ${secs} s from $from, chain $chain"
  echo
  gawk -v rI="$rI" -v lI="$lI" -v rTP="$rTP" -v lTP="$lTP" '
  function c(x){ return x<-60 ? -60 : (x>0 ? 0 : x) }
  function q(S, k, f,   A) { asort(S, A); return A[int((k-1)*f)+1] }
  FILENAME ~ /raw.env$/ { w[n++] = $2; next }
  # Float tails far below anything audible count as silence.
  FILENAME ~ /render.env$/ { o[no++] = $2 < -120 ? -120 : $2; next }
  { e[ne++] = $2 < -120 ? -120 : $2 }
  END {
    best = -3
    for (L = 0; L + no/2 < ne && L < 1000; L++) { sx = sy = sxx = syy = sxy = k = 0
      for (i = 0; i < no; i++) { j = i + L; if (j >= ne) break
        x = c(o[i]); y = c(e[j]); sx += x; sy += y; sxx += x*x; syy += y*y; sxy += x*y; k++ }
      v = (sxx - sx*sx/k) * (syy - sy*sy/k); v = v > 0 ? (sxy - sx*sy/k) / sqrt(v) : -2
      if (v > best) { best = v; lag = L } }
    for (i = 0; i < no; i++) { j = i + lag; if (j >= ne) break
      if (w[i] > -40 && o[i] > -60) d[nd++] = e[j] - o[i]
      if (o[i] <= -120) sr++; if (e[j] <= -120) se++; m++ }
    printf "%-26s %10s %10s\n", "", "live", "offline"
    printf "%-26s %10s %10s\n", "loudness (LUFS)", lI, rI
    printf "%-26s %10s %10s\n", "true peak (dBFS)", lTP, rTP
    printf "%-26s %9.1f%% %9.1f%%\n", "digital silence", 100*se/m, 100*sr/m
    printf "\nCorrelation of the 10 ms levels: %.4f\n", best
    printf "Voice (raw above -40 dBFS, %d windows), live - offline: median %+.2f dB, p10 %+.2f, p90 %+.2f\n",
      nd, q(d, nd, .5), q(d, nd, .1), q(d, nd, .9)
  }' "$out/raw.env" "$out/render.env" "$out/live.env"
  echo
  echo "fabrial's log:"
  sed 's/^/  /' "$out/fabrial.log"
} > "$out/report.txt"
rm -f "$out"/*.env
cat "$out/report.txt"
echo "→ $out"
