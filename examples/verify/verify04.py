"""Independent v4-source check for examples/04-freelancer.

This reads the typed flow, claim, measure, price, and `for YEAR` records from
source text directly. It does not use Axiom parsing, engine output, or saved
report snapshots. Arithmetic and the expected 2025 return are checked below.
"""
from datetime import date, timedelta
from decimal import Decimal as D, ROUND_HALF_EVEN
import glob
import os
import re

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.join(HERE, "..", "04-freelancer")
YEAR = 2025
CLIENTS = {"brightwave", "fernhill", "orbit-labs", "delta-rugs", "northpeak"}
BUSINESS_PURPOSES = {
    "business-software", "business-insurance", "business-equipment",
    "business-education", "business-travel", "business-printing",
    "business-fonts", "business-hosting", "business-fees",
}

def cents(x):
    return x.quantize(D("0.01"), rounding=ROUND_HALF_EVEN)

def amount(text):
    m = re.match(r"^\s*([0-9][0-9_,]*(?:\.\d+)?)\s+([A-Z][A-Z0-9._]*)", text)
    if not m:
        return None
    return D(m.group(1).replace("_", "").replace(",", "")), m.group(2), text[m.end():]

def span_recognized(value, start, end, year):
    if end is None:
        return value if start.year == year else D(0)
    days = (end - start).days + 1
    def through(day):
        elapsed = min(max((day - start).days + 1, 0), days)
        return cents(value * D(elapsed) / D(days))
    return through(date(year, 12, 31)) - through(date(year, 1, 1) - timedelta(days=1))

def source_files():
    return sorted(glob.glob(os.path.join(ROOT, "journal", "**", "*.ax"), recursive=True))

def date_prefix(line):
    m = re.match(r"^(\d{4}-\d\d-\d\d)(?:\.\.(\d{4}-\d\d-\d\d))?\s+(.*)$", line)
    if not m:
        return None
    start = date.fromisoformat(m.group(1))
    end = date.fromisoformat(m.group(2)) if m.group(2) else None
    return start, end, m.group(3)

# Read the declared daily mileage rates independently from the price file.
prices = []
for path in sorted(glob.glob(os.path.join(ROOT, "prices", "*.ax"))):
    for raw in open(path, encoding="utf-8"):
        m = re.match(r"^(\d{4}-\d\d-\d\d)\s+MI\s*=\s*([0-9.]+)\s+USD", raw)
        if m:
            prices.append((date.fromisoformat(m.group(1)), D(m.group(2))))
prices.sort()
def mileage_rate(day):
    prior = [rate for since, rate in prices if since <= day]
    if not prior:
        raise AssertionError(f"no MI rate on {day}")
    return prior[-1]

receipts = D(0)
expenses = D(0)
health = D(0)
interest = D(0)
sep = D(0)
paid = D(0)
payments_n = 0
fee_legs = D(0)
miles = D(0)
for path in source_files():
    lines = open(path, encoding="utf-8").read().splitlines()
    i = 0
    while i < len(lines):
        raw = lines[i]
        i += 1
        if not raw.strip() or raw.lstrip().startswith("//"):
            continue
        line = re.sub(r"\s//.*$", "", raw)
        parsed = date_prefix(line)
        if parsed is None:
            continue
        day, until, body = parsed
        explicit_year = re.search(r"for\s+(20\d\d)\b", body)
        year = int(explicit_year.group(1)) if explicit_year else day.year
        # A use measure is priced with the rate in force on its date.
        measure = re.match(r"^car\s+used\s+([0-9][0-9_,]*(?:\.\d+)?)\s+MI\s+#business-mile\b", body)
        if measure:
            quantity = D(measure.group(1).replace("_", "").replace(",", ""))
            miles += quantity
            if day.year == YEAR:
                expenses += cents(quantity * mileage_rate(day))
            continue
        # A code waiver closes an unpaid claim; it creates neither cash nor a
        # cash-method deduction.
        if re.match(r"^\^\S+\s+waived\b", body) or " owes me " in body:
            continue
        # Native grouped receipt: party -> gross amount, followed by legs.
        grouped = re.match(r"^(\S+)\s+->\s+([0-9][0-9_,]*(?:\.\d+)?)\s+USD\b(.*)$", body)
        if grouped:
            source, raw_amount, tail = grouped.groups()
            gross = D(raw_amount.replace("_", "").replace(",", ""))
            children = []
            while i < len(lines) and lines[i].startswith("  ") and lines[i].strip() and not lines[i].lstrip().startswith("//"):
                child = re.sub(r"\s//.*$", "", lines[i]).strip()
                i += 1
                children.append(child)
            if source in CLIENTS and year == YEAR:
                receipts += gross
                payments_n += 1
            for child in children:
                m = re.match(r"^(\S+)\s+([0-9][0-9_,]*(?:\.\d+)?)\s+USD\b(.*)$", child)
                if not m:
                    raise AssertionError(f"unreadable grouped leg in {path}: {child}")
                _, raw_leg, leg_tail = m.groups()
                if "#business-fees" in leg_tail and year == YEAR:
                    amount_leg = D(raw_leg.replace("_", "").replace(",", ""))
                    expenses += amount_leg
                    fee_legs += amount_leg
            continue
        # Client settlement with a source amount before the arrow.
        direct = re.match(r"^(\S+)\s+([0-9][0-9_,]*(?:\.\d+)?)\s+USD\s+->\s+(\S+)(.*)$", body)
        if direct:
            source, raw_amount, target, tail = direct.groups()
            value = D(raw_amount.replace("_", "").replace(",", ""))
            if source in CLIENTS and "#design" in tail and year == YEAR:
                receipts += value
                payments_n += 1
                continue
            if target == "irs" and "#federal-tax" in tail and year == YEAR:
                paid += value
                continue
        flow = re.match(r"^(\S+)\s+->\s+(\S+)\s+([0-9][0-9_,]*(?:\.\d+)?)\s+([A-Z][A-Z0-9._]*)(.*)$", body)
        if not flow:
            continue
        source, target, raw_amount, unit, tail = flow.groups()
        value = D(raw_amount.replace("_", "").replace(",", ""))
        if target == "irs" and "#federal-tax" in tail and year == YEAR:
            paid += value
            continue
        if target == "sep" and source == "business-checking" and year == YEAR:
            sep += value
            continue
        if source == "interest-source" and target == "tax-vault" and year == YEAR:
            interest += value
            continue
        purpose = re.search(r"#([a-z0-9-]+)", tail)
        purpose = purpose.group(1) if purpose else ""
        if purpose == "health-premium":
            if year == YEAR:
                health += value
            continue
        recognized = (value if year == YEAR else D(0)) if explicit_year and until is None else span_recognized(value, day, until, YEAR)
        if purpose in BUSINESS_PURPOSES:
            expenses += recognized
        elif purpose == "business-meal":
            expenses += cents(recognized * D("0.5"))
        elif purpose == "home-office-cost":
            expenses += cents(recognized * D("0.15"))
        elif purpose == "business-phone":
            expenses += cents(recognized * D("0.60"))

net = receipts - expenses
earnings = cents(net * D("0.9235"))
se_tax = cents(min(earnings, D(176100)) * D("0.124")) + cents(earnings * D("0.029"))
half_se = cents(se_tax * D("0.5"))
adjustments = half_se + health + sep
total_income = net + interest
agi = total_income - adjustments
standard_deduction = D(15750)
attributable = net - half_se - health - sep
before_qbi = agi - standard_deduction
qbi = cents(min(max(attributable, D(0)), before_qbi) * D("0.2"))
deductions = standard_deduction + qbi
taxable = agi - deductions
brackets = [(0, D("0.10")), (11925, D("0.12")), (48475, D("0.22")), (103350, D("0.24"))]
def progressive(value):
    result = D(0)
    for i, (floor, rate) in enumerate(brackets):
        if value <= floor:
            break
        ceiling = value if i + 1 == len(brackets) else min(value, D(brackets[i + 1][0]))
        result += cents((ceiling - D(floor)) * rate)
    return result
income_tax = progressive(taxable)
total_tax = income_tax + se_tax
owed = total_tax - paid
sep_cap = cents(min(cents((net - half_se) * D("0.20")), D(70000)))

expected = {
    "gross receipts": D("74800"),
    "business expenses": D("13471.01"),
    "net profit": D("61328.99"),
    "health premiums": D("4944"),
    "interest": D("290.12"),
    "sep contributions": D("8000"),
    "SE tax": D("8665.51"),
    "adjustments": D("17276.76"),
    "AGI": D("44342.35"),
    "QBI deduction": D("5718.47"),
    "taxable income": D("22873.88"),
    "income tax": D("2506.37"),
    "payments": D("11100"),
    "owed": D("71.88"),
}
actual = {
    "gross receipts": receipts,
    "business expenses": expenses,
    "net profit": net,
    "health premiums": health,
    "interest": interest,
    "sep contributions": sep,
    "SE tax": se_tax,
    "adjustments": adjustments,
    "AGI": agi,
    "QBI deduction": qbi,
    "taxable income": taxable,
    "income tax": income_tax,
    "payments": paid,
    "owed": owed,
}
for key, wanted in expected.items():
    if actual[key] != wanted:
        raise SystemExit(f"{key}: {actual[key]} != independently retained v3 oracle {wanted}")
print(f"PASS v4 source oracle: {payments_n} client receipts, {miles} business miles, fee legs {fee_legs}")
for key, value in actual.items():
    print(f"{key}: {value}")
print(f"SEP room: {sep_cap - sep}")
