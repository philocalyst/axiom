"""Independent check of examples/06-investor: the 2025 return and the net worth,
worked out without Axiom. Income and holdings come from the journal text; the
realized gains come from oracle06.json, written by the small lot engine (FIFO,
LIFO, HIFO, named lots, transfers, an exchange, a split) that generated the journal
and kept its own books.

Run: python3 examples/verify/verify06.py
"""
import json
import os
import re
import sys
from collections import defaultdict
from decimal import Decimal as D, ROUND_HALF_EVEN

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from axparse import read_journal  # noqa: E402

ROOT = os.path.join(HERE, "..", "..", "tests", "v4-syntax", "examples", "06-investor")
TODAY = "2026-04-16"
all_flows = read_journal(ROOT)
flows = [f for f in all_flows if f.day.year == 2025]
oracle = json.load(open(os.path.join(HERE, "oracle06.json")))


def cents(x):
    return x.quantize(D("0.01"), rounding=ROUND_HALF_EVEN)


def value(f):
    """What arrived, in dollars: the amount, or quantity times the price written after `@`."""
    if f.price:
        return cents(f.into * f.price[0])
    if f.out_unit == "USD":
        return f.out
    return f.into if f.into_unit == "USD" else None


# ── the return ─────────────────────────────────────────────────────────────────
wages = pretax = federal = interest = dividends = staking = rsu = espp_discount = D(0)
for f in flows:
    if f.src == "northwind" and f.legs:
        wages += f.into
        for place, amt, unit in f.legs:
            if place == "retirement":
                pretax += amt
            elif place == "irs":
                federal += amt
    elif f.src == "northwind" and f.dst.endswith(".basis"):
        espp_discount += f.into
    elif f.src == "northwind":
        rsu += value(f)
    elif f.dst == "irs":
        federal += f.into
    elif f.src == "interest-source":
        interest += value(f)
    elif f.src == "dividend-source":
        dividends += value(f)
    elif f.src == "staking-source":
        staking += value(f)
wages += rsu + espp_discount

realized = [r for r in oracle["realized"] if r[0].startswith("2025")]
st = sum((D(r[4]) - D(r[5]) for r in realized if r[7] == "ST"), D(0))
lt = sum((D(r[4]) - D(r[5]) for r in realized if r[7] == "LT"), D(0))
wash = D(oracle["wash_disallowed"])
st_counted = st + wash                       # the wash sale's loss is added back

# gains and losses net across terms before they are taxed
st_net = max(st_counted + min(lt, D(0)), D(0))
lt_net = max(lt + min(st_counted, D(0)), D(0))
loss = min(max(-(st_counted + lt), D(0)), D(3000))

total_income = wages - pretax + interest + dividends + staking
agi = total_income + st_net + lt_net - loss
std = D(15750)
taxable = agi - std

single = [(0, "0.10"), (11925, "0.12"), (48475, "0.22"), (103350, "0.24"), (197300, "0.32"), (250525, "0.35"), (626350, "0.37")]
gains_rates = [(0, "0"), (48350, "0.15"), (533400, "0.20")]


def progressive(schedule, x):
    t = D(0)
    for i, (lo, rate) in enumerate(schedule):
        hi = schedule[i + 1][0] if i + 1 < len(schedule) else None
        if x <= lo:
            break
        top = x if hi is None else min(x, D(hi))
        t += cents((top - D(lo)) * D(rate))
    return t


ordinary_income = agi - lt_net
taxable_ordinary = min(max(ordinary_income - std, D(0)), taxable)
income_tax = progressive(single, taxable_ordinary) + progressive(gains_rates, taxable) - progressive(gains_rates, taxable_ordinary)
nii = interest + dividends + st_net + lt_net
niit = cents(D("0.038") * min(nii, max(agi - D(200000), D(0))))
total_tax = income_tax + niit
owed = total_tax - federal

# ── net worth: every asset place, unit by unit, at the latest price ──────────────────
held = defaultdict(lambda: defaultdict(D))          # place -> unit -> quantity
ASSETS = {"checking", "savings", "espp-cash", "fidelity", "schwab", "etrade", "coinbase", "wallet", "retirement"}


def place_of(name):
    return re.sub(r"\[.*?\]", "", name).replace("assets/", "")


in_opening = False                                   # the indented lines of the `opening` block only
for l in open(os.path.join(ROOT, "journal", "2024", "09.ax")).read().split("\n"):
    if l.startswith("opening "):
        in_opening = True
    elif not l.startswith("  "):
        in_opening = False
    m = re.match(r"^  (\S+)\s+([\d_.]+)\s+([A-Z]+)", l)
    if in_opening and m:
        held[m.group(1)][m.group(3)] += D(m.group(2).replace("_", ""))

splits = []                                          # (day, unit, ratio): the two-for-one is a quantity times 2
for path in sorted(os.popen(f"find {ROOT}/journal -name '*.ax'").read().split()):
    for l in open(path):
        m = re.match(r"^(\d{4}-\d\d-\d\d) ([A-Z]+) split (\d+) for (\d+)", l)
        if m:
            splits.append((m.group(1), m.group(2), D(m.group(3)) / D(m.group(4))))
splits.sort()

for f in all_flows:
    while splits and f.day.isoformat() >= splits[0][0]:   # a split applies before the first flow on or after it
        day, unit, ratio = splits.pop(0)
        for place in list(held):
            if unit in held[place]:
                held[place][unit] *= ratio
    if f.dst.endswith(".basis") or f.src.endswith(".basis"):
        continue
    m = re.match(r"^(\S+) all (\S+)$", f.src)
    if m:                                            # every parcel of a unit, in kind
        src, unit, dst = place_of(m.group(1)), m.group(2), place_of(f.dst)
        held[dst][unit] += held[src][unit]
        held[src][unit] = D(0)
        continue
    src, dst = place_of(f.src), place_of(f.dst)
    if f.legs and not f.dst:                          # a paycheck: legs are targets, `...` the rest
        rest = f.into - sum(a for _, a, _ in f.legs if a is not None)
        for place, amt, unit in f.legs:
            held[place_of(place)]["USD"] += amt if amt is not None else rest
        continue
    if f.out is None:                                 # `SRC -> DST 5 UNIT`: SRC gives 5 UNIT, or pays the price for it
        if f.price:
            held[src]["USD"] -= cents(f.into * f.price[0])
        else:
            held[src][f.into_unit] -= f.into
        held[dst][f.into_unit] += f.into
        continue
    held[src][f.out_unit] -= f.out
    held[dst][f.into_unit if f.into is not None else f.out_unit] += f.into if f.into is not None else f.out

prices = {}
for name in sorted(os.listdir(os.path.join(ROOT, "prices"))):
    for l in open(os.path.join(ROOT, "prices", name)):
        m = re.match(r"^(\d{4}-\d\d-\d\d) ([A-Z]+) = ([\d_.]+) USD", l)
        if m and m.group(1) <= TODAY:
            prices[m.group(2)] = D(m.group(3).replace("_", ""))
units = defaultdict(D)
for place in ASSETS:
    for unit, qty in held[place].items():
        units[unit] += qty
net_worth = sum((units["USD"],), D(0))
for unit, qty in units.items():
    if unit != "USD":
        net_worth += cents(qty * prices[unit])

print("wages", wages, "= salary", wages - rsu - espp_discount, "+ RSU", rsu, "+ ESPP discount", espp_discount)
print("pre-tax", pretax, " interest", interest, " dividends", dividends, " staking", staking)
print("realized 2025: short-term", st, " long-term", lt, " wash sale disallowed", wash)
print("short-term counted", st_counted, "(after the add-back); netted: short", st_net, "long", lt_net, "loss deduction", loss)
print("total income", total_income, " agi", agi, " taxable", taxable)
print("income tax", income_tax, " net investment income", nii, " NIIT", niit, " total tax", total_tax)
print("withheld", federal, " owed (negative = refund)", owed)
print("net worth on", TODAY, net_worth, "  (cash and 401(k)", units["USD"], ")")
print("realized lots:")
for r in realized:
    print("  ", *r)
