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
                                        the mutants of lanes K3c's and K3d's code: each is built and must be caught by the
                                        verdict or by `compare` against the baseline

What it is for. Lane K3c changes how a claim is settled and ends: `exact` is a relief policy and a claim place relieves by
it, the codes of a flow name the claims it settles, `waived` forgives a claim, a tab is a claim by its kind. Everything else
the engine does with parcels (securities in lots, the parts of an asset, a loan's debt, a bill owed) must not move, so
the proof is two things: a *reference* (this file) of what LANGUAGE §7 says for claims, which each build is held to, and a
*comparison* of the two builds on every other project, which must be byte for byte the same.

Each project is one family, written by a seeded random generator (deterministic: the same SEED and N write the same books),
with a little noise (ordinary flows) between its lines:

    tab      claims on parties (`ann owes me 300 USD due ... ^i1`, some itemized), paid by the party (`ann -> checking 300
             USD ^p1`, with a claim's code or `[^code]` or neither, of the claim's size, half of it, or more than all of
             them), some of the payments written as a split with a leg to a third party (`ann -> 300 USD` with the legs
             `checking 280 USD` and `stripe 20 USD`, in either order), some payments to a third party and splits that
             reach no owner, some payments returned, and claims written off on later days, some twice
    place    claims in a declared claim place (`ann -> owed 300 USD due ... #design ^i1`), settled by flows out of it: plain,
             with a code of its own that names a claim or none, with a written `[^code]` or `[day]`, equal amounts, larger
             than any claim, more than all of them; and written off
    boxes    claims of a commodity that has no policy (`ann -> owed-boxes 12 BOX ...`), settled in part
    lots     purchases of a security on several days and sales by a policy, a selector, or none
    assets   a purchase, improvements, a law that consumes each month, a sale
    debts    bills the owner owes (`me owes pge ...`, some itemized, with a purpose or none), paid by the exact amount, by a
             code, oldest first, more than they come to, written off, returned; bills a declared `payable` place holds and
             payments into it; and a loan paid in kind and a deposit that is owed to the owner
    mixed    a claim family with a lots family beside it

The reference simulates the book it wrote: the claims it made, in the order the fold reaches them (a day's movements in the
order written, then its write-offs), and what each rule leaves open. It has three sets of rules. `old` is the engine at 36ead82:
a written `[^code]` or `[day]` filters, then the commodity's policy (FIFO for a currency, FIFO in effect for any other), a
flow's own codes are labels, a payment from a party settles nothing, `waived` does nothing, and a write-off in a declared
place is refused. `k3c` is lane K3c's (the engine at 368e5e8): a written selector filters, else the codes of the flow that some
claim carries name the claims it may settle, then the exact amount, then the oldest, for a flow out of a claim place and for a
payment from a party alike, a returned payment opens what it settled, and `waived` forgives what is open and warns when nothing
is; of a payment written as a split, only the legs that reach the owner settle, each by its own amount. `new` is lane K3d's
(LANGUAGE §7 for a payment in all): the legs of a statement that pay the owner, and those that pay a third party beside them, are
one payment, each settling in its turn, "exactly the flow's" being what the party still pays from that leg on; a statement that
pays no owner settles nothing.
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
REPORTS = ["check", "balance", "lots", "claims", "gains", "available", "flow"]

# What a book that does not say means by `books`: the build under test says (K3d's first commit, accrual; its flip, cash).
DEFAULT_BOOKS = os.environ.get("CLAIMS_DEFAULT_BOOKS", "accrual")

# What each purpose's law counts, by tally.
TALLIES = {"design": "receipts", "retail": "retail-receipts", "electricity": "bill-spending"}
NAMED = {tally: purpose for purpose, tally in TALLIES.items()}


PRELUDE = """use std
base USD
entity me : person
{books}entity ann : org
entity bob : org
entity cy : org
entity stripe : org
entity pge : org
entity bank : org
entity ben : org
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
  law receipts
    on flow
    count amount as receipts
purpose retail : income
  law retail-receipts
    on flow
    count amount as retail-receipts
purpose shopping : spending
purpose electricity : spending
  law bill-law
    on flow
    count amount as bill-spending
account checking : bank
account owed : receivable
account owed-to-ben : payable
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
    """A claim: its lines (an itemized invoice has several), each with what it was made for, taken in order."""

    def __init__(self, code, lines, when, order):
        self.code, self.lines, self.when, self.order = code, [list(line) for line in lines], when, order

    @property
    def qty(self):
        return sum(qty for qty, _ in self.lines)

    def take(self, wanted):
        """Takes `wanted` from the first lines that have it: (line, purpose, quantity) for each."""
        taken = []
        for at, line in enumerate(self.lines):
            part = min(line[0], wanted)
            if part > 0:
                line[0] -= part
                wanted -= part
                taken.append((at, line[1], part))
        return taken

    def restore(self, taken):
        for at, _, part in taken:
            self.lines[at][0] += part


class Sim:
    """The claims a book makes and what each set of rules leaves open of them.

    `rules` is `old`, `k3c` or `new`; the last two differ only in what a split payment settles. A place is `fifo` when its
    kind says so (`queue`), else it is `exact` under the later rules and the commodity's FIFO under the old ones. `plain` is what a place holds beyond its parcels: negative where a flow took
    more than the claims held."""

    def __init__(self, rules, books="accrual"):
        self.rules = rules
        self.books = books
        self.modern = rules != "old"
        self.places = {}
        self.plain = Counter()
        self.empty = 0
        self.refused = 0
        self.ambiguous = False
        self.unit = {}
        self.forgiven = Counter()
        self.paid = {}
        self.fired = {}
        self.counted = []
        self.net = Counter()
        self.replaced = Counter()

    def make(self, place, code, lines, when, order, unit="USD"):
        self.places.setdefault(place, []).append(Parcel(code, lines, when, order))
        self.unit[place] = unit

    def live(self, place):
        return sorted((p for p in self.places.get(place, []) if p.qty > 0), key=lambda p: (p.when, p.order))

    def settle(self, place, need, select=None, tail=()):
        """A flow out of a claim place; says what it took, by the purpose of each line: [(purpose, quantity)]."""
        live = self.live(place)
        candidates, filtered = live, select is not None
        if select is not None:
            kind, value = select
            candidates = [p for p in live if (p.code == value if kind == "code" else p.when == value)]
        elif self.modern and tail:
            named = [p for p in live if p.code in tail]
            candidates, filtered = (named, True) if named else (live, False)
        exact = self.modern and place != "queue"
        ordered = sorted(candidates, key=lambda p: (p.qty != need, p.when, p.order)) if exact else candidates
        if self.rules == "old" and self.unit.get(place) == "BOX" and not filtered:
            total = sum(p.qty for p in live)
            self.ambiguous |= len(live) > 1 and need < total and len({p.qty for p in live}) > 0
        left, taken = need, []
        for parcel in ordered:
            took = parcel.take(min(parcel.qty, left))
            left -= sum(part for _, _, part in took)
            taken += [(purpose, part) for _, purpose, part in took]
            if not left:
                break
        self.plain[place] -= left
        return taken

    def pay_into(self, place, qty, tail):
        """A payment into a place that holds what the owner owes: it settles the bills its codes name, else the one whose open
        amount is exactly what it settles, else the oldest, and what is more than is owed is a credit with the party (a positive
        balance). Says what it took, by the purpose of each line."""
        live = self.live(place)
        candidates = [p for p in live if p.code in tail] or live
        need = min(qty, sum(p.qty for p in candidates))
        left, taken = need, []
        for parcel in sorted(candidates, key=lambda p: (p.qty != need, p.when, p.order)):
            took = parcel.take(min(parcel.qty, left))
            left -= sum(part for _, _, part in took)
            taken += [(purpose, part) for _, purpose, part in took]
        self.plain[place] += qty - need
        return taken

    def pay(self, party, legs, select, tail, label, place=None):
        """A payment from `party`, written as `legs` ((`owner` | `third`, quantity, purpose) in the order written): under the
        later rules it settles the claims on the party, as far as they go, and what remains is an ordinary flow; under the old a
        payment from a party settled nothing. Under `k3c` the legs that reach the owner each settle by their own amount;
        under `new` the legs that reach the owner and those that pay someone else beside them are one payment, each leg
        settling in its turn and "exactly the flow's" being what the party still pays from it on. Says what each leg took
        of the claims, by the purpose of each line."""
        if self.rules == "old":
            return [(kind, qty, purpose, []) for kind, qty, purpose in legs]
        place = place or "tab:" + party
        in_all = self.rules == "new" and any(kind == "owner" for kind, _, _ in legs)
        rest = sum(qty for kind, qty, _ in legs if kind == "owner" or in_all)
        taken, settled = [], []
        for kind, qty, purpose in legs:
            if kind != "owner" and not in_all:
                settled.append((kind, qty, purpose, []))
                continue
            live = self.live(place)
            candidates = live
            if select is not None:
                how, value = select
                candidates = [p for p in live if (p.code == value if how == "code" else p.when == value)]
            elif tail:
                named = [p for p in live if p.code in tail]
                candidates = named or live
            open_ = sum(p.qty for p in candidates)
            need = min(qty, open_)
            exact = min(open_, rest) if self.rules == "new" else need
            ordered = sorted(candidates, key=lambda p: (p.qty != exact, p.when, p.order))
            left, leg = need, []
            for parcel in ordered:
                took = parcel.take(min(parcel.qty, left))
                left -= sum(part for _, _, part in took)
                leg += [(purpose, part) for _, purpose, part in took]
                taken.append((parcel, took))
                if not left:
                    break
            rest -= qty
            settled.append((kind, qty, purpose, leg))
        self.paid[label] = taken
        return settled

    def give_back(self, label):
        for parcel, took in self.paid.pop(label, []):
            parcel.restore(took)

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
            for _, purpose, part in p.take(p.qty):
                if self.rules == "new" and self.books == "accrual" and purpose in TALLIES:
                    self.net[purpose] -= part

    def open(self):
        left = Counter()
        for parcels in self.places.values():
            for p in parcels:
                if p.qty > 0:
                    left[p.code] += p.qty
        return left

    # What the purposes' laws counted: the recognition of LANGUAGE §7. Each count is (tally, day, quantity).

    def count(self, day, purpose, qty):
        if purpose in TALLIES and qty > 0:
            self.counted.append((TALLIES[purpose], str(day), qty))
            self.net[purpose] += qty

    def recognizes(self, day, claims):
        """The claims a flow settled, counted as their purposes in cash books: one piece for each purpose."""
        if self.rules == "new" and self.books == "cash":
            for purpose in dict.fromkeys(purpose for purpose, _ in claims):
                self.count(day, purpose, sum(part for found, part in claims if found == purpose))

    def made(self, day, lines):
        """A claim made: counted when it is made, but in cash books after the later rules."""
        if self.rules != "new" or self.books == "accrual":
            for qty, purpose in lines:
                self.count(day, purpose, qty)

    def paid_by(self, day, legs, label):
        """What a payment counted, leg by leg: its own purpose for what it moved, less what the claims it settled were when
        it reached the owner, and the claims' purposes in cash books. Says the pieces, for its return."""
        pieces = []
        for kind, qty, purpose, claims in legs:
            before = len(self.counted)
            replaced = kind == "owner" and self.rules == "new"
            replaced = sum(part for found, part in claims if found in TALLIES) if replaced else 0
            self.replaced[label] += replaced
            self.count(day, purpose, qty - replaced)
            self.recognizes(day, claims)
            pieces += self.counted[before:]
        return pieces

    def returned(self, label):
        """A payment that is returned runs backwards, and what its laws count is counted again: on the days the payment
        was recognized, which a return does not change. `flow` does not show a flow that was returned at all."""
        pieces = self.fired.pop(label, [])
        self.replaced.pop(label, None)
        self.counted += pieces
        for tally, _, qty in pieces:
            self.net[NAMED[tally]] -= qty


def run_events(events, rules, books="accrual"):
    """The events of a claims project as the fold reaches them: by day, a day's movements in the order written, then its
    write-offs."""
    sim = Sim(rules, books)
    days = sorted({e["day"] for e in events})
    for d in days:
        today = [e for e in events if e["day"] == d]
        for e in (e for e in today if e["kind"] == "return"):
            if rules != "old":
                sim.give_back(e["label"])
                sim.returned(e["label"])
        for e in (e for e in today if e["kind"] not in ("writeoff", "return")):
            scale = 100 if e.get("unit", "USD") == "USD" else 1
            if e["kind"] == "make":
                lines = [(qty * scale, purpose) for qty, purpose in e["lines"]]
                sim.make(e["place"], e["code"], lines, d, e["order"], e["unit"])
                sim.made(d, lines)
            elif e["kind"] == "pay":
                select = tuple(e["select"]) if e.get("select") else None
                legs = [(kind, qty * 100, purpose) for kind, qty, purpose in e["legs"]]
                settled = sim.pay(e["party"], legs, select, e.get("tail", ()), e["label"], e.get("place"))
                sim.fired[e["label"]] = sim.paid_by(d, settled, e["label"])
            elif e["kind"] == "into":
                sim.recognizes(d, sim.pay_into(e["place"], e["qty"] * 100, e.get("tail", ())))
            elif e["kind"] == "settle":
                select = tuple(e["select"]) if e.get("select") else None
                sim.recognizes(d, sim.settle(e["place"], e["qty"] * scale, select, e.get("tail", ())))
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
        self.books = rng.choice(["cash", "accrual", "cash", "accrual", None])

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
        out = PRELUDE.format(books=f"  books {self.books}\n" if self.books else "") + extra
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
        purposes = ["design", "design", "retail", None]
        if rng.random() < 0.25:
            first, kinds = rng.choice(AMOUNTS), [rng.choice(purposes), rng.choice(purposes)]
            lines = [[first, kinds[0]], [qty, kinds[1]]]
            tag = lambda purpose: f" #{purpose}" if purpose else ""
            line = (f"{party} owes me due {text(due)} ^{code}\n  {amount(first)} USD{tag(kinds[0])}\n"
                    f"  {amount(qty)} USD{tag(kinds[1])}")
            qty += first
            book.forms["itemized"] += 1
        else:
            kind = rng.choice(purposes)
            lines = [[qty, kind]]
            line = f"{party} owes me {amount(qty)} USD due {text(due)}{f' #{kind}' if kind else ''} ^{code}"
            book.forms["claim with a purpose" if kind else "claim with no purpose"] += 1
        book.add(d, line, dict(kind="make", place="tab:" + party, code=code, qty=qty, lines=lines, unit="USD"))
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
        own = rng.choice([None, None, "design", "retail"])
        ptag = f" #{own}" if own else ""
        legs, body = [["owner", need, own]], f" -> checking {amount(need)} USD{ptag}{line_tail}"
        if rng.random() < 0.35 and need > 40:
            fee = rng.choice([10, 20, 30])
            legs = [["owner", need - fee, own], ["third", fee, "shopping"]]
            if rng.random() < 0.3:
                legs.reverse()
            to = {"owner": lambda q: f"checking {amount(q)} USD", "third": lambda q: f"stripe {amount(q)} USD #shopping"}
            body = f" -> {amount(need)} USD{ptag}{line_tail}" + "".join(f"\n  {to[kind](q)}" for kind, q, _ in legs)
            book.forms["payment written as a split"] += 1
        book.forms["payment with a purpose" if own else "payment with no purpose"] += 1
        book.add(when, f"{party}{line_select}{body}",
                 dict(kind="pay", party=party, legs=legs, select=select, tail=tail, label=label))
        if rng.random() < 0.25:
            book.add(when + datetime.timedelta(days=rng.randint(1, 10)), f"^{label} returned",
                     dict(kind="return", label=label))
            book.forms["payment returned"] += 1
    if rng.random() < 0.25:
        party, when, qty = rng.choice(["ann", "bob", "cy"]), day(rng, 60, 150), rng.choice([50, 200, 300])
        elsewhere = "bob" if party != "bob" else "cy"
        if rng.random() < 0.5:
            book.add(when, f"{party} -> {elsewhere} {amount(qty)} USD #shopping")
            book.forms["payment to a third party"] += 1
        else:
            book.add(when, f"{party} -> {amount(qty)} USD\n  {elsewhere} {amount(qty - 20)} USD #shopping\n  stripe 20 USD #shopping")
            book.forms["split that reaches no owner"] += 1
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
                 dict(kind="make", place=place, code=code, qty=qty, lines=[[qty, None if boxes else "design"]], unit=unit))
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


def bills_family(book):
    """Bills the owner owes parties, a few with lines, and what pays them."""
    rng = book.rng
    bills = []
    for _ in range(rng.randint(2, 5)):
        d, code, party = day(rng, 2, 100), book.code("b"), rng.choice(["pge", "ben"])
        due = d + datetime.timedelta(days=rng.choice([10, 30, 45]))
        qty = rng.choice(AMOUNTS)
        purposes = ["electricity", "electricity", None]
        if rng.random() < 0.25:
            first, kinds = rng.choice(AMOUNTS), [rng.choice(purposes), rng.choice(purposes)]
            lines = [[first, kinds[0]], [qty, kinds[1]]]
            tag = lambda purpose: f" #{purpose}" if purpose else ""
            line = (f"me owes {party} due {text(due)} ^{code}\n  {amount(first)} USD{tag(kinds[0])}\n"
                    f"  {amount(qty)} USD{tag(kinds[1])}")
            qty += first
            book.forms["itemized bill"] += 1
        else:
            kind = rng.choice(purposes)
            lines = [[qty, kind]]
            line = f"me owes {party} {amount(qty)} USD due {text(due)}{f' #{kind}' if kind else ''} ^{code}"
            book.forms["bill with a purpose" if kind else "bill with no purpose"] += 1
        book.add(d, line, dict(kind="make", place="debt:" + party, code=code, qty=qty, lines=lines, unit="USD"))
        bills.append((d, code, party, qty))
    for _ in range(rng.choice([0, 1, 1, 2, 3])):
        d0, code0, party, qty0 = rng.choice(bills)
        total = sum(c[3] for c in bills if c[2] == party)
        choice = rng.random()
        need = (qty0 if choice < 0.3 else max(1, qty0 // 2) if choice < 0.5 else total + 50 if choice < 0.6
                else qty0 + rng.choice(AMOUNTS))
        label = book.code("pay-")
        tail, line_tail = [label], f" ^{label}"
        if rng.random() < 0.35:
            tail, line_tail = [label, code0], f" ^{label} ^{code0}"
            book.forms["payment names a bill"] += 1
        own = rng.choice([None, None, "electricity"])
        when = d0 + datetime.timedelta(days=rng.randint(1, 50))
        book.add(when, f"checking -> {party} {amount(need)} USD{f' #{own}' if own else ''}{line_tail}",
                 dict(kind="pay", party=party, place="debt:" + party, legs=[["owner", need, own]], select=None,
                      tail=tail, label=label))
        book.forms["bill paid"] += 1
        if rng.random() < 0.25:
            book.add(when + datetime.timedelta(days=rng.randint(1, 10)), f"^{label} returned",
                     dict(kind="return", label=label))
            book.forms["payment of a bill returned"] += 1
    for d, code, _, _ in bills:
        if rng.random() < 0.4:
            when = d + datetime.timedelta(days=rng.choice([0, 0, 5, 20, 40]))
            book.add(when, f'^{code} waived "off {code}"', dict(kind="writeoff", code=code, declared=False))
            book.forms["bill forgiven"] += 1
    if rng.random() < 0.5:
        place_bills(book)


def place_bills(book):
    """Bills held in a declared `payable` place, and payments into it."""
    rng = book.rng
    made = []
    for _ in range(rng.randint(1, 3)):
        d, code, qty, kind = day(rng, 2, 60), book.code("p"), rng.choice(AMOUNTS), rng.choice(["electricity", None])
        due = d + datetime.timedelta(days=30)
        book.add(d, f"owed-to-ben -> pge {amount(qty)} USD due {text(due)}{f' #{kind}' if kind else ''} ^{code}",
                 dict(kind="make", place="owed-to-ben", code=code, qty=qty, lines=[[qty, kind]], unit="USD"))
        made.append((code, qty))
    held = sum(qty for _, qty in made)
    for _ in range(rng.randint(1, 3)):
        code, qty = rng.choice(made)
        choice = rng.random()
        need = qty if choice < 0.3 else max(1, qty // 2) if choice < 0.5 else held + 40 if choice < 0.6 else qty + 100
        tail, line_tail = [], ""
        if rng.random() < 0.5:
            tail, line_tail = [code], f" ^{code}"
            book.forms["payment into a payable names a bill"] += 1
        book.add(day(rng, 61, 150), f"checking -> owed-to-ben {amount(need)} USD{line_tail}",
                 dict(kind="into", place="owed-to-ben", qty=need, tail=tail))
        book.forms["payment into a payable"] += 1
    for code, _ in made:
        if rng.random() < 0.3:
            book.add(day(rng, 100, 150), f'^{code} waived "off {code}"', dict(kind="writeoff", code=code, declared=False))
            book.forms["bill in a payable forgiven"] += 1


def debts_family(book):
    """Bills, a deposit the owner is owed, and a loan paid in kind: the debts that are parcels, a claim beside them, and the
    one that stays a balance. Says the loan's contract, which is written before the journal."""
    rng = book.rng
    bills_family(book)
    book.add(datetime.date(2026, 1, 3), "bank owes me 2_000 USD due 2026-12-31 ^deposit",
             dict(kind="make", place="tab:bank", code="deposit", qty=2000, lines=[[2000, None]], unit="USD"))
    contract = ""
    if rng.random() < 0.5:
        contract = "contract mortgage with bank\n  loan 3_000 USD on 2026-01-01 at 0% over 3m\n  monthly on 1 from checking\n  from 2026-02-01\n"
        for m in (2, 3, 4):
            if rng.random() < 0.7:
                book.add(datetime.date(2026, m, 1), "mortgage")
        book.forms["loan"] += 1
    book.forms["debts"] += 1
    return contract


FAMILIES = [("tab", 22), ("place", 28), ("boxes", 10), ("lots", 12), ("assets", 8), ("debts", 24), ("mixed", 10)]


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
        extra = debts_family(book)
    else:
        (tab_family if rng.random() < 0.5 else place_family)(book)
        lots_family(book)
    book.noise()
    spec = dict(family=family, events=book.events, books=book.books)
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

CLAIM_PLACES = {"owed", "queue", "owed-boxes", "owed-to-ben"}
# What the owner owes is held as negative parcels: the open quantity of a bill is the parcel's, the other way round.
DEBT_PLACES = {"owed-to-ben"}


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
            if place.startswith("tab(") and ",loan," in place:
                continue
            qty = int(lot.group(1)) * (-1 if place in DEBT_PLACES or ",owed-by," in place else 1)
            if qty > 0:
                for code in lot.group(2).split():
                    open_[code] += qty
    forgiven = Counter()
    for line in dump.split("\n"):
        found = re.match(r"written-off change \d+ (\S+) \S+ qty (\d+) basis (\d+)", line)
        if found:
            forgiven["total"] += int(found.group(2))
            forgiven["basis"] += int(found.group(3))
            # a debt's parcel has no basis: what is owed is not value held
            forgiven["debts"] += int(found.group(2)) * (found.group(1) in DEBT_PLACES or ",owed-by," in found.group(1))
    diagnostics = Counter(re.findall(r"^diagnostic \w+ (\S+) ", dump, re.M))
    return open_, plain, forgiven, diagnostics, dump


def effects_of(dump):
    """What the purposes' laws counted: (tally, day, quantity) for each, as the dump says."""
    found = Counter()
    for tally, when, qty in re.findall(r"^effect (\S+) (\S+) (-?\d+)$", dump, re.M):
        if tally in TALLIES.values():
            found[(tally, when, int(qty))] += 1
    return found


def flow_view(output):
    """What `flow` says each purpose came to over all its months: the amount under the `Total` heading, which is blank
    when the months add up to nothing."""
    said, found = section(output, "flow"), Counter()
    heading = re.search(r"^.*\bTotal$", said, re.M)
    end = len(heading.group(0)) if heading else 0
    for purpose in TALLIES:
        row = re.search(rf"^\s+{purpose}\s+(.*)$", said, re.M)
        if row:
            line = row.group(0)
            under = [m for m in re.finditer(r"(-?[\d,]+\.\d\d) USD", line) if m.end() == end]
            found[purpose] = round(float(under[0].group(1).replace(",", "")) * 100) if under else 0
    return found


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
    sim = run_events(spec["events"], rules, spec.get("books") or DEFAULT_BOOKS)
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
    if spec["family"] not in ("tab", "place", "boxes", "mixed", "debts") or (spec["family"] == "debts" and rules != "new"):
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
    if rules != "old":
        if reports and +claims_view(output) != +want_open:
            failures.append(f"claims view {dict(+claims_view(output))} wanted {dict(+want_open)}")
        if diagnostics.get("claim-writeoff-empty", 0) != sim.empty:
            failures.append(f"empty write-offs {diagnostics.get('claim-writeoff-empty', 0)} wanted {sim.empty}")
        if forgiven["total"] != sum(sim.forgiven.values()):
            failures.append(f"forgiven {forgiven['total']} wanted {sum(sim.forgiven.values())}")
        if forgiven["basis"] != forgiven["total"] - forgiven["debts"]:
            failures.append(f"forgiven basis {forgiven['basis']} wanted {forgiven['total'] - forgiven['debts']}")
        if diagnostics.get("ambiguous-lots", 0) and spec["family"] != "mixed":
            failures.append("a claim place is ambiguous")
        shown, wanted = ({k: v for k, v in c.items() if v} for c in (flow_view(output), sim.net))
        if reports and shown != wanted:
            failures.append(f"flow says {shown} wanted {wanted} ({spec.get('books')})")
        if effects_of(dump) != Counter(sim.counted):
            counted, wanted = effects_of(dump), Counter(sim.counted)
            failures.append(f"counted {sorted((counted - wanted).items())} too much, "
                            f"{sorted((wanted - counted).items())} too little ({spec.get('books')})")
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


MOVES_WITH_CLAIMS = CLAIM_PLACES | {"ann", "bob", "cy", "stripe", "market", "stockroom", "pge", "ben"}


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
    """Whether the lane under test leaves a project in a different state from its baseline, so that the two builds should
    differ. The baseline of lane K3f is lane K3d's build, which already settles what a party pays, counts a claim by the owner's
    books and forgives one: only what the owner owes differs, for a bill was a balance that nothing settled and is a parcel now,
    which `claims` lists and a payment to the party (or into the place that holds it) settles."""
    return spec["family"] == "debts" and any(e["kind"] == "make" for e in spec["events"])


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
    ("crates/engine/src/lots.rs", "if held == req.exact {", "if held >= req.exact {", "exact takes a claim of at least the need"),
    ("crates/engine/src/lots.rs", "(b.claim == exact).cmp(&(a.claim == exact)).then(a.source.cmp(&b.source))",
     "(a.claim == exact).cmp(&(b.claim == exact)).then(a.source.cmp(&b.source))", "scanning puts the exact claims last"),
    ("crates/engine/src/lots.rs", "(b.claim == exact).cmp(&(a.claim == exact)).then(a.source.cmp(&b.source))",
     "(b.claim == exact).cmp(&(a.claim == exact)).then(b.source.cmp(&a.source))", "of equal claims the newest, when scanning"),
    ("crates/engine/src/lots.rs", "            if policy == Some(Policy::Exact) {\n                self.take_exact(&mut left, of_colour, req, out);\n            }\n",
     "", "the ordered path never takes the exact lot"),
    ("crates/engine/src/lots.rs", "let held: Qty = self.holding.lots[at..end].iter().filter(|&lot| keep(lot)).map(|lot| lot.qty).sum();",
     "let held: Qty = self.holding.lots[at..end].iter().map(|lot| lot.qty).sum();", "a claim holds the lines of every colour, when ordered"),
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
    ("crates/engine/src/claims.rs", "            self.world.holdings.credit(party, unit, self.plan.sides.display(place, qty));\n", "", "the forgiven value goes nowhere"),
    ("crates/engine/src/claims.rs", "if forgiven == 0 {", "if forgiven == 1 {", "an empty write-off is said when one parcel was forgiven"),
    ("crates/engine/src/claims.rs", "let Slice { qty, basis, acquired, .. } = *slice;", "let Slice { qty, acquired, .. } = *slice;\n            let basis = Qty::ZERO;",
     "a write-off records no basis"),
    ("crates/engine/src/claims.rs", "let Slice { qty, basis, acquired, .. } = *slice;", "let Slice { basis, acquired, .. } = *slice;\n            let qty = slice.qty + slice.qty;",
     "a write-off records twice the parcel"),
    ("crates/model/src/lower/record.rs", "let claimed = matches!(used, CodeUse::ClaimWaiver) && self.claims.by_code.contains_key(&symbol);",
     "let claimed = false;", "a code on a claim and its payment is ambiguous for a write-off"),
    ("crates/model/src/lower/statements.rs", "if !flows().any(|flow| book.makes_claim(flow)) {", "if !flows().any(|flow| book.is_claim(flow.to)) {",
     "a claim place funded by the owner's own money is a claim on a party"),
    ("crates/model/src/said.rs", "if from == Class::Outside && self.is_claim(flow.to) {", "if self.is_claim(flow.to) {",
     "a claim made of the owner's own money can be forgiven"),
    ("crates/model/src/declare.rs", "        self.say(kinds.claim, builtin::CLAIM, true);\n", "", "the kind of a tab does not say `claim`"),
    ("crates/engine/src/settle.rs", "if m.target.class == Class::Asset && !self.plan.traits.place(m.to).claim {",
     "if m.target.class == Class::Asset {", "a flow that makes a claim settles the claims before it"),
    ("crates/engine/src/settle.rs", "let need = open.min(m.out.qty);", "let need = open;", "a payment settles more than it paid"),
    ("crates/engine/src/settle.rs", "let named = self.name_claims(m, tab);", "let named = false;",
     "the codes of a payment from a party name no claim"),
    ("crates/engine/src/settle.rs", "        settlement.parcels.iter().for_each(|&parcel| slot.restore(parcel, codes));\n", "",
     "a returned payment does not open its claims"),
    ("crates/engine/src/settle.rs", "        let claiming = Claiming { settlement, dir };\n        self.credit_party(m, &claiming);\n",
     "        let claiming = Claiming { settlement, dir };\n", "a returned payment makes value when it opens its claims"),
    ("crates/engine/src/post.rs", "            false => self.world.holdings.credit(m.from, m.out.unit, settled - m.out.qty),",
     "            false => self.world.holdings.credit(m.from, m.out.unit, -m.out.qty),", "a payment that settles claims makes value"),
    ("crates/engine/src/post.rs", "            self.world.holdings.credit(m.from, m.out.unit, paid - m.out.qty);\n",
     "            self.world.holdings.credit(m.from, m.out.unit, -m.out.qty);\n", "a leg to a third party that settles claims makes value"),
    ("crates/engine/src/settle.rs", "        let settlement = self.record.settled.remove(&flow)?;\n", "        let settlement = self.record.settled.get(&flow)?.clone();\n",
     "a payment that is returned forgets what it settled, and a return of it settles again"),
    ("crates/engine/src/settle.rs", "exact: open.min(rest),", "exact: need,", "exactly the flow's amount is judged on the leg and not on what the party pays in all"),
    ("crates/engine/src/settle.rs", "            Class::Outside => Some(Leg::Elsewhere),\n", "            Class::Outside => None,\n",
     "a leg to a third party is no payment"),
    ("crates/engine/src/settle.rs", "filter(move |&id| id >= this)", "filter(move |&id| id > this)", "what the party still pays leaves out the leg being paid"),
    ("crates/engine/src/settle.rs", "filter(move |&id| id >= this)", "filter(move |&id| id == this)", "what the party still pays is this leg alone"),
    ("crates/engine/src/settle.rs", "same && self.plan.events.state(*id, flow).is_real_on(day)", "same",
     "a leg that is not real yet is paid"),
    ("crates/engine/src/settle.rs", "let same = flow.from == source.from && flow.out.unit == source.out.unit && !flow.is_exchange();",
     "let same = flow.out.unit == source.out.unit && !flow.is_exchange();", "a payment of another party is this payment's"),
    ("crates/engine/src/settle.rs", "let toward = self.leg(flow).is_some_and(|leg| leg == Leg::Owner(owner) || leg == Leg::Elsewhere);",
     "let toward = self.leg(flow).is_some_and(|leg| leg == Leg::Owner(owner));", "a leg to a third party is not what the party pays in all"),
    ("crates/engine/src/settle.rs", "paid.filter(|_| m.target.class == Class::Outside)", "None::<Id<Entity>>",
     "a leg to a third party is never paid on the owner's behalf"),
    ("crates/engine/src/traits.rs", "partition_point(|&(found, by, way, _)| (found, by, way) < key)",
     "partition_point(|&(found, by, way, _)| (found, by, way) <= key)", "the tab of a party is not found"),
    ("crates/engine/src/lots.rs", "lots.iter().take_while(|lot| lot.txn == lots[0].txn).count()", "1",
     "the lines of an invoice are claims of their own"),
    ("crates/engine/src/recognition.rs", "            (Dealing::Making, Books::Cash) if self.purpose.is_some() => {}\n", "",
     "a claim made counts in cash books"),
    ("crates/engine/src/recognition.rs", "(Dealing::Making, Books::Cash) if self.purpose.is_some() => {}", "(Dealing::Making, Books::Accrual) if self.purpose.is_some() => {}",
     "a claim made counts in cash books and not in accrual"),
    ("crates/engine/src/recognition.rs", "            if books == Books::Accrual {\n                continue;\n            }\n", "",
     "a claim settled counts again in accrual books"),
    ("crates/engine/src/recognition.rs", "let replaced = if settlement.reaches == Reaches::Owner { settled } else { Qty::ZERO };", "let replaced = settled;",
     "a leg to a third party is replaced by the claims it paid"),
    ("crates/engine/src/recognition.rs", "Share::Part(moved - replaced)", "Share::Part(moved)",
     "a payment counts what it moved as well as the claims it settled"),
    ("crates/engine/src/recognition.rs", "(Dealing::Forgiving { tab, qty, dir }, Books::Accrual) if self.purpose.is_some() =>",
     "(Dealing::Forgiving { tab, qty, dir }, Books::Cash) if self.purpose.is_some() =>", "a write-off reverses in cash books and not in accrual"),
    ("crates/engine/src/recognition.rs", "let dealing = Dealing::Forgiving { tab, qty, dir: claim_dir(plan.book.places[tab].class).reversed() };",
     "let dealing = Dealing::Forgiving { tab, qty, dir: claim_dir(plan.book.places[tab].class) };", "a write-off adds what the claim recognized"),
    ("crates/engine/src/recognition.rs", "book.txn_flow(part.origin, part.ordinal)?", "book.txn_flow(part.origin, 0)?",
     "every parcel of a claim is for its first line's purpose"),
    ("crates/engine/src/recognition.rs", "let Some(purpose) = claim_purpose(book, parcel) else { continue };",
     "let Some(purpose) = claim_purpose(book, parcel) else {\n                total += parcel.qty;\n                continue;\n            };",
     "a claim with no purpose replaces what pays it"),
    ("crates/engine/src/claims.rs", "slice.part.and_then(|part| book.txn_flow(part.origin, part.ordinal)).unwrap_or(claim)", "claim",
     "a write-off takes back the first line's purpose for every parcel"),
    ("crates/engine/src/post.rs", "None if self.plan.makes_claim(m.from, m.to) => Dealing::Making,", "None if false => Dealing::Making,",
     "the fold counts a claim made as an ordinary flow"),
    ("crates/engine/src/recognition.rs", "None if plan.makes_claim(flow.from, flow.to) => Dealing::Making,", "None if false => Dealing::Making,",
     "the readers count a claim made as an ordinary flow", "cli"),
    ("crates/engine/src/settle.rs", "Some(Claiming { settlement, dir: Dir::In })\n    }\n}", "None\n    }\n}",
     "a flow out of a claim place settles nothing as far as counting goes"),
    ("crates/engine/src/settle.rs", "let settlement = Settlement { tab: m.from, unit: m.out.unit, parcels, reaches: Reaches::Elsewhere };",
     "let settlement = Settlement { tab: m.from, unit: m.out.unit, parcels, reaches: Reaches::Owner };", "a flow out of a claim place reaches the owner's money"),
    ("crates/engine/src/settle.rs", "            self.record.settlements.push((flow, claiming.settlement.clone()));\n", "",
     "the readers are not told what a payment settled", "cli"),
    ("crates/engine/src/settle.rs", "Dir::In => self.settled(),", "Dir::In => Qty::ZERO,",
     "a payment that settled claims debits the party for them as well"),
    ("crates/engine/src/post.rs", "Counts::Claim { dir, .. } => dir,", "Counts::Claim { .. } => Dir::Out,", "a claim settled counts the way the payment goes"),
    ("crates/engine/src/recognition.rs", "Dealing::Settling { settlement, dir, moved: posted.out }", "Dealing::Settling { settlement, dir, moved: Qty::ZERO }",
     "the readers count a payment that settled claims without what it moved", "cli"),
    ("crates/report/src/flow.rs", "Some(Qty(if piece.takes_back(book) { -volume } else { volume }))", "Some(Qty(volume))",
     "a claim taken back is more of a purpose that passes through"),
    ("crates/engine/src/explain.rs", "let of_other_days = KEEPS_FLOW_DAYS && !flow.recognized.overlaps(read_days);",
     "let of_other_days = KEEPS_FLOW_DAYS && flow.recognized.overlaps(read_days);", "a limit's explanation names the flows of other days"),
    ("crates/engine/src/plan.rs", "let made = traits.place(to).claim && outside(from);", "let made = traits.place(to).claim;",
     "a flow from a place of the owner into a claim place makes a claim"),
    # Lane K3f: what the owner owes
    ("crates/engine/src/lots.rs",
     "        if self.owes {\n            self.mirror();",
     "        if false {\n            self.mirror();",
     "a debt is relieved as if it were held"),
    ("crates/engine/src/lots.rs",
     "        self.qty = -self.qty;\n        for lot",
     "        for lot",
     "the mirror leaves the balance of a debt as it was"),
    ("crates/engine/src/lots.rs",
     "self.land_with_codes(Parcel { qty: -owed.qty, basis: -owed.basis, ..owed }, false, codes);",
     "self.land_with_codes(owed, false, codes);",
     "a bill is a parcel of a positive quantity"),
    ("crates/engine/src/lots.rs",
     "true => self.holding.plain - self.qty,",
     "true => self.qty - self.holding.plain,",
     "what a debt admits is its balance and not what is owed"),
    ("crates/engine/src/lots.rs",
     "        let admitted: Qty = found.iter().map(|c| c.qty).sum();\n        if self.owes { -admitted } else { admitted }",
     "        found.iter().map(|c| c.qty).sum()",
     "what a selector admits of a debt is negative"),
    ("crates/engine/src/lots.rs",
     "            true => self.owe(parcel, codes),\n            false => self.land_with_codes(parcel, false, codes),",
     "            true => self.land_with_codes(parcel, false, codes),\n            false => self.land_with_codes(parcel, false, codes),",
     "a payment that is returned lands the bill positive"),
    ("crates/engine/src/post.rs",
     "        let holds = |end: &Place, at: Id<Place>| end.class == Class::Asset || self.plan.traits.place(at).claim;",
     "        let holds = |end: &Place, _: Id<Place>| end.class == Class::Asset;",
     "a bill is a flow between two places that hold no parcels"),
    ("crates/engine/src/post.rs",
     "match self.plan.traits.place(m.from).claim && m.course == Course::Forward {",
     "match self.plan.traits.place(m.from).claim {",
     "a bill that is returned owes again"),
    ("crates/engine/src/settle.rs",
     "let tab = self.plan.traits.tab_of(m.to, m.source.owner, Class::Debt)?;",
     "let tab = self.plan.traits.tab_of(m.to, m.source.owner, Class::Asset)?;",
     "a payment to a party settles the claims on the party"),
    ("crates/engine/src/settle.rs",
     "let debt = m.target.class == Class::Debt && self.plan.traits.place(m.to).claim;",
     "let debt = self.plan.traits.place(m.to).claim;",
     "a payment into a place that is a claim of the owner's settles it"),
    ("crates/engine/src/settle.rs",
     "        if claiming.dir == Dir::Out {\n            self.world.holdings.credit(m.to,",
     "        if claiming.dir == Dir::In {\n            self.world.holdings.credit(m.to,",
     "the party is credited what settled a bill"),
    ("crates/engine/src/settle.rs",
     "forward.flatten().or_else(|| self.paid_into_debt(m))",
     "forward.flatten()",
     "nothing is paid into a place that holds what the owner owes"),
    ("crates/engine/src/settle.rs",
     "let forward = (m.course == Course::Forward).then(",
     "let forward = (m.course == Course::Back).then(",
     "a payment that is returned settles"),
    ("crates/engine/src/settle.rs",
     "Some(Payment { tab, rest: m.out.qty, reaches: Reaches::Owner })",
     "Some(Payment { tab, rest: m.out.qty, reaches: Reaches::Elsewhere })",
     "what a payment to a party settles is not what it paid"),
    ("crates/engine/src/settle.rs",
     "debt.then_some(Payment { tab: m.to, rest: m.out.qty, reaches: Reaches::Elsewhere })",
     "debt.then_some(Payment { tab: m.to, rest: m.out.qty, reaches: Reaches::Owner })",
     "a payment into a place that holds a debt replaces what it counts of itself"),
    ("crates/engine/src/settle.rs",
     "let from_owner = m.source.class == Class::Asset && !self.plan.traits.place(m.from).claim;",
     "let from_owner = true;",
     "anything paid to a party pays its bills"),
    ("crates/engine/src/settle.rs",
     "!matches!(m.target.role, Role::Outside(Some(_))) || !self.plan.traits.has_tab(m.to)",
     "!self.plan.traits.has_tab(m.to)",
     "a payment to a place that is no party's pays its bills"),
    ("crates/engine/src/settle.rs",
     "        let dir = claim_dir(self.plan.book.places[settlement.tab].class).reversed();\n        let claiming",
     "        let dir = claim_dir(self.plan.book.places[settlement.tab].class);\n        let claiming",
     "a payment returned counts the way it was paid"),
    ("crates/engine/src/settle.rs",
     "        let day = m.detail().since.unwrap_or(m.day);\n        let part",
     "        let day = m.day;\n        let part",
     "a bill made `since` another day is made on the day written"),
    ("crates/engine/src/settle.rs",
     "        let part = Some(PartId { origin: m.txn, ordinal: m.flow_ordinal });",
     "        let part = Some(PartId { origin: m.txn, ordinal: 0 });",
     "every parcel of a bill is for its first line's purpose"),
    ("crates/engine/src/recognition.rs",
     "        Class::Debt => Dir::Out,\n        Class::Asset | Class::Outside => Dir::In,",
     "        Class::Debt => Dir::In,\n        Class::Asset | Class::Outside => Dir::Out,",
     "a claim counts the other way"),
    ("crates/engine/src/recognition.rs",
     "Counts::Claim { tab, dir } => dir == claim_dir(book.places[tab].class).reversed(),",
     "Counts::Claim { dir, .. } => dir == Dir::Out,",
     "a bill settled takes back what it counted, as a claim forgiven does", "cli"),
    ("crates/engine/src/claims.rs",
     "self.world.holdings.credit(party, unit, self.plan.sides.display(place, qty));",
     "self.world.holdings.credit(party, unit, qty);",
     "the value a bill forgiven gives back goes the wrong way"),
    ("crates/engine/src/claims.rs",
     "let party = if place == line.to { line.from } else { line.to };",
     "let party = line.from;",
     "the party of a bill forgiven is where the money came from"),
    ("crates/model/src/said.rs",
     "} else if (from, to) == (Class::Debt, Class::Outside) && self.is_claim(flow.from) {",
     "} else if false {",
     "a bill is not a claim that can be forgiven"),
    ("crates/model/src/book.rs",
     "                if self.places[place].class == Class::Debt { (flow.from, flow.to) } else { (flow.to, flow.from) };",
     "                (flow.to, flow.from);",
     "a bill says nothing of who is owed and when"),
    ("crates/engine/src/plan.rs",
     "let unmade = traits.place(from).claim && outside(to);",
     "let unmade = traits.place(from).claim && outside(to) && book.places[from].class == Class::Asset;",
     "a bill is counted as an ordinary flow"),
    ("crates/engine/src/traits.rs",
     "Role::Tab(party) if book.is_claim(id) =>",
     "Role::Tab(party) =>",
     "a loan's tab is settled by what is paid to its lender"),
    ("crates/model/src/lower/contracts.rs",
     "    let debt = world.tab(party, owner, world.book.roots.kinds.debt, prop.loc);",
     "    let debt = world.tab(party, owner, world.book.roots.kinds.debt_claim, prop.loc);",
     "a loan is a bill"),
    ("crates/report/src/claims.rs",
     "mine: book.places[holding.place].class == Class::Asset,",
     "mine: true,",
     "what the owner owes is owed to it", "cli"),
    ("crates/report/src/claims.rs",
     "let left = lens.plan().sides().display(holding.place, lens.place_qty(holding.place, lot.qty));",
     "let left = lens.place_qty(holding.place, lot.qty);",
     "a bill is listed as a negative amount", "cli"),
    ("crates/report/src/flow.rs",
     "let signed = Amount::new(if dir == Dir::In { taken } else { -taken }, unit);",
     "let signed = Amount::new(-taken, unit);",
     "a bill forgiven adds to what was spent", "cli"),
    ("crates/engine/src/eval.rs",
     "let open = self.env.plan.sides.display(place, lot.qty);",
     "let open = lot.qty;",
     "what is open of a bill is negative"),
]


def build_cli(source, work):
    """The CLI of the (mutated) tree, for a mutant of what the readers do with a run: the dump does not read it."""
    env = dict(os.environ, CARGO_TARGET_DIR=os.path.join(work, "cli-target"))
    result = subprocess.run(["cargo", "build", "--release", "--offline", "-p", "axiom-cli"], cwd=source, env=env,
                            capture_output=True, text=True)
    if result.returncode:
        raise SystemExit("the CLI did not build")
    return os.path.join(work, "cli-target", "release", "axiom")


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
    for number, (path, old, replacement, what, *how) in enumerate(MUTANTS):
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
            if how == ["cli"]:
                do_dump(build_cli(source, work), binary, directory, tag)
                caught_by = [path for path in projects(directory) if check_one(path, read(path, tag), "new")]
            else:
                dump_only(binary, directory, tag)
                caught_by = caught(directory, "base", tag)
            if caught_by:
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
