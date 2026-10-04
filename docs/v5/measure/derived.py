#!/usr/bin/env python3
"""What a law derives from a flow that has posted, against the book that has written what it derives.

    derived.py gen DIR N [SEED]                  write N projects into DIR (p0000/derived, /law, /written, /ahead)
    derived.py run BINARY DIR [JOBS]             the CLI of any build on every project: the books must print the same
    derived.py dump DUMP DIR [JOBS]              the engine's own dump of every flow it posted (`forecasts/main.rs`)
    derived.py compare BASELINE NEW DIR [JOBS]   both builds on the written books: they must print what they printed
    derived.py mutate TREE WORK DIR [N,M..]      the mutants of the code under test: each must be caught

What it is for. Lane K6b gives a law that is written under a kind, a purpose, an entity, an account or an asset a host: when
a flow has posted, the laws that watch it derive other flows (a card's cash back, a processor's fee, sales tax collected),
which post right after it, derive in their turn, and are returned with it. Everything such a law makes is a flow a book can
already write, so the strongest check is the book that writes it: every project is four books.

    derived   the laws as `also LINE [when E]` under the owners that carry them
    law       the same laws as `law dN` / `on flow` / `[when E]` / `derive LINE`, which an `also` abbreviates
    written   no law at all, and after each line the lines that the laws derive from it, in the order they post
    ahead     `derived`, with every occurrence of every contract kept by a line (the forecast of `derived` is its fold)

`derived` and `law` must be the same book to every command, and so must `derived` and `written` (less what only a written
line can say: where it was written, how many flows there are, and who derived it). The dump is read as well, for the flows
themselves, and `ahead`'s fold against `derived`'s forecast: a forecast is the fold, with no second path.

The reference. Which laws watch a flow, in what order, and what they derive is worked out here a second time, from what
LANGUAGE says and not from the engine: a flow at a place fires the laws of that place and of the places it lies in, of its
kind, of the party that stands there and of its kind; then the laws of its purpose and of the purposes above it; then those
of the asset it is for; each in the order it was written, once for the flow however many places it touches; none for value
that moved around inside what the law governs; a law does not watch what it made itself, a chain that comes back to a law
is stopped and said once, and so is one of more than eight laws. What a flow derives posts right after it, the first it
derived first, each one's own before the next is looked at.

The books. Every flow of a project is on its own day, so that no order is left to the file's. A project holds the flows a
journal writes (spending, income, moves between accounts, repairs for an asset), some pending and then settled, void or
still pending, some returned, some written ahead of today, and the occurrences of contracts, kept up to the last day the journal says anything of and
promised after it. A project whose laws make a chain that is stopped is told so once, and `check` is held to the reference's count.
"""
import itertools
import os
import random
import re
import shutil
import subprocess
import sys
from collections import Counter
from concurrent.futures import ThreadPoolExecutor
from datetime import date, timedelta
from decimal import ROUND_HALF_EVEN, Decimal

TODAY = "2026-06-30"
UNTIL = "2026-12-31"
DEPTH = 8
MOST_FLOWS = 40

# ─── The world every book is about ───────────────────────────────────────────────────────────────────────────────────

# The word that declares it, its name, and the declaration. A law is ranked by where its owner is declared here and where
# it is written under it, which is the order the engine fires laws in when nothing says one reads what another counts.
DECLARATIONS = [
    ("kind", "rewards", "kind rewards : credit-card"),
    ("kind", "gold", "kind gold : rewards"),
    ("kind", "saver", "kind saver : bank"),
    ("kind", "vendor", "kind vendor : merchant"),
    ("kind", "processor", "kind processor : vendor"),
    ("kind", "adult", "kind adult : person"),
    ("kind", "flat", "kind flat : property"),
    ("kind", "slowbox", "kind slowbox : escrow"),
    ("purpose", "fee", "purpose fee : spending"),
    ("purpose", "rebate", "purpose rebate : income"),
    ("purpose", "levy", "purpose levy : spending"),
    ("purpose", "treat", "purpose treat : dining"),
    ("purpose", "coffee", "purpose coffee : treat"),
    ("entity", "me", "entity me : adult"),
    ("entity", "issuer", "entity issuer"),
    ("entity", "taxman", "entity taxman"),
    ("entity", "shop", "entity shop : vendor"),
    ("entity", "stripe", "entity stripe : processor"),
    ("entity", "acme", "entity acme : employer"),
    ("place", "checking", "account checking : bank"),
    ("place", "savings", "account savings : saver"),
    ("place", "vault", "account vault : bank"),
    ("place", "vault/box", "account vault/box : bank"),
    ("place", "visa", "account visa : gold"),
    ("place", "amex", "account amex : credit-card"),
    ("place", "fund", "account fund : slowbox"),
    ("asset", "house", "asset house : flat"),
]
PURPOSES_END = 13  # where the entities begin: the rungs of a ladder are declared before them
# What a flow for `levy` owes: not a law that derives, so that `available` can say what drawing the fund would cost.
OWED = "  law owed\n    on flow\n    owe 10% * amount to taxman by 2026-12-31 as levy-owed\n"
RUNGS = 11
LADDER = [("purpose", f"r{n}", f"purpose r{n} : spending") for n in range(RUNGS)]

CASH = {"checking", "savings", "vault", "vault/box", "fund"}  # the places that are assets: the cards are debts
FUND = 80_000  # what `fund` holds, which is slow money: `available` asks what drawing it would take
# `checking` holds so much more than any other place that it is the one `available` would draw the fund into, whatever flows.
OPENING = {place: "5_000_000.00 USD" for place in CASH} | {"fund": f"{FUND // 100}.00 USD", "checking": "50_000_000.00 USD"}
LIES_IN = {"vault/box": ["vault/box", "vault"]}  # a place, then every place it lies in
KIND_OF_PLACE = {"visa": ["gold", "rewards"], "savings": ["saver"], "fund": ["slowbox"]}  # the kinds of a place that carry laws
PLACE_KINDS = {"rewards", "gold", "saver", "slowbox"}
PARTIES = ["shop", "stripe", "acme", "issuer", "taxman"]
KIND_OF_ENTITY = {"me": ["adult"], "shop": ["vendor"], "stripe": ["processor", "vendor"]}
PURPOSE_ABOVE = {"coffee": ["coffee", "treat"]}  # a purpose, and the purposes above it that carry laws
KIND_OF_ASSET = ["flat"]
ASSETS = {("asset", "house"), ("kind", "flat")}

OWNERS = [(word, name) for word, name, _ in DECLARATIONS if name not in ("issuer", "taxman")]


def standing(place):
    """Who stands at a place: the party whose outside it is, and else the one who owns it."""
    return place if place in PARTIES else "me"


def cents(value):
    return int(Decimal(value).quantize(Decimal(1), rounding=ROUND_HALF_EVEN))


def money(qty):
    return f"{qty // 100}.{qty % 100:02d} USD"


# ─── A law, and what it derives ──────────────────────────────────────────────────────────────────────────────────────


class Law:
    """`also + 5% of amount #fee when value(amount, USD) > 50.00 USD`, written under `owner`."""

    def __init__(self, owner, shape, percent=None, fixed=None, purpose=None, ends=(None, None), guard=None):
        self.owner, self.shape, self.percent, self.fixed = owner, shape, percent, fixed
        self.purpose, self.ends, self.guard = purpose, ends, guard
        self.order = 0

    def line(self):
        amount = f"{self.percent}% of amount" if self.percent else money(self.fixed)
        tag = f"#{self.purpose}" if self.purpose else ""
        if self.shape in ("+", "-"):
            return " ".join(part for part in (self.shape, amount, tag) if part)
        source, sink = self.ends
        return " ".join(part for part in (source, "->", sink, amount, tag) if part)

    def when(self):
        test, threshold = self.guard
        return f"when value(amount, USD) {test} {money(threshold)}"

    def of(self, qty):
        """What it comes to for a flow of QTY."""
        return cents(Decimal(qty) * self.percent / 100) if self.percent else self.fixed

    def passes(self, qty):
        if self.guard is None:
            return True
        test, threshold = self.guard
        return qty > threshold if test == ">" else qty <= threshold

    def says(self, spelling):
        if spelling == "also":
            return f"  also {self.line()}" + (f" {self.when()}" if self.guard else "") + "\n"
        guard = f"    {self.when()}\n" if self.guard else ""
        return f"  law d{self.order}\n    on flow\n{guard}    derive {self.line()}\n"


class Flow:
    """A flow as the reference posts it: its ends, its amount, what it is for, and what derived it."""

    def __init__(self, source, sink, qty, purpose=None, of=None, law=None):
        self.source, self.sink, self.qty, self.purpose, self.of, self.law = source, sink, qty, purpose, of, law

    def text(self, day, tail="", pending=False):
        amount = f"({money(self.qty)})" if pending else money(self.qty)
        tag = f" #{self.purpose}" + (f" of {self.of}" if self.of else "") if self.purpose else ""
        return f"{day} {self.source} -> {self.sink} {amount}{tag}{tail}"


class Reject(Exception):
    """A book the reference will not write: a flow of a place to itself, or a chain too long to read."""


def governs(owner, place):
    """What a law written under `owner` is about at `place`: the subject it fires for, or nothing."""
    word, name = owner
    if word == "kind":
        if name in PLACE_KINDS:
            return ("place", place) if name in KIND_OF_PLACE.get(place, []) else None
        return ("entity", standing(place)) if name in KIND_OF_ENTITY.get(standing(place), []) else None
    if word == "place":
        return ("place", name) if name in LIES_IN.get(place, [place]) else None
    if word == "entity":
        return ("entity", name) if standing(place) == name else None
    return None


def inside(subject, place):
    """Whether `place` lies within what the subject governs: a place's subtree, or an entity's own assets."""
    kind, name = subject
    if kind == "place":
        return name in LIES_IN.get(place, [place])
    return name == "me" and place in CASH


def stands_for(subject, flow):
    """The end of the flow that `self` is, in a law that fires for the subject."""
    kind, name = subject
    here = name in LIES_IN.get(flow.source, [flow.source]) if kind == "place" else standing(flow.source) == name
    return flow.source if here else flow.sink


class Reference:
    """The laws of a book, and what they derive: the order, the guard, the cycle and the depth, from LANGUAGE."""

    def __init__(self, laws):
        self.laws = sorted(laws, key=lambda law: law.order)
        self.stopped = {}

    def watchers(self, flow):
        """The laws that fire for a flow, in order, each with the end `self` stands for; a law fires once."""
        fired, seen = [], set()
        for place in dict.fromkeys([flow.source, flow.sink]):
            for law in self.laws:
                subject = governs(law.owner, place)
                if subject is None or law.order in seen:
                    continue
                if not (inside(subject, flow.source) and inside(subject, flow.sink)):
                    seen.add(law.order)
                    fired.append((law, stands_for(subject, flow)))
        mine = flow.source if standing(flow.source) == "me" else flow.sink
        above = PURPOSE_ABOVE.get(flow.purpose, [flow.purpose])
        fired += [(law, mine) for law in self.laws if law.owner[0] == "purpose" and law.owner[1] in above]
        if flow.of:
            owners = [("kind", kind) for kind in KIND_OF_ASSET] + [("asset", flow.of)]
            fired += [(law, flow.sink) for law in self.laws if law.owner in owners]
        return fired

    def derive(self, flow, law, stands):
        """What a law derives from a flow."""
        if law.shape == "+":
            ends = (flow.source, flow.sink)
        elif law.shape == "-":
            ends = (flow.sink, flow.source)
        else:
            named = [stands if end == "self" else end for end in law.ends]
            ends = (named[0] or flow.source, named[1] or flow.sink)
        if ends[0] == ends[1]:
            raise Reject("a flow from a place to itself")
        purpose, of = (law.purpose, None) if law.purpose else (flow.purpose, flow.of)
        return Flow(*ends, law.of(flow.qty), purpose, of, law)

    def post(self, root):
        """Every flow the root derives, root first, in the order they post, each with the laws it descends through."""
        posted, waiting = [], [(root, ())]
        while waiting:
            flow, lineage = waiting.pop()
            posted.append((flow, lineage))
            if len(posted) > MOST_FLOWS:
                raise Reject("a chain that does not end soon enough to read")
            fresh = []
            for law, stands in self.watchers(flow):
                if not law.passes(flow.qty) or law.of(flow.qty) <= 0 or any(made.law is law for made, _ in fresh):
                    continue
                if lineage[-1:] == (law.order,):
                    continue
                if law.order in lineage or len(lineage) == DEPTH:
                    self.stopped.setdefault((lineage, law.order), "derive-cycle" if law.order in lineage else "derive-depth")
                    continue
                fresh.append((self.derive(flow, law, stands), lineage + (law.order,)))
            waiting.extend(reversed(fresh))
        return [flow for flow, _ in posted[1:]]


# ─── A project: laws, contracts and the events of a journal ──────────────────────────────────────────────────────────


class Contract:
    def __init__(self, rng, name, taken):
        self.name = name
        self.party = rng.choice(["stripe", "shop", "acme"])
        self.direction = "into" if self.party == "acme" else "from"
        self.account = rng.choice(["checking", "savings"] if self.party == "acme" else ["visa", "amex", "checking", "vault"])
        self.qty = rng.randrange(2_000, 40_000)
        self.purpose = "wages" if self.party == "acme" else rng.choice(["dining", "coffee", "groceries"])
        self.day, self.first = rng.randrange(1, 29), rng.randrange(1, 6)
        taken.update(self.due())

    def due(self):
        return [f"2026-{month:02d}-{self.day:02d}" for month in range(self.first, 13)]

    def head(self):
        tag = f" #{self.purpose}" if self.purpose else ""
        return (
            f"contract {self.name} with {self.party}\n"
            f"  {money(self.qty)} monthly on {self.day} {self.direction} {self.account}{tag}\n"
            f"  from 2026-{self.first:02d}-{self.day:02d}\n"
        )

    def flow(self):
        ends = (self.account, self.party) if self.direction == "from" else (self.party, self.account)
        return Flow(*ends, self.qty, self.purpose)


def fresh_day(rng, taken, low, high):
    """A day between LOW and HIGH (dates) that no other event of the project is on."""
    for _ in range(200):
        day = (low + timedelta(days=rng.randrange((high - low).days + 1))).isoformat()
        if day not in taken:
            taken.add(day)
            return day
    raise Reject("no day left between two events")


def random_flow(rng, family):
    qty = rng.randrange(1_500, 60_000)
    if family == "ladder":
        return Flow("checking", "shop", qty, "r0")
    kind = rng.choice(["spend"] * 4 + ["income", "move", "move", "card", "repair"])
    if kind == "spend":
        purposes = ["groceries", "dining", "treat", "coffee", "repair", "fee", "rebate", "levy", None, None]
        sources = ["visa", "visa", "amex", "checking", "savings"]
        return Flow(rng.choice(sources), rng.choice(["shop", "stripe"]), qty, rng.choice(purposes))
    if kind == "income":
        # The book gives pay a purpose when the line says none (an employer pays wages): a written line says it.
        return Flow("acme", rng.choice(["checking", "savings"]), qty, "wages")
    if kind == "move":
        moves = [("checking", "savings"), ("checking", "vault/box"), ("vault", "vault/box"), ("vault/box", "checking"),
                 ("savings", "vault"), ("vault/box", "vault")]
        return Flow(*rng.choice(moves), qty)
    if kind == "card":
        return Flow(rng.choice(["checking", "savings"]), rng.choice(["visa", "amex"]), qty)
    return Flow(rng.choice(["checking", "visa"]), "shop", qty, "repair", "house")


def random_law(rng, owner):
    """One law of a pool: a shape, an amount, a purpose and, for some, a guard on the flow's amount."""
    shape, ends = rng.choice(["+", "-", "->", "->"]), (None, None)
    if shape == "->":
        forms = [("issuer", None), (None, "taxman"), ("issuer", "taxman"), ("taxman", None)]
        # `self` in the law of an asset is the asset's own place, which no flow comes from.
        forms += [] if owner in ASSETS else [("issuer", "self"), ("self", "taxman"), ("self", "taxman")]
        ends = rng.choice(forms)
    percent, fixed = (rng.choice([1, 2, 3, 5, 10, 25, 50, 100]), None) if rng.random() < 0.75 else (None, rng.randrange(50, 2_500))
    guard = rng.choice([None, None, (">", rng.choice([2_000, 5_000, 10_000, 20_000])), ("<=", rng.choice([5_000, 15_000]))])
    # An item with no purpose is a part of the flow it comes with, which a flow that has posted cannot give: only a flow of
    # its own may say none, and is then for what its cause is for.
    purpose = rng.choice(["fee", "rebate", "levy", "fee"] + (["", ""] if shape == "->" else []))
    return Law(owner, shape, percent, fixed, purpose or None, ends, guard)


def cycle_laws(rng):
    """A law of a card that credits it and a law of what it credits that takes it back: a chain that closes."""
    owner = rng.choice([("kind", "rewards"), ("kind", "gold"), ("place", "visa")])
    laws = [Law(owner, "->", rng.choice([2, 5, 10, 50]), None, "fee", ("issuer", "self")),
            Law(("purpose", "fee"), rng.choice(["-", "+"]), rng.choice([10, 50, 100]), None, "rebate")]
    if rng.random() < 0.5:
        laws.append(Law(("purpose", "rebate"), "-", rng.choice([10, 50]), None, "levy"))
    return laws


def ladder_laws(rng):
    """Purposes each of which derives a flow for the next, along or back along the ends: more laws than the chain may have."""
    return [Law(("purpose", f"r{n}"), rng.choice(["-", "+"]), 100, None, f"r{n + 1}") for n in range(rng.choice([8, 9, 10]))]


class Event:
    """One thing the journal says: a flow, a pending one and what became of it, a returned one, an occurrence."""

    def __init__(self, kind, day, flow, later=None, outcome=None, contract=None):
        self.kind, self.day, self.flow, self.later, self.outcome, self.contract = kind, day, flow, later, outcome, contract


class Project:
    """What one seed makes: the laws, the contracts, the events, and the lines every book is made of."""

    def __init__(self, rng):
        self.family = rng.choice(["pool"] * 12 + ["cycle"] * 3 + ["ladder"] * 2)
        self.laws = {"pool": self.pool, "cycle": cycle_laws, "ladder": ladder_laws}[self.family](rng)
        self.declared = self.declare()
        self.taken = set()
        self.contracts = [Contract(rng, f"k{n}", self.taken) for n in range(rng.choice([0, 1, 1, 2]))]
        self.events = self.make_events(rng)
        self.reference = Reference(self.laws)
        self.write()

    def pool(self, rng):
        owners = rng.sample(OWNERS, rng.choice([1, 2, 2, 3, 3, 4]))
        return [random_law(rng, owner) for owner in owners for _ in range(rng.choice([1, 1, 1, 2]))]

    def declare(self):
        """What is declared, each with the laws written under it; and the laws take their place in the order."""
        written = {}
        for law in self.laws:
            written.setdefault(law.owner, []).append(law)
        rungs = LADDER if self.family == "ladder" else []
        declared, position = [], 0
        for word, name, text in DECLARATIONS[:PURPOSES_END] + rungs + DECLARATIONS[PURPOSES_END:]:
            declared.append((text, OWED if name == "levy" else "", written.get((word, name), [])))
            for law in written.get((word, name), []):
                law.order, position = position, position + 1
        return declared

    def make_events(self, rng):
        """The journal's flows: about a third of the projects write some of them ahead of today, and then a pending flow may
        settle, and a flow be returned, after it too."""
        ahead = rng.random() < 0.35
        kinds = ["flow"] * 6 + ["pending"] * 3 + ["returned"] * 2 + ["ahead"] * (2 if ahead else 0)
        before, after, today, last = date(2026, 1, 3), date(2026, 7, 2), date(2026, 6, 27), date(2026, 12, 20)
        events = []
        for kind in (rng.choice(kinds) for _ in range(rng.randrange(6, 18))):
            then = fresh_day(rng, self.taken, after if kind == "ahead" else before, last if kind == "ahead" else today)
            flow, born = random_flow(rng, self.family), date.fromisoformat(then)
            if kind in ("pending", "returned"):
                end = min(born + timedelta(days=60), date(2026, 12, 28) if ahead else date(2026, 6, 28))
                later = fresh_day(rng, self.taken, born + timedelta(days=1), end)
                outcome = rng.choice(["settled", "settled", "void", "open"]) if kind == "pending" else "returned"
                events.append(Event(kind, then, flow, None if outcome == "open" else later, outcome))
            else:
                events.append(Event("flow", then, flow))
        for contract in self.contracts:
            events += [Event("occurrence", day, contract.flow(), contract=contract) for day in contract.due()]
        return sorted(events, key=lambda event: event.day)

    def write(self):
        """Every line of every book, `(day, number, text)`: the journal's, and what the laws derive from it."""
        self.journal, self.derived, self.stats = [], [], Counter()
        numbers, codes = itertools.count(1), itertools.count(1)
        facts = [event.day for event in self.events if event.kind != "occurrence"]
        facts += [event.later for event in self.events if event.later]
        self.horizon = max([TODAY] + facts)

        def put(into, day, text):
            into.append((day, next(numbers), text))

        def derive(flow, day, into, tagged="", month=""):
            """The lines of what the root derives; the codes they carry when they are returned with it, or the month a
            settled flow was written in, which what it derives belongs to as it does."""
            tags = []
            for made in self.reference.post(flow):
                tag = f" ^d{next(codes)}" if tagged else (f" for {month}" if month else "")
                tags.append(tag.strip())
                put(into, day, made.text(day, tag))
                self.stats["derived"] += 1
            return tags

        for event in self.events:
            flow, day, later, code = event.flow, event.day, event.later, f" ^c{next(codes)}"
            if event.kind == "flow":
                put(self.journal, day, flow.text(day))
                derive(flow, day, self.derived)
            elif event.kind == "occurrence" and day <= self.horizon:
                put(self.journal, day, f"{day} {event.contract.name}")
                derive(flow, day, self.derived)
            elif event.kind == "occurrence":
                self.stats["promised-derived"] += len(self.reference.post(flow))
            elif event.kind == "pending":
                put(self.journal, day, flow.text(day, code, pending=True))
                if event.outcome != "open":
                    put(self.journal, later, f"{later}{code} {event.outcome}")
                if event.outcome == "settled":
                    derive(flow, later, self.derived, month=day[:7])
            else:
                put(self.journal, day, flow.text(day, code))
                tags = derive(flow, day, self.derived, tagged="yes")
                put(self.journal, later, f"{later}{code} returned")
                for tag in tags:
                    put(self.derived, later, f"{later} {tag} returned")
            self.stats[event.kind + ("-" + event.outcome if event.outcome else "")] += 1
        self.stopped = Counter(self.reference.stopped.values())
        # What `available` asks of the fund: its whole holding drawn into checking, a flow nothing wrote, and what its laws
        # derive from it that is for `levy`, of which a tenth is owed. The chains it stops are not the book's to say.
        drawn = Reference(self.laws).post(Flow("fund", "checking", FUND))
        self.owed = sum(cents(Decimal(made.qty) / 10) for made in drawn if made.purpose == "levy")

    def book(self, spelling):
        """The text of one book: `derived`, `law`, `written` or `ahead`."""
        laws = {"derived": "also", "law": "law", "ahead": "also", "written": None}[spelling]
        text = "use std\nbase USD\n"
        for declaration, native, written in self.declared:
            text += declaration + "\n" + native + "".join(law.says(laws) for law in written if laws)
        text += "opening 2025-12-31\n" + "".join(f"  {place} {OPENING[place]}\n" for place in sorted(CASH))
        text += "".join(contract.head() for contract in self.contracts)
        lines = list(self.journal)
        if spelling == "written":
            lines += self.derived
        if spelling == "ahead":
            lines += [(event.day, 0, f"{event.day} {event.contract.name}") for event in self.events
                      if event.kind == "occurrence" and event.day > self.horizon]
        return text + "".join(f"{line}\n" for _, _, line in sorted(lines, key=lambda line: line[:2]))


SPELLINGS = ("derived", "law", "written", "ahead")


def gen(directory, count, seed=1):
    shutil.rmtree(directory, ignore_errors=True)
    totals, owners, rejected = Counter(), Counter(), 0
    for number in range(count):
        for attempt in range(1000):
            try:
                project = Project(random.Random((seed * 100_000 + number) * 1000 + attempt))
                break
            except Reject:
                rejected += 1
        for spelling in SPELLINGS:
            path = os.path.join(directory, f"p{number:04d}", spelling)
            os.makedirs(path)
            with open(os.path.join(path, "axiom.ax"), "w") as handle:
                handle.write(project.book(spelling))
        with open(os.path.join(directory, f"p{number:04d}", "expect"), "w") as handle:
            handle.write(f"{project.family} {project.stopped['derive-cycle']} {project.stopped['derive-depth']} {project.horizon} {project.owed}\n")
        totals.update(project.stats)
        totals[project.family] += 1
        totals["stopped-cycle"] += project.stopped["derive-cycle"] > 0
        totals["stopped-depth"] += project.stopped["derive-depth"] > 0
        owners.update(f"{law.owner[0]}:{law.owner[1]}" for law in project.laws)
    print(f"{count} projects ({rejected} rejected); " + ", ".join(f"{name} {n}" for name, n in sorted(totals.items())))
    print("owners: " + ", ".join(f"{name} {n}" for name, n in sorted(owners.items())))


# ─── The commands, and what they must print ──────────────────────────────────────────────────────────────────────────

COMMANDS = [
    ["check", "--all"],
    ["balance"],
    ["flow"],
    ["flow", "--by", "party"],
    ["claims"],
    ["tax", "2026"],
] + [["register", target] for target in ("checking", "savings", "vault", "visa", "entity:shop", "entity:stripe", "house")]
FORECAST = ["forecast", "--until", UNTIL, "--paths", "1"]
# What only a book with the laws has: the same laws through another spelling, and the laws forecasting what a contract
# will make. A book that has them written is not a forecast of them: its occurrences past its last line are promised bare.
LAWS_ONLY = [["available"], ["balance", "--at", "2026-03-31"], FORECAST]


def tell(text):
    """What a command printed, less where the project is and the line that says a book has errors in it."""
    return re.sub(r"✗ rests on[^\n]*\n\n", "", re.sub(r"/[^ ]*/p\d+/[\w-]+", "<project>", text))


def derivation(text):
    """Less who derived a flow: the note a register has on a derived row, the line it was written on, and the code of the
    flow it came from, which a written line has a code of its own for."""
    text = re.sub(r"axiom\.ax:\d+", "axiom.ax", re.sub(r"derived by .*? from \S+:\d+", "", text))
    text = re.sub(r" ?\^[cd]\d+", "", text)
    text = re.sub(r" +", " ", re.sub(r"─+", "─", text.replace("·", "")))
    return re.sub(r" +$", "", re.sub(r" axiom\.ax$", "", text, flags=re.M), flags=re.M)


def written(text):
    """Less what only a book that has no laws and more lines says: how many laws and flows, who derived a flow, and that
    its derived lines are said to be recognized over a month (as the flow they came from was)."""
    text = re.sub(r" · \d+ laws? enforced", "", derivation(text))
    text = re.sub(r"(?<=[✓·] )\d+ flows?", "N flows", text)
    text = re.sub(r"(\d{4}-\d\d-\d\d) derived ", r"\1 flow ", text)
    return text.replace(" Flows written over a date range are recognized a little each day across the periods they cover.", "").rstrip()


def liquid(text):
    """The part of a forecast that is what the books will hold: the rest lists what recurs and what is promised."""
    return text.split("What recurs")[0]


def outputs(binary, project, commands):
    out = {}
    for command in commands:
        args = [binary, *command, "-C", project, "--today", TODAY, "--color", "never"]
        done = subprocess.run(args, capture_output=True, text=True, timeout=300)
        # The report is on stdout and what is wrong with the book is on stderr: only `check` is about the second.
        out[" ".join(command)] = tell(done.stdout + done.stderr if command[0] == "check" or not done.stdout else done.stdout)
    return out


def drawn(text):
    """What `available` says drawing the fund would owe, in cents: the books run the year out with the fund taken into
    checking, through the laws, and what they owe beyond what they already did is the cost."""
    found = re.search(r"driven by levy-owed ([\d,]+)\.(\d\d) USD", text)
    return int(found.group(1).replace(",", "")) * 100 + int(found.group(2)) if found else 0


def errors(text):
    """How many errors a check counts: a chain that is stopped is one, and the same one is said once however often it comes."""
    found = re.search(r"✗ (\d+) errors?", text)
    return int(found.group(1)) if found else 0


def pair(binary, directory, name):
    """What differs between the books of a project: the commands, and what each said of the chains that were stopped."""
    here = os.path.join(directory, name)
    commands = {"derived": COMMANDS + LAWS_ONLY, "law": COMMANDS + LAWS_ONLY, "written": COMMANDS, "ahead": [FORECAST]}
    derived, law, wrote, ahead = (outputs(binary, os.path.join(here, spelling, "axiom.ax"), commands[spelling]) for spelling in SPELLINGS)
    family, cycles, depths, _, owed = open(os.path.join(here, "expect")).read().split()
    wrong = []
    if drawn(derived["available"]) != int(owed):
        wrong.append(f"available:{drawn(derived['available'])} not {owed}")
    # A forecast is the fold: what a contract will make is the net worth of the book that has it kept.
    if liquid(derived[" ".join(FORECAST)]) != liquid(ahead[" ".join(FORECAST)]):
        wrong.append("forecast:ahead")
    for command, text in derived.items():
        if derivation(text) != derivation(law[command]) and not command.startswith("check"):
            wrong.append(f"law:{command}")
        if command in wrote and not command.startswith("check") and written(text) != written(wrote[command]):
            wrong.append(f"written:{command}")
    check = derived["check --all"]
    if errors(check) != int(cycles) + int(depths) or errors(law["check --all"]) != int(cycles) + int(depths):
        wrong.append(f"stopped:{errors(check)} not {int(cycles) + int(depths)}")
    clean = "error" not in wrote["check --all"] and "warning" not in wrote["check --all"]
    if not clean or (int(cycles) + int(depths) == 0 and ("error" in check or "warning" in check)):
        wrong.append("unclean")
    return wrong, family


def run(binary, directory, jobs=4, quiet=False):
    names = sorted(name for name in os.listdir(directory) if name.startswith("p"))
    with ThreadPoolExecutor(jobs) as pool:
        results = list(pool.map(lambda name: pair(binary, directory, name), names))
    differs = [(name, wrong) for name, (wrong, _) in zip(names, results) if wrong]
    if not quiet:
        for name, wrong in differs[:10]:
            print(f"{name}: {', '.join(wrong)}")
        families = Counter(family for _, family in results)
        print(f"{len(names)} projects, {len(differs)} differ; " + ", ".join(f"{f} {n}" for f, n in sorted(families.items())))
    return len(differs)


def compare(baseline, new, directory, jobs=4):
    """The books that have no law are what they were: the written books through both builds."""
    names = sorted(name for name in os.listdir(directory) if name.startswith("p"))
    commands = [command for command in COMMANDS if command[0] != "check"]

    def one(name):
        path = os.path.join(directory, name, "written", "axiom.ax")
        return outputs(baseline, path, commands) != outputs(new, path, commands)

    with ThreadPoolExecutor(jobs) as pool:
        differs = [name for name, differ in zip(names, pool.map(one, names)) if differ]
    print(f"{len(names)} written books, {len(differs)} differ between the builds: {differs[:10]}")
    return len(differs)


# ─── The engine's own dump ───────────────────────────────────────────────────────────────────────────────────────────

FLOW_ORDINAL = re.compile(r" ord=\d+")
PURPOSE_SOURCE = re.compile(r"purpose: (\w+#\d+), of: (None|Some\([^)]*\)), source: [^}]*\}")


def same_flow(flow):
    """An occurrence's flow as the dump prints it, less its place in the occurrence and where its purpose was read."""
    return PURPOSE_SOURCE.sub(r"purpose: \1, of: \2 }", FLOW_ORDINAL.sub("", flow))


def rows_of(binary, project, mode):
    """What the dump says of a project: `flow` rows (in order), `derived` rows (in order), and the rest, sorted."""
    done = subprocess.run([binary, mode, os.path.join(project, "axiom.ax"), TODAY, UNTIL], capture_output=True, text=True, timeout=300)
    rows = {"flow": [], "derived": [], "holding": [], "occurrence": [], "other": [], "diagnostic": []}
    for line in (done.stdout + done.stderr).splitlines():
        word = line.split(" ", 1)[0]
        if word in ("flow", "derived"):
            rows[word].append(line)
        elif word == "holding":
            rows["holding"].append(" ".join(line.split()[:5]))
        elif word in ("kept", "planned"):
            head, _, flows = line.partition(" [")
            rows["occurrence"] += [f"occurrence {head.split(' ', 1)[1]} {same_flow(flow)}" for flow in flows.rstrip("]").split(" | ")]
        elif word == "diagnostic":
            rows["diagnostic"].append(line)
        elif word in ("effect", "violation", "gain", "missed"):
            rows["other"].append(line)
    return {word: rows[word] if word in ("flow", "derived") else sorted(rows[word]) for word in rows}


def dumped(binary, directory, name):
    """What the dump says differs between the books of a project."""
    here = os.path.join(directory, name)
    derived, law, wrote, ahead = (rows_of(binary, os.path.join(here, spelling), "history") for spelling in SPELLINGS)
    forecast = rows_of(binary, os.path.join(here, "derived"), "forecast")
    wrong = []
    stopped = Counter(row.split()[1] for row in derived["diagnostic"] if row.split()[1].startswith("derive-"))
    family, cycles, depths, horizon, _ = open(os.path.join(here, "expect")).read().split()
    if (stopped["derive-cycle"], stopped["derive-depth"]) != (int(cycles), int(depths)):
        wrong.append(f"stopped:{dict(stopped)} not {cycles}/{depths}")
    if derived != law:
        wrong.append("law:history")
    if rows_of(binary, os.path.join(here, "law"), "forecast") != forecast:
        wrong.append("law:forecast")
    if sorted(derived["flow"]) != sorted(wrote["flow"]):
        wrong.append("written:flows")
    if derived["holding"] != wrote["holding"]:
        wrong.append("written:holdings")
    if forecast["derived"] != ahead["derived"]:
        wrong.append("forecast:derived")
    if forecast["holding"] != ahead["holding"]:
        wrong.append("forecast:holdings")
    # The occurrences between today and the last day the journal says anything of are kept in both: only past it is a promise.
    promised = [row for row in ahead["occurrence"] if row.split()[2] > horizon]
    if forecast["occurrence"] != promised:
        wrong.append("forecast:occurrences")
    if forecast["other"] != ahead["other"]:
        wrong.append("forecast:effects")
    return wrong, len(derived["flow"]), len(derived["derived"]) + len(forecast["derived"])


def dump(binary, directory, jobs=4, quiet=False):
    names = sorted(name for name in os.listdir(directory) if name.startswith("p"))
    with ThreadPoolExecutor(jobs) as pool:
        results = list(pool.map(lambda name: dumped(binary, directory, name), names))
    differs = [(name, wrong) for name, (wrong, _, _) in zip(names, results) if wrong]
    if not quiet:
        for name, wrong in differs[:10]:
            print(f"{name}: {', '.join(wrong)}")
        print(f"{len(names)} projects, {len(differs)} differ in the flows they post; {sum(f for _, f, _ in results)} flows, "
              f"{sum(d for _, _, d in results)} of them derived after today")
    return len(differs)


# ─── The mutants ─────────────────────────────────────────────────────────────────────────────────────────────────────

OFFSPRING = "crates/engine/src/offspring.rs"
STATE = "crates/engine/src/state.rs"
LAW = "crates/model/src/law.rs"
POST = "crates/engine/src/post.rs"
MOTION = "crates/engine/src/motion.rs"
RULES = "crates/model/src/rules.rs"
BOOK = "crates/model/src/book.rs"
SAID = "crates/model/src/said.rs"
LAWS = "crates/model/src/laws/mod.rs"
COMPILE = "crates/model/src/laws/compile/derive.rs"
HISTORY = "crates/report/src/history.rs"
FLOW = "crates/report/src/flow.rs"
REGISTER = "crates/report/src/register.rs"

# (file, the text, what replaces it, what the mutant is, the layer that can see it). Each must be caught by the oracle
# (the dump: the flows the fold posts; or the CLI: the views of them, in the order they list them), or by a unit test that
# names what it checks. The dump holds flows, holdings and diagnostics; it does not hold the order flows are listed in.
MUTANTS = [
    (OFFSPRING, "if self.laws().last() == Some(&law) {", "if false {",
     "a law watches what it made itself: the cash back of a card earns cash back, and is said to be a cycle", "dump"),
    (OFFSPRING, "if self.laws().contains(&law) {", "if false {",
     "a chain that comes back to a law is not stopped there but where it is too long", "dump"),
    (OFFSPRING, "pub(crate) const DEPTH: usize = 8;", "pub(crate) const DEPTH: usize = 9;", "a chain may have nine laws", "dump"),
    (OFFSPRING, "pub(crate) const DEPTH: usize = 8;", "pub(crate) const DEPTH: usize = 7;", "a chain may have seven laws", "dump"),
    (OFFSPRING, "waiting.extend(fresh.drain(..).rev());", "waiting.extend(fresh.drain(..));",
     "what a flow derived posts last-first: a chain is not read in the order the laws fired", "cli"),
    (OFFSPRING, "if self.scratch.brood.fresh.iter().any(|waiting| waiting.template == template) {", "if false {",
     "a law that watches both ends of a flow derives from it twice", "dump"),
    (OFFSPRING, "if !matches!(self.plan.events.state(id, &book.flows[id]), State::Returned(_)) {",
     "if matches!(self.plan.events.state(id, &book.flows[id]), State::Returned(_)) {",
     "the flows a returned flow derived are the ones that are not remembered", "dump"),
    (OFFSPRING, "self.post_flow(&forward.reversed());", "self.post_flow(&forward);",
     "a return posts what the flow derived again instead of back", "dump"),
    (OFFSPRING, "let payee = book.party_at(flow.to).or_else(|| book.party_at(flow.from));",
     "let payee = book.party_at(flow.from).or_else(|| book.party_at(flow.to));",
     "a derived flow is paid to the party at the end it comes from before the end it goes to", "dump"),
    (OFFSPRING, "let flow = Flow { day: m.day, mode: Mode::Actual, recognized: ctx.over, payee, ..flow };",
     "let flow = Flow { day: m.day, mode: Mode::Actual, recognized: ctx.over, ..flow };",
     "a derived flow is paid to whoever the flow it came from was", "dump"),
    (OFFSPRING, "_ if stands(m.from) => m.from,", "_ if stands(m.to) => m.to,",
     "`self` is the end of the flow at the place the law does not govern when both ends are", "dump"),
    (OFFSPRING, "Subject::Asset(asset) => book.assets[asset].place,", "Subject::Asset(asset) => m.to,",
     "`self` in the law of an asset is the end of the flow", "dump"),
    (OFFSPRING, "self.record.stopped.insert((lineage, law))", "true",
     "a chain that is stopped is said every time it is, not once", "dump"),
    (OFFSPRING, "Id::new(self.record.first_offspring + self.record.offspring.len() as u32)",
     "Id::new(self.record.offspring.len() as u32)", "a record that goes on from a checkpoint numbers its flows from zero", "dump"),
    (POST, "        self.fire_touching(m, on);\n", "", "the laws of the places a flow touches never fire", "dump"),
    (POST, "let on = Occasion { amount: Some(m.out), skip_internal: true, ..*on };",
     "let on = Occasion { amount: Some(m.out), skip_internal: false, ..*on };",
     "value that moved around inside what a law governs fires it", "dump"),
    (POST, "if let Some(Object::Asset(asset)) = purpose.of {", "if let Some(Object::Asset(asset)) = None::<Object> {",
     "the laws of an asset do not see the money that was for it", "dump"),
    (POST, "        self.fire_touching(m, on);\n        self.fire_purpose(m, on);",
     "        self.fire_purpose(m, on);\n        self.fire_touching(m, on);",
     "the laws of a purpose fire before the laws of the places a flow touches", "cli"),
    (MOTION, "self.course == Course::Forward && self.cause != Cause::Time", "self.cause != Cause::Time",
     "a flow that is run backwards derives", "dump"),
    (MOTION, "self.course == Course::Forward && self.cause != Cause::Time", "self.course == Course::Forward",
     "a flow the run makes to be consistent with itself (a pad, a claim the monitor makes) derives", "dump"),
    (RULES, "Trigger::Flow => Some(Watch::Touching(place)),", "Trigger::Flow => None,",
     "a law that is not a contract's is looked up by no place", "dump"),
    (RULES, "for governing in book.places.lineage(place) {", "for governing in std::iter::once(place) {",
     "the law of an account is not the law of the accounts that lie in it", "dump"),
    (RULES, ".filter(|&&law| book.laws[law].trigger == Trigger::Flow);\n    flows.map(move |&law| always(law, Subject::Entity(entity)))",
     ".filter(|&&law| book.laws[law].trigger == Trigger::Always);\n    flows.map(move |&law| always(law, Subject::Entity(entity)))",
     "the party that stands at a place says nothing of the flows there", "dump"),
    (RULES, "let kind_laws = book.kinds.lineage(book.entities[entity].kind).flat_map(|kind| book.kinds[kind].laws.iter());\n    let laws = kind_laws.chain(written.entities[entity].iter());",
     "let kind_laws = book.kinds.lineage(book.entities[entity].kind).flat_map(|kind| book.kinds[kind].laws.iter()).take(0);\n    let laws = kind_laws.chain(written.entities[entity].iter());",
     "the kind of a party says nothing of the flows at its places", "dump"),
    (SAID, "Role::Outside(Some(party)) => party,", "Role::Outside(Some(_)) => self.places[place].owner,",
     "the party whose outside a place is does not stand there", "dump"),
    (BOOK, "Stand::Subject => subject.unwrap_or(own),", "Stand::Subject => own,",
     "`self` is where the flow has its own end", "dump"),
    (BOOK, "Shape::Item(Sign::Less) => (header.to, header.from),", "Shape::Item(Sign::Less) => (header.from, header.to),",
     "a `-` item goes the way of the flow it comes with", "dump"),
    (BOOK, "purpose: self.purpose.or(header.purpose),", "purpose: self.purpose,",
     "a derived flow that says no purpose is for nothing, not for what the flow it came from was for", "dump"),
    (BOOK, "Shape::Item(Sign::Carve) => false,", "Shape::Item(Sign::Carve) => true",
     "a law that is not a contract's may carve a part out of a flow that has posted", "dump"),
    (LAWS, "Owner::Purpose(_) | Owner::Asset(_) | Owner::Contract(_) | Owner::Place(_) | Owner::Entity(_)\n            ) || place_kind\n                || thing_kind\n                || entity_kind",
     "Owner::Purpose(_) | Owner::Asset(_) | Owner::Contract(_) | Owner::Place(_) | Owner::Entity(_)\n            ) || place_kind\n                || thing_kind",
     "a law of a kind of party cannot be on `flow`", "dump"),
    (COMPILE, "if !in_contract && !derived.follows_a_posted_flow() {", "if false {",
     "an item that is part of a flow is accepted in the law of a card", "dump"),
    (HISTORY, "State::Returned(on) => State::Returned(on),", "State::Returned(_) => State::Actual,",
     "a derived flow is not returned with the flow it came from, as the reports see it", "cli"),
    (HISTORY, "PostingId::Derived(id) => match run.offspring[id.index()].root {\n                Cause::Flow(root) => (root.index() as u32, 1 + id.index() as u32),",
     "PostingId::Derived(id) => match run.offspring[id.index()].root {\n                Cause::Flow(root) => (root.index() as u32, 0),",
     "a derived flow is listed anywhere among the flows of its day", "cli"),
    (FLOW, "for posting in all_postings(book, run).filter(|posting| posting.is_real_on(cutoff)) {",
     "for posting in crate::history::postings(book, run).filter(|posting| posting.is_real_on(cutoff)) {",
     "the flow view counts the lines of the journal and not what the laws derived", "cli"),
    (REGISTER, "let touching = all_postings(book, run).filter(|posting| touches_entity(lens, posting.flow, entity, window));",
     "let touching = crate::history::postings(book, run).filter(|posting| touches_entity(lens, posting.flow, entity, window));",
     "the register of a party lists the lines and not what was derived of them", "cli"),
    (MOTION, "self.course == Course::Forward && self.cause != Cause::Time",
     "self.course == Course::Forward && self.cause != Cause::Time && !matches!(self.cause, Cause::Applied(_))",
     "a flow handed to the fold, as `available` hands it one, derives nothing", "cli"),
    (STATE, "first_offspring: self.first_offspring + self.offspring.len() as u32,", "first_offspring: 0,",
     "a record forked from a checkpoint begins numbering its flows at zero", "dump"),
    (OFFSPRING, "if !matches!(m.cause, Cause::Derived(_)) {\n            self.scratch.brood.begin(m);\n        }", "",
     "a flow no law derived descends through what the last chain did", "dump"),
    (LAW, "!self.table(table).is_empty()", "false", "a book never looks up what a flow touches", "dump"),
    (LAW, "!self.table(table).is_empty()", "true", "a book always looks up what a flow touches, which only costs", "dump"),
]


def detect(source, work, layer=None):
    """What the oracle says of a build of SOURCE on the sample: the engine's dump of the flows (for a mutant that changes
    what the engine posts) and then the CLI's views of them (for one that changes how they are listed)."""
    from forecast import build as build_dump
    from forecast import build_cli

    sample = os.path.join(work, "sample")
    if layer in (None, "dump"):
        binary = build_dump(source, os.path.join(work, "dump"))
        if dump(binary, sample, 3, quiet=True):
            return "killed by the engine dump"
    if run(build_cli(source, work), sample, 3, quiet=True):
        return "killed by the CLI views"
    return None


def mutate(tree, work, directory, only=None):
    """Each mutant must be caught by the oracle on the first projects of DIRECTORY, or by a test that fails only with it."""
    from mutation import mutate as run_mutants

    work = os.path.abspath(work)
    sample = os.path.join(work, "sample")
    shutil.rmtree(sample, ignore_errors=True)
    os.makedirs(sample)
    for name in sorted(n for n in os.listdir(directory) if n.startswith("p"))[:SAMPLE]:
        shutil.copytree(os.path.join(directory, name), os.path.join(sample, name))
    return run_mutants(tree, work, MUTANTS, detect, only)


SAMPLE = 120


def main(argv):
    if argv[1] == "gen":
        return gen(argv[2], int(argv[3]), int(argv[4]) if len(argv) > 4 else 1)
    if argv[1] == "run":
        return run(argv[2], argv[3], int(argv[4]) if len(argv) > 4 else 4) and 1
    if argv[1] == "dump":
        return dump(argv[2], argv[3], int(argv[4]) if len(argv) > 4 else 4) and 1
    if argv[1] == "compare":
        return compare(argv[2], argv[3], argv[4]) and 1
    if argv[1] == "mutate":
        return mutate(argv[2], argv[3], argv[4], {int(n) for n in argv[5].split(",")} if len(argv) > 5 else None)
    raise SystemExit(__doc__)


if __name__ == "__main__":
    sys.exit(main(sys.argv) or 0)
