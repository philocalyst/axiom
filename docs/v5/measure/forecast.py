#!/usr/bin/env python3
"""The forecast against the fold: a book's forecast from one day, and the same book with what it forecast written down, from a later one.

    forecast.py gen DIR N [SEED]                    write N projects of promises into DIR (splits.py's `promises` kind)
    forecast.py cli BINARY DIR [JOBS] [UNTIL]       layer A: through the CLI of any build
    forecast.py dump BINARY DIR [JOBS] [UNTIL]      layer B: through `forecasts/main.rs`, which asks the engine itself
    forecast.py build TREE OUT                      build the dump of layer B against the crates of TREE
    forecast.py mutate TREE WORK DIR [N,M..]        the mutants of the code under test: each must be caught by layer B

What it is for. Lane K5c makes the forecast the fold, continued past today: the report no longer has a loop of its own that
decides which occurrences fall due, numbers them and applies them to a second ledger. The strongest thing that can be said of such
a forecast is that **it is what the fold would have done had the book said so**. A book B is run on `today`, and its forecast to
`UNTIL` lists the occurrences the contracts promise after `today`. The book B' is B with a line `DAY name` for each of them (on the
due day, after every other line of the day, in the order the forecast made them), and B' is run on `UNTIL`, so that what the
forecast posted is now history. The two must agree on every occurrence they share and on everything the fold makes of them.

    layer A   (any CLI binary; the baseline's too) the net worth at every month end the forecast shows is the net worth `balance
              --at DAY` shows for B' on `UNTIL`. It is what the user sees, and it measures how often the second driver of
              the old forecast disagrees with the fold.
    layer B   (the dump, built against a tree that has `Ledger::promise`) the engine asked directly: the forecast ledger (the
              fold to `today`, `promise`, advanced through each month end) against the fold of B' on `UNTIL`:
                * every occurrence the forecast posted is the occurrence a line of B' kept: same contract, schedule, ordinal, due
                  day, and the same flows (ends, amounts, day, ordinal);
                * every holding, at every month end, is the same: place, commodity, quantity, basis, the parcels;
                * every effect the laws recorded after `today` is the same (the tallies, the owed, the penalties);
                * every violation after `today` is the same; and the occurrences the monitor missed are the same.
              A book that already wrote occurrences after `today` (some projects do) is the test that a line written ahead is not
              promised again.

The books are `splits.py`'s promises (`gen`): monthly, twice monthly and weekly contracts, a standing `buy`, a loan, legs, items,
inputs, `about`, escalation, `covers`, `prorated`, `due .. else`, a deposit, and kept occurrences before `today`. A forecast that
is not about a promise (a habit found in the journal) is left out and counted: it is not in B'.
"""
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from collections import Counter
from concurrent.futures import ThreadPoolExecutor

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

TODAY = "2026-06-30"
UNTIL = "2027-01-31"


def sh(binary, args, timeout=120):
    done = subprocess.run([binary] + args, capture_output=True, text=True, timeout=timeout)
    return done.stdout + done.stderr


def report_of(output):
    """The report a command printed as JSON: its last line that is a report, and the diagnostics before it."""
    report, diagnostics = None, []
    for line in output.splitlines():
        if line.startswith('{"title"'):
            report = json.loads(line)
        elif line.startswith('{"code"'):
            diagnostics.append(json.loads(line)["code"])
    return report, diagnostics


def section(report, heading=None, first=None):
    for found in report["sections"]:
        if heading and found["heading"] == heading:
            return found
        if first and found["columns"][0]["title"] == first:
            return found
    return None


def cell(c):
    return c.get("value", "")


# ─── layer A: the CLI ───────────────────────────────────────────────────────────────────────────────────────────────

def forecast_of(binary, path, until):
    out = sh(binary, ["forecast", "-C", path, "--today", TODAY, "--until", until, "--paths", "1", "--json"])
    return report_of(out)


def written_lines(rows):
    """`(day, line)` for each occurrence the forecast lists, in the order it lists them."""
    lines = []
    for row in rows:
        name = cell(row["cells"][0]).split(" · ")[0]
        lines.append((cell(row["cells"][3]), f"{cell(row['cells'][3])} {name}"))
    return lines


def codes_of(binary, path, until):
    out = sh(binary, ["check", "-C", path, "--today", until, "--json"])
    return Counter(report_of(out)[1])


def net_worth(binary, path, day):
    """What the book says it is worth on `day`, folded through that day: the journal after it is written nowhere."""
    balance, _ = report_of(sh(binary, ["balance", "-C", path, "--today", day, "--json"]))
    net = section(balance, first="Net worth") if balance else None
    return cell(net["rows"][-1]["cells"][1]) if net else "?"


def project_cli(binary, source, until, work):
    """What layer A says of one project: ok, a list of differences, or why it was left out. The forecast's net worth at each
    month end is held to the net worth of the book that wrote down every occurrence due by then, folded to that day."""
    path = os.path.join(source, "main.ax")
    forecast, _ = forecast_of(binary, path, until)
    if forecast is None:
        return "no-report", []
    recurs = section(forecast, "What recurs")
    if recurs and recurs["rows"]:
        return "habit", []
    occurrences = section(forecast, "Contract occurrences")
    if occurrences is None or not occurrences["rows"]:
        return "no-occurrences", []
    outlook = section(forecast, "Liquid net worth")
    lines = written_lines(occurrences["rows"])
    os.makedirs(work, exist_ok=True)
    original = open(path).read().rstrip("\n") + "\n"
    differences = []
    for row in outlook["rows"]:
        day, worth = cell(row["cells"][0]), cell(row["cells"][2])
        written = os.path.join(work, "main.ax")
        open(written, "w").write(original + "".join(text + "\n" for due, text in lines if due <= day))
        found = net_worth(binary, written, day)
        if found != worth:
            differences.append(f"{day}: forecast {worth}, history {found}")
    return ("ok" if not differences else "differ"), differences


def cli(binary, directory, jobs=4, until=UNTIL):
    projects = sorted(p for p in os.listdir(directory) if re.fullmatch(r"p\d+", p))
    results = {}
    with tempfile.TemporaryDirectory() as scratch, ThreadPoolExecutor(jobs) as pool:
        def one(name):
            return name, project_cli(binary, os.path.join(directory, name), until, os.path.join(scratch, name))
        for name, result in pool.map(one, projects):
            results[name] = result
    tally = Counter(status for status, _ in results.values())
    print(f"{len(results)} projects: " + ", ".join(f"{k} {v}" for k, v in sorted(tally.items())))
    shown = 0
    for name, (status, details) in sorted(results.items()):
        if status == "differ" and shown < 12:
            shown += 1
            print(f"  {name}: " + "; ".join(details[:3]))
    return 1 if tally["differ"] else 0


# ─── layer B: the engine ────────────────────────────────────────────────────────────────────────────────────────────

def build(tree, out):
    """Builds `forecasts/main.rs` against the crates of TREE into OUT/forecasts."""
    os.makedirs(out, exist_ok=True)
    src = os.path.join(out, "src")
    os.makedirs(src, exist_ok=True)
    shutil.copy(os.path.join(HERE, "forecasts", "main.rs"), os.path.join(src, "main.rs"))
    deps = "\n".join(f'axiom-{name} = {{ path = "{os.path.abspath(tree)}/crates/{name}" }}' for name in
                     ["core", "syntax", "model", "engine", "systems"])
    open(os.path.join(out, "Cargo.toml"), "w").write(
        f'[package]\nname = "forecasts"\nversion = "0.0.0"\nedition = "2024"\n\n[workspace]\n\n[dependencies]\n{deps}\n')
    target = os.environ.get("CARGO_TARGET_DIR") or os.path.join(out, "target")
    done = subprocess.run(["cargo", "build", "--release", "-q"], cwd=out, env={**os.environ, "CARGO_TARGET_DIR": target},
                          capture_output=True, text=True)
    if done.returncode:
        print(done.stderr[-3000:])
        raise SystemExit(1)
    return os.path.join(target, "release", "forecasts")


def dump(binary, mode, path, today, until):
    return sh(binary, [mode, path, today, until])


def occurrences_of(text):
    return [line.split(" ", 1)[1] for line in text.splitlines() if line.startswith("planned ")]


def asked(text):
    """How many lines of each kind a dump holds: what a comparison of two dumps was about."""
    return Counter(line.split(" ", 1)[0] for line in text.splitlines())


COVERED = Counter()


def project_dump(binary, source, until, work):
    path = os.path.join(source, "main.ax")
    forecast = dump(binary, "forecast", path, TODAY, until)
    if forecast.startswith("error"):
        return "no-dump", [forecast[:200]]
    names = []
    for line in forecast.splitlines():
        if line.startswith("planned "):
            fields = line.split()
            names.append((fields[1], fields[2]))
    if not names:
        return "no-occurrences", []
    lines = [f"{due} {name}" for name, due in names]
    os.makedirs(work, exist_ok=True)
    written = os.path.join(work, "main.ax")
    open(written, "w").write(open(path).read().rstrip("\n") + "\n" + "\n".join(lines) + "\n")
    history = dump(binary, "history", written, TODAY, until)
    if history.startswith("error"):
        return "no-dump", [history[:200]]
    before, after = Counter(matching(forecast)), Counter(matching(history))
    if after - before:
        return "unwritable", sorted(after - before)
    status = compare(forecast, history)
    if status[0] == "ok":
        COVERED.update(asked(forecast))
    return status


def matching(text):
    """The diagnostics about a line meeting its occurrence: a line the book had to say that the forecast's own run had not."""
    return [line.split()[1] for line in text.splitlines()
            if line.startswith("diagnostic ") and line.split()[1].startswith(("contract-occurrence", "ambiguous-contract"))]


def compare(forecast, history):
    """Every kind of line the dump prints, forecast against history: the ones the forecast owns must be equal."""
    def kinds(text):
        found = {}
        for line in text.splitlines():
            kind = line.split(" ", 1)[0]
            found.setdefault(kind, []).append(line)
        return found
    f, h = kinds(forecast), kinds(history)
    differences = []
    planned = sorted(set(f.get("planned", [])))
    kept = sorted(set(h.get("kept", [])))
    shared = [line.replace("planned ", "", 1) for line in planned]
    wanted = [line.replace("kept ", "", 1) for line in kept]
    missing = [line for line in shared if line not in wanted]
    if missing:
        differences.append(f"planned but not kept: {missing[0]}")
    for kind in ("holding", "effect", "violation", "missed"):
        if sorted(f.get(kind, [])) != sorted(h.get(kind, [])):
            only_f = [line for line in f.get(kind, []) if line not in h.get(kind, [])]
            only_h = [line for line in h.get(kind, []) if line not in f.get(kind, [])]
            differences.append(f"{kind}: forecast {only_f[:1]}, history {only_h[:1]}")
    return ("ok" if not differences else "differ"), differences


def dump_all(binary, directory, jobs=4, until=UNTIL):
    projects = sorted(p for p in os.listdir(directory) if re.fullmatch(r"p\d+", p))
    results = {}
    with tempfile.TemporaryDirectory() as scratch, ThreadPoolExecutor(jobs) as pool:
        def one(name):
            return name, project_dump(binary, os.path.join(directory, name), until, os.path.join(scratch, name))
        for name, result in pool.map(one, projects):
            results[name] = result
    tally = Counter(status for status, _ in results.values())
    print(f"{len(results)} projects: " + ", ".join(f"{k} {v}" for k, v in sorted(tally.items())))
    print("compared, forecast against history, in the projects that agree: " +
          ", ".join(f"{COVERED[k]} {k}" for k in ("planned", "holding", "effect", "violation", "gain", "missed")))
    shown = 0
    for name, (status, details) in sorted(results.items()):
        if status in ("differ", "no-dump") and shown < 12:
            shown += 1
            print(f"  {name} {status}: " + "; ".join(details[:3]))
    return 1 if tally["differ"] or tally["no-dump"] else 0


# What the laws of a book make of the flows of a forecast: a penalty on an outflow of the checking account, one on a small
# inflow into savings, and a year that closes inside the forecast's horizon. The laws that close a period begin with the
# first real fact of a book (an opening is not one), which the one flow below is, so that a book that writes its occurrences
# down has the same periods as one that does not.
LAWS = """
2026-01-03 checking -> shop 1 USD
entity treasury : government
law out-fee
  on out
  when from is checking
  owe 3 USD to treasury by date(2027, 6, 1) as out-fee
law in-fee
  on in
  when to is savings
  require amount < 1_000 USD else owe 9 USD to treasury by date(2027, 7, 1) as in-fee
law year-note
  each year
  owe 11 USD to treasury by date(year + 1, 3, 1) as year-note
"""


def gen(directory, count, seed):
    """Projects of promises, a half of them with the laws above, so that the laws the fold knows are asked of the forecast."""
    import splits
    splits.gen(directory, count, seed, "promises")
    for index in range(count):
        if index % 2:
            with open(os.path.join(directory, f"p{index:04d}", "main.ax"), "a") as out:
                out.write(LAWS)


def main(argv):
    if len(argv) >= 3 and argv[1] == "gen":
        gen(argv[2], int(argv[3]), int(argv[4]) if len(argv) > 4 else 1)
        return 0
    if len(argv) >= 4 and argv[1] == "cli":
        return cli(argv[2], argv[3], int(argv[4]) if len(argv) > 4 else 4, argv[5] if len(argv) > 5 else UNTIL)
    if len(argv) >= 4 and argv[1] == "dump":
        return dump_all(argv[2], argv[3], int(argv[4]) if len(argv) > 4 else 4, argv[5] if len(argv) > 5 else UNTIL)
    if len(argv) >= 4 and argv[1] == "build":
        print(build(argv[2], argv[3]))
        return 0
    print(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
