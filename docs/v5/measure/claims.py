#!/usr/bin/env python3
"""A generator of small books full of claims and lots, and the oracle that holds two builds of the engine to them.

    claims.py gen DIR N [SEED]          write N projects into DIR (p0000/main.ax, p0000/spec.json ...), and DIR/forms.json
    claims.py build TREE OUT [--new]    build the dump (parcels/) against the crates of TREE into OUT/
                                        (--new: with `Run.written_off`, which a tree before lane K3c does not have)
    claims.py dump CLI DUMP DIR TAG     what a build says of every project: DIR/pNNNN/out.TAG.txt (the CLI's reports, and
                                        the dump's own account of every parcel, gain, part and diagnostic)
    claims.py compare DIR A B           the projects whose out.A.txt and out.B.txt differ, and whether the references
                                        say they should: the difference between two builds must be a difference of the model
    claims.py verdict DIR TAG old|new   what a build says of the claims, against the reference for the old or the new rules
    claims.py cover DIR                 what the projects hold
    claims.py mutate TREE WORK DIR [N,M..]
                                        the mutants of lane K3c's code: each is built and must be caught by the verdict
                                        or by `compare` against the baseline

What it is for. Lane K3c changes how a claim is settled and ends: `exact` is a relief policy and a claim place relieves by
it, the codes of a flow name the claims it settles, `waived` forgives a claim, a tab is a claim by its kind. Everything else
the engine does with parcels (securities in lots, the parts of an asset, a loan's debt, a bill owed) must not move, so
the proof is two things: a *reference* (this file) of what LANGUAGE §7 says for claims, which each build is held to, and a
*comparison* of the two builds on every other project, which must be byte for byte the same.

Each project is one family, written by a seeded random generator (deterministic: the same SEED and N write the same books),
with a little noise (ordinary flows) between its lines:

    tab      claims on parties (`ann owes me 300 USD due ... ^i1`, some itemized), written off on later days, some twice
    place    claims in a declared claim place (`ann -> owed 300 USD due ... #design ^i1`), settled by flows out of it: plain,
             with a code of its own that names a claim or none, with a written `[^code]` or `[day]`, equal amounts, larger
             than any claim, more than all of them; and written off
    boxes    claims of a commodity that has no policy (`ann -> owed-boxes 12 BOX ...`), settled in part
    lots     purchases of a security on several days and sales by a policy, a selector, or none
    assets   a purchase, improvements, a law that consumes each month, a sale
    debts    bills the owner owes (`me owes pge ...`), paid, and a loan paid in kind
    mixed    a claim family with a lots family beside it

The reference simulates the book it wrote: the claims it made, in the order the fold reaches them (a day's movements in the
order written, then its write-offs), and what each rule leaves open. It has two sets of rules. `old` is the engine at 36ead82:
a written `[^code]` or `[day]` filters, then the commodity's policy (FIFO for a currency, FIFO in effect for any other), a
flow's own codes are labels, `waived` does nothing, and a write-off in a declared place is refused. `new` is LANGUAGE §7:
a written selector filters, else the codes of the flow that some claim carries name the claims it may settle, then the exact
amount, then the oldest; `waived` forgives what is open; and warns when nothing is.
"""
import calendar
import datetime
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

HERE = os.path.dirname(os.path.abspath(__file__))
CRATES = ["core", "syntax", "model", "engine", "systems"]
PROFILE = "opt-level = 1\ncodegen-units = 16\nincremental = true"
TODAY = "2026-06-30"
REPORTS = ["check", "balance", "lots", "claims", "gains", "available"]

PRELUDE = """use std
base USD
entity me : person
entity ann : org
entity bob : org
entity cy : org
entity pge : org
entity bank : org
entity seller : org
entity buyer : org
commodity BOX : commodity
  precision 0
commodity VTI : stock
  precision 0
kind fifo-claim : asset
  claim
  select fifo
purpose design : income
purpose shopping : spending
account checking : bank
account owed : receivable
account queue : fifo-claim
account owed-boxes : receivable
account stockroom : asset
account brokerage : brokerage
account ira : brokerage
2025-12-31 market -> checking 5_000 USD
2025-12-31 BOX = 10 USD
"""

ASSET_KINDS = """kind rental : std/property
  has in-service date
  law depreciation
    each month
    consume {consume} USD
asset condo : rental
  in-service 2026-01-01
"""


# ─── Days ────────────────────────────────────────────────────────────────────────────────────────────────────


def day(rng, first=2, last=151):
    """A day of 2026 as an offset from January 1: from the 2nd to the 31st of May."""
    return datetime.date(2026, 1, 1) + datetime.timedelta(days=rng.randint(first - 1, last - 1))


def text(d):
    return d.isoformat()


def amount(n):
    return f"{n:_}" if n >= 1000 else str(n)


# ─── The reference ───────────────────────────────────────────────────────────────────────────────────────────


class Parcel:
    def __init__(self, code, qty, when, order):
        self.code, self.qty, self.when, self.order = code, qty, when, order


class Sim:
    """The claims a book makes and what each set of rules leaves open of them.

    `rules` is `old` or `new`. A place is `fifo` when its kind says so (`queue`), else it is `exact` under the new rules and
    the commodity's FIFO under the old ones. `plain` is what a place holds beyond its parcels: negative where a flow took
    more than the claims held."""

    def __init__(self, rules):
        self.rules = rules
        self.places = {}
        self.plain = Counter()
        self.empty = 0
        self.refused = 0
        self.ambiguous = False
        self.unit = {}
        self.forgiven = Counter()
        self.paid = {}

    def make(self, place, code, qty, when, order, unit="USD"):
        self.places.setdefault(place, []).append(Parcel(code, qty, when, order))
        self.unit[place] = unit

    def live(self, place):
        return sorted((p for p in self.places.get(place, []) if p.qty > 0), key=lambda p: (p.when, p.order))

    def settle(self, place, need, select=None, tail=()):
        live = self.live(place)
        candidates, filtered = live, select is not None
        if select is not None:
            kind, value = select
            candidates = [p for p in live if (p.code == value if kind == "code" else p.when == value)]
        elif self.rules == "new" and tail:
            named = [p for p in live if p.code in tail]
            candidates, filtered = (named, True) if named else (live, False)
        exact = self.rules == "new" and place != "queue"
        ordered = sorted(candidates, key=lambda p: (p.qty != need, p.when, p.order)) if exact else candidates
        if self.rules == "old" and self.unit.get(place) == "BOX" and not filtered:
            total = sum(p.qty for p in live)
            self.ambiguous |= len(live) > 1 and need < total and len({p.qty for p in live}) > 0
        left = need
        for parcel in ordered:
            take = min(parcel.qty, left)
            parcel.qty -= take
            left -= take
            if not left:
                break
        self.plain[place] -= left

    def pay(self, party, need, select, tail, label):
        """A payment from `party`: under the new rules it settles the claims on the party, as far as they go, and what
        remains is an ordinary flow; under the old a payment from a party settled nothing."""
        if self.rules == "old":
            return
        place = "tab:" + party
        live = self.live(place)
        candidates = live
        if select is not None:
            kind, value = select
            candidates = [p for p in live if (p.code == value if kind == "code" else p.when == value)]
        elif tail:
            named = [p for p in live if p.code in tail]
            candidates = named or live
        need = min(need, sum(p.qty for p in candidates))
        ordered = sorted(candidates, key=lambda p: (p.qty != need, p.when, p.order))
        taken, left = [], need
        for parcel in ordered:
            take = min(parcel.qty, left)
            if take:
                parcel.qty -= take
                left -= take
                taken.append((parcel, take))
            if not left:
                break
        self.paid[label] = taken

    def give_back(self, label):
        for parcel, qty in self.paid.pop(label, []):
            parcel.qty += qty

    def write_off(self, code, declared):
        if declared and self.rules == "old":
            self.refused += 1
            return
        if self.rules == "old":
            return
        found = [p for ps in self.places.values() for p in ps if p.code == code and p.qty > 0]
        if not found:
            self.empty += 1
        for p in found:
            self.forgiven[code] += p.qty
            p.qty = 0

    def open(self):
        left = Counter()
        for parcels in self.places.values():
            for p in parcels:
                if p.qty > 0:
                    left[p.code] += p.qty
        return left


def run_events(events, rules):
    """The events of a claims project as the fold reaches them: by day, a day's movements in the order written, then its
    write-offs."""
    sim = Sim(rules)
    days = sorted({e["day"] for e in events})
    for d in days:
        today = [e for e in events if e["day"] == d]
        for e in (e for e in today if e["kind"] == "return"):
            if rules == "new":
                sim.give_back(e["label"])
        for e in (e for e in today if e["kind"] not in ("writeoff", "return")):
            if e["kind"] == "make":
                sim.make(e["place"], e["code"], e["qty"] * 100 if e["unit"] == "USD" else e["qty"], d, e["order"], e["unit"])
            elif e["kind"] == "pay":
                select = tuple(e["select"]) if e.get("select") else None
                sim.pay(e["party"], e["qty"] * 100, select, e.get("tail", ()), e["label"])
            elif e["kind"] == "settle":
                scale = 100 if e["unit"] == "USD" else 1
                select = tuple(e["select"]) if e.get("select") else None
                sim.settle(e["place"], e["qty"] * scale, select, e.get("tail", ()))
        for e in (e for e in today if e["kind"] == "writeoff"):
            sim.write_off(e["code"], e["declared"])
    return sim


# ─── The generator ───────────────────────────────────────────────────────────────────────────────────────────

AMOUNTS = [100, 200, 200, 300, 300, 500]


class Book:
    """The lines of a project, dated, and the events the reference replays."""

    def __init__(self, rng):
        self.rng, self.lines, self.events, self.forms = rng, [], [], Counter()
        self.order = 0
        self.codes = 0

    def code(self, prefix="i"):
        self.codes += 1
        return f"{prefix}{self.codes}"

    def add(self, d, line, event=None):
        self.order += 1
        self.lines.append((d, self.order, line))
        if event is not None:
            event.update(day=d, order=self.order)
            self.events.append(event)

    def noise(self):
        for _ in range(self.rng.randint(0, 3)):
            n = self.rng.randint(5, 80)
            self.add(day(self.rng), f"checking -> cy {n} USD #shopping")

    def render(self, extra=""):
        out = PRELUDE + extra
        for d, _, line in sorted(self.lines, key=lambda l: (l[0], l[1])):
            out += f"{text(d)} {line}\n"
        return out


def tab_family(book):
    rng = book.rng
    claims = []
    for _ in range(rng.randint(2, 6)):
        d, code, party = day(rng, 2, 100), book.code(), rng.choice(["ann", "bob", "cy"])
        due = d + datetime.timedelta(days=rng.choice([10, 30, 45]))
        qty = rng.choice(AMOUNTS)
        if rng.random() < 0.25:
            first = rng.choice(AMOUNTS)
            line = f"{party} owes me due {text(due)} ^{code}\n  {amount(first)} USD #design\n  {amount(qty)} USD #design"
            qty += first
            book.forms["itemized"] += 1
        else:
            line = f"{party} owes me {amount(qty)} USD due {text(due)} ^{code}"
        book.add(d, line, dict(kind="make", place="tab:" + party, code=code, qty=qty, unit="USD"))
        claims.append((d, code, party, qty))
    for _ in range(rng.choice([0, 1, 1, 2, 3])):
        d0, code0, party, qty0 = rng.choice(claims)
        mine = [c for c in claims if c[2] == party]
        when = d0 + datetime.timedelta(days=rng.randint(1, 50))
        choice = rng.random()
        total = sum(c[3] for c in mine)
        need = (qty0 if choice < 0.3 else max(1, qty0 // 2) if choice < 0.5 else total + 50 if choice < 0.6
                else qty0 + rng.choice(AMOUNTS))
        label = book.code("pay-")
        select, tail, line_select, line_tail = None, [label], "", f" ^{label}"
        how = rng.random()
        if how < 0.3:
            tail = [label, code0]
            line_tail = f" ^{label} ^{code0}"
            book.forms["payment names a claim"] += 1
        elif how < 0.4:
            select, line_select = ["code", code0], f"[^{code0}]"
            book.forms["payment selects a claim"] += 1
        else:
            book.forms["payment from a party"] += 1
        book.add(when, f"{party}{line_select} -> checking {amount(need)} USD{line_tail}",
                 dict(kind="pay", party=party, qty=need, select=select, tail=tail, label=label))
        if rng.random() < 0.25:
            book.add(when + datetime.timedelta(days=rng.randint(1, 10)), f"^{label} returned",
                     dict(kind="return", label=label))
            book.forms["payment returned"] += 1
    for d, code, _, _ in claims:
        if rng.random() < 0.45:
            when = d + datetime.timedelta(days=rng.choice([0, 0, 5, 20, 40]))
            for _ in range(2 if rng.random() < 0.15 else 1):
                book.add(when, f'^{code} waived "off {code}"', dict(kind="writeoff", code=code, declared=False))
                book.forms["writeoff twice" if _ else "writeoff"] += 1
    book.forms["tab"] += 1


def place_family(book, place="owed", unit="USD", boxes=False):
    rng = book.rng
    made = []
    for _ in range(rng.randint(2, 5)):
        d, code, party = day(rng, 2, 60), book.code(), rng.choice(["ann", "bob"])
        due = d + datetime.timedelta(days=rng.choice([10, 30]))
        qty = rng.choice([5, 12, 12, 7] if boxes else AMOUNTS)
        tag = "" if boxes else " #design"
        book.add(d, f"{party} -> {place} {amount(qty)} {unit} due {text(due)}{tag} ^{code}",
                 dict(kind="make", place=place, code=code, qty=qty, unit=unit))
        made.append((d, code, qty))
    sink = "stockroom" if boxes else "checking"
    held = remaining = sum(q for _, _, q in made)
    for _ in range(rng.randint(1, 4)):
        when = day(rng, 61, 150)
        _, named, qty = rng.choice(made)
        choice = rng.random()
        if choice < 0.30:
            need = qty
            book.forms["settle equal to a claim"] += 1
        elif choice < 0.50:
            need = max(1, qty // 2)
            book.forms["settle part of a claim"] += 1
        elif choice < 0.62:
            need = held + rng.choice([1, 50])
            book.forms["settle more than all"] += 1
        elif choice < 0.80:
            need = rng.choice(AMOUNTS) + qty
            book.forms["settle across claims"] += 1
        else:
            need = rng.choice([q for _, _, q in made])
        if choice >= 0.50 and choice < 0.62:
            remaining = 0
        else:
            need = min(need, remaining)
            if need < 1:
                continue
            remaining -= need
        select, tail, line_select, line_tail = None, [], "", ""
        if not boxes:
            how = rng.random()
            if how < 0.25:
                select, line_select = ["code", named], f"[^{named}]"
                book.forms["settle by written code"] += 1
            elif how < 0.33:
                d0 = rng.choice(made)[0]
                select, line_select = ["day", text(d0)], f"[{text(d0)}]"
                book.forms["settle by written day"] += 1
            elif how < 0.60:
                codes = [named] + ([rng.choice(made)[1]] if rng.random() < 0.3 else [])
                tail, line_tail = codes, "".join(f" ^{c}" for c in dict.fromkeys(codes))
                book.forms["settle by the flow's codes"] += 1
            elif how < 0.70:
                line_tail = f" ^pay-{book.code('n')}"
                book.forms["settle with a label"] += 1
            if select and rng.random() < 0.6:
                tail = [rng.choice(made)[1]]
                line_tail = f" ^{tail[0]}"
                book.forms["selector and codes"] += 1
        to = f"{sink} {amount(need)} {unit}"
        book.add(when, f"{place}{line_select} -> {to}{line_tail}",
                 dict(kind="settle", place=place, qty=need, unit=unit, select=select, tail=tail))
    if not boxes:
        for d, code, _ in made:
            if rng.random() < 0.3:
                when = day(rng, 100, 150)
                book.add(when, f'^{code} waived "off {code}"', dict(kind="writeoff", code=code, declared=True))
                book.forms["writeoff in a place"] += 1
    book.forms["boxes" if boxes else "place"] += 1


def lots_family(book):
    rng = book.rng
    held, price = 0, 100
    for _ in range(rng.randint(2, 5)):
        d, qty, cost = day(rng, 2, 80), rng.randint(2, 20), rng.randint(80, 130)
        book.add(d, f"checking {qty * cost} USD -> brokerage {qty} VTI")
        held += qty
    book.add(datetime.date(2026, 1, 2), f"VTI = {price} USD")
    for _ in range(rng.randint(1, 3)):
        d, qty = day(rng, 82, 140), rng.randint(1, 12)
        qty = min(qty, held)
        if not qty:
            continue
        policy = rng.choice(["", "[fifo]", "[lifo]", "[hifo]", "[prorata]"])
        book.add(d, f"brokerage{policy} {qty} VTI -> checking {qty * rng.randint(90, 150)} USD")
        book.forms["sale " + (policy or "no policy")] += 1
        held -= qty
    if rng.random() < 0.4 and held > 1:
        book.add(day(rng, 82, 140), f"brokerage {held // 2} VTI -> ira")
        book.forms["transfer between accounts"] += 1
    book.forms["lots"] += 1


def assets_family(book):
    rng = book.rng
    book.add(datetime.date(2026, 1, 2), f"checking -> seller {amount(rng.choice([1000, 1200, 2000]))} USD #purchase of condo")
    for _ in range(rng.randint(0, 3)):
        book.add(day(rng, 20, 100), f"checking -> seller {rng.choice([60, 120, 300])} USD #improvement of condo")
    if rng.random() < 0.6:
        book.add(day(rng, 100, 140), f"buyer -> checking {amount(rng.choice([1100, 1500, 2500]))} USD #sale of condo")
        book.forms["asset sold"] += 1
    book.forms["assets"] += 1
    return ASSET_KINDS.format(consume=rng.choice([5, 10, 25]))


def debts_family(book):
    rng = book.rng
    for _ in range(rng.randint(1, 4)):
        d, code = day(rng, 2, 80), book.code("b")
        qty = rng.choice([50, 142, 300])
        book.add(d, f"me owes pge {amount(qty)} USD due {text(d + datetime.timedelta(days=30))} ^{code}")
        if rng.random() < 0.6:
            book.add(d + datetime.timedelta(days=rng.randint(3, 40)), f"checking -> pge {amount(qty)} USD ^{code}")
    book.add(datetime.date(2026, 1, 3), "bank owes me 2_000 USD due 2026-12-31 ^deposit")
    if rng.random() < 0.5:
        book.add(datetime.date(2026, 2, 1), "contract mortgage with bank\n  loan 3_000 USD on 2026-01-01 at 0% over 3m\n  monthly on 1 from checking\n  from 2026-02-01")
        for m in (2, 3, 4):
            if rng.random() < 0.7:
                book.add(datetime.date(2026, m, 1), "mortgage")
        book.forms["loan"] += 1
    book.forms["debts"] += 1


FAMILIES = [("tab", 22), ("place", 28), ("boxes", 10), ("lots", 12), ("assets", 8), ("debts", 10), ("mixed", 10)]


def project(seed, index):
    rng = random.Random(f"{seed}/{index}")
    book = Book(rng)
    family = rng.choices([f for f, _ in FAMILIES], [w for _, w in FAMILIES])[0]
    extra = ""
    if family == "tab":
        tab_family(book)
    elif family == "place":
        place_family(book, rng.choice(["owed", "owed", "queue"]))
    elif family == "boxes":
        place_family(book, "owed-boxes", "BOX", boxes=True)
    elif family == "lots":
        lots_family(book)
    elif family == "assets":
        extra = assets_family(book)
    elif family == "debts":
        debts_family(book)
    else:
        (tab_family if rng.random() < 0.5 else place_family)(book)
        lots_family(book)
    book.noise()
    spec = dict(family=family, events=book.events)
    return book.render(extra), spec, book.forms


def gen(directory, count, seed):
    os.makedirs(directory, exist_ok=True)
    forms = Counter()
    for index in range(count):
        source, spec, found = project(seed, index)
        path = os.path.join(directory, f"p{index:04d}")
        os.makedirs(path, exist_ok=True)
        with open(os.path.join(path, "main.ax"), "w") as handle:
            handle.write(source)
        with open(os.path.join(path, "spec.json"), "w") as handle:
            json.dump(spec, handle, default=str)
        forms.update(found)
        forms[f"family {spec['family']}"] += 1
    with open(os.path.join(directory, "forms.json"), "w") as handle:
        json.dump(forms, handle, indent=1, sort_keys=True)
    print(f"{count} projects written to {directory}")


# ─── Building and running ────────────────────────────────────────────────────────────────────────────────────


def build(tree, out, new=False, source=None):
    """Builds the dump against the crates of TREE, with its own profile (a dependency is built with the profile of the
    workspace that asks for it, and the tree's is `lto = thin`)."""
    os.makedirs(out, exist_ok=True)
    tree = os.path.abspath(tree)
    source = source or os.path.join(HERE, "parcels")
    deps = "\n".join(f'axiom-{c} = {{ path = "{tree}/crates/{c}" }}' for c in CRATES)
    manifest = (f'[package]\nname = "parcels-dump"\nversion = "0.0.0"\nedition = "2024"\n\n[workspace]\n\n'
                f'[features]\nnew = []\n\n[[bin]]\nname = "dump"\npath = "main.rs"\n\n[dependencies]\n{deps}\n\n'
                f'[profile.release]\n{PROFILE}\n')
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


MOST_MEMORY = 2 << 30


def limit_resources():
    resource.setrlimit(resource.RLIMIT_AS, (MOST_MEMORY, MOST_MEMORY))


def run(command, limit=60):
    with tempfile.TemporaryFile() as said:
        try:
            result = subprocess.run(command, stdout=said, stderr=subprocess.STDOUT, timeout=limit, preexec_fn=limit_resources)
        except subprocess.TimeoutExpired:
            return 124, f"no answer in {limit} seconds\n"
        said.seek(0)
        return result.returncode, said.read().decode(errors="replace")


def report(cli, dump, path, limit=60):
    """Everything a build says of a project: the CLI's reports and the dump's account."""
    main = os.path.join(path, "main.ax")
    out = []
    for name in REPORTS:
        code, said = run([cli, name, "-C", main, "--today", TODAY, "--color", "never"], limit)
        out.append(f"##### {name} exit {code}\n{said}")
    code, said = run([dump, main], limit)
    out.append(f"##### dump exit {code}\n{said}")
    return "".join(out)


def do_dump(cli, dump, directory, tag, jobs=4, limit=60):
    def one(path):
        with open(os.path.join(path, f"out.{tag}.txt"), "w") as handle:
            handle.write(report(cli, dump, path, limit))
    with ThreadPoolExecutor(jobs) as pool:
        list(pool.map(one, projects(directory)))
    print(f"{len(projects(directory))} projects dumped as {tag}")


def read(path, tag):
    with open(os.path.join(path, f"out.{tag}.txt")) as handle:
        return handle.read()


def section(output, name):
    found = re.search(rf"##### {name} exit \d+\n(.*?)(?=##### |\Z)", output, re.S)
    return found.group(1) if found else ""


def spec_of(path):
    with open(os.path.join(path, "spec.json")) as handle:
        return json.load(handle)


# ─── What a build says of the claims ─────────────────────────────────────────────────────────────────────────

CLAIM_PLACES = {"owed", "queue", "owed-boxes"}


def held(output):
    """What the dump says the claim places hold: the open quantity by code, and the plain balance by place, the tabs'
    claims by code, what was forgiven, and the diagnostics."""
    dump = section(output, "dump")
    open_ = Counter()
    plain = Counter()
    place = None
    for line in dump.split("\n"):
        found = re.match(r"holding (\S+) (\S+) plain (-?\d+)", line)
        if found:
            place = found.group(1)
            if place in CLAIM_PLACES:
                plain[place] += int(found.group(3))
            continue
        lot = re.match(r"  lot qty (-?\d+) basis .* codes \[(.*)\]$", line)
        if lot and place and (place in CLAIM_PLACES or place.startswith("tab(")):
            if place.startswith("tab(") and "owed-by-party" not in place:
                continue
            qty = int(lot.group(1))
            if qty > 0:
                for code in lot.group(2).split():
                    open_[code] += qty
    forgiven = Counter()
    for line in dump.split("\n"):
        found = re.match(r"written-off change \d+ \S+ (\S+) qty (\d+) basis (\d+)", line)
        if found:
            forgiven["total"] += int(found.group(2))
            forgiven["basis"] += int(found.group(3))
    diagnostics = Counter(re.findall(r"^diagnostic \w+ (\S+) ", dump, re.M))
    return open_, plain, forgiven, diagnostics, dump


def claims_view(output):
    """What `claims` lists as owed to you: the open quantity by code, as a reader of the report sees it."""
    said = section(output, "claims")
    left = Counter()
    for line in said.split("\n"):
        found = re.search(r"\^(\S+)\s+([\d,]+(?:\.\d+)?) (USD|BOX)\b", line)
        if found:
            qty = found.group(2).replace(",", "")
            left[found.group(1)] += round(float(qty) * 100) if found.group(3) == "USD" else int(float(qty))
    return left


def expected(spec, rules):
    sim = run_events(spec["events"], rules)
    open_ = sim.open()
    plain = Counter({place: v for place, v in sim.plain.items() if v})
    return sim, open_, plain


def conserved(dump):
    """What leaves a place arrives in another: in a project of claims and no exchange, the dollars and the boxes held by
    every place, the parties' and the market's included, add up to nothing."""
    total = Counter()
    unit = place = None
    for line in dump.split("\n"):
        found = re.match(r"holding (\S+) (\S+) plain (-?\d+)", line)
        if found:
            unit = found.group(2)
            total[unit] += int(found.group(3))
            continue
        lot = re.match(r"  lot qty (-?\d+) ", line)
        if lot and unit:
            total[unit] += int(lot.group(1))
    return {u: v for u, v in total.items() if v and u in ("USD", "BOX")}


def check_one(path, output, rules, reports=True):
    """The failures of one project's output against the reference for `rules`: a list of words."""
    spec = spec_of(path)
    if spec["family"] not in ("tab", "place", "boxes", "mixed"):
        return []
    if not any(e["kind"] in ("make", "settle", "writeoff", "pay") for e in spec["events"]):
        return []
    sim, want_open, want_plain = expected(spec, rules)
    open_, plain, forgiven, diagnostics, dump = held(output)
    failures = []
    if spec["family"] != "mixed" and conserved(dump):
        failures.append(f"value was not conserved: {conserved(dump)}")
    if +open_ != +want_open:
        failures.append(f"open {dict(+open_)} wanted {dict(+want_open)}")
    claim_plain = Counter({p: v for p, v in plain.items() if v})
    if claim_plain != want_plain:
        failures.append(f"plain {dict(claim_plain)} wanted {dict(want_plain)}")
    if rules == "new":
        if reports and +claims_view(output) != +want_open:
            failures.append(f"claims view {dict(+claims_view(output))} wanted {dict(+want_open)}")
        if diagnostics.get("claim-writeoff-empty", 0) != sim.empty:
            failures.append(f"empty write-offs {diagnostics.get('claim-writeoff-empty', 0)} wanted {sim.empty}")
        if forgiven["total"] != sum(sim.forgiven.values()):
            failures.append(f"forgiven {forgiven['total']} wanted {sum(sim.forgiven.values())}")
        if forgiven["basis"] != forgiven["total"]:
            failures.append(f"forgiven basis {forgiven['basis']} wanted {forgiven['total']}")
        if diagnostics.get("ambiguous-lots", 0) and spec["family"] != "mixed":
            failures.append("a claim place is ambiguous")
    else:
        if (diagnostics.get("ambiguous-lots", 0) > 0) != sim.ambiguous and spec["family"] == "boxes":
            failures.append(f"ambiguity {diagnostics.get('ambiguous-lots', 0)} wanted {sim.ambiguous}")
        said = re.findall(r"^model (.*)$", dump, re.M)
        refused = any(code in words.split() for words in said for code in ("claim-writeoff-target", "ambiguous-claim-reference"))
        if sim.refused and not refused:
            failures.append("a write-off in a place was not refused")
    return failures


def verdict(directory, tag, rules, show=5):
    failed = []
    for path in projects(directory):
        failures = check_one(path, read(path, tag), rules)
        if failures:
            failed.append((path, failures))
    print(f"{len(projects(directory))} projects held to the {rules} rules, {len(failed)} fail")
    for path, failures in failed[:show]:
        print("  ", os.path.basename(path), "; ".join(failures))
    return failed


MOVES_WITH_CLAIMS = CLAIM_PLACES | {"ann", "bob", "cy", "market", "stockroom"}


def holding_blocks(dump):
    """The `holding` lines of a dump with their parcels, by place."""
    blocks, place = {}, None
    for line in dump.split("\n"):
        found = re.match(r"holding (\S+) ", line)
        if found:
            place = found.group(1)
            blocks.setdefault(place, []).append(line)
        elif line.startswith("  lot") and place:
            blocks[place].append(line)
    return blocks


def unmoved(left, right):
    """What no rule of lane K3c may move, in whatever project: the gains, the adjustments, the parts of every asset, the
    carries, what each flow posted, and every place that is not a claim, a tab, or where a claim is paid from or to."""
    failures = []
    for name in ("gains", "adjustments", "assets", "carries", "posted"):
        pick = lambda dump: re.search(rf"== {name}.*?(?=\n== |\Z)", dump, re.S).group(0)
        if pick(left) != pick(right):
            failures.append(name)
    a, b = holding_blocks(left), holding_blocks(right)
    for place in sorted(set(a) | set(b)):
        if place in MOVES_WITH_CLAIMS or place.startswith("tab("):
            continue
        if a.get(place) != b.get(place):
            failures.append(f"holding {place}")
    return failures


def predicted_to_differ(spec):
    """Whether the two sets of rules leave a project in different states (so that the two builds should differ)."""
    if spec["family"] not in ("tab", "place", "boxes", "mixed"):
        return False
    old, new = expected(spec, "old"), expected(spec, "new")
    differ = +old[1] != +new[1] or {p: v for p, v in old[2].items() if v} != {p: v for p, v in new[2].items() if v}
    return differ or new[0].empty > 0 or old[0].refused > 0 or old[0].ambiguous


def may_differ(spec):
    """A payment that is returned puts its claims back, as open as they were, but a line that was used up comes back after
    the lines of its day that were not: the same claims, in another order. So a project with a return may differ from the
    baseline without the states differing."""
    return any(e["kind"] == "return" for e in spec["events"])


def compare(directory, a, b, show=5):
    """Projects whose outputs differ: each must be one the references say differs. Returns the unexplained."""
    unexplained, explained, same, missed = [], 0, 0, []
    for path in projects(directory):
        left, right = read(path, a), read(path, b)
        spec = spec_of(path)
        should, may = predicted_to_differ(spec), may_differ(spec)
        if left == right:
            same += 1
            if should:
                missed.append(path)
            continue
        moved = unmoved(section(left, "dump"), section(right, "dump"))
        if (should or may) and not moved:
            explained += 1
        else:
            unexplained.append(path)
            if moved:
                print(f"  {os.path.basename(path)} moved what no claim rule may: {moved}")
    print(f"{same} same, {explained} differ as the references say, {len(unexplained)} differ and should not, "
          f"{len(missed)} should differ and do not")
    for path in unexplained[:show]:
        print("  unexplained:", os.path.basename(path))
        lines = difflines(read(path, a), read(path, b))
        for line in lines[:6]:
            print("     ", line)
    for path in missed[:show]:
        print("  missed:", os.path.basename(path))
    return unexplained, missed


def difflines(left, right):
    import difflib
    return [l for l in difflib.unified_diff(left.split("\n"), right.split("\n"), lineterm="", n=0)
            if l[:1] in "+-" and l[:3] not in ("+++", "---")]


def cover(directory):
    forms = json.load(open(os.path.join(directory, "forms.json")))
    for name, count in sorted(forms.items()):
        print(f"{count:6}  {name}")


# ─── The mutants ─────────────────────────────────────────────────────────────────────────────────────────────

# (path, text, replacement, what). Each text occurs once in the file at the lane's commit. A mutant is caught when the
# dump of the mutated tree fails the verdict for the new rules, or moves what no claim rule may move, or differs from the
# baseline on a project that the references say is the same; or, failing that, when the tests of the crates fail.
MUTANTS = [
    ("crates/engine/src/lots.rs", "lot.qty == req.need && keep(lot)", "lot.qty >= req.need && keep(lot)",
     "exact takes a lot of at least the need"),
    ("crates/engine/src/lots.rs", "(b.qty == need).cmp(&(a.qty == need)).then(a.source.cmp(&b.source))",
     "(a.qty == need).cmp(&(b.qty == need)).then(a.source.cmp(&b.source))", "scanning puts the exact lots last"),
    ("crates/engine/src/lots.rs", "(b.qty == need).cmp(&(a.qty == need)).then(a.source.cmp(&b.source))",
     "(b.qty == need).cmp(&(a.qty == need)).then(b.source.cmp(&a.source))", "of equal lots the newest, when scanning"),
    ("crates/engine/src/lots.rs", "            if policy == Some(Policy::Exact) {\n                self.take_exact(&mut left, of_colour, req, out);\n            }\n",
     "", "the ordered path never takes the exact lot"),
    ("crates/engine/src/lots.rs", "self.holding.lots[self.first..].iter().position(|lot| lot.qty == req.need && keep(lot))",
     "self.holding.lots[self.first..].iter().rposition(|lot| lot.qty == req.need && keep(lot))",
     "of equal lots the newest, when ordered"),
    ("crates/engine/src/lots.rs", "by = made.peek().is_none() || made.any(|txn| lot.txn.source_txn() == Some(txn))",
     "by = made.peek().is_none() || made.any(|txn| lot.txn.source_txn() != Some(txn))", "a write-off selects the other transactions"),
    ("crates/engine/src/lots.rs", "pool[lot.codes.header].contains(&code) || pool[lot.codes.local].contains(&code)",
     "pool[lot.codes.header].contains(&code)", "a code is carried by a transaction's header only"),
    ("crates/engine/src/traits.rs", "let select = book.select(place).or(claim.then_some(Policy::Exact));", "let select = book.select(place);",
     "a claim place has no default policy"),
    ("crates/engine/src/traits.rs", "let select = book.select(place).or(claim.then_some(Policy::Exact));",
     "let select = book.select(place).or(claim.then_some(Policy::Lifo));", "a claim place is LIFO by default"),
    ("crates/engine/src/traits.rs", "let select = book.select(place).or(claim.then_some(Policy::Exact));",
     "let select = claim.then_some(Policy::Exact).or(book.select(place));", "a claim place's own policy is overridden"),
    ("crates/engine/src/post.rs", "&& !chosen) else { return false };", "&& chosen) else { return false };",
     "a flow's codes name claims only when it chose by a code or a day"),
    ("crates/engine/src/post.rs", "matches!(select, Select::Code(_) | Select::Range(_))", "matches!(select, Select::Range(_))",
     "a written code does not stop the flow's codes naming claims"),
    ("crates/engine/src/post.rs", "matches!(select, Select::Code(_) | Select::Range(_))", "matches!(select, Select::Code(_))",
     "a written day does not stop the flow's codes naming claims"),
    ("crates/engine/src/post.rs", ".filter(|&code| slot.carries(code, &book.codes))", ".filter(|&code| slot.carries(code, &book.codes) || true)",
     "a code that names no claim selects nothing"),
    ("crates/engine/src/post.rs", "[m.code_runs.header, m.code_runs.local]", "[m.code_runs.header]", "the flow's own line's codes are not read"),
    ("crates/engine/src/claims.rs", "let made = [Select::Txn(change.target)];", "let made: [Select; 0] = [];",
     "a write-off forgives every claim in the place"),
    ("crates/engine/src/claims.rs", "        self.world.holdings.credit(back, unit, open);\n", "", "the forgiven value goes nowhere"),
    ("crates/engine/src/claims.rs", "if forgiven == 0 {", "if forgiven == 1 {", "an empty write-off is said when one parcel was forgiven"),
    ("crates/engine/src/claims.rs", "basis: s.basis,", "basis: Qty::ZERO,", "a write-off records no basis"),
    ("crates/engine/src/claims.rs", "qty: s.qty,", "qty: open,", "a write-off records the whole open amount for each parcel"),
    ("crates/model/src/lower/record.rs", "let claimed = matches!(used, CodeUse::ClaimWaiver) && self.claims.by_code.contains_key(&symbol);",
     "let claimed = false;", "a code on a claim and its payment is ambiguous for a write-off"),
    ("crates/model/src/lower/statements.rs", "if !flows().any(|flow| book.makes_claim(flow)) {", "if !flows().any(|flow| book.is_claim(flow.to)) {",
     "a claim place funded by the owner's own money is a claim on a party"),
    ("crates/model/src/said.rs", "self.is_claim(flow.to) && self.places[flow.from].class == Class::Outside", "self.is_claim(flow.to)",
     "a claim made of the owner's own money can be forgiven"),
    ("crates/model/src/declare.rs", "    world.say(kinds.claim, builtin::CLAIM, true);\n", "", "the kind of a tab does not say `claim`"),
    ("crates/report/src/history.rs", "        && by(book.claim_changes.last().map(|change| change.day))\n", "",
     "a view dated before a write-off is the run's final state"),
    ("crates/engine/src/settle.rs", "let money = m.target.class == Class::Asset && !self.plan.traits.place(m.to).claim;",
     "let money = m.target.class == Class::Asset;", "a flow that makes a claim settles the claims before it"),
    ("crates/engine/src/settle.rs", "let need = open.min(m.out.qty);", "let need = open;", "a payment settles more than it paid"),
    ("crates/engine/src/settle.rs", "let named = self.name_claims(m, tab);", "let named = false;",
     "the codes of a payment from a party name no claim"),
    ("crates/engine/src/settle.rs", "        parcels.iter().for_each(|&parcel| slot.land_with_codes(parcel, false, codes));\n", "",
     "a returned payment does not open its claims"),
    ("crates/engine/src/settle.rs", "        self.world.holdings.credit(m.to, unit, -reopened);\n", "",
     "a returned payment makes value when it opens its claims"),
    ("crates/engine/src/post.rs", "self.world.holdings.credit(m.from, unit, settled - m.out.qty);",
     "self.world.holdings.credit(m.from, unit, -m.out.qty);", "a payment that settles claims makes value"),
    ("crates/engine/src/traits.rs", "partition_point(|&(found, by, _)| (found, by) < (party, owner))",
     "partition_point(|&(found, by, _)| (found, by) <= (party, owner))", "the tab of a party is not found"),
    ("crates/engine/src/lots.rs", "lots.iter().take_while(|lot| lot.txn == lots[0].txn).count()", "1",
     "the lines of an invoice are claims of their own"),
]


def leave_out(names):
    return [name for name in names if name in ("target", ".git", ".claude", "docs", "examples", "tests")]


def own_tests_fail(source, work):
    """Whether the tests of the crates lane K3c changes fail beyond the three failures the integration branch has."""
    env = dict(os.environ, CARGO_TARGET_DIR=os.path.join(work, "tests-target"))
    run = subprocess.run(["cargo", "test", "--release", "--offline", "--no-fail-fast", "-p", "axiom-engine", "-p", "axiom-model",
                          "-p", "axiom-report"], cwd=source, env=env, capture_output=True, text=True)
    if "test result" not in run.stdout:
        raise SystemExit("the tests did not run: " + run.stderr[-300:])
    failed = set(re.findall(r"^test (\S+) \.\.\. FAILED", run.stdout, re.M))
    known = {"tests::a_prorata_place_realizes_only_the_lots_share_and_deferrals_merge_into_one_lot",
             "source_tests::a_context_forecast_keeps_historical_and_same_day_obligations_once",
             "source_tests::native_loan_forecast_stops_after_the_typed_principal_is_repaid"}
    return bool(failed - known)


def dump_only(binary, directory, tag, jobs=4, limit=60):
    def one(path):
        code, said = run([binary, os.path.join(path, "main.ax")], limit)
        with open(os.path.join(path, f"out.{tag}.txt"), "w") as handle:
            handle.write(f"##### dump exit {code}\n{said}")
    with ThreadPoolExecutor(jobs) as pool:
        list(pool.map(one, projects(directory)))


def caught(directory, base, tag):
    """What the dump of a build fails: the verdict for the new rules (without the reports), what no claim rule may move,
    and any difference from the baseline where the references say there is none."""
    failures = []
    for path in projects(directory):
        output, spec = read(path, tag), spec_of(path)
        found = check_one(path, output, "new", reports=False)
        left, right = section(read(path, base), "dump"), section(output, "dump")
        found += unmoved(left, right)
        if left != right and not predicted_to_differ(spec) and not may_differ(spec):
            found.append("differs from the baseline where the references say it should not")
        if found:
            failures.append((path, found))
    return failures


def mutate(tree, work, directory, only=None):
    work = os.path.abspath(work)
    source = os.path.join(work, "tree")
    if not os.path.isdir(source):
        os.makedirs(work, exist_ok=True)
        shutil.copytree(os.path.abspath(tree), source, ignore=lambda at, names: leave_out(names) if os.path.samefile(at, tree) else [])
    out, snapshot = os.path.join(work, "build"), os.path.join(work, "parcels")
    if not os.path.isdir(snapshot):
        shutil.copytree(os.path.join(HERE, "parcels"), snapshot)
    binary = build(source, out, new=True, source=snapshot)
    dump_only(binary, directory, "clean")
    clean = caught(directory, "base", "clean")
    assert not clean, f"the unmutated tree fails the oracle: {clean[:2]}"
    results = []
    for number, (path, old, replacement, what) in enumerate(MUTANTS):
        if only is not None and number not in only:
            continue
        target = os.path.join(source, path)
        original = open(target).read()
        assert original.count(old) == 1, f"mutant {number}: the text occurs {original.count(old)} times in {path}"
        open(target, "w").write(original.replace(old, replacement))
        tag = f"m{number:02d}"
        outcome = "SURVIVED"
        try:
            binary = build(source, out, new=True, source=snapshot)
            dump_only(binary, directory, tag)
            if caught(directory, "base", tag):
                outcome = "killed"
            elif own_tests_fail(source, work):
                outcome = "killed by the tests"
        except SystemExit:
            outcome = "does not build"
        finally:
            open(target, "w").write(original)
            for project in projects(directory):
                try:
                    os.remove(os.path.join(project, f"out.{tag}.txt"))
                except FileNotFoundError:
                    pass
        results.append((number, outcome, what))
        print(f"mutant {number:02d} {outcome:<20} {what}", flush=True)
    summary = Counter(outcome for _, outcome, _ in results)
    print(f"{len(results)} mutants: {summary['killed']} killed, {summary['killed by the tests']} killed by the tests, "
          f"{summary['SURVIVED']} survived, {summary['does not build']} did not build")
    with open(os.path.join(work, "mutants.txt"), "w") as handle:
        for number, outcome, what in results:
            handle.write(f"{number:02d} {outcome} {what}\n")


def main(argv):
    command = argv[1] if len(argv) > 1 else ""
    if command == "gen":
        gen(argv[2], int(argv[3]), int(argv[4]) if len(argv) > 4 else 1)
    elif command == "build":
        print(build(argv[2], argv[3], new="--new" in argv))
    elif command == "dump":
        do_dump(argv[2], argv[3], argv[4], argv[5])
    elif command == "compare":
        unexplained, missed = compare(argv[2], argv[3], argv[4])
        return 1 if unexplained or missed else 0
    elif command == "verdict":
        return 1 if verdict(argv[2], argv[3], argv[4]) else 0
    elif command == "cover":
        cover(argv[2])
    elif command == "mutate":
        only = {int(n) for n in argv[5].split(",")} if len(argv) > 5 else None
        mutate(argv[2], argv[3], argv[4], only)
    else:
        print(__doc__)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
