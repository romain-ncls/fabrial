#!/usr/bin/env bash
# What each stage of a chain does, from a render made with --taps
# (fabrial render … --taps), 10 ms window by 10 ms window:
# - by raw input level: how much each stage changes the level;
# - on your voice: how much that change varies (a fixed gain doesn't vary,
#   a compressor at work does), and how often peaks pass full scale;
# - after each stage: loudness, true peak and digital silence.
# Writes <render>/stages/report.txt and the envelopes.
# Usage: scripts/stage-report.sh <render-dir>
# Tunables (environment): SPEECH_DB (raw level that counts as your voice, -40).
set -euo pipefail
r=${1:?usage: stage-report.sh <render-dir>}
SPEECH_DB=${SPEECH_DB:--40}
. "$(dirname "$0")/lib.sh"
[ -f "$r/taps.wav" ] || { echo "no taps.wav in $r: render with --taps" >&2; exit 1; }
s="$r/stages"; mkdir -p "$s"

mapfile -t names < <(gawk '/^  [0-9]+-[a-z0-9-]+ +latency/{print $1}' "$r/render.txt")
n=${#names[@]}
rate=$(ffprobe -v error -show_entries stream=sample_rate -of csv=p=0 "$r/taps.wav")
echo "envelopes…"
for c in $(seq 0 "$n"); do envelope "$r/taps.wav" "pan=mono|c0=c$c," $((rate / 100)) "$s/$c.env"; done
echo "loudness…"
: > "$s/loudness"
for c in $(seq 0 "$n"); do loud "$r/taps.wav" "pan=mono|c0=c$c," >> "$s/loudness"; done

# shellcheck disable=SC2046
paste $(for c in $(seq 0 "$n"); do echo "$s/$c.env"; done) |
gawk -v SP="$SPEECH_DB" -v N="$n" -v names="input ${names[*]}" -v lf="$s/loudness" '
function q(S, k, f,   A) { asort(S, A); return A[int((k-1)*f)+1] }
function rms(c) { return $(3*c+2) }
function pk(c) { return $(3*c+3) }
BEGIN { split(names, name, " ")
  for (c = 0; c <= N; c++) { getline line < lf; split(line, L, " "); I[c] = L[1]; TP[c] = L[3] } }
{ w = rms(0); t++
  for (c = 0; c <= N; c++) if (rms(c) <= -120) sil[c]++
  if (w < -90) next
  b = int((w + 120) / 6); cnt[b]++
  for (c = 1; c <= N; c++) if (rms(c) <= -120) gone[b, c]++
  for (c = 1; c <= N; c++) if (rms(c-1) > -120 && rms(c) > -120) {
    k = ++nb[b, c]; D[b, c, k] = rms(c) - rms(c-1) }
  if (w > SP) { v++
    for (c = 1; c <= N; c++) {
      if (pk(c) > 0) over[c]++
      if (rms(c-1) > -120 && rms(c) > -120) { k = ++nv[c]; V[c, k] = rms(c) - rms(c-1) } } }
}
END {
  printf "Level change at each stage (median dB), by raw input level:\n\n%-12s %7s", "raw (dBFS)", "time"
  for (c = 1; c <= N; c++) printf " %12s", substr(name[c+1], 1, 12)
  printf "\n"
  for (b = 0; b < 25; b++) if (cnt[b]) {
    printf "%4d..%4d  %6.1f %%", b*6-120, b*6-114, 100*cnt[b]/t
    for (c = 1; c <= N; c++) { k = nb[b, c]
      if (!k || gone[b, c] > cnt[b] / 2) { printf " %12s", "silent"; continue }
      delete A; for (i = 1; i <= k; i++) A[i] = D[b, c, i]; printf " %+12.1f", q(A, k, 0.5) }
    printf "\n" }
  printf "\n(\"silent\": by then, most windows of that level are digital silence.)\n\n"
  printf "Your voice (raw > %d dBFS, %.1f s):\n\n", SP, v/100
  printf "%-20s %8s %8s %8s %18s\n", "stage", "median", "p10", "p90", "peaks over 0 dBFS"
  for (c = 1; c <= N; c++) { k = nv[c]; delete A; for (i = 1; i <= k; i++) A[i] = V[c, i]
    printf "%-20s %+8.1f %+8.1f %+8.1f %16.1f %%\n", name[c+1], q(A, k, 0.5), q(A, k, 0.1), q(A, k, 0.9), 100*over[c]/v }
  printf "\n(median, p10, p90: how much the stage changes your voice\x27s level. When\n p10 and p90 are close, the stage acts as a fixed volume change.)\n\n"
  printf "After each stage, over the whole stretch:\n\n%-20s %10s %12s %16s\n", "", "LUFS", "true peak", "digital silence"
  for (c = 0; c <= N; c++) printf "%-20s %10s %12s %14.1f %%\n", name[c+1], I[c], TP[c], 100*sil[c]/t
}' | tee "$s/report.txt"
