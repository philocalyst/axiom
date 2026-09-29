# Independent check of the numbers in README.md: the mortgage, the year's
# expenses and income, depreciation, and the gain on sale with recapture, worked
# out without Axiom. Run: python3 verify.py
from decimal import Decimal as D, ROUND_HALF_EVEN
q = lambda x: D(x).quantize(D("0.01"), rounding=ROUND_HALF_EVEN)
LOAN = D(279000); r = D("0.0675") / 12
pmt = q(LOAN * r / (1 - (1 + r) ** -360))
bal = LOAN; interest = D(0)
for m in range(2, 13):
    i = q(bal * r); interest += i; bal -= (pmt - i)
accrued = q(bal * r * D(28) / D(30))
interest += accrued
print("payment", pmt, "interest 2025", interest, "payoff principal", bal, "accrued", accrued)
prop_tax = D(4380); ins = D(1560); repairs = D(385 + 240 + 180 + 1150 + 96); adv = D(149); util = D("154.60")
mgmt = sum(q(D(x) * D("0.08")) for x in [2400, 2400, 2400, 2400, 2475, 2400, 2400, 2500, 2500, 2500])
amort = D(3120)
bldg = q(D(376850) * D("0.80")); dm = q(bldg / D("27.5") / 12); rm = q(D(14200) / D("27.5") / 12)
dep = q(dm / 2) + dm * 10 + q(dm / 2) + q(rm / 2) + rm * 2 + q(rm / 2)
total = interest + prop_tax + ins + repairs + adv + util + mgmt + amort + dep
print("mgmt", mgmt, "dep", dep, "TOTAL expenses", total)
income = D(2400) * 6 + D(2475) + D(2500) * 3 + D(350)
print("income", income, "net", income - total)
# recapture and gains
net_proceeds = D(431500) - q(D(431500) * D("0.05")) - q(D(431500) * D("0.0125"))
dep_h = q(dm / 2) + dm * 10 + q(dm / 2)
dep_r = q(rm / 2) + rm * 2 + q(rm / 2)
roof_alloc = D(14200) - dep_r
house_alloc = net_proceeds - roof_alloc
print("net proceeds", net_proceeds, "house alloc", house_alloc, "roof alloc", roof_alloc)
print("engine gain house", house_alloc - D(376850), "roof", roof_alloc - D(14200))
print("tax gain house (adjusted basis)", house_alloc - (D(376850) - dep_h), "roof", roof_alloc - (D(14200) - dep_r))
print("recapture total", dep_h + dep_r)
