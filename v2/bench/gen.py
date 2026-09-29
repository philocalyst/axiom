#!/usr/bin/env python3
"""Deterministic generator of synthetic but realistic Axiom projects.

    gen.py --flows 10k|100k|1m|5m --out DIR [--seed N] [--variant full|nolaws]

Writes a directory project:

    axiom.ax             base currency, systems, the people (entities that live somewhere)
    accounts.ax          commodities, hundreds of places in a deep hierarchy, payees, grants
    plans.ax             recurring plans for the forecast, after the history ends
    journal/YYYY/MM.ax   one file per month, chronological, every person interleaved
    prices/YYYY.ax      daily prices for every commodity but the base

What is in the journal, per person and month: two salary splits (401(k) deferral
and three or four withholding legs, the rest to checking), rent and utilities,
card spending paid off at month end, hundreds of small purchases on the card or
debit, transfers to savings and to a 529 plan, brokerage buys (a new lot every
time, in six commodities) and sales (which relieve lots under the account's
policy), quarterly dividends, interest, growth in the 401(k) and 529, EUR trips,
pending checks that are settled or voided days later, a restricted scholarship
that is spent on tuition, 529 withdrawals for tuition, an occasional `?` amount
that is inferred from month-end assertions, and a month-end assertion for every
place that matters. All assertions hold: the generator keeps exact integer
balances (cents), so `axiom check` on a generated project is clean.

The variant `nolaws` writes the same journal with no kinds, systems, budgets,
residence or grants' restrictions, so no law is ever evaluated. Comparing the
two isolates the cost of the law engine.

Deterministic: the only source of randomness is `random.Random(seed)`; the same
seed, scale and Python major version give byte-identical output.
"""

import argparse
import datetime
import math
import os
import random
import sys
import time

SCALES = {  # flows, people, years
    "10k": (10_000, 1, 5),
    "100k": (100_000, 4, 8),
    "1m": (1_000_000, 12, 12),
    "5m": (5_000_000, 30, 20),
}
START_YEAR = 2024  # the standard systems' parameter tables start here

# ---------------------------------------------------------------------------
# The expense tree: 10 categories x 4 subcategories x 4 leaves = 160 leaves per person.

CATEGORIES = [
    # name, weight of variable spending, [(sub, [leaves])]
    ("housing", 0.06, [
        ("rent", ["landlord", "parking", "storage", "renters-insurance"]),
        ("utilities", ["electric", "gas", "water", "internet"]),
        ("maintenance", ["plumber", "handyman", "appliances", "cleaning"]),
        ("furnishing", ["furniture", "decor", "bedding", "kitchenware"]),
    ]),
    ("food", 0.30, [
        ("groceries", ["whole-foods", "trader-joes", "costco", "farmers-market"]),
        ("dining", ["restaurants", "fast-food", "delivery", "bars"]),
        ("coffee", ["starbucks", "local-cafe", "bakery", "tea-house"]),
        ("snacks", ["convenience", "kiosk", "vending", "ice-cream"]),
    ]),
    ("transport", 0.14, [
        ("transit", ["metro", "bus", "bikeshare", "ferry"]),
        ("car", ["fuel", "tolls", "car-wash", "repairs"]),
        ("rideshare", ["uber", "lyft", "taxi", "scooter"]),
        ("rail", ["commuter", "intercity", "tickets", "passes"]),
    ]),
    ("health", 0.06, [
        ("doctor", ["primary-care", "specialist", "lab", "urgent-care"]),
        ("pharmacy", ["prescriptions", "otc", "vitamins", "supplies"]),
        ("dental", ["cleaning", "fillings", "ortho", "x-ray"]),
        ("fitness", ["gym", "classes", "gear", "trainer"]),
    ]),
    ("fun", 0.12, [
        ("streaming", ["video", "music", "games", "books"]),
        ("events", ["concerts", "sports", "theater", "festivals"]),
        ("hobbies", ["crafts", "photography", "cycling", "climbing"]),
        ("outings", ["museums", "parks", "cinema", "arcade"]),
    ]),
    ("shopping", 0.14, [
        ("clothing", ["basics", "shoes", "outerwear", "accessories"]),
        ("electronics", ["phones", "computers", "cables", "audio"]),
        ("household", ["cleaning-supplies", "tools", "garden", "office"]),
        ("online", ["marketplace", "subscriptions", "software", "misc"]),
    ]),
    ("gifts", 0.03, [
        ("family", ["parents", "siblings", "cousins", "grandparents"]),
        ("friends", ["birthdays", "weddings", "housewarming", "baby"]),
        ("charity", ["local", "global", "religious", "political"]),
        ("cards", ["greeting", "postage", "wrapping", "flowers"]),
    ]),
    ("travel", 0.05, [
        ("domestic", ["flights", "hotels", "rentals", "meals"]),
        ("abroad", ["hotels", "meals", "tours", "transit"]),
        ("gear", ["luggage", "adapters", "guides", "insurance"]),
        ("fees", ["visas", "baggage", "resort", "exchange"]),
    ]),
    ("pets", 0.04, [
        ("food", ["kibble", "treats", "litter", "bird-seed"]),
        ("vet", ["checkup", "vaccines", "emergency", "dental"]),
        ("gear", ["toys", "beds", "leashes", "cages"]),
        ("care", ["grooming", "boarding", "walker", "training"]),
    ]),
    ("edu", 0.02, [
        ("tuition", ["university", "community-college", "online-courses", "bootcamp"]),
        ("books", ["textbooks", "ebooks", "journals", "stationery"]),
        ("supplies", ["lab", "art", "printing", "software"]),
        ("fees", ["registration", "exams", "library", "parking"]),
    ]),
]
BUDGET_FACTOR = 3.0  # budgets are this multiple of a category's mean month: rarely breached

COMMODITIES = [
    # symbol, kind, precision, start price (dollars), yearly drift, daily vol, name
    ("VTI", "fund", 0, 220.0, 0.08, 0.010, "Total stock market ETF"),
    ("VXUS", "fund", 0, 58.0, 0.05, 0.011, "Total international stock ETF"),
    ("BND", "bond", 0, 72.0, 0.02, 0.003, "Total bond market ETF"),
    ("AAPL", "stock", 0, 185.0, 0.12, 0.016, "Apple Inc."),
    ("BTC", "crypto", 8, 42_000.0, 0.25, 0.035, "Bitcoin"),
    ("GLD", "good", 0, 190.0, 0.04, 0.008, "Gold shares"),
]
EUR_START, EUR_DRIFT, EUR_VOL = 1.09, 0.0, 0.003
RESIDENCES = ["us/ca/san-francisco", "us/ny/nyc", "us", "us/ca"]
SELECT_POLICIES = ["fifo", "hifo", "lifo", "prorata"]


def parse_flows(text):
    text = text.strip().lower()
    if text in SCALES:
        return SCALES[text][0]
    mult = {"k": 1_000, "m": 1_000_000}.get(text[-1], 1)
    return int(float(text[:-1] if mult != 1 else text) * mult)


def scale_for(flows):
    for name, (f, p, y) in SCALES.items():
        if f == flows:
            return p, y
    lo = math.log10(SCALES["10k"][0])
    hi = math.log10(SCALES["5m"][0])
    t = min(1.0, max(0.0, (math.log10(flows) - lo) / (hi - lo)))
    return max(1, round(1 + 29 * t ** 1.3)), max(2, round(5 + 15 * t))


def fmt(c):
    """Integer cents as `1234.56`."""
    return f"{c // 100}.{c % 100:02d}"


def qty(units, precision):
    if precision == 0:
        return str(units)
    return f"{units // 100}.{units % 100:02d}"  # BTC is traded in 0.01 steps


def last_of(y, m):
    return (datetime.date(y + (m == 12), m % 12 + 1, 1) - datetime.timedelta(days=1)).day


class Person:
    def __init__(self, idx, rng, variant, months):
        self.idx = idx
        self.k = f"p{idx + 1}"
        self.full = variant == "full"
        self.res = RESIDENCES[idx % len(RESIDENCES)]
        self.filing = "single" if idx % 2 == 0 else "joint"
        self.born = f"{1975 + (idx * 7) % 20}-{1 + (idx * 5) % 12:02d}-{1 + (idx * 11) % 27:02d}"
        self.salary_m = rng.randrange(9_500, 15_500) * 100  # gross per month, cents
        self.deferral = rng.randrange(6, 9) * 100 * 100  # 600..800 USD per paycheck
        self.rent = rng.randrange(1_400, 2_300) * 100
        self.policy = SELECT_POLICIES[idx % 4]
        self.grants = idx < 3
        self.state_kind = {"us/ca/san-francisco": "ca-tax", "us/ca": "ca-tax", "us/ny/nyc": "ny-tax"}.get(self.res)
        self.has_city = self.res == "us/ny/nyc"
        # places (paths under their root) and the names used in the journal
        k = self.k
        self.p = dict(
            chk=f"assets/{k}/bank/checking", sav=f"assets/{k}/bank/savings", cash=f"assets/{k}/bank/cash",
            brok=f"assets/{k}/invest/brokerage", k401=f"assets/{k}/retire/k401", p529=f"assets/{k}/edu/plan529",
            eur=f"assets/{k}/travel/eur-wallet", visa=f"liabilities/{k}/cards/visa",
            salary=f"income/{k}/salary", interest=f"income/{k}/interest", dividends=f"income/{k}/dividends",
            growth=f"income/{k}/growth", grants=f"income/{k}/grants", other=f"income/{k}/other",
            opening=f"equity/{k}/opening", fed=f"expenses/{k}/taxes/federal", state=f"expenses/{k}/taxes/state",
            city=f"expenses/{k}/taxes/city", payroll=f"expenses/{k}/taxes/payroll",
        )
        self.n = {name: path.split("/", 1)[1] for name, path in self.p.items()}  # names as written in the journal
        self.acme = f"acme-{k}"
        self.contractor = f"contractor-{k}"
        self.university = f"university-{k}"
        self.landlord = f"landlord-{k}"
        # the expense leaves, with payees for some
        self.leaves = []  # (path, name, payee or None)
        weights = []
        self.payees = []
        for cat, w, subs in CATEGORIES:
            for sub, leaves in subs:
                for leaf in leaves:
                    path = f"expenses/{k}/{cat}/{sub}/{leaf}"
                    payee = f"{leaf}-{k}" if (len(self.leaves) % 6 == 0 and (cat, sub, leaf) not in
                                              {("housing", "rent", "landlord"), ("edu", "tuition", "university"),
                                               ("housing", "maintenance", "plumber")}) else None
                    self.leaves.append((path, path.split("/", 1)[1], payee, cat))
                    weights.append(w / 16)
                    if payee:
                        self.payees.append((payee, path))
        acc = 0.0
        self.cum = []
        for w in weights:
            acc += w
            self.cum.append(acc)
        self.gifts_ix = [i for i, l in enumerate(self.leaves) if l[3] == "gifts"]
        # a payee is written either as the destination itself (`-> whole-foods-p1`) or after a slash
        self.dest, self.tail = [], []
        for i, l in enumerate(self.leaves):
            if l[2] and (i // 6) % 2 == 1:
                self.dest.append(l[2])
                self.tail.append("")
            else:
                self.dest.append(l[1])
                self.tail.append(f" / {l[2]}" if l[2] else "")
        # balances, cents
        self.chk = self.sav = self.visa = self.k401 = self.p529 = 0
        self.held = {c[0]: 0 for c in COMMODITIES}
        self.carry = []  # (day, 'settled'|'void', code, amount) for the next month
        self.checks = 0
        self.raise_pct = 1.0

    # -- declarations ------------------------------------------------------
    def declare_entities(self, out, first_year, last_year):
        k = self.k
        if self.full:
            out.append(f"/// Person {k}: lives under {self.res}.")
            out.append(f"entity {k} : person\n  born   {self.born}\n  filing {self.filing}\n  lives  {self.res}\n")
        else:
            out.append(f"entity {k}\n")

    def declare_accounts(self, out):
        k, p, full = self.k, self.p, self.full
        kind = (lambda kd: f" : {kd}") if full else (lambda kd: "")
        own = f"  owner {k}\n"
        extra = lambda s: s if full else ""
        lines = []
        lines.append(f"// ─── {k}: assets, liabilities, income, equity ───")
        lines.append(f"account {p['chk']}{kind('bank')}\n{own}".rstrip("\n"))
        lines.append(f"account {p['sav']}{kind('bank')}\n{own}".rstrip("\n"))
        lines.append(f"account {p['cash']}{kind('cash')}\n{own}".rstrip("\n"))
        lines.append(f"account {p['brok']}{kind('broker')}\n{own}  select {self.policy}")
        if full:
            lines.append(f"account {p['k401']} : 401k\n{own}  employer {self.acme}")
            lines.append(f"account {p['p529']} : 529-plan\n{own}  beneficiary {k}")
        else:
            lines.append(f"account {p['k401']}\n{own}".rstrip("\n"))
            lines.append(f"account {p['p529']}\n{own}".rstrip("\n"))
        lines.append(f"account {p['eur']}\n{own}  select fifo")
        lines.append(f"account {p['visa']}{kind('credit-card')}\n{own}".rstrip("\n"))
        lines.append(f"account {p['salary']}{kind('wages')}\n{own}".rstrip("\n"))
        lines.append(f"account {p['interest']}{kind('interest')}\n{own}".rstrip("\n"))
        lines.append(f"account {p['dividends']}{kind('dividends')}\n{own}".rstrip("\n"))
        lines.append(f"account {p['growth']}{kind('gains')}\n{own}".rstrip("\n"))
        lines.append(f"account {p['grants']}\n{own}".rstrip("\n"))
        lines.append(f"account {p['other']}\n{own}".rstrip("\n"))
        lines.append(f"account {p['opening']}\n{own}".rstrip("\n"))
        lines.append(f"account {p['fed']}{kind('federal-tax')}\n{own}".rstrip("\n"))
        lines.append(f"account {p['state']}{kind(self.state_kind) if self.state_kind else ''}\n{own}".rstrip("\n"))
        if self.has_city:
            lines.append(f"account {p['city']}{kind('nyc-tax')}\n{own}".rstrip("\n"))
        lines.append(f"account {p['payroll']}\n{own}".rstrip("\n"))
        out.extend(lines)
        out.append(f"\n// ─── {k}: expenses ───")
        if full:
            seen = set()
            for cat, w, subs in CATEGORIES:
                top = f"expenses/{k}/{cat}"
                mean = 2_800 * w
                budget = {"housing": 9_000, "travel": 5_000, "edu": 12_000, "gifts": 250}.get(cat)
                if budget is None:
                    budget = max(200, int(math.ceil(mean * BUDGET_FACTOR / 100.0)) * 100)
                out.append(f"account {top}\n  budget {budget} USD monthly")
        for path, name, payee, cat in self.leaves:
            is_tuition = "/edu/tuition/" in path
            out.append(f"account {path}" + (" : education" if (full and is_tuition) else ""))
        out.append("")
        out.append(f"// ─── {k}: payees ───")
        for payee, path in self.payees:
            out.append(f"entity {payee}{' : org' if full else ''}\n  via {path}")
        out.append(f"entity {self.acme}{' : employer' if full else ''}\n  via {p['salary']}")
        out.append(f"entity {self.contractor}{' : org' if full else ''}\n  via expenses/{k}/housing/maintenance/plumber")
        out.append(f"entity {self.university}{' : org' if full else ''}\n  via expenses/{k}/edu/tuition/university")
        out.append(f"entity {self.landlord}{' : org' if full else ''}\n  via expenses/{k}/housing/rent/landlord")
        out.append("")

    # -- one month ------------------------------------------------------------
    def month(self, y, m, mi, ctx):
        rng = ctx.rng
        last = last_of(y, m)
        ds = ctx.dstr[(y, m)]
        n = self.n
        chk_n, sav_n, visa_n = n["chk"], n["sav"], n["visa"]
        ev = [[] for _ in range(last + 1)]
        flows = 0

        def add(d, delta, text, nflows=1):
            nonlocal flows
            ev[d].append((delta, text))
            flows += nflows

        # carried pending checks
        for d, kind, code, amt in self.carry:
            add(d, -amt if kind == "settled" else 0, f"{ds[d]} #{code} {kind}", 0)
        self.carry = []

        if mi == 0:  # opening balances
            for key, c in (("chk", 30_000_00), ("sav", 20_000_00), ("k401", 60_000_00), ("p529", 10_000_00)):
                add(1, c if key == "chk" else 0, f"{ds[1]} {n['opening']} -> {n[key]} {fmt(c)} USD")
            self.sav, self.k401, self.p529 = 20_000_00, 60_000_00, 10_000_00

        # salary, twice a month; a raise every January
        if m == 1 and mi > 0:
            self.salary_m = self.salary_m * 103 // 100
        gross = self.salary_m // 2
        fed = gross * 12 // 100
        st = gross * (45 if self.res == "us/ny/nyc" else 40) // 1000 if self.state_kind else gross * 30 // 1000
        city = gross * 10 // 1000 if self.has_city else 0
        pay = gross * 765 // 10_000
        take = gross - self.deferral - fed - st - city - pay
        for d in ((15,) if mi == 0 else (1, 15)):
            legs = [f"{ds[d]} {self.acme} -> {fmt(gross)} USD",
                    f"  {n['k401']} {fmt(self.deferral)} USD",
                    f"  {n['fed']} {fmt(fed)} USD",
                    f"  {n['state']} {fmt(st)} USD"]
            if city:
                legs.append(f"  {n['city']} {fmt(city)} USD")
            legs.append(f"  {n['payroll']} {fmt(pay)} USD")
            legs.append(f"  {chk_n} ...")
            add(d, take, "\n".join(legs), len(legs) - 1)
            self.k401 += self.deferral

        # rent and utilities
        add(1, -self.rent, f"{ds[1]} {chk_n} -> {self.landlord} {fmt(self.rent)} USD")
        for d, leaf, base in ((3, "housing/utilities/electric", 90), (5, "housing/utilities/internet", 70)):
            a = (base + rng.randrange(0, 40)) * 100 + rng.randrange(0, 100)
            add(d, -a, f"{ds[d]} {chk_n} -> {self.k}/{leaf} {fmt(a)} USD")

        # savings and 529
        add(20, -400_00, f"{ds[20]} {chk_n} -> {sav_n} 400.00 USD")
        self.sav += 400_00
        add(21, -150_00, f"{ds[21]} {chk_n} -> {n['p529']} 150.00 USD")
        self.p529 += 150_00

        # brokerage buys: a new lot every time
        held_start = dict(self.held)
        comms = COMMODITIES[:ctx.ncomm]
        nb = ctx.buys
        for i in range(nb):
            d = 10 + 7 * i if nb == 3 else 1 + (i * 24) // nb
            sym, _, prec, *_ = comms[rng.randrange(len(comms))]
            price = ctx.price(sym, y, m, d)  # cents per unit (BTC: cents per 0.01 BTC == dollars per BTC)
            spend = rng.randrange(150, 700) * 100 * 3 // max(3, nb)
            units = max(1, spend // price)
            cost = units * price
            add(d, -cost, f"{ds[d]} {chk_n} -> {n['brok']} {qty(units, prec)} {sym} @ {fmt(ctx.quote(sym, price))} USD")
            self.held[sym] += units
        # sales under the account's policy (only what was held at the start of the month)
        nsell = int(ctx.sells) + (1 if rng.random() < ctx.sells - int(ctx.sells) else 0)
        sold = {}
        for i in range(nsell):
            owned = [c for c in comms if held_start[c[0]] - sold.get(c[0], 0) > 0]
            if not owned:
                break
            sym, _, prec, *_ = owned[rng.randrange(len(owned))]
            avail = held_start[sym] - sold.get(sym, 0)
            units = max(1, min(avail, avail * rng.randrange(15, 60) // 100 // nsell))
            d = 26 if nsell == 1 else 1 + (i * 27) // nsell
            price = ctx.price(sym, y, m, d)
            proceeds = units * price
            add(d, proceeds, f"{ds[d]} {n['brok']} {qty(units, prec)} {sym} -> {chk_n} {fmt(proceeds)} USD")
            self.held[sym] -= units
            sold[sym] = sold.get(sym, 0) + units
        if m % 3 == 0:  # dividends land in the brokerage as cash
            dv = rng.randrange(2_000, 30_000)
            add(27, 0, f"{ds[27]} {n['dividends']} -> {n['brok']} {fmt(dv)} USD")

        # interest, growth
        intr = self.sav * 3 // 12_000
        add(28, 0, f"{ds[28]} {n['interest']} -> {sav_n} {fmt(intr)} USD")
        self.sav += intr
        g401 = self.k401 * rng.randrange(20, 90) // 10_000
        add(28, 0, f"{ds[28]} {n['growth']} -> {n['k401']} {fmt(g401)} USD")
        self.k401 += g401
        g529 = self.p529 * rng.randrange(20, 90) // 10_000
        add(28, 0, f"{ds[28]} {n['growth']} -> {n['p529']} {fmt(g529)} USD")
        self.p529 += g529

        # a trip abroad in June and December: exchange USD for EUR, then spend EUR
        if m in (6, 12):
            eur = rng.randrange(6, 15) * 100 * 100
            rate = ctx.eur_rate(y, m, 3)  # cents of USD per EUR
            usd = eur * rate // 100
            add(3, -usd, f"{ds[3]} {chk_n} {fmt(usd)} USD -> {n['eur']} {fmt(eur)} EUR")
            left = eur
            for j, d in enumerate((4, 5, 6, 7, 8, 9)):
                a = min(left, eur // 7)
                if a <= 0:
                    break
                leaf = ("abroad/hotels", "abroad/meals", "abroad/tours", "abroad/transit", "abroad/meals", "abroad/tours")[j]
                add(d, 0, f"{ds[d]} {n['eur']} -> {self.k}/travel/{leaf} {fmt(a)} EUR")
                left -= a

        # scholarship, spent on tuition; a 529 withdrawal that pays tuition directly
        if self.grants and m == 1:
            add(6, 4_000_00, f"{ds[6]} scholarship-{self.k}-{y} -> {chk_n} 4000.00 USD")
            add(8, -2_000_00, f"{ds[8]} {chk_n} -> {self.university} 2000.00 USD")
            add(9, -2_000_00, f"{ds[9]} {chk_n} -> {self.university} 2000.00 USD")
        if m == 8 and self.p529 > 5_000_00:
            w = self.p529 // 10 // 100 * 100
            add(15, 0, f"{ds[15]} {n['p529']} -> {self.university} {fmt(w)} USD")
            self.p529 -= w

        # pending checks, settled or voided days later
        for _ in range(rng.choice((0, 1, 1, 2, 3))):
            self.checks += 1
            code = f"chk-{self.k}-{self.checks:06d}"
            d = rng.randrange(2, 25)
            amt = rng.randrange(80, 900) * 100
            add(d, 0, f"{ds[d]} {chk_n} -> {self.contractor} ({fmt(amt)} USD) #{code}")
            if rng.random() < 0.88:
                sd, kind = d + rng.randrange(3, 10), "settled"
            else:
                sd, kind = d + rng.randrange(5, 13), "void"
            if sd <= 28:
                add(sd, -amt if kind == "settled" else 0, f"{ds[sd]} #{code} {kind}", 0)
            else:
                self.carry.append((sd - 28, kind, code, amt))

        # one unknown amount between two month-end assertions
        if mi > 0 and rng.random() < 0.05 and ctx.infer:
            a = rng.randrange(40, 300) * 100
            add(12, -a, f"{ds[12]} {chk_n} -> {n['cash']} ? USD")

        # the everyday spending
        nspend = ctx.spend_per_month
        picks = rng.choices(range(len(self.leaves)), cum_weights=self.cum, k=nspend)
        mu = math.log(ctx.mean_amount) - 0.32
        amts = [rng.lognormvariate(mu, 0.8) for _ in range(nspend)]
        days = rng.choices(range(1, 29), k=nspend)
        oncard = [rng.random() < 0.75 for _ in range(nspend)]
        dest, tail = self.dest, self.tail
        for i in range(nspend):
            a = max(50, int(amts[i] * 100))
            d = days[i]
            if oncard[i]:
                self.visa += a
                ev[d].append((0, f"{ds[d]} {visa_n} -> {dest[picks[i]]} {a // 100}.{a % 100:02d} USD{tail[picks[i]]}"))
            else:
                ev[d].append((-a, f"{ds[d]} {chk_n} -> {dest[picks[i]]} {a // 100}.{a % 100:02d} USD{tail[picks[i]]}"))
        flows += nspend
        if m == 12 and self.idx == 0:  # holiday gifts: the one budget that is broken
            for j in range(3):
                d = 10 + 4 * j
                a = rng.randrange(60, 140) * 100
                self.visa += a
                leaf = self.leaves[self.gifts_ix[j]]
                ev[d].append((0, f"{ds[d]} {visa_n} -> {leaf[1]} {fmt(a)} USD"))
                flows += 1
        if self.visa:
            ev[28].append((-self.visa, f"{ds[28]} {chk_n} -> {visa_n} {fmt(self.visa)} USD"))
            flows += 1
            self.visa = 0

        # walk the month in order, keeping checking above zero
        out = []
        run = self.chk
        for d in range(1, last + 1):
            for delta, text in ev[d]:
                if run + delta < 1_500_00:
                    need = 1_500_00 - (run + delta) + rng.randrange(0, 500) * 100
                    if self.sav - need > 1_000_00:
                        out.append((d, f"{ds[d]} {sav_n} -> {chk_n} {fmt(need)} USD"))
                        self.sav -= need
                    else:
                        out.append((d, f"{ds[d]} {n['other']} -> {chk_n} {fmt(need)} USD"))
                    run += need
                    flows += 1
                run += delta
                out.append((d, text))
        self.chk = run
        tail_lines = [
            f"{ds[last]} {chk_n} = {fmt(self.chk)} USD",
            f"{ds[last]} {sav_n} = {fmt(self.sav)} USD",
            f"{ds[last]} {visa_n} = empty",
            f"{ds[last]} {n['k401']} = {fmt(self.k401)} USD",
            f"{ds[last]} {n['p529']} = {fmt(self.p529)} USD",
        ]
        return out, tail_lines, flows


class Ctx:
    """Shared state: the price tables and the calendar strings."""

    def __init__(self, rng, years, spend_per_month, infer, buys=3, sells=0.25, ncomm=6):
        self.rng = rng
        self.buys, self.sells, self.ncomm = buys, sells, ncomm
        self.spend_per_month = spend_per_month
        self.mean_amount = 2_800.0 / spend_per_month
        self.infer = infer
        self.start = datetime.date(START_YEAR, 1, 1)
        self.days = (datetime.date(START_YEAR + years, 1, 1) - self.start).days
        self.dstr = {}
        for y in range(START_YEAR, START_YEAR + years):
            for m in range(1, 13):
                self.dstr[(y, m)] = [None] + [f"{y}-{m:02d}-{d:02d}" for d in range(1, 32)]
        self.series = {}
        for sym, _, prec, p0, drift, vol, _name in COMMODITIES:
            self.series[sym] = self._walk(p0, drift, vol, whole_dollars=(sym == "BTC"))
        self.series["EUR"] = self._walk(EUR_START, EUR_DRIFT, EUR_VOL, whole_dollars=False)

    def _walk(self, p0, drift, vol, whole_dollars):
        rng, out, p = self.rng, [], p0
        step = drift / 365.0
        for _ in range(self.days):
            p = max(0.5, p * (1.0 + step + vol * rng.gauss(0.0, 1.0)))
            cents = int(round(p * 100))
            if whole_dollars:
                cents = max(100, int(round(p)) * 100)
            out.append(max(1, cents))
        return out

    def index(self, y, m, d):
        return (datetime.date(y, m, d) - self.start).days

    def price(self, sym, y, m, d):
        """Cost of one traded unit in cents. BTC is traded in 0.01 steps, so one unit costs dollars-per-BTC cents."""
        p = self.series[sym][self.index(y, m, d)]
        return p // 100 if sym == "BTC" else p

    def quote(self, sym, price):
        """The quoted price (per whole commodity) in cents, for `@`."""
        return price * 100 if sym == "BTC" else price

    def eur_rate(self, y, m, d):
        return self.series["EUR"][self.index(y, m, d)]


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--flows", default="10k", help="10k, 100k, 1m, 5m, or any number such as 250k")
    ap.add_argument("--out", required=True, help="directory to write (created; existing files are overwritten)")
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--people", type=int)
    ap.add_argument("--years", type=int)
    ap.add_argument("--variant", choices=("full", "nolaws"), default="full")
    ap.add_argument("--no-infer", action="store_true", help="write no `?` amounts")
    ap.add_argument("--buys", type=int, default=3, help="brokerage buys per person-month (each is a new lot)")
    ap.add_argument("--sells", type=float, default=0.25, help="expected brokerage sales per person-month")
    ap.add_argument("--commodities", type=int, default=6, help="how many of the six traded commodities are used")
    ap.add_argument("--budget-factor", type=float, default=None, help="budget as a multiple of the mean month; below 1 every month breaks it")
    ap.add_argument("--layout", choices=("months", "single"), default="months", help="`single` writes one journal.ax and `layout free`")
    ap.add_argument("--fixed-flows", type=float, default=26.0, help="initial estimate of flows per person-month besides the everyday spending (the generator steers itself)")
    args = ap.parse_args()

    target = parse_flows(args.flows)
    people, years = scale_for(target)
    people = args.people or people
    years = args.years or years
    rng = random.Random(args.seed)
    t0 = time.time()

    person_months = people * years * 12
    per_pm = target / person_months
    spend = max(4, int(round(per_pm - args.fixed_flows)))
    ctx = Ctx(rng, years, spend, not args.no_infer, args.buys, args.sells, args.commodities)
    if args.budget_factor is not None:
        global BUDGET_FACTOR
        BUDGET_FACTOR = args.budget_factor
    persons = [Person(i, rng, args.variant, years * 12) for i in range(people)]
    full = args.variant == "full"
    root = args.out
    os.makedirs(os.path.join(root, "journal"), exist_ok=True)
    os.makedirs(os.path.join(root, "prices"), exist_ok=True)

    # axiom.ax
    ax = [f"// Synthetic benchmark project: {target:,} flows target, {people} people, {years} years from {START_YEAR}.",
          f"// seed {args.seed}, variant {args.variant}. Generated by bench/gen.py; do not edit.", "", "base USD"]
    if full:
        ax += ["use std", "use us/401k", "use us/529", "use us/ca", "use us/ny", "use us/ny/nyc"]
    if args.layout == "single":
        ax.append("layout free")
    ax.append("")
    for p in persons:
        p.declare_entities(ax, START_YEAR, START_YEAR + years - 1)
    with open(os.path.join(root, "axiom.ax"), "w") as f:
        f.write("\n".join(ax) + "\n")

    # accounts.ax
    acc = ["// Commodities, places, payees and grants.", ""]
    for sym, kind, prec, *_r, name in COMMODITIES:
        acc.append(f"commodity {sym}{' : ' + kind if full else ''}\n  precision {prec}\n  name \"{name}\"" +
                   ("\n  grows 6% yearly" if full and kind in ("fund", "stock") else ""))
    acc.append("")
    if full:
        acc += ["/// Checks are written from a bank account.", "code chk-*", "  on bank", ""]
    for p in persons:
        p.declare_accounts(acc)
        if p.grants:
            for y in range(START_YEAR, START_YEAR + years):
                acc.append(f"entity scholarship-{p.k}-{y}{' : grant' if full else ''}\n  via {p.p['grants']}" +
                           (f"\n  purpose education\n  until {y}-12-31" if full else ""))
            acc.append("")
    with open(os.path.join(root, "accounts.ax"), "w") as f:
        f.write("\n".join(acc) + "\n")

    # plans.ax: after the history ends
    end = START_YEAR + years
    plans = ["// What has not happened yet, for the forecast.", ""]
    for p in persons:
        n = p.n
        plans += [
            f"every month on 1 from {end}-01-01 until {end + 2}-12 {n['chk']} -> {p.landlord} {fmt(int(p.rent * 1.03 ** years))} USD",
            f"every month on 3 from {end}-01-01 until {end + 2}-12 {n['chk']} -> {p.k}/housing/utilities/electric 120.00 USD",
            f"every year on 07-10 from {end}-07-10 {n['chk']} -> {p.k}/travel/domestic/flights 2_200 USD",
            f"every year on 01-01 from {end}-01-01 {n['chk']} -> {p.k}/housing/rent/renters-insurance 630 USD",
            f"every year on 12-15 from {end}-12-15 {p.acme} -> 4_000 USD\n  {n['k401']} 400 USD\n  {n['fed']} 880 USD\n"
            f"  {n['state']} 409 USD\n  {n['payroll']} 306 USD\n  {n['chk']} ...",
            f"every year on 08-20 from {end}-08-20 {n['p529']} -> {p.university} 1_200 USD",
            "",
        ]
    with open(os.path.join(root, "plans.ax"), "w") as f:
        f.write("\n".join(plans) + "\n")

    # prices, one file per year
    for y in range(START_YEAR, START_YEAR + years):
        lines = [f"// Daily prices for {y}. Synthetic."]
        d = datetime.date(y, 1, 1)
        while d.year == y:
            i = (d - ctx.start).days
            ds = d.isoformat()
            for sym, _k, prec, *_r in COMMODITIES:
                p = ctx.series[sym][i]
                lines.append(f"{ds} {sym} {fmt(p)} USD")
            lines.append(f"{ds} EUR {fmt(ctx.series['EUR'][i])} USD")
            d += datetime.timedelta(days=1)
        with open(os.path.join(root, "prices", f"{y}.ax"), "w") as f:
            f.write("\n".join(lines) + "\n")

    # the journal
    total_flows = 0
    total_lines = 0
    mi = 0
    fixed_seen = args.fixed_flows
    single = []
    for y in range(START_YEAR, START_YEAR + years):
        if args.layout == "months":
            os.makedirs(os.path.join(root, "journal", str(y)), exist_ok=True)
        for m in range(1, 13):
            # steer the everyday spending so the total lands on the target
            left_pm = person_months - mi * people
            need_pm = (target - total_flows) / left_pm
            ctx.spend_per_month = max(4, int(round(need_pm - fixed_seen)))
            ctx.mean_amount = 2_800.0 / ctx.spend_per_month
            month_flows = 0
            by_day = {}
            tails = []
            for p in persons:
                out, tail, flows = p.month(y, m, mi, ctx)
                total_flows += flows
                month_flows += flows
                for d, text in out:
                    by_day.setdefault(d, []).append(text)
                tails.extend(tail)
            lines = [f"// {y}-{m:02d}"]
            for d in sorted(by_day):
                lines.extend(by_day[d])
            lines.extend(tails)
            total_lines += len(lines)
            if args.layout == "single":
                single.extend(lines)
            else:
                with open(os.path.join(root, "journal", str(y), f"{m:02d}.ax"), "w") as f:
                    f.write("\n".join(lines) + "\n")
            fixed_seen = 0.7 * fixed_seen + 0.3 * (month_flows / people - ctx.spend_per_month)
            mi += 1

    if args.layout == "single":
        with open(os.path.join(root, "journal.ax"), "w") as f:
            f.write("\n".join(single) + "\n")
    last_year = START_YEAR + years - 1
    with open(os.path.join(root, "MANIFEST"), "w") as f:  # read by run.sh (shell `key=value`)
        f.write(f"variant={args.variant}\nseed={args.seed}\nflows={total_flows}\npeople={people}\nyears={years}\n"
                f"first_year={START_YEAR}\nlast_year={last_year}\ntoday={last_year}-12-31\nlast_month={last_year}-12\n"
                f"journal_lines={total_lines}\n")
    print(f"{args.variant} {args.flows}: {people} people, {years} years, ~{ctx.spend_per_month} everyday txns/person-month, "
          f"{total_flows:,} flows, {total_lines:,} journal lines, {time.time() - t0:.1f}s", file=sys.stderr)


if __name__ == "__main__":
    main()
