"""Independent arithmetic check for the native v4 05-family project.

Reads the dated native flows and statements directly.  It does not import the
model parser or inspect any Axiom output.

Run from this directory with ``python3 verify05.py``.
"""
from datetime import date
from decimal import Decimal as D, ROUND_HALF_EVEN
import glob
import os
import re

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.join(HERE, "..", "05-family")
JOURNAL = os.path.join(ROOT, "journal", "2025")
NUMBER = r"([\d_]+(?:\.\d+)?)"
FLOW = re.compile(
    rf"^(2025-\d\d-\d\d)\s+(\S+)\s+->\s+(\S+)\s+{NUMBER}\s+USD(?:\s+(.*?))?\s*(?://(.*))?$"
)
STATEMENT = re.compile(
    rf"^(2025-\d\d-\d\d)\s+(\S+)\s*=\s*{NUMBER}\s+USD(?:\s+via\s+(\S+))?\s*(?://.*)?$"
)
OPENING = re.compile(
    rf"^\s+riley-529\s+{NUMBER}\s+USD\s+basis\s+{NUMBER}\s+USD\s+since\s+(\d{{4}}-\d\d-\d\d)"
)


def amount(text):
    return D(text.replace("_", ""))


def cents(value):
    return value.quantize(D("0.01"), rounding=ROUND_HALF_EVEN)


flows = []
statements = []
unparsed = []
for path in sorted(glob.glob(os.path.join(JOURNAL, "*.ax"))):
    pending_comment = ""
    with open(path, encoding="utf-8") as source:
        for line_no, raw in enumerate(source, 1):
            line = raw.strip()
            if line.startswith("//"):
                pending_comment += " " + line.lstrip("/").strip()
                continue
            flow = FLOW.match(line)
            statement = STATEMENT.match(line)
            if flow:
                day, src, dst, raw_amount, tail, comment = flow.groups()
                flows.append({
                    "day": date.fromisoformat(day), "src": src, "dst": dst,
                    "amount": amount(raw_amount), "tail": tail or "",
                    "comment": f"{pending_comment} {comment or ''}",
                    "order": (path, line_no),
                })
                pending_comment = ""
            elif statement:
                day, place, raw_amount, via = statement.groups()
                statements.append({
                    "day": date.fromisoformat(day), "place": place,
                    "amount": amount(raw_amount), "via": via,
                    "order": (path, line_no),
                })
                pending_comment = ""
            elif line and not line.startswith("//") and not line.startswith("opening "):
                unparsed.append(f"{path}:{line_no}: {line}")

if unparsed:
    raise SystemExit("unrecognized dated journal rows:\n" + "\n".join(unparsed))

def tagged(flow, purpose):
    return f"#{purpose}" in flow["tail"].split()


def total(predicate):
    return sum((f["amount"] for f in flows if predicate(f)), D(0))


# The displayed wages are gross flows from the two employers. The native journal
# records deductions as separate flows, with the purpose on each flow.
wages = total(lambda f: tagged(f, "wages"))
pretax = total(lambda f: tagged(f, "household-pre-tax") or tagged(f, "household-deferral"))
federal = total(lambda f: tagged(f, "federal-tax"))
state = total(lambda f: tagged(f, "state-tax") or tagged(f, "prior-year-state-tax"))
sdi = total(lambda f: tagged(f, "state-disability"))
interest = total(lambda f: tagged(f, "interest-income"))
mortgage_interest = total(lambda f: tagged(f, "interest") and "house" in f["tail"].split())
property_tax = total(lambda f: tagged(f, "property-tax"))
charity = total(lambda f: tagged(f, "charity"))

# Read the opening 529 value and basis from the opening statement, then replay
# contributions, market revaluations, and withdrawals in source order.
opening_line = None
with open(os.path.join(ROOT, "journal", "2024", "12.ax"), encoding="utf-8") as source:
    for raw in source:
        found = OPENING.match(raw)
        if found:
            opening_line = found
            break
if opening_line is None:
    raise SystemExit("opening statement must provide the 529 value, basis, and since date")
initial_value, basis, opened = opening_line.groups()
value = amount(initial_value)
basis = amount(basis)
if opened != "2019-01-01":
    raise SystemExit(f"unexpected 529 opening date: {opened}")

events = []
contributions = D(0)
hsa_reimbursed = D(0)
for flow in flows:
    if flow["dst"] == "riley-529":
        events.append((flow["day"], flow["order"], "in", flow["amount"], None))
        if tagged(flow, "contribution"):
            contributions += flow["amount"]
    if flow["src"] == "riley-529":
        events.append((flow["day"], flow["order"], "out", flow["amount"], flow["dst"]))
    if flow["src"] == "hsa" and "!" in flow["tail"].split():
        hsa_reimbursed += flow["amount"]
for statement in statements:
    if statement["place"] == "riley-529" and statement["via"] == "market":
        events.append((statement["day"], statement["order"], "mark", statement["amount"], None))

earnings = D(0)
withdrawals = []
for day, order, kind, value_or_amount, destination in sorted(events, key=lambda e: (e[0], e[1])):
    if kind == "in":
        value += value_or_amount
        basis += value_or_amount
    elif kind == "mark":
        value = value_or_amount
    else:
        withdrawal = value_or_amount
        released_basis = basis * withdrawal / value
        gain = withdrawal - released_basis
        basis -= released_basis
        value -= withdrawal
        withdrawals.append((day, destination, withdrawal, cents(gain)))
        if destination != "st-annes-school":
            earnings += gain
earnings = cents(earnings)
distributions = earnings + hsa_reimbursed

salt = state + sdi + property_tax
itemized = mortgage_interest + charity + min(salt, D(40_000))
standard = D(31_500)
deduction = max(standard, itemized)
total_income = wages - pretax + interest + distributions
agi = total_income
taxable = agi - deduction

def progressive(schedule, taxable_income):
    result = D(0)
    for i, (lower, rate) in enumerate(schedule):
        upper = schedule[i + 1][0] if i + 1 < len(schedule) else None
        if taxable_income <= lower:
            break
        top = taxable_income if upper is None else min(taxable_income, D(upper))
        result += cents((top - D(lower)) * D(rate))
    return result


joint = [(0, "0.10"), (23_850, "0.12"), (96_950, "0.22"),
         (206_700, "0.24"), (394_600, "0.32"), (501_050, "0.35"),
         (751_600, "0.37")]
income_tax = progressive(joint, taxable)
credit = D(2_200)  # 2025 child credit for the one child declared in axiom.ax.
total_tax = income_tax - credit
owed = total_tax - federal

ca_schedule = [(0, "0.01"), (22_158, "0.02"), (52_528, "0.04"),
               (82_904, "0.06"), (115_084, "0.08"), (145_448, "0.093"),
               (742_958, "0.103")]
ca_taxable = agi - D(11_412)
ca_tax = progressive(ca_schedule, ca_taxable)
state_prior_payment = total(lambda f: tagged(f, "prior-year-state-tax"))
state_withholding = state - state_prior_payment
ca_owed = ca_tax - state_withholding

alex_deferral = total(lambda f: f["dst"] == "alex-401k" and tagged(f, "household-deferral"))
jordan_deferral = total(lambda f: f["dst"] == "jordan-401k" and tagged(f, "household-deferral"))
hsa_payroll = total(lambda f: f["dst"] == "hsa" and tagged(f, "household-pre-tax"))
dcfsa_payroll = total(lambda f: f["dst"] == "dcfsa" and tagged(f, "household-pre-tax"))

# These journal rhythms were the source of the old forecast's standing monthly
# transfers. Keep them in history rather than inventing counterparty contracts.
monthly_529 = [f for f in flows if f["src"] == "joint-checking" and
               f["dst"] == "riley-529" and tagged(f, "contribution")]
monthly_savings = [f for f in flows if f["src"] == "joint-checking" and
                   f["dst"] == "joint-savings"]
for label, rows, expected_amount, expected_day in (
    ("529 transfer", monthly_529, D(250), 8),
    ("savings transfer", monthly_savings, D(2_800), 10),
):
    if (len(rows) != 12 or [r["day"].month for r in rows] != list(range(1, 13)) or
            any(r["day"].day != expected_day or r["amount"] != expected_amount for r in rows)):
        raise AssertionError(f"{label}: monthly journal rhythm changed")

# Reconcile the independently recomputed values against the long-standing hand
# oracle. These assertions intentionally make source changes fail loudly.
expected = {
    "wages": D("247440.07"), "pretax": D("36946.36"),
    "interest": D("2479.02"), "distributions": D("1012.94"),
    "total income": D("213985.67"), "mortgage interest": D("24077.32"),
    "property tax": D("6480.00"), "charity": D("3900.00"),
    "federal withholding": D("27594.00"), "state withholding": D("11239.60"),
    "state prior payment": D("412.00"),
    "state disability": D("2776.15"), "HSA reimbursement": D("620.00"),
    "529 contributions": D("6000.00"), "Alex 401(k) deferral": D("15000.00"),
    "Jordan 401(k) deferral": D("5846.36"), "HSA payroll": D("6000.00"),
    "FSA payroll": D("5000.00"),
}
actual = {
    "wages": wages, "pretax": pretax, "interest": interest,
    "distributions": distributions, "total income": total_income,
    "mortgage interest": mortgage_interest, "property tax": property_tax,
    "charity": charity, "federal withholding": federal,
    "state withholding": state_withholding, "state prior payment": state_prior_payment,
    "state disability": sdi,
    "HSA reimbursement": hsa_reimbursed, "529 contributions": contributions,
    "Alex 401(k) deferral": alex_deferral, "Jordan 401(k) deferral": jordan_deferral,
    "HSA payroll": hsa_payroll, "FSA payroll": dcfsa_payroll,
}
for name, want in expected.items():
    got = cents(actual[name])
    if got != want:
        raise AssertionError(f"{name}: native source gives {got}, expected {want}")

if cents(earnings) != D("392.94"):
    raise AssertionError(f"nonqualified 529 earnings: {cents(earnings)}, expected 392.94")
if cents(itemized) != D("48885.07") or cents(taxable) != D("165100.60"):
    raise AssertionError(f"itemized/taxable mismatch: {cents(itemized)} / {cents(taxable)}")
if cents(income_tax) != D("26150.13") or cents(total_tax) != D("23950.13"):
    raise AssertionError(f"federal tax mismatch: {cents(income_tax)} / {cents(total_tax)}")
if cents(owed) != D("-3643.87") or cents(ca_owed) != D("477.03"):
    raise AssertionError(f"return balance mismatch: federal {cents(owed)}, California {cents(ca_owed)}")

print("native 05-family arithmetic: PASS")
print("wages", wages, "pretax", pretax, "interest", interest)
print("529 withdrawals (date, to, amount, taxable gain):", withdrawals)
print("distributions", distributions, "(529 taxable earnings", earnings,
      "+ HSA reimbursement", hsa_reimbursed, ")")
print("total income / AGI", total_income)
print("itemized", itemized, "= mortgage interest", mortgage_interest,
      "+ SALT", min(salt, D(40_000)), "(state", state, "SDI", sdi,
      "property tax", property_tax, ") + charity", charity)
print("taxable", taxable, "income tax", income_tax, "credit", credit,
      "total tax", total_tax, "federal payments", federal, "owed", owed)
print("California taxable", ca_taxable, "tax", ca_tax, "withheld", state - D(412),
      "owed", ca_owed)
print("history-derived monthly transfers: 529", len(monthly_529), "× 250 USD; savings",
      len(monthly_savings), "× 2,800 USD")
print("limits: Alex 401(k)", alex_deferral, "of 23,500 ->", D(23_500) - alex_deferral,
      "| Jordan", jordan_deferral, "of 23,500 ->", D(23_500) - jordan_deferral,
      "| HSA", hsa_payroll + D(1_000), "of 8,550 ->", D(8_550) - hsa_payroll - D(1_000),
      "| DCFSA", dcfsa_payroll, "| 529 contributions", contributions,
      "of 19,000 ->", D(19_000) - contributions)
