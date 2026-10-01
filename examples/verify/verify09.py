"""Independent check of examples/09-shared: the claims (who owes whom, how much is open and
how late), the 2025 return, the collective's grant and the net worth, worked out without Axiom
from the journal text.

Run: python3 examples/verify/verify09.py
"""
import os
import re
import sys
from collections import defaultdict
from datetime import date
from decimal import Decimal as D, ROUND_HALF_EVEN

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from axparse import read_journal  # noqa: E402

ROOT = os.path.join(HERE, "..", "09-shared")
flows = read_journal(ROOT)
TODAY = date(2026, 4, 16)

ENTITY = {"ben": "by-ben", "cleo": "by-cleo", "riley": "by-riley", "lantern-owes": "by-lantern", "lantern": "income/wages"}
RECEIVABLE = {"by-ben", "by-cleo", "by-riley", "by-lantern"}
PAYABLE = {"owed-to-ben", "owed-to-cleo"}
LIABILITY = {"visa"} | PAYABLE


def cents(x):
    return x.quantize(D("0.01"), rounding=ROUND_HALF_EVEN)


def place(name):
    return ENTITY.get(name, name)


# ── every flow, replayed ──────────────────────────────────────────────────────────────────────
held = defaultdict(D)
claims = []                 # [place, code, amount left, due, made] in the order they were made
open_at = {}                # a snapshot of the open claims on chosen days


def open_claims(day):
    return [(p, c, a, due, made) for p, c, a, due, made in claims if a > 0 and date.fromisoformat(made) <= day]


def make(pl, code, amount, due, day):
    claims.append([pl, code, amount, due, day])


def settle(pl, code, amount):
    left = amount
    for c in claims:
        if c[0] == pl and c[1] == code and c[2] > 0 and left > 0:
            take = min(c[2], left)
            c[2] -= take
            left -= take
    assert left == 0, (pl, code, amount)


in_opening = False
for l in open(os.path.join(ROOT, "journal", "2025", "03.ax")).read().split("\n"):
    if l.startswith("opening "):
        in_opening = True
    elif not l.startswith("  "):
        in_opening = False
    m = re.match(r"^  (\S+)\s+([\d_.]+) USD", l)
    if in_opening and m:
        held[m.group(1)] += (-1 if m.group(1) in LIABILITY else 1) * D(m.group(2).replace("_", ""))

tips = D(0)
snap_day = date(2025, 5, 1)
snapped = False
# an assertion marked `!` accepts a gap: the statement says what the wallet held, the ledger said more
pad = re.search(r"^(\d{4}-\d\d-\d\d) wallet = ([\d_.]+) USD !", "".join(open(os.path.join(ROOT, "journal", "2025", "08.ax"))), re.M)
pad_day, pad_says = date.fromisoformat(pad.group(1)), D(pad.group(2).replace("_", ""))
wallet_then = None
for f in flows:
    if not snapped and f.day > snap_day:
        open_at["2025-05-01"] = open_claims(snap_day)
        snapped = True
    if wallet_then is None and f.day > pad_day:
        wallet_then = held["wallet"]
    day = f.day.isoformat()
    code = f.codes[0].lstrip("#") if f.codes else None
    for_code = f.for_.lstrip("#") if f.for_ and f.for_.startswith("#") else None
    src = place(f.src)
    if f.legs:
        # the header names one side; the legs are the targets (a source split)
        total = f.into
        put = sum((a for p, a, u in f.legs if a is not None), D(0))
        held[src] -= total
        for pl, amount, unit in f.legs:
            amount = total - put if amount is None else amount
            pl = place(pl)
            held[pl] += amount
            if pl in RECEIVABLE and f.due:
                make(pl, code, amount, f.due, day)
        continue
    dst = place(f.dst)
    amount = f.into
    if src in RECEIVABLE:                                          # a payment, or a forgiveness, `for` the claim
        settle(src, for_code, amount)
    if dst in RECEIVABLE and f.due:                                # a loan or an expense report
        make(dst, code, amount, f.due, day)
    if dst in PAYABLE and for_code:
        pass                                                       # settles what I owe: no parcel of mine
    if src in PAYABLE and f.due:
        pass                                                       # a bill I owe: a payable claim, not tracked here
    held[src] -= amount
    held[dst] += amount
    if f.src == "income/tips":
        tips += amount

# ── claims ───────────────────────────────────────────────────────────────────────────────────────
print("claims open on 2025-05-01:")
for p, c, a, due, made in open_at["2025-05-01"]:
    print("  ", p, c, a, "due", due)
print("  total", sum(c[2] for c in open_at["2025-05-01"]))
print("claims open on", TODAY)
for p, c, a, due, made in open_claims(TODAY):
    late = (TODAY - date.fromisoformat(due)).days
    print("  ", p, c, a, "due", due, "overdue", late, "days")
print("  total", sum(c[2] for c in open_claims(TODAY)))

# ── the return: wages of the Lantern (pay and card tips) and cash tips ───────────────────────────────
gross_pay = sum((f.into for f in flows if f.src == "lantern"), D(0))
total_wages = gross_pay + tips
payments = sum((a for f in flows if f.src == "lantern" for p, a, u in f.legs if p == "taxes/federal"), D(0))
std = D(15750)
taxable = total_wages - std
single = [(0, "0.10"), (11925, "0.12"), (48475, "0.22")]


def progressive(schedule, x):
    t = D(0)
    for i, (lo, r) in enumerate(schedule):
        hi = schedule[i + 1][0] if i + 1 < len(schedule) else None
        if x <= lo:
            break
        top = x if hi is None else min(x, D(hi))
        t += cents((top - D(lo)) * D(r))
    return t


tax = progressive(single, taxable)
print("wages", total_wages, "= pay and card tips", gross_pay, "+ cash tips", tips)
print("taxable income", taxable, "income tax", tax, "withheld", payments, "owed (negative = refund)", tax - payments)

# ── the collective and its grant ───────────────────────────────────────────────────────────────────
grant_spent = sum((f.into for f in flows if f.src == "collective-checking" and f.dst in
                   ("soil", "seeds", "lumber", "irrigation", "tools", "fencing") and f.day < date(2025, 11, 1) and f.payee), D(0))
returned = sum((f.into for f in flows if f.dst == "riverfront"), D(0))
print("grant 6000.00: spent on garden supplies", grant_spent, "returned", returned, "| left", D(6000) - grant_spent - returned)
print("the collective holds", held["collective-checking"] + held["collective-cash"])

# ── net worth ──────────────────────────────────────────────────────────────────────────────────────
gap = wallet_then - pad_says
print("the wallet held", pad_says, "on", pad_day, "against", wallet_then, "in the ledger: a gap of", gap, "accepted with !")
held["wallet"] -= gap
worth = sum(held[p] for p in ("checking", "savings", "wallet", "collective-checking", "collective-cash",
                              "by-ben", "by-cleo", "by-riley", "by-lantern", "visa", "owed-to-ben", "owed-to-cleo"))
print("owed by Ben", held["by-ben"], "Cleo", held["by-cleo"], "Riley", held["by-riley"], "the Lantern", held["by-lantern"])
print("net worth on", TODAY, worth)
