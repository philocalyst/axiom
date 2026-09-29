"""A tiny reader of the flows in an example's journal, independent of Axiom.

It understands what the generated journals use: `DATE[..DATE] SRC -> DST AMOUNT
UNIT [@ PRICE UNIT] [/ payee] [#codes] [for WHAT] [due WHEN] [basis AMOUNT] [!]`,
one-side splits (indented legs, `...` remainders), `opening` blocks, prices,
splits, assertions and doc comments. It yields plain records; tax and balance
rules are the caller's.
"""
import glob
import os
import re
from dataclasses import dataclass, field
from datetime import date, timedelta
from decimal import Decimal as D, ROUND_HALF_EVEN

DATE = re.compile(r"^(\d{4})-(\d{2})-(\d{2})")


def to_date(s):
    y, m, d = s.split("-")
    return date(int(y), int(m), int(d))


def money(s):
    return D(s.replace("_", ""))


def q2(x):
    return x.quantize(D("0.01"), rounding=ROUND_HALF_EVEN)


@dataclass
class Flow:
    day: date
    until: date | None            # end of the recognition range (DATE..DATE)
    src: str
    dst: str
    out: D | None                 # amount leaving src
    out_unit: str | None
    into: D | None                # amount arriving (for exchanges)
    into_unit: str | None
    payee: str | None
    codes: list
    for_: str | None
    due: str | None
    basis: D | None
    waive: bool
    pending: bool
    legs: list = field(default_factory=list)   # (place, amount|None, unit)
    line: str = ""
    file: str = ""
    lineno: int = 0
    header_split: str | None = None            # "src" or "dst" when legs exist
    price: tuple | None = None                 # (amount, unit) after `@`


AMOUNT = r"\(?(\d[\d_]*(?:\.\d+)?)\)?\s+([A-Z][A-Z0-9._]*)"


def parse_tail(rest):
    payee = None
    codes = []
    for_ = None
    due = None
    basis = None
    waive = False
    rest = re.sub(r"\s//.*$", "", rest)
    if rest.strip().endswith("!"):
        waive = True
        rest = rest.rstrip()[:-1]
    m = re.search(r"\sbasis\s+(\d[\d_]*(?:\.\d+)?)\s+[A-Z]+", rest)
    if m:
        basis = money(m.group(1))
        rest = rest[: m.start()] + rest[m.end():]
    m = re.search(r"\sdue\s+(\S+)", rest)
    if m:
        due = m.group(1)
        rest = rest[: m.start()] + rest[m.end():]
    m = re.search(r"\sfor\s+(\S+)", rest)
    if m:
        for_ = m.group(1)
        rest = rest[: m.start()] + rest[m.end():]
    m = re.search(r"\s/\s+([a-z0-9-]+)", rest)
    if m:
        payee = m.group(1)
        rest = rest[: m.start()] + rest[m.end():]
    codes = re.findall(r"#[a-z0-9:./-]+", rest)
    return payee, codes, for_, due, basis, waive


def read_journal(root):
    """All flows of the project, in file order (files sorted by path)."""
    flows = []
    files = sorted(glob.glob(os.path.join(root, "journal", "**", "*.ax"), recursive=True))
    for path in files:
        lines = open(path).read().split("\n")
        i = 0
        in_opening = False
        while i < len(lines):
            raw = lines[i]
            i += 1
            line = re.sub(r"\s//.*$", "", raw) if not raw.startswith("///") else ""
            if not line.strip() or line.lstrip().startswith("//"):
                continue
            if line.startswith("opening"):
                in_opening = True
                continue
            if line.startswith(" ") and in_opening:
                continue
            if not line.startswith(" "):
                in_opening = False
            m = re.match(r"^(\d{4}-\d{2}-\d{2})(?:\.\.(\d{4}-\d{2}-\d{2}))?\s+(.*)$", line)
            if not m or "->" not in line:
                continue
            day, until, body = m.group(1), m.group(2), m.group(3)
            left, right = body.split("->", 1)
            left, right = left.strip(), right.strip()
            src = left
            out = out_unit = None
            am = re.match(r"^(.*?)\s*(?:\((\d[\d_]*(?:\.\d+)?)\s+([A-Z][A-Z0-9._]*)\)|(\d[\d_]*(?:\.\d+)?)\s+([A-Z][A-Z0-9._]*)|all(?:\s+([A-Z][A-Z0-9._]*))?)$", left)
            if am and am.group(1) is not None and (am.group(2) or am.group(4)):
                src = am.group(1).strip()
                out = money(am.group(2) or am.group(4))
                out_unit = am.group(3) or am.group(5)
            dst = right
            into = into_unit = None
            price = None
            # destination place and amount
            tm = re.match(r"^(.*?)\s*(\d[\d_]*(?:\.\d+)?)\s+([A-Z][A-Z0-9._]*)(.*)$", right)
            place_only = right.split()[0] if right and not right[0].isdigit() and not right.startswith("?") else ""
            unknown_place = bool(re.match(r"^\?\s+\(?\d", right))         # `-> ? 14.20 USD`: money out to an unknown place
            if right and not unknown_place and (right[0].isdigit() or right.startswith("(") or right.startswith("?")):
                # header names only the source: `-> 5_200 USD` (legs are targets)
                tm2 = re.match(r"^\(?(\d[\d_]*(?:\.\d+)?)\)?\s+([A-Z][A-Z0-9._]*)(.*)$", right)
                dst = ""
                into = money(tm2.group(1)) if tm2 else None
                into_unit = tm2.group(2) if tm2 else None
                tail = tm2.group(3) if tm2 else ""
            elif not right:
                # `SRC AMOUNT UNIT ->` and nothing after it: the indented legs are the targets
                dst, tail = "", ""
            else:
                dst_m = re.match(r"^(\S+)(.*)$", right)
                dst = dst_m.group(1)
                tail = dst_m.group(2)
                am2 = re.match(r"^\s*\(?(\d[\d_]*(?:\.\d+)?)\)?\s+([A-Z][A-Z0-9._]*)(.*)$", tail)
                if am2:
                    into = money(am2.group(1))
                    into_unit = am2.group(2)
                    tail = am2.group(3)
                pm = re.match(r"^\s*@\s*(\d[\d_]*(?:\.\d+)?)\s+([A-Z]+)(.*)$", tail)
                if pm:
                    price = (money(pm.group(1)), pm.group(2))
                    tail = pm.group(3)
            payee, codes, for_, due, basis, waive = parse_tail(tail)
            pending = "(" in left[-12:] or bool(re.search(r"\(\d", right))
            flow = Flow(to_date(day), to_date(until) if until else None, src, dst, out, out_unit, into, into_unit,
                        payee, codes, for_, due, basis, waive, pending, [], raw, path, i, price=price)
            # legs
            while i < len(lines) and lines[i].startswith("  ") and lines[i].strip() and not lines[i].strip().startswith("//"):
                leg = re.sub(r"\s//.*$", "", lines[i]).strip()
                i += 1
                lm = re.match(r"^(\S+)\s+(?:\.\.\.|\(?(\d[\d_]*(?:\.\d+)?)\)?\s+([A-Z][A-Z0-9._]*))(.*)$", leg)
                if lm:
                    flow.legs.append((lm.group(1), money(lm.group(2)) if lm.group(2) else None, lm.group(3)))
            if not dst and flow.legs:
                flow.header_split = "dst"
            flows.append(flow)
    return flows


def recognize(flow, amount, year_of_interest):
    """The share of `amount` recognized in a calendar year, as Axiom cuts it:
    the difference of rounded running shares over the days of the range."""
    if flow.until is None and (flow.for_ is None or not re.fullmatch(r"\d{4}", flow.for_ or "")):
        return amount if flow.day.year == year_of_interest else D(0)
    if flow.for_ and re.fullmatch(r"\d{4}", flow.for_):
        start, end = date(int(flow.for_), 1, 1), date(int(flow.for_), 12, 31)
    else:
        start, end = flow.day, flow.until
    days = (end - start).days + 1

    def through(d):
        elapsed = min(max((d - start).days + 1, 0), days)
        return (amount * elapsed / days).quantize(D("0.01"), rounding=ROUND_HALF_EVEN)
    ys, ye = date(year_of_interest, 1, 1), date(year_of_interest, 12, 31)
    return through(ye) - through(ys - timedelta(days=1))
