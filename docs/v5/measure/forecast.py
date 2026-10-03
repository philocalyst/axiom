#!/usr/bin/env python3
"""The forecast against the fold: a book's forecast from one day, and the same book with what it forecast written down, from a later one.

    forecast.py gen DIR N [SEED]                    write N projects of promises into DIR (splits.py's `promises` kind)
    forecast.py cli BINARY DIR [JOBS] [UNTIL]       layer A: through the CLI of any build
    forecast.py dump BINARY DIR [JOBS] [UNTIL]      layer B: through `forecasts/main.rs`, which asks the engine itself
    forecast.py against BASE NEW DIR                layer C: the occurrences each forecast lists, one build's against another's
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


def cli(binary, directory, jobs=4, until=UNTIL, quiet=False):
    projects = sorted(p for p in os.listdir(directory) if re.fullmatch(r"p\d+", p))
    results = {}
    with tempfile.TemporaryDirectory() as scratch, ThreadPoolExecutor(jobs) as pool:
        def one(name):
            return name, project_cli(binary, os.path.join(directory, name), until, os.path.join(scratch, name))
        for name, result in pool.map(one, projects):
            results[name] = result
    tally = Counter(status for status, _ in results.values())
    if not quiet:
        print(f"{len(results)} projects: " + ", ".join(f"{k} {v}" for k, v in sorted(tally.items())))
        shown = 0
        for name, (status, details) in sorted(results.items()):
            if status == "differ" and shown < 12:
                shown += 1
                print(f"  {name}: " + "; ".join(details[:3]))
    return 1 if tally["differ"] else 0


def listed(binary, directory, until=UNTIL, jobs=4):
    """The occurrences the forecast of each project lists, as a build says them: the rows of its `Contract occurrences`."""
    projects = sorted(p for p in os.listdir(directory) if re.fullmatch(r"p\d+", p))

    def one(name):
        forecast, _ = forecast_of(binary, os.path.join(directory, name, "main.ax"), until)
        found = section(forecast, "Contract occurrences") if forecast else None
        return name, [[cell(c) for c in row["cells"]] for row in found["rows"]] if found else []

    with ThreadPoolExecutor(jobs) as pool:
        return dict(pool.map(one, projects))


def against(base, new, directory, until=UNTIL):
    """layer C: the occurrences the forecast of a build lists, against another build's. A forecast that history cannot
    contradict (history is written from what the forecast lists) can still leave an occurrence out; the driver it replaced, or
    the same build without a mutant, is the reference for that."""
    before, after = listed(base, directory, until), listed(new, directory, until)
    differ = sorted(name for name in before if before[name] != after[name])
    count = sum(1 for rows in before.values() if rows)
    print(f"{len(before)} projects, {count} list occurrences, {len(differ)} list others: {differ[:12]}")
    return 1 if differ else 0


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


def dump_all(binary, directory, jobs=4, until=UNTIL, quiet=False):
    projects = sorted(p for p in os.listdir(directory) if re.fullmatch(r"p\d+", p))
    results = {}
    with tempfile.TemporaryDirectory() as scratch, ThreadPoolExecutor(jobs) as pool:
        def one(name):
            return name, project_dump(binary, os.path.join(directory, name), until, os.path.join(scratch, name))
        for name, result in pool.map(one, projects):
            results[name] = result
    tally = Counter(status for status, _ in results.values())
    if not quiet:
        print(f"{len(results)} projects: " + ", ".join(f"{k} {v}" for k, v in sorted(tally.items())))
        print("compared, forecast against history, in the projects that agree: " +
              ", ".join(f"{COVERED[k]} {k}" for k in ("planned", "holding", "effect", "violation", "gain", "missed")))
        shown = 0
        for name, (status, details) in sorted(results.items()):
            if status in ("differ", "no-dump") and shown < 12:
                shown += 1
                print(f"  {name} {status}: " + "; ".join(details[:3]))
    return 1 if tally["differ"] or tally["no-dump"] or tally["unwritable"] else 0


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


# A law that reads the balance of the account when a month closes, so that the order of what a forecast posts on a month's
# last day and what the closing reads is something the comparison can tell.
MONTH_END = """account checking : bank
  law month-end-fee
    each month
    owe self.balance * 1% to treasury by date(year + 1, 2, 1) as balance-fee
"""


def gen(directory, count, seed):
    """Projects of promises, a half of them with the laws above, so that the laws the fold knows are asked of the forecast."""
    import splits
    splits.gen(directory, count, seed, "promises")
    for index in range(count):
        if index % 2:
            path = os.path.join(directory, f"p{index:04d}", "main.ax")
            text = open(path).read()
            assert text.count("account checking : bank\n") == 1
            with open(path, "w") as out:
                out.write(text.replace("account checking : bank\n", MONTH_END) + LAWS)


# ─── mutants ────────────────────────────────────────────────────────────────────────────────────────────────────────

PROMISING = "crates/engine/src/promising.rs"
LEDGER = "crates/engine/src/ledger.rs"
TRACE = "crates/report/src/forecast/trace.rs"
FORECAST = "crates/report/src/forecast.rs"
CLAIMS = "crates/engine/src/claims.rs"
PLAN = "crates/engine/src/plan.rs"
MONITOR = "crates/engine/src/monitor.rs"
LOWER = "crates/model/src/lower/contracts.rs"
BOOK = "crates/model/src/book.rs"

# (file, the text, what it becomes, what it is, which layer is to say). Each text occurs once in its file.
MUTANTS = [
    (PROMISING, "after.add_days(1)", "after", "a forecast promises the day it stands on too", "engine"),
    (PROMISING, "after.add_days(1)", "after.add_days(2)", "a forecast leaves out the day after it", "engine"),
    (PROMISING, "binary_search(&(stream_key(*contract, *schedule), due)).is_err()",
     "binary_search(&(stream_key(*contract, *schedule), due)).is_ok()", "it promises only what a line wrote", "engine"),
    (PROMISING, "Some((stream_key(txn.contract?, written.schedule), written.due))",
     "Some((stream_key(txn.contract?, ScheduleKind::Regular), written.due))", "a written standing day is looked up as a regular one", "engine"),
    (PROMISING, "ordinal: ahead.residual.ordinal(), due }", "ordinal: ahead.residual.ordinal() + 1, due }", "an ordinal one too many", "engine"),
    (PROMISING, "        if made.is_ok() {\n            self.settle(occurrence);",
     "        if made.is_err() {\n            self.settle(occurrence);", "the monitor is told of a failure and not of a success", "engine"),
    (PROMISING, "self.post_occurrence(occurrence, None, self.clock.day)",
     "self.post_occurrence(occurrence, None, occurrence.due.add_days(1))", "an occurrence is posted a day late", "engine"),
    (PROMISING, "let mut written: Vec<_> = kept.collect();", "let mut written: Vec<_> = kept.take(0).collect();",
     "what a line wrote ahead of today is promised again", "engine"),
    (PROMISING, "falling.push(Reverse((due, at as u32)));", "falling.push(Reverse((due, u32::MAX - at as u32)));",
     "streams due on one day come in the wrong order", "engine"),
    (PROMISING, "self.promising.next_due().filter(|&due| due <= day.min(self.horizon))?;",
     "self.promising.next_due().filter(|&due| due <= day)?;", "a step goes past the horizon", "tests"),
    (LEDGER, "(Some(fact), Some(promised)) if promised.at() < fact.at() => Some(promised),",
     "(Some(fact), Some(promised)) if true => Some(promised),", "a promise comes before the journal's facts of earlier days", "engine"),
    (LEDGER, "Upcoming::Promised(Moment::after_flows(due))", "Upcoming::Promised(Moment::end_of(due))",
     "a promise falls due after the closings of its day", "engine"),
    (LEDGER, "        self.miss_through(day);\n        self.sample_temporal_through(day);\n        self.clock.day = day;",
     "        self.sample_temporal_through(day);\n        self.clock.day = day;", "a day's facts do not miss what is out of reach first", "engine"),
    (LEDGER, "let limit = limit.min(Moment::end_of(self.horizon));", "let limit = limit;",
     "the fold goes past its horizon", "tests"),
    (TRACE, "    ledger.reach(horizon);\n", "", "the forecast's horizon is the day it stands on", "cli"),
    (TRACE, "ledger.promise(|contract| lens.owns_entity(book.contracts[contract].owner));", "ledger.promise(|_| true);",
     "a forecast for one owner promises everyone's contracts", "tests"),
    (TRACE, "    ledger.advance(today);\n    ledger.reach", "    ledger.reach", "today is not closed before the forecast begins", "tests"),
    (TRACE, "while let Some(planned) = ledger.promise_through(day) {",
     "while let Some(planned) = ledger.promise_through(day).filter(|_| false) {", "the report does not see each promised occurrence", "tests"),
    (FORECAST, "flows.sort_by_key(|flow| flow.day);", "", "the habits are applied out of date order", "tests"),
    # Item 3, a missed day is a claim: what the corpus has no deadline-and-party books for, the tests of claims say.
    (CLAIMS, "header.flow.to = tab;", "header.flow.to = header.flow.to;", "a claim is paid where the occurrence would have paid", "tests"),
    (CLAIMS, "header.flow.purpose = None;", "", "a claim recognizes what the contract's purpose says", "tests"),
    (CLAIMS, "let day = found.max(self.clock.day);", "let day = due;", "a claim is dated the day it fell due", "tests"),
    (CLAIMS, "header.flow.out.qty > Qty::ZERO", "true", "a header that is no amount is claimed", "tests"),
    (PLAN, "== Blame::Party).then_some(())?;", "== Blame::Owner).then_some(())?;", "the owner's debts are claimed, and the party's are not", "tests"),
    (MONITOR, ".chain(deadline).max()", ".chain(deadline).min()", "a day is missed at the earlier of its reach and its deadline", "tests"),
    (MONITOR, "day.checked_add(1)).map(Day);", "day.checked_add(0)).map(Day);", "a day is missed a day early", "tests"),
    (MONITOR, "!promise.claimed", "true", "what became a claim is warned of as missed too", "tests"),
    (LEDGER, "let claimed = self.claim_missed(promise, found);", "self.claim_missed(promise, found);\n            let claimed = true;",
     "a day is said to be claimed whether or not it was", "tests"),
    (LOWER, "terms.due.is_some() && terms.blame() == Blame::Party", "terms.blame() == Blame::Party",
     "a tab is asked for by every contract the party pays the owner by", "tests"),
    (BOOK, "due: Some(day),\n", "due: None,\n", "a claim the monitor made has no due day", "tests"),
]


def own_tests_fail(source, work):
    """Whether the unit tests of the crate that holds a mutant fail in SOURCE: what the corpus cannot say (the horizon a step
    stops at, whose contracts a forecast takes, what the report sees of each occurrence)."""
    env = dict(os.environ, CARGO_TARGET_DIR=os.path.join(work, "tests-target"))
    for package, targets in (("axiom-engine", ["--lib"]), ("axiom-report", ["--lib"])):
        run = subprocess.run(["cargo", "test", "--release", "--offline", "-p", package, *targets], cwd=source, env=env,
                             capture_output=True, text=True)
        if "could not compile" in run.stderr:
            raise SystemExit("the tests do not build")
        if run.returncode:
            return True
    return False


def build_cli(source, work):
    target = os.path.join(work, "cli-target")
    done = subprocess.run(["cargo", "build", "--release", "-q", "--offline", "-p", "axiom-cli"], cwd=source,
                          env=dict(os.environ, CARGO_TARGET_DIR=target), capture_output=True, text=True)
    if done.returncode:
        raise SystemExit(done.stderr[-2000:])
    return os.path.join(target, "release", "axiom")


def mutate(tree, work, directory, only=None):
    """Each mutant is built and must be caught: by the engine layer (the dump), by the CLI layer, or by the unit tests of its
    crate (which say what the corpus cannot). A mutant that is not is listed as SURVIVED: equivalent, or the corpus too weak."""
    from contracts import leave_out
    work = os.path.abspath(work)
    source = os.path.join(work, "tree")
    if not os.path.isdir(source):
        os.makedirs(work, exist_ok=True)
        shutil.copytree(os.path.abspath(tree), source, ignore=lambda at, names: leave_out(tree, at, names))
    out = os.path.join(work, "build")
    binary = build(source, out)
    cli_binary = build_cli(source, work)
    assert dump_all(binary, directory, 4, quiet=True) == 0, "the baseline fails its own comparison"
    assert cli(cli_binary, directory, 4, quiet=True) == 0, "the baseline's CLI fails its own comparison"
    reference = listed(cli_binary, directory)
    results = []
    for number, (path, old, replacement, what, layer) in enumerate(MUTANTS):
        if only is not None and number not in only:
            continue
        target = os.path.join(source, path)
        original = open(target).read()
        assert original.count(old) == 1, f"mutant {number}: the text occurs {original.count(old)} times in {path}"
        open(target, "w").write(original.replace(old, replacement))
        try:
            COVERED.clear()
            outcome = "killed by the dump" if layer == "engine" and dump_all(build(source, out), directory, 4, quiet=True) else None
            if outcome is None and layer in ("engine", "cli"):
                mutant = build_cli(source, work)
                if cli(mutant, directory, 4, quiet=True):
                    outcome = "killed by the CLI layer"
                elif listed(mutant, directory) != reference:
                    outcome = "killed by the occurrences it lists"
            if outcome is None:
                outcome = "killed by the tests" if own_tests_fail(source, work) else "SURVIVED"
        except SystemExit:
            outcome = "does not build"
        except subprocess.TimeoutExpired:
            outcome = "killed by a hang"
        finally:
            open(target, "w").write(original)
        results.append((number, outcome, what))
        print(f"mutant {number:02d} {outcome:<24} {what}", flush=True)
    summary = Counter(outcome for _, outcome, _ in results)
    print(f"{len(results)} mutants: " + ", ".join(f"{count} {outcome}" for outcome, count in sorted(summary.items())))
    with open(os.path.join(work, "mutants.txt"), "w") as handle:
        for number, outcome, what in results:
            handle.write(f"{number:02d} {outcome} {what}\n")


def main(argv):
    if len(argv) >= 3 and argv[1] == "gen":
        gen(argv[2], int(argv[3]), int(argv[4]) if len(argv) > 4 else 1)
        return 0
    if len(argv) >= 4 and argv[1] == "cli":
        return cli(argv[2], argv[3], int(argv[4]) if len(argv) > 4 else 4, argv[5] if len(argv) > 5 else UNTIL)
    if len(argv) >= 4 and argv[1] == "dump":
        return dump_all(argv[2], argv[3], int(argv[4]) if len(argv) > 4 else 4, argv[5] if len(argv) > 5 else UNTIL)
    if len(argv) >= 5 and argv[1] == "mutate":
        return mutate(argv[2], argv[3], argv[4], {int(n) for n in argv[5].split(",")} if len(argv) > 5 else None) or 0
    if len(argv) >= 5 and argv[1] == "against":
        return against(argv[2], argv[3], argv[4])
    if len(argv) >= 4 and argv[1] == "build":
        print(build(argv[2], argv[3]))
        return 0
    print(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
