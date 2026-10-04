#!/bin/sh
# usage: run.sh AXIOM-BINARY OUTDIR
#
# Runs the CLI over every case here and writes what it says to OUTDIR, one file per case and command.
# `cases/` holds mistakes and odd shapes (checked for the diagnostics they raise); `cases3/` one book per way a declared
# line can be wrong (written by `declared.py`); `cases2/` holds valid projects, read by the commands that show what was
# lowered. Run it with the binary of the commit before a refactor and the binary after, then compare.sh the two
# directories: every difference is a change in behaviour.
bin=$1
out=$2
here=$(cd "$(dirname "$0")" && pwd)
rm -rf "$out"
mkdir -p "$out"
cd "$here/cases" || exit 1
for f in *.ax; do
    timeout 60 "$bin" check "$f" --today 2026-06-30 --color never > "$out/${f%.ax}.out" 2>&1
done
for d in */; do
    [ -d "$d" ] || continue
    timeout 60 "$bin" check -C "$d" --today 2026-06-30 --color never > "$out/${d%/}.out" 2>&1
done
cd "$here/cases3" || exit 1
for f in *.ax; do
    timeout 60 "$bin" check "$f" --today 2026-06-30 --color never > "$out/${f%.ax}.out" 2>&1
done
cd "$here/cases2" || exit 1
for f in *.ax; do
    for c in check balance lots contracts claims gains available flow limits budget; do
        timeout 60 "$bin" "$c" -C "$f" --today 2026-06-30 --color never > "$out/${f%.ax}.$c.out" 2>&1
    done
    timeout 60 "$bin" budget 2026-03 -C "$f" --today 2026-06-30 --color never > "$out/${f%.ax}.budget-march.out" 2>&1
    timeout 60 "$bin" budget 2026 -C "$f" --today 2026-06-30 --color never > "$out/${f%.ax}.budget-year.out" 2>&1
done
exit 0
