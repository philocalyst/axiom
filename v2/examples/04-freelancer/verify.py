# Independent check of the numbers in README.md: the 2025 return worked out by
# hand, without Axiom. Run: python3 verify.py
from decimal import Decimal as D, ROUND_HALF_EVEN
def r(x): return x.quantize(D("0.01"), rounding=ROUND_HALF_EVEN)
gross = D("74800.00")           # 27 payment legs received in 2025
exp = D("13742.54")             # deductible expenses
net = gross - exp
earn = net * D("0.9235")
ss = min(earn, D(176100)) * D("0.124")
med = earn * D("0.029")
se = r(ss + med)
half = r(se * D("0.5"))
sehi = D("4944")                # health premiums
sep = D("6000")                 # SEP-IRA deduction
adj = net - half - sehi - sep
interest = D("290.12")
agi_pre_qbi = adj + interest
std = D("15750")
before = agi_pre_qbi - std
qbi = r(min(adj * D("0.2"), before * D("0.2")))
taxable = before - qbi
def tax(x):
    br = [(0, "0.10"), (11925, "0.12"), (48475, "0.22"), (103350, "0.24")]
    t = D(0)
    for i, (lo, rate) in enumerate(br):
        hi = br[i + 1][0] if i + 1 < len(br) else None
        if x > lo:
            t += (min(x, D(hi)) - D(lo) if hi else x - D(lo)) * D(rate)
    return r(t)
inc = tax(taxable)
print("net", net, "SE", se, "half", half, "adj", adj, "QBI", qbi, "taxable", taxable, "income tax", inc)
print("total", se + inc, "paid 2025", D(7200), "owed", se + inc - 7200, "with Q4", se + inc - 9600)
# what Axiom computes: Q1's SEP-IRA market loss counted as a distribution (FINDINGS F03)
dist = D("385.35")
before2 = agi_pre_qbi + dist - std
qbi2 = r(min(adj * D("0.2"), before2 * D("0.2")))
print("with distribution artifact: taxable", before2 - qbi2, "tax", tax(before2 - qbi2), "penalty", r(dist * D("0.10")))
