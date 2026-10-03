#!/usr/bin/env python3
"""A generator of small books full of splits, and a differential run of two builds of the CLI over them.

    splits.py gen DIR N [SEED [KIND]]           write N projects into DIR (p0000/main.ax ...), and DIR/forms.json
                                                KIND: all (default), promises (contracts only), statements (no
                                                contracts), or recipe:NAME (one block of one recipe, so that a
                                                difference between two builds is that recipe's)
    splits.py run BASELINE NEW DIR [JOBS]       run the commands below over every project through both binaries
    splits.py all BASELINE NEW DIR N [SEED]     gen, then run
    splits.py internals BASELINE NEW DIR [JOBS] compare what two builds of `internals/main.rs` print for each project
    splits.py survey BINARY DIR                 which diagnostics BINARY raises over the projects
    splits.py equiv BASELINE NEW DIR N [SEED]   a split statement against the plain transfers it says (LANGUAGE §3)

What it is for. Lane K4a unifies the types that say how a statement or a promise splits (a header, its legs and
its items) without changing what any of them means. `docs/v5/measure/diff/` has the mistakes and the valid projects
of lane K0a, and does not stress splits. This does. Every project is a few recipes picked by a seeded random
generator (deterministic: the same SEED and N write the same books), each recipe a block of lines that uses one
or more of the forms a quantity can take:

    headers   an amount, `(pending)`, `all`, `? UNIT` solved by a later balance, a computed amount (`12% of 100 USD`),
              `@ PRICE`, an exchange, a source named alone (`checking 100 USD ->`) or the target alone (`-> checking`)
    legs      an amount, `(pending)`, `...` (the remainder), `= TARGET`, `all`, a share (`30%`, contracts), computed
    items     `AMOUNT` (carved), `+ AMOUNT` (added), `- AMOUNT` (taken off), with and without purposes, in the
              header's unit and in another, computed (`10% of amount`)
    promises  a fixed amount, `about`, a standing `buy`, legs and items, escalation (`rising`, `indexed to`), inputs,
              a loan whose payment is derived, a deadline `due ... else`, `covers`, `prorated`
    kept      a written occurrence that states another amount (literal, computed, pending), replaces a leg, adds a leg,
              adds items, binds an input or leaves one out, carries a tail of its own
    trades    an exchange with a cost item (a purchase and a sale), an asset sale with a selling cost

Each project is run through `check`, `balance`, `register` of every place, `flow`, `contracts`, `claims`, `lots`,
`gains`, `forecast`, and `why` of every line and every contract, with and without `--json` where the form
differs, and the run fails on the first difference in stdout, stderr or exit status between the two binaries.

What the commands show is not all the engine decides. `internals/main.rs` is a small program (it is not a member
of the workspace: give it a Cargo.toml with path dependencies on the `core`, `syntax`, `model`, `engine` and
`systems` crates of a tree, and build it once for each tree) that prints, for a project's file, every promise the
fold kept or missed with the flows it materialized, the debug text of each posted flow, gain and holding, and what
`instantiate_occurrence` makes of every due day to the end of 2027 as the forecast would call it. `internals` runs
two builds of it over the projects. A mutation that changes only a flow's mode, which no report prints, is seen by
it and by nothing else.

It is not vacuous, and says so: `gen` counts the forms it writes, and `run` reports, from what the BASELINE
printed, how many projects were clean (no error), silent (no diagnostic at all), and moved money, and how many of
each of those hold each form. A generator whose books are all rejected would prove nothing; the report shows it
would be seen.
"""
import hashlib
import json
import os
import random
import re
import subprocess
import sys
import tempfile
from collections import Counter
from concurrent.futures import ThreadPoolExecutor

TODAY = "2026-06-30"

PRELUDE = """\
use std
base USD
commodity VTI : stock
  precision 3
commodity EURX : currency
  precision 2
purpose fees : spending
purpose selling-costs : spending
purpose sale : capital
  of asset
param cpi
  2026 100
param fee-rate
  2026-01-01 2%
  2026-03-10 5%
  2026-05-01 9%
entity me : person
entity acme : org
entity shop : org
entity buyer : org
entity broker-co : org
entity landlord : landlord
account checking : bank
account savings : bank
account bonus : bank
account reserve : bank
account wallet : cash
account broker : brokerage
asset condo : property
asset laptop : good
opening 2026-01-01
  checking 200_000 USD
  checking 1_000 EURX
  savings 5_000 USD
  bonus 5_000 USD
  reserve 5_000 USD
  wallet 2_000 USD
  broker 10 VTI basis 1_000 USD since 2020-01-01
  condo basis 400 USD since 2020-01-01
2026-01-01 EURX = 1.1 USD
2026-01-01 VTI = 120 USD
"""

# Where value can move, by kind, and what a party is for.
ACCOUNTS = ["checking", "savings", "bonus", "reserve"]
PARTIES = ["shop", "acme", "buyer"]
PURPOSES = ["fun", "groceries", "household", "dining", "transport", "fees"]


class Book:
    """Lines of one project, and the forms they use."""

    def __init__(self, rng):
        self.rng = rng
        self.lines = []
        self.forms = Counter()
        self.codes = 0
        self.contracts = 0

    def add(self, text, *forms):
        self.lines.extend(text.rstrip("\n").split("\n"))
        for form in forms:
            self.forms[form] += 1

    def code(self):
        self.codes += 1
        return f"^c{self.codes}"

    def day(self, month=None):
        month = month or self.rng.randint(2, 5)
        return f"2026-{month:02d}-{self.rng.randint(2, 27):02d}"


def amount(rng, low=5, high=400):
    return rng.randint(low, high)


def usd(rng, low=5, high=400):
    return f"{amount(rng, low, high)} USD"


def tail(rng, forms):
    """A written tail: a purpose, a description, a code, in the order `axiom fmt` writes them."""
    parts = []
    if rng.random() < 0.5:
        parts.append("#" + rng.choice(PURPOSES))
        forms["tail:purpose"] += 1
    if rng.random() < 0.2:
        parts.append(rng.choice(['"for the month"', '"a note"']))
        forms["tail:description"] += 1
    return (" " + " ".join(parts)) if parts else ""


def computed(rng, forms, header_unit="USD"):
    """A computed amount of the journal: a share of an amount written out."""
    kind = rng.choice(["pct", "pct", "fraction"])
    base = amount(rng, 20, 300)
    if kind == "pct":
        forms["amount:computed-share"] += 1
        return f"{rng.choice([5, 10, 12, 25, 50])}% of {base} {header_unit}"
    forms["amount:computed-fraction"] += 1
    return f"{rng.choice(['1/3', '1/4', '2/5'])} of {base} {header_unit}"


def declared(rng, forms, header_unit="USD"):
    """A computed amount of a declaration (a template): the journal's forms, or a param that changes with the day."""
    if rng.random() < 0.5:
        return computed(rng, forms, header_unit)
    forms["amount:computed-param"] += 1
    if rng.random() < 0.5:
        return f"fee-rate * {rng.randint(200, 3_000)} {header_unit}"
    return f"cpi * {rng.randint(1, 9)} {header_unit}"


# ─── Statements ──────────────────────────────────────────────────────────────────────────────────────────────


def transfer(book):
    """One flow between two places, whichever way its amount is written."""
    rng, forms = book.rng, Counter()
    a, b = rng.choice([("checking", "savings"), ("savings", "checking"), ("checking", "shop"), ("buyer", "checking"),
                       ("checking", "acme"), ("savings", "bonus"), ("bonus", "checking"), ("checking", "reserve")])
    form = rng.choices(["amount", "pending", "all", "computed", "price"], [8, 3, 2, 3, 2])[0]
    day = book.day()
    mark = book.code() if rng.random() < 0.3 else ""
    if form == "amount":
        forms["hdr:amount"] += 1
        text = f"{day} {a} -> {b} {usd(rng)}{tail(rng, forms)} {mark}"
    elif form == "pending":
        forms["hdr:pending"] += 1
        text = f"{day} {a} -> {b} ({usd(rng)}){tail(rng, forms)} {mark}"
    elif form == "all":
        forms["hdr:all"] += 1
        text = (f"{book.day(2)} checking -> reserve {usd(rng, 50, 300)}\n"
                f"{day} reserve all -> {rng.choice(['savings', 'bonus', 'shop'])} {mark}")
    elif form == "computed":
        forms["hdr:computed"] += 1
        text = f"{day} {a} -> {b} {computed(rng, forms)}{tail(rng, forms)} {mark}"
    else:
        forms["hdr:price"] += 1
        text = f"{day} checking {usd(rng, 50, 300)} -> wallet {rng.randint(40, 300)} EURX {mark}"
    book.add(text.strip(), *forms)
    book.forms.update(forms)


def unknown(book):
    """`? USD` solved by a balance written after it."""
    rng = book.rng
    start, spent, left = amount(rng, 200, 600), amount(rng, 20, 150), None
    left = start - spent
    day = book.day(3)
    source = rng.choice(["reserve", "reserve"])
    other = rng.choice(["savings", "bonus", "shop"])
    form = rng.choice(["unknown", "unknown", "unknown-split", "unknown-exchange"])
    book.add(f"{day} checking -> {source} {start} USD")
    if form == "unknown-exchange":
        bought, sold = rng.randint(1, 4), rng.randint(1, 3)
        book.add(f"{day} checking {rng.choice([2, 5, 10])}% of {rng.randint(200, 900)} USD -> broker {bought} VTI\n"
                 f"{day} broker ? VTI -> checking {rng.randint(100, 400)} USD\n"
                 f"{day} broker = {10 + bought - sold} VTI", "hdr:unknown", "hdr:exchange", "unknown:computed-exchange")
    elif form == "unknown":
        book.add(f"{day} {source} -> {other} ? USD\n{day} {source} = {left} USD", "hdr:unknown")
    else:
        book.add(f"{day} {source} ->\n  {other} ? USD\n  shop {rng.randint(5, 30)} USD\n{day} {source} = {left} USD",
                 "hdr:unknown", "leg:unknown", "split:from")


def items_under_header(book):
    """A flow with items under it: carved, added, taken off; with and without purposes; computed; another unit."""
    rng, forms = book.rng, Counter()
    a, b = rng.choice([("checking", "shop"), ("checking", "acme"), ("buyer", "checking"), ("checking", "savings")])
    header = rng.randint(100, 600)
    day = book.day()
    purpose = rng.choice(["", " #household", " #fun"])
    lines = [f"{day} {a} -> {b} {header} USD{purpose}"]
    forms["hdr:amount"] += 1
    for _ in range(rng.randint(1, 3)):
        sign = rng.choice(["", "+ ", "- ", "+ ", "- "])
        forms["item:" + {"": "carve", "+ ": "add", "- ": "less"}[sign]] += 1
        forms["item:header"] += 1
        what = rng.choices(["usd", "computed", "other-unit", "bare"], [6, 3, 1, 1])[0]
        if what == "bare" and sign == "- ":
            what = "usd"
        if what == "other-unit" and (sign == "- " or a != "checking"):
            what = "usd"
        extra = rng.choice([" #fees", " #groceries", " #fun", " #fees", ' #fun "a note"', " #fun ^t%d" % rng.randint(1, 999)])
        if what == "usd":
            qty = f"{rng.randint(1, 30)} USD"
        elif what == "computed":
            qty = f"{rng.choice([5, 10, 25])}% of amount"
            forms["item:computed"] += 1
        elif what == "other-unit":
            qty = f"{rng.randint(1, 30)} EURX"
            forms["item:other-unit"] += 1
        else:
            qty, extra = f"{rng.randint(1, 30)} USD", ""
        if extra:
            forms["item:purpose"] += 1
        else:
            forms["item:nopurpose"] += 1
        lines.append(f"  {sign}{qty}{extra}")
    book.add("\n".join(lines), *forms)
    book.forms.update(forms)


def split(book):
    """A split: the header names one end, the legs the other, with or without a header amount and items."""
    rng, forms = book.rng, Counter()
    day = book.day()
    from_side = rng.random() < 0.5
    forms["split:from" if from_side else "split:to"] += 1
    shared = rng.choice(["checking", "savings"] if from_side else ["checking", "bonus"])
    ends = rng.sample(["shop", "acme", "bonus", "reserve", "savings", "buyer"], 4)
    ends = [end for end in ends if end != shared][:3]
    amount_at = rng.choice(["before", "after", "none"])
    total = amount(rng, 150, 900)
    mark = book.code() if rng.random() < 0.2 else ""
    if amount_at == "before":
        forms["hdr:amount"] += 1
        header = f"{day} {shared} {total} USD ->" if from_side else f"{day} -> {shared} {total} USD"
    elif amount_at == "after":
        forms["hdr:amount"] += 1
        header = f"{day} {shared} -> {total} USD" if from_side else f"{day} {total} USD -> {shared}"
    else:
        forms["hdr:none"] += 1
        header = f"{day} {shared} ->" if from_side else f"{day} -> {shared}"
    lines = [header + (" " + mark if mark else "")]
    rest_at = rng.choice([None, None, "last", "first"])
    kinds = []
    for index, end in enumerate(ends):
        kinds.append(rng.choices(["amount", "pending", "target", "all", "computed", "tail"],
                                 [10, 2, 2, 1, 2, 2])[0])
    if rest_at == "last":
        kinds[-1] = "rest"
    elif rest_at == "first":
        kinds[0] = "rest"
    for end, kind in zip(ends, kinds):
        if kind == "amount":
            qty, key = usd(rng, 10, 120), "leg:amount"
        elif kind == "pending":
            qty, key = f"({usd(rng, 10, 90)})", "leg:pending"
        elif kind == "target":
            # a leg that receives ends above what it holds; one that gives, at or below it
            qty, key = f"= {rng.randint(40_000, 90_000) if from_side else 0} USD", "leg:target"
            if end in PARTIES:
                qty, key = usd(rng, 10, 120), "leg:amount"
        elif kind == "all":
            qty, key = "all", "leg:all"
        elif kind == "computed":
            qty, key = computed(rng, forms), "leg:computed"
        elif kind == "rest":
            qty, key = "...", "leg:rest"
        else:
            qty, key = usd(rng, 10, 120) + " #" + rng.choice(PURPOSES) + ' "a leg"', "leg:amount"
            forms["tail:purpose"] += 1
        forms[key] += 1
        lines.append(f"  {end} {qty}")
    if rng.random() < 0.4:
        for _ in range(rng.randint(1, 2)):
            sign = rng.choice(["", "+ ", "- "])
            forms["item:" + {"": "carve", "+ ": "add", "- ": "less"}[sign]] += 1
            forms["item:split"] += 1
            forms["item:purpose"] += 1
            lines.append(f"  {sign}{rng.randint(1, 25)} USD #{rng.choice(['fees', 'fun', 'groceries'])}")
    book.add("\n".join(lines), *forms)
    book.forms.update(forms)


def cost_amount(rng, forms):
    """What a cost item under a trade says: an amount, or, now and then, a share of one that is computed."""
    if rng.random() < 0.7:
        return f"{rng.randint(2, 20)} USD"
    forms["item:computed"] += 1
    forms["ex:cost-computed"] += 1
    return rng.choice([f"{rng.choice([2, 5, 10])}% of {rng.randint(50, 300)} USD", f"{rng.choice([1, 2])}% of amount"])


def exchange(book):
    """A trade: a purchase or a sale, with a cost item, at a stated or a quoted price."""
    rng, forms = book.rng, Counter()
    day = book.day()
    kind = rng.choice(["buy", "sell", "buy-cost", "sell-cost", "buy-price", "sale-asset", "purchase-asset"])
    cost = cost_amount(rng, forms)
    if kind == "buy":
        text = f"{day} checking {rng.randint(300, 900)} USD -> broker {rng.randint(1, 5)} VTI"
        forms["hdr:exchange"] += 1
    elif kind == "sell":
        text = f"{day} broker {rng.randint(1, 4)} VTI -> checking {rng.randint(300, 900)} USD"
        forms["hdr:exchange"] += 1
    elif kind == "buy-cost":
        text = (f"{day} checking {rng.randint(300, 900)} USD -> broker {rng.randint(1, 5)} VTI\n"
                f"  {cost} #fees")
        forms["hdr:exchange"] += 1
        forms["ex:cost-purchase"] += 1
        forms["item:carve"] += 1
    elif kind == "sell-cost":
        sign = rng.choice(["- ", "- ", ""])
        text = (f"{day} broker {rng.randint(1, 4)} VTI -> checking {rng.randint(300, 900)} USD\n"
                f"  {sign}{cost} #fees")
        forms["hdr:exchange"] += 1
        forms["ex:cost-sale"] += 1
        forms["item:less" if sign else "item:carve"] += 1
    elif kind == "buy-price":
        text = f"{day} checking -> broker {rng.randint(1, 5)} VTI @ {rng.randint(100, 300)} USD"
        forms["hdr:price"] += 1
    elif kind == "sale-asset":
        selling = f"{rng.randint(5, 50)} USD" if rng.random() < 0.85 else "10% of 100 USD"
        text = f"{day} buyer -> checking {rng.randint(300, 900)} USD #sale of condo\n  - {selling} #selling-costs"
        forms["ex:asset-sale"] += 1
        forms["item:less"] += 1
    else:
        text = (f"{day} checking -> shop {rng.randint(300, 900)} USD #purchase of laptop\n"
                f"  + {rng.randint(5, 30)} USD #fees")
        forms["ex:asset-purchase"] += 1
        forms["item:add"] += 1
    book.add(text, *forms)
    book.forms.update(forms)


def claims(book):
    """A claim written whole, or only by its items."""
    rng, forms = book.rng, Counter()
    day = book.day(2)
    code = book.code()
    if rng.random() < 0.5:
        book.add(f"{day} shop owes me {usd(rng, 20, 200)} {code}" + rng.choice(["", " due 30d", " #fun"]), "claim:whole")
    else:
        lines = [f"{day} shop owes me due 30d {code}"]
        for _ in range(rng.randint(1, 3)):
            sign = rng.choice(["", "+ ", "- "])
            forms["item:" + {"": "carve", "+ ": "add", "- ": "less"}[sign]] += 1
            qty = usd(rng, 5, 60) if rng.random() < 0.8 else f"{rng.choice([5, 10])}% of amount"
            lines.append(f"  {sign}{qty} #{rng.choice(['fun', 'fees'])}")
        forms["claim:items"] += 1
        book.add("\n".join(lines), *forms)
        book.forms.update(forms)
        return
    if rng.random() < 0.5:
        book.add(f"{book.day(4)} shop -> checking {usd(rng, 5, 60)} {code}", "claim:settle")


def basis(book):
    """An asset's basis, and a flow whose basis is computed."""
    rng = book.rng
    if rng.random() < 0.5:
        book.add(f"{book.day()} laptop basis {usd(rng, 200, 900)} since 2025-0{rng.randint(1, 9)}-01", "basis:statement")
    else:
        book.add(f"{book.day()} checking -> shop {usd(rng, 100, 500)} basis {rng.choice([5, 10])}% of {amount(rng, 200, 900)} USD",
                 "basis:computed")


# ─── Promises ────────────────────────────────────────────────────────────────────────────────────────────────


class Contract:
    """The pieces of one promise, so that its written occurrences can be made to fit."""

    def __init__(self, name, direction, holding, day, every, start):
        self.name, self.direction, self.holding = name, direction, holding
        self.day, self.every, self.start = day, every, start
        self.legs, self.inputs, self.has_items = [], [], False
        self.until = 6
        self.first = start


def due_days(contract, book):
    """Some due days of a monthly promise that fall before today: the days its occurrences may be written on."""
    first = int(contract.start[5:7])
    return [f"2026-{month:02d}-{contract.day:02d}" for month in range(first, contract.until + 1)]


def contract(book):
    rng, forms = book.rng, Counter()
    book.contracts += 1
    name = rng.choice(["salary", "retainer", "stipend", "lease", "flat", "plan", "dues"]) + str(book.contracts)
    party = rng.choice(["acme", "shop", "broker-co", "acme", "shop", "broker-co", "me"])
    direction = rng.choice(["into", "from"])
    holding = rng.choice(["checking", "checking", "savings", "savings", "wallet", "shop"])
    day = rng.choice([1, 5, 15, 28])
    start_month = rng.choice([1, 2])
    start = f"2026-{start_month:02d}-{day:02d}"
    shape = rng.choices(["fixed", "about", "buy", "loan", "twice", "weekly"], [10, 2, 4, 3, 1, 1])[0]
    if shape in ("loan", "buy"):
        holding = rng.choice(["checking", "checking", "savings"])
    c = Contract(name, direction, holding, day, "monthly", start)
    lines = [f"contract {name} with {party}"]
    if shape == "loan":
        forms["contract:loan"] += 1
        principal = rng.choice([30_000, 100_000])
        extra = rng.choice(["", " for condo"])
        lines.append(f"  loan {principal} USD on 2026-01-01 at {rng.choice(['4', '5.5', '0'])}% over {rng.choice(['3y', '10y'])}{extra}")
        if rng.random() < 0.3:
            lines.append("    prepay " + rng.choice(["recasts", "shortens"]))
        lines.append(f"  monthly on {day} from {holding}")
        lines.append(f"  from 2026-02-{day:02d}")
        c.loan = True
        c.first = f"2026-02-{day:02d}"
    else:
        amount_text = f"{rng.randint(500, 3_000)} USD"
        if shape == "fixed":
            forms["contract:fixed"] += 1
            if rng.random() < 0.3:
                amount_text = computed(rng, forms)
                forms["contract:computed"] += 1
            lines.append(f"  {amount_text} monthly on {day} {direction} {holding}" + rng.choice(["", " #fun", ' "a promise"']))
        elif shape == "about":
            forms["contract:about"] += 1
            lines.append(f"  about {amount_text} monthly on {day} {direction} {holding}")
        elif shape == "buy":
            forms["contract:buy"] += 1
            lines.append(f"  buy VTI for {rng.randint(100, 800)} USD monthly on {day} from {holding}")
        elif shape == "twice":
            forms["contract:twice-monthly"] += 1
            lines.append(f"  {amount_text} twice monthly on {day}, last {direction} {holding}")
        else:
            forms["contract:weekly"] += 1
            lines.append(f"  {amount_text} weekly on monday {direction} {holding}")
        lines.append(f"  from {start}")
        if rng.random() < 0.15:
            lines.append("  until 2026-04-28")
            c.until = 4
        if shape != "buy":
            c.legs = template_legs(rng, forms, lines, party)
        elif rng.random() < 0.5:
            exchange_template(rng, forms, lines)
        if rng.random() < 0.18 and shape in ("fixed", "twice"):
            lines.append("  rising 3% yearly" if rng.random() < 0.6 else "  indexed to cpi yearly")
            forms["contract:escalation"] += 1
        if rng.random() < 0.2 and shape != "buy":
            lines.append("  input water USD")
            lines.append(f"  + {rng.choice([10, 12])}% of water #fees")
            forms["contract:input"] += 1
            forms["item:computed"] += 1
            forms["item:add"] += 1
            c.inputs = ["water"]
        if rng.random() < 0.1:
            lines.append("  due 5d else + 5% of 100 USD #fees")
            forms["contract:due-else"] += 1
        if rng.random() < 0.12:
            lines.append("  covers the month")
            forms["contract:covers"] += 1
            if rng.random() < 0.5:
                lines.append("  prorated")
                forms["contract:prorated"] += 1
        if rng.random() < 0.1 and holding in ("checking", "savings"):
            lines.append(f"  deposit {rng.randint(200, 900)} USD")
            forms["contract:deposit"] += 1
    book.add("\n".join(lines), *forms)
    book.forms.update(forms)
    occurrences(book, c, shape)


def template_legs(rng, forms, lines, party):
    """Legs and items a promise's template carries, each of the kinds a template leg may be."""
    made = []
    if rng.random() < 0.7:
        choices = ["savings", "bonus", "reserve"]
        rng.shuffle(choices)
        kinds = rng.sample(["amount", "share", "rest", "pending", "target", "all", "computed", "tail", "unknown"], rng.randint(1, 3))
        for kind, end in zip(kinds, choices):
            if kind == "amount":
                qty, key = f"{rng.randint(50, 400)} USD", "leg:amount"
            elif kind == "share":
                qty, key = f"{rng.choice([5, 10, 30])}%", "leg:share"
            elif kind == "rest":
                qty, key = "...", "leg:rest"
            elif kind == "pending":
                qty, key = f"({rng.randint(50, 200)} USD)", "leg:pending"
            elif kind == "target":
                qty, key = f"= {rng.randint(40_000, 90_000)} USD", "leg:target"
            elif kind == "all":
                qty, key = "all", "leg:all"
            elif kind == "computed":
                qty, key = declared(rng, forms), "leg:computed"
            elif kind == "unknown":
                qty, key = "? USD", "promise-leg:unknown"
            else:
                qty, key = f"{rng.randint(20, 90)} USD #{rng.choice(['fees', 'fun'])}", "leg:amount"
            if kind == "rest" and any(m[1] == "rest" for m in made):
                continue
            forms[key] += 1
            lines.append(f"  {end} {qty}")
            made.append((end, kind))
    if rng.random() < 0.3:
        for _ in range(rng.randint(1, 2)):
            sign = rng.choice(["", "+ ", "- "])
            forms["item:" + {"": "carve", "+ ": "add", "- ": "less"}[sign]] += 1
            forms["item:header"] += 1
            forms["item:purpose"] += 1
            if sign and rng.random() < 0.3:
                forms["item:computed"] += 1
                qty = declared(rng, forms)
            else:
                qty = f"{rng.randint(1, 30)} USD"
            lines.append(f"  {sign}{qty} #{rng.choice(['fees', 'fun'])}")
    return made


def combo(book, c, lines, forms):
    """An occurrence that says several things at once: another amount, a leg replaced, a leg added, items."""
    rng = book.rng
    lines[0] += f" {rng.randint(300, 3_500)} USD"
    forms["occ:amount"] += 1
    taken = [m[0] for m in c.legs]
    if c.legs:
        how = rng.choice(["amount", "pending"])
        qty = f"{rng.randint(20, 400)} USD" if how == "amount" else f"({rng.randint(20, 300)} USD)"
        lines.append(f"  {c.legs[0][0]} {qty}")
        forms["occ:replace-leg"] += 1
        forms["occ-leg:" + how] += 1
    free = [end for end in ("bonus", "reserve", "savings") if end not in taken]
    if free:
        lines.append(f"  {rng.choice(free)} {rng.randint(10, 200)} USD")
        forms["occ:add-leg"] += 1
    for _ in range(rng.randint(1, 2)):
        sign = rng.choice(["+ ", "- "])
        forms["item:" + {"+ ": "add", "- ": "less"}[sign]] += 1
        forms["occ:item"] += 1
        lines.append(f"  {sign}{rng.randint(1, 40)} USD #{rng.choice(['fees', 'fun'])}")
    forms["occ:combo"] += 1


def shifted(c, due, rng):
    """The day a promise's occurrence is written on when it is not its due day: a few days early or late."""
    year, month, day = (int(part) for part in due.split("-"))
    offsets = [offset for offset in (-3, -2, -1, 1, 2, 3)
               if 1 <= day + offset <= 28 and (due != c.first or offset > 0) and (c.until == 6 or offset < 0)]
    return f"{year}-{month:02d}-{day + rng.choice(offsets):02d}" if offsets else due


def exchange_template(rng, forms, lines):
    """What a promise to buy may carry under its exchange header: a share of it, and items in its spend unit."""
    if rng.random() < 0.5:
        forms["leg:share"] += 1
        forms["exchange-template:leg"] += 1
        lines.append(f"  {rng.choice(['savings', 'bonus'])} {rng.choice([5, 10, 30])}%")
    for _ in range(rng.randint(0, 2)):
        sign = rng.choice(["+ ", "- "])
        forms["item:" + {"+ ": "add", "- ": "less"}[sign]] += 1
        forms["exchange-template:item"] += 1
        lines.append(f"  {sign}{rng.randint(1, 30)} USD #{rng.choice(['fees', 'fun'])}")


def occurrences(book, c, shape):
    """Written occurrences of a promise: the days it was kept, each in a way of its own."""
    rng, forms = book.rng, Counter()
    days = due_days(c, book) if shape not in ("twice", "weekly") else []
    if shape == "loan":
        if rng.random() < 0.6:
            book.add("2026-01-01 " + c.name, "occ:loan-origin")
        days = [d for d in days if d >= f"2026-02-{c.day:02d}"]
    rng.shuffle(days)
    for due in days[: rng.randint(0, 3)]:
        weights = [3, 3, 2, 6 if c.legs else 0, 3, 3, 8 if c.inputs else 0, 2, 0 if shape == "loan" else 5]
        kind = rng.choices(["plain", "amount", "computed", "legs", "add-leg", "items", "input", "tail", "combo"], weights)[0]
        first_leg = c.legs[0][0] if c.legs else None
        written = due
        if rng.random() < 0.3:
            written = shifted(c, due, rng)
            if written != due:
                forms["occ:off-due"] += 1
        lines = [f"{written} {c.name}"]
        if shape == "buy":
            kind = rng.choice(["plain", "buy", "buy", "items", "tail"])
        if kind == "buy":
            lines[0] += f" {rng.randint(1, 4)}.{rng.randint(0, 9)} VTI"
            forms["occ:amount"] += 1
        elif kind == "amount":
            lines[0] += f" {rng.randint(300, 3_500)} USD"
            forms["occ:amount"] += 1
        elif kind == "computed":
            lines[0] += " " + computed(rng, forms)
            forms["occ:computed-amount"] += 1
        elif kind == "legs" and first_leg:
            how = rng.choice(["amount", "pending", "target", "rest", "rest", "all", "computed", "unknown"])
            qty = {"amount": f"{rng.randint(20, 500)} USD", "pending": f"({rng.randint(20, 300)} USD)",
                   "target": f"= {rng.randint(40_000, 90_000)} USD", "rest": "...", "all": "all",
                   "computed": computed(rng, forms), "unknown": "? USD"}[how]
            lines.append(f"  {first_leg} {qty}")
            forms["occ:replace-leg"] += 1
            forms["occ-leg:" + how] += 1
        elif kind in ("add-leg", "legs"):
            end = rng.choice(["bonus", "reserve", "savings"])
            if end not in [m[0] for m in c.legs]:
                lines.append(f"  {end} {rng.randint(10, 300)} USD")
                forms["occ:add-leg"] += 1
                forms["leg:amount"] += 1
        elif kind == "items":
            for _ in range(rng.randint(1, 2)):
                sign = rng.choice(["", "+ ", "- "])
                forms["item:" + {"": "carve", "+ ": "add", "- ": "less"}[sign]] += 1
                forms["occ:item"] += 1
                qty = f"{rng.randint(1, 40)} USD" if rng.random() < 0.7 else f"{rng.choice([5, 10])}% of {rng.randint(50, 400)} USD"
                forms["item:computed"] += qty.endswith("USD") and "%" in qty
                lines.append(f"  {sign}{qty} #{rng.choice(['fees', 'fun'])}" + rng.choice(["", ' "a note"', " " + book.code()]))
        elif kind == "input" and c.inputs:
            lines.append(f"  water = {rng.randint(40, 300)} USD")
            forms["occ:input"] += 1
        elif kind == "combo":
            combo(book, c, lines, forms)
        elif kind == "tail":
            clause = rng.choice(['"a note"', "#fun", book.code(), "#fees ^t" + str(rng.randint(1, 999)), "for 2026-03",
                                 "for last month", "for 2026-03-01..2026-03-31", "via acme", "for acme", "!", '! "ok"'])
            lines[0] += " " + clause
            forms["occ:tail"] += 1
            if clause.startswith("for ") and clause != "for acme":
                forms["occ:tail-recognition"] += 1
            if clause.startswith("!"):
                forms["occ:tail-waive"] += 1
        else:
            forms["occ:plain"] += 1
        if c.inputs and kind != "input" and rng.random() < 0.5:
            forms["occ:omit-input"] += 1
        book.add("\n".join(lines))
    book.forms.update(forms)


RECIPES = [(transfer, 10), (unknown, 2), (items_under_header, 8), (split, 14), (exchange, 6), (claims, 3),
           (basis, 2), (contract, 16)]


def recipes_of(kind):
    """The recipes a KIND of project draws from, with their weights."""
    if kind == "promises":
        return [(recipe, weight) for recipe, weight in RECIPES if recipe is contract]
    if kind == "statements":
        return [(recipe, weight) for recipe, weight in RECIPES if recipe is not contract]
    if kind.startswith("recipe:"):
        named = [(recipe, weight) for recipe, weight in RECIPES if recipe.__name__ == kind[len("recipe:"):]]
        if not named:
            raise SystemExit(f"no recipe {kind[len('recipe:'):]}: {', '.join(r.__name__ for r, _ in RECIPES)}")
        return named
    return RECIPES


def project(seed, index, kind="all"):
    rng = random.Random(seed * 1_000_003 + index)
    book = Book(rng)
    drawn = recipes_of(kind)
    blocks = 1 if kind.startswith("recipe:") else rng.choices([1, 2, 3, 4], [3, 4, 3, 1])[0]
    for _ in range(blocks):
        recipe = rng.choices([r for r, _ in drawn], [w for _, w in drawn])[0]
        recipe(book)
    return book


def gen(directory, count, seed, kind="all"):
    os.makedirs(directory, exist_ok=True)
    all_forms = {}
    for index in range(count):
        book = project(seed, index, kind)
        path = os.path.join(directory, f"p{index:04d}")
        os.makedirs(path, exist_ok=True)
        with open(os.path.join(path, "main.ax"), "w") as out:
            out.write(PRELUDE + "\n".join(book.lines) + "\n")
        all_forms[f"p{index:04d}"] = dict(book.forms)
    with open(os.path.join(directory, "forms.json"), "w") as out:
        json.dump(all_forms, out, indent=0, sort_keys=True)
    return all_forms


# ─── The differential run ───────────────────────────────────────────────────────────────────────────────────


def sh(binary, args, cwd):
    run = subprocess.run([binary, *args, "--today", TODAY, "--color", "never"], cwd=cwd, capture_output=True,
                         text=True, timeout=120)
    return run.returncode, run.stdout, run.stderr


def commands(path):
    """Every command run over one project: the file it is in, and the places, statements and contracts it has."""
    text = open(os.path.join(path, "main.ax")).read()
    lines = text.split("\n")
    places = re.findall(r"^account (\S+)", text, re.M) + ["shop", "acme", "buyer", "broker-co"]
    contracts = re.findall(r"^contract (\S+)", text, re.M)
    work = [["check"], ["balance"], ["flow"], ["contracts"], ["claims"], ["lots"], ["gains"],
            ["forecast", "--until", "2027-03-31", "--paths", "20"], ["contracts", "--json"], ["flow", "--json"]]
    work += [["register", place] for place in places]
    work += [["register", place, "--json"] for place in ("checking", "savings")]
    work += [["why", f"contract:{name}"] for name in contracts]
    work += [["why", "asset:condo"], ["why", "asset:laptop"]]
    first = len(PRELUDE.split("\n")) - 1
    written = [number for number in range(first + 1, len(lines) + 1) if re.match(r"\d|contract ", lines[number - 1])]
    work += [["why", f"main.ax:{number}"] for number in written]
    return [[*args, "-C", "main.ax"] for args in work]


def baseline_outputs(baseline, path, commands_):
    """What the BASELINE says to each command, kept beside the project: it never changes, so it is said once."""
    kept = os.path.join(path, "baseline.json")
    with open(os.path.join(path, "main.ax"), "rb") as source:
        stamp = [os.path.getmtime(baseline), os.path.getsize(baseline), hashlib.sha1(source.read()).hexdigest()]
    if os.path.exists(kept):
        saved = json.load(open(kept))
        if saved["stamp"] == stamp and saved["commands"] == commands_:
            return [tuple(said) for said in saved["said"]]
    said = [sh(baseline, args, path) for args in commands_]
    with open(kept, "w") as out:
        json.dump({"stamp": stamp, "commands": commands_, "said": said}, out)
    return said


def run_project(baseline, new, path):
    """The commands of one project through both binaries: what differs, and what the baseline said."""
    differences, summary = [], {"clean": True, "silent": True, "moves": False}
    work = commands(path)
    said = baseline_outputs(baseline, path, work)
    for index, (args, before) in enumerate(zip(work, said)):
        after = sh(new, args, path)
        if before != after:
            differences.append((args, before, after))
        if index == 0:
            summary["clean"] = before[0] == 0 and "error[" not in before[1] + before[2]
            summary["silent"] = summary["clean"] and before[1].startswith("✓") and before[1].count("\n") <= 1 and not before[2]
        if args[0] == "register" and "--json" not in args:
            for row in before[1].split("\n"):
                amounts = re.findall(r"(-?[0-9][0-9_,]*(?:\.[0-9]+)?) (?:USD|EURX|VTI)", row)
                if amounts and "market" not in row and "opening" not in row:
                    summary["moves"] |= any(float(a.replace(",", "")) != 0 for a in amounts[:1])
    return path, differences, summary


def run(baseline, new, directory, jobs=3):
    paths = sorted(os.path.join(directory, name) for name in os.listdir(directory) if name.startswith("p"))
    forms = json.load(open(os.path.join(directory, "forms.json")))
    with ThreadPoolExecutor(jobs) as pool:
        results = list(pool.map(lambda path: run_project(baseline, new, path), paths))
    failed = [(path, diff) for path, diff, _ in results if diff]
    if os.environ.get("SPLITS_LIST"):  # every project that differs, one path a line, for a classification
        with open(os.environ["SPLITS_LIST"], "w") as listing:
            listing.write("".join(path + "\n" for path, _ in failed))
    for path, diff in failed[:5]:
        args, before, after = diff[0]
        print(f"DIFFERENT {path}: {' '.join(args)}\n--- baseline (exit {before[0]})\n{before[1]}{before[2]}\n"
              f"--- new (exit {after[0]})\n{after[1]}{after[2]}")
    covered = Counter()
    states = Counter()
    for path, _, summary in results:
        name = os.path.basename(path)
        states["projects"] += 1
        states["clean"] += summary["clean"]
        states["silent"] += summary["silent"]
        states["moves"] += summary["moves"]
        states["clean+moves"] += summary["clean"] and summary["moves"]
        for form in forms.get(name, {}):
            covered[form, "all"] += 1
            if summary["clean"]:
                covered[form, "clean"] += 1
            if summary["clean"] and summary["moves"]:
                covered[form, "clean+moves"] += 1
    print(f"{states['projects']} projects, {sum(len(commands(p)) for p in paths)} commands, "
          f"{len(failed)} projects differ")
    print(f"clean (no error) {states['clean']}, silent (no diagnostic at all) {states['silent']}, "
          f"moved money {states['moves']}, clean and moved money {states['clean+moves']}")
    width = max((len(form) for form, _ in covered), default=0)
    print(f"{'form':<{width}}  {'all':>5} {'clean':>6} {'clean+moves':>12}")
    for form in sorted({form for form, _ in covered}):
        print(f"{form:<{width}}  {covered[form, 'all']:>5} {covered[form, 'clean']:>6} {covered[form, 'clean+moves']:>12}")
    return len(failed)


def internals(baseline, new, directory, jobs=3):
    """What two builds of internals/main.rs print for each project: the materialized flows, kept and forecast."""
    paths = sorted(os.path.join(directory, name) for name in os.listdir(directory) if name.startswith("p"))

    def say(binary, path):
        run = subprocess.run([binary, "main.ax"], cwd=path, capture_output=True, text=True, timeout=120)
        return [run.returncode, run.stdout, run.stderr]

    def one(path):
        with open(os.path.join(path, "main.ax"), "rb") as source:
            stamp = [os.path.getmtime(baseline), os.path.getsize(baseline), hashlib.sha1(source.read()).hexdigest()]
        kept = os.path.join(path, "internals.json")
        before = None
        if os.path.exists(kept):
            saved = json.load(open(kept))
            before = saved["said"] if saved["stamp"] == stamp else None
        if before is None:
            before = say(baseline, path)
            json.dump({"stamp": stamp, "said": before}, open(kept, "w"))
        after = say(new, path)
        return path, before, after

    with ThreadPoolExecutor(jobs) as pool:
        results = list(pool.map(one, paths))
    different = [(path, before, after) for path, before, after in results if before != after]
    for path, before, after in different[:3]:
        old, now = before[1].split("\n"), after[1].split("\n")
        at = next((i for i, (a, b) in enumerate(zip(old, now)) if a != b), min(len(old), len(now)))
        print(f"DIFFERENT {path} at line {at}:\n--- baseline\n{old[at][:600]}\n--- new\n{now[at][:600]}")
    promises = sum(before[1].count("\npromise ") for _, before, _ in results)
    forecast = sum(before[1].count("\nforecast ") for _, before, _ in results)
    print(f"{len(results)} projects, {promises} promises and {forecast} forecast occurrences, {len(different)} differ")
    return len(different)


def survey(binary, directory, jobs=3):
    """Which diagnostics the BINARY raises over the projects, and how clean each form's projects are."""
    paths = sorted(os.path.join(directory, name) for name in os.listdir(directory) if name.startswith("p"))
    forms = json.load(open(os.path.join(directory, "forms.json")))

    def one(path):
        code, out, err = sh(binary, ["check", "-C", "main.ax"], path)
        return path, code, re.findall(r"^(error|warning|note)\[([a-z-]+)\]", out + err, re.M)

    with ThreadPoolExecutor(jobs) as pool:
        results = list(pool.map(one, paths))
    codes, clean, per_form = Counter(), 0, Counter()
    for path, code, found in results:
        bad = code != 0 or any(kind == "error" for kind, _ in found)
        clean += not bad
        for kind, name in set(found):
            codes[kind, name] += 1
        for form in forms[os.path.basename(path)]:
            per_form[form, "all"] += 1
            per_form[form, "clean"] += not bad
    print(f"{clean} of {len(results)} projects have no error")
    for (kind, name), count in codes.most_common():
        print(f"  {kind:<8} {name:<32} {count}")
    for form in sorted({form for form, _ in per_form}):
        print(f"  {form:<26} {per_form[form, 'clean']:>4} of {per_form[form, 'all']:>4} clean")


# ─── A split against what it says ───────────────────────────────────────────────────────────────────────────────


def equivalent(rng):
    """One split statement written out, and the plain transfers LANGUAGE §3 says it is: (split, plain, forms).

    The legs add up to the total (or one is `...`), so a build that reads §3 moves exactly what the plain transfers
    move. A leg may be a computed amount (`25% of 400 USD`) or a target (`= 5_040 USD` of an account that holds
    5,000), which the fold solves when the split lands; an item may be a share of the total (`- 6%`) or a share of
    `amount`. The plain book is run by the BASELINE, which gets plain transfers right; the split book by the NEW build.
    """
    forms = Counter()
    day = "2026-03-%02d" % rng.randint(2, 27)
    if rng.random() < 0.15:
        return exchanged(rng, day, forms)
    named = rng.random() < 0.3
    from_side = rng.random() < 0.5
    source = "checking"
    ends = rng.sample(["shop", "acme", "savings", "bonus", "reserve", "buyer"], rng.randint(1, 3))
    total = rng.choice(range(800, 1201, 20))
    shared = rng.choice(["before", "after", "none"]) if not named else "before"
    items = []
    for _ in range(rng.choice([0, 0, 1, 2])):
        kind = rng.choice(["carve", "add", "less", "less-bare", "share-carve", "share-add", "share-less", "pct-carve", "pct-less"])
        if kind.startswith(("share", "pct")) and shared == "none":
            kind = "carve"
        rate = rng.choice([5, 10])
        value = total * rate // 100 if kind.startswith(("share", "pct")) else rng.randint(1, 20)
        items.append((kind, value, rate, rng.choice(["fees", "fun", "groceries"])))
    taking = ("carve", "less-bare", "share-carve", "pct-carve")
    carved = sum(value for kind, value, _, _ in items if kind in taking)
    if named:
        forms["named"] += 1
        end = rng.choice(["shop", "acme", "savings"])
        purpose = rng.choice(["", " #household", " #fun"])
        split = [f"{day} {source} -> {end} {total} USD{purpose}"]
        plain = [f"{day} {source} -> {end} {total - carved} USD{purpose}"]
    else:
        forms["split:from" if from_side else "split:to"] += 1
        rest_at = rng.choice([None, "last", "first"]) if len(ends) > 1 or rng.random() < 0.5 else None
        kinds = []
        for index in range(len(ends)):
            if (rest_at == "last" and index == len(ends) - 1) or (rest_at == "first" and index == 0):
                kinds.append("rest")
            else:
                targets = from_side and ends[index] in ("savings", "bonus", "reserve")
                kinds.append(rng.choices(["amount", "computed", "target"], [6, 2, 2 if targets else 0])[0])
        amounts = [rng.randint(5, 120) for _ in ends]
        for index, kind in enumerate(kinds):
            if kind == "computed":
                amounts[index] = rng.choice([5, 10, 25, 50]) * rng.choice([20, 40, 100, 200]) // 100
        if rest_at is None:
            # no remainder: the last leg that is not computed takes what the others leave of the total
            index = max(i for i, kind in enumerate(kinds) if kind == "amount") if "amount" in kinds else None
            others = sum(a for i, a in enumerate(amounts) if i != index)
            if index is None or total - carved - others < 5:
                kinds = ["amount"] * len(ends)
                amounts = [5 for _ in ends]
                index = len(ends) - 1
                amounts[index] = total - carved - 5 * (len(ends) - 1)
            else:
                amounts[index] = total - carved - others
        if rest_at is not None and shared == "none":
            shared = "before"
        forms["total:" + shared] += 1
        forms["rest:" + str(rest_at)] += 1
        head = {
            (True, "before"): f"{day} {source} {total} USD ->",
            (True, "after"): f"{day} {source} -> {total} USD",
            (True, "none"): f"{day} {source} ->",
            (False, "before"): f"{day} -> {source} {total} USD",
            (False, "after"): f"{day} {total} USD -> {source}",
            (False, "none"): f"{day} -> {source}",
        }[from_side, shared]
        split = [head]
        plain = []
        remainder_end = None
        for end, kind, amount in zip(ends, kinds, amounts):
            purpose = rng.choice(["", "", " #fun", " #groceries"])
            forms["leg:" + kind] += 1
            if kind == "rest":
                text, moved = "...", None
                remainder_end = end
            elif kind == "computed":
                rate = amount * 100 // rng.choice([20, 40, 100, 200]) if False else None
                text = f"{amount * 100 // 100} USD"
                base = rng.choice([20, 40, 100, 200])
                for percent in (5, 10, 25, 50):
                    if amount * 100 % base == 0 and amount * 100 // base == percent:
                        text = f"{percent}% of {base} USD"
                        break
                moved = amount
            elif kind == "target":
                text, moved = f"= {5_000 + amount} USD", amount
            else:
                text, moved = f"{amount} USD", amount
            split.append(f"  {end} {text}{purpose}")
            if moved is None:
                moved = total - sum(m for m in amounts if m) + 0
                moved = total - carved - sum(a for a, k in zip(amounts, kinds) if k != "rest")
            plain.append(f"{day} {source} -> {end} {moved} USD{purpose}" if from_side else f"{day} {end} -> {source} {moved} USD{purpose}")
        carry = remainder_end or ends[0]
        forms["leg-count:%d" % len(ends)] += 1
    for kind, value, rate, purpose in items:
        forms["item:" + kind] += 1
        sign = {"carve": "", "add": "+ ", "less": "- ", "less-bare": "- ", "share-carve": "", "share-add": "+ ", "share-less": "- ",
                "pct-carve": "", "pct-less": "- "}[kind]
        written = {"share": f"{rate}%", "pct": f"{rate}% of amount"}.get(kind.split("-")[0], f"{value} USD")
        tail = "" if kind == "less-bare" else f" #{purpose}"
        split.append(f"  {sign}{written}{tail}")
        if kind == "less-bare":
            continue
        if named:
            a, b = source, end
        else:
            a, b = (source, carry) if from_side else (carry, source)
        if kind.endswith("less"):
            a, b = b, a
        plain.append(f"{day} {a} -> {b} {value} USD #{purpose}")
    return "\n".join(split) + "\n", "\n".join(plain) + "\n", forms


def exchanged(rng, day, forms):
    """A split with a leg in another commodity: the exchange of what the other legs leave (README of example 08).

    `checking 900 USD ->` with a fee leg (`shop 5 USD`) and `savings 800 EURX` is a fee of 5 USD and an exchange of
    the 895 USD that is left for the 800 EURX; written the other way (`-> savings 900 USD`) the euros are what leaves
    `checking` and the dollars the remainder that arrives.
    """
    from_side = rng.random() < 0.6
    total = rng.choice(range(800, 1201, 20))
    fees = [rng.randint(1, 30) for _ in range(rng.choice([1, 1, 2]))]
    left = total - sum(fees)
    euros = min(left * rng.choice([85, 90, 92]) // 100, 990)  # checking holds 1,000 EURX
    ends = rng.sample(["shop", "acme", "buyer"], len(fees))
    forms["exchange:" + ("from" if from_side else "to")] += 1
    forms["exchange:fees:%d" % len(fees)] += 1
    legs = [f"  {end} {fee} USD" for end, fee in zip(ends, fees)]
    leg = f"  savings {euros} EURX"
    place = rng.choice(["last", "first"])
    forms["exchange:leg-" + place] += 1
    legs = legs + [leg] if place == "last" else [leg] + legs
    if from_side:
        split = [f"{day} checking {total} USD ->"] + legs
        plain = [f"{day} checking -> {end} {fee} USD" for end, fee in zip(ends, fees)]
        plain.append(f"{day} checking {left} USD -> savings {euros} EURX")
    else:
        # the euros are `checking`'s, the only account that holds any
        legs = [leg.replace("savings", "checking") for leg in legs]
        split = [f"{day} -> savings {total} USD"] + legs
        plain = [f"{day} {end} -> savings {fee} USD" for end, fee in zip(ends, fees)]
        plain.append(f"{day} checking {euros} EURX -> savings {left} USD")
    return "\n".join(split) + "\n", "\n".join(plain) + "\n", forms


def equiv(baseline, new, directory, count, seed, jobs=3):
    """The NEW build on each split against the BASELINE on the plain transfers it is: balance, flow, net worth."""
    os.makedirs(directory, exist_ok=True)
    cases = []
    for index in range(count):
        rng = random.Random(seed * 7_000_003 + index)
        split, plain, forms = equivalent(rng)
        for name, text in (("split", split), ("plain", plain)):
            path = os.path.join(directory, f"e{index:05d}", name)
            os.makedirs(path, exist_ok=True)
            with open(os.path.join(path, "main.ax"), "w") as out:
                out.write(PRELUDE + text)
        cases.append((index, forms, split, plain))

    def one(case):
        index, forms, split, plain = case
        said = {}
        for name, binary in (("split", new), ("plain", baseline)):
            path = os.path.join(directory, f"e{index:05d}", name)
            said[name] = [sh(binary, args, path) for args in (["balance", "-C", "main.ax"], ["flow", "-C", "main.ax"], ["check", "-C", "main.ax"])]
        return index, forms, split, plain, said

    with ThreadPoolExecutor(jobs) as pool:
        results = list(pool.map(one, cases))
    wrong, counts, clean = [], Counter(), 0
    for index, forms, split, plain, said in results:
        counts.update(forms)
        net = lambda out: re.findall(r"net worth ([-0-9,.]+ \w+)", out)
        same = said["split"][0] == said["plain"][0] and said["split"][1] == said["plain"][1] and net(said["split"][2][1]) == net(said["plain"][2][1])
        clean += said["split"][2][0] == 0
        if not same:
            wrong.append((index, split, plain, said))
    for index, split, plain, said in wrong[:4]:
        print(f"DIFFERENT e{index:05d}\n--- the split\n{split}--- the plain transfers\n{plain}")
        for what, a, b in zip(("balance", "flow", "check"), said["split"], said["plain"]):
            if a != b:
                print(f"--- {what}: split said\n{a[1][-900:]}{a[2][-300:]}\n--- {what}: plain said\n{b[1][-900:]}{b[2][-300:]}")
    print(f"{len(results)} splits, {clean} without an error, {len(wrong)} not the plain transfers they say")
    for form, number in sorted(counts.items()):
        print(f"  {form:<24} {number}")
    return len(wrong)


def main(argv):
    if len(argv) >= 5 and argv[1] == "internals":
        return 1 if internals(argv[2], argv[3], argv[4], int(argv[5]) if len(argv) > 5 else 3) else 0
    if len(argv) >= 6 and argv[1] == "equiv":
        return 1 if equiv(argv[2], argv[3], argv[4], int(argv[5]), int(argv[6]) if len(argv) > 6 else 1) else 0
    if len(argv) >= 4 and argv[1] == "survey":
        survey(argv[2], argv[3])
        return 0
    if len(argv) >= 4 and argv[1] == "gen":
        forms = gen(argv[2], int(argv[3]), int(argv[4]) if len(argv) > 4 else 1, argv[5] if len(argv) > 5 else "all")
        total = Counter()
        for one in forms.values():
            total.update(one.keys())
        print(f"wrote {len(forms)} projects to {argv[2]}; projects holding each form:")
        for form, count in sorted(total.items()):
            print(f"  {form:<24} {count}")
        return 0
    if len(argv) >= 5 and argv[1] == "run":
        return 1 if run(argv[2], argv[3], argv[4], int(argv[5]) if len(argv) > 5 else 3) else 0
    if len(argv) >= 6 and argv[1] == "all":
        gen(argv[4], int(argv[5]), int(argv[6]) if len(argv) > 6 else 1, argv[7] if len(argv) > 7 else "all")
        return 1 if run(argv[2], argv[3], argv[4]) else 0
    print(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
