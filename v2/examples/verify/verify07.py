"""Independent check of examples/07-landlord: the loan, the depreciation, the rental's
year, the sale and the 2025 return, worked out without Axiom from the journal text
(regular flows through axparse, plan occurrences by their own reading of the lines).

Run: python3 examples/verify/verify07.py
"""
import os
import re
import sys
from datetime import date
from decimal import Decimal as D, ROUND_HALF_EVEN

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from axparse import read_journal  # noqa: E402

ROOT = os.path.join(HERE, "..", "07-landlord")
flows = read_journal(ROOT)


def cents(x):
    return x.quantize(D("0.01"), rounding=ROUND_HALF_EVEN)


def money(s):
    return D(s.replace("_", ""))


# ── plan occurrences, read from the lines ─────────────────────────────────────────
OCC = re.compile(r"^(\d{4}-\d\d-\d\d) (paycheck|rent-a|rent-b|mortgage-payment|manager-fee|depreciation)(?: ([\d_.]+) USD)?\s*$")
occ = []                      # (date, plan, amount or None, legs)
files = sorted(os.popen(f"find {ROOT}/journal -name '*.ax'").read().split())
for path in files:
    lines = open(path).read().split("\n")
    for i, l in enumerate(lines):
        m = OCC.match(re.sub(r"\s//.*$", "", l))
        if not m:
            continue
        legs = {}
        j = i + 1
        while j < len(lines) and lines[j].startswith("  ") and lines[j].strip() and not lines[j].strip().startswith("//"):
            lm = re.match(r"^\s+(\S+)\s+([\d_.]+) USD", lines[j])
            legs[lm.group(1)] = money(lm.group(2))
            j += 1
        occ.append((m.group(1), m.group(2), money(m.group(3)) if m.group(3) else None, legs))

# ── the loan: 279,000 at 6.75%, 30 years, first payment on 2025-02-01 ───────────────────
LOAN = D(279000)
rate = D("0.0675") / 12
payment = cents(LOAN * rate / (1 - (1 + rate) ** -360))
balance = LOAN
schedule = {}
interest_2025 = D(0)
for month in range(2, 13):
    interest = cents(balance * rate)
    principal = payment - interest
    schedule[month] = (principal, interest)
    interest_2025 += interest
    balance -= principal
accrued = cents(balance * rate * D(28) / D(30))     # 28 days of the last month, at the payoff
interest_2025 += accrued
print("payment", payment, "| interest paid Feb-Dec", interest_2025 - accrued, "| accrued at payoff", accrued, "| payoff principal", balance)

# each mortgage-payment occurrence in the journal against the schedule
paid_interest = D(0)
for day, plan, amount, legs in occ:
    if plan != "mortgage-payment":
        continue
    month = int(day[5:7])
    principal, interest = schedule[month]
    if legs:
        assert legs["mortgage"] == principal and legs["interest"] == interest, (day, legs, principal, interest)
    else:
        assert (D("240.21"), D("1569.38")) == (principal, interest), day    # the plan's own legs
    paid_interest += interest
assert sum(1 for o in occ if o[1] == "mortgage-payment") == 11

# ── depreciation: global cumulative rounding, including the sale-day half-month ──────────────
building = cents(D(376850) * D("0.80"))
life_months = D("27.5") * 12
roof = D(14200)
house_edges = {month: D(month) - D("0.5") for month in range(1, 12)}
house_edges[12] = D(11)
roof_edges = {month: D(month - 8) - D("0.5") for month in range(9, 12)}
roof_edges[12] = D(3)


def cumulative_recovery(cost, service_months):
    return cents(cost * service_months / life_months)


expected_dep = {}
previous_house = previous_roof = D(0)
for month in range(1, 13):
    house_total = cumulative_recovery(building, house_edges[month])
    roof_total = cumulative_recovery(roof, roof_edges[month]) if month in roof_edges else previous_roof
    expected_dep[month] = house_total - previous_house + roof_total - previous_roof
    previous_house, previous_roof = house_total, roof_total
dep_total = D(0)
depreciation_months = set()
for day, plan, amount, legs in occ:
    if plan != "depreciation":
        continue
    month = int(day[5:7])
    assert amount is not None, (day, "write the computed monthly amount")
    assert month not in depreciation_months, (day, "duplicate depreciation month")
    depreciation_months.add(month)
    said = amount
    assert said == expected_dep[month], (day, said, expected_dep[month])
    dep_total += said
assert depreciation_months == set(range(1, 13))
assert dep_total == sum(expected_dep.values())
assert dep_total == D("10178.42")
assert previous_house == D("10049.33") and previous_roof == D("129.09")
print("depreciation: building", building, "and roof", roof, "| 11 building months + 3 roof months", dep_total)

# ── management fees: 8% of the rent of the month before ─────────────────────────────────────
rent_by_month = {}
late = D(0)
kept = D(0)
for day, plan, amount, legs in occ:
    if plan == "rent-a":
        rent_by_month[int(day[5:7])] = D(2400)
    if plan == "rent-b":
        rent_by_month[int(day[5:7])] = D(2500)
for f in flows:
    if f.day.year == 2025 and f.src == "income/late-fees":
        late += f.into
        rent_by_month[f.day.month] += f.into
    if f.day.year == 2025 and f.src == "income/forfeited-deposits":
        kept += f.into
mgmt = D(0)
fees = [(day, amount) for day, plan, amount, legs in occ if plan == "manager-fee"]
for day, amount in fees:
    m = int(day[5:7])
    behind = 12 if day == "2025-12-29" else m - 1                     # paid the 5th for last month, and the last one at the sale
    want = cents(rent_by_month[behind] * D("0.08"))
    said = amount if amount is not None else D(192)
    assert said == want, (day, said, want)
    mgmt += said
print("management fees", mgmt, "in", len(fees), "payments")

# ── the rental's year ───────────────────────────────────────────────────────────────────────────
rent = sum(rent_by_month[m] for m in rent_by_month) - late
rental_income = rent + late + kept
by_place = {}
for f in flows:
    if f.day.year == 2025 and f.src == "rental-bank" and f.dst in ("insurance", "property-tax", "repairs", "advertising", "utilities"):
        by_place[f.dst] = by_place.get(f.dst, D(0)) + f.into
insurance, property_tax, repairs = by_place["insurance"], by_place["property-tax"], by_place["repairs"]
advertising, utilities = by_place["advertising"], by_place["utilities"]

# loan costs: paid on 2024-12-18 for the range 2024-12-18..2025-12-29, recognized a little each day
first, lastday = date(2024, 12, 18), date(2025, 12, 29)
days = (lastday - first).days + 1


def through(d):
    elapsed = min(max((d - first).days + 1, 0), days)
    return cents(D(3120) * elapsed / days)


amort_2024 = through(date(2024, 12, 31))
amort_2025 = through(date(2025, 12, 31)) - amort_2024
print("loan costs: range", days, "days; 2024", amort_2024, "2025", amort_2025)

expenses = {
    "interest": interest_2025, "depreciation": dep_total, "property tax": property_tax, "amortization": amort_2025,
    "repairs": repairs, "management": mgmt, "insurance": insurance, "utilities": utilities, "advertising": advertising,
}
rental_expenses = sum(expenses.values())
rental_net = rental_income - rental_expenses
assert rental_expenses == D("42065.18")
assert rental_net == D("-17340.18")
print("rental income", rental_income, "(rent", rent, "+ late fee", late, "+ kept deposit", kept, ")")
print("rental expenses", rental_expenses, expenses)
print("rental net", rental_net)

# ── the sale ────────────────────────────────────────────────────────────────────────────────────
price = D(431500)
costs = cents(price * D("0.05")) + cents(price * D("0.0125"))
amount_realized = price - costs
adjusted_basis = D(376850) + roof - dep_total
gain = amount_realized - adjusted_basis
recaptured = min(max(gain, D(0)), dep_total)
long_gain = gain - recaptured
assert adjusted_basis == D("380871.58")
assert gain == D("23659.67")
assert recaptured == D("10178.42") and long_gain == D("13481.25")
print("sale: costs", costs, "amount realized", amount_realized, "adjusted basis", adjusted_basis, "gain", gain,
      "| recapture (ordinary)", recaptured, "long-term", long_gain)
# the journal's closing statement is the price, with the seller's costs as a leg and the loan payoff in another
sale_legs = [f for f in flows if f.src == "house" and f.legs][0]
legs = {p: a for p, a, u in sale_legs.legs}
assert sale_legs.into == price and legs["selling-costs"] == costs and legs["mortgage"] == balance
# the interest to the day of payoff is a flow of its own: Schedule E interest, not a cost of the sale
payoff_interest = [f for f in flows if f.day.isoformat() == "2025-12-29" and f.dst == "interest"]
assert [f.into for f in payoff_interest] == [accrued]
paid_to_lender = payment * 11 + legs["mortgage"] + accrued

# ── the return ──────────────────────────────────────────────────────────────────────────────────
wages = sum((D(8000) for d, plan, a, l in occ if plan == "paycheck"), D(0))
withheld = sum((D(1215) for d, plan, a, l in occ if plan == "paycheck"), D(0))
allowance = D(25000)
schedule_e = max(rental_net, -allowance)
total_income = wages + schedule_e
ordinary_gain, long_term = recaptured, long_gain
agi = total_income + ordinary_gain + long_term                        # nothing to net: no loss
std = D(15750)
taxable = agi - std
single = [(0, "0.10"), (11925, "0.12"), (48475, "0.22"), (103350, "0.24"), (197300, "0.32"), (250525, "0.35"), (626350, "0.37")]
gains_rates = [(0, "0"), (48350, "0.15"), (533400, "0.20")]


def progressive(schedule, x):
    t = D(0)
    for i, (lo, r) in enumerate(schedule):
        hi = schedule[i + 1][0] if i + 1 < len(schedule) else None
        if x <= lo:
            break
        top = x if hi is None else min(x, D(hi))
        t += cents((top - D(lo)) * D(r))
    return t


ordinary_taxable = min(max(agi - long_term - std, D(0)), taxable)
income_tax = progressive(single, ordinary_taxable) + progressive(gains_rates, taxable) - progressive(gains_rates, ordinary_taxable)
nii = long_term + ordinary_gain + D(0)                                 # no interest or dividends; rent is left out, as us does
niit = cents(D("0.038") * min(nii, max(agi - D(200000), D(0))))
total_tax = income_tax + niit
owed = total_tax - withheld
print("wages", wages, "| passive loss allowed", schedule_e, "| total income", total_income)
print("agi", agi, "taxable", taxable, "ordinary part", ordinary_taxable, "income tax", income_tax, "niit", niit)
print("total tax", total_tax, "withheld", withheld, "owed (negative = refund)", owed)

# ── the cash at the end: every flow replayed ──────────────────────────────────────────────────────
cash = {"checking": D(128000)}


def add(place, amount):
    cash[place] = cash.get(place, D(0)) + amount


for day, plan, amount, legs in occ:
    m = int(day[5:7])
    if plan == "paycheck":
        add("checking", D(8000) - D(1215) - D(612))
    elif plan in ("rent-a", "rent-b"):
        add("rental-bank", D(2400) if plan == "rent-a" else D(2500))
    elif plan == "mortgage-payment":
        add("rental-bank", -payment)
    elif plan == "manager-fee":
        add("rental-bank", -(amount if amount is not None else D(192)))
for f in flows:
    if f.dst.endswith(".basis") or f.src.endswith(".basis"):        # basis flows move no cash
        continue
    if f.legs and f.src == "house":                                    # the sale: what is left after the legs
        rest = f.into - sum(a for p, a, u in f.legs if a is not None)
        add("rental-bank", rest)
        continue
    if f.out is None:
        unit = f.into_unit
        amount = cents(f.into * f.price[0]) if f.price else f.into
        if unit == "HOME":
            add(f.src, -amount)
            continue
        add(f.src, -amount)
        add(f.dst, amount)
    else:
        add(f.src, -f.out)
        add(f.dst, f.into)
print("cash: checking", cash["checking"], "rental-bank", cash["rental-bank"], "deposit-bank", cash.get("deposit-bank"))
print("net worth on 2026-04-16 (all of it cash)", cash["checking"] + cash["rental-bank"] + cash.get("deposit-bank", D(0)))
print("paid to the lender in 2025 (11 payments and the payoff)", paid_to_lender)
