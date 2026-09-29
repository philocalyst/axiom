# Independent check of the 529 numbers in README.md and FINDINGS.md (F01): the
# pro-rata relief of Riley's 529 with and without basis on grandma's gift.
# Run: python3 verify-529.py
from decimal import Decimal as D
# (date, kind, amount, basis): c = contribution, g = growth, gift, w = withdrawal
ev = [("2024-12-31", "c", 24600, 19850), ("2025-01-08", "c", 250, 250), ("2025-02-08", "c", 250, 250), ("2025-03-08", "c", 250, 250),
      ("2025-03-31", "g", 101.40, 0), ("2025-04-08", "c", 250, 250), ("2025-05-08", "c", 250, 250), ("2025-05-16", "gift", 3000, 3000),
      ("2025-06-08", "c", 250, 250), ("2025-06-30", "g", 2365.31, 0), ("2025-07-08", "c", 250, 250), ("2025-08-08", "c", 250, 250),
      ("2025-08-20", "w", 4800, 0), ("2025-09-08", "c", 250, 250), ("2025-09-30", "g", 1540.94, 0), ("2025-10-08", "c", 250, 250),
      ("2025-10-15", "w", 1500, 0), ("2025-11-08", "c", 250, 250), ("2025-12-08", "c", 250, 250), ("2025-12-31", "g", 594.46, 0)]
def run(gift_has_basis):
    V = D(0); I = D(0)
    for date, k, amt, basis in ev:
        amt = D(str(amt)); basis = D(str(basis))
        if k == "gift" and not gift_has_basis:
            basis = D(0)
        if k == "w":
            ratio_basis = I / V
            relieved = (amt * ratio_basis).quantize(D("0.01"))
            earn = amt - relieved
            I -= relieved; V -= amt
            print(f"  {date} withdraw {amt}: basis relieved {relieved}, earnings {earn}")
        else:
            V += amt; I += basis
    return V, I
print("gift counted as contribution (correct):"); print(run(True))
print("gift counted as earnings (Axiom, from an income place):"); print(run(False))
