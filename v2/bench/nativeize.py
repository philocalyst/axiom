"""Rewrite the benchmark simulation's records into the native v4 surface.

The simulation in :mod:`gen` is intentionally shared with the retained 1m
workload: this module changes its account taxonomy and source spelling without
changing the seed, daily events, cents, or number of journal rows.
"""

from __future__ import annotations

import os
import re
from decimal import Decimal, ROUND_HALF_UP


def _purpose(person: str, old_path: str) -> str:
    parts = old_path.split("/")
    if parts[0] == "expenses":
        parts = parts[2:]
    return person + "-" + "-".join(parts)


def _files(persons):
    places: dict[str, str] = {}
    parties: dict[str, str] = {}
    leaves: dict[str, tuple[str, str]] = {}
    payees: dict[str, tuple[str, str]] = {}

    for p in persons:
        k = p.k
        n = p.n
        for key, target in (
            ("chk", f"{k}-checking"),
            ("sav", f"{k}-savings"),
            ("cash", f"{k}-cash"),
            ("brok", f"{k}-brokerage"),
            ("k401", f"{k}-401k"),
            ("p529", f"{k}-529"),
            ("eur", f"{k}-eur-wallet"),
            ("visa", f"{k}-visa"),
        ):
            places[n[key]] = target
        parties[n["salary"]] = p.acme
        parties[n["interest"]] = f"{k}-interest-source"
        # A commodity and an issuer are separate native concepts. Keep the
        # original dividend source as an explicit issuer party rather than
        # resolving the commodity symbol as an entity.
        parties[n["dividends"]] = "vti-issuer"
        parties[n["growth"]] = f"{k}-market"
        parties[n["other"]] = f"{k}-other"
        parties[n["opening"]] = f"{k}-opening"
        parties[n["grants"]] = f"{k}-grant-income"
        parties[n["fed"]] = f"{k}-federal-tax"
        parties[n["state"]] = f"{k}-state-tax"
        if p.has_city:
            parties[n["city"]] = f"{k}-city-tax"
        parties[n["payroll"]] = f"{k}-payroll-tax"

        for path, old_name, payee, category in p.leaves:
            old = old_name
            purpose = _purpose(k, path)
            party = f"{k}-leaf-" + "-".join(path.split("/")[2:])
            leaves[old] = (party, purpose)
            if payee:
                payees[payee] = (purpose, party)

        parties[p.acme] = p.acme
        parties[p.contractor] = p.contractor
        parties[p.university] = p.university
        parties[p.landlord] = p.landlord

    return places, parties, leaves, payees


def _native_accounts(persons, full: bool, years: int, budget_factor: float) -> str:
    out = ["// Native v4 purposes, positions and counterparties.", ""]
    if full:
        out.extend(["kind gold-share : security", "", "// Traded units retain their baseline price process and precision."])
        for sym, kind, precision, _start, _drift, _vol, name in _commodities():
            native_kind = "gold-share" if kind == "good" else kind
            out.append(f'commodity {sym} : {native_kind}')
            out.append(f"  precision {precision}")
            out.append(f'  name "{name}"')
            if kind in ("fund", "stock"):
                out.append("  grows 6% yearly")
            out.append("")
        out.extend(["code chk-*", "  on bank", ""])
    else:
        for sym, _kind, precision, *_rest in _commodities():
            out.extend([f"commodity {sym}", f"  precision {precision}", ""])
    out.extend(["entity vti-issuer #dividend", ""])

    for p in persons:
        k = p.k
        out.extend([
            f"entity {k}-bank" + (" : org" if full else ""),
            f"entity {k}-custodian" + (" : org" if full else ""),
            f"entity {k}-market" + (" : org #gain" if full else " #gain"),
            f"entity {k}-opening" + (" : org #contribution" if full else " #contribution"),
            f"entity {k}-interest-source" + (f" : org #interest-income" if full else " #interest-income"),
            f"entity {k}-other" + (f" : org #{k}-other-income" if full else f" #{k}-other-income"),
            f"purpose {k}-other-income : income",
            f"entity {k}-federal-tax" + (" : tax-authority" if full else " #tax-paid"),
            f"entity {k}-state-tax" + (" : tax-authority" if full else " #tax-paid"),
            f"entity {k}-payroll-tax" + (f" : org #{k}-payroll-withholding" if full else f" #{k}-payroll-withholding"),
            f"purpose {k}-payroll-withholding : spending",
        ])
        if p.has_city:
            out.append(f"entity {k}-city-tax" + (" : tax-authority" if full else " #tax-paid"))
        out.extend([
            f"entity {p.acme}" + (" : employer" if full else ""),
            f"entity {p.contractor}" + (" : contractor" if full else ""),
            f"entity {p.university}" + (f" : org #{k}-edu-tuition-university" if full else f" #{k}-edu-tuition-university"),
            f"entity {p.landlord}" + (f" : landlord #{k}-housing-rent-landlord" if full else f" #{k}-housing-rent-landlord"),
        ])

        # The old expense chart becomes a typed purpose tree, with the same
        # 160 leaf destinations and category budget totals.
        category_budgets = {
            "housing": 9_000,
            "travel": 5_000,
            "edu": 12_000,
            "gifts": 250,
        }
        seen = set()
        for category, weight, _subs in _categories():
            budget = category_budgets.get(category)
            if budget is None:
                mean = 2_800 * weight
                import math
                budget = max(200, int(math.ceil(mean * budget_factor / 100.0)) * 100)
            parent = f"{k}-{category}"
            out.append(f"purpose {parent} : spending")
            if p.full:
                out.append(f"  budget {budget} USD monthly")
            for path, _old_name, _payee, _leaf_category in p.leaves:
                parts = path.split("/")
                if parts[2] != category:
                    continue
                sub = parts[3]
                parent_sub = f"{k}-{category}-{sub}"
                if parent_sub not in seen:
                    out.append(f"purpose {parent_sub} : {parent}")
                    seen.add(parent_sub)
                out.append(f"purpose {_purpose(k, path)} : {parent_sub}")

        # Every old leaf/payee remains addressable as a party. The party's
        # purpose replaces the old expense-account destination.
        for path, _old_name, payee, _category in p.leaves:
            if payee:
                purpose = _purpose(k, path)
                out.append(f"entity {payee}" + (f" : merchant #{purpose}" if full else f" #{purpose}"))
            party = f"{k}-leaf-" + "-".join(path.split("/")[2:])
            purpose = _purpose(k, path)
            out.append(f"entity {party}" + (f" : merchant #{purpose}" if full else f" #{purpose}"))

        if full:
            accounts = [
                ("checking", "bank", "at {k}-bank"),
                ("savings", "bank", "at {k}-bank"),
                ("cash", "cash", ""),
                ("brokerage", "brokerage", "at {k}-custodian"),
                ("401k", "401k", "at {k}-custodian"),
                ("529", "529-plan", ""),
                ("eur-wallet", "bank", "at {k}-bank"),
                ("visa", "credit-card", "at {k}-bank"),
            ]
            for suffix, kind, holder in accounts:
                place = f"{k}-{suffix}"
                out.append(f"account {place} : {kind} {holder.format(k=k)}".rstrip())
                out.append(f"  owner {k}")
                if suffix == "brokerage":
                    out.append("  holds " + ", ".join(sym for sym, *_ in _commodities()))
                    out.append(f"  select {p.policy}")
                elif suffix == "eur-wallet":
                    out.append("  holds EUR")
                    out.append("  select fifo")
                elif suffix == "401k":
                    out.append(f"  employer {p.acme}")
                elif suffix == "529":
                    out.append(f"  beneficiary {k}")
                out.append("")
        else:
            for suffix in ("checking", "savings", "cash", "brokerage", "401k", "529", "eur-wallet", "visa"):
                out.append(f"account {k}-{suffix}")
                out.append(f"  owner {k}")
                if suffix == "eur-wallet":
                    out.append("  holds EUR")
                if suffix == "brokerage":
                    out.append("  holds " + ", ".join(sym for sym, *_ in _commodities()))
                out.append("")

        for year in range(2024, 2024 + years):
            if p.grants and full:
                out.extend([
                    f"entity scholarship-{k}-{year} : grant",
                    f"  purpose {k}-edu",
                    f"  until {year}-12-31",
                    "",
                ])
        out.append("")
    return "\n".join(out) + "\n"


def _commodity_data():
    # Keep this table in sync with the generator's original deterministic price
    # simulation. It is repeated here to avoid importing/starting the CLI.
    return (
        ("VTI", "fund", 0, 220.0, 0.08, 0.010, "Total stock market ETF"),
        ("VXUS", "fund", 0, 58.0, 0.05, 0.011, "Total international stock ETF"),
        ("BND", "bond", 0, 72.0, 0.02, 0.003, "Total bond market ETF"),
        ("AAPL", "stock", 0, 185.0, 0.12, 0.016, "Apple Inc."),
        ("BTC", "crypto", 8, 42_000.0, 0.25, 0.035, "Bitcoin"),
        ("GLD", "good", 0, 190.0, 0.04, 0.008, "Gold shares"),
    )


def _commodities():
    return _commodity_data()


_COMMODITY_NAMES = frozenset(row[0] for row in _commodity_data())


def _categories():
    # Same categories and weights as gen.py. The names/leaves stay there; this
    # helper only needs the category budget headings.
    return (
        ("housing", 0.06, ()),
        ("food", 0.30, ()),
        ("transport", 0.14, ()),
        ("health", 0.06, ()),
        ("fun", 0.12, ()),
        ("shopping", 0.14, ()),
        ("gifts", 0.03, ()),
        ("travel", 0.05, ()),
        ("pets", 0.04, ()),
        ("edu", 0.02, ()),
    )


def _endpoint(value: str, places, parties, leaves, payees) -> str:
    if value in places:
        return places[value]
    if value in parties:
        return parties[value]
    if value in leaves:
        return leaves[value][0]
    if value in payees:
        return value
    return value


def _purpose_for_endpoint(value: str, leaves, payees):
    if value in leaves:
        return leaves[value][1]
    if value in payees:
        return payees[value][0]
    return None


def _native_line(line, places, parties, leaves, payees, account_names):
    indent = line[: len(line) - len(line.lstrip())]
    body = line.strip()
    if not body or body.startswith("//"):
        return line

    body = re.sub(r"#(chk-[A-Za-z0-9-]+)", r"^\1", body)
    # Source-code markers changed sigil from # to ^ in v4.
    if re.match(r"\d{4}-\d\d-\d\d\s+#chk-", body):
        return indent + body

    if " = " in body:
        left, right = body.split(" = ", 1)
        tokens = left.split()
        if len(tokens) >= 2 and re.fullmatch(r"\d{4}-\d\d-\d\d", tokens[0]):
            tokens[1] = _endpoint(tokens[1], places, parties, leaves, payees)
            return indent + " ".join(tokens) + " = " + right

    if " -> " not in body:
        tokens = body.split()
        if tokens:
            tokens[0] = _endpoint(tokens[0], places, parties, leaves, payees)
            return indent + " ".join(tokens)
        return line

    before, after = body.split(" -> ", 1)
    left = before.split()
    if len(left) < 2:
        return line
    old_source = left[1]
    new_source = _endpoint(old_source, places, parties, leaves, payees)
    left[1] = new_source

    right = after.split()
    if not right:
        return indent + " ".join(left) + " ->"
    old_target = right[0]
    has_target = (
        old_target in places
        or old_target in parties
        or old_target in leaves
        or old_target in payees
        or bool(re.fullmatch(r"scholarship-p\d+-\d{4}", old_target))
        or old_target in _COMMODITY_NAMES
        or old_target == "?"
    )
    # A split header and an asset sale end in an amount rather than naming the
    # receiving endpoint on the right.
    old_target_path = old_target
    purpose = _purpose_for_endpoint(old_target_path, leaves, payees)
    target_name = _endpoint(old_target, places, parties, leaves, payees) if has_target else None
    if has_target:
        right[0] = target_name

    # Native exchange lines state the cash leg explicitly as well as the
    # number of units. The simulation already computed that exact cash amount
    # when it produced quantity and price; reconstruct it in decimal
    # arithmetic. The retained daily quote series carries market prices.
    if "@" in right:
        try:
            at = right.index("@")
            quantity = Decimal(right[1].replace("_", ""))
            price = Decimal(right[at + 1].replace("_", ""))
            cash = (quantity * price).quantize(Decimal("0.01"), rounding=ROUND_HALF_UP)
            left.append(f"{cash:.2f}")
            left.append(right[-1])
            right = right[:at]
        except (IndexError, ArithmeticError, ValueError):
            raise ValueError(f"cannot derive native price cash leg from: {line}")

    # Legacy `/ party` tails selected the real merchant in the v3 chart. The
    # v4 party is the endpoint and its typed purpose remains explicit.
    if has_target and old_target in leaves:
        try:
            slash = right.index("/")
        except ValueError:
            slash = -1
        if slash >= 0 and slash + 1 < len(right):
            merchant = right[slash + 1]
            right[0] = merchant
            target_name = merchant
            right = right[:slash] + right[slash + 2 :]
            purpose = _purpose_for_endpoint(merchant, leaves, payees) or purpose

    all_accounts = account_names
    tail_text = " ".join(right[1:] if has_target else right)
    has_marker = "#" in tail_text or "^" in tail_text
    add_purpose = None
    if purpose and not has_marker:
        add_purpose = purpose
    elif has_target and target_name in all_accounts and new_source in all_accounts and not has_marker:
        # Internal movements still state that money stays with the same owner.
        add_purpose = "contribution"
    elif has_target and target_name in all_accounts and old_target not in places and new_source in all_accounts and not has_marker:
        add_purpose = "contribution"

    # Prices and quantities on a trade name the commodity being bought or sold.
    # A priced transfer into a brokerage already identifies the acquired
    # commodity. `purchase of` is reserved for identified things, not units.
    if has_target and target_name in all_accounts and new_source in all_accounts and not has_marker:
        trade_unit = next((token for token in right[1:] if token in _COMMODITY_NAMES), None)
        if not trade_unit:
            add_purpose = "contribution"
    if not has_target and new_source in all_accounts:
        left_unit = next((token for token in left[2:] if token in _COMMODITY_NAMES), None)
        if left_unit:
            add_purpose = f"sale of {left_unit}"

    converted = " ".join(left) + " -> " + " ".join(right)
    if add_purpose:
        converted += f" #{add_purpose}"
    return indent + converted


def _native_prices(root):
    directory = os.path.join(root, "prices")
    if not os.path.isdir(directory):
        return
    for name in os.listdir(directory):
        path = os.path.join(directory, name)
        if not os.path.isfile(path):
            continue
        rows = []
        with open(path, encoding="utf-8") as source:
            for line in source:
                stripped = line.rstrip("\n")
                if stripped and not stripped.startswith("//"):
                    day, unit, amount, quote = stripped.split()
                    rows.append(f"{day} {unit} = {amount} {quote}")
                else:
                    rows.append(stripped)
        with open(path, "w", encoding="utf-8") as output:
            output.write("\n".join(rows) + "\n")


def _native_plans(root, persons, places, parties, leaves, payees):
    source = os.path.join(root, "plans.ax")
    if not os.path.exists(source):
        return
    lines = open(source, encoding="utf-8").read().splitlines()
    out = ["// Future obligations, expressed as native contracts.", ""]
    plan_index = 0
    index = 0
    while index < len(lines):
        line = lines[index]
        match = re.match(
            r"every (month|year) on (\S+) from (\S+)(?: until (\S+))? (\S+) -> (.+)$",
            line,
        )
        if not match:
            if line.strip() and not line.lstrip().startswith("//"):
                raise ValueError(f"unrecognized legacy benchmark plan: {line}")
            index += 1
            continue
        cadence, on, start, until, raw_source, raw_tail = match.groups()
        raw_source = _endpoint(raw_source, places, parties, leaves, payees)
        parts = raw_tail.split()
        if len(parts) < 2:
            raise ValueError(f"incomplete legacy benchmark plan: {line}")
        period = "monthly" if cadence == "month" else "yearly"
        plan_index += 1
        contract = f"benchmark-{plan_index}"
        children = []
        child_index = index + 1
        while child_index < len(lines) and (lines[child_index].startswith("  ") or not lines[child_index].strip()):
            if lines[child_index].strip():
                children.append(lines[child_index])
            child_index += 1
        # The legacy incoming payroll header is an amount, followed by the
        # payroll split destinations. Native contracts keep that same split
        # as template legs, with the residual landing in checking.
        amount_head = bool(re.fullmatch(r"[\d_,.]+\s+[A-Za-z][A-Za-z0-9_-]*", " ".join(parts[:2])))
        if amount_head:
            amount = " ".join(parts[:2])
            residual = next((child.strip().split()[0] for child in children if child.strip().endswith(" ...")), None)
            target = _endpoint(residual, places, parties, leaves, payees) if residual else None
            if target is None:
                raise ValueError(f"incoming plan has no receiving account: {line}")
            out.extend([
                f"contract {contract} with {raw_source}",
                f"  {amount} {period} on {on} into {target}",
            ])
            out.extend("  " + _native_line(child, places, parties, leaves, payees, set(places.values())).strip()
                       for child in children if not child.strip().endswith(" ..."))
        else:
            raw_target = parts[0]
            target = _endpoint(raw_target, places, parties, leaves, payees)
            amount = " ".join(parts[1:])
            purpose = _purpose_for_endpoint(raw_target, leaves, payees)
            suffix = f" #{purpose}" if purpose else ""
            out.extend([
                f"contract {contract} with {target}",
                f"  {amount} {period} on {on} from {raw_source}{suffix}",
            ])
        out.append(f"  from {start}")
        if until:
            # The old generator uses inclusive year-month limits. Native
            # contract limits are inclusive dates.
            year, month = map(int, until.split("-"))
            import calendar
            last_day = calendar.monthrange(year, month)[1]
            out.append(f"  until {year:04d}-{month:02d}-{last_day:02d}")
        index += 1
        while index < len(lines) and (lines[index].startswith("  ") or not lines[index].strip()):
            index += 1
        out.append("")
    with open(source, "w", encoding="utf-8") as output:
        output.write("\n".join(out) + "\n")


def _rewrite_journals(root, places, parties, leaves, payees):
    journal = os.path.join(root, "journal")
    account_names = set(places.values())
    if os.path.isdir(journal):
        for base, _dirs, files in os.walk(journal):
            for name in files:
                path = os.path.join(base, name)
                with open(path, encoding="utf-8") as source:
                    lines = source.read().splitlines()
                output = [
                    _native_line(line, places, parties, leaves, payees, account_names)
                    for line in lines
                ]
                with open(path, "w", encoding="utf-8") as dest:
                    dest.write("\n".join(output) + "\n")
    path = os.path.join(root, "journal.ax")
    if os.path.isfile(path):
        with open(path, encoding="utf-8") as source:
            lines = source.read().splitlines()
        with open(path, "w", encoding="utf-8") as dest:
            dest.write("\n".join(_native_line(line, places, parties, leaves, payees, account_names) for line in lines) + "\n")


def nativeize_project(root, persons, full: bool, years: int, budget_factor: float) -> None:
    """Replace chart-account output with the native purpose/party model.

    The journal generator is unchanged: event dates, signed cents, security
    quantities, counts and assertions remain seed-for-seed identical.
    """
    places, parties, leaves, payees = _files(persons)
    with open(os.path.join(root, "accounts.ax"), "w", encoding="utf-8") as output:
        output.write(_native_accounts(persons, full, years, budget_factor))
    _native_prices(root)
    _native_plans(root, persons, places, parties, leaves, payees)
    _rewrite_journals(root, places, parties, leaves, payees)
