"""Independent check of examples/10-budgeter: every balance (with the amounts left blank solved from
the statements and the gaps accepted with `!`), what each envelope holds, the budgets of a month and
of a year (with the yearly membership and the premium recognized where they belong), and the 2025
return, worked out without Axiom from the journal text.

Run: python3 examples/verify/verify10.py
"""
import os
import re
import sys
from collections import defaultdict
from datetime import date, timedelta
from decimal import Decimal as D, ROUND_HALF_EVEN

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from axparse import read_journal  # noqa: E402

ROOT = os.path.join(HERE, "..", "10-budgeter")
TODAY = date(2026, 2, 14)
flows = [f for f in read_journal(ROOT) if f.day <= TODAY]


def cents(x):
    return x.quantize(D("0.01"), rounding=ROUND_HALF_EVEN)


def money(s):
    return D(s.replace("_", ""))


# ── the journal in order: flows, the statements between them, and the words that change a pending flow ─────
ASSERT = re.compile(r"^(\d{4}-\d\d-\d\d) (\S+) = ([\d_.]+) USD( !)?\s*(//.*)?$")
STATUS = re.compile(r"^(\d{4}-\d\d-\d\d) #(\S+) (settled|void|returned)\s*$")
events = [(f.file, f.lineno, "flow", f) for f in flows]
for path in sorted(os.popen(f"find {ROOT}/journal -name '*.ax'").read().split()):
    for i, l in enumerate(open(path).read().split("\n"), start=1):
        m = ASSERT.match(l)
        if m and date.fromisoformat(m.group(1)) <= TODAY:
            events.append((path, i, "assert", (m.group(1), m.group(2), money(m.group(3)), bool(m.group(4)))))
        m = STATUS.match(l)
        if m:
            events.append((path, i, "status", (m.group(1), m.group(2), m.group(3))))
events.sort(key=lambda e: (e[0], e[1]))

opening = open(os.path.join(ROOT, "journal", "2025", "09.ax")).read()
assert "  checking    4_942.10 USD" in opening and "  wallet      45 USD" in opening
held = defaultdict(D)               # checking, wallet, visa (negative: owed)
held["checking"] = D("4942.10")
held["wallet"] = D("45")
tied = defaultdict(D)               # the savings account, by the envelope its money is held for
pending = {}                        # #code -> the flow written but not yet cleared
actual = {}                         # #code -> the flow that happened, in case it is returned
unknown_atm = []
gaps = []
solved = []
CASH = {"checking", "wallet", "visa"}
FUNDS = {"emergency-fund", "car-fund", "insurance-fund", "trip-fund"}


def move(f, sign=1):
    """`SRC -> DST AMOUNT`: cash places and the envelopes; income, expenses and unknown are not tracked.
    A deposit into savings is `for` the envelope it belongs to, and a payment out of an envelope is
    written with the envelope as its source."""
    amount = f.into * sign
    if f.src in CASH:
        held[f.src] -= amount
    if f.src in FUNDS:
        tied[f.src] -= amount
    if f.dst in CASH:
        held[f.dst] += amount
    if f.dst == "savings":
        tied[f.for_] += amount


for _, _, kind, e in events:
    if kind == "status":
        day, code, what = e
        if what == "settled":
            move(pending.pop(code))                       # pending becomes real on the day it clears
        elif what == "void":
            pending.pop(code)
        else:                                             # returned: the deposit is reversed on this day
            move(actual[code], -1)
        continue
    if kind == "assert":
        day, name, said, bang = e
        if name == "savings":
            have = sum(tied.values())
        else:
            if name == "checking" and unknown_atm:
                assert len(unknown_atm) == 1, "two unknowns between two statements"
                amount = held["checking"] - said          # the bank shows less than the ledger: that much left
                held["checking"] -= amount
                held["wallet"] += amount
                solved.append((unknown_atm.pop()[0], amount))
            have = -held[name] if name == "visa" else held[name]       # a card statement shows what is owed
        if have != said:
            assert bang, (day, name, have, said)
            gaps.append((day, name, have - said))
            if name != "savings":
                held[name] = said
        continue
    f = e
    code = f.codes[0].lstrip("#") if f.codes else None
    if f.pending:                                         # `(240 USD)`: written, not yet real
        pending[code] = f
        continue
    if f.into is None and not f.legs:                     # `checking -> wallet ? USD`
        unknown_atm.append((f.day.isoformat(),))
        continue
    if f.legs:
        listed = sum((a for p, a, u in f.legs if a is not None), D(0))
        for place, amt, unit in f.legs:
            amt = f.into - listed if amt is None else amt
            if f.src == "studio":                         # a pay stub: the legs are where the gross goes
                if place == "checking":
                    held["checking"] += amt
            else:                                         # the premium: the legs are the sources
                if place in FUNDS:
                    tied[place] -= amt
                else:
                    held[place] -= amt
        continue
    if code and code.startswith("deposit-"):
        actual[code] = f
    move(f)

# ── envelopes ──────────────────────────────────────────────────────────────────────────────────────────
names = {"emergency-fund": "emergency fund", "car-fund": "car fund", "insurance-fund": "insurance fund",
         "trip-fund": "trip fund"}
print("the envelopes:", {names[c]: str(tied[c]) for c in names}, "| the savings account", sum(tied.values()))
print("amounts left blank at the ATM, solved from the statements:", [(d, str(a)) for d, a in solved])
print("gaps accepted with !:", [(d, n, str(g)) for d, n, g in gaps])
premium = [f for f in flows if f.dst == "insurance" and f.legs][0]
from_fund = [a for p, a, u in premium.legs if p in FUNDS][0]
print("premium", premium.into, "= from the insurance fund", from_fund, "+ from checking", premium.into - from_fund)
net = held["checking"] + held["wallet"] + sum(tied.values()) + held["visa"]
print("checking", held["checking"], "wallet", held["wallet"], "savings", sum(tied.values()), "card owed", -held["visa"])
print("net worth on", TODAY, net)


# ── budgets: what each envelope counts in a window, with a flow counted where it is recognized ───────────
def share(f, start, end):
    """The part of the flow recognized between two days: whole on its day, or the difference of rounded
    running shares over the days of its range (`DATE..DATE`, or `for YEAR`)."""
    amount = f.into
    if f.until is None and not (f.for_ and f.for_.isdigit()):
        return amount if start <= f.day <= end else D(0)
    a, b = (date(int(f.for_), 1, 1), date(int(f.for_), 12, 31)) if f.for_ and f.for_.isdigit() else (f.day, f.until)
    days = (b - a).days + 1

    def through(d):
        elapsed = min(max((d - a).days + 1, 0), days)
        return cents(amount * elapsed / days)
    return through(end) - through(start - timedelta(days=1))


VIA = {"jo": "fun", "food-bank": "gifts"}                  # an entity written where a place goes stands for its place


def spent(account, start, end):
    """What the flows into the account count in the window. A written check counts on the day it is written
    unless it was voided."""
    total = D(0)
    voided = {"#check-1029"}
    for f in flows:
        if f.legs and not f.dst:                           # a pay stub: not a place under a budget
            continue
        if set(f.codes) & voided:
            continue
        if VIA.get(f.dst, f.dst) == account:
            total += share(f, start, end)
    return total


def month(y, m):
    nxt = date(y + (m == 12), m % 12 + 1, 1)
    return date(y, m, 1), nxt - timedelta(days=1)


year = lambda y: (date(y, 1, 1), date(y, 12, 31))
print("budgets:")
for account, window, label in [("dining", month(2025, 11), "2025-11"), ("dining", month(2025, 12), "2025-12"), ("fun", month(2025, 10), "2025-10"),
                               ("subscriptions", month(2025, 11), "2025-11"), ("subscriptions", month(2026, 1), "2026-01"),
                               ("gifts", year(2025), "2025"), ("medical", year(2025), "2025"), ("insurance", year(2025), "2025"), ("insurance", year(2026), "2026")]:
    print("  ", account, label, spent(account, *window))

# ── the 2025 return ──────────────────────────────────────────────────────────────────────────────────────
stubs = [f for f in flows if f.src == "studio" and f.day.year == 2025]
wages = sum((f.into for f in stubs), D(0))
pretax = sum((a for f in stubs for p, a, u in f.legs if p == "health"), D(0))
withheld = sum((a for f in stubs for p, a, u in f.legs if p == "taxes/federal"), D(0))
std = D(15750)
taxable = wages - pretax - std
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
print("wages", wages, "(", len(stubs), "stubs) pre-tax", pretax, "taxable", taxable, "income tax", tax, "withheld", withheld, "owed (negative = refund)", tax - withheld)
