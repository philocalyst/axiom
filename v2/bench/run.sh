#!/bin/sh
# Benchmarks the axiom CLI on generated projects.
#
#   sh bench/run.sh                 10k 100k 1m
#   sh bench/run.sh 10k 100k 1m 5m  any of the four scales
#
# For each scale: build release, generate the project (and its laws-free twin)
# into a temporary directory (never inside the repository), then time every
# command with peak RSS. Results are appended to $WORK/results.tsv, and a table
# is printed at the end.
#
#   AXIOM_BENCH_DIR      where projects are generated       (default $TMPDIR/axiom-bench)
#   AXIOM_BENCH_RUNS     runs per command, fastest is kept   (default 3; 1 for 5m)
#   AXIOM_BENCH_TIMEOUT  seconds before a command is killed  (default 60)
#   AXIOM_BENCH_KEEP=1   reuse projects that are already generated
#
# Phases: the CLI has no timing switch. `axiom sync` on a project with no
# `sync` declaration loads, parses and builds the model and stops (it never
# runs the engine), so
#     parse+model  = sync
#     engine       = check - sync
#     report       = command - check
# and `check` on the laws-free twin (same journal, no kinds, systems, budgets
# or restricted grants) isolates what evaluating laws costs.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)
work=${AXIOM_BENCH_DIR:-${TMPDIR:-/tmp}/axiom-bench}
timeout_s=${AXIOM_BENCH_TIMEOUT:-60}
scales=${*:-10k 100k 1m}

(cd "$root" && cargo build --release -q)
axiom=$root/target/release/axiom
mkdir -p "$work"
results=$work/results.tsv
[ -s "$results" ] || printf 'scale\tcommand\twall_s\tuser_s\tsys_s\tmaxrss_kb\texit\tout_bytes\truns\tminflt\tflows\n' > "$results"

for scale in $scales; do
    runs=${AXIOM_BENCH_RUNS:-3}
    [ "$scale" = 5m ] && runs=${AXIOM_BENCH_RUNS:-1}
    for variant in full nolaws; do
        dir=$work/$scale
        [ "$variant" = nolaws ] && dir=$work/$scale-nolaws
        if [ -z "${AXIOM_BENCH_KEEP:-}" ] || [ ! -f "$dir/MANIFEST" ]; then
            rm -rf "$dir"
            python3 "$here/gen.py" --flows "$scale" --variant "$variant" --out "$dir"
        fi
    done
    . "$work/$scale/MANIFEST"
    dir=$work/$scale
    # $1 = label, rest = axiom arguments (project and day are added)
    measure() {
        label=$1
        shift
        line=$(python3 "$here/timeit.py" --runs "$runs" --timeout "$timeout_s" --label "$label" -- \
            "$axiom" "$@" -C "$dir" --today "$today" --color never)
        printf '%s\t%s\t%s\n' "$scale" "$line" "$flows" | tee -a "$results" >/dev/null
        printf '  %-16s %s\n' "$label" "$(printf '%s' "$line" | cut -f2-6 | tr '\t' ' ')"
    }
    echo "== $scale: $flows flows, $people people, $years years"
    measure sync sync
    measure check check
    measure balance balance
    measure balance-value balance --value
    measure balance-monthly balance --monthly
    measure register register p1/bank/checking
    measure flow flow
    measure available available
    measure budget budget "$last_month"
    measure tax tax "$last_year" --for p1
    measure lots lots p1/invest/brokerage
    measure forecast forecast
    measure why-place why p1/bank/checking
    measure why-law why deferral-limit
    measure why-code why '^chk-p1-000010'
    measure why-line why journal/2024/03.ax:100
    # the laws-free twin: same journal, no laws
    dir=$work/$scale-nolaws
    measure check-nolaws check
    measure sync-nolaws sync
done

echo
echo "wall seconds, user+sys seconds, peak RSS MB, flows per second (of wall), by scale"
awk -F'\t' 'NR > 1 { printf "%-6s %-16s wall %8.3f  cpu %8.3f  rss %8.0f MB  %10.0f flows/s  exit %s\n", $1, $2, $3, $4 + $5, $6 / 1024, $11 / ($3 > 0 ? $3 : 1), $7 }' "$results"
