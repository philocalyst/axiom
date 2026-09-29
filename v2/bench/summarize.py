#!/usr/bin/env python3
"""Turns bench results into Markdown tables.

    summarize.py [results.tsv]        default: $AXIOM_BENCH_DIR/results.tsv

Prints, by command and scale: wall seconds, peak RSS, throughput (flows per second of
wall time), CPU seconds (user + sys) as a share of wall (a parallelism gauge), and the
scaling exponent between neighbouring scales (log-log slope of wall time against
flows: 1.0 is linear, above 1.15 is worth a look). If a command appears several times
for one scale the fastest is used.
"""

import math
import os
import sys
from collections import OrderedDict

SCALES = ["10k", "100k", "1m", "5m"]


def load(path):
    rows = OrderedDict()
    with open(path) as f:
        header = f.readline().rstrip("\n").split("\t")
        for line in f:
            r = dict(zip(header, line.rstrip("\n").split("\t")))
            key = (r["command"], r["scale"])
            wall = float(r["wall_s"])
            if key not in rows or wall < rows[key]["wall"]:
                rows[key] = dict(wall=wall, cpu=float(r["user_s"]) + float(r["sys_s"]), sys=float(r["sys_s"]),
                                 rss=int(r["maxrss_kb"]) / 1024, exit=r["exit"], flows=int(r["flows"]),
                                 faults=int(r.get("minflt", 0) or 0))
    return rows


def table(rows, title, cell, commands, scales):
    out = [f"**{title}**", "", "| command | " + " | ".join(scales) + " |", "|---|" + "---|" * len(scales)]
    for c in commands:
        out.append(f"| {c} | " + " | ".join(cell(rows.get((c, s))) if rows.get((c, s)) else "" for s in scales) + " |")
    return "\n".join(out)


def main():
    path = sys.argv[1] if len(sys.argv) > 1 else os.path.join(
        os.environ.get("AXIOM_BENCH_DIR", os.path.join(os.environ.get("TMPDIR", "/tmp"), "axiom-bench")), "results.tsv")
    rows = load(path)
    commands = list(OrderedDict.fromkeys(c for c, _ in rows))
    scales = [s for s in SCALES if any(k[1] == s for k in rows)]
    print(table(rows, "Wall time, seconds (fastest run)", lambda r: f"{r['wall']:.3f}" if r["wall"] < 10 else f"{r['wall']:.1f}", commands, scales))
    print()
    print(table(rows, "Peak RSS, MB", lambda r: f"{r['rss']:,.0f}", commands, scales))
    print()
    print(table(rows, "Throughput, thousand flows per second of wall time", lambda r: f"{r['flows'] / r['wall'] / 1000:,.0f}", commands, scales))
    print()
    print(table(rows, "CPU (user+sys) ÷ wall: cores kept busy", lambda r: f"{r['cpu'] / r['wall']:.1f}", commands, scales))
    print()
    print(table(rows, "Minor page faults, thousands (sys time is mostly these)", lambda r: f"{r['faults'] / 1000:,.0f}", commands, scales))
    print()
    print("**Scaling exponent** between neighbouring scales (log-log slope of wall time; 1.0 = linear)\n")
    pairs = list(zip(scales, scales[1:]))
    print("| command | " + " | ".join(f"{a}→{b}" for a, b in pairs) + " |")
    print("|---|" + "---|" * len(pairs))
    for c in commands:
        cells = []
        for a, b in pairs:
            ra, rb = rows.get((c, a)), rows.get((c, b))
            if ra and rb and ra["wall"] > 0 and rb["wall"] > 0:
                k = math.log(rb["wall"] / ra["wall"]) / math.log(rb["flows"] / ra["flows"])
                cells.append(f"{k:.2f}")
            else:
                cells.append("")
        print(f"| {c} | " + " | ".join(cells) + " |")


if __name__ == "__main__":
    main()
