#!/usr/bin/env bash
# First-pass analysis of a take from record-take.sh, with ffmpeg and gawk.
# Writes <take>/analysis/: report.txt, spectrograms (raw on top, processed below)
# and the 10 ms envelopes it computed them from.
# Usage: scripts/analyze-take.sh <take-dir>
# Tunables (environment): SPEECH_DB (raw level that counts as your voice, -40),
# LAG_MAX (largest raw/processed offset searched, in seconds, 120).
set -euo pipefail
d=${1:?usage: analyze-take.sh <take-dir>}
a="$d/analysis"; mkdir -p "$a"
SPEECH_DB=${SPEECH_DB:--40}; LAG_MAX=${LAG_MAX:-120}
. "$(dirname "$0")/lib.sh"
echo "envelopes…"
envelope "$d/raw.flac" "" 160 "$a/raw.env"
envelope "$d/processed.flac" "pan=mono|c0=0.5*c0+0.5*c1," 480 "$a/processed.env"

echo "levels…"
read -r rawpk rawpkc < <(peak "$d/raw.flac"); read -r propk propkc < <(peak "$d/processed.flac")
read -r rawI rawLRA rawTP < <(loud "$d/raw.flac"); read -r proI proLRA proTP < <(loud "$d/processed.flac")

echo "alignment and report…"
gawk -v SP="$SPEECH_DB" -v LMAX="$LAG_MAX" -v rawpk="$rawpk" -v propk="$propk" -v propkc="$propkc" \
     -v rawI="$rawI" -v proI="$proI" -v proLRA="$proLRA" -v proTP="$proTP" -v out="$a" '
function c(x){ return x<-60 ? -60 : (x>0 ? 0 : x) }
function mmss(s){ return sprintf("%d:%04.1f", int(s/60), s-60*int(s/60)) }
function corr(A, B, n, m, L, step,   i, x, y, sx, sy, sxx, syy, sxy, k, v) {
  for (i = 0; i < n; i += step) { if (i+L < 0 || i+L >= m) continue
    x = A[i]; y = B[i+L]; sx += x; sy += y; sxx += x*x; syy += y*y; sxy += x*y; k++ }
  if (k < 100) return -2
  v = (sxx - sx*sx/k) * (syy - sy*sy/k); return v > 0 ? (sxy - sx*sy/k) / sqrt(v) : -2 }
NR==FNR { r[FNR-1]=$2; n=FNR; next }
{ p[FNR-1]=$2; pk[FNR-1]=$3; m=FNR }
END {
  # Offset of processed vs raw: 100 ms averages first, then 10 ms around the best.
  for (i = 0; i < n; i++) R[int(i/10)] += c(r[i])/10
  for (i = 0; i < m; i++) P[int(i/10)] += c(p[i])/10
  best = -3; for (L = -LMAX*10; L <= LMAX*10; L++) { v = corr(R, P, int(n/10), int(m/10), L, 1); if (v > best) { best = v; bl = L } }
  for (i = 0; i < n; i++) rc[i] = c(r[i]); for (i = 0; i < m; i++) pc[i] = c(p[i])
  best = -3; for (L = bl*10-15; L <= bl*10+15; L++) { v = corr(rc, pc, n, m, L, 1); if (v > best) { best = v; lag = L } }

  printf "Take: raw %s, processed %s\n", mmss(n/100), mmss(m/100)
  printf "Processed lags raw by %.2f s (correlation %.3f). Times below are in processed.flac.\n\n", lag/100, best
  for (i = 0; i < n; i++) { if (r[i] > SP) sp++; F[i] = r[i] }
  asort(F); printf "Raw: median level %.1f dBFS, peak %.2f dBFS, %s LUFS, your voice (> %d dBFS) %.1f s\n", F[int(n/2)], rawpk, rawI, SP, sp/100
  printf "Processed: %s LUFS integrated, LRA %s LU, true peak %s dBTP\n", proI, proLRA, proTP
  if (propk + 0 >= -0.01) printf "Processed CLIPS: %d samples at full scale\n", propkc
  else printf "Processed peak %.2f dBFS, no clipping\n", propk

  # How the chain treats each raw level.
  printf "\nraw level (dBFS) | %% of time | processed median | processed p90 | %% fully silenced\n"
  for (i = 0; i < n; i++) { j = i + lag; if (j < 0 || j >= m) continue
    b = int((r[i] + 120) / 3); if (r[i] < -90) b = 10
    cnt[b]++; tot++; vals[b] = vals[b] " " p[j]; if (p[j] <= -119) { sil[b]++; silall++ } }
  for (b = 10; b <= 40; b++) if (cnt[b]) { k = split(substr(vals[b], 2), A, " "); asort(A)
    printf "%4d..%4d | %5.1f %% | %7.1f | %7.1f | %5.1f %%\n", b*3-120, b*3-117, 100*cnt[b]/tot, A[int(k*0.5)+1], A[int(k*0.9)+1], 100*sil[b]/cnt[b] }
  printf "Processed is digital silence %.1f %% of the time.\n", 100*silall/tot

  # Clipping and voice by 30 s block.
  for (i = 0; i < n; i++) { j = i + lag; if (j < 0 || j >= m) continue; s = int(j/3000)
    if (r[i] > SP) bsp[s]++; if (pk[j] >= -0.1) bcl[s]++ }
  printf "\n30 s blocks with the most clipping (10 ms windows at full scale / seconds of your voice):\n"
  for (s in bcl) printf "%d %d\n", bcl[s], s | "sort -rn | head -5 > \"" out "/.blocks\""
  close("sort -rn | head -5 > \"" out "/.blocks\"")
  while ((getline line < (out "/.blocks")) > 0) { split(line, f, " "); if (!gotTop) { top = f[2]; gotTop = 1 }
    printf "  %s  %d windows / %.1f s\n", mmss(f[2]*30), f[1], bsp[f[2]]/100 }
  if (!gotTop) { top = 0; for (s in bsp) if (bsp[s] > bsp[top]) top = s; print "  none" }
  print top*30 > (out "/.spec-voice")

  # Mid-level sound away from your voice (within 1 s): background voices, clicks, breath.
  for (i = 0; i < n; i++) if (r[i] > SP) for (k = i-100; k <= i+100; k++) near[k] = 1
  inrun = 0; lbest = -1000
  for (i = 0; i <= n; i++) { mid = (i < n && r[i] > -70 && r[i] <= SP && !near[i])
    if (mid) { if (!inrun) { s0 = i; inrun = 1 }; last = i }
    if (inrun && (i - last > 10 || i == n)) { inrun = 0; if (last - s0 < 30) continue
      runs++; dur += (last - s0) / 100; mx = -200; pmx = -200
      for (k = s0; k <= last; k++) { if (r[k] > mx) mx = r[k]; if (k+lag >= 0 && k+lag < m && p[k+lag] > pmx) pmx = p[k+lag] }
      rl[runs] = sprintf("%7.1f %s  %4.1f s  raw max %6.1f  processed max %6.1f", pmx, mmss((s0+lag)/100), (last-s0)/100, mx, pmx)
      if (pmx > lbest) { lbest = pmx; lstart = s0 + lag } } }
  printf "\nSound away from your voice: %d stretches, %.1f s in total. Loudest after processing:\n", runs, dur
  for (k = 1; k <= runs; k++) print rl[k] | "sort -rn | head -10 | cut -c9-"
  close("sort -rn | head -10 | cut -c9-")
  print (runs ? int(lstart/100) - 10 : -1) > (out "/.spec-leak") }
' "$a/raw.env" "$a/processed.env" | tee "$a/report.txt"

# Spectrograms of 30 s (raw on top, processed resampled to 16 kHz below).
lag=$(gawk '/lags raw by/{print $5}' "$a/report.txt")
spec() { # processed-time start, name
  local s=$1; [ "$s" -lt 0 ] && s=0
  ffmpeg -v error -y -ss "$(gawk -v s="$s" -v l="$lag" 'BEGIN{x=s-l; print x<0?0:x}')" -t 30 -i "$d/raw.flac" -ss "$s" -t 30 -i "$d/processed.flac" \
    -filter_complex "[0:a]showspectrumpic=s=1400x300:legend=1:scale=log:drange=100[a];[1:a]pan=mono|c0=c0,aresample=16000,showspectrumpic=s=1400x300:legend=1:scale=log:drange=100[b];[a][b]vstack" \
    -frames:v 1 "$a/$2.png"; echo "spectrogram: $a/$2.png"; }
spec "$(cat "$a/.spec-voice")" spectrogram-voice
[ "$(cat "$a/.spec-leak")" -ge 0 ] && spec "$(cat "$a/.spec-leak")" spectrogram-background
rm -f "$a"/.blocks "$a"/.spec-*
