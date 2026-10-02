#!/bin/sh
# usage: compare.sh BASELINE-BINARY NEW-BINARY
#
# Runs every case with both binaries and prints the cases whose output differs. Silence is no change.
#
# A binary older than 5271942 (the catalog's wording) differs on the amb-*, dup-*, occ*, sys*, unk-base and
# g-*-twice cases, and on nothing else: that commit is the only one of lane K0a that changed what is said.
here=$(cd "$(dirname "$0")" && pwd)
tmp=$(mktemp -d)
sh "$here/run.sh" "$1" "$tmp/before"
sh "$here/run.sh" "$2" "$tmp/after"
diff -rq "$tmp/before" "$tmp/after"
status=$?
rm -rf "$tmp"
exit $status
