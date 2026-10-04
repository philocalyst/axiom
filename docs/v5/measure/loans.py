#!/usr/bin/env python3
"""The oracle of lane K5d: a loan is a state machine with four inputs, and it is held to an independent ACTUS annuity.

    loans.py selftest                       the reference against a textbook, 07-landlord's statements and its own laws
    loans.py gen DIR N [SEED]               write N projects (DIR/pNNNN/main.ax and expect.txt), and DIR/forms.json
    loans.py build TREE OUT                 build the dump (loans/) against the crates of TREE into OUT/
    loans.py check BINARY DIR [JOBS]        what BINARY says of every project, against what the reference says
    loans.py mutate TREE WORK DIR [N,M..]   the mutants of the loan's arithmetic and its walk: each must be caught

What it is for. K5a built the arithmetic of a level payment and held it to the engine's. K5d makes a loan the fold of four
events (a payment falls due, a prepayment, a reset of the rate by an index, a rate a statement says) over a state, writes
every payment as a split of interest and principal, and holds a statement of the balance to the schedule. What judges it is
**a reference written from the language and not from the code**: Python integers (no overflow, no floats), K5a's fixed-point
rule for the annuity (18 places, rounded half to even at every step of the loop, because the cents of a payment are the ones
the books have always had), and the schedule's events merged by day:

    day order   a rate that changes on a day applies to that day's payment; a payment comes before a prepayment of its day
    Pay         interest is the balance times the period's rate, to the cent; the last payment is what is left
    Prepay      anything paid beyond the scheduled payment: a flow into the loan's debt tab, and what a line that states its
                own amount pays over the payment of its due day. `shortens` keeps the payment and the loan ends sooner;
                `recasts` keeps the payments left and lowers the payment
    Reset       `index + margin`, held to the previous rate +- `cap` and to the first rate +- `life`; the payment is
                refigured over the payments left
    Rate        `DATE LOAN now at R%`: the same, with no clamp

The corpus is a seeded generator (the same SEED and N write the same books) of one loan to a project, over the cadences a
loan has (monthly, quarterly, every 6m, every 12m, twice monthly, every 2w, every 10d, every 45d), with prepayments in both
modes, rate changes, resets with caps, a loan paid off early, a payment nobody wrote, a line that states its own amount (over
the payment, under it), and statements of the balance, some of them wrong in a way a missed payment, a short one, a
prepayment nobody wrote or an extra counted as principal explains. For every one the reference writes what the fold must say
(`expect.txt`): each payment's interest, principal and balance, each prepayment, the balance of the schedule on probe days,
what the kept lines post and what the forecast promises, the debt tab on the day of the run, the occurrences nothing kept,
and the diagnostics of the statements (and the cause each names, when exactly one candidate explains the difference).
"""
import calendar
import datetime
import json
import os
import random
import sys
from collections import Counter
from fractions import Fraction

HERE = os.path.dirname(os.path.abspath(__file__))

SCALE = 10 ** 18                     # the annuity factor is fixed point at 18 places
TODAY = datetime.date(2026, 6, 30)   # the day of the run
UNTIL = datetime.date(2027, 8, 31)   # how far the forecast looks


# ─── The arithmetic ──────────────────────────────────────────────────────────────────────────────────────────


def div_round(n, d):
    """n / d rounded half to even: the one rounding of the books."""
    if d < 0:
        n, d = -n, -d
    q, r = divmod(n, d)
    if 2 * r > d or (2 * r == d and q & 1):
        q += 1
    return q


def mul_div(a, n, d):
    return div_round(a * n, d)


def scale(qty, ratio):
    """qty times a Fraction, to the quantum."""
    return mul_div(qty, ratio.numerator, ratio.denominator)


def factor(rate, n):
    """The share of the principal paid each period, `r / (1 - (1 + r)^-n)`, as the engine has always worked it out: a loop of
    n multiplications at 18 places, rounded at each step (a power by squaring would round differently)."""
    if rate == 0:
        return Fraction(1, n)
    r = mul_div(rate.numerator, SCALE, rate.denominator)
    growth = SCALE
    for _ in range(n):
        growth = mul_div(growth, SCALE + r, SCALE)
    return Fraction(mul_div(r, growth, growth - SCALE), SCALE)


def needs(open_, payment, rate, bound):
    """How many payments of `payment` a loan of `open_` needs, by the loan's own recurrence: the first payment that covers what
    is owed. At most `bound`, the payments there were: a smaller balance never needs more."""
    for k in range(1, bound + 1):
        interest = scale(open_, rate)
        if open_ + interest <= payment or k == bound:
            return k
        open_ -= max(min(payment - interest, open_), 0)
    return bound


class Loan:
    """The terms of a loan and the state it is in: what the reference walks."""

    def __init__(self, principal, annual, per_year, periods, mode="shortens", resets=None):
        self.principal, self.first, self.per_year, self.mode, self.resets = principal, annual, per_year, mode, resets
        self.periods = periods
        self.open, self.annual, self.remaining = principal, annual, periods
        self.rate = annual * per_year
        self.payment = scale(principal, factor(self.rate, periods))

    def refigure(self, annual):
        """A new yearly rate (held between 0% and 100%): the payment is the one that pays what is left in the payments left."""
        self.annual = min(max(annual, Fraction(0)), Fraction(1))
        self.rate = self.annual * self.per_year
        if self.open and self.remaining:
            self.payment = scale(self.open, factor(self.rate, self.remaining))

    def reset(self, index):
        r = self.resets
        rate = index + r["margin"]
        if r["cap"] is not None:
            rate = min(max(rate, self.annual - r["cap"]), self.annual + r["cap"])
        if r["life"] is not None:
            rate = min(max(rate, self.first - r["life"]), self.first + r["life"])
        self.refigure(rate)

    def pay(self):
        interest = scale(self.open, self.rate)
        due = self.open + interest if self.remaining <= 1 else self.payment
        principal = min(max(due - interest, 0), self.open)
        self.open -= principal
        self.remaining = 0 if self.open == 0 else self.remaining - 1
        return interest, principal

    def prepay(self, amount):
        amount = min(amount, self.open)
        self.open -= amount
        if self.open == 0:
            self.remaining = 0
        elif self.mode == "shortens":
            self.remaining = needs(self.open, self.payment, self.rate, self.remaining)
        else:
            self.payment = scale(self.open, factor(self.rate, self.remaining))
        return amount


def add_months(day, months):
    index = day.year * 12 + day.month - 1 + months
    year, month = divmod(index, 12)
    return datetime.date(year, month + 1, min(day.day, calendar.monthrange(year, month + 1)[1]))


def add_span(day, months, days):
    return add_months(day, months) + datetime.timedelta(days=days)


def index_on(day):
    """What a reset reads: `idx` as PRELUDE says it, the latest row on or before the day."""
    rows = [(datetime.date(2020, 1, 1), Fraction(2, 100)), (datetime.date(2024, 1, 1), Fraction(3, 100)), (datetime.date(2025, 1, 1), Fraction(45, 1000)),
            (datetime.date(2026, 1, 1), Fraction(55, 1000)), (datetime.date(2027, 1, 1), Fraction(7, 100)),
            (datetime.date(2028, 1, 1), Fraction(2, 100))]
    return [value for when, value in rows if when <= day][-1]


def walk(loan, begins, dues, rates, prepays, stated):
    """The loan over its events in day order. `dues` are the owed days after `begins`; `rates` and `prepays` are (day, value)
    lists; `stated` maps a due day to the amount its line states. Rows: ("pay", day, interest, principal, open) and
    ("prepay", day, 0, amount, open)."""
    items = [(day, 2, number, "pay", None) for number, day in enumerate(dues[:loan.periods])]
    items += [(day, 1, number, "rate", rate) for number, (day, rate) in enumerate(rates)]
    items += [(day, 3, number, "prepay", amount) for number, (day, amount) in enumerate(prepays)]
    if loan.resets:
        every, k = loan.resets["every"], 0
        while (day := add_span(loan.resets["from"], every[0] * k, every[1] * k)) <= dues[min(loan.periods, len(dues)) - 1]:
            items.append((day, 0, k, "reset", index_on(day)))
            k += 1
    rows = []
    for day, _, _, kind, value in sorted(items, key=lambda item: item[:3]):
        if day < begins or loan.open == 0:
            continue
        if kind == "reset":
            loan.reset(value)
        elif kind == "rate":
            loan.refigure(value)
        elif kind == "pay":
            interest, principal = loan.pay()
            rows.append(("pay", day, interest, principal, loan.open))
            extra = stated.get(day, 0) - (interest + principal)
            if extra > 0 and loan.open:
                rows.append(("prepay", day, 0, loan.prepay(extra), loan.open))
        else:
            rows.append(("prepay", day, 0, loan.prepay(value), loan.open))
    return rows


def open_on(principal, rows, day):
    """The balance of the schedule on `day`, after every payment and prepayment on or before it."""
    open_ = principal
    for _, when, _, _, left in rows:
        if when > day:
            break
        open_ = left
    return open_


# ─── The corpus ──────────────────────────────────────────────────────────────────────────────────────────────

# A cadence: its text, how it is counted, what it may be `on`, how far from a due day a line may be and still keep it (half a
# cadence, a month being 31 days), and the terms a loan of it runs over: (text, months, days).
CADENCES = [
    ("monthly", ("months", 1), ["on 1", "on 5", "on 15", "on 28", "on last"], 15,
     [("10y", 120, 0), ("15y", 180, 0), ("30y", 360, 0), ("27y6m", 330, 0), ("5y", 60, 0)]),
    ("quarterly", ("months", 3), ["on 1", "on 15", "on last"], 46, [("5y", 60, 0), ("10y", 120, 0), ("20y", 240, 0)]),
    ("every 6m", ("months", 6), ["on 1", "on 15"], 93, [("10y", 120, 0), ("20y", 240, 0)]),
    ("every 12m", ("months", 12), [None], 186, [("5y", 60, 0), ("10y", 120, 0)]),
    ("twice monthly", ("twice", 0), ["on 15, last", "on 1, 15"], 15, [("2y", 24, 0), ("5y", 60, 0), ("10y", 120, 0)]),
    ("every 2w", ("days", 14), [None], 7, [("520d", 0, 520), ("700d", 0, 700), ("900d", 0, 900)]),
    ("every 45d", ("days", 45), [None], 22, [("900d", 0, 900), ("1800d", 0, 1800), ("3600d", 0, 3600)]),
    ("every 10d", ("days", 10), [None], 5, [("300d", 0, 300), ("500d", 0, 500)]),
]

RATES = [Fraction(0), Fraction(1, 100), Fraction(25, 1000), Fraction(4, 100), Fraction(5875, 100000), Fraction(675, 10000),
         Fraction(9, 100), Fraction(12, 100)]


def landings(on, month):
    """The days of a month an `on` names, in order."""
    last = calendar.monthrange(month.year, month.month)[1]
    if on is None:
        return [month.day]
    names = on.removeprefix("on ").split(", ")
    return sorted({last if part == "last" else min(int(part), last) for part in names})


def owed_days(cadence, anchor, count):
    """`count` owed days of a schedule counted from its `from` day, as `calendar::due` counts them: each step is the anchor
    plus a multiple of the cadence, months first and clamped, never counted from the step before; a step lands on each day
    its `on` names in its month; nothing is due before the anchor."""
    kind, n = cadence["count"]
    days, step = [], 0
    while len(days) < count:
        if kind in ("months", "twice"):
            month = add_months(anchor.replace(day=1), (n if kind == "months" else 1) * step)
            days.extend(d for d in (month.replace(day=day) for day in landings(cadence["on"], month)) if d >= anchor)
        else:
            days.append(anchor + datetime.timedelta(days=n * step))
        step += 1
    return days


def periods_of(cadence, months, days):
    """How many payments pay the loan off: as many periods as its term holds, a part of one being one."""
    kind, n = cadence["count"]
    return {"months": -(-months // n) if n else 0, "twice": months * 2, "days": -(-days // n) if n else 0}[kind]


def per_year(cadence):
    kind, n = cadence["count"]
    return {"months": Fraction(n, 12), "twice": Fraction(1, 24), "days": Fraction(n, 365)}[kind]


def money(cents):
    return f"{cents // 100:_}.{cents % 100:02d} USD"


def percent(fraction):
    """A rate as the language writes it: 5.875%."""
    return f"{float(fraction * 100):.4f}".rstrip("0").rstrip(".") + "%"


PRELUDE = """\
use std
base USD
param idx
  2020-01-01 2%
  2024-01-01 3%
  2025-01-01 4.5%
  2026-01-01 5.5%
  2027-01-01 7%
  2028-01-01 2%
entity me : person
entity bank : org
account checking : bank
asset condo : property
opening 2022-01-01
  checking 90_000_000 USD
"""


class Case:
    """One loan and the days its book writes down: what is drawn, and what the reference makes of it."""

    # Two of these are the names of kinds in std (`loan`, `mortgage`), which a statement about the contract must not mistake it for.
    NAMES = ["loan", "mortgage", "home-loan", "car-note"]

    def __init__(self, seed, number):
        self.rng = random.Random(seed * 1_000_003 + number)
        self.forms = Counter()
        self.name = self.rng.choice(self.NAMES)
        while not self.draw():
            pass
        self.settle()

    # -- what is drawn
    def draw(self):
        rng, forms = self.rng, self.forms
        forms.clear()
        text, count, ons, reach, terms = rng.choice(CADENCES)
        self.cadence = {"text": text, "count": count, "on": rng.choice(ons)}
        self.reach, (self.term_text, months, days) = reach, rng.choice(terms)
        forms[f"cadence:{text}"] += 1
        self.periods = periods_of(self.cadence, months, days)
        self.begins = datetime.date(rng.choice([2023, 2024, 2025, 2025, 2026]), rng.randint(1, 12), rng.randint(1, 28))
        self.anchor = add_months(self.begins.replace(day=1), rng.choice([0, 1, 1, 2]))
        self.annual = rng.choice(RATES)
        forms["rate:zero" if self.annual == 0 else "rate:positive"] += 1
        self.principal = rng.randint(2_000, 900_000) * 100 + rng.choice([0, 0, 17, 50, 99])
        self.mode = rng.choice(["shortens", "shortens", "recasts"])
        forms[f"prepay:{self.mode}"] += 1
        self.resets = None
        if self.cadence["count"][0] in ("months", "twice") and rng.random() < 0.4:
            self.resets = {"every": rng.choice([(12, 0), (6, 0), (24, 0)]), "margin": Fraction(rng.choice([0, 15, 25, 30]), 1000),
                           "cap": rng.choice([None, Fraction(1, 100), Fraction(2, 100)]),
                           "life": rng.choice([None, Fraction(3, 100), Fraction(5, 100)]),
                           "from": add_months(self.begins, rng.choice([6, 12, 14]))}
            forms["resets" + (":cap" if self.resets["cap"] else "") + (":life" if self.resets["life"] else "")] += 1
        self.dues = [d for d in owed_days(self.cadence, self.anchor, self.periods + 12) if d > self.begins][:self.periods]
        span = (min(TODAY, self.dues[-1]) - self.begins).days
        if span < 60 or sum(d <= TODAY for d in self.dues) < 3:
            return False
        self.span = span
        self.rates = sorted((self.begins + datetime.timedelta(days=rng.randint(20, span)), Fraction(rng.randint(100, 1100), 10000))
                            for _ in range(rng.choice([0, 0, 1, 2])))
        forms["rate-change"] += bool(self.rates)
        self.prepays = sorted((self.begins + datetime.timedelta(days=rng.randint(10, span)), rng.randint(1, 400) * 100 + rng.choice([0, 33]))
                              for _ in range(rng.choice([0, 0, 1, 2, 3])))
        forms["prepayment-flow"] += bool(self.prepays)
        # the lines that keep due days: most are written, on the day or a day or two off; some are not (missed)
        self.lines = {}
        for due in (d for d in self.dues if d <= TODAY):
            if rng.random() < 0.12:
                continue
            off = rng.choice([0, 0, 0, 1, -1, 2]) if self.reach >= 3 else 0
            at = due + datetime.timedelta(days=off)
            self.lines[due] = due if at == self.begins or at > TODAY else at
        forms["missed"] += len(self.lines) < sum(d <= TODAY for d in self.dues)
        # what a line states of its own: over the payment (an extra) or under it (short)
        self.over = {due: rng.choice([1, 1, -1, -2]) * rng.randint(1, 60_000) for due in self.lines if rng.random() < 0.12}
        self.payoff = rng.random() < 0.12
        self.seed_statements = rng.randint(0, 4)
        return True

    # -- what the reference makes of it
    def loan(self):
        return Loan(self.principal, self.annual, per_year(self.cadence), self.periods, self.mode, self.resets)

    def run(self, prepays, stated):
        return walk(self.loan(), self.begins, self.dues, self.rates, prepays, stated)

    def settle(self):
        rng, forms = self.rng, self.forms
        first = self.run(self.prepays, {})
        payments = {row[1]: row[2] + row[3] for row in first if row[0] == "pay"}
        self.stated = {d: max(1, payments[d] + over) for d, over in self.over.items() if d in payments}
        if self.payoff:
            day = self.begins + datetime.timedelta(days=rng.randint(self.span // 2, self.span))
            owed = open_on(self.principal, self.run(self.prepays, self.stated), day)
            if owed > 0:
                self.prepays = sorted(self.prepays + [(day, owed)])
                forms["paid-off"] += 1
        self.rows = self.run(self.prepays, self.stated)
        self.paid = {row[1]: row for row in self.rows if row[0] == "pay"}
        self.kept = {due: at for due, at in self.lines.items() if due in self.paid}
        forms["line-with-amount"] += bool(self.stated and set(self.stated) & set(self.kept))
        self.excess = {}
        for due, amount in self.stated.items():
            if due in self.paid and amount > self.paid[due][2] + self.paid[due][3]:
                self.excess[due] = amount - self.paid[due][2] - self.paid[due][3]
        self.statements()

    def posted(self, due):
        """What the fold posts for the line that keeps `due`: the interest, then what the amount it states leaves."""
        _, _, interest, principal, _ = self.paid[due]
        header = self.stated.get(due, interest + principal)
        paid = min(interest, header)
        return paid, header - paid

    def tab_on(self, day):
        """The debt tab as the books have it after what is written on or before `day`: the principal the origination made,
        less the principal each kept line posted and each flow written to the loan."""
        tab = self.principal - sum(self.posted(due)[1] for due, at in self.kept.items() if at <= day)
        return tab - sum(amount for when, amount in self.prepays if when <= day)

    def statements(self):
        rng = self.rng
        last = min(TODAY, self.dues[-1] + datetime.timedelta(days=30))
        self.values = []
        for _ in range(self.seed_statements):
            day = self.begins + datetime.timedelta(days=rng.randint(5, (last - self.begins).days))
            owed = open_on(self.principal, self.rows, day)
            stated = {"schedule": owed, "tab": self.tab_on(day), "prepayment": owed - rng.randint(1, 50) * 100,
                      "off": owed + rng.randint(1, 5000)}[rng.choice(["schedule", "tab", "tab", "prepayment", "off"])]
            self.values.append((day, stated if stated >= 0 else owed))
        self.values.sort()
        self.forms["statements"] += len(self.values)

    def cause(self, day, stated):
        """What explains a statement to the cent, if exactly one candidate does; None if it agrees with the schedule."""
        gap = stated - open_on(self.principal, self.rows, day)
        if gap == 0:
            return None
        payments = [row for row in self.rows if row[0] == "pay" and row[1] <= day]
        holds = []
        if gap < 0:
            holds.append("prepayment")
        if gap > 0:
            if sum(row[3] for row in payments if row[1] not in self.kept) == gap:
                holds.append("missed")
            short = sum(row[3] - self.posted(row[1])[1] for row in payments if row[1] in self.kept)
            if short == gap:
                holds.append("short")
            extras = [amount for due, amount in self.excess.items() if due <= day]
            if extras and (gap in extras or gap == sum(extras)):
                holds.append("extra")
        return holds[0] if len(holds) == 1 else "none"

    # -- the book
    def text(self):
        c, rng = self.cadence, random.Random(self.principal)
        loan = [f"  loan {self.principal // 100:_}.{self.principal % 100:02d} USD on {self.begins} at {percent(self.annual)} "
                f"over {self.term_text}" + (" for condo" if rng.random() < 0.5 else "")]
        if self.resets:
            r = self.resets
            every = f"{r['every'][0] // 12}y" if r["every"][0] % 12 == 0 else f"{r['every'][0]}m"
            tail = (f" cap {percent(r['cap'])}" if r["cap"] else "") + (f" life {percent(r['life'])}" if r["life"] else "")
            loan.append(f"    resets {every} from {r['from']} to idx + {percent(r['margin'])}{tail}")
        loan.append(f"    prepay {self.mode}")
        lines = [f"contract {self.name} with bank", *loan, f"  {c['text']}{' ' + c['on'] if c['on'] else ''} from checking",
                 f"  from {self.anchor}"]
        events = [(self.begins, f"{self.begins} {self.name}")]
        for due, at in self.lines.items():
            if due in self.paid:
                events.append((at, f"{at} {self.name}" + (f" {money(self.stated[due])}" if due in self.stated else "")))
        events += [(day, f"{day} {self.name} now at {percent(rate)}") for day, rate in self.rates]
        events += [(day, f"{day} checking -> {self.name} {money(amount)}") for day, amount in self.prepays]
        events += [(day, f"{day} {self.name} = {money(stated)}") for day, stated in self.values]
        events.sort(key=lambda event: event[0])
        return "\n".join([PRELUDE.rstrip("\n"), *lines, *(text for _, text in events)]) + "\n"

    def expectation(self):
        name, out = self.name, []
        out += [f"{row[0]} {name} {row[1]} {row[2]} {row[3]} {row[4]}" for row in self.rows]
        for due in sorted(self.kept):
            out.append("posted {} {} {} {}".format(name, due, *self.posted(due)))
        for due in self.dues:
            if TODAY < due <= UNTIL and due in self.paid and due not in self.lines:
                out.append(f"planned {name} {due} {self.paid[due][2]} {self.paid[due][3]}")
        last_kept = max(self.kept, default=None)
        for due in self.dues:
            if due in self.paid and due not in self.kept:
                if due + datetime.timedelta(days=self.reach + 1) <= TODAY or (last_kept and due < last_kept):
                    out.append(f"missed {name} {due}")
        probe, end = self.begins, min(UNTIL, self.dues[-1] + datetime.timedelta(days=90))
        while probe <= end:
            out.append(f"open {name} {probe} {open_on(self.principal, self.rows, probe)}")
            probe = add_months(probe.replace(day=1), 1)
            probe = probe.replace(day=calendar.monthrange(probe.year, probe.month)[1])
        out.append(f"tab {name} {TODAY} {self.tab_on(TODAY)}")
        last_tab = last_loan = 0
        for day, stated in self.values:
            gap = stated - self.tab_on(day)
            out += ["diag assertion"] * (gap != 0 and gap != last_tab)
            last_tab = gap
            gap = stated - open_on(self.principal, self.rows, day)
            out += [f"diag loan-balance {self.cause(day, stated)}"] * (gap != 0 and gap != last_loan)
            last_loan = gap
        return out


# ─── The commands ────────────────────────────────────────────────────────────────────────────────────────────


def selftest():
    # A textbook: 100,000.00 at 6% over 30 years, monthly: 599.55 a month, 500.00 of interest in the first.
    loan = Loan(100_000_00, Fraction(6, 100), Fraction(1, 12), 360)
    assert loan.payment == 59955, loan.payment
    assert loan.pay() == (50_000, 9_955)
    # The statements 07-landlord was written with, hand-checked by its author, to the cent.
    loan = Loan(279_000_00, Fraction(675, 10000), Fraction(1, 12), 360)
    assert loan.payment == 180959
    for want in [278_759_79, 278_518_22, 278_275_29, 278_031_00, 277_785_33, 277_538_28, 277_289_84, 277_040_01, 276_788_77,
                 276_536_12, 276_282_05]:
        loan.pay()
        assert loan.open == want, (loan.open, want)
    # A loan pays itself off: the last payment is what is left.
    for principal, annual, periods in [(300_000, Fraction(0), 3), (30_000_00, Fraction(5, 100), 36), (99_99, Fraction(4, 100), 7)]:
        loan = Loan(principal, annual, Fraction(1, 12), periods)
        paid = [loan.pay() for _ in range(periods)]
        assert loan.open == 0 and sum(p for _, p in paid) == principal
    # A prepayment that shortens never lengthens; one that recasts at the start of nothing leaves the payment as it was.
    loan = Loan(250_000_00, Fraction(5875, 100000), Fraction(1, 12), 360)
    for _ in range(11):
        loan.pay()
    before = loan.remaining
    loan.prepay(5_000_00)
    assert loan.remaining < before
    loan = Loan(250_000_00, Fraction(5875, 100000), Fraction(1, 12), 360, "recasts")
    first = loan.payment
    loan.prepay(0)
    assert loan.payment == first and loan.remaining == 360
    # Stepping the payments a payment needs ends at zero, on the last and not before.
    rng = random.Random(3)
    for _ in range(300):
        periods = rng.choice([12, 36, 120, 360])
        loan = Loan(rng.randint(2_000, 800_000) * 100, Fraction(rng.randint(1, 1200), 10000), Fraction(1, 12), periods)
        for _ in range(rng.randint(0, periods // 2)):
            loan.pay()
        if loan.open:
            loan.prepay(rng.randint(1, max(1, loan.open - 1)))
            for _ in range(loan.remaining):
                assert loan.open > 0, "paid off early"
                loan.pay()
            assert loan.open == 0, "not paid off"
    # A reset is held to its caps: the previous rate +- cap, the first rate +- life.
    loan = Loan(100_000_00, Fraction(5, 100), Fraction(1, 12), 360, "shortens",
                {"every": (12, 0), "from": datetime.date(2027, 1, 1), "margin": Fraction(3, 100), "cap": Fraction(1, 100),
                 "life": Fraction(3, 100)})
    for index, want in [(8, 6), (10, 7), (20, 8)]:
        loan.reset(Fraction(index, 100))
        assert loan.annual == Fraction(want, 100), (index, loan.annual)
    # The corpus is deterministic, and what it writes is consistent with itself.
    a, b = Case(7, 3), Case(7, 3)
    assert a.text() == b.text() and a.expectation() == b.expectation()
    for number in range(200):
        case = Case(1, number)
        assert sum(row[3] for row in case.rows) <= case.principal
    print("selftest ok")


def gen(directory, count, seed):
    os.makedirs(directory, exist_ok=True)
    forms = Counter()
    for number in range(count):
        case = Case(seed, number)
        path = os.path.join(directory, f"p{number:04d}")
        os.makedirs(path, exist_ok=True)
        with open(os.path.join(path, "main.ax"), "w") as handle:
            handle.write(case.text())
        with open(os.path.join(path, "expect.txt"), "w") as handle:
            handle.write("\n".join(case.expectation()) + "\n")
        forms.update(case.forms)
    with open(os.path.join(directory, "forms.json"), "w") as handle:
        json.dump(forms, handle, indent=1, sort_keys=True)
    print(f"{count} projects")
    for key in sorted(forms):
        print(f"  {key:<24} {forms[key]:>6}")


def main(argv):
    if not argv:
        print(__doc__)
        return 2
    command, rest = argv[0], argv[1:]
    if command == "selftest":
        return selftest() or 0
    if command == "gen":
        return gen(rest[0], int(rest[1]), int(rest[2]) if len(rest) > 2 else 7) or 0
    print(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
