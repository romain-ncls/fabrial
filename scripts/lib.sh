# Shared ffmpeg + gawk helpers for the analysis scripts. Source it.
for t in ffmpeg ffprobe gawk; do command -v $t >/dev/null || { echo "missing: $t" >&2; exit 1; }; done

# 10 ms RMS and peak (dBFS) of a file, one "time rms peak" line per window
# (silence is -120).
envelope() { # in, prefilter (ends with a comma, or empty), samples per 10 ms, out
  ffmpeg -v error -i "$1" -af "${2}asetnsamples=n=$3:p=0,astats=metadata=1:reset=1:measure_perchannel=none:measure_overall=RMS_level+Peak_level,ametadata=mode=print:file=$4.meta" -f null -
  gawk 'NR>3 && /^frame/{split($3,a,":"); t=a[2]} /RMS_level/{split($0,b,"="); r=b[2]} /Peak_level/{split($0,c,"="); p=c[2]; if(r=="-inf")r=-120; if(p=="-inf")p=-120; if(t!="") printf "%.2f %.2f %.2f\n", t, r, p}' "$4.meta" > "$4"
  rm "$4.meta"; }

# "peak-dBFS count-at-peak" of the first channel.
peak() { ffmpeg -nostats -i "$1" -af "pan=mono|c0=c0,astats=measure_perchannel=none:measure_overall=Peak_level+Peak_count" -f null - 2>&1 |
  gawk '/Peak level dB/{l=$NF} /Peak count/{c=$NF} END{printf "%s %d\n", l, c}'; }

# "integrated-LUFS LRA true-peak" (EBU R128), after an optional prefilter
# (ends with a comma).
loud() { ffmpeg -nostats -i "$1" -af "${2:-}ebur128=peak=true" -f null - 2>&1 |
  gawk '/Summary/{s=1} s&&/ I:/{i=$2} s&&/LRA:/{r=$2} s&&/Peak:/{p=$2} END{printf "%s %s %s\n", i, r, p}'; }

# Duration in seconds.
duration() { ffprobe -v error -show_entries format=duration -of csv=p=0 "$1"; }
