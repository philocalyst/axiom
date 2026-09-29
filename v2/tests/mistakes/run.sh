#!/bin/sh
# Regenerates every NN-*.out: `axiom check` on each mistake, colourless, on a
# fixed day, under a 60 second timeout. Single files are checked as one-file
# projects; directories are checked as projects (`-C`). The last line of each
# .out records the exit status (1 = errors, 101 = a panic, 124 = a hang).
#
#   sh tests/mistakes/run.sh            all of them
#   sh tests/mistakes/run.sh 07 14      only the cases whose name starts 07 or 14
set -u
cd "$(dirname "$0")"
axiom=../../target/release/axiom
[ -x "$axiom" ] || (cd ../.. && cargo build --release -q)
today=2026-06-30

case_out() {
    name=$1
    shift
    timeout 60 "$axiom" "$@" --today "$today" --color never > "$name.out" 2>&1
    status=$?
    printf '[exit status %s]\n' "$status" >> "$name.out"
}

want() {
    [ $# -eq 0 ] && return 0
    for n in "$@"; do
        case "$name" in "$n"*) return 0 ;; esac
    done
    return 1
}

for f in [0-9][0-9]-*.ax; do
    [ -e "$f" ] || continue
    name=${f%.ax}
    want "$@" || continue
    case_out "$name" check "$f"
done
for d in [0-9][0-9]-*/; do
    [ -d "$d" ] || continue
    name=${d%/}
    want "$@" || continue
    case_out "$name" check -C "$d"
done

# Robustness probes: inputs that are not plausible mistakes but must never
# panic or hang. Named robust/rNN-*.ax; `want` matches on `robust/rNN`.
for f in robust/r[0-9][0-9]-*.ax; do
    [ -e "$f" ] || continue
    name=${f%.ax}
    want "$@" || continue
    case_out "$name" check "$f"
done
