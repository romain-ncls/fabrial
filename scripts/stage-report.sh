#!/usr/bin/env bash
# What each stage of a chain does, from a render made with --taps
# (fabrial render … --taps), 10 ms window by 10 ms window:
# - by raw input level: how much each stage changes the level;
# - on your voice: how much that change varies (a fixed gain doesn't vary,
#   a compressor at work does), and how often peaks pass full scale;
# - how even your voice is: its level in quiet and loud moments (400 ms
#   blocks where you talk), after each stage;
# - the ends and starts of your phrases (around a pause of 0.5 s or more):
#   how much each stage lowers them, since a gate that cuts the quiet last
#   word of a sentence is the worst thing it can do;
# - between your words (at least 1 s from your voice): how much sound is
#   left after each stage, and how loud;
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
function cl(x) { return x < -100 ? -100 : x }
# How much each stage lowers the windows W[1..n] that still have sound.
function edge(title, what, W, n,   c, j, k, x, A, big) {
  printf "\n%s (%s; %d windows with sound in the raw input):\n\n", title, what, n
  printf "%-20s %8s %8s %18s\n", "stage", "median", "p10", "lowered > 6 dB"
  for (c = 1; c <= N; c++) { k = 0; big = 0; delete A
    for (j = 1; j <= n; j++) { if (L[c-1, W[j]] <= -100) continue
      x = cl(L[c, W[j]]) - cl(L[c-1, W[j]]); A[++k] = x; if (x < -6) big++ }
    if (k) printf "%-20s %+8.1f %+8.1f %16.1f %%\n", name[c+1], q(A, k, 0.5), q(A, k, 0.1), 100*big/k
    else printf "%-20s %8s\n", name[c+1], "silent" } }
BEGIN { split(names, name, " ")
  for (c = 0; c <= N; c++) { getline line < lf; split(line, F, " "); I[c] = F[1]; TP[c] = F[3] } }
{ w = rms(0); t++
  vo[t] = w > SP
  for (c = 0; c <= N; c++) { L[c, t] = rms(c); if (rms(c) <= -120) sil[c]++ }
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
  # Blocks of 400 ms where you talk at least half the time: the level of
  # each, quiet ones against loud ones. Unlike the loudness range (LRA),
  # nearby voices between your words do not count.
  for (i = 1; i <= t; i++) { k = int((i-1)/40); bv[k] += vo[i]; for (c = 0; c <= N; c++) BE[c, k] += 10^(L[c, i]/10) }
  for (k in bv) if (bv[k] >= 20) blk[++nbk] = k
  printf "How even your voice is (%d blocks of 400 ms where you talk at least half the time):\n\n", nbk
  printf "%-20s %10s %10s %10s %10s\n", "", "quiet p10", "middle", "loud p90", "spread"
  if (nbk) for (c = 0; c <= N; c++) { delete A; for (j = 1; j <= nbk; j++) A[j] = 10*log(BE[c, blk[j]]/40)/log(10)
    lo = q(A, nbk, 0.1); hi = q(A, nbk, 0.9)
    printf "%-20s %10.1f %10.1f %10.1f %10.1f\n", name[c+1], lo, q(A, nbk, 0.5), hi, hi - lo }
  printf "\n(your voice\x27s level, dBFS, in its quiet, middle and loud moments. A\n compressor that evens it out narrows the spread.)\n\n"
  printf "After each stage, over the whole stretch:\n\n%-20s %10s %12s %16s\n", "", "LUFS", "true peak", "digital silence"
  for (c = 0; c <= N; c++) printf "%-20s %10s %12s %14.1f %%\n", name[c+1], I[c], TP[c], 100*sil[c]/t
  # Windows at least 1 s (100 windows) from any window of your voice.
  for (i = 1; i <= t; i++) pre[i] = pre[i-1] + vo[i]
  for (i = 1; i <= t; i++) { a = i - 100 < 1 ? 1 : i - 100; z = i + 100 > t ? t : i + 100
    if (pre[z] - pre[a-1] == 0) away[++na] = i }
  printf "\nBetween your words (at least 1 s from your voice, %.1f s):\n\n", na/100
  printf "%-20s %16s %20s %16s\n", "", "digital silence", "average level (dB)", "loudest 10 ms"
  for (c = 0; c <= N; c++) { e = 0; s0 = 0; top = -120
    for (j = 1; j <= na; j++) { x = L[c, away[j]]; if (x <= -120) s0++; else e += 10^(x/10); if (x > top) top = x }
    printf "%-20s %14.1f %% %20.1f %16.1f\n", name[c+1], 100*s0/na, (e > 0 ? 10*log(e/na)/log(10) : -120), top }
  printf "\n(average level: the power average over that time, silence included.)\n"

  # Phrase ends: up to 0.6 s after a window of your voice that no other
  # follows for 0.5 s. Starts: up to 0.3 s before one that none precedes.
  for (i = 1; i <= t; i++) if (vo[i]) {
    z = i + 50 > t ? t : i + 50; if (pre[z] - pre[i] == 0) for (j = i + 1; j <= i + 60 && j <= t; j++) if (L[0, j] > -65) tail[++nt] = j
    a = i - 50 < 1 ? 1 : i - 50; if (i > 1 && pre[i-1] - pre[a-1] == 0) for (j = i - 30; j < i; j++) if (j >= 1 && L[0, j] > -65) head[++nh] = j }
  edge("Ends of your phrases", "up to 0.6 s after your voice, before a pause", tail, nt)
  edge("Starts of your phrases", "up to 0.3 s before your voice, after a pause", head, nh)
  printf "\n(how much each stage lowers those windows; a window it silences counts\n as lowered down to -100 dB.)\n"
}' | tee "$s/report.txt"
