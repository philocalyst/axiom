"""Independent check of examples/04-freelancer: the 2025 return worked out from
the journal text, without Axiom. Prints what the README quotes.

Run: python3 examples/verify/verify04.py"""
import os
import sys
from decimal import Decimal as D, ROUND_HALF_EVEN

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from axparse import read_journal, recognize  # noqa: E402

ROOT = os.path.join(HERE, "..", "04-freelancer")
YEAR = 2025
flows = read_journal(ROOT)

CLIENTS = {"brightwave", "fernhill", "orbit-labs", "delta-rugs", "northpeak"}
BUSINESS = {"software", "business/insurance", "equipment", "education", "travel", "printing", "fonts", "hosting", "fees"}
HOME_VIA = {"landlord", "pge", "comcast", "statefarm"}


def cents(x):
    return x.quantize(D("0.01"), rounding=ROUND_HALF_EVEN)


receipts = D(0)
expenses = D(0)
health = D(0)
interest = D(0)
sep = D(0)
paid = D(0)
payments_n = 0
for f in flows:
    year = f.day.year
    if f.src in CLIENTS and f.dst != "bad-debt":
        # a payment: cash method, counted the day it arrives (never for a range)
        gross = f.out if f.out is not None else f.into
        if year == YEAR:
            receipts += gross
            payments_n += 1
        continue
    if f.dst in ("federal",):
        amount = f.out if f.out is not None else f.into
        yr = int(f.for_) if f.for_ and f.for_.isdigit() else year
        if yr == YEAR:
            paid += amount
        continue
    if f.dst == "sep" and f.src in ("business-checking",):
        amount = f.out if f.out is not None else f.into
        yr = int(f.for_) if f.for_ and f.for_.isdigit() else year
        if yr == YEAR:
            sep += amount
        continue
    if f.src == "interest" and f.dst == "tax-vault":
        if year == YEAR:
            interest += f.out if f.out is not None else f.into
        continue
    amount = f.out if f.out is not None else f.into
    # what the flow is recognized as, per payee or place
    place = f.dst
    via = f.payee
    if f.dst == "mileage":
        expenses += cents(recognize(f, amount * D("0.70"), YEAR)) if year == YEAR else D(0)
        continue
    if f.dst == "meals" or f.dst in ("meals",):
        expenses += cents(recognize(f, cents(amount * D("0.5")), YEAR))
        continue
    if f.dst == "bluecross" or via == "bluecross":
        if year == YEAR:
            health += amount
        continue
    if f.dst in HOME_VIA or via in HOME_VIA:
        if year == YEAR:
            expenses += cents(amount * D("0.15"))
        continue
    if f.dst == "tmobile":
        if year == YEAR:
            expenses += cents(amount * D("0.60"))
        continue
    is_biz = f.dst in BUSINESS or via in {"adobe", "figma", "notion", "google", "freshbooks", "dropbox", "hiscox", "apple", "ixdf", "config", "monotype", "squarespace", "alaska-air", "sf-hotel", "lyft"}
    if f.dst == "fees" and f.src in CLIENTS:
        continue
    if is_biz:
        expenses += recognize(f, amount, YEAR)
        continue
# fee legs of client payments are business expenses too
for f in flows:
    if f.src in CLIENTS and f.legs and f.day.year == YEAR:
        for place, amt, unit in f.legs:
            if place == "fees":
                expenses += amt

net = receipts - expenses
earnings = cents(net * D("0.9235"))
se = cents(min(earnings, D(176100)) * D("0.124")) + cents(earnings * D("0.029"))
half = cents(se * D("0.5"))
adjust = half + health + sep
total_income = net + interest
agi = total_income - adjust
std = D(15750)
attributable = net - half - health - sep
before = agi - std
qbi = cents(min(max(attributable, D(0)), before) * D("0.2"))
deductions = std + qbi
taxable = agi - deductions
brackets = [(0, D("0.10")), (11925, D("0.12")), (48475, D("0.22")), (103350, D("0.24"))]


def progressive(x):
    t = D(0)
    for i, (lo, rate) in enumerate(brackets):
        hi = brackets[i + 1][0] if i + 1 < len(brackets) else None
        if x <= lo:
            break
        top = x if hi is None else min(x, D(hi))
        t += cents((top - D(lo)) * rate)
    return t


income_tax = progressive(taxable)
total_tax = income_tax + se
owed = total_tax - paid
sep_cap = cents(min(cents((net - half) * D("0.20")), D(70000)))

print("gross receipts", receipts, "  (payments received:", payments_n, ")")
print("business expenses", expenses)
print("net profit", net)
print("health premiums", health, " interest", interest, " sep contributions for 2025", sep)
print("SE tax", se, "  half", half)
print("adjustments", adjust, "  total income", total_income, "  agi", agi)
print("qbi", qbi, " deductions", deductions, " taxable", taxable)
print("income tax", income_tax, "  total tax", total_tax)
print("payments for 2025", paid, "  owed", owed)
print("sep cap", sep_cap, " room", sep_cap - sep)
