#!/usr/bin/env python3
"""Counts what a build says when two sources disagree about a flow's purpose, and which two.

    purposes.py BINARY [PROJECT...]     one line per project: its `purpose-disagreement` diagnostics, by the pair of
                                        sources that disagree (the examples by default, on the day the goldens use)

What it is for. Lane K6 replaces the three functions that infer a purpose (`endpoint_purpose`, `taken_purpose`,
`infer_for_flow`) with one ranking (LANGUAGE §2: written, promise, party, party kind, commodity kind, account kind,
first match winning). A flow whose sources disagree is dropped from the book today, so the count of these
diagnostics is the count of flows the ranking can keep. `K6-map.md` section 2 has the numbers this prints for the
baseline; `purposes.py NEW` shows what is left.
"""
import collections
import json
import os
import subprocess
import sys

TODAY = "2026-04-16"
SOURCES = [
    ("the written purpose", "written"),
    ("contract `", "promise"),
    ("party kind", "party-kind"),
    ("party `", "party"),
    ("commodity kind", "commodity-kind"),
    ("account kind", "account-kind"),
    ("the derived flow", "derived"),
]
SAYS = "these sources classify the same flow differently"


def source(label):
    return next((name for start, name in SOURCES if label.startswith(start)), "?")


def disagreements(binary, project):
    run = subprocess.run(
        [binary, "check", "-C", project, "--today", TODAY, "--json", "--all"], capture_output=True, text=True
    )
    pairs = collections.Counter()
    for line in run.stdout.splitlines():
        try:
            found = json.loads(line)
        except ValueError:
            continue
        if found.get("code") == "purpose-disagreement":
            says = [label["text"] for label in found["labels"] if label["text"] != SAYS]
            pairs[tuple(sorted(source(text) for text in says))] += 1
    return pairs


def main():
    binary, projects = sys.argv[1], sys.argv[2:]
    if not projects:
        projects = sorted(
            os.path.join("examples", name) for name in os.listdir("examples") if name[:2].isdigit()
        )
    total = collections.Counter()
    for project in projects:
        pairs = disagreements(binary, project)
        total.update(pairs)
        print(f"{project}: {sum(pairs.values())}", dict(pairs))
    print("total", sum(total.values()), dict(total))


if __name__ == "__main__":
    main()
