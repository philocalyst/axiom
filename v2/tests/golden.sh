#!/bin/sh
# Regenerates the golden outputs: every command on every example, colourless,
# on a fixed day. Review `git diff tests/golden` after any change: it is the
# behaviour the change made.
set -eu
cd "$(dirname "$0")/.."
cargo build --release -q
axiom=target/release/axiom
out=tests/golden
today=2026-03-31

run() {
    name=$1
    shift
    "$axiom" "$@" --today "$today" --color never > "$out/$name.txt" 2>&1 || true
}

run first-steps-check   check examples/01-first-steps.ax
"$axiom" check examples/03-violations.ax --today 2026-12-31 --color never > "$out/violations-check.txt" 2>&1 || true
household="-C examples/02-household"
run household-check     check $household
run household-balance   balance $household
run household-value     balance --value $household
run household-register  register checking $household
run household-flow      flow $household
run household-available available $household
run household-budget    budget 2026-02 $household
run household-tax       tax 2026 $household
run household-lots      lots $household
run household-forecast  forecast --paths 200 $household
run household-why-law   why budget $household
run household-why-place why college $household
run household-why-code  why '#check-1041' $household
