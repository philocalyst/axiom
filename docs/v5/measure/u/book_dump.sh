#!/bin/sh
# usage: book_dump.sh OUTDIR [EXTRA-PROJECT ...]
#
# Builds `crates/session/examples/book_dump.rs` from this tree and writes the Book of every project we have, one file
# each, into OUTDIR: the examples (and their v4 text in tests/v4-syntax), the mistakes corpus, the differential
# harness's cases (cases, cases2, cases3) and any EXTRA-PROJECT (a generated bench project, say). Run it on the commit
# before a lowering change and on the commit after, then `diff -r` the two directories: lane U's C1 to C3 must leave
# every Book as it was, and where one changes on purpose the commit says which and why. With DUMP set to another
# tree's book_dump binary (a baseline's), that binary reads this tree's projects, so both dumps name the same files.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../../../.." && pwd)
out=$1
shift
if [ -z "${DUMP:-}" ]; then
    (cd "$root" && CARGO_INCREMENTAL=0 cargo build --release --offline -q -p axiom-session --example book_dump 2>/dev/null)
fi
dump=${DUMP:-$root/target/release/examples/book_dump}
rm -rf "$out"
mkdir -p "$out"
one() {
    name=$(printf '%s' "$1" | sed "s|^$root/||; s|/*$||; s|/|__|g")
    timeout 120 "$dump" "$1" > "$out/$name.txt" 2>&1 || echo "failed: $1" >> "$out/FAILED"
}
for project in "$root"/examples/*.ax "$root"/examples/*/ "$root"/examples/explore-v5/*/ "$root"/tests/v4-syntax/examples/*.ax \
    "$root"/tests/v4-syntax/examples/*/ "$root"/tests/mistakes/*.ax "$root"/tests/mistakes/*/ \
    "$root"/docs/v5/measure/diff/cases/*.ax "$root"/docs/v5/measure/diff/cases/*/ "$root"/docs/v5/measure/diff/cases2/*.ax \
    "$root"/docs/v5/measure/diff/cases3/*.ax "$@"; do
    [ -e "$project" ] || continue
    case "$project" in */explore-v5/|*/robust/) continue ;; esac
    one "$project"
done
ls "$out" | wc -l
