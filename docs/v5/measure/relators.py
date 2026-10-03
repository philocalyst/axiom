#!/usr/bin/env python3
"""One kind, two books: what a relator's legs make in the book of each side of it, against the legs written by hand there.

    relators.py gen DIR N [SEED]             write N cases into DIR (p0000/household-kind, household-hand, employer-kind, ...)
    relators.py run BINARY DIR [JOBS]        every case through the CLI of BINARY: the kind and the hand must print the same
    relators.py dump DUMP DIR [JOBS]         the same through the engine's own dump of every flow it made
    relators.py mutate TREE WORK DIR [N,M..] the mutants of the code under test: each must be caught

What it is for. Lane K6 lets a kind of contract say its legs once, with roles for ends (`also employer -> irs 7.65% of
amount #payroll-tax`), and projects them onto the book of whoever owns the contract: a role its owner (or a member of the
owner) fills stands at the account the schedule pays from or into, a role anyone else fills stands outside, and a leg with
both ends outside touches nothing and is left out (`lower/contracts/relator.rs`). The check is the legs written by hand:
every case is a kind with two to three roles, legs of its own and of a kind beneath it (flows between a role, an account
and a party, with amounts, purposes and guards, and items), and a contract of it, and is written four times:

    household-kind   the book of the person the contract pays into: `family` owns `checking`, the employee is a member
                     of it or the owner itself, and the contract is the kind's, with the roles filled
    household-hand   the same book with no kind: the contract carries each leg the household has, its role ends written as
                     the account they stand at or the party they are
    employer-kind    the book of the employer, which owns `payroll` and pays the employee from it, with the same kind
    employer-hand    the same book with no kind

Each kind must print what its hand prints, on every command and in every flow the engine makes (`derives.py` has the
commands and the dump). The books are clean and silent. One kind, two books: the household book is the one in which the
employer's half of a payroll tax is not, and the employer book is the one in which it is.
"""
import os
import random
import shutil
import sys
from concurrent.futures import ThreadPoolExecutor

import derives
from derives import TODAY, UNTIL, flows_made, money, outputs

OUTSIDERS = ["taxman", "county"]
PURPOSES = ["levy", "kickback"]
FILLERS = {"employee": "p1", "employer": "e1", "agent": "o1"}
KINDS = {"employee": "person", "employer": "employer", "agent": "org"}
BOOKS = ("household", "employer")
SPELLINGS = ("kind", "hand")

PRELUDE = """\
use std
base USD
purpose levy : spending
purpose kickback : income
entity family : household
entity p1 : person
  member family
entity e1 : employer
entity o1 : org
entity taxman : org
entity county : org
"""


class Leg:
    """One `also` of a kind: a flow between two ends, or an item of the header."""

    def __init__(self, rng, roles, header):
        self.header = header
        self.item = rng.random() < 0.2
        self.ends = rng.sample(roles + OUTSIDERS, 2)
        self.share = rng.choice([1, 2, 5, 8, 10])
        self.fixed = rng.randrange(5, 200)
        self.percent = rng.random() < 0.6
        self.purpose = rng.choice(PURPOSES)
        self.sign = rng.choice(["+", "-"])
        self.guard = rng.choice(["", "", "true", "false"])

    def amount(self):
        return f"{self.share}% of amount" if self.percent else money(self.fixed * 100)

    def when(self):
        if self.guard == "true":
            return f" when value(amount, USD) > {money(max(1, self.header // 2))}"
        if self.guard == "false":
            return f" when value(amount, USD) > {money(self.header * 2)}"
        return ""

    def line(self, end):
        """The `also` line, with `end` saying what a role's name is written as."""
        if self.item:
            return f"  also {self.sign} {self.share}% of amount #{self.purpose}{self.when()}\n"
        source, target = (end(name) for name in self.ends)
        return f"  also {source} -> {target} {self.amount()} #{self.purpose}{self.when()}\n"


class Case:
    def __init__(self, rng):
        self.header = rng.randrange(20_000, 400_000)
        self.roles = ["employee", "employer"] + (["agent"] if rng.random() < 0.5 else [])
        self.base = [Leg(rng, self.roles, self.header) for _ in range(rng.randrange(1, 4))]
        self.beneath = [Leg(rng, self.roles, self.header) for _ in range(rng.randrange(0, 3))]
        self.direct = rng.random() < 0.3
        self.day = rng.randrange(1, 29)
        self.begins = "2026-%02d-%02d" % (rng.randrange(1, 6), self.day)

    def legs(self):
        return self.base + self.beneath

    def declaration(self):
        text = "kind pay-kind : contract\n"
        text += "".join(f"  has {role} {KINDS[role]}\n" for role in self.roles)
        text += "".join(leg.line(lambda name: name) for leg in self.base)
        if self.beneath:
            text += "kind pay-kind-beneath : pay-kind\n" + "".join(leg.line(lambda name: name) for leg in self.beneath)
        return text

    def owner(self, book):
        return "e1" if book == "employer" else ("p1" if self.direct else "family")

    def accounts(self, book):
        account = "payroll" if book == "employer" else "checking"
        return f"account {account} : bank\n  owner {self.owner(book)}\n2025-12-31 market -> {account} 5_000_000.00 USD\n"

    def stands(self, book):
        """Which of the roles stand at the owner's account in this book, and what each is called otherwise."""
        owner = {"household": "employee", "employer": "employer"}[book]
        account = "payroll" if book == "employer" else "checking"
        return lambda name: account if name == owner else FILLERS.get(name, name)

    def contract(self, book, spelling):
        kind = "pay-kind-beneath" if self.beneath else "pay-kind"
        party, direction, account = ("e1", "into", "checking") if book == "household" else ("p1", "from", "payroll")
        head = f"contract c0 : {kind} with {party}\n" if spelling == "kind" else f"contract c0 with {party}\n"
        fills = "".join(f"  {role} {FILLERS[role]}\n" for role in self.roles) if spelling == "kind" else ""
        schedule = f"  {money(self.header)} monthly on {self.day} {direction} {account} #wages\n  from {self.begins}\n"
        hand = ""
        if spelling == "hand":
            at = self.stands(book)
            outside = lambda name: at(name) not in ("checking", "payroll")
            for leg in self.legs():
                if not leg.item and outside(leg.ends[0]) and outside(leg.ends[1]):
                    continue
                hand += leg.line(at)
        return head + fills + schedule + hand

    def lines(self):
        days, year, month = [], int(self.begins[:4]), int(self.begins[5:7])
        while f"{year}-{month:02d}-{self.day:02d}" <= TODAY:
            days.append(f"{year}-{month:02d}-{self.day:02d} c0")
            month += 1
        return "".join(line + "\n" for line in days)

    def book(self, book, spelling):
        kinds = self.declaration() if spelling == "kind" else ""
        return PRELUDE + kinds + self.accounts(book) + self.contract(book, spelling) + self.lines()


def gen(directory, count, seed=1):
    shutil.rmtree(directory, ignore_errors=True)
    for number in range(count):
        case = Case(random.Random(seed * 100_000 + number))
        for book in BOOKS:
            for spelling in SPELLINGS:
                path = os.path.join(directory, f"p{number:04d}", f"{book}-{spelling}")
                os.makedirs(path)
                with open(os.path.join(path, "axiom.ax"), "w") as handle:
                    handle.write(case.book(book, spelling))
    print(f"{count} cases, each in {len(BOOKS)} books, each written {len(SPELLINGS)} ways")


def projects(directory, name):
    return [os.path.join(directory, name, f"{book}-{spelling}") for book in BOOKS for spelling in SPELLINGS]


def same(binary, directory, name):
    """The books on which the kind prints something other than what its hand does, and what the household's kind printed."""
    by = {os.path.basename(path): outputs(binary, path) for path in projects(directory, name)}
    differing = [
        book for book in BOOKS if any(by[f"{book}-kind"][command] != by[f"{book}-hand"][command] for command in by[f"{book}-kind"])
    ]
    return differing, by["employer-kind"]


def run(binary, directory, jobs=4, quiet=False):
    names = sorted(name for name in os.listdir(directory) if name.startswith("p"))
    with ThreadPoolExecutor(jobs) as pool:
        results = list(pool.map(lambda name: same(binary, directory, name), names))
    differs = [(name, books) for name, (books, _) in zip(names, results) if books]
    clean = sum("error" not in out["check"] and "warning" not in out["check"] for _, out in results)
    if not quiet:
        for name, books in differs[:10]:
            print(f"{name}: {', '.join(books)}")
        print(f"{len(names)} cases, {len(differs)} differ; {clean} employer books clean and silent")
    return len(differs)


def dump(binary, directory, jobs=4, quiet=False):
    names = sorted(name for name in os.listdir(directory) if name.startswith("p"))

    def one(name):
        rows = {os.path.basename(path): flows_made(binary, path) for path in projects(directory, name)}
        return [book for book in BOOKS if rows[f"{book}-kind"] != rows[f"{book}-hand"]], len(rows["employer-kind"])

    with ThreadPoolExecutor(jobs) as pool:
        results = list(pool.map(one, names))
    differs = [(name, books) for name, (books, _) in zip(names, results) if books]
    if not quiet:
        print(f"{len(names)} cases, {len(differs)} differ in the flows they make: {differs[:8]}; {sum(n for _, n in results)} rows")
    return len(differs)


RELATOR = "crates/model/src/lower/contracts/relator.rs"
LINE = "crates/model/src/laws/compile/line.rs"
CONTRACTS = "crates/model/src/lower/contracts.rs"

# (file, the text, what replaces it, what the mutant is)
MUTANTS = [
    (RELATOR, "if is_member(book, filler.entity, contract.owner) {", "if false {", "no role stands at the owner's account"),
    (RELATOR, "if is_member(book, filler.entity, contract.owner) {", "if true {", "every role stands at the owner's account"),
    (RELATOR, "for _ in 0..book.entities.len() {", "for _ in 0..0 {", "not even the owner stands at its own account"),
    (RELATOR, "Some(up) => at = up,", "Some(_) => return false,", "a member of the owner does not stand where it does"),
    (RELATOR, "ast::Direction::From => header.from,\n        ast::Direction::Into => header.to,",
     "ast::Direction::From => header.to,\n        ast::Direction::Into => header.from,", "the holding is the party's end of the header"),
    (RELATOR, "outside(from) && outside(to)", "outside(from) || outside(to)", "a leg with one end outside is left out"),
    (RELATOR, "outside(from) && outside(to)", "false", "a leg with both ends outside is kept"),
    (RELATOR, "matches!(world.book.places[place].role, Role::Outside(_))", "matches!(world.book.places[place].role, Role::Holding(_))",
     "what is outside is what is held"),
    (RELATOR, "lineage.reverse();", "", "the kind beneath's legs come before the kind's"),
    (RELATOR, "compile_also(world, diags, &site, also, positions)", "compile_also(world, diags, &site, also, Positions::NONE)",
     "no role stands anywhere"),
    (RELATOR, "if touches_nothing(world, &site, also, positions) {", "if false {", "no leg is left out"),
    (RELATOR, "let takes_entities = |slot: &&Slot| matches!(slot.range, Range::Kinds(_));", "let takes_entities = |_: &&Slot| false;",
     "no role is ever left empty"),
    (RELATOR, "if let Some(first) = found.iter().find(|filled| filled.slot == slot.name).filter(|_| !many(slot)) {",
     "if let Some(first) = found.iter().find(|filled| filled.slot == slot.name).filter(|_| false) {", "a slot may be filled twice"),
    (RELATOR, "View::Kinds(takes) if takes.iter().any(|&takes| world.book.kinds.covers(takes, kind)) => Ok(()),",
     "View::Kinds(_) => Ok(()),", "any entity fills any slot"),
    (RELATOR, "if world.book.kinds[kind].sort != Sort::Contract {", "if false {", "any kind is a kind of contract"),
    (RELATOR, "let needs = |slot: &&Slot| matches!(slot.mult, Mult::One | Mult::Some) && matches!(slot.range, Range::Kinds(_));",
     "let needs = |_: &&Slot| false;", "no slot is needed"),
    (LINE, "Standing::Stands(place) => Some(Some(place)),", "Standing::Stands(_) => Some(None),", "a role stands at the header's own end"),
    (LINE, "None if self.empty.contains(&name) => Standing::Empty,", "None if self.empty.contains(&name) => Standing::NoRole,",
     "a role left empty is a name like any other"),
    (LINE, "match self.stands.iter().find(|(slot, _)| *slot == name) {", "match self.stands.iter().rev().find(|(slot, _)| *slot == name) {",
     "the last of two fillers is the one that stands"),
    (CONTRACTS, "laws.extend(relator::legs(world, collected, written, diags));", "", "a contract's kind writes no legs"),
]


def detect(source, work):
    """What the oracle says of a build of SOURCE: the engine's dump of the kind's flows against the hand's, on the sample."""
    from forecast import build as build_dump

    binary = build_dump(source, os.path.join(work, "dump"))
    return "killed by the engine dump" if dump(binary, os.path.join(work, "sample"), 3, quiet=True) else None


def mutate(tree, work, directory, only=None):
    from mutation import mutate as run_mutants

    work = os.path.abspath(work)
    sample = os.path.join(work, "sample")
    shutil.rmtree(sample, ignore_errors=True)
    os.makedirs(sample)
    for name in sorted(n for n in os.listdir(directory) if n.startswith("p"))[:120]:
        shutil.copytree(os.path.join(directory, name), os.path.join(sample, name))
    return run_mutants(tree, work, MUTANTS, detect, only)


def main(argv):
    if argv[1] == "gen":
        return gen(argv[2], int(argv[3]), int(argv[4]) if len(argv) > 4 else 1)
    if argv[1] == "run":
        return run(argv[2], argv[3], int(argv[4]) if len(argv) > 4 else 4) and 1
    if argv[1] == "dump":
        return dump(argv[2], argv[3], int(argv[4]) if len(argv) > 4 else 4) and 1
    if argv[1] == "mutate":
        return mutate(argv[2], argv[3], argv[4], {int(n) for n in argv[5].split(",")} if len(argv) > 5 else None)
    raise SystemExit(__doc__)


if __name__ == "__main__":
    sys.exit(main(sys.argv) or 0)
