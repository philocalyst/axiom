#!/bin/sh
# Checks the copies in this folder against what they copy, with the build of the axiom named by $AXIOM (default: the release
# build of this tree). Silence on success; a diff, and a failing exit, where a copy prints something other than its original.
#
#   05-family and 07-landlord, their contracts written by the kinds of kinds.ax, on the day each README uses: the same
#   `check` (its diagnostics, whose line numbers and excerpts name another file), balances, what is available, limits, claims,
#   tax and forecast. The household's book has none of the employer's half of a payroll tax, or anything of what a kind of
#   lease or management says of the other side.
#
#   employer/, the same employment from the employer's side, against the same contracts with the employer's half written by
#   hand as a leg of the contract: the same everything, and the half is in it (`payroll` is 3,557.28 USD lower than the
#   gross it paid).
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../../.." && pwd)
axiom=${AXIOM:-$root/target/release/axiom}
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
status=0

# `check` says where each diagnostic is, in lines of a file the copy writes differently: what it says, and how many.
view() {
    dir=$1 day=$2 year=$3
    "$axiom" check -C "$dir" --today "$day" --color never 2>&1 | grep -E '^(error|warning|note)\[|^[✓✗]' || true
    for command in balance available limits claims "tax $year" "forecast --paths 20"; do
        echo "== $command"
        "$axiom" $command -C "$dir" --today "$day" --color never 2>&1 || true
    done
}

household() {
    name=$1 day=$2 year=$3
    cp -R "$root/examples/$name" "$tmp/$name.original"
    cp -R "$root/examples/$name" "$tmp/$name.kinds"
    cp "$here/$name.contracts.ax" "$tmp/$name.kinds/contracts.ax"
    cp "$here/kinds.ax" "$tmp/$name.kinds/kinds.ax"
    view "$tmp/$name.original" "$day" "$year" > "$tmp/$name.original.txt"
    view "$tmp/$name.kinds" "$day" "$year" > "$tmp/$name.kinds.txt"
    if diff "$tmp/$name.original.txt" "$tmp/$name.kinds.txt" > "$tmp/$name.diff"; then
        echo "$name: the copy in the kinds' spelling prints what the original does ($(wc -l < "$tmp/$name.original.txt") lines)"
    else
        echo "$name: the copy prints something else"; head -20 "$tmp/$name.diff"; status=1
    fi
}

household 05-family 2026-04-16 2025
household 07-landlord 2026-04-16 2025

cmp "$here/kinds.ax" "$here/employer/kinds.ax" || { echo "employer/kinds.ax is a copy of kinds.ax and is not"; status=1; }
cp -R "$here/employer" "$tmp/employer.kinds"
cp -R "$here/employer" "$tmp/employer.hand"
cp "$here/employer/by-hand.contracts.txt" "$tmp/employer.hand/contracts.ax"
rm "$tmp/employer.kinds/by-hand.contracts.txt" "$tmp/employer.hand/by-hand.contracts.txt"
rm "$tmp/employer.hand/kinds.ax"
view "$tmp/employer.kinds" 2026-04-16 2026 > "$tmp/employer.kinds.txt"
view "$tmp/employer.hand" 2026-04-16 2026 > "$tmp/employer.hand.txt"
if diff "$tmp/employer.kinds.txt" "$tmp/employer.hand.txt" > "$tmp/employer.diff"; then
    echo "employer: the kind prints what the legs written by hand do ($(wc -l < "$tmp/employer.kinds.txt") lines)"
    "$axiom" balance -C "$tmp/employer.kinds" --today 2026-04-16 --color never | grep payroll
else
    echo "employer: the kind prints something else"; head -20 "$tmp/employer.diff"; status=1
fi
exit $status
