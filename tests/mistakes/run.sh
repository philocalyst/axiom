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

here=$(pwd -P)

case_out() {
    name=$1
    shift
    timeout 60 "$axiom" "$@" --today "$today" --color never > "$name.out" 2>&1
    status=$?
    # absolute paths in messages (`no axiom.ax in /home/…`) would make the output machine-specific
    sed "s|$here|<mistakes>|g" "$name.out" > "$name.out.tmp" && mv "$name.out.tmp" "$name.out"
    printf '[exit status %s]\n' "$status" >> "$name.out"
}

want() {
    [ $# -eq 0 ] && return 0
    for n in "$@"; do
        case "$name" in "$n"*) return 0 ;; esac
    done
    return 1
}

for f in [0-9][0-9]-*.ax [0-9][0-9][0-9]-*.ax; do
    [ -e "$f" ] || continue
    name=${f%.ax}
    want "$@" || continue
    case_out "$name" check "$f"
done
for d in [0-9][0-9]-*/ [0-9][0-9][0-9]-*/; do
    [ -d "$d" ] || continue
    name=${d%/}
    want "$@" || continue
    case_out "$name" check -C "$d"
done

# Robustness probes: inputs that are not plausible mistakes but must never
# panic or hang. Named robust/rNN-*.ax; `want` matches on `robust/rNN`.
# The long-line probe is 2 MB of comment, so it is written here, not kept.
[ -e robust/r10-long-line.ax ] || {
    printf 'base USD\nuse std\n\naccount assets/checking : bank\naccount expenses/food\naccount equity/opening\n\n2025-12-31 equity/opening -> checking 1_000 USD\n// '
    head -c 2000000 /dev/zero | tr '\0' x
    printf '\n2026-01-08 checking -> food 84.20 USD\n'
} > robust/r10-long-line.ax
for f in robust/r[0-9][0-9]-*.ax; do
    [ -e "$f" ] || continue
    name=${f%.ax}
    want "$@" || continue
    case_out "$name" check "$f"
done
