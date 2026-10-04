#!/bin/sh
# Runs fontelle-app's real-plugin walks (tests/real_plugin_sessions.rs) on
# every installed instrument, each in a process of its own, and writes one
# line per instrument and walk to $OUT (default target/real-plugin-sweep).
#
#   tools/real-plugin-sweep.sh [walk-filter]
#
# A plugin that crashes or hangs is reported as such and the sweep goes on.
set -u
cd "$(dirname "$0")/.."
OUT="${OUT:-target/real-plugin-sweep}"
mkdir -p "$OUT"
FILTER="${1:-}"
BIN=$(cargo test -p fontelle-app --test real_plugin_sessions --no-run --message-format=json 2>/dev/null \
    | grep -o '"executable":"[^"]*real_plugin_sessions[^"]*"' | head -1 | cut -d'"' -f4)
[ -x "$BIN" ] || { echo "could not build the walks"; exit 1; }
"$BIN" list_installed_instruments --ignored --exact --nocapture 2>/dev/null \
    | grep '^SWEEP' | cut -f3 | sort -u > "$OUT/instruments.txt"
echo "$(wc -l < "$OUT/instruments.txt") instruments" >&2
: > "$OUT/summary.txt"
while IFS= read -r name; do
    safe=$(printf '%s' "$name" | tr -c 'A-Za-z0-9._-' '_')
    FONTELLE_REAL_ONLY="$name" timeout 300 "$BIN" $FILTER --ignored --nocapture --test-threads=1 \
        > "$OUT/$safe.log" 2>&1
    code=$?
    case $code in
        0) verdict=ok ;;
        124) verdict=HUNG ;;
        101) verdict=FAILED ;;
        *) verdict="CRASHED($code)" ;;
    esac
    failing=$(grep -E '^test .* FAILED$' "$OUT/$safe.log" | sed 's/^test //; s/ \.\.\. FAILED//' | tr '\n' ' ')
    printf '%s\t%s\t%s\n' "$verdict" "$name" "$failing" | tee -a "$OUT/summary.txt"
done < "$OUT/instruments.txt"
