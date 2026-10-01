"""Independent oracle for the native v4 07-landlord source.

The expected figures are recomputed from contract terms, the identified house
and the dated journal flows. No Axiom model/engine code is imported. This checks
inputs and arithmetic; it does not claim that the current runtime implements all
of the native contract, claim, asset-sale, or tax behavior yet.
"""
import glob
import os
import re
from dataclasses import dataclass
from datetime import date, timedelta
from decimal import Decimal as D, ROUND_HALF_EVEN

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.join(HERE, "..", "07-landlord")


def cents(x):
    return x.quantize(D("0.01"), rounding=ROUND_HALF_EVEN)


def money(s):
    return D(s.replace("_", ""))


def read_num(pattern, text, label):
    match = re.search(pattern, text, re.M)
    assert match, f"missing {label} in source"
    return match


@dataclass
class Flow:
    day: date
    until: date | None
    src: str
    dst: str
    amount: D
    purpose: str | None
    code: str | None
    raw: str


@dataclass
class Occurrence:
    day: date
    name: str
    amount: D | None


def read_source():
    contracts = open(os.path.join(ROOT, "contracts.ax")).read()
    assets = open(os.path.join(ROOT, "assets.ax")).read()
    flows, occurrences, statements = [], [], []
    for path in sorted(glob.glob(os.path.join(ROOT, "journal", "**", "*.ax"), recursive=True)):
        sale_header = None
        for raw in open(path):
            line = re.sub(r"\s//.*$", "", raw).strip()
            if not line or line.startswith("//") or line.startswith("opening"):
                continue
            item = re.match(r"^- ([\d_,.]+) (USD|%) #([\w-]+)(?: of house)?$", line)
            if item:
                assert sale_header is not None, f"orphaned sale line item: {line}"
                stated = money(item.group(1))
                item_amount = stated if item.group(2) == "USD" else cents(sale_header.amount * stated / 100)
                flows.append(Flow(
                    sale_header.day, None, sale_header.dst, sale_header.src,
                    item_amount, item.group(3), None, line,
                ))
                sale_header = None
                continue
            occ = re.match(r"^(\d{4}-\d\d-\d\d) (paycheck|rent-a|rent-b|home-loan|manager-fee)(?:\s+([\d_,.]+) USD)?$", line)
            if occ:
                sale_header = None
                occurrences.append(Occurrence(date.fromisoformat(occ.group(1)), occ.group(2), money(occ.group(3)) if occ.group(3) else None))
                continue
            stmt = re.match(r"^(\d{4}-\d\d-\d\d) (checking|rental-bank|deposit-bank|mortgage|bills|deposits) = (empty|[\d_,.]+ USD)$", line)
            if stmt:
                sale_header = None
                value = D(0) if stmt.group(3) == "empty" else money(stmt.group(3).split()[0])
                statements.append((date.fromisoformat(stmt.group(1)), stmt.group(2), value))
                continue
            m = re.match(r"^(\d{4}-\d\d-\d\d)(?:\.\.(\d{4}-\d\d-\d\d))? (\S+) -> (\S+) ([\d_,.]+) USD(.*)$", line)
            if m:
                day, until, src, dst, amt, tail = m.groups()
                purpose = re.search(r"#([\w-]+)", tail)
                code = re.search(r"\^([\w-]+)", tail)
                flow = Flow(date.fromisoformat(day), date.fromisoformat(until) if until else None, src, dst, money(amt), purpose.group(1) if purpose else None, code.group(1) if code else None, line)
                flows.append(flow)
                sale_header = flow if flow.purpose == "sale" else None
                continue
            sale_header = None
    return contracts, assets, flows, occurrences, statements


contracts, assets, flows, occ, statements = read_source()
tax_source = open(os.path.join(ROOT, "tax.ax")).read()
all_journal = "\n".join(open(p).read() for p in sorted(glob.glob(os.path.join(ROOT, "journal", "**", "*.ax"), recursive=True)))

# Contract terms and asset facts come from current source rather than duplicated
# hard-coded fixture constants.
loan = read_num(r"loan\s+([\d_,.]+) USD on (\d{4}-\d\d-\d\d) at ([\d.]+)% over (\d+)y for house", contracts, "mortgage terms")
LOAN = money(loan.group(1))
loan_start = date.fromisoformat(loan.group(2))
rate = D(loan.group(3)) / 100 / 12
months = int(loan.group(4)) * 12
payment = cents(LOAN * rate / (1 - (1 + rate) ** -months))

purchase = read_num(r"(?m)^(\d{4}-\d\d-\d\d) rental-bank -> title-co ([\d_,.]+) USD #purchase of house", "\n".join(f.raw for f in flows), "house purchase")
purchase_day = date.fromisoformat(purchase.group(1))
COST = money(purchase.group(2))
land = read_num(r"land\s+([\d_,.]+) USD", assets, "land value")
LAND = money(land.group(1))
BUILDING = cents(COST - LAND)
service = date.fromisoformat(read_num(r"in-service (\d{4}-\d\d-\d\d)", assets, "in-service date").group(1))
roof_invoice = read_num(r"(?m)^(\d{4}-\d\d-\d\d) me owes summit-roofing due 30d \^inv-roof\s*$", all_journal, "roof invoice")
roof_start = date.fromisoformat(roof_invoice.group(1))
roof_rows = re.findall(r"(?m)^\s+([\d_,.]+) USD #improvement of house", all_journal)
assert len(roof_rows) == 1, roof_rows
ROOF = money(roof_rows[0])
sale_line = read_num(r"(?m)^(\d{4}-\d\d-\d\d) buyer -> rental-bank ([\d_,.]+) USD #sale of house", all_journal, "sale flow")
sale_day = date.fromisoformat(sale_line.group(1))
lease_b_end = date.fromisoformat(read_num(
    r"contract rent-b with tenant-b[\s\S]*?until (\d{4}-\d\d-\d\d)",
    contracts, "lease B end",
).group(1))
assert lease_b_end == sale_day, ("Jamie must stop earning rent when the property and lease transfer", lease_b_end, sale_day)

# ── Mortgage amortization: exact monthly interest rounds at the due date. ──
balance = LOAN
schedule = {}
interest_2025 = D(0)
for month in range(2, 13):
    interest = cents(balance * rate)
    principal = payment - interest
    schedule[month] = (principal, interest)
    interest_2025 += interest
    balance -= principal
accrued = cents(balance * rate * D(28) / D(30))
interest_2025 += accrued
assert payment == D("1809.59")
assert cents(balance) == D("276282.05")
loan_occ = [o for o in occ if o.name == "home-loan" and o.day.year == 2025]
assert len(loan_occ) == 11
for o in loan_occ:
    principal, interest = schedule[o.day.month]
    assert (principal + interest) == payment, (o.day, principal, interest)
print("payment", payment, "| interest paid Feb-Dec", interest_2025 - accrued, "| accrued at payoff", accrued, "| payoff principal", balance)

# ── Depreciation: global cumulative recovery at each period boundary. ────────
life_months = D(27.5) * 12
def cumulative_months(first_service, disposed, month):
    first = first_service.year * 12 + first_service.month
    end = disposed.year * 12 + disposed.month
    current = disposed.year * 12 + month
    if current < first:
        return D(0)
    if current > end:
        current = end
    if current == end:
        return D("0.5") if current == first else D(current - first)
    return D("0.5") + D(current - first)

building_edges = {m: cumulative_months(service, sale_day, m) for m in range(1, 13)}
roof_edges = {m: cumulative_months(roof_start, sale_day, m) for m in range(1, 13)}

def recovered(cost, service_months):
    return cents(cost * service_months / life_months)

previous_building = previous_roof = D(0)
month_dep = {}
for month in range(1, 13):
    building_total = recovered(BUILDING, building_edges[month])
    roof_total = recovered(ROOF, roof_edges[month]) if month in roof_edges else previous_roof
    month_dep[month] = building_total - previous_building + roof_total - previous_roof
    previous_building, previous_roof = building_total, roof_total

dep_total = sum(month_dep.values(), D(0))
assert dep_total == D("10178.42")
assert previous_building == D("10049.33") and previous_roof == D("129.09")
print("depreciation: building", BUILDING, "and roof", ROOF, "| 11 building months + 3 roof months", dep_total)

# ── Rents and management: terms and occurrence deviations are sourced above. ─
rent_a = money(read_num(r"contract rent-a with tenant-a\s+([\d_,.]+) USD monthly on 1", contracts, "lease A rent").group(1))
rent_b = money(read_num(r"contract rent-b with tenant-b\s+([\d_,.]+) USD monthly on 1", contracts, "lease B rent").group(1))
deposit_a = money(read_num(r"contract rent-a with tenant-a[\s\S]*?deposit ([\d_,.]+) USD", contracts, "lease A deposit").group(1))
deposit_b = money(read_num(r"contract rent-b with tenant-b[\s\S]*?deposit ([\d_,.]+) USD", contracts, "lease B deposit").group(1))
manager_percent = D(read_num(r"The manager's ([\d.]+)% fee", contracts, "manager fee rate").group(1)) / 100
manager_default = money(read_num(r"about ([\d_,.]+) USD monthly on 5 from rental-bank #management", contracts, "manager fee default").group(1))
rent_by_month = {}
for o in occ:
    if o.name == "rent-a": rent_by_month[o.day.month] = rent_a
    if o.name == "rent-b": rent_by_month[o.day.month] = rent_b
assert set(rent_by_month) == {2, 3, 4, 5, 6, 7, 8, 10, 11, 12}
late = sum((f.amount for f in flows if f.purpose == "late-fees"), D(0))
kept = sum((f.amount for f in flows if f.purpose == "forfeited-deposit"), D(0))
rent_received = sum(rent_by_month.values(), D(0))
rental_income = rent_received + late + kept
assert (rent_received, late, kept, rental_income) == (D("24300"), D("75"), D("350"), D("24725"))
manager_terms = [o for o in occ if o.name == "manager-fee"]
rent_for_fee = dict(rent_by_month)
for f in flows:
    if f.purpose == "late-fees":
        rent_for_fee[f.day.month] = rent_for_fee.get(f.day.month, D(0)) + f.amount
mgmt = sum((o.amount if o.amount is not None else manager_default for o in manager_terms), D(0))
assert len(manager_terms) == 10 and mgmt == D("1950")
for o in manager_terms:
    behind = 12 if o.day == date(2025, 12, 29) else o.day.month - 1
    actual = o.amount if o.amount is not None else manager_default
    assert actual == cents(rent_for_fee[behind] * manager_percent), (o.day, actual, rent_for_fee[behind])

# ── Schedule E costs, each read by purpose from native journal events. ───────
by_purpose = {}
for f in flows:
    if f.day.year == 2025:
        by_purpose[f.purpose] = by_purpose.get(f.purpose, D(0)) + f.amount
insurance = by_purpose.get("insurance", D(0))
property_tax = by_purpose.get("property-tax", D(0))
repairs = by_purpose.get("repair", D(0))
advertising = by_purpose.get("advertising", D(0))
utilities = by_purpose.get("rental-utilities", D(0))
# Loan costs are recognized over the original dated range. The native input is a
# dated flow; period allocation is independently computed at calendar boundaries.
loan_cost = next(f for f in flows if f.purpose == "loan-cost")
first, last = loan_cost.day, loan_cost.until
assert first == date(2024, 12, 18) and last == date(2025, 12, 29)
days = D((last - first).days + 1)
def through(d):
    elapsed = min(max((d - first).days + 1, 0), int(days))
    return cents(loan_cost.amount * elapsed / days)
amortization_2025 = through(date(2025, 12, 31)) - through(date(2024, 12, 31))
expenses = {"interest": interest_2025, "depreciation": dep_total, "property tax": property_tax,
            "amortization": amortization_2025, "repairs": repairs, "management": mgmt,
            "insurance": insurance, "utilities": utilities, "advertising": advertising}
rental_expenses = sum(expenses.values(), D(0))
rental_net = rental_income - rental_expenses
assert (insurance, property_tax, repairs, advertising, utilities) == (D("1560"), D("4380"), D("2051"), D("149"), D("154.60"))
assert amortization_2025 == D("3004.14")
assert rental_expenses == D("42065.18") and rental_net == D("-17340.18")
print("rental income", rental_income, "| rental expenses", rental_expenses, expenses, "| net", rental_net)

# ── Sale and tax arithmetic. ─────────────────────────────────────────────────
price = next(f.amount for f in flows if f.purpose == "sale" and f.dst == "rental-bank")
costs = sum((f.amount for f in flows if f.purpose == "selling-cost"), D(0))
payoff = next(f.amount for f in flows if f.code == "loan" and f.day == sale_day)
assert price == D("431500") and costs == D("26968.75") and payoff == D("276282.05")
amount_realized = price - costs
adjusted_basis = COST + ROOF - dep_total
gain = amount_realized - adjusted_basis
recaptured = min(max(gain, D(0)), dep_total)
long_gain = gain - recaptured
assert (amount_realized, adjusted_basis, gain, recaptured, long_gain) == (D("404531.25"), D("380871.58"), D("23659.67"), D("10178.42"), D("13481.25"))
payoff_interest = [f.amount for f in flows if f.day == sale_day and f.purpose == "interest"]
assert payoff_interest == [accrued]
print("sale: costs", costs, "amount realized", amount_realized, "adjusted basis", adjusted_basis,
      "gain", gain, "| recapture", recaptured, "long-term", long_gain)

# Tax formulas retain the independent 2025 table used by the original example.
paycheck_section = contracts.split("contract paycheck with acme", 1)[1].split("contract rent-a with tenant-a", 1)[0]
gross_pay = money(read_num(r"([\d_,.]+) USD monthly on 28 into checking", paycheck_section, "gross wages").group(1))
federal_withholding = money(read_num(r"irs ([\d_,.]+) USD #federal-tax", paycheck_section, "federal withholding").group(1))
payroll_withholding = money(read_num(r"payroll-office ([\d_,.]+) USD #tax-paid", paycheck_section, "payroll withholding").group(1))
wages = sum((gross_pay for o in occ if o.name == "paycheck"), D(0))
withheld = sum((federal_withholding for o in occ if o.name == "paycheck"), D(0))
allowance = money(read_num(r"2025 ([\d_,.]+) USD", tax_source, "passive allowance").group(1))
schedule_e = max(rental_net, -allowance)
total_income = wages + schedule_e
agi = total_income + recaptured + long_gain
std = D(15750)
taxable = agi - std
single = [(0, "0.10"), (11925, "0.12"), (48475, "0.22"), (103350, "0.24"), (197300, "0.32"), (250525, "0.35"), (626350, "0.37")]
gains_rates = [(0, "0"), (48350, "0.15"), (533400, "0.20")]
def progressive(schedule, x):
    total = D(0)
    for i, (lo, rate) in enumerate(schedule):
        hi = schedule[i + 1][0] if i + 1 < len(schedule) else None
        if x <= lo: break
        top = x if hi is None else min(x, D(hi))
        total += cents((top - D(lo)) * D(rate))
    return total
ordinary_taxable = min(max(agi - long_gain - std, D(0)), taxable)
income_tax = progressive(single, ordinary_taxable) + progressive(gains_rates, taxable) - progressive(gains_rates, ordinary_taxable)
niit = cents(D("0.038") * min(long_gain + recaptured, max(agi - D(200000), D(0))))
total_tax = income_tax + niit
owed = total_tax - withheld
assert (wages, total_income, agi, taxable, income_tax, niit, withheld, owed) == (D("96000"), D("78659.82"), D("102319.49"), D("86569.49"), D("13015.59"), D("0.00"), D("14580"), D("-1564.41"))
print("wages", wages, "| Schedule E", schedule_e, "| total income", total_income)
print("AGI", agi, "taxable", taxable, "tax", total_tax, "withheld", withheld, "owed (negative = refund)", owed)

# ── Reconcile every dated bank, mortgage, bill and deposit statement. ───────
cash = {"checking": D(128000), "rental-bank": D(0), "deposit-bank": D(0)}
claims = {"mortgage": D(0), "bills": D(0), "deposits": D(0)}
def add(place, amount):
    cash[place] = cash.get(place, D(0)) + amount

events = []
for o in occ:
    events.append((o.day, "occurrence", o))
for f in flows:
    events.append((f.day, "flow", f))
lease_a_start = date.fromisoformat(read_num(r"contract rent-a with tenant-a[\s\S]*?from (\d{4}-\d\d-\d\d)", contracts, "lease A start").group(1))
lease_b_start = date.fromisoformat(read_num(r"contract rent-b with tenant-b[\s\S]*?from (\d{4}-\d\d-\d\d)", contracts, "lease B start").group(1))
events.extend([(lease_a_start, "deposit", deposit_a), (lease_b_start, "deposit", deposit_b),
               (roof_start, "invoice", ROOF)])
for day, place, expected in statements:
    events.append((day, "statement", (place, expected)))
events.sort(key=lambda event: (event[0], {"occurrence": 0, "deposit": 1, "invoice": 2, "flow": 3, "statement": 4}[event[1]]))

for day, kind, item in events:
    if kind == "occurrence":
        o = item
        if o.name == "paycheck":
            add("checking", gross_pay - federal_withholding - payroll_withholding)
        elif o.name == "rent-a":
            add("rental-bank", rent_a)
        elif o.name == "rent-b":
            add("rental-bank", rent_b)
        elif o.name == "home-loan":
            if o.day == loan_start:
                add("rental-bank", LOAN)
                claims["mortgage"] += LOAN
            else:
                add("rental-bank", -payment)
                claims["mortgage"] -= schedule[o.day.month][0]
        elif o.name == "manager-fee":
            add("rental-bank", -(o.amount if o.amount is not None else manager_default))
    elif kind == "deposit":
        add("deposit-bank", item)
        claims["deposits"] += item
    elif kind == "invoice":
        claims["bills"] += item
    elif kind == "flow":
        f = item
        if f.src in cash: add(f.src, -f.amount)
        if f.dst in cash: add(f.dst, f.amount)
        if f.code == "inv-roof" and f.day > roof_start:
            claims["bills"] -= f.amount
        if f.code == "loan":
            claims["mortgage"] -= f.amount
        if f.purpose == "deposit-return": claims["deposits"] -= f.amount
        if f.purpose == "forfeited-deposit": claims["deposits"] -= f.amount
        if f.purpose == "deposit" and f.src == "rental-bank": claims["deposits"] -= f.amount
    else:
        place, expected = item
        actual = cash[place] if place in cash else claims[place]
        assert actual == expected, (day, place, actual, expected)

assert cash["checking"] == D("173866.60"), cash
assert cash["rental-bank"] == D("0.00"), cash
assert cash["deposit-bank"] == D("0.00"), cash
assert claims == {"mortgage": D(0), "bills": D(0), "deposits": D(0)}, claims
print("cash: checking", cash["checking"], "rental-bank", cash["rental-bank"], "deposit-bank", cash["deposit-bank"])
print("net worth on 2026-04-16 (all of it cash)", cash["checking"] + cash["rental-bank"] + cash["deposit-bank"])
print("paid to lender in 2025", payment * 11 + payoff + accrued)
