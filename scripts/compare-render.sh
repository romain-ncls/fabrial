#!/usr/bin/env bash
# Compares a render (fabrial render) with what EasyEffects sent for the same
# stretch of the take, 10 ms window by 10 ms window, sorted by raw level.
# Writes <render>/compare/: report.txt, spectrograms.png (raw, render,
# EasyEffects) and the envelopes.
# Needs the take's analysis first (scripts/analyze-take.sh): its raw/processed
# offset places the stretch in processed.flac.
# Usage: scripts/compare-render.sh <take-dir> <render-dir>
# Tunables (environment): SPEECH_DB (raw level that counts as your voice, -40).
set -euo pipefail
take=${1:?usage: compare-render.sh <take-dir> <render-dir>}
r=${2:?usage: compare-render.sh <take-dir> <render-dir>}
SPEECH_DB=${SPEECH_DB:--40}
. "$(dirname "$0")/lib.sh"
c="$r/compare"; mkdir -p "$c"

lag=$(gawk '/lags raw by/{print $5}' "$take/analysis/report.txt")
start=$(gawk '$1=="start"{print $2}' "$r/render.txt")
dur=$(duration "$r/out.wav")
# EasyEffects' output for the stretch, with up to 1 s more on each side for the alignment.
read -r ee_start margin < <(gawk -v s="$start" -v l="$lag" 'BEGIN{m = s+l < 1 ? s+l : 1; print s+l-m, m}')
echo "cutting…"
ffmpeg -v error -y -ss "$start" -t "$dur" -i "$take/raw.flac" -c:a pcm_s16le "$c/raw.wav"
ffmpeg -v error -y -ss "$ee_start" -t "$(gawk -v d="$dur" -v m="$margin" 'BEGIN{print d+2*m}')" \
  -i "$take/processed.flac" -af "pan=mono|c0=0.5*c0+0.5*c1" -c:a pcm_f32le "$c/easyeffects.wav"
echo "envelopes…"
envelope "$c/raw.wav" "" 160 "$c/raw.env"
envelope "$r/out.wav" "" 480 "$c/render.env"
envelope "$c/easyeffects.wav" "" 480 "$c/easyeffects.env"
echo "levels…"
read -r rI rLRA rTP < <(loud "$r/out.wav"); read -r eI eLRA eTP < <(loud "$c/easyeffects.wav")
read -r rpk rpkc < <(peak "$r/out.wav"); read -r epk epkc < <(peak "$c/easyeffects.wav")

gawk -v SP="$SPEECH_DB" -v M="$margin" -v start="$start" -v out="$c" \
     -v rI="$rI" -v rLRA="$rLRA" -v rTP="$rTP" -v eI="$eI" -v eLRA="$eLRA" -v eTP="$eTP" \
     -v rpk="$rpk" -v rpkc="$rpkc" -v epk="$epk" -v epkc="$epkc" '
function c(x){ return x<-60 ? -60 : (x>0 ? 0 : x) }
function mmss(s){ s = int(s*10+0.5)/10; return sprintf("%d:%04.1f", int(s/60), s-60*int(s/60)) }
function q(S, k, f,   A) { asort(S, A); return k ? A[int((k-1)*f)+1] : 0 }
FILENAME ~ /raw.env$/ { w[n++] = $2; next }
# A float render keeps tails far below what a 24-bit recording holds: count them as silence.
FILENAME ~ /render.env$/ { o[no++] = $2 < -120 ? -120 : $2; next }
{ e[ne++] = $2 }
END {
  # Offset of EasyEffects vs the render (whose own delay is already removed).
  best = -3; L0 = int(M*100)
  for (L = 0; L <= 2*L0; L++) { sx = sy = sxx = syy = sxy = k = 0
    for (i = 0; i < no; i++) { j = i + L; if (j >= ne) break
      x = c(o[i]); y = c(e[j]); sx += x; sy += y; sxx += x*x; syy += y*y; sxy += x*y; k++ }
    v = (sxx - sx*sx/k) * (syy - sy*sy/k); v = v > 0 ? (sxy - sx*sy/k) / sqrt(v) : -2
    if (v > best) { best = v; lag = L } }
  printf "Stretch: %s from %s into raw.flac\n", mmss(no/100), mmss(start)
  printf "EasyEffects lined up %+d ms from the offset found by analyze-take.sh (correlation %.3f).\n\n", (lag-L0)*10, best
  printf "%-12s %10s %8s %12s %22s\n", "", "LUFS", "LRA", "true peak", "samples at peak"
  printf "%-12s %10s %8s %12s %12.2f dBFS × %d\n", "render", rI, rLRA, rTP, rpk, rpkc
  printf "%-12s %10s %8s %12s %12.2f dBFS × %d\n\n", "EasyEffects", eI, eLRA, eTP, epk, epkc

  printf "raw level (dBFS) | %% of time | render median | EE median | render-EE median, p10, p90 | silenced: render, EE\n"
  for (i = 0; i < no && i < n; i++) { j = i + lag; if (j >= ne) break
    b = int((w[i] + 120) / 3); if (w[i] < -90) b = 10
    cnt[b]++; tot++; R[b] = R[b] " " o[i]; E[b] = E[b] " " e[j]
    if (o[i] <= -119) { rs[b]++; rsall++ }; if (e[j] <= -119) { es[b]++; esall++ }
    if (o[i] > -119 && e[j] > -119) { D[b] = D[b] " " (o[i] - e[j])
      if (w[i] > SP) { nv++; DV[nv] = o[i] - e[j] } } }
  for (b = 10; b <= 40; b++) if (cnt[b]) {
    k = split(substr(R[b], 2), A, " "); rm = q(A, k, 0.5)
    k = split(substr(E[b], 2), A, " "); em = q(A, k, 0.5)
    k = split(substr(D[b], 2), A, " ")
    printf "%4d..%4d | %5.1f %% | %7.1f | %7.1f | %s | %5.1f %% %5.1f %%\n", b*3-120, b*3-117, 100*cnt[b]/tot, rm, em,
      k ? sprintf("%+6.1f %+6.1f %+6.1f", q(A, k, 0.5), q(A, k, 0.1), q(A, k, 0.9)) : "     (both silent)    ",
      100*rs[b]/cnt[b], 100*es[b]/cnt[b] }
  printf "Digital silence: render %.1f %%, EasyEffects %.1f %% of the time.\n", 100*rsall/tot, 100*esall/tot
  if (nv) printf "Your voice (raw > %d dBFS, %.1f s): render - EasyEffects median %+.1f dB, p10 %+.1f, p90 %+.1f.\n", SP, nv/100, q(DV, nv, 0.5), q(DV, nv, 0.1), q(DV, nv, 0.9)

  # The 30 s with the most voice, for the spectrograms.
  for (i = 0; i < n; i++) if (w[i] > SP) v30[int(i/3000)]++
  top = 0; for (s in v30) if (v30[s] > v30[top]) top = s
  print top*30, lag/100 > (out "/.spec") }
' "$c/raw.env" "$c/render.env" "$c/easyeffects.env" | tee "$c/report.txt"

read -r s elag < "$c/.spec"; rm "$c/.spec"
ffmpeg -v error -y -ss "$s" -t 30 -i "$c/raw.wav" -ss "$s" -t 30 -i "$r/out.wav" \
  -ss "$(gawk -v s="$s" -v l="$elag" 'BEGIN{print s+l}')" -t 30 -i "$c/easyeffects.wav" \
  -filter_complex "[0:a]showspectrumpic=s=1400x260:legend=1:scale=log:drange=100[a];[1:a]aresample=16000,showspectrumpic=s=1400x260:legend=1:scale=log:drange=100[b];[2:a]aresample=16000,showspectrumpic=s=1400x260:legend=1:scale=log:drange=100[c];[a][b][c]vstack=inputs=3" \
  -frames:v 1 "$c/spectrograms.png"
echo "spectrograms (raw, render, EasyEffects; 30 s from $s s into the stretch): $c/spectrograms.png"
