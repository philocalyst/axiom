"""Independent check of examples/05-family: the household's 2025 return worked out
from the journal text, without Axiom. Prints what the README quotes.

Run: python3 examples/verify/verify05.py
"""
import os
import re
import sys
from decimal import Decimal as D, ROUND_HALF_EVEN

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from axparse import read_journal  # noqa: E402

ROOT = os.path.join(HERE, "..", "05-family")
flows = [f for f in read_journal(ROOT) if f.day.year == 2025]


def cents(x):
    return x.quantize(D("0.01"), rounding=ROUND_HALF_EVEN)


# ── a paycheck is a header (gross) and legs; wages are the gross, pre-tax is what
#    went to a 401(k), the HSA, the FSA and the premium
wages = pretax = D(0)
federal = state = D(0)
deferral = {"alex-401k": D(0), "jordan-401k": D(0)}
hsa_payroll = dcfsa = D(0)
for f in flows:
    if f.src in ("acme", "bluefin") and f.legs:
        wages += f.into
        for place, amt, unit in f.legs:
            if place in ("alex-401k", "jordan-401k"):
                pretax += amt
                deferral[place] += amt
            elif place == "hsa":
                pretax += amt
                hsa_payroll += amt
            elif place == "dcfsa":
                pretax += amt
                dcfsa += amt
            elif place == "health-premium":
                pretax += amt
            elif place == "taxes/federal":
                federal += amt
            elif place == "taxes/state":
                state += amt

interest = sum((f.out or f.into for f in flows if f.src == "interest"), D(0))

# ── the 529: value and basis, to find the earnings in each withdrawal (pro rata)
value, basis = D("24600"), D("19850")
earnings = D(0)
plan_in = D(0)
import glob
lines = []
for path in sorted(glob.glob(os.path.join(ROOT, "journal", "2025", "*.ax"))):
    lines += [(os.path.basename(path), l) for l in open(path).read().split("\n")]
events = []
for f in flows:
    if f.dst == "riley-529":
        events.append((f.day, 1, "in", f.out or f.into))
        if f.src != "market":
            plan_in += f.out or f.into
    if f.src == "riley-529":
        events.append((f.day, 1, "out", f.out or f.into, f.dst))
for _, l in lines:
    m = re.match(r"^(2025-\d\d-\d\d) riley-529 = ([\d_.]+) USD via market", l)
    if m:
        events.append((__import__("datetime").date.fromisoformat(m.group(1)), 2, "stmt", D(m.group(2).replace("_", ""))))
withdrawals = []
for ev in sorted(events, key=lambda e: (e[0], e[1])):
    if ev[2] == "in":
        value += ev[3]
        basis += ev[3]
    elif ev[2] == "stmt":
        value = ev[3]
    else:
        amount = ev[3]
        share = basis * amount / value
        gain = amount - share
        basis -= share
        value -= amount
        withdrawals.append((ev[0], ev[4], amount, cents(gain)))
        if ev[4] != "tuition":
            earnings += gain
earnings = cents(earnings)
hsa_reimbursed = D(620)                       # counted: a count is not a violation
distributions = earnings + hsa_reimbursed

# ── itemizing
mortgage_interest = sum((amt for f in flows if f.dst == "lender" or f.payee == "lender" for place, amt, u in f.legs if place == "mortgage-interest"), D(0))
property_tax = sum((f.out or f.into for f in flows if f.dst == "property-tax"), D(0))
charity = sum((f.out or f.into for f in flows if f.dst == "charity"), D(0))
sdi = sum((amt for f in flows if f.src in ("acme", "bluefin") for place, amt, u in f.legs if place == "taxes/sdi"), D(0))
state_prior = sum((f.out or f.into for f in flows if f.dst == "state-prior"), D(0))
salt = state + sdi + state_prior + property_tax
itemized = mortgage_interest + charity + min(salt, D(40000))

total_income = wages - pretax + interest + distributions
agi = total_income
std = D(31500)
deduction = max(std, itemized)
taxable = agi - deduction
joint = [(0, "0.10"), (23850, "0.12"), (96950, "0.22"), (206700, "0.24"), (394600, "0.32"), (501050, "0.35"), (751600, "0.37")]


def progressive(schedule, x):
    t = D(0)
    for i, (lo, rate) in enumerate(schedule):
        hi = schedule[i + 1][0] if i + 1 < len(schedule) else None
        if x <= lo:
            break
        top = x if hi is None else min(x, D(hi))
        t += cents((top - D(lo)) * D(rate))
    return t


income_tax = progressive(joint, taxable)
credit = D(2200)
total_tax = income_tax - credit
payments = federal
owed = total_tax - payments

ca_joint = [(0, "0.01"), (22158, "0.02"), (52528, "0.04"), (82904, "0.06"), (115084, "0.08"), (145448, "0.093"), (742958, "0.103")]
ca_taxable = agi - D(11412)
ca_tax = progressive(ca_joint, ca_taxable)
ca_owed = ca_tax - state

print("wages", wages, " pretax", pretax, " interest", interest)
print("529 withdrawals (date, to, amount, earnings):", withdrawals)
print("distributions", distributions, "(529 earnings", earnings, "+ HSA reimbursement", hsa_reimbursed, ")  penalty 10% of earnings:", cents(earnings * D("0.1")), " HSA penalty waived:", cents(hsa_reimbursed * D("0.2")))
print("total income / agi", total_income)
print("itemized", itemized, "= interest", mortgage_interest, "+ salt", min(salt, D(40000)), "(state", state, "sdi", sdi, "prior", state_prior, "property", property_tax, ") + charity", charity, " vs standard", std)
print("taxable", taxable, " income tax", income_tax, " credit", credit, " total tax", total_tax, " payments", payments, " owed", owed)
print("CA taxable", ca_taxable, " tax", ca_tax, " withheld", state, " owed", ca_owed)
print("limits: alex 401k", deferral["alex-401k"], "of 23,500 ->", D(23500) - deferral["alex-401k"],
      "| jordan", deferral["jordan-401k"], "->", D(23500) - deferral["jordan-401k"],
      "| hsa", hsa_payroll + 1000, "of 8,550 ->", D(8550) - hsa_payroll - 1000,
      "| dcfsa", dcfsa, "| 529 contributions", plan_in, "->", D(19000) - plan_in)
