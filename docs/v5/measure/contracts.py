#!/usr/bin/env python3
"""A generator of small projects full of contracts, and the oracle that holds the fold to a reference.

    contracts.py gen DIR N [SEED] [--slow]    write N projects into DIR (p0000/main.ax ...), and DIR/forms.json
    contracts.py build TREE OUT [--debug-assertions]
                                              build the dump (promises/) against the crates of TREE into OUT/
                                              (--debug-assertions: with the asserts the model and the core make)
    contracts.py dump BINARY DIR [JOBS] [TAG] [--slow]
                                              what BINARY says of every project: DIR/pNNNN/dump.TAG.txt (default TAG: old)
    contracts.py compare DIR A B              the projects whose dump.A.txt and dump.B.txt differ
    contracts.py check BINARY DIR [JOBS] [--slow]
                                              the dump's own verdict over every project
    contracts.py reproduce DIR A B            whether dump A (the frozen old one) is what dump B rebuilds of the old walkers
    contracts.py cover DIR [TAG]              what the projects hold, and what the dumps asked of them
    contracts.py mutate TREE WORK DIR [N,M..] the mutants of the code under test: each is built and must be caught (by the
                                              verdict, by the pinned factors, recognition windows and payments, or by tests)

What it is for. Lane K5a built a structure that says by arithmetic when a promise is due (a schedule whose due days and
ordinals are computed, not searched) and held it to the old walkers. Lane K5b moved the fold, the lowering and the reports to
it and deleted the walkers, so what judges it now is **a reference**: the plainest reading of the language, `calendar::due`
walked from the contract's first day with the days taken once, the waived ones left out and the nearest day within the
reach LANGUAGE section 7 says (the `grace`, else half a cadence of the schedule's own). The verdict (`check`) asks the
fold the same questions and equals the reference on every one: the days due in a window, an ordinal, the line each day
keeps, every occurrence a line kept, and every occurrence the monitor says was missed (and no other). The old walkers are
gone, and what they said is kept two ways. The old rule for the days, the ordinals and the line a day keeps is **rebuilt**
from `calendar::due` as they asked it, and `reproduce` holds the rebuild to a **frozen dump** (`dump.old.txt`, made from the
tree before K5b); the verdict then says, of every question the fold answers otherwise than the rebuilt rule did, why: the
reach (a line a whole cadence away kept a due day, and a schedule's own reach says no), a day the old walk lost (`on last`
after a waiver), days it found twice or out of order (`weekly on 15`, `monthly on 1, monday`), or a walk with no first day
(counted from 1970 now, from the beginning of time then: not compared). Any other difference is a failure. What the old
walkers said of a factor, a recognition window and a loan's payment has no second implementation: the frozen dump is what
`compare` holds the new dump to.

Each project is one to three contracts written by a seeded random generator (deterministic: the same SEED and N write the
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
later ones, the factor and the recognition window of each of 2,200 days, a loan's payment, and the fold's own promises.
`mutate` shows that this is enough: each mutant of the code under test is built and must be caught.
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


def build(tree, out, source=None, profile=PROFILE, features=()):
    """Builds the dump against the crates of TREE. Its own profile, because a dependency is built with the profile of
    the workspace that asks for it, and the tree's is `lto = thin`. SOURCE is where the dump's files are read from; with
    FEATURES, the dump of an earlier lane that has them (`new` for K5a's)."""
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
    command = ["cargo", "build", "--release", "--offline"] + (["--features", ",".join(features)] if features else [])
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


def kinds_of(path, tag):
    """The lines of a dump, by their first word, each kind in the order it was said. What the fold adds to the dump of
    an earlier tree (`missed`), and what its type makes true and a line used to say (`equal-stretches true`), is left
    out, so that a dump from before and one from after can be compared."""
    kinds = {}
    for line in open(os.path.join(path, f"dump.{tag}.txt")).read().split("\n"):
        line = line.replace(" equal-stretches true", "")
        word = line.split(" ", 1)[0]
        if word != "missed" and line != "== missed" and not line.startswith("== missed "):
            kinds.setdefault(word, []).append(line)
    # What the new dump rebuilt of the old walkers (`dueold`, `keepold`, `ordinalold`) is not a question of the old dump.
    return kinds


REBUILT = ("due", "keep", "ordinal")


def reproduce(directory, a, b):
    """Whether what the old walkers said (dump A, the frozen one) is what the new dump B rebuilds of them from
    `calendar::due` as they asked it: the days due, the lines kept and the ordinals, project by project. Where it is,
    the ways the fold differs from A are the ways the rebuilt rule and the reference differ, and nothing else."""
    different = []
    for path in projects(directory):
        old, new = kinds_of(path, a), kinds_of(path, b)
        for word in REBUILT:
            said = lambda lines: [line.partition(" ")[2] for line in lines]
            if said(old.get(word, [])) != said(new.get(word + "old", [])):
                different.append((path, word))
    for path, word in different[:5]:
        print(f"NOT REPRODUCED {os.path.basename(path)}: {word}")
    print(f"{len(projects(directory))} projects, {len(different)} kinds of line not reproduced")
    return len(different)


def compare(directory, a, b, show=3):
    """The projects whose dumps differ, and in which kinds of line. A dump that is not there (a mutant that hung) differs."""
    different, by_kind = [], Counter()
    for path in projects(directory):
        try:
            left, right = kinds_of(path, a), kinds_of(path, b)
        except FileNotFoundError:
            different.append((path, "not run", [], []))
            continue
        kinds = sorted(
            word for word in set(left) | set(right) if not word.endswith("old") and left.get(word) != right.get(word)
        )
        if kinds:
            word = kinds[0]
            before, after = left.get(word, []), right.get(word, [])
            at = next((i for i, (x, y) in enumerate(zip(before, after)) if x != y), min(len(before), len(after)))
            different.append((path, ", ".join(kinds), before[at:at + 1], after[at:at + 1]))
            by_kind.update(kinds)
    for path, kinds, before, after in different[:show]:
        print(f"DIFFERENT {path} in {kinds}:\n  {a}: {before}\n  {b}: {after}")
    for word, count in sorted(by_kind.items()):
        print(f"  {word:<12} differs in {count} projects")
    print(f"{len(projects(directory))} projects, {len(different)} differ")
    return len(different)



# ─── Mutants of the code under test ──────────────────────────────────────────────────────────────────────────

# (file, text, replacement, what it breaks). Each text occurs once in its file. A mutant is built into the dump and the
# verdict must fail on it, or the tests of the crate that holds it: if neither does, the corpus cannot tell the code from
# a wrong one, or the mutant is equivalent and says why.
DUES, SCHED = "crates/core/src/dues.rs", "crates/model/src/promise/schedule.rs"
PROMISE, RECKON = "crates/model/src/promise.rs", "crates/model/src/promise/reckon.rs"
ANNUITY, RESIDUAL = "crates/model/src/promise/annuity.rs", "crates/model/src/promise/residual.rs"
BOOK, MONITOR = "crates/model/src/book.rs", "crates/engine/src/monitor.rs"
LEDGER, TIMELINE = "crates/engine/src/ledger.rs", "crates/engine/src/timeline.rs"
MUTANTS = [
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
    (PROMISE, "i64::from(span.months).saturating_mul(31)", "i64::from(span.months).saturating_mul(30)", "the reach of a month is 30 days"),
    (BOOK, "waiver.is_some().then_some(days)", "waiver.is_none().then_some(days)", "the holes are the active stretches"),
    (PROMISE, "Cadence::TwiceMonthly => 31,", "Cadence::TwiceMonthly => 15,", "the cadence of twice monthly is 15 days"),
    (PROMISE, "terms.grace.map_or(cadence / 2, days)", "terms.grace.map_or(cadence, days)", "the reach is a whole cadence"),
    (PROMISE, "terms.grace.map_or(cadence / 2, days)", "terms.grace.map_or((cadence + 1) / 2, days)", "half a cadence is rounded up"),
    (PROMISE, "terms.grace.map_or(cadence / 2, days)", "terms.grace.map_or(cadence / 2, |_| cadence / 2)", "a grace is not read"),
    (SCHED, "schedule.nearest(day, schedule.reach())", "schedule.nearest(day, 1 << 20)", "a line any distance away keeps a due day"),
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
    (RESIDUAL, "let ordinal = schedule.before(day).max(began);", "let ordinal = began;", "a stream starts on its first day, not on the day asked"),
    (RESIDUAL, "let open = annuity.map_or(Qty::ZERO, |annuity| annuity.owed_after(ordinal - began));",
     "let open = annuity.map_or(Qty::ZERO, |annuity| annuity.principal().qty);", "a loan started late owes all of it"),
    (MONITOR, "self.misses.peek().filter(|&&Reverse((miss, _))| miss <= day)", "self.misses.peek().filter(|&&Reverse((miss, _))| miss < day)",
     "a miss on the day itself waits for the next fact"),
    (MONITOR, "due.0.checked_add(waiting.reach)?.checked_add(1)", "due.0.checked_add(waiting.reach)?.checked_add(0)", "a due day is missed on the last day it can be kept"),
    (MONITOR, "due.0.checked_add(waiting.reach)?.checked_add(1)", "due.0.checked_add(waiting.reach)?.checked_add(2)", "a due day is missed a day late"),
    (MONITOR, "if self.waiting[at].miss != Some(miss) {\n                continue;\n            }", "if false {\n                continue;\n            }",
     "a stale entry of the heap misses the day it was made for"),
    (MONITOR, "while !waiting.residual.is_done() && waiting.residual.ordinal() < ordinal {", "while !waiting.residual.is_done() && waiting.residual.ordinal() <= ordinal {",
     "the day a line kept is missed"),
    (MONITOR, "if !waiting.residual.is_done() && waiting.residual.ordinal() == ordinal {", "if false {", "a kept day is waited for again"),
    (MONITOR, "let at = self.waiting.partition_point(|waiting| waiting.key() < key);", "let at = self.waiting.partition_point(|waiting| waiting.key() <= key);",
     "a line settles the stream after its own"),
    (MONITOR, "monitor.expect(monitor.waiting.len() - 1);", "", "a stream is never missed"),
    (LEDGER, "        self.miss_through(limit.day);\n", "", "what is missed after the last fact is not found"),
    (LEDGER, "            self.miss_through(moment.day);\n", "", "a miss comes after the facts of the day it is missed on"),
    (LEDGER, "            .settle(&book.promises, contract_id, schedule, ordinal, |missed| record.promises.push(missed));", "            ;",
     "a kept line settles nothing"),
    (TIMELINE, "[first_flow, first_occurrence, first_assert, first_split, first_claim_change].into_iter().flatten().min()",
     "[first_flow, first_occurrence, first_assert, first_split, first_claim_change].into_iter().flatten().max()", "the book begins on its last day"),
]


def leave_out(tree, directory, names):
    """What a copy of TREE does not need: what is built, kept, or the book's own, and its goldens (`tests/` at the top;
    a crate's own are what `own_tests_fail` runs)."""
    top = os.path.samefile(directory, tree)
    return [name for name in names if name in ("target", ".git", ".claude", "docs", "examples") or (top and name == "tests")]


def own_tests_fail(source, work):
    """Whether the tests of `core`, `model` and the engine's monitor (their units, and `tests/promises.rs`) fail in
    SOURCE: what a mutant the dump's verdict does not catch has still to get past, because the corpus cannot say what a
    loan has left after a payment, what the days before a hole's last day are, or in what order the fold finds a miss."""
    env = dict(os.environ, CARGO_TARGET_DIR=os.path.join(work, "tests-target"))
    for package, targets in (
        ("axiom-core", ["--lib"]),
        ("axiom-model", ["--lib", "--test", "promises"]),
        ("axiom-engine", ["--lib", "monitor"]),
    ):
        run = subprocess.run(["cargo", "test", "--release", "--offline", "-p", package, *targets], cwd=source, env=env,
                             capture_output=True, text=True)
        if run.returncode:
            return True
    return False


PINNED = ("factor", "recog", "payment")


def pinned_differs(binary, directory, jobs=4, limit=120, enough=1):
    """Whether BINARY says of any project a factor, a recognition window or a payment that the fold under test
    (`dump.new.txt`) did not. Nothing in this repository is a second implementation of those (the old code that was is
    gone); what judges them is the frozen dump of that code, which `compare` holds `dump.new.txt` to, so a mutant that
    moves one is caught by being different from it."""
    different = []

    def pinned(text):
        return [line for line in text.split("\n") if line.split(" ", 1)[0] in PINNED]

    def one(path):
        if len(different) >= enough:
            return
        code, out, _ = run_dump(binary, path, (), limit)
        wanted = pinned(open(os.path.join(path, "dump.new.txt")).read())
        if code or pinned(out) != wanted:
            different.append(path)

    with ThreadPoolExecutor(jobs) as pool:
        list(pool.map(one, projects(directory)))
    return bool(different)


def mutate(tree, work, directory, only=None):
    """Builds each mutant into the dump. The verdict must fail on it, or the factors, recognition windows and payments it
    says must not be the fold's (`pinned_differs`), or the tests of the crate that holds it (which say what the corpus
    cannot). Mutants that are not caught are listed: each is either equivalent, and the report says why, or the corpus is
    too weak."""
    work = os.path.abspath(work)
    source = os.path.join(work, "tree")
    if not os.path.isdir(source):
        os.makedirs(work, exist_ok=True)
        shutil.copytree(os.path.abspath(tree), source, ignore=lambda at, names: leave_out(tree, at, names))
    out = os.path.join(work, "build")
    snapshot = os.path.join(work, "promises")
    if not os.path.isdir(snapshot):
        shutil.copytree(os.path.join(HERE, "promises"), snapshot)
    binary = build(source, out, source=snapshot)
    baseline = verdict(binary, directory, 4)[1]
    assert not baseline, f"the baseline fails its own verdict: {baseline[:2]}"
    assert not pinned_differs(binary, directory, 4), "the baseline says what dump.new.txt does not: dump it again as `new`"
    assert not own_tests_fail(source, work), "the baseline fails its own tests"
    limit = 20
    results = []
    for number, (path, old, replacement, what) in enumerate(MUTANTS):
        if only is not None and number not in only:
            continue
        target = os.path.join(source, path)
        original = open(target).read()
        assert original.count(old) == 1, f"mutant {number}: the text occurs {original.count(old)} times in {path}"
        open(target, "w").write(original.replace(old, replacement))
        by_tests = False
        try:
            binary = build(source, out, source=snapshot)
            caught = bool(verdict(binary, directory, 4, limit=limit, enough=3)[1])
            pinned = not caught and pinned_differs(binary, directory, 4, limit=limit)
            by_tests = not caught and not pinned and own_tests_fail(source, work)
            outcome = "killed" if caught else "killed by the dump" if pinned else "killed by the tests" if by_tests else "SURVIVED"
        except SystemExit:
            outcome = "does not build"
        finally:
            open(target, "w").write(original)
        results.append((number, outcome, what))
        print(f"mutant {number:02d} {outcome:<8} {what}", flush=True)
    summary = Counter(outcome for _, outcome, _ in results)
    print(f"{len(results)} mutants: {summary['killed']} killed by the verdict, {summary['killed by the dump']} by the dump, "
          f"{summary['killed by the tests']} by the tests, {summary['SURVIVED']} survived, "
          f"{summary['does not build']} did not build")
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
            if word == "missed":
                facts["promises missed"] += 1
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
        print(build(argv[2], argv[3], profile=profile))
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
    if len(argv) >= 5 and argv[1] == "reproduce":
        return 1 if reproduce(argv[2], argv[3], argv[4]) else 0
    if len(argv) >= 5 and argv[1] == "mutate":
        only = {int(n) for n in argv[5].split(",")} if len(argv) > 5 else None
        mutate(argv[2], argv[3], argv[4], only)
        return 0
    if len(argv) >= 3 and argv[1] == "cover":
        cover(argv[2], argv[3] if len(argv) > 3 else "old")
        return 0
    print(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
