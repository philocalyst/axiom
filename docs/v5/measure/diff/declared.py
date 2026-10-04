"""The declared-lines corpus: one small book per way a declared line can be wrong.

Lane U's U4 and U5 replace the hand-written readers of property lines (a contract's `loan`, `resets`, `prepay`, dates,
`grace`, `for last`, `covers`, `rising`, `indexed`, `area`, `deposit`, `share`; `input`; `now at`; the built-in
properties; the lines of a `format` and a `sync` source; a relator's slots) with one reader of a signature. Before this
corpus no golden and no mistake book printed most of what those readers say, so nothing would see a sentence change.

Each case below is one book with one thing wrong and the code it must raise. `write` turns the table into
`cases3/decl-*.ax`, which `run.sh` reads like `cases/` (so `compare.sh` shows every change between two binaries);
`check BINARY` runs every case and says which does not raise its code, which is how the corpus is held to the paths it
claims to reach.

    python3 declared.py write
    python3 declared.py check AXIOM-BINARY
"""
import os
import re
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "cases3")

BOOK = """\
use std
base USD
entity me : person
entity acme : org
entity shop : org
account checking : bank
account savings : bank
account escrow : bank
  owner acme
account wallet : bank
  holds USD
asset car : vehicle
asset house : vehicle
commodity VTI : stock
kind sq : measure
commodity SQF : sq
param cpi
  2026 3%
param fee USD
  2026 5 USD
2025-12-31 market -> checking 9_000 USD
"""

LEASE = ["500 USD monthly on 1 from checking", "from 2026-07-01"]


def contract(*lines, head="contract lease with shop", schedule=LEASE):
    """A lease with `lines` under it, after its schedule."""
    return BOOK + head + "\n" + "".join(f"  {line}\n" for line in list(schedule) + list(lines))


LOAN = "loan 100_000 USD on 2026-01-01 at 5% over 10y"


def loan(line=LOAN, *nested, head="contract mortgage with shop"):
    """A mortgage whose `loan` line is `line`, with `nested` lines under it."""
    text = BOOK + head + "\n" + f"  {line}\n" + "".join(f"    {n}\n" for n in nested)
    return text + "  monthly on 1 from checking\n  from 2026-07-01\n"


def resets(rule):
    return loan(LOAN, rule)


def under(declaration, *lines):
    """A declaration with property lines under it, after the book."""
    return BOOK + declaration + "\n" + "".join(f"  {line}\n" for line in lines)


# A kind of contract with two slots, over the standard kinds (`model/tests/relator.rs` has it without them).
RELATED = """\
use std
base USD
kind boss : org
kind employment : contract
  has employee person
  has employer boss
entity acme : boss
entity alex : person
entity me : person
account checking : bank
"""
PAID = ["employee me", "employer acme", "5_000 USD monthly on 15 into checking"]


def employment(lines, head="contract pay : employment with acme", prelude=RELATED):
    return prelude + head + "\n" + "".join(f"  {line}\n" for line in lines)


def sync(*lines, name="s1", fmt=("date \"Posting Date\" \"MM/DD/YYYY\"", "amount \"Amount\"")):
    text = BOOK + f"sync {name}\n" + "".join(f"  {line}\n" for line in lines)
    if fmt is not None:
        text += "  format csv\n" + "".join(f"    {line}\n" for line in fmt)
    return text


def fmt(*lines):
    return sync('read "a.csv"', fmt=lines)


CASES = [
    # a valid book of every shape below: nothing is wrong
    ("ok-lease", None, contract("area 1_000 SQF", "share 120 SQF for acme", "grace 5d", "rising 3% yearly")),
    ("ok-loan", None, loan(LOAN, "resets 1y from 2027-01-01 to cpi + 2% cap 2% life 5%", "prepay recasts")),
    ("ok-employment", None, employment(PAID)),
    ("ok-format", None, fmt("date \"Posting Date\" \"MM/DD/YYYY\"", "amount \"Amount\"", "memo \"Description\"")),
    # loan: the line (lower/contracts.rs loan_fields, loan_principal, loan_asset, contract_loan)
    ("loan-twice", "duplicate-loan", loan(LOAN).replace(f"  {LOAN}\n", f"  {LOAN}\n  {LOAN}\n")),
    ("loan-empty", "contract-loan", loan("loan")),
    ("loan-principal-word", "contract-loan-principal", loan("loan big on 2026-01-01 at 5% over 10y")),
    ("loan-principal-unit", "unknown-commodity", loan("loan 100_000 XYZ on 2026-01-01 at 5% over 10y")),
    ("loan-principal-zero", "contract-loan-principal", loan("loan 0 USD on 2026-01-01 at 5% over 10y")),
    ("loan-short", "contract-loan", loan("loan 100_000 USD on 2026-01-01 at 5%")),
    ("loan-long", "contract-loan", loan("loan 100_000 USD on 2026-01-01 at 5% over 10y for")),
    ("loan-word-on", "contract-loan", loan("loan 100_000 USD in 2026-01-01 at 5% over 10y")),
    ("loan-word-at", "contract-loan", loan("loan 100_000 USD on 2026-01-01 by 5% over 10y")),
    ("loan-word-over", "contract-loan", loan("loan 100_000 USD on 2026-01-01 at 5% for 10y")),
    ("loan-date", "contract-loan-date", loan("loan 100_000 USD on soon at 5% over 10y")),
    ("loan-rate-number", "contract-loan-rate", loan("loan 100_000 USD on 2026-01-01 at 5 over 10y")),
    # a minus before a rate is an argument of its own: the shape is wrong before the rate is read
    ("loan-rate-minus", "contract-loan", loan("loan 100_000 USD on 2026-01-01 at -5% over 10y")),
    ("loan-term-number", "contract-loan-term", loan("loan 100_000 USD on 2026-01-01 at 5% over 10")),
    ("loan-term-zero", "contract-loan-term", loan("loan 100_000 USD on 2026-01-01 at 5% over 0d")),
    ("loan-asset-word", "contract-loan-asset", loan(LOAN + " to house")),
    ("loan-asset-value", "contract-loan-asset", loan(LOAN + " for 5%")),
    ("loan-asset-unknown", "contract-loan-asset", loan(LOAN + " for boat")),
    ("loan-party", "contract-loan-party", loan(LOAN, head="contract mortgage with me")),
    ("loan-nested-unknown", "contract-loan-property", loan(LOAN, "balloon 5y")),
    # loan: prepay (loan_prepay)
    ("prepay-twice", "duplicate-prepayment-rule", loan(LOAN, "prepay shortens", "prepay recasts")),
    ("prepay-word", "contract-loan-prepay", loan(LOAN, "prepay sometimes")),
    ("prepay-empty", "contract-loan-prepay", loan(LOAN, "prepay")),
    ("prepay-extra", "contract-loan-prepay", loan(LOAN, "prepay shortens now")),
    # loan: resets (loan_resets, reset_schedule, reset_index, reset_limits)
    ("resets-twice", "duplicate-reset-rule",
     loan(LOAN, "resets 1y from 2027-01-01 to cpi + 2%", "resets 1y from 2028-01-01 to cpi + 2%")),
    ("resets-empty", "contract-loan-resets", resets("resets")),
    ("resets-interval-number", "contract-loan-resets", resets("resets 5 from 2027-01-01 to cpi + 2%")),
    ("resets-interval-zero", "contract-loan-resets", resets("resets 0d from 2027-01-01 to cpi + 2%")),
    ("resets-word-from", "contract-loan-resets", resets("resets 1y on 2027-01-01 to cpi + 2%")),
    ("resets-no-date", "contract-loan-resets", resets("resets 1y from")),
    ("resets-date-word", "contract-loan-resets", resets("resets 1y from soon to cpi + 2%")),
    ("resets-date-early", "contract-loan-resets", resets("resets 1y from 2025-01-01 to cpi + 2%")),
    ("resets-word-to", "contract-loan-resets", resets("resets 1y from 2027-01-01 at cpi + 2%")),
    ("resets-no-index", "contract-loan-resets", resets("resets 1y from 2027-01-01 to")),
    ("resets-index-only", "contract-loan-resets", resets("resets 1y from 2027-01-01 to 2%")),
    ("resets-margin-number", "contract-loan-resets", resets("resets 1y from 2027-01-01 to cpi + 2")),
    ("resets-margin-minus", "contract-loan-resets", resets("resets 1y from 2027-01-01 to cpi - 2%")),
    ("resets-margin-negative", "contract-loan-resets", resets("resets 1y from 2027-01-01 to cpi + -2%")),
    ("resets-index-unknown", "unknown-param", resets("resets 1y from 2027-01-01 to nope + 2%")),
    ("resets-index-unit", "contract-loan-index-unit", resets("resets 1y from 2027-01-01 to fee + 2%")),
    ("resets-limit-word", "contract-loan-resets", resets("resets 1y from 2027-01-01 to cpi + 2% floor 1%")),
    ("resets-limit-empty", "contract-loan-resets", resets("resets 1y from 2027-01-01 to cpi + 2% cap")),
    ("resets-limit-number", "contract-loan-resets", resets("resets 1y from 2027-01-01 to cpi + 2% cap 2")),
    ("resets-limit-negative", "contract-loan-resets", resets("resets 1y from 2027-01-01 to cpi + 2% life -2%")),
    ("resets-limit-twice", "contract-loan-resets", resets("resets 1y from 2027-01-01 to cpi + 2% cap 2% cap 3%")),
    # a contract's days (contract_days)
    ("dates-from-twice", "duplicate-start-date", contract("from 2026-02-01")),
    ("dates-until-twice", "duplicate-end-date", contract("until 2026-06-01", "until 2026-07-01")),
    ("dates-from-word", "contract-date", contract(schedule=["500 USD monthly on 1 from checking", "from soon"])),
    ("dates-until-two", "contract-date", contract("until 2026-06-01 2026-07-01")),
    ("dates-range", "contract-range", contract("until 2025-06-01")),
    # grace (grace_property, span_property)
    ("grace-twice", "duplicate-grace-interval", contract("grace 3d", "grace 4d")),
    ("grace-number", "contract-span", contract("grace 5")),
    ("grace-two", "contract-span", contract("grace 5d 6d")),
    ("grace-minus", "contract-span", contract("grace -5d")),
    # for last (relative_property)
    ("period-week", "contract-period", contract("for last week")),
    ("period-bare", "contract-period", contract("for month")),
    # covers (coverage_property)
    ("covers-number", "contract-covers", contract("covers 5")),
    ("covers-week", "contract-covers", contract("covers the week")),
    ("covers-empty", "contract-covers", contract("covers")),
    # rising and indexed (escalation_property)
    ("rising-empty", "contract-escalation", contract("rising")),
    ("rising-no-yearly", "contract-rate", contract("rising 3%")),
    ("rising-number", "contract-rate", contract("rising 3 yearly")),
    ("rising-negative", "contract-rate", contract("rising -3% yearly")),
    ("indexed-empty", "contract-escalation", contract("indexed")),
    ("indexed-no-to", "contract-index", contract("indexed cpi yearly")),
    ("indexed-no-yearly", "contract-index", contract("indexed to cpi")),
    ("indexed-value", "contract-index", contract("indexed to 3% yearly")),
    ("indexed-unknown", "unknown-param", contract("indexed to nope yearly")),
    # area (contract_area)
    ("area-twice", "duplicate-area", contract("area 1_000 SQF", "area 2_000 SQF")),
    ("area-two", "contract-area", contract("area 1_000 SQF 2_000 SQF")),
    ("area-word", "contract-area", contract("area big")),
    ("area-number", "contract-area", contract("area 1_000")),
    ("area-unknown-unit", "unknown-commodity", contract("area 1_000 XYZ")),
    ("area-not-measure", "contract-area-unit", contract("area 1_000 USD")),
    ("area-zero", "contract-area-positive", contract("area 0 SQF")),
    # deposit (contract_deposit, deposit_amount, holding_name, deposit_holding)
    ("deposit-twice", "duplicate-deposit", contract("deposit 500 USD", "deposit 600 USD")),
    ("deposit-two", "contract-deposit", contract("deposit 500 USD into")),
    ("deposit-empty", "contract-deposit", contract("deposit")),
    ("deposit-word", "contract-deposit-amount", contract("deposit nothing")),
    ("deposit-zero", "contract-deposit-positive", contract("deposit 0 USD")),
    ("deposit-unit", "unknown-commodity", contract("deposit 500 XYZ")),
    ("deposit-no-holding", "contract-deposit-holding-required",
     contract("deposit 500 USD", schedule=["from 2026-01-01"])),
    ("deposit-holding-value", "contract-deposit-holding", contract("deposit 500 USD into 5%")),
    ("deposit-holding-word", "contract-deposit-holding", contract("deposit 500 USD in savings")),
    ("deposit-holding-asset", "asset-endpoint", contract("deposit 500 USD into house")),
    ("deposit-holding-party", "contract-deposit-holding", contract("deposit 500 USD into shop")),
    ("deposit-holding-unknown", "unknown-entity", contract("deposit 500 USD into vault")),
    ("deposit-holding-owner", "contract-deposit-owner", contract("deposit 500 USD into escrow")),
    ("deposit-holding-unit", "contract-deposit-unit", contract("deposit 5 VTI into wallet")),
    # share (shares, read_share_line, share_rate, measured_numerator, share_owner, add_share)
    ("share-empty", "contract-share", contract("share")),
    ("share-word", "contract-share", contract("share most for acme")),
    ("share-negative", "contract-share", contract("share -10% for acme")),
    ("share-no-for", "contract-share", contract("share 10%")),
    ("share-word-to", "contract-share", contract("share 10% to acme")),
    ("share-no-owner", "contract-share", contract("share 10% for")),
    ("share-owner-value", "contract-share", contract("share 10% for 5%")),
    ("share-owner-unknown", "unknown-entity", contract("share 10% for nobody")),
    ("share-total", "contract-share-total", contract("share 60% for acme", "share 50% for shop")),
    ("share-measure-no-area", "contract-share-measure", contract("share 120 SQF for acme")),
    ("share-measure-unit", "contract-share-unit", contract("area 1_000 SQF", "share 120 USD for acme")),
    # The baseline said this three times over (the measure, the rate, the unit); U4 says the cause, once.
    ("share-measure-other", "contract-share-unit", contract("area 1_000 SQF", "share 5 VTI for acme")),
    ("share-fraction-zero", "zero-fraction", contract("share 1/0 for acme")),
    # input (lower.rs inputs)
    ("input-empty", "contract-input", contract("input")),
    ("input-three", "contract-input", contract("input hours USD now")),
    ("input-value", "contract-input", contract("input 5")),
    ("input-twice", "duplicate-input", contract("input hours", "input hours")),
    ("input-unit-value", "contract-input-unit", contract("input hours 5%")),
    ("input-unit-unknown", "unknown-commodity", contract("input hours XYZ")),
    # a loan's rate from a day (statements.rs lower_rate_change)
    ("rate-no-loan", "contract-rate-change", contract() + "2026-06-01 lease now at 6.25%\n"),
    ("rate-number", "contract-loan-rate", loan() + "2026-06-01 mortgage now at 6\n"),
    ("rate-negative", "contract-loan-rate", loan() + "2026-06-01 mortgage now at -6%\n"),
    ("rate-two", "contract-loan-rate", loan() + "2026-06-01 mortgage now at 6% 7%\n"),
    # a relator's slots (lower/contracts/relator.rs)
    ("relator-slot-unknown", "relator-slot-unknown", employment(["employe me", "employer acme", PAID[2]])),
    ("relator-slot-twice", "relator-slot-twice", employment(["employee me", "employer acme", "employer acme", PAID[2]])),
    ("relator-slot-kind", "relator-slot-kind", employment(["employee me", "employer alex", PAID[2]])),
    ("relator-slot-missing", "relator-slot-missing", employment(["employee me", PAID[2]])),
    ("relator-kind-sort", "relator-kind-sort", employment(PAID, head="contract pay : person with acme")),
    ("relator-slot-unfilled-name", "unknown-entity", employment(["employee nobody", "employer acme", PAID[2]])),
    # a format's lines (sync_lower.rs format_shape, FormatReader)
    ("format-records-two", "bad-format", fmt("records STMTTRN BANKTRAN", "date DTPOSTED", "amount TRNAMT")),
    ("format-records-twice", "bad-format", fmt("records STMTTRN", "records BANKTRAN", "date DTPOSTED", "amount TRNAMT")),
    ("format-escape", "bad-string-escape", fmt("date \"Posting\\q Date\"", "amount \"Amount\"")),
    ("format-category", "bad-format", fmt("date \"Posting Date\"", "amount \"Amount\"", "category \"Food\" #food")),
    ("format-unknown-field", "unknown-format-field", fmt("date \"Posting Date\"", "amount \"Amount\"", "colour 3")),
    ("format-field-twice", "duplicate-format-field", fmt("date \"Posting Date\"", "amount \"Amount\"", "amount 4")),
    ("format-field-empty", "bad-format", fmt("date \"Posting Date\"", "amount")),
    ("format-column-zero", "bad-format", fmt("date 0", "amount \"Amount\"")),
    ("format-date-three", "bad-format", fmt("date 1 \"MM/DD/YYYY\" \"DD.MM.YYYY\"", "amount 2")),
    ("format-date-layout", "bad-date-layout", fmt("date 1 \"sometime\"", "amount 2")),
    ("format-amount-rule", "bad-format", fmt("date 1", "amount 2 backwards")),
    ("format-amount-sign", "bad-format", fmt("date 1", "amount 2 sign 3")),
    ("format-party-two", "bad-format", fmt("date 1", "amount 2", "party 3 4")),
    ("format-payee", "unknown-format-field", fmt("date 1", "amount 2", "payee 3")),
    ("format-no-amount", "bad-format", fmt("date 1")),
    ("format-no-date", "bad-format", fmt("amount 2")),
    # a source's sink and feed (sync_lower.rs source_sink, source_feed)
    ("sync-into-param", "sync-sink", sync('read "a.csv"', "into param")),
    ("sync-into-param-unknown", "unknown-param", sync('read "a.csv"', "into param nope")),
    ("sync-feed-asset", "sync-feed", sync('read "a.csv"', name="house")),
    ("sync-feed-no-format", "sync-format", sync('read "a.csv"', name="checking", fmt=None)),
]

# The built-in properties (props.rs BUILTINS, read by `Args`): for each, the line under the thing it is written under,
# with nothing after it, with the wrong kind of value, and with one value too many.
BUILTINS = [
    # (property, under, a good line, a value of the wrong kind)
    ("holds", "account purse : bank", "holds USD", "holds 5%"),
    ("select", "account brokerage : bank", "select fifo", "select 5%"),
    ("opened", "account old : bank", "opened 2020-01-01", "opened soon"),
    ("closed", "account old : bank", "closed 2026-01-01", "closed soon"),
    ("liquidity", "account cd : bank", "liquidity 30d", "liquidity 30"),
    ("via", "entity kid : person", "via savings", "via 5%"),
    ("lives", "entity kid : person", "lives us", "lives 5%"),
    ("member", "entity kid : person", "member me", "member 5%"),
    ("currency", "entity kid : person", "currency USD", "currency 5%"),
    ("citizen", "entity kid : person", "citizen us", "citizen 5%"),
    ("books", "entity kid : person", "books cash", "books barter"),
    ("purpose", "kind gadget : asset", "purpose groceries", "purpose 5%"),
    ("pays", "kind plan : deposit", "pays groceries", "pays 5%"),
    ("takes", "kind plan : deposit", "takes groceries from dining", "takes groceries to dining"),
    ("sales-tax", "kind shopkind : org", "sales-tax 8%", "sales-tax 8"),
    ("share", "kind pool : deposit", "share 50% for me", "share half for me"),
    ("part", "asset roof : vehicle", "part of house", "part in house"),
    ("precision", "commodity PTS : stock", "precision 2", "precision 99"),
    ("name", "commodity PTS : stock", "name \"Points\"", "name points"),
    ("grows", "commodity PTS : stock", "grows 3% yearly", "grows 3 yearly"),
    ("basis", "kind wrapper : deposit", "basis zero", "basis some"),
]
FLAGS = [("restricted", "kind locked : deposit"), ("deferred", "kind later : deposit"), ("claim", "kind owed : deposit")]


# Where a built-in says something else than the generic `property-argument` or `property-type`: `holds`, `lives` and
# `citizen` read every argument, so one too many is a value of the wrong kind; `share` checks its own rate.
SAYS = {
    "builtin-holds-extra": "property-type",
    "builtin-lives-extra": "property-type",
    "builtin-citizen-extra": "unknown-system",
    "builtin-share-type": "share-rate",
}


def builtin_cases():
    cases = []
    for prop, decl, good, wrong in BUILTINS:
        for name, code, line in [
            (f"builtin-{prop}-empty", "property-argument", prop),
            (f"builtin-{prop}-type", "property-type", wrong),
            (f"builtin-{prop}-extra", "property-argument", good + " now"),
        ]:
            cases.append((name, SAYS.get(name, code), under(decl, line)))
    for prop, decl in FLAGS:
        cases.append((f"builtin-{prop}-extra", "property-argument", under(decl, prop + " now")))
    return cases


def all_cases():
    return CASES + builtin_cases()


def write():
    os.makedirs(OUT, exist_ok=True)
    for old in os.listdir(OUT):
        if old.startswith("decl-") and old.endswith(".ax"):
            os.remove(os.path.join(OUT, old))
    for name, code, text in all_cases():
        said = code or "nothing"
        with open(os.path.join(OUT, f"decl-{name}.ax"), "w") as out:
            out.write(f"// raises {said}\n{text}")
    print(f"{len(all_cases())} cases in {OUT}")


CODE = re.compile(r"^(?:error|warning)\[([a-z0-9-]+)\]", re.M)


def check(binary):
    wrong = 0
    for name, code, _ in all_cases():
        path = os.path.join(OUT, f"decl-{name}.ax")
        run = subprocess.run([binary, "check", path, "--today", "2026-06-30", "--color", "never"],
                             capture_output=True, text=True, timeout=60)
        said = CODE.findall(run.stdout + run.stderr)
        errors = [c for c in said if c]
        if code is None and not any(line.startswith("error") for line in (run.stdout + run.stderr).splitlines()):
            continue
        if code is not None and code in errors:
            continue
        wrong += 1
        print(f"decl-{name}: wants {code or 'no error'}, says {', '.join(errors) or 'nothing'}")
    print(f"{len(all_cases()) - wrong} of {len(all_cases())} cases raise what they claim")
    return wrong


if __name__ == "__main__":
    if sys.argv[1:2] == ["write"]:
        write()
    elif sys.argv[1:2] == ["check"] and len(sys.argv) == 3:
        sys.exit(1 if check(sys.argv[2]) else 0)
    else:
        print(__doc__)
        sys.exit(2)
