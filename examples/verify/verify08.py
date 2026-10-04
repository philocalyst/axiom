"""Independent check of examples/08-expat: the 2025 return (with the foreign earned income
exclusion and its stacking tax), California's part-year return, the foreign account peaks and
the net worth, worked out without Axiom from the journal text and the exchange-rate file.

Run: python3 examples/verify/verify08.py
"""
import os
import re
import sys
from collections import defaultdict
from decimal import Decimal as D, ROUND_HALF_EVEN

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from axparse import read_journal  # noqa: E402

ROOT = os.path.join(HERE, "..", "..", "tests", "v4-syntax", "examples", "08-expat")
TODAY = "2026-04-16"
flows = read_journal(ROOT)


def cents(x):
    return x.quantize(D("0.01"), rounding=ROUND_HALF_EVEN)


# ── exchange rates: the latest price on or before a day ─────────────────────────────
rates = defaultdict(list)
for l in open(os.path.join(ROOT, "prices", "fx.ax")):
    m = re.match(r"^(\d{4}-\d\d-\d\d) ([A-Z]+) ([\d.]+) USD", l)
    if m:
        rates[m.group(2)].append((m.group(1), D(m.group(3))))


def rate(unit, day):
    if unit == "USD":
        return D(1)
    best = None
    for d, r in rates[unit]:
        if d <= day:
            best = r
    return best


def usd(amount, unit, day):
    return amount if unit == "USD" else cents(amount * rate(unit, day))


# ── replay every flow, unit by unit, place by place ──────────────────────────────────
held = defaultdict(lambda: defaultdict(D))
FOREIGN = {"girokonto": "EUR", "tagesgeld": "EUR", "kaution": "EUR", "wise": "GBP"}
peaks = {p: D(0) for p in FOREIGN}


def put(place, unit, amount, day):
    held[place][unit] += amount
    if place in FOREIGN:                       # the `fbar-max` law: the value in dollars after each change
        v = usd(held[place][unit], unit, day)
        peaks[place] = max(peaks[place], v)


in_opening = False
for l in open(os.path.join(ROOT, "journal", "2025", "01.ax")).read().split("\n"):
    if l.startswith("opening "):
        in_opening = True
    elif not l.startswith("  "):
        in_opening = False
    m = re.match(r"^  (\S+)\s+([\d_.]+)\s+([A-Z]+)", l)
    if in_opening and m:
        sign = -1 if m.group(1) in ("student-loan", "visa") else 1          # a liability's opening is what is owed
        held[m.group(1)][m.group(3)] += sign * D(m.group(2).replace("_", ""))

wages = pretax = interest = foreign_earned = foreign_tax = student = ca_withheld = federal_paid = D(0)
for f in flows:
    day = f.day.isoformat()
    year_for = int(f.for_) if f.for_ and re.fullmatch(r"\d{4}", f.for_) else f.day.year
    if f.legs:
        if f.dst:                                  # not in this journal
            raise SystemExit("unexpected split shape")
        # the legs are the targets of the header's source
        if f.out is not None:                      # `girokonto 900.00 EUR ->` : the source names its amount
            put(f.src, f.out_unit, -f.out, day)
            rest_unit = None
        else:                                      # `acme -> 5_400.00 EUR` : the header states the total
            rest_unit = f.into_unit
        total = sum((a for p, a, u in f.legs if a is not None and u == rest_unit), D(0))
        for place, amount, unit in f.legs:
            if amount is None:
                amount, unit = f.into - total, rest_unit
            put(place, unit, amount, day)
            value = usd(amount, unit, day)
            if f.src in ("acme", "employer-gmbh"):                        # income from a wages place
                wages += value
                if f.src == "employer-gmbh":
                    foreign_earned += value
                if place in ("us-401k",):
                    pretax += value
                if place == "taxes/federal":
                    federal_paid += value
                if place == "taxes/state":
                    ca_withheld += value
            if f.src in ("interest", "income/interest-de"):
                interest += value
            if place in ("lohnsteuer", "kapitalertragsteuer"):
                foreign_tax += value
            if place == "loan-interest":
                student += value
        if f.out is None:                          # the header's source gave the whole total
            put(f.src, f.into_unit, -f.into, day)
        continue
    if f.out is None:                              # `SRC -> DST 5 UNIT [@ price]`
        if f.price:
            put(f.src, f.price[1], -cents(f.into * f.price[0]), day)
        else:
            put(f.src, f.into_unit, -f.into, day)
        put(f.dst, f.into_unit, f.into, day)
    else:                                          # an exchange with both amounts
        put(f.src, f.out_unit, -f.out, day)
        put(f.dst, f.into_unit, f.into, day)
    if f.src in ("interest", "income/interest-de"):
        interest += usd(f.into, f.into_unit, day)
    if f.dst == "taxes/federal" and year_for == 2025:
        federal_paid += f.into
    if f.dst == "taxes/state" and year_for == 2025:
        ca_withheld += f.into

# ── the return ──────────────────────────────────────────────────────────────────────────────────────
single = [(0, "0.10"), (11925, "0.12"), (48475, "0.22"), (103350, "0.24"), (197300, "0.32"), (250525, "0.35"), (626350, "0.37")]
ca_single = [(0, "0.01"), (11079, "0.02"), (26264, "0.04"), (41452, "0.06"), (57542, "0.08"), (72724, "0.093"),
             (371479, "0.103"), (445771, "0.113"), (742953, "0.123")]


def progressive(schedule, x):
    t = D(0)
    for i, (lo, r) in enumerate(schedule):
        hi = schedule[i + 1][0] if i + 1 < len(schedule) else None
        if x <= lo:
            break
        top = x if hi is None else min(x, D(hi))
        t += cents((top - D(lo)) * D(r))
    return t


std = D(15750)
total_income = wages - pretax + interest
student_deduction = min(student, D(2500))
cap = D(130000) * 184 / 365
excluded = min(foreign_earned, cap)
with_it = max(total_income - student_deduction - std, D(0))
without = max(with_it - excluded, D(0))
stacking = progressive(single, with_it) - progressive(single, excluded) - progressive(single, without)
adjustments = student_deduction + excluded
agi = total_income - adjustments
salt = ca_withheld
deductions = max(std, salt)
taxable = agi - deductions
income_tax = progressive(single, taxable)
total_tax = income_tax + stacking
owed = total_tax - federal_paid
ca_taxable = max(agi - D(5706), D(0))
ca_tax = progressive(ca_single, ca_taxable)
ca_owed = ca_tax - ca_withheld

print("wages", wages, "= San Francisco", wages - foreign_earned, "+ Berlin", foreign_earned)
print("pre-tax", pretax, " interest", interest, " total income", total_income)
print("student loan interest", student, "-> deduction", student_deduction, "| exclusion cap", cap, "excluded", excluded)
print("agi", agi, " taxable income", taxable, " income tax", income_tax)
print("stacking: with the excluded pay", with_it, "without it", without, "extra tax", stacking)
print("total tax", total_tax, " paid for 2025", federal_paid, " owed", owed)
print("California: taxable", ca_taxable, "tax", ca_tax, "withheld", ca_withheld, "owed (negative = refund)", ca_owed)
print("German tax paid, in dollars", foreign_tax)
print("FBAR peaks", {p: str(v) for p, v in peaks.items()}, "total", sum(peaks.values()))

# ── net worth ──────────────────────────────────────────────────────────────────────────────────────
worth = D(0)
for place in ("us-checking", "us-savings", "us-401k", "girokonto", "tagesgeld", "kaution", "wise", "student-loan", "visa"):
    for unit, qty in held[place].items():
        worth += usd(qty, unit, TODAY)
print("foreign holdings", {p: {u: str(q) for u, q in held[p].items() if q} for p in FOREIGN})
print("net worth on", TODAY, worth)
