#!/usr/bin/env python3
"""A generator of small projects full of claims, loans and parties, and a differential run of two builds of the CLI.

    tabs.py gen DIR N [SEED]                  write N projects into DIR (p0000/axiom.ax, journal/..), DIR/forms.json
    tabs.py run BASELINE NEW DIR [JOBS]       run the commands below over every project through both binaries
    tabs.py all BASELINE NEW DIR N [SEED]     gen, then run

What it is for. Lane K3a makes a claim tab, which a claim, a loan or a `for` clause needs as a place, exist when
the lowering asks for it and not when a survey of the journal predicts it. The tab is a place; where it stands in
the place tree decides the order a report lists it in, and what the ledger folds. `docs/v5/measure/diff/` and
`fuzz.py` mutate books that have a claim or two. This writes books that are all claims, in the orders where a
predicted tab and a lazily made one come apart:

    parties     declared, undeclared (implied by being written), nested (`shops/mill`), an owner other than `me`
                (who holds what it owns: the other side of a claim is then a `Debt` tab), a contract named for its
                own party (`contract quill`, no `with`)
    claims      `X owes me`, `me owes X`, `X owes pat`, `pat owes X`, `X owes Y` (neither holds), with and without
                `due`, a code, items; in an `opening`; written off (`^c waived`); settled (`X -> checking .. for ^c`)
    mentions    flows with `due`, `for WHOM`, `via PARTY` that predict a tab no claim ever asks for
    loans       `loan` contracts paid from an account of `me`, of `pat`, or by a suffix (`pat-save`), whose name is a
                flow end in a template (declared before and after the loan) and in the journal
    order       blocks are put in random files, in random order, so the text order of a claim and its date disagree:
                the predicted tab is made in text order, a lazy one in the order the journal is lowered in

Every project is run through `check`, `balance`, `balance --value`, `lots`, `claims`, `contracts`, `flow`,
`available`, `gains`, the registers of every party and account, and `why` of some lines, with and without `--json`
where the form differs. The run reports every project on which the two builds differ in stdout, stderr or exit
status, in two classes:

    places    the only difference is the number of places the `check` summary counts: a predicted tab nobody
              asked for is a place the summary counted (a tab has a source line, and so counts as declared)
    missed    the BASELINE said `unregistered-tab`: its survey predicted a tab with the wrong owner (a loan paid from
              `joint`, which is `assets/joint`, was a loan of `me`) and the lowering asked for the right one
    other     anything else

and, from what the BASELINE printed, how many projects were clean (no error), moved money, and held each form, so
that a generator whose books are all rejected would be seen.
"""
import hashlib
import json
import os
import random
import re
import subprocess
import sys
from collections import Counter
from concurrent.futures import ThreadPoolExecutor

TODAY = "2026-06-30"

ACCOUNTS = """\
use std
base USD
entity me : person
entity pat : person
entity ann : org
entity bob : org
entity cy : org
account checking : bank
account savings : bank
account assets/joint : bank
  owner pat
account assets/pat-save : bank
  owner pat
"""

# Declared, undeclared and nested parties; `me` and `pat` hold what they own.
DECLARED = ["ann", "bob", "cy"]
IMPLIED = ["zed", "quill", "vera", "shops/mill"]
OWNERS = ["me", "pat"]
PARTIES = DECLARED + IMPLIED
HOLDINGS = ["checking", "savings", "joint", "pat-save"]


class Book:
    """The blocks of one project, each with the file it goes to, and the forms they use."""

    def __init__(self, rng):
        self.rng = rng
        self.blocks = []
        self.forms = Counter()
        self.codes = 0
        self.contracts = []
        self.loans = []

    def add(self, text, *forms, day=None):
        self.blocks.append((day or "2026-01-01", text.rstrip("\n")))
        for form in forms:
            self.forms[form] += 1

    def code(self):
        self.codes += 1
        return f"^c{self.codes}"

    def day(self, month=None):
        month = month or self.rng.randint(1, 5)
        return f"2026-{month:02d}-{self.rng.randint(2, 27):02d}"

    def party(self):
        return self.rng.choice(PARTIES)

    def usd(self, low=5, high=300):
        return f"{self.rng.randint(low, high)} USD"


def due(book):
    return book.rng.choice(["", "", " due 30d", " due 2026-05-10", " due 10d"])


def claim(book):
    """One claim between two parties, in each of the five shapes the holder test of `lower_owes` tells apart."""
    rng, forms = book.rng, []
    x, y = book.party(), book.party()
    shape = rng.choice(["x-me", "me-x", "x-pat", "pat-x", "x-y", "me-pat", "pat-me"])
    debtor, creditor = {"x-me": (x, "me"), "me-x": ("me", x), "x-pat": (x, "pat"), "pat-x": ("pat", x),
                        "x-y": (x, y), "me-pat": ("me", "pat"), "pat-me": ("pat", "me")}[shape]
    if debtor == creditor:
        return
    forms.append("claim:" + shape)
    mark = book.code() if rng.random() < 0.5 else ""
    day = book.day()
    if rng.random() < 0.25:
        lines = [f"{day} {debtor} owes {creditor}{due(book)} {mark}".rstrip()]
        for _ in range(rng.randint(1, 3)):
            lines.append(f"  {book.usd()}" + rng.choice(["", " #fun", ' "a part"']))
        forms.append("claim:items")
        book.add("\n".join(lines), *forms, day=day)
        return
    tail = rng.choice(["", " #fun", ' "a note"'])
    book.add(f"{day} {debtor} owes {creditor} {book.usd()}{due(book)}{tail} {mark}".rstrip(), *forms, day=day)
    if mark and rng.random() < 0.6:
        later = book.day(rng.randint(3, 6))
        if rng.random() < 0.5 and creditor == "me":
            book.add(f"{later} {debtor} -> checking {book.usd()} against {mark}", "claim:settled", day=later)
        elif rng.random() < 0.5:
            book.add(f"{later} {mark} waived \"forgiven\"", "claim:waived", day=later)


def opening_claims(book):
    rng = book.rng
    lines = ["opening 2025-12-31"]
    for _ in range(rng.randint(1, 3)):
        x = book.party()
        lines.append(f"  {rng.choice([f'{x} owes me', f'me owes {x}', f'{x} owes pat'])} {book.usd()}{due(book)}")
    book.add("\n".join(lines), "claim:opening", day="2025-12-31")


def mentions(book):
    """Flows whose `due`, `for` and `via` predict a tab that no claim asks for."""
    rng, forms = book.rng, ["mention"]
    day, x, y = book.day(), book.party(), book.party()
    hold = rng.choice(HOLDINGS)
    clauses = rng.choice([f" due 30d", f" for {y}", f" via {y}", f" due 20d for {y}", ""])
    forms.append("mention:" + (clauses.split()[0] if clauses else "plain"))
    if rng.random() < 0.7:
        book.add(f"{day} {hold} -> {x} {book.usd()}{clauses} #fun", *forms, day=day)
    else:
        book.add(f"{day} {x} -> {hold} {book.usd()}{clauses} #fun", *forms, day=day)


def loan(book):
    """A loan contract, paid from an account of `me`, of `pat`, or one only a suffix names."""
    rng, forms = book.rng, ["loan"]
    name = f"mortgage{len(book.loans)}"
    party = rng.choice(DECLARED + ["zed"])
    hold = rng.choice(HOLDINGS)
    forms.append("loan:from-" + ("pat" if hold in ("joint", "pat-save") else "me"))
    lines = [f"contract {name} with {party}",
             f"  loan {rng.choice([20_000, 90_000])} USD on 2026-01-01 at {rng.choice(['4', '5.5'])}% over 10y",
             f"  monthly on {rng.randint(1, 28)} from {hold}", "  from 2026-02-01"]
    book.loans.append(name)
    book.add("\n".join(lines), *forms, day="2025-01-01")
    for _ in range(rng.randint(0, 2)):
        day = book.day()
        book.add(f"{day} {hold} -> {name} {book.usd()}", "loan:paid-in-journal", day=day)


def contract(book):
    """A contract that is not a loan: with a party, or named for its party; sometimes a leg ends at a loan's name."""
    rng, forms = book.rng, ["contract"]
    if rng.random() < 0.25:
        name = rng.choice(["quill", "vera"]) + str(len(book.contracts))
        head, party = f"contract {name}", name
        forms.append("contract:no-with")
    else:
        name = f"plan{len(book.contracts)}"
        party = book.party()
        head = f"contract {name} with {party}"
    book.contracts.append(name)
    lines = [head, f"  {book.usd(50, 400)} monthly on {rng.randint(1, 28)} from {rng.choice(HOLDINGS)}",
             "  from 2026-02-01"]
    if rng.random() < 0.4:
        lines.append(f"  {rng.choice(['savings', 'ann', 'bob'])} {book.usd(5, 40)}")
    book.add("\n".join(lines), *forms, day="2025-01-01")
    if book.loans and rng.random() < 0.5:
        target = rng.choice(book.loans)
        # The loan may be declared later than this contract: its name is a flow end either way.
        book.add(f"contract leg{len(book.contracts)} with {rng.choice(DECLARED)}\n"
                 f"  {book.usd(50, 400)} monthly on 3 from checking\n  from 2026-02-01\n"
                 f"  {target} {book.usd(5, 40)}", "contract:leg-to-loan", day="2025-01-01")


RECIPES = [(claim, 18), (opening_claims, 3), (mentions, 8), (loan, 5), (contract, 5)]


def project(seed, index):
    rng = random.Random(seed * 1_000_003 + index)
    book = Book(rng)
    for _ in range(rng.choices([2, 3, 4, 5, 6], [2, 4, 4, 3, 2])[0]):
        recipe = rng.choices([r for r, _ in RECIPES], [w for _, w in RECIPES])[0]
        recipe(book)
    return book


def layout(book):
    """Where each block goes: contracts to their file, the journal to files whose month names do not match the days."""
    rng = book.rng
    files = {"axiom.ax": [ACCOUNTS + "2025-12-31 market -> checking 9_000 USD\n2025-12-31 market -> joint 5_000 USD"],
             "contracts.ax": [], "journal/2026/01.ax": [], "journal/2026/02.ax": [], "journal/2026/03.ax": []}
    for day, text in book.blocks:
        if text.startswith("contract"):
            files["contracts.ax"].append(text)
        elif day.startswith("2025-12"):
            files["journal/2026/01.ax"].append(text)
        else:
            files[rng.choice(["journal/2026/01.ax", "journal/2026/02.ax", "journal/2026/03.ax"])].append(text)
    if rng.random() < 0.5:
        rng.shuffle(files["contracts.ax"])
    for name in files:
        if name.startswith("journal"):
            rng.shuffle(files[name])
    return files


def gen(directory, count, seed):
    all_forms = {}
    for index in range(count):
        book = project(seed, index)
        path = os.path.join(directory, f"p{index:04d}")
        for name, blocks in layout(book).items():
            if not blocks:
                continue
            os.makedirs(os.path.dirname(os.path.join(path, name)), exist_ok=True)
            with open(os.path.join(path, name), "w") as out:
                out.write("\n".join(blocks) + "\n")
        all_forms[f"p{index:04d}"] = dict(book.forms)
    with open(os.path.join(directory, "forms.json"), "w") as out:
        json.dump(all_forms, out, indent=0, sort_keys=True)
    return all_forms


# ─── The differential run ───────────────────────────────────────────────────────────────────────────────────


def sh(binary, args, cwd):
    run = subprocess.run([binary, *args, "--today", TODAY, "--color", "never"], cwd=cwd, capture_output=True,
                         text=True, timeout=120)
    return run.returncode, run.stdout, run.stderr


def commands(path):
    """Every command over one project: the reports, and the register of each party and account it names."""
    parties = DECLARED + [name.split("/")[-1] for name in IMPLIED] + ["pat", "checking", "joint", "savings"]
    work = [["check"], ["balance"], ["balance", "--value"], ["flow"], ["contracts"], ["claims"], ["lots"], ["gains"],
            ["available"], ["limits"], ["claims", "--json"], ["balance", "--json"], ["contracts", "--json"]]
    work += [["register", name] for name in parties]
    work += [["register", "pat", "--json"]]
    for name in sorted(os.listdir(os.path.join(path, "journal", "2026"))) if os.path.isdir(path + "/journal/2026") else []:
        text = open(os.path.join(path, "journal", "2026", name)).read().split("\n")
        written = [n for n in range(1, len(text) + 1) if re.match(r"\d", text[n - 1])]
        work += [["why", f"journal/2026/{name}:{n}"] for n in written[:2]]
    return [[*args, "-C", "."] for args in work]


def baseline_outputs(baseline, path, work):
    """What the BASELINE says to each command, kept beside the project: it never changes, so it is said once."""
    kept = os.path.join(path, "baseline.json")
    digest = hashlib.sha1()
    for root, _, names in sorted(os.walk(path)):
        for name in sorted(names):
            if name.endswith(".ax"):
                digest.update(open(os.path.join(root, name), "rb").read())
    stamp = [os.path.getmtime(baseline), os.path.getsize(baseline), digest.hexdigest()]
    if os.path.exists(kept):
        saved = json.load(open(kept))
        if saved["stamp"] == stamp and saved["commands"] == work:
            return [tuple(said) for said in saved["said"]]
    said = [sh(baseline, args, path) for args in work]
    with open(kept, "w") as out:
        json.dump({"stamp": stamp, "commands": work, "said": said}, out)
    return said


def without_places(said):
    """The `check` summary says how many places there are; a predicted tab nobody asked for was one of them."""
    return tuple(re.sub(r"\b\d+ places\b", "N places", part) if isinstance(part, str) else part for part in said)


def classify(before, after):
    """Which kind of difference this is: the baseline's own survey failing, only the places counted, or something else."""
    if any("unregistered-tab" in part for part in before[1:]):
        return "missed"
    return "places" if without_places(before) == without_places(after) else "other"


def run_project(baseline, new, path):
    """The commands of one project through both binaries: what differs, and what the baseline said."""
    differences, summary = [], {"clean": True, "moves": False}
    work = commands(path)
    said = baseline_outputs(baseline, path, work)
    for index, (args, before) in enumerate(zip(work, said)):
        after = sh(new, args, path)
        if before != after:
            differences.append((args, before, after, classify(before, after)))
        if index == 0:
            summary["clean"] = before[0] == 0 and "error[" not in before[1] + before[2]
        if args[0] == "claims" and "--json" not in args:
            summary["moves"] |= "Counterparty" in before[1]
    return path, differences, summary


def run(baseline, new, directory, jobs=3):
    paths = sorted(os.path.join(directory, name) for name in os.listdir(directory) if name.startswith("p"))
    forms = json.load(open(os.path.join(directory, "forms.json")))
    with ThreadPoolExecutor(jobs) as pool:
        results = list(pool.map(lambda path: run_project(baseline, new, path), paths))
    kinds = {path: {d[3] for d in diff} for path, diff, _ in results}
    other = [(path, diff) for path, diff, _ in results if "other" in kinds[path]]
    missed = [path for path, found in kinds.items() if "missed" in found and "other" not in found]
    places = [path for path, found in kinds.items() if found == {"places"}]
    for path, diff in other[:5]:
        args, before, after, _ = next(d for d in diff if d[3] == "other")
        print(f"DIFFERENT {path}: {' '.join(args)}\n--- baseline (exit {before[0]})\n{before[1]}{before[2]}\n"
              f"--- new (exit {after[0]})\n{after[1]}{after[2]}")
    covered, states = Counter(), Counter()
    for path, _, summary in results:
        name = os.path.basename(path)
        states["projects"] += 1
        states["clean"] += summary["clean"]
        states["claims open"] += summary["moves"]
        for form in forms.get(name, {}):
            covered[form, "all"] += 1
            covered[form, "clean"] += summary["clean"]
    print(f"{states['projects']} projects, {sum(len(commands(p)) for p in paths)} commands: "
          f"{len(other)} differ in something else, {len(missed)} had a survey miss in the baseline, "
          f"{len(places)} differ only in the number of places")
    print(f"clean (no error) {states['clean']}, with an open claim {states['claims open']}")
    width = max((len(form) for form, _ in covered), default=0)
    print(f"{'form':<{width}}  {'all':>5} {'clean':>6}")
    for form in sorted({form for form, _ in covered}):
        print(f"{form:<{width}}  {covered[form, 'all']:>5} {covered[form, 'clean']:>6}")
    return len(other)


def main(argv):
    if len(argv) >= 4 and argv[1] == "gen":
        forms = gen(argv[2], int(argv[3]), int(argv[4]) if len(argv) > 4 else 1)
        total = Counter()
        for one in forms.values():
            total.update(one.keys())
        print(f"wrote {len(forms)} projects to {argv[2]}; projects holding each form:")
        for form, count in sorted(total.items()):
            print(f"  {form:<24} {count}")
        return 0
    if len(argv) >= 5 and argv[1] == "run":
        return 1 if run(argv[2], argv[3], argv[4], int(argv[5]) if len(argv) > 5 else 3) else 0
    if len(argv) >= 6 and argv[1] == "all":
        gen(argv[4], int(argv[5]), int(argv[6]) if len(argv) > 6 else 1)
        return 1 if run(argv[2], argv[3], argv[4]) else 0
    print(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
