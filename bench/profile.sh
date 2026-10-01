#!/bin/sh
# Where does the time and memory go? Profiles `axiom check` on a generated
# project with whatever is installed, and writes small text summaries to
# bench/perf/ (committed; the raw profiler data is not).
#
#   sh bench/profile.sh [SCALE]        default 100k (valgrind is ~50x slower than native)
#
# Uses `perf record -g` when it exists; otherwise valgrind's callgrind (exact
# instruction counts per function, deterministic) and massif (heap by
# allocation site). Instruction counts are the honest hotspot measure here:
# the CPU-time split between phases depends on the core count, the counts do not.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)
work=${AXIOM_BENCH_DIR:-${TMPDIR:-/tmp}/axiom-bench}
scale=${1:-100k}
out=$here/perf
mkdir -p "$out"

(cd "$root" && cargo build --release -q)
axiom=$root/target/release/axiom
dir=$work/$scale
[ -f "$dir/MANIFEST" ] || python3 "$here/gen.py" --flows "$scale" --out "$dir"
. "$dir/MANIFEST"
args="check -C $dir --today $today --color never"

if command -v perf >/dev/null 2>&1; then
    perf record -g -o "$work/perf.data" "$axiom" $args >/dev/null 2>&1 || true
    perf report -i "$work/perf.data" --stdio 2>/dev/null | head -80 > "$out/perf-$scale.txt"
    echo "wrote $out/perf-$scale.txt"
elif command -v valgrind >/dev/null 2>&1; then
    valgrind --tool=callgrind --callgrind-out-file="$work/callgrind.out" "$axiom" $args >/dev/null 2>"$work/callgrind.err" || true
    callgrind_annotate "$work/callgrind.out" 2>/dev/null | sed -n 20,60p | sed -E "s| \[/[^]]*\]||" | cut -c1-150 > "$out/callgrind-self-$scale.txt"
    callgrind_annotate --inclusive=yes "$work/callgrind.out" 2>/dev/null | sed -n 20,70p | sed -E "s| \[/[^]]*\]||" | cut -c1-150 > "$out/callgrind-inclusive-$scale.txt"
    valgrind --tool=massif --massif-out-file="$work/massif.out" "$axiom" $args >/dev/null 2>&1 || true
    ms_print --threshold=2 "$work/massif.out" > "$work/massif.txt" 2>/dev/null
    peak=$(sed -n 's/.*[ \[]\([0-9][0-9]*\) (peak).*/\1/p' "$work/massif.txt" | head -1)
    awk -v n="$peak" '$1 == n && $2 ~ /^[0-9,]+$/ { seen = 1 } seen' "$work/massif.txt" | sed -n 1,45p | sed -E "s| \(in /[^)]*\)||" | cut -c1-150 > "$out/massif-peak-$scale.txt"
    echo "wrote $out/callgrind-*-$scale.txt and $out/massif-peak-$scale.txt"
else
    echo "neither perf nor valgrind is installed; sample with gdb instead:" >&2
    echo "  $axiom $args & pid=\$!; for i in \$(seq 20); do gdb -p \$pid -batch -ex 'thread apply all bt 12'; sleep 0.2; done" >&2
    exit 1
fi
