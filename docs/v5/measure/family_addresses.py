#!/usr/bin/env python3
"""The acceptance of lane K3b: `examples/05-family` written with addresses, and the proof it is the same book.

    family_addresses.py write                      write the copy into examples/explore-v5/06-family-addresses
    family_addresses.py prove BINARY [DAY]         run both books through BINARY and compare what they say

`examples/05-family/accounts.ax` names its accounts after what they are related to (`jordan-401k`, `riley-529`,
`joint-checking`) and then says the relation again on a line under each (`owner jordan`, `employer bluefin`,
`beneficiary riley`), and nothing checks that the two agree. The copy writes each account once, as the entities that fill
its slots and then its name, and every reference of the journal, the contracts and the code rules as the shortest address
that stays unique among all the accounts the book ever has:

    account jordan-401k : 401k at fidelity          account jordan/bluefin/401k at fidelity
      owner jordan
      employer bluefin                          ->  2 lines of 3 written relations gone, and nothing to disagree with

The copy is a rewrite by this script of the original, which is not edited: the goldens read it.

`prove` runs the commands of the goldens over both with `--json`, and holds the copy to the original: the same
diagnostics (its 141 errors are the v3 syntax every example still carries, and are the same errors in the same lines), the
same balances, the same claims, the same counted tallies, the same limits, the same tax. The two books name their accounts
differently, so the original's output is read with each account called what the copy calls it, and the order of rows,
which follows the order of names, is not compared.
"""
import json
import os
import re
import shutil
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", "..", ".."))
SOURCE = os.path.join(ROOT, "examples", "05-family")
COPY = os.path.join(ROOT, "examples", "explore-v5", "06-family-addresses")

# The accounts, as the original names them; the header the copy writes in its place (the lines under the header go: an
# address says them), the path it is written at, and the shortest reference that is unique among all of them.
ACCOUNTS = [
    ("joint-checking", "family/checking : deposit at chase", [], "family/checking", "checking"),
    ("joint-savings", "family/savings : deposit at chase", [], "family/savings", "savings"),
    ("escrow", "family/escrow at lender", [], "family/escrow", "escrow"),
    ("alex-401k", "me/acme/401k at fidelity", [], "me/acme/401k", "me/401k"),
    ("jordan-401k", "jordan/bluefin/401k at fidelity", [], "jordan/bluefin/401k", "jordan/401k"),
    ("hsa", "me/hsa at fidelity", ["  coverage family"], "me/hsa", "hsa"),
    ("dcfsa", "family/dcfsa : dependent-care-fsa at acme", [], "family/dcfsa", "dcfsa"),
    ("riley-529", "family/riley/529 : 529-plan at fidelity", [], "family/riley/529", "riley/529"),
    ("mortgage", "family/mortgage at lender", [], "family/mortgage", "mortgage"),
    ("car-loan", "family/car-loan : loan at honda-finance", [], "family/car-loan", "car-loan"),
    ("card", "family/card : credit-card at chase", [], "family/card", "card"),
]
BY_OLD = {old: (header, lines, path, short) for old, header, lines, path, short in ACCOUNTS}

# Where a name stands as a name and not as a word of prose, a kind or a purpose: not after `#`, `^`, `/`, `-`, `.`, a
# letter or a digit, and not before one, or before `-`, `/` or `:`.
BOUNDARY = r"(?<![\w/#^.:-])({})(?![\w/:-])"


def rewrite(text):
    """Every reference of a name that the copy writes differently, as the address that means only it."""
    changed = {old: short for old, (_, _, _, short) in BY_OLD.items() if short != old}
    pattern = re.compile(BOUNDARY.format("|".join(re.escape(old) for old in sorted(changed, key=len, reverse=True))))
    return pattern.sub(lambda m: changed[m.group(1)], text)


ABOUT = """\
// Each account is written once, as the entities that fill its slots and then its name. `jordan/bluefin/401k` is
// jordan's 401(k), sponsored by bluefin: the original called it `jordan-401k` and said `owner jordan` and `employer
// bluefin` on two lines under it, which nothing checked against the name. A word is placed in the slot its entity's kind
// fits, and the journal says the shortest address that means one account: `checking`, `me/401k`, `jordan/401k`,
// `riley/529`. `at` is the custodian: a path word cannot say it until a `has` line can say `as with`.
"""


def explain(text):
    """The comment that says what the accounts are, above the first of them."""
    first = text.index("\naccount ") + 1
    return text[:first] + ABOUT + text[first:]


def rewrite_accounts(text):
    """The `account` blocks of accounts.ax: a header and the indented lines under it, replaced by one header, and the
    lines the address cannot say."""
    out, lines, i = [], text.split("\n"), 0
    while i < len(lines):
        line = lines[i]
        match = re.match(r"account (\S+)", line)
        if not match or match.group(1) not in BY_OLD:
            out.append(line)
            i += 1
            continue
        header, extra, _, _ = BY_OLD[match.group(1)]
        i += 1
        while i < len(lines) and lines[i].startswith("  "):
            i += 1
        out += [f"account {header}"] + extra
    return "\n".join(out)


def write():
    if os.path.exists(COPY):
        shutil.rmtree(COPY)
    shutil.copytree(SOURCE, COPY, ignore=shutil.ignore_patterns("outputs", "README.md"))
    for folder, _, files in os.walk(COPY):
        for name in files:
            if name.endswith(".ax"):
                path = os.path.join(folder, name)
                text = open(path).read()
                text = explain(rewrite(rewrite_accounts(text))) if name == "accounts.ax" else rewrite(text)
                open(path, "w").write(text)
    with open(os.path.join(COPY, "README.md"), "w") as out:
        out.write(README)
    print(f"wrote {os.path.relpath(COPY, ROOT)}")


README = """\
# 05 — A family ledger, with its accounts written as addresses

`examples/05-family` with one thing changed: each account is written once, as the entities that fill its slots and then
its name, and every reference to one is the shortest address that means only it. It is generated from the original by
`docs/v5/measure/family_addresses.py write`; the original is not edited, and the goldens read it.

```text
// 05-family                                          // here
account jordan-401k : 401k at fidelity                account jordan/bluefin/401k at fidelity
  owner jordan
  employer bluefin
account riley-529 : 529-plan at fidelity              account family/riley/529 : 529-plan at fidelity
  owner family
  beneficiary riley
account joint-checking : deposit at chase             account family/checking : deposit at chase
  owner family
```

`jordan/bluefin/401k` says that jordan owns it and bluefin sponsors it, once. The name `jordan-401k` said it in a
string nothing checked against the `owner jordan` line under it. Each word is placed in the slot its entity's kind
fits: `jordan` is a person and can only be the owner, so `bluefin`, an employer, is the 401(k)'s `employer`. `family`
is a household and can only be the owner of the 529, so `riley` is its beneficiary. A word that could fill two slots
and that no other word settles is `ambiguous-placement`, and a role line (`employer bluefin`) settles it.

The journal says `checking` for the family's checking account, and `me/401k` and `jordan/401k` for the two 401(k)s,
which need the owner to tell them apart. Each reference is the shortest address that is unique among all the accounts
the book has, so it stays unique as the book grows. `riley/529` and not `529`: a word of digits alone is a number.

The accounts' `at` is how this book says who holds them. It is the one relation still written the long way: a kind
can say which slot a word in the path fills as the custodian only when a `has` line can say `as with`.

`python3 docs/v5/measure/family_addresses.py prove target/release/axiom` runs both books and shows they say the same: the
same 141 diagnostics, balances, claims, tallies, limits and tax.
"""


# ─── The proof ──────────────────────────────────────────────────────────────────────────────────────────

COMMANDS = [["check"], ["balance"], ["available"], ["limits"], ["claims"], ["tax", "2025"], ["flow"], ["lots"],
            ["contracts"], ["balance", "--monthly"], ["budget", "2025-05"]]
DROPPED = {"column", "start_byte", "end_byte", "end_column", "file_id", "depth", "style"}
SORTED = {"rows", "facts", "labels"}


def run(binary, project, args, day):
    done = subprocess.run([binary, *args, "-C", project, "--today", day, "--color", "never", "--json"],
                          capture_output=True, text=True, timeout=300)
    return done.stdout + done.stderr


def as_documents(output):
    docs = []
    for line in output.splitlines():
        if line.startswith("{"):
            docs.append(json.loads(line))
    return docs


def names_to_old(text):
    """What the copy says of an account, as the original names it: its path, which is what the tool prints."""
    for old, (_, _, path, short) in sorted(BY_OLD.items(), key=lambda item: -len(item[1][2])):
        text = re.sub(r"(?<![\w/#^.:-])" + re.escape(path) + r"(?![\w/:-])", old, text)
    return text


def normalize(value, copy):
    if isinstance(value, dict):
        kept = {k: normalize(v, copy) for k, v in value.items() if k not in DROPPED}
        for key in SORTED & kept.keys():
            kept[key] = sorted(kept[key], key=lambda item: json.dumps(item, sort_keys=True))
        return kept
    if isinstance(value, list):
        return [normalize(v, copy) for v in value]
    if isinstance(value, str) and copy:
        return names_to_old(value)
    return value


def moved_lines(docs):
    """The diagnostics, with no line for what they point at in accounts.ax: the lines under each account went."""
    for doc in docs:
        for label in doc.get("labels", []):
            if str(label.get("file", "")).endswith("accounts.ax"):
                label.pop("line", None)
    return docs


def prove(binary, day="2026-04-16"):
    wrong = 0
    for args in COMMANDS:
        old = as_documents(run(binary, SOURCE, args, day))
        new = as_documents(run(binary, COPY, args, day))
        split = lambda docs: ([d for d in docs if "code" in d], [d for d in docs if "code" not in d])
        (old_said, old_report), (new_said, new_report) = split(old), split(new)
        left = normalize(moved_lines(old_said), copy=False), normalize(old_report, copy=False)
        right = normalize(moved_lines(new_said), copy=True), normalize(new_report, copy=True)
        key = lambda doc: json.dumps(doc, sort_keys=True)
        left = sorted(left[0], key=key), left[1]
        right = sorted(right[0], key=key), right[1]
        same = left == right
        print(f"{'same' if same else 'DIFFERENT'}  {' '.join(args):<22} {len(old_said)} diagnostics, {len(old_report)} report")
        if not same:
            wrong += 1
            for a, b in list(zip(left[0], right[0])) + list(zip(left[1], right[1])):
                if a != b:
                    print("  original:", json.dumps(a, sort_keys=True)[:700])
                    print("  copy:    ", json.dumps(b, sort_keys=True)[:700])
                    break
            else:
                print(f"  {len(left[0])}+{len(left[1])} documents against {len(right[0])}+{len(right[1])}")
    print("the copy says what the original says" if not wrong else f"{wrong} commands differ")
    return wrong


def main(argv):
    if len(argv) >= 2 and argv[1] == "write":
        write()
        return 0
    if len(argv) >= 3 and argv[1] == "prove":
        return 1 if prove(argv[2], *(argv[3:4])) else 0
    print(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
