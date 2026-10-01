"""Independent arithmetic for examples/11-sam.

This oracle reads only the inputs named in the example and uses Decimal
arithmetic.  It does not call Axiom or copy a report.  The loan calculation
states its convention explicitly: annual rate divided by twelve, the level
payment rounded to cents, each due day's interest rounded to cents, and the
remaining payment applied to principal (clamped to the final balance). The
first payment is the first scheduled day after origination (2024-03-01). This
is the fixture's monthly convention; it is not a claim of ACTUS conformance.

Run: python3 examples/verify/verify11.py
"""
from decimal import Decimal as D, ROUND_HALF_EVEN
from pathlib import Path
import re

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent / "11-sam"


def source(path):
    return (ROOT / path).read_text()


def require(pattern, text, description):
    match = re.search(pattern, text, re.MULTILINE)
    assert match, f"missing {description} in 11-sam source"
    return match


def cents(value):
    return value.quantize(D("0.01"), rounding=ROUND_HALF_EVEN)


def money(text):
    return D(text.replace("_", ""))


contracts = source("contracts.ax")
assets = source("assets.ax")
opening = source("journal/2026/01.ax")
parties = source("parties.ax")
january = source("journal/2026/01.ax")
february = source("journal/2026/02.ax")
march = source("journal/2026/03.ax")

# Mortgage from its declared principal, rate, and term. The first scheduled
# payment follows the February 20 origination on March 1, 2024. 22 payments
# have therefore reduced the balance at the start of January 2026.
loan = require(
    r"loan\s+([\d_,]+)\s+USD\s+on\s+2024-02-20\s+at\s+([\d.]+)%\s+over\s+(\d+)y\s+for\s+condo",
    contracts,
    "mortgage terms",
)
principal, annual_rate, years = money(loan[1]), D(loan[2]) / 100, int(loan[3])
monthly_rate = annual_rate / 12
payment = cents(principal * monthly_rate / (1 - (1 + monthly_rate) ** (-12 * years)))
balance = principal
monthly = []
for month_number in range(1, 12 * years + 1):
    interest = cents(balance * monthly_rate)
    due = balance + interest if month_number == 12 * years else payment
    principal_paid = due - interest
    balance = cents(max(D(0), balance - principal_paid))
    monthly.append((due, interest, principal_paid, balance))

assert payment == D("1892.92")
assert monthly[21][3] == D("312441.12")  # after 22 payments, through Dec 2025
assert monthly[22] == (D("1892.92"), D("1529.66"), D("363.26"), D("312077.86"))
assert monthly[23] == (D("1892.92"), D("1527.88"), D("365.04"), D("311712.82"))
assert monthly[24] == (D("1892.92"), D("1526.09"), D("366.83"), D("311345.99"))
assert monthly[-1][3] == D("0.00")
assert re.search(r"monthly on 1 from checking", contracts)
assert re.search(r"also\s+-> escrow 410 USD", contracts)
assert "1,892.92" in contracts

# Mid-month residential depreciation is computed from the exact annual rate;
# rounding each monthly slice before summing would introduce a cent of drift.
property_basis = money(require(r"condo\s+basis\s+([\d_,]+)\s+USD", opening, "condo opening basis")[1])
land = money(require(r"land\s+([\d_,]+)\s+USD", assets, "condo land value")[1])
life_years = D(require(r"straight-line\(self\.cost - self\.land,\s*([\d.]+)y", source("std-sketch.ax"), "rental recovery life")[1])
condo_service_months_through_2025 = D("21.5")
base_depreciation_2024_2025 = cents(
    (property_basis - land) * condo_service_months_through_2025 / (life_years * 12)
)
base_depreciation_2026_q1 = cents((property_basis - land) * 3 / (life_years * 12))
improvement_line = require(
    r"^(\d{2}) checking -> bay-plumbing\s+([\d_,]+)\s+USD #improvement of condo",
    february,
    "condo improvement",
)
improvement_day = int(improvement_line[1])
improvement = money(improvement_line[2])
assert improvement_day == 2
# The improvement is its own part: its February half-month and March month use
# its full cost. The home's $120,000 land property belongs only to the purchase
# part and must not be subtracted from this one.
improvement_depreciation_q1 = cents(improvement * D("1.5") / (life_years * 12))
depreciation_2026_q1 = base_depreciation_2026_q1 + improvement_depreciation_q1
condo_basis_2026_03_31 = property_basis + improvement - base_depreciation_2024_2025 - depreciation_2026_q1

assert land == D("120000")
assert re.search(r"in-service 2024-03-01", assets)
assert base_depreciation_2024_2025 == D("18372.73")
assert base_depreciation_2026_q1 == D("2563.64")
assert improvement_depreciation_q1 == D("6.73")
assert depreciation_2026_q1 == D("2570.37")
assert condo_basis_2026_03_31 == D("382536.90")

# The purchase amount is tax-inclusive. A percentage of tax-inclusive price is
# tax = price * rate / (1 + rate), not price * rate.
sales_tax_rate = D(require(r"sales-tax\s+([\d.]+)%", parties, "store sales-tax rate")[1]) / 100
laptop_price = money(require(r"visa -> best-buy\s+([\d_,.]+)\s+USD #purchase of laptop", january, "laptop purchase")[1])
laptop_tax = cents(laptop_price * sales_tax_rate / (1 + sales_tax_rate))
assert laptop_tax == D("138.09")

# Paris card conversion uses the declared day's USD/EUR quote. The charge and
# received euros remain separate inputs, so the spread can be independently
# reconciled.
price_file = ROOT / "prices/2026.ax"
price = require(r"2026-02-10 EUR\s*=\s*([\d.]+)\s+USD", price_file.read_text(), "Paris EUR quote")
eur_rate = D(price[1])
eur_amount = money(require(r"visa\s+([\d.]+)\s+USD -> cafe-de-flore\s+([\d.]+)\s+EUR", february, "Paris card conversion")[2])
card_amount = money(require(r"visa\s+([\d.]+)\s+USD -> cafe-de-flore", february, "Paris card charge")[1])
eur_value = cents(eur_amount * eur_rate)
exchange_cost = cents(card_amount - eur_value)
assert eur_value == D("63.73") and exchange_cost == D("0.39")

# Payroll, shares, and the replacement lot after a wash sale.
gross = money(require(r"(4_600) USD twice monthly", contracts, "gross salary recurrence")[1])
deferral_rate = D(require(r"retirement\s+([\d.]+)%", contracts, "retirement deferral")[1]) / 100
deferral = cents(gross * deferral_rate)
first_stub = january.split("15 job", 1)[1].split("\n16 checking", 1)[0]
withholding = sum(
    money(amount)
    for amount in re.findall(r"^\s+(?:blue-shield|irs|ftb|ssa|edd)\s+([\d_,.]+)\s+USD", first_stub, re.MULTILINE)
)
net_pay = cents(gross - deferral - withholding)
annual_gross = gross * 24
annual_deferral = deferral * 24
match = cents(deferral * D("0.5"))
assert gross == D("4600") and deferral == D("276.00")
assert withholding == D("1269.30") and net_pay == D("3054.70")
assert annual_gross == D("110400") and annual_deferral == D("6624.00")
assert match == D("138.00")

buy = require(r"buy VTI for\s+([\d_,.]+)\s+USD monthly on 20", contracts, "VTI standing-order amount")
opening_lot_cost = money(buy[1])
sale_line = require(r"fidelity\[2026-01-20\]\s+([\d.]+) VTI -> ([\d.]+) USD", february, "VTI lot sale")
sale_quantity, sale = D(sale_line[1]), money(sale_line[2])
purchase_line = require(r"20 vti-monthly\s+([\d.]+) VTI", january, "January VTI purchase quantity")
assert sale_quantity == D(purchase_line[1])
wash_loss = opening_lot_cost - sale
replacement_cost = opening_lot_cost
replacement_basis = replacement_cost + wash_loss
assert wash_loss == D("18.86") and replacement_basis == D("518.86")

assert cents(D("2900") * D("120") / D("1000")) == D("348.00")
assert cents(D("45") * D("60") / 100) == D("27.00")
assert cents(D("155") * D("12") / 100) == D("18.60")

print(f"mortgage: payment {payment}; Jan/Feb/Mar interest {monthly[22][1]}, {monthly[23][1]}, {monthly[24][1]}; balance after Mar {monthly[24][3]}")
print(f"condo: depreciation through 2025 {base_depreciation_2024_2025}; 2026 Q1 {depreciation_2026_q1}; basis at 2026-03-31 {condo_basis_2026_03_31}")
print(f"laptop sales tax {laptop_tax}; Paris EUR value {eur_value}; card exchange cost {exchange_cost}")
print(f"payroll: gross/year {annual_gross}; employee deferral/year {annual_deferral}; employer match/paycheck {match}; net/paycheck {net_pay}")
print(f"wash sale: disallowed loss {wash_loss}; replacement basis {replacement_basis}")
