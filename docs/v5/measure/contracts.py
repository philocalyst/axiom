#!/usr/bin/env python3
"""A generator of small projects full of contracts, and the oracle that compares what two ways of reading them say.

    contracts.py gen DIR N [SEED] [--slow]    write N projects into DIR (p0000/main.ax ...), and DIR/forms.json
    contracts.py build TREE OUT [--new] [--debug-assertions]
                                              build the dump (promises/) against the crates of TREE into OUT/
                                              (--new: with the compiled promise, which `check` asks;
                                              --debug-assertions: with the asserts the compile makes of the book)
    contracts.py dump BINARY DIR [JOBS] [TAG] [--slow]
                                              what BINARY says of every project: DIR/pNNNN/dump.TAG.txt (default TAG: old)
    contracts.py compare DIR A B              the projects whose dump.A.txt and dump.B.txt differ
    contracts.py check BINARY DIR [JOBS] [--slow]
                                              the dump's own verdict (a binary built --new) over every project
    contracts.py cover DIR [TAG]              what the projects hold, and what the dumps asked of them
    contracts.py mutate TREE WORK DIR [N,M..] [--new]
                                              the mutants of the oracle: each is built and must be caught by `compare`
                                              (--new: mutants of the new structure, caught by the verdict)

What it is for. Lane K5a builds, beside the machinery that says when a promise is due (`Contract::occurrences`,
`nearest_occurrence`, `amount_on_schedule`, the engine's count of an occurrence's ordinal), a structure that says it by
arithmetic: a schedule whose due days and ordinals are computed, not searched. It changes no behaviour, so the
proof is a comparison, and a comparison is worth what the corpus it ran over can tell apart. This is the corpus. Each
project is one to three contracts written by a seeded random generator (deterministic: the same SEED and N write the
same books), each a spec drawn from the forms a promise can take:

    cadence   daily, weekly, monthly, quarterly, yearly, twice monthly, every 2w / 6w / 10d / 45d / 3m / 18m / 5y,
              a mixed span (1m15d), a zero span
    on        none, a day of the month (1, 15, 28 to 31), last, several, a day of the year (04-15, 02-29, four of
              them), a weekday, two weekdays; and what the language allows and gives no meaning to (see `shape`)
    shape     plain (no `on`), tiled (every step lands in a period of its own), clamp (two days that fall on one in a
              short month: `on 30, last`), coarse (an `on` for a longer period than the cadence steps by: `weekly on
              15`), mixed (days of two kinds of period)
    anchor    `from` on the 1st, mid-month, at the month's end, on 02-29, mid-month against `on 1` (the first landing
              is before the anchor), and no `from` at all, which begins at Day::MIN
    life      `until`, `X ends`, `X waived`, `X waived until D`, adjacent and overlapping waivers, a waiver before
              the contract begins
    schedules a standing `buy` beside the regular schedule, alone, and due on the same day (ambiguous)
    amount    `rising 3% yearly`, `indexed to cpi yearly` (with a row missing, and with no `from`), `covers` the month,
              the quarter, the year or a span, `for last month|quarter|year`, `prorated`, both a period and a cover
    clauses   `grace`, `about`, `due .. else`, `deposit`, `share`, `input`, `area`
    loans     monthly, twice monthly, every 2w, every 45d, quarterly, a mixed span (which has no payment), a rate of
              0%, `resets`, `prepay`, `for ASSET`, and the line that originates it
    kept      lines on the due day, off it, in a waived stretch, after the end

`gen` counts the projects that hold each form. The oracle's own questions (the dump, `promises/main.rs`) are chosen
from a contract's days and never from an answer, so every build is asked the same: the due days in thirty windows,
the occurrence that each of several hundred days keeps, the ordinal of the first dozen due days and a spread of
later ones, `amount_on_schedule` and the recognition window of each of 2,200 days, a loan's payment, and the
engine's own promises. `mutate` shows that this is enough: each mutant of the machinery under test is built and
the dump of the mutant must differ from the dump of the baseline.
"""
import calendar
import datetime
import hashlib
import json
import os
import random
import re
import resource
import shutil
import subprocess
import sys
import tempfile
import time
from collections import Counter
from concurrent.futures import ThreadPoolExecutor

TODAY = "2026-06-30"

PRELUDE = """\
use std
base USD
purpose fees : spending
commodity VTI : stock
  precision 3
param cpi
  2021 100
  2022 104
  2023 108
  2024 111
  2025 115
  2026 119
param sofr
  2024 4%
  2025 5%
param lull
  2021 0
  2022 5
  2025 0
entity me : person
entity acme : org
entity shop : org
entity landlord : landlord
entity broker-co : org
account checking : bank
account savings : bank
account wallet : cash
account broker : brokerage
asset condo : property
opening 2026-01-01
  checking 200_000 USD
  savings 20_000 USD
"""

PARTIES = ["acme", "shop", "landlord", "broker-co"]
HOLDINGS = ["checking", "checking", "savings", "wallet"]

# cadence text, what each of its `on` may be, and the shape each makes: plain, tiled, clamp, coarse, mixed.
# A fine pairing is listed first, so that a draw of the first few is the likely one.
FINE, CLAMP, COARSE, MIXED = "tiled", "clamp", "coarse", "mixed"
CADENCES = [
    ("daily", [(None, "plain")], [("on monday", COARSE), ("on 15", COARSE)]),
    ("weekly", [(None, "plain"), ("on monday", FINE), ("on friday", FINE), ("on tuesday, friday", FINE)],
     [("on 15", COARSE), ("on last", COARSE), ("on 04-15", COARSE)]),
    ("every 2w", [(None, "plain"), ("on monday", FINE), ("on sunday", FINE)], []),
    ("every 6w", [(None, "plain"), ("on wednesday", FINE)], []),
    ("every 10d", [(None, "plain"), ("on monday", FINE)], []),
    ("every 3d", [(None, "plain")], [("on monday", COARSE)]),
    ("every 45d", [(None, "plain"), ("on 15", FINE), ("on last", FINE)], []),
    ("every 28d", [(None, "plain")], [("on 15", COARSE)]),
    ("every 30d", [(None, "plain")], [("on last", COARSE)]),
    ("every 400d", [(None, "plain"), ("on 04-15", FINE)], []),
    ("monthly", [(None, "plain"), ("on 1", FINE), ("on 15", FINE), ("on 28", FINE), ("on 29", FINE), ("on 30", FINE),
                 ("on 31", FINE), ("on last", FINE), ("on 1, 15", FINE), ("on 15, last", FINE), ("on 1, 15, last", FINE),
                 ("on friday", FINE), ("on monday, thursday", FINE)],
     [("on 04-15", COARSE), ("on 1, monday", MIXED), ("on 30, last", CLAMP), ("on 29, 30, 31", CLAMP),
      ("on 28, last", CLAMP)]),
    ("quarterly", [(None, "plain"), ("on 1", FINE), ("on 15", FINE), ("on last", FINE), ("on 31", FINE)],
     [("on 04-15", COARSE)]),
    ("yearly", [(None, "plain"), ("on 04-15", FINE), ("on 02-29", FINE), ("on 12-31", FINE),
                ("on 04-15, 06-15, 09-15, 01-15", FINE), ("on 15", FINE), ("on last", FINE)],
     [("on 02-28, 02-29", CLAMP), ("on 04-15, 15", MIXED)]),
    ("twice monthly", [("on 1, 15", FINE), ("on 15, last", FINE), ("on 10, 20", FINE), (None, "plain")],
     [("on 30, last", CLAMP), ("on 29, 30", CLAMP)]),
    ("every 2m", [(None, "plain"), ("on 1", FINE), ("on last", FINE)], []),
    ("every 3m", [("on 31", FINE), (None, "plain")], []),
    ("every 18m", [(None, "plain"), ("on 04-15", FINE)], []),
    ("every 5y", [(None, "plain"), ("on 02-29", FINE)], []),
    ("every 1m15d", [(None, "plain"), ("on 1", FINE), ("on monday", FINE)], []),
    # Two steps of a mixed span can land in one month (from 01-29, 1m1d: 03-01, then 03-31), so a day of the month
    # named for it is due twice a month: not a tiling, and what the criterion that says so must be caught keeping out.
    ("every 1m1d", [(None, "plain"), ("on 15", FINE), ("on 31", FINE), ("on last", FINE)], []),
    # Half a year is a step too short for a day of the year to land in a year of its own: 04-15 is due twice.
    ("every 6m", [(None, "plain"), ("on 15", FINE), ("on last", FINE)], [("on 04-15", COARSE), ("on 04-15, 10-15", COARSE)]),
    ("every 0d", [(None, "plain")], []),
]

FROMS = ["2020-01-01", "2024-02-29", "2026-01-29", "2025-06-15", "2025-12-31", "2026-01-01", "2026-01-15", "2026-01-30",
         "2026-01-31", "2026-02-01", "2026-03-31", "2026-05-17"]
UNTILS = ["2026-04-28", "2026-08-31", "2027-06-30", "2028-02-29", "2030-12-31"]


def parse(text):
    return datetime.date.fromisoformat(text)


def add_months(day, months):
    index = day.year * 12 + day.month - 1 + months
    year, month = divmod(index, 12)
    return datetime.date(year, month + 1, min(day.day, calendar.monthrange(year, month + 1)[1]))


class Book:
    """Lines of one project, and the forms it uses."""

    def __init__(self, rng, slow):
        self.rng, self.slow = rng, slow
        self.lines, self.forms, self.contracts = [], Counter(), 0

    def add(self, text, *forms):
        self.lines.extend(text.rstrip("\n").split("\n"))
        for form in forms:
            self.forms[form] += 1


def cadence_form(text):
    if text.startswith("every"):
        return {"d": "every-days", "w": "every-weeks", "m": "every-months", "y": "every-years"}.get(text[-1], "every")
    return text.replace(" ", "-")


def on_forms(on, text):
    """The kinds of day an `on` names, for the forms report."""
    if on is None:
        return ["on:none"]
    found = []
    for part in on[3:].split(", "):
        if re.fullmatch(r"\d\d-\d\d", part):
            found.append("on:day-of-year")
        elif part == "last":
            found.append("on:last")
        elif part.isdigit():
            found.append("on:day-of-month" + ("-clamping" if int(part) >= 29 else ""))
        else:
            found.append("on:weekday")
    if len(on[3:].split(", ")) > 1:
        found.append("on:several")
    return found


def pick_cadence(rng, standing=False):
    """A cadence and an `on`: usually a pairing the language means, now and then one it does not."""
    text, fine, odd = rng.choice(CADENCES if not standing else CADENCES[:15])
    pool = fine if (rng.random() < 0.82 or not odd) else odd
    on, shape = rng.choice(pool)
    return text, on, shape


def aim(start, text, on, count):
    """Dates a line might be written on: the steps of a cadence from `start`, roughly. A line that keeps nothing is
    a diagnostic, not a failure; this only has to be near often enough."""
    days = []
    span = re.fullmatch(r"every (\d+)([dwmy])", text)
    step = None
    if text == "daily":
        step = ("d", 1)
    elif text == "weekly":
        step = ("d", 7)
    elif text in ("monthly", "twice monthly"):
        step = ("m", 1)
    elif text == "quarterly":
        step = ("m", 3)
    elif text == "yearly":
        step = ("m", 12)
    elif span:
        unit, n = span.group(2), int(span.group(1))
        step = {"d": ("d", n), "w": ("d", 7 * n), "m": ("m", n), "y": ("m", 12 * n)}[unit]
    elif text == "every 1m15d":
        step = ("m", 1)
    if step is None or step[1] == 0:
        return [start]
    for index in range(count):
        base = start + datetime.timedelta(days=step[1] * index) if step[0] == "d" else add_months(start, step[1] * index)
        if on and on[3:].split(", ")[0].isdigit():
            wanted = int(on[3:].split(", ")[0])
            base = base.replace(day=min(wanted, calendar.monthrange(base.year, base.month)[1]))
        days.append(base)
    return days


def schedule_line(text, on, holding, direction, amount, about):
    words = [about and "about", amount, text, on, f"{direction} {holding}"]
    return "  " + " ".join(word for word in words if word)


def contract(book):
    rng, forms = book.rng, Counter()
    book.contracts += 1
    name = f"c{book.contracts}"
    party = rng.choice(PARTIES)
    holding = rng.choice(HOLDINGS)
    direction = rng.choice(["from", "into"])
    loan = rng.random() < 0.14
    standing = not loan and rng.random() < 0.12
    standing_only = standing and rng.random() < 0.3
    start_text = rng.choice(FROMS) if rng.random() > 0.08 else None
    forms["anchor:no-from" if start_text is None else "anchor:from"] += 1
    start = parse(start_text) if start_text else parse("2026-01-01")
    lines = [f"contract {name} with {party}"]
    shape = "plain"
    on = text = None
    if not standing_only:
        text, on, shape = pick_cadence(rng)
        if loan:
            text, on, shape = rng.choice([("monthly", "on 1", FINE), ("monthly", None, "plain"),
                                          ("twice monthly", "on 1, 15", FINE), ("every 2w", "on monday", FINE),
                                          ("every 45d", None, "plain"), ("quarterly", "on 15", FINE),
                                          ("every 1m15d", None, "plain"), ("every 10d", None, "plain"),
                                          ("monthly", "on 15, last", FINE)])
        forms[f"cadence:{cadence_form(text)}"] += 1
        forms[f"shape:{shape}"] += 1
        for form in on_forms(on, text):
            forms[form] += 1
        direction = "from" if loan else direction
        holding = "checking" if loan else holding
        amount = None if loan else f"{rng.randint(5, 3000)} USD"
        about = (not loan) and rng.random() < 0.06
        lines.append(schedule_line(text, on, holding, direction, amount, about))
        if about:
            forms["clause:about"] += 1
    if standing:
        stext, son, sshape = pick_cadence(rng, standing=True)
        if rng.random() < 0.25 and text:
            stext, son = text, on  # the same days: equally near both schedules
            forms["schedules:same-days"] += 1
        sline = f"  buy VTI for {rng.randint(50, 900)} USD {stext}" + (f" {son}" if son else "") + " from checking"
        lines.append(sline)
        forms["schedules:standing-only" if standing_only else "schedules:both"] += 1
        forms[f"standing-shape:{sshape}"] += 1
    if start_text:
        lines.append(f"  from {start_text}")
    until = None
    if start_text and rng.random() < 0.25:
        until = rng.choice([u for u in UNTILS if parse(u) > start] or [None])
        if until:
            lines.append(f"  until {until}")
            forms["life:until"] += 1
    if rng.random() < 0.1:
        lines.append("  grace " + rng.choice(["3d", "2w", "0d", "1m"]))
        forms["clause:grace"] += 1
    if not loan and not standing_only:
        escalation(rng, lines, forms, start_text)
        recognition(rng, lines, forms)
    clauses(book, lines, forms, holding)
    origin = loan_lines(rng, lines, forms, start) if loan else None
    book.add("\n".join(lines), *forms)
    life(book, name, (start, start_text, until), (text, on, standing_only), origin)


def escalation(rng, lines, forms, start_text):
    roll = rng.random()
    if roll < 0.12:
        lines.append("  rising " + rng.choice(["3%", "0%", "7.5%", "100%"]) + " yearly")
        forms["amount:rising"] += 1
        if start_text is None:
            forms["amount:escalation-no-from"] += 1
    elif roll < 0.22:
        index = "lull" if rng.random() < 0.12 else "cpi"
        lines.append(f"  indexed to {index} yearly")
        if index == "lull":
            forms["amount:index-zero"] += 1
        forms["amount:indexed"] += 1
        if start_text is None:
            forms["amount:escalation-no-from"] += 1
        elif parse(start_text).year < 2021:
            forms["amount:index-row-missing"] += 1


def recognition(rng, lines, forms):
    roll = rng.random()
    window = None
    if roll < 0.08:
        window = rng.choice(["covers the month", "covers the quarter", "covers the year", "covers 6m", "covers 45d",
                             "covers 0d"])
        forms["recognition:" + window.replace(" ", "-")] += 1
    elif roll < 0.14:
        window = rng.choice(["for last month", "for last quarter", "for last year"])
        forms["recognition:" + window.replace(" ", "-")] += 1
    if window:
        lines.append("  " + window)
        if rng.random() < 0.15:
            both = "covers the month" if window.startswith("for") else "for last month"
            lines.append("  " + both)
            forms["recognition:conflict"] += 1
    if rng.random() < 0.08:
        lines.append("  prorated")
        forms["recognition:prorated" if window else "recognition:prorated-no-window"] += 1


def clauses(book, lines, forms, holding):
    rng = book.rng
    if rng.random() < 0.08:
        lines.append("  due 5d else + 5% of 100 USD #fees")
        forms["clause:due-else"] += 1
    if rng.random() < 0.08 and holding in ("checking", "savings"):
        lines.append(f"  deposit {rng.randint(200, 900)} USD")
        forms["clause:deposit"] += 1
    if rng.random() < 0.05:
        lines.append("  input water USD")
        lines.append("  + 12% of water #fees")
        forms["clause:input"] += 1
    if rng.random() < 0.04:
        lines.append("  area 1_000 SQFT")
        lines.append("  share 120 SQFT for acme")
        forms["clause:share"] += 1
    elif rng.random() < 0.04:
        lines.append("  share 20% for acme")
        forms["clause:share"] += 1


def loan_lines(rng, lines, forms, start):
    """A loan's lines; the day it is originated on."""
    principal = rng.choice([3_000, 30_000, 100_000, 320_000])
    rate = rng.choice(["0", "4", "5.5", "9.99"])
    term = rng.choice(["1y", "3y", "10y", "90d", "1y6m", "2m"])
    on = start - datetime.timedelta(days=rng.choice([0, 14, 31]))
    lines.append(f"  loan {principal} USD on {on.isoformat()} at {rate}% over {term}" + rng.choice(["", "", " for condo"]))
    forms["loan:present"] += 1
    forms[f"loan:rate-{'zero' if rate == '0' else 'positive'}"] += 1
    forms[f"loan:term-{term}"] += 1
    if rng.random() < 0.3:
        lines.append(f"    resets 1y from {(on + datetime.timedelta(days=366)).isoformat()} to sofr + 2.75% cap 2% life 5%")
        forms["loan:resets"] += 1
    if rng.random() < 0.3:
        lines.append("    prepay " + rng.choice(["recasts", "shortens"]))
        forms["loan:prepay"] += 1
    return on.isoformat()


def life(book, name, life_days, schedule, origin):
    """The lines that come after a contract: its occurrences kept, waived and ended."""
    rng, forms = book.rng, Counter()
    (start, start_text, until), (text, on, standing_only) = life_days, schedule
    if origin and rng.random() < 0.6:
        book.add(f"{origin} {name}", "loan:origination-written")
    if start_text is None and not book.slow:
        kept = 0
    else:
        kept = rng.choice([0, 0, 1, 2, 3, 4])
    stop = parse(until) if until else parse("2026-06-30")
    dues = [d for d in aim(max(start, parse("2025-12-01")), text or "monthly", on, 14) if d <= stop and d >= start]
    for due in rng.sample(dues, min(kept, len(dues))) if dues else []:
        off = rng.choice([0, 0, 0, -3, -2, -1, 1, 2, 5, 12])
        day = due + datetime.timedelta(days=off)
        if day < start:
            day = start
        book.add(f"{day.isoformat()} {name}", "kept:on-due" if off == 0 else "kept:off-due")
    waives = rng.choice([0, 0, 0, 1, 1, 2, 3]) if text or standing_only else 0
    spans = []
    horizon = ((parse(until) if until else start + datetime.timedelta(days=400)) - start).days
    for _ in range(waives):
        early = -20 if rng.random() < 0.15 else 0
        first = start + datetime.timedelta(days=rng.randint(early, max(horizon, 1)))
        if rng.random() < 0.5:
            book.add(f"{first.isoformat()} {name} waived", "life:waived-one")
            spans.append((first, first))
        else:
            last = first + datetime.timedelta(days=rng.randint(0, 120))
            book.add(f"{first.isoformat()} {name} waived until {last.isoformat()}", "life:waived-span")
            spans.append((first, last))
        if first < start:
            forms["life:waived-before-start"] += 1
    if len(spans) > 1:
        spans.sort()
        forms["life:waived-several"] += 1
        if any(a[1] + datetime.timedelta(days=1) >= b[0] for a, b in zip(spans, spans[1:])):
            forms["life:waived-overlap-or-adjacent"] += 1
    if rng.random() < 0.12 and start_text:
        end = start + datetime.timedelta(days=rng.randint(30, 600))
        book.add(f"{end.isoformat()} {name} ends", "life:ended")
        if kept:
            forms["kept:before-end"] += 1
    book.forms.update(forms)


def project(seed, index, slow):
    rng = random.Random(seed * 1_000_003 + index)
    book = Book(rng, slow)
    for _ in range(rng.choices([1, 2, 3], [5, 3, 1])[0]):
        contract(book)
    return book


def gen(directory, count, seed, slow):
    os.makedirs(directory, exist_ok=True)
    all_forms = {}
    for index in range(count):
        book = project(seed, index, slow)
        path = os.path.join(directory, f"p{index:04d}")
        os.makedirs(path, exist_ok=True)
        with open(os.path.join(path, "main.ax"), "w") as out:
            out.write(PRELUDE + "\n".join(book.lines) + "\n")
        all_forms[f"p{index:04d}"] = dict(book.forms)
    with open(os.path.join(directory, "forms.json"), "w") as out:
        json.dump(all_forms, out, indent=0, sort_keys=True)
    return all_forms


# ─── Building and running the dump ───────────────────────────────────────────────────────────────────────────

HERE = os.path.dirname(os.path.abspath(__file__))
CRATES = ["core", "syntax", "model", "engine", "systems"]


PROFILE = "opt-level = 1\ncodegen-units = 16\nincremental = true"
DEBUG_PROFILE = PROFILE + "\ndebug-assertions = true"


def build(tree, out, new=False, source=None, profile=PROFILE):
    """Builds the dump against the crates of TREE. Its own profile, because a dependency is built with the profile of
    the workspace that asks for it, and the tree's is `lto = thin`. With NEW it also has the compiled promise to ask
    (`--check`), which a tree from before the lane does not. SOURCE is where the dump's files are read from."""
    os.makedirs(out, exist_ok=True)
    tree = os.path.abspath(tree)
    source = source or os.path.join(HERE, "promises")
    deps = "\n".join(f'axiom-{c} = {{ path = "{tree}/crates/{c}" }}' for c in CRATES)
    manifest = f'[package]\nname = "promises-dump"\nversion = "0.0.0"\nedition = "2024"\n\n[workspace]\n\n' \
               f'[features]\nnew = []\n\n[[bin]]\nname = "dump"\npath = "main.rs"\n\n[dependencies]\n{deps}\n\n' \
               f'[profile.release]\n{profile}\n'
    with open(os.path.join(out, "Cargo.toml"), "w") as handle:
        handle.write(manifest)
    for name in os.listdir(source):
        if name.endswith(".rs"):
            shutil.copy(os.path.join(source, name), os.path.join(out, name))
    shutil.copy(os.path.join(tree, "Cargo.lock"), os.path.join(out, "Cargo.lock"))
    command = ["cargo", "build", "--release", "--offline"] + (["--features", "new"] if new else [])
    result = subprocess.run(command, cwd=out, capture_output=True, text=True)
    if result.returncode:
        sys.stderr.write(result.stderr[-4000:])
        raise SystemExit("the dump did not build")
    return os.path.join(out, "target", "release", "dump")


def projects(directory):
    return sorted(os.path.join(directory, name) for name in os.listdir(directory) if name.startswith("p"))


def run_dump(binary, path, extra=(), limit=300):
    """What the dump says of a project. One that does not answer in LIMIT seconds is a hang, one that wants more than
    MOST_MEMORY or writes more than MOST_OUTPUT stops: a mutant that loops, or collects, or says too much is caught like
    one that is wrong."""
    with tempfile.TemporaryFile() as said, tempfile.TemporaryFile() as complained:
        try:
            result = subprocess.run([binary, *extra, path], stdout=said, stderr=complained, timeout=limit,
                                    preexec_fn=limit_resources)
        except subprocess.TimeoutExpired:
            return 124, "", f"no answer in {limit} seconds"
        said.seek(0)
        complained.seek(0, os.SEEK_END)
        complained.seek(max(0, complained.tell() - 4000))
        return result.returncode, said.read().decode(errors="replace"), complained.read().decode(errors="replace")


MOST_MEMORY = 2 << 30
MOST_OUTPUT = 32 << 20


def limit_resources():
    """A mutant that collects what it should count took 14 GB (the kernel killed the run), and a harness that held what
    a mutant said took 11."""
    resource.setrlimit(resource.RLIMIT_AS, (MOST_MEMORY, MOST_MEMORY))
    resource.setrlimit(resource.RLIMIT_FSIZE, (MOST_OUTPUT, MOST_OUTPUT))


def dump(binary, directory, jobs=3, tag="old", extra=(), limit=300, times=None):
    """Runs BINARY over every project. After three that do not answer in time the rest are not asked. With TIMES, how
    long each took (seconds) is added to it."""
    hung = []

    def one(path):
        if len(hung) >= 3:
            return path, 124
        began = time.monotonic()
        code, out, err = run_dump(binary, path, extra, limit)
        if times is not None:
            times.append(time.monotonic() - began)
        if code == 124:
            hung.append(path)
        with open(os.path.join(path, f"dump.{tag}.txt"), "w") as handle:
            handle.write(out)
            if code:
                handle.write(f"EXIT {code}\n{err[-2000:]}\n")
        return path, code

    with ThreadPoolExecutor(jobs) as pool:
        results = list(pool.map(one, projects(directory)))
    failed = [path for path, code in results if code]
    print(f"{len(results)} projects dumped as {tag}, {len(failed)} failed")
    for path in failed[:5]:
        print("  failed:", path)
    return len(failed)


def compare(directory, a, b, show=3):
    different = []
    for path in projects(directory):
        try:
            left = open(os.path.join(path, f"dump.{a}.txt")).read().split("\n")
            right = open(os.path.join(path, f"dump.{b}.txt")).read().split("\n")
        except FileNotFoundError:
            different.append((path, 0, ["not run"], ["not run"]))  # the mutant hung, and the rest were not asked
            continue
        if left != right:
            at = next((i for i, (x, y) in enumerate(zip(left, right)) if x != y), min(len(left), len(right)))
            different.append((path, at, left[at:at + 1], right[at:at + 1]))
    for path, at, left, right in different[:show]:
        print(f"DIFFERENT {path} at line {at}:\n  {a}: {left}\n  {b}: {right}")
    print(f"{len(projects(directory))} projects, {len(different)} differ")
    return len(different)


# ─── Mutants of the machinery under test ─────────────────────────────────────────────────────────────────────

# (file, text, replacement, what it breaks). Each text occurs once in its file. A mutant is built into the dump and the
# dump must differ from the baseline's: if it does not, the corpus cannot tell the code from a wrong one.
CAL, BOOK = "crates/core/src/calendar.rs", "crates/model/src/book.rs"
REC, STM = "crates/model/src/lower/record.rs", "crates/model/src/lower/statements.rs"
LED = "crates/engine/src/ledger.rs"
MUTANTS = [
    (CAL, "On::Last => checked_day(year, month, days_in_month(year, month)),",
     "On::Last => checked_day(year, month, days_in_month(year, month) - 1),", "`on last` is the day before the last"),
    (CAL, "On::MonthDay(day) => clamped(month, u32::from(day)),", "On::MonthDay(day) => checked_day(year, month, u32::from(day)),",
     "a day of the month past the month's end is no day"),
    (CAL, "((u32::from(weekday) + 7 - base.weekday()) % 7)", "((u32::from(weekday) + 7 - base.weekday()) % 6)",
     "a weekday lands on the wrong day"),
    (CAL, "if len == 2 && days[1] < days[0] {", "if len == 2 && days[1] > days[0] {", "two landings of a step in the wrong order"),
    (CAL, "if len == 2 && days[0] == days[1] {", "if len == 2 && days[0] != days[1] {", "two landings on one day are not one"),
    (CAL, ".filter(|&day| self.previous.is_none_or(|previous| day > previous))",
     ".filter(|&day| self.previous.is_none_or(|previous| day >= previous))", "three landings, one repeated"),
    (CAL, "let base_limit = (i64::from(within.last().0) + backward_landing).min(i64::from(i32::MAX));",
     "let base_limit = i64::from(within.last().0).min(i64::from(i32::MAX));", "a landing before its step is not looked for"),
    (CAL, "(i64::from(anchor.max(within.first()).0) - forward_landing)", "(i64::from(anchor.max(within.first()).0) + forward_landing)",
     "the walk starts too late"),
    (CAL, "Cadence::TwiceMonthly => Span::months(1),", "Cadence::TwiceMonthly => Span::months(2),", "twice monthly steps by two months"),
    (CAL, "Some(day) if day < target => {", "Some(day) if day <= target => {", "the first step is one late"),
    (CAL, "candidate if candidate <= day => Some((candidate, years)),", "candidate if candidate < day => Some((candidate, years)),",
     "an anniversary is not its own day"),
    (CAL, ".filter(move |&day| day >= anchor && within.contains(day))", ".filter(move |&day| within.contains(day))",
     "a landing before the anchor is due"),
    (CAL, "let clamped = |month: u32, day: u32| checked_day(year, month, day.min(days_in_month(year, month)));",
     "let clamped = |month: u32, day: u32| checked_day(year, month, day);", "a day past a short month's end is no day"),
    (CAL, "(On::YearDay { .. }, _) => 365,", "(On::YearDay { .. }, _) => 300,", "the walk starts too late for a day of the year"),
    (CAL, "let landed = checked_day(year, month, day_of_month.min(days_in_month(year, month)))?;",
     "let landed = checked_day(year, month, day_of_month)?;", "a step from the 31st has no February"),
    (BOOK, "(Some(regular), Some(standing)) if standing.day < regular.day => self.standing.next(),",
     "(Some(regular), Some(standing)) if standing.day <= regular.day => self.standing.next(),", "a standing day before a regular one on a tie"),
    (BOOK, "let window = within.intersect(self.days);", "let window = Some(within);", "days outside the contract are due"),
    (BOOK, ".filter(move |(_, terms)| enabled && !terms.is_waived())", ".filter(move |(_, terms)| enabled)",
     "a waived stretch is due"),
    (BOOK, "ratio_pow(yearly, u32::try_from(years).map_err(|_| ForecastError::Overflow)?)",
     "ratio_pow(yearly, u32::try_from(years + 1).map_err(|_| ForecastError::Overflow)?)", "a rise one year early"),
    (BOOK, "current.checked_div(base).ok_or(ForecastError::Overflow)", "base.checked_div(current).ok_or(ForecastError::Overflow)",
     "the index moves the wrong way"),
    (BOOK, "Value::Num(value) if value > Ratio::ZERO => Ok(value),", "Value::Num(value) if value >= Ratio::ZERO => Ok(value),",
     "an index of zero is an index"),
    (BOOK, "let part = i64::from(overlap.last().0) - i64::from(overlap.first().0) + 1;",
     "let part = i64::from(overlap.last().0) - i64::from(overlap.first().0);", "a share is one day short"),
    (BOOK, "if after <= start {", "if after < start {", "a cover of no days is one"),
    (BOOK, "(Some(Relative::LastQuarter), None) => Ok(Some(calendar::quarter(day, -1))),",
     "(Some(Relative::LastQuarter), None) => Ok(Some(calendar::quarter(day, 0))),", "last quarter is this one"),
    (BOOK, "(None, Some(Coverage::Calendar(period))) => Ok(Some(Window::containing(period, day).days())),",
     "(None, Some(Coverage::Calendar(period))) => Ok(Some(Window::containing(period, day).previous().days())),",
     "the month covered is the last"),
    (BOOK, "if !self.days.contains(day) {\n            return Err(ForecastError::OutsideContract(day));\n        }\n        let terms = self.terms_on_schedule(schedule, day).ok_or(ForecastError::OutsideContract(day))?;",
     "let terms = self.terms_on_schedule(schedule, day).ok_or(ForecastError::OutsideContract(day))?;",
     "a day outside the contract has a factor"),
    (BOOK, "ScheduleKind::Standing => self.standing.as_ref().map(|terms| terms.at(day)),",
     "ScheduleKind::Standing => self.terms.as_ref().map(|terms| terms.at(day)),", "the standing schedule has the regular terms"),
    (BOOK, "if exponent & 1 == 1 {", "if exponent & 1 == 0 {", "a power by squaring that squares wrong"),
    (BOOK, "let (_, years) = calendar::anniversary(self.days.first(), day).ok_or(ForecastError::Overflow)?;",
     "let (_, years) = calendar::anniversary(day, day).ok_or(ForecastError::Overflow)?;", "years counted from the day itself"),
    (BOOK, "let yearly = Ratio::ONE.checked_add(rate).ok_or(ForecastError::Overflow)?;", "let yearly = rate;",
     "a rise of 3% is a factor of 3%"),
    (BOOK, "let shift = day.0.checked_sub(template.day.0).ok_or(ForecastError::Overflow)?;",
     "let shift = template.day.0.checked_sub(day.0).ok_or(ForecastError::Overflow)?;", "a recognition moved the wrong way"),
    (REC, "i64::from(span.months).saturating_mul(31).saturating_add(i64::from(span.days))",
     "i64::from(span.months).saturating_mul(30).saturating_add(i64::from(span.days))", "the reach of a month is 30 days"),
    (REC, "if best.is_none_or(|(best_distance, best_future, _, _)| (distance, candidate.1) < (best_distance, best_future))",
     "if best.is_none_or(|(best_distance, best_future, _, _)| (distance, candidate.1) <= (best_distance, best_future))",
     "the later of two equally near is kept"),
    (REC, "let candidate = (distance, occurrence.day > day, occurrence.day, occurrence.terms);",
     "let candidate = (distance, occurrence.day >= day, occurrence.day, occurrence.terms);", "the day itself counts as later"),
    (REC, "if r_distance == s_distance {", "if r_distance < s_distance {", "equally near two schedules is not ambiguous"),
    (REC, "if !contract.days.contains(day) {\n        return Ok(None);\n    }\n    let mut radius = 0i64;",
     "let mut radius = 0i64;", "a line outside the contract keeps an occurrence"),
    (REC, "crate::book::Cadence::TwiceMonthly => 31,", "crate::book::Cadence::TwiceMonthly => 15,", "the reach of twice monthly is 15 days"),
    (REC, "radius = radius.max(cadence);", "radius = radius.min(cadence);", "the reach is the smallest cadence"),
    (STM, "terms.paint(change.days, waived);", "terms.paint(Days::on(change.days.first()), waived);", "a waiver is one day"),
    (STM, "Days::new(contract.days.first(), day.min(contract.days.last()))", "Days::new(contract.days.first(), day)",
     "an end can extend a contract"),
    ("crates/model/src/lower/contracts.rs", "        anchor: cx.anchor,\n        template: Box::new([template]),",
     "        anchor: Day::MIN,\n        template: Box::new([template]),", "every schedule counts from Day::MIN"),
    (LED, ".filter(|occurrence| occurrence.schedule == schedule && occurrence.day <= due)",
     ".filter(|occurrence| occurrence.schedule == schedule && occurrence.day < due)", "the ordinal of an occurrence is one short"),
    (LED, "let Some(ordinal) = count.checked_sub(1)", "let Some(ordinal) = count.checked_sub(0)", "the ordinal is one over"),
    (LED, "let periods = loan.term.months.checked_add(months - 1)?.checked_div(months)?;",
     "let periods = loan.term.months.checked_add(months)?.checked_div(months)?;", "a loan has a period too many"),
    (LED, "let rate = annual.checked_mul(Ratio::new(months as i128, 12)?)?;", "let rate = annual.checked_mul(Ratio::new(months as i128, 11)?)?;",
     "a monthly rate of a twelfth is an eleventh"),
    (LED, "let periods = loan.term.months.checked_mul(2)?;", "let periods = loan.term.months.checked_mul(3)?;", "twice monthly pays three times"),
    (LED, "let rate = annual.checked_div(Ratio::int(24))?;", "let rate = annual.checked_div(Ratio::int(12))?;", "twice monthly pays a monthly rate"),
    (LED, "let periods = loan.term.days.checked_add(days - 1)?.checked_div(days)?;",
     "let periods = loan.term.days.checked_add(days)?.checked_div(days)?;", "a loan in days has a period too many"),
    (LED, "Err(axiom_model::ForecastError::Overflow) if template.header.flow.day == Day::MIN => Days::on(due),",
     "Err(axiom_model::ForecastError::Overflow) if false => Days::on(due),", "no recognition for a contract with no start"),
    ("crates/core/src/timeline.rs", ".skip_while(move |(days, _)| days.last() < within.first())",
     ".skip_while(move |(days, _)| days.last() <= within.first())", "a stretch that ends on the first day is skipped"),
]


# The same, of the new structure: each of these must make the verdict fail.
DUES, SCHED = "crates/core/src/dues.rs", "crates/model/src/promise/schedule.rs"
PROMISE, RECKON = "crates/model/src/promise.rs", "crates/model/src/promise/reckon.rs"
ANNUITY, RESIDUAL = "crates/model/src/promise/annuity.rs", "crates/model/src/promise/residual.rs"
MUTANTS_NEW = [
    (DUES, "take_while(|&&day| day < i64::from(anchor.0))", "take_while(|&&day| day <= i64::from(anchor.0))",
     "a day on the anchor is before it"),
    (DUES, "let slot = u64::from(n) + u64::from(head);", "let slot = u64::from(n);", "the days before the anchor are due"),
    (DUES, ".is_some_and(|&last| last < x)", ".is_some_and(|&last| last <= x)", "a step that ends on the day is before it"),
    (DUES, ".saturating_sub(u64::from(head))", ".saturating_sub(u64::from(head) + 1)", "one day too few before"),
    (DUES, "let early = Day(within.first().0.saturating_sub(35).max(self.anchor.0));",
     "let early = Day(within.first().0.saturating_sub(0).max(self.anchor.0));", "a walked window loses the last of a month"),
    (DUES, "if kept == 0 || block.days[kept - 1] != block.days[at] {", "if kept == 0 || block.days[kept - 1] == block.days[at] {",
     "a day two landings share is not one, or the others are lost"),
    (DUES, "if count == 0 || named[count - 1] != named[at] {", "if count == 0 || named[count - 1] == named[at] {",
     "the days of an on are counted with their repeats"),
    (DUES, "Within::Month => (step.days == 0 && step.months >= 1) || (step.months == 0 && step.days >= 31),",
     "Within::Month => step.months >= 1 || step.days >= 31,", "a mixed span lands each step in a month of its own"),
    (DUES, "Within::Year => (step.days == 0 && step.months >= 12)", "Within::Year => (step.days == 0 && step.months >= 6)",
     "a half-yearly step lands each in a year of its own"),
    (DUES, "Within::Week => step.months >= 1 || step.days >= 7,", "Within::Week => step.months >= 1 || step.days >= 1,",
     "a step shorter than a week lands each in a week of its own"),
    (DUES, "On::MonthDay(day) => *day >= 28,", "On::MonthDay(day) => *day >= 29,", "the 28th does not clamp"),
    (DUES, "        2 => 28,\n", "        2 => 29,\n", "February is 29 days at the shortest"),
    (DUES, "        if holds(middle) {", "        if !holds(middle) {", "the search goes the wrong way"),
    (DUES, "let advances = step.months >= 0 && step.days >= 0 && step > Span::default();",
     "let advances = step.months >= 0 && step.days >= 0;", "a cadence of no days has due days"),
    (SCHED, "Some(hole) if hole.days.contains(day) => Err(ForecastError::Waived(day)),",
     "Some(hole) if false && hole.days.contains(day) => Err(ForecastError::Waived(day)),", "a waived day has a factor"),
    (SCHED, "hole.owed_before() <= n", "hole.owed_before() < n", "the day after a hole is one early"),
    (SCHED, "self.schedule.life.last().0.saturating_add(1)", "self.schedule.life.last().0", "the last day is not owed"),
    (SCHED, "Some(hole) if hole.days.last() < limit => hole.before + hole.gone,",
     "Some(hole) if hole.days.last() <= limit => hole.before + hole.gone,", "the day after a hole is inside it"),
    (SCHED, "=> hole.before + hole.gone,", "=> hole.before,", "a hole swallows nothing"),
    (SCHED, "gone: through(dues, days.last()) - from", "gone: through(dues, Day(days.last().0.saturating_sub(1))) - from",
     "a hole swallows the days but its last"),
    (SCHED, "(nearest.apart, nearest.due > day)", "(nearest.apart, nearest.due < day)", "the later of two equally near is kept"),
    (SCHED, "if regular.apart < standing.apart =>", "if regular.apart > standing.apart =>", "the farther schedule is kept"),
    (PROMISE, "i64::from(months).saturating_mul(31)", "i64::from(months).saturating_mul(30)", "the reach of a month is 30 days"),
    (PROMISE, ".filter(|(_, terms)| terms.is_waived())", ".filter(|(_, terms)| !terms.is_waived())", "the holes are the active stretches"),
    (PROMISE, "axiom_core::Cadence::TwiceMonthly => 31,", "axiom_core::Cadence::TwiceMonthly => 15,", "the reach of twice monthly is 15"),
    (RECKON, "Ratio::ONE.checked_add(rate)", "Ratio::ONE.checked_sub(rate)", "a rise of 3% is a fall"),
    (RECKON, "power(yearly, u32::try_from(years)", "power(yearly, u32::try_from(years + 1)", "a rise a year early"),
    (RECKON, "if years & 1 == 1 {", "if years & 1 == 0 {", "a power by squaring that squares wrong"),
    (RECKON, "Ratio::new(count(alive), count(period))", "Ratio::new(count(alive) + 1, count(period))", "a share one day too many"),
    (RECKON, "if terms.prorated { Proration::Prorated } else { Proration::Whole }",
     "if terms.prorated { Proration::Whole } else { Proration::Prorated }", "prorated is whole"),
    (RECKON, "(Some(relative), None) => Recognition::Last(relative),", "(Some(_), None) => Recognition::OnTheDay,", "for last month is the day"),
    (ANNUITY, "loan.term.months.checked_add(months - 1)?", "loan.term.months.checked_add(months)?", "a loan has a payment too many"),
    (ANNUITY, "if index + 1 >= self.periods {", "if index + 1 > self.periods {", "the last payment is the level payment"),
    (RESIDUAL, "self.ordinal - self.began", "self.ordinal", "a loan's payments are counted from the schedule's first day"),
    (RESIDUAL, "schedule.before(Day(annuity.begins().0.saturating_add(1)))", "schedule.before(annuity.begins())",
     "a loan's first payment is on the day it was made"),
    (RESIDUAL, "            self.open == Qty::ZERO\n", "            false\n", "a loan is never done"),
    (RESIDUAL, "paid.map_or(Qty::ZERO, |paid| paid.open)", "paid.map_or(self.open, |paid| paid.open + Qty(1))",
     "a payment leaves a cent more owed"),
]


def leave_out(tree, directory, names):
    """What a copy of TREE does not need: what is built, kept, or the book's own, and its goldens (`tests/` at the top;
    a crate's own are what `own_tests_fail` runs)."""
    top = os.path.samefile(directory, tree)
    return [name for name in names if name in ("target", ".git", ".claude", "docs", "examples") or (top and name == "tests")]


def own_tests_fail(source, work):
    """Whether the tests of `core` and `model` (their units, and `tests/promises.rs`) fail in SOURCE: what a mutant of
    the new structure that the dump's verdict does not catch has still to get past, because the old code cannot say
    what a loan has left after a payment, or what the days before a hole's last day are."""
    env = dict(os.environ, CARGO_TARGET_DIR=os.path.join(work, "tests-target"))
    for package, targets in (("axiom-core", ["--lib"]), ("axiom-model", ["--lib", "--test", "promises"])):
        run = subprocess.run(["cargo", "test", "--release", "--offline", "-p", package, *targets], cwd=source, env=env,
                             capture_output=True, text=True)
        if run.returncode:
            return True
    return False


def mutate(tree, work, directory, only=None, new=False):
    """Builds each mutant into the dump. Of the machinery under test (the old code), the dump of the mutant must differ
    from the baseline's; of the new structure (NEW), its verdict must fail, or its own tests, which say what the old code cannot. Mutants that are not caught are listed:
    each is either equivalent, and the report says why, or the corpus is too weak."""
    table = MUTANTS_NEW if new else MUTANTS
    work = os.path.abspath(work)
    source = os.path.join(work, "tree")
    if not os.path.isdir(source):
        os.makedirs(work, exist_ok=True)
        shutil.copytree(os.path.abspath(tree), source, ignore=lambda at, names: leave_out(tree, at, names))
    out = os.path.join(work, "build")
    snapshot = os.path.join(work, "promises")
    if not os.path.isdir(snapshot):
        shutil.copytree(os.path.join(HERE, "promises"), snapshot)
    binary = build(source, out, new=new, source=snapshot)
    if new:
        baseline = verdict(binary, directory, 4)[1]
        assert not baseline, f"the baseline fails its own verdict: {baseline[:2]}"
        assert not own_tests_fail(source, work), "the baseline fails its own tests"
        limit = 20
    else:
        times = []
        dump(binary, directory, 4, "base", times=times)
        # The old code is slow on some projects (a cadence of no days is walked for a minute): a mutant is given three
        # times what the slowest of them took, or it would be caught by a clock and not by what it says.
        limit = max(20, int(3 * max(times)) + 1)
    results = []
    for number, (path, old, replacement, what) in enumerate(table):
        if only is not None and number not in only:
            continue
        target = os.path.join(source, path)
        original = open(target).read()
        assert original.count(old) == 1, f"mutant {number}: the text occurs {original.count(old)} times in {path}"
        open(target, "w").write(original.replace(old, replacement))
        tag = f"m{number:02d}"
        by_tests = False
        try:
            binary = build(source, out, new=new, source=snapshot)
            if new:
                caught = bool(verdict(binary, directory, 4, limit=limit, enough=3)[1])
                by_tests = not caught and own_tests_fail(source, work)
            else:
                dump(binary, directory, 4, tag, limit=limit)
                caught = bool(compare(directory, "base", tag, show=0))
            outcome = "killed" if caught else "killed by the tests" if by_tests else "SURVIVED"
        except SystemExit:
            outcome = "does not build"
        finally:
            open(target, "w").write(original)
            for project in projects(directory):
                try:
                    os.remove(os.path.join(project, f"dump.{tag}.txt"))
                except FileNotFoundError:
                    pass
        results.append((number, outcome, what))
        print(f"mutant {number:02d} {outcome:<8} {what}", flush=True)
    summary = Counter(outcome for _, outcome, _ in results)
    print(f"{len(results)} mutants: {summary['killed']} killed, {summary['killed by the tests']} killed by the tests, "
          f"{summary['SURVIVED']} survived, {summary['does not build']} did not build")
    with open(os.path.join(work, "mutants.txt"), "w") as handle:
        for number, outcome, what in results:
            handle.write(f"{number:02d} {outcome} {what}\n")


# ─── The verdict ─────────────────────────────────────────────────────────────────────────────────────────────


def verdict(binary, directory, jobs=3, extra=(), limit=120, enough=None):
    """Runs the dump's own comparison (`--check`) over every project: what it says, summed, and what failed. A project's
    answer is read as it comes and not kept: a mutant can make it as long as MOST_OUTPUT, for every project. With
    ENOUGH, the projects not yet asked are not once that many have failed: a mutant is caught by one."""
    failed = []

    def one(path):
        if enough is not None and len(failed) >= enough:
            return Counter(), []
        code, out, err = run_dump(binary, path, ("--check", *extra), limit)
        tallies, failures = Counter(), []
        for line in out.split("\n"):
            if line.startswith("check "):
                *key, count = line.split()
                tallies[" ".join(key)] += int(count)
            elif line.startswith("FAIL "):
                failures.append((path, line[:300]))
        if code not in (0, 1):
            failures.append((path, f"FAIL the dump stopped: exit {code} {err[-300:]}"))
        failed.extend(failures)
        return tallies, failures[:3]

    tallies, failures, projects_run = Counter(), [], 0
    with ThreadPoolExecutor(jobs) as pool:
        for found, failed in pool.map(one, projects(directory)):
            tallies.update(found)
            failures.extend(failed)
            projects_run += 1
    return tallies, failures, projects_run


def check(binary, directory, jobs=3, extra=()):
    """The verdict, printed: for each question how often the new structure agreed with the reference, how often the old
    code did, and the ways it did not."""
    tallies, failures, projects_run = verdict(binary, directory, jobs, extra)
    width = max((len(key) for key in tallies), default=0)
    for key in sorted(tallies):
        print(f"  {key:<{width}} {tallies[key]:>10}")
    for path, line in failures[:10]:
        print(f"{os.path.basename(path)}: {line[:600]}")
    print(f"{projects_run} projects, {len(failures)} failures")
    return len(failures)


# ─── What the corpus holds ───────────────────────────────────────────────────────────────────────────────────


def cover(directory, tag="old"):
    forms = json.load(open(os.path.join(directory, "forms.json")))
    held, facts = Counter(), Counter()
    clean = Counter()
    for path in projects(directory):
        name = os.path.basename(path)
        for form in forms[name]:
            held[form] += 1
        try:
            text = open(os.path.join(path, f"dump.{tag}.txt")).read()
        except FileNotFoundError:
            continue
        codes = text.split("\n", 1)[0].split()[1:]
        ok = not any(code for code in codes)
        clean["projects"] += 1
        clean["without a diagnostic"] += ok
        for line in text.split("\n"):
            word = line.split(" ", 1)[0]
            facts[word] += 1
            if word == "due" and not line.endswith(": "):
                facts["due, not empty"] += 1
            if word == "keep" and not line.endswith("none"):
                facts["keep, a due day"] += 1
            if word == "keep" and line.endswith("none"):
                facts["keep, none"] += 1
            if word == "keep" and "ambiguous" in line:
                facts["keep, ambiguous"] += 1
            if word in ("factor", "recog"):
                facts[f"{word}, {'error' if 'Err(' in line else 'ok'}"] += 1
            if word == "ordinal" and "Some" in line:
                facts["ordinal, a number"] += 1
            if word == "promise":
                facts["promises kept"] += 1
            if word == "terms" and "equal-stretches false" in line:
                facts["stretches that differ in more than state"] += 1
            if word == "stretch" and line.endswith("waived"):
                facts["waived stretches"] += 1
        for form in forms[name]:
            if ok:
                held[form + " (clean)"] += 1
    width = max((len(form) for form in held), default=0)
    print(f"{len(forms)} projects; {clean['projects']} dumped, {clean['without a diagnostic']} with no diagnostic at all")
    for form in sorted(f for f in held if not f.endswith("(clean)")):
        print(f"  {form:<{width}} {held[form]:>5} {held[form + ' (clean)']:>5}")
    print("what the dumps asked:")
    for word, count in sorted(facts.items()):
        print(f"  {word:<{width}} {count:>8}")


def main(argv):
    if len(argv) >= 4 and argv[1] == "gen":
        slow = "--slow" in argv
        rest = [a for a in argv[2:] if a != "--slow"]
        forms = gen(rest[0], int(rest[1]), int(rest[2]) if len(rest) > 2 else 1, slow)
        total = Counter()
        for one in forms.values():
            total.update(one.keys())
        print(f"wrote {len(forms)} projects to {rest[0]}; projects holding each form:")
        for form, count in sorted(total.items()):
            print(f"  {form:<34} {count}")
        return 0
    if len(argv) >= 4 and argv[1] == "build":
        profile = DEBUG_PROFILE if "--debug-assertions" in argv else PROFILE
        print(build(argv[2], argv[3], new="--new" in argv, profile=profile))
        return 0
    if len(argv) >= 4 and argv[1] == "check":
        extra = ("--slow",) if "--slow" in argv else ()
        rest = [a for a in argv if a != "--slow"]
        return 1 if check(rest[2], rest[3], int(rest[4]) if len(rest) > 4 else 3, extra) else 0
    if len(argv) >= 4 and argv[1] == "dump":
        extra = ("--slow",) if "--slow" in argv else ()
        rest = [a for a in argv if a != "--slow"]
        jobs = int(rest[4]) if len(rest) > 4 else 3
        tag = rest[5] if len(rest) > 5 else "old"
        return 1 if dump(rest[2], rest[3], jobs, tag, extra) else 0
    if len(argv) >= 5 and argv[1] == "compare":
        return 1 if compare(argv[2], argv[3], argv[4]) else 0
    if len(argv) >= 5 and argv[1] == "mutate":
        new = "--new" in argv
        rest = [a for a in argv if a != "--new"]
        only = {int(n) for n in rest[5].split(",")} if len(rest) > 5 else None
        mutate(rest[2], rest[3], rest[4], only, new)
        return 0
    if len(argv) >= 3 and argv[1] == "cover":
        cover(argv[2], argv[3] if len(argv) > 3 else "old")
        return 0
    print(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
