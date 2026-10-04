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

ROOT = os.path.join(HERE, "..", "..", "tests", "v4-syntax", "examples", "09-shared")
flows = read_journal(ROOT)
TODAY = date(2026, 4, 16)

OWN = {"checking", "savings", "wallet", "visa", "collective-checking", "collective-cash"}
LIABILITY = {"visa"}
PEOPLE = {"ben", "cleo", "riley", "lantern"}


def cents(x):
    return x.quantize(D("0.01"), rounding=ROUND_HALF_EVEN)


def code_of(f):
    return next((c.lstrip("^") for c in f.codes if c.startswith("^")), None)


# ── every flow and every claim, replayed ──────────────────────────────────────────────────────
# A claim is `PARTY owes me AMOUNT USD ... due DATE ^code` (a receivable) or `me owes PARTY ...` (a payable). A payment
# from the party that carries the code, or one to the party that carries it, settles it, oldest first. Nothing else does.
held = defaultdict(D)
claims = []                 # {who, side, code, amount, left, due, made} in the order they were made
open_at = {}                # a snapshot of the open claims on chosen days

OWES = re.compile(r"^(\d{4}-\d\d-\d\d) (\S+) owes (\S+) ([\d_.]+) USD(.*)$")


def open_claims(day, side="me"):
    return [(c["who"], c["code"], c["left"], c["due"], c["made"]) for c in claims
            if c["side"] == side and c["left"] > 0 and date.fromisoformat(c["made"]) <= day]


def settle(who, side, code, amount):
    left = amount
    for c in claims:
        if c["who"] == who and c["side"] == side and c["code"] == code and c["left"] > 0 and left > 0:
            take = min(c["left"], left)
            c["left"] -= take
            left -= take
    return left


events = [(f.file, f.lineno, "flow", f) for f in flows]
for path in sorted(os.popen(f"find {ROOT}/journal -name '*.ax'").read().split()):
    for i, l in enumerate(open(path).read().split("\n"), start=1):
        m = OWES.match(l)
        if m:
            events.append((path, i, "owes", m))
events.sort(key=lambda e: (e[0], e[1]))

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
for _, _, kind, e in events:
    day = date.fromisoformat(e.day.isoformat() if kind == "flow" else e.group(1))
    if not snapped and day > snap_day:
        open_at["2025-05-01"] = open_claims(snap_day)
        snapped = True
    if wallet_then is None and day > pad_day:
        wallet_then = held["wallet"]
    if kind == "owes":
        _, debtor, creditor, amount, rest = e.groups()
        due = re.search(r"due (\S+)", rest).group(1)
        code = re.search(r"\^(\S+)", rest).group(1)
        who, side = (debtor, "me") if creditor == "me" else (creditor, "owed")
        claims.append({"who": who, "side": side, "code": code, "left": D(amount.replace("_", "")),
                       "due": due, "made": day.isoformat()})
        continue
    f = e
    amount = f.into
    code = code_of(f)
    if f.src in PEOPLE and f.dst in OWN and code:                  # a payment, or the employer's reimbursement
        settle(f.src, "me", code, amount)
    if f.dst in PEOPLE and f.src in OWN and code:                  # what I owe Ben or Cleo, paid
        assert settle(f.dst, "owed", code, amount) == 0, (f.dst, code, amount)
    if f.src in OWN:
        held[f.src] -= amount
    if f.dst in OWN:
        held[f.dst] += amount
    if "#tips" in f.codes:
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
print("owed by me on", TODAY, sum((c[2] for c in open_claims(TODAY, "owed")), D(0)))

# ── the return: wages of the Lantern (pay and card tips) and cash tips ───────────────────────────────
gross_pay = sum((f.into for f in flows if f.src == "lantern" and "#wages" in f.codes), D(0))
total_wages = gross_pay + tips
payments = sum((f.into for f in flows if f.dst == "irs"), D(0))
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
GARDEN_SUPPLIERS = ("bayview-soil", "seed-savers", "northside-lumber", "drip-depot", "shed-co")   # not the water, not the pizza
grant_spent = sum((f.into for f in flows if f.src == "collective-checking" and f.dst in GARDEN_SUPPLIERS
                   and f.day < date(2025, 11, 1)), D(0))
returned = sum((f.into for f in flows if f.dst == "riverfront"), D(0))
print("grant 6000.00: spent on garden supplies", grant_spent, "returned", returned, "| left", D(6000) - grant_spent - returned)
print("the collective holds", held["collective-checking"] + held["collective-cash"])

# ── net worth ──────────────────────────────────────────────────────────────────────────────────────
gap = wallet_then - pad_says
print("the wallet held", pad_says, "on", pad_day, "against", wallet_then, "in the ledger: a gap of", gap, "accepted with !")
held["wallet"] -= gap
receivable = sum((c[2] for c in open_claims(TODAY)), D(0))
payable = sum((c[2] for c in open_claims(TODAY, "owed")), D(0))
worth = sum(held[p] for p in OWN) + receivable - payable
for who in ("ben", "cleo", "riley", "lantern"):
    print("owed by", who, sum((c[2] for c in open_claims(TODAY) if c[0] == who), D(0)))
print("net worth on", TODAY, worth)
