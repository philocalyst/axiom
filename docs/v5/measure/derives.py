#!/usr/bin/env python3
"""What a `derive` makes, against the contract that would have said it.

    derives.py gen DIR N [SEED]                 write N triples of projects into DIR (p0000/derived, /sugar, /written)
    derives.py run BINARY DIR [JOBS]            run the three books of every triple through BINARY: they must print the same
    derives.py dump DUMP DIR [JOBS]             the same three books through the engine's own dump of every flow it made
    derives.py compare BASELINE NEW DIR [JOBS]  both builds on the written book: it must print what it printed
    derives.py mutate TREE WORK DIR [N,M..]     the mutants of the code under test: each must be caught

What it is for. Lane K6 gives a law one more effect, `derive`: when a promise's occurrence is made, the laws written in
its contract make flows and items that join it (`engine/src/occurrence/derive.rs`). A contract's `also` and `share` are
laws it abbreviates. A derived flow is nothing a contract cannot already say, so the strongest check is the contract
that says it: every book is written three times, and the three must be the same book to every command.

    derived   a contract whose body has `law derived` / `on flow` / `derive ...`
    sugar     the same contract with each law written as the line that abbreviates it: `also LINE [when E]`, or
              `share 12% for me` for a carved item of the header's own purpose
    written   the same contract with what the law derives written in its body: a flow of its own is a header that is
              larger by it and a leg that takes it (`ACC N USD #p`), an added, taken-off or carved item is the item
              line the template already has (`+ 3.35 USD #fee`, `- 2.00 USD #refund`, `40.00 USD #fee`)

A form is one of those, with the amount it is written with (a literal, or a share of the header, which the writer works
out as the fold rounds it), and a `when` that is true for every occurrence or false for every one (the guard the law
has and the written book cannot say: a false one is a form the written book leaves out). Contracts pay from or into an
account, are kept for every due day up to today, and forecast for the rest. Every book is clean and silent, so a
difference is one the two spellings made. One thing the two spellings do differ in is not asked: the order of the
flows inside one occurrence (a derived flow follows the group's own, a written leg is made with them), which a forecast
sees only in the balance an overdraft is first reported at; the account holds enough that it never is.

The commands: `check` (the footer without the count of laws, which only the derived book has), `balance`, `flow`,
`claims`, `tax 2026` and `forecast`, each on the same day. The reports say little of what an occurrence's flows are
for (`flow` and `register` leave a kept occurrence out), so the engine layer is read as well: `forecasts/main.rs` dumps
every flow of every kept and forecast occurrence, with its ends, amounts, purpose, owner and payee, and the three
books must make the same flows, whatever order the fold made them in.
"""
import os
import random
import re
import shutil
import subprocess
import sys
from collections import Counter
from concurrent.futures import ThreadPoolExecutor
from decimal import ROUND_HALF_EVEN, Decimal

TODAY = "2026-06-30"
UNTIL = "2026-12-31"

PRELUDE = """\
use std
base USD
purpose fee : spending
purpose refund : income
purpose match : transfer
purpose escrowed : transfer
entity me : person
entity lender
entity acme : employer
entity shop
account checking : bank
account savings : bank
account escrow : bank
account k401 : bank
2025-12-31 market -> checking 5_000_000.00 USD
"""

ACCOUNTS = ["escrow", "savings", "k401"]
PURPOSES = ["match", "escrowed"]


def cents(value):
    return int(value.quantize(Decimal(1), rounding=ROUND_HALF_EVEN))


def money(qty):
    return f"{qty // 100}.{qty % 100:02d} USD"


class Form:
    """One thing a law derives, and how a contract says it."""

    def __init__(self, rng, header, direction):
        self.kind = rng.choice(["flow", "flow-share", "add", "add", "less", "less-nothing", "carve", "share"])
        self.share = Decimal(rng.choice([1, 2, 3, 5, 8, 10, 15, 20])) / 100
        self.fixed = rng.randrange(500, 9_000)
        self.account = rng.choice(ACCOUNTS)
        self.purpose = rng.choice(PURPOSES)
        self.header, self.direction = header, direction
        self.guard = "" if self.kind == "share" else rng.choice(["", "", "true", "false"])
        self.threshold = rng.randrange(1, 10_000)

    def amount(self):
        """What the form comes to for one occurrence, in cents, as the fold rounds it."""
        if self.kind in ("flow-share", "add", "less", "less-nothing", "share"):
            return cents(Decimal(self.header) * self.share)
        return self.fixed

    def holds(self):
        return self.guard != "false"

    def when(self):
        """The guard line of the law, always true or always false for this contract."""
        if self.guard == "true":
            return f"    when value(amount, USD) > {money(max(1, self.header // 2))}\n"
        if self.guard == "false":
            return f"    when value(amount, USD) > {money(self.header * 2)}\n"
        return ""

    def derived(self):
        """The law's step."""
        percent = f"{int(self.share * 100)}%"
        source = "" if self.direction == "from" else "acme "
        return {
            "flow": f"    derive {source}-> {self.account} {money(self.fixed)} #{self.purpose}\n",
            "flow-share": f"    derive {source}-> {self.account} {percent} of amount #{self.purpose}\n",
            "add": f"    derive + {percent} of amount #fee\n",
            "less": f"    derive - {percent} of amount #refund\n",
            "less-nothing": f"    derive - {percent} of amount\n",
            "carve": f"    derive {money(self.fixed)} #fee\n",
            "share": f"    derive {percent} of amount #fee\n",
        }[self.kind]

    def sugared(self):
        """The line that abbreviates the law: an `also` with the law's guard, or a `share` that is the owner's own."""
        if self.kind == "share":
            return f"  share {int(self.share * 100)}% for me\n"
        condition = {"true": f" when value(amount, USD) > {money(max(1, self.header // 2))}",
                     "false": f" when value(amount, USD) > {money(self.header * 2)}"}.get(self.guard, "")
        return "  also" + self.derived().strip().removeprefix("derive") + condition + "\n"

    def written(self):
        """What the contract says in the law's place: a leg, or an item. A share is written as the amount the fold makes
        of it, because a template's own `5% of amount` is not something a contract can read today."""
        return {
            "flow": f"  {self.account} {money(self.fixed)} #{self.purpose}\n",
            "flow-share": f"  {self.account} {money(self.amount())} #{self.purpose}\n",
            "add": f"  + {money(self.amount())} #fee\n",
            "less": f"  - {money(self.amount())} #refund\n",
            "less-nothing": f"  - {money(self.amount())}\n",
            "carve": f"  {money(self.fixed)} #fee\n",
            "share": f"  {money(self.amount())} #fee\n",
        }[self.kind]

    def widens(self):
        """How much a flow of its own makes the header larger in the written book."""
        return self.amount() if self.kind in ("flow", "flow-share") and self.holds() else 0


class Contract:
    def __init__(self, rng, name):
        self.name = name
        self.direction = rng.choice(["from", "into"])
        self.party = "lender" if self.direction == "from" else "acme"
        self.header = rng.randrange(20_000, 400_000)
        self.day = rng.randrange(1, 29)
        self.forms = [Form(rng, self.header, self.direction) for _ in range(rng.randrange(1, 4))]
        self.begins = "2026-%02d-%02d" % (rng.randrange(1, 6), self.day)

    def head(self, amount):
        return (
            f"contract {self.name} with {self.party}\n"
            f"  {money(amount)} monthly on {self.day} {self.direction} checking #fee\n  from {self.begins}\n"
        )

    def derived(self):
        text = self.head(self.header)
        for form in self.forms:
            text += f"  law {self.name}-{self.forms.index(form)}\n    on flow\n{form.when()}{form.derived()}"
        return text

    def sugar(self):
        return self.head(self.header) + "".join(form.sugared() for form in self.forms)

    def written(self):
        text = self.head(self.header + sum(form.widens() for form in self.forms))
        for form in self.forms:
            if form.holds():
                text += form.written()
        return text

    def lines(self):
        """The occurrences the journal keeps: every due day up to today, so a book is silent, and the forecast has the rest."""
        days = []
        year, month = int(self.begins[:4]), int(self.begins[5:7])
        while f"{year}-{month:02d}-{self.day:02d}" <= TODAY:
            days.append(f"{year}-{month:02d}-{self.day:02d} {self.name}")
            month += 1
        return days


def gen(directory, count, seed=1):
    shutil.rmtree(directory, ignore_errors=True)
    forms = Counter()
    for number in range(count):
        rng = random.Random(seed * 100_000 + number)
        contracts = [Contract(rng, f"c{index}") for index in range(rng.randrange(1, 4))]
        for contract in contracts:
            forms.update(f"{form.kind}{'' if form.guard == '' else ':' + form.guard}" for form in contract.forms)
        lines = sorted(line for contract in contracts for line in contract.lines())
        for sort, write in (("derived", Contract.derived), ("sugar", Contract.sugar), ("written", Contract.written)):
            path = os.path.join(directory, f"p{number:04d}", sort)
            os.makedirs(path)
            with open(os.path.join(path, "axiom.ax"), "w") as handle:
                handle.write(PRELUDE + "".join(write(contract) for contract in contracts) + "\n".join(lines) + "\n")
    with open(os.path.join(directory, "forms.txt"), "w") as handle:
        for form, n in sorted(forms.items()):
            handle.write(f"{form} {n}\n")
    print(f"{count} triples; forms: " + ", ".join(f"{form} {n}" for form, n in sorted(forms.items())))


COMMANDS = [
    ["check"],
    ["balance"],
    ["flow"],
    ["claims"],
    ["tax", "2026"],
    ["forecast", "--until", UNTIL, "--paths", "1"],
]


SORTS = ("derived", "sugar", "written")
FLOW_ORDINAL = re.compile(r" ord=\d+")
PURPOSE_SOURCE = re.compile(r"purpose: (\w+#\d+), of: (None|Some\([^)]*\)), source: [^}]*\}")


def same_flow(flow):
    """A flow as the engine dumps it, less its place in the occurrence and where its purpose was read."""
    return PURPOSE_SOURCE.sub(r"purpose: \1, of: \2 }", FLOW_ORDINAL.sub("", flow))


def flows_made(binary, project):
    """The flows the engine made for the project's occurrences, kept and forecast, one row each, in no order: what a
    flow is and is for, and nothing of which line or law made it or the place it had in its occurrence."""
    path = os.path.join(project, "axiom.ax")
    rows = []
    for mode, today in (("history", "2025-12-31"), ("forecast", TODAY)):
        done = subprocess.run([binary, mode, path, today, UNTIL], capture_output=True, text=True, timeout=300)
        for line in (done.stdout + done.stderr).splitlines():
            if line.startswith(("kept ", "planned ")):
                head, _, flows = line.partition(" [")
                rows += [head + " " + same_flow(flow) for flow in flows.rstrip("]").split(" | ")]
            elif line.startswith("holding "):
                # What a place holds, not which parcels: money moved by a leg and by a header is relieved in the order the
                # fold posts them, which the two spellings do not share.
                rows.append(" ".join(line.split()[:5]))
            elif line.startswith(("error ", "diagnostic ")):
                rows.append(line)
    return sorted(rows)


def dumped(binary, directory, name):
    """Whether the three books make the same flows, and how many flows the derived one made."""
    made = [flows_made(binary, os.path.join(directory, name, sort)) for sort in SORTS]
    return made[0] == made[1] == made[2], len(made[0])


def dump(binary, directory, jobs=4, quiet=False):
    names = sorted(name for name in os.listdir(directory) if name.startswith("p"))
    with ThreadPoolExecutor(jobs) as pool:
        results = list(pool.map(lambda name: dumped(binary, directory, name), names))
    differs = [name for name, (same, _) in zip(names, results) if not same]
    if not quiet:
        print(f"{len(names)} triples, {len(differs)} differ in the flows they make: {differs[:10]}; {sum(n for _, n in results)} rows")
    return len(differs)


def outputs(binary, project):
    out = {}
    for command in COMMANDS:
        args = [binary, *command, "-C", project, "--today", TODAY, "--color", "never"]
        done = subprocess.run(args, capture_output=True, text=True, timeout=120)
        text = done.stdout + done.stderr
        out[" ".join(command)] = re.sub(r" · \d+ laws? enforced", "", re.sub(r"/[^ ]*/p\d+/[\w-]+", "<project>", text))
    return out


def pair(binary, directory, name):
    """The commands on which the derived book differs from the written one or from the sugared one."""
    derived, sugar, written = (outputs(binary, os.path.join(directory, name, sort)) for sort in SORTS)
    return [command for command in derived if derived[command] != written[command] or derived[command] != sugar[command]], derived


def run(binary, directory, jobs=4, quiet=False):
    names = sorted(name for name in os.listdir(directory) if name.startswith("p"))
    differs, clean, moved = [], 0, 0
    with ThreadPoolExecutor(jobs) as pool:
        for name, (commands, derived) in zip(names, pool.map(lambda name: pair(binary, directory, name), names)):
            clean += "error" not in derived["check"] and "warning" not in derived["check"]
            moved += "net worth" in derived["check"]
            if commands:
                differs.append((name, commands))
    if not quiet:
        for name, commands in differs[:10]:
            print(f"{name}: {', '.join(commands)}")
        print(f"{len(names)} pairs, {len(differs)} differ; {clean} clean and silent, {moved} fold to a net worth")
    return len(differs)


def compare(baseline, new, directory, jobs=4):
    """The books written without a `derive` are what they were: the written books through both builds."""
    names = sorted(name for name in os.listdir(directory) if name.startswith("p"))

    def one(name):
        path = os.path.join(directory, name, "written")
        return outputs(baseline, path) != outputs(new, path)

    with ThreadPoolExecutor(jobs) as pool:
        differs = [name for name, differ in zip(names, pool.map(one, names)) if differ]
    print(f"{len(names)} written books, {len(differs)} differ between the builds")
    return len(differs)


ENGINE = "crates/engine/src/occurrence/derive.rs"
COMPILE = "crates/model/src/laws/compile/derive.rs"
RULES = "crates/model/src/rules.rs"
POST = "crates/engine/src/post.rs"
CALC = "crates/engine/src/calc.rs"

# (file, the text, what replaces it, what the mutant is). Each must be caught by the oracle (a build that makes the
# two spellings of a contract two books) or by a test that names what it checks.
MUTANTS = [
    (ENGINE, "Occasion { amount: Some(given.out), ..Occasion::flow(&motion) }",
     "Occasion { amount: Some(group[0].flow.out), ..Occasion::flow(&motion) }",
     "`amount` is the header after the legs and items took from it, not as the template gave it"),
    (ENGINE, "header_motion(book, &group[0], details, cx.making.source_day)",
     "header_motion(book, &group[group.len() - 1], details, cx.making.source_day)", "the law fires for the group's last flow"),
    (ENGINE, "let takes = sign == Sign::Carve || (sign == Sign::Less && derived.purpose.is_none());",
     "let takes = sign == Sign::Carve;", "a `-` item with no purpose takes nothing from the header"),
    (ENGINE, "let takes = sign == Sign::Carve || (sign == Sign::Less && derived.purpose.is_none());",
     "let takes = sign == Sign::Carve || sign == Sign::Less;", "every `-` item takes from the header"),
    (ENGINE, "let takes = sign == Sign::Carve || (sign == Sign::Less && derived.purpose.is_none());",
     "let takes = sign == Sign::Carve || (sign == Sign::Less && derived.purpose.is_some());",
     "only a `-` item with a purpose takes"),
    (ENGINE, "let takes = sign == Sign::Carve || (sign == Sign::Less && derived.purpose.is_none());",
     "let takes = sign == Sign::Add || (sign == Sign::Less && derived.purpose.is_none());", "an added item takes, a carved one does not"),
    (ENGINE, "(header.out, header.arrive) = (left.out, left.arrive);\n        }\n        Ok(())",
     "(header.out, header.arrive) = (header.out, header.arrive);\n        }\n        Ok(())",
     "the header keeps what a carved item took"),
    (ENGINE, ".map_or(0, |flow| flow.ordinal + 1);", ".map_or(0, |flow| flow.ordinal);", "a derived flow has the ordinal of the one before it"),
    (ENGINE, "if !derived.makes_flow() {", "if false {", "an item with nothing to tell it from its header makes a flow"),
    (ENGINE, "if !derived.makes_flow() {", "if true {", "no derived flow is made"),
    (ENGINE, "Shape::Item(Sign::Less) => (header.to, header.from),", "Shape::Item(Sign::Less) => (header.from, header.to),",
     "a `-` item goes the header's way"),
    (ENGINE, "Shape::Item(Sign::Add | Sign::Carve) => (header.from, header.to),",
     "Shape::Item(Sign::Add | Sign::Carve) => (header.to, header.from),", "an added or carved item goes back"),
    (ENGINE, "Shape::Flow { from, to } => (from.unwrap_or(header.from), to.unwrap_or(header.to)),",
     "Shape::Flow { from, to } => (from.unwrap_or(header.to), to.unwrap_or(header.from)),",
     "a flow that names one end has the other end of the header backwards"),
    (ENGINE, "purpose: derived.purpose.or(header.purpose),", "purpose: header.purpose,", "a derived flow has the header's purpose"),
    (ENGINE, "purpose: derived.purpose.or(header.purpose),", "purpose: derived.purpose,", "a derived flow that says no purpose has none"),
    (ENGINE, "owner: derived.owner.unwrap_or(header.owner),", "owner: header.owner,", "a share's flow is borne by the header's owner"),
    (ENGINE, "description: derived.description.or(header.description),", "description: derived.description,",
     "a derived flow does not keep the header's description"),
    (ENGINE, "origin: Origin::Derived(Derivation::Law(made.law)),", "origin: header.origin,",
     "a derived flow does not say which law made it"),
    (ENGINE, "out: made.amount,\n            arrive: made.amount,", "out: made.amount,\n            arrive: header.arrive,",
     "a derived flow arrives as the header did"),
    (ENGINE, "Bear { amount: Cut::Of(Expr::Literal(made.amount)), side, unit, takes }",
     "Bear { amount: Cut::Of(Expr::Literal(made.amount)), side: side.other(), unit, takes }",
     "an item is borne on the other side of the header"),
    (RULES, "let watch = if book.laws[law].derives() { Watch::Occurrence(contract) } else { Watch::Contract(contract) };",
     "let watch = Watch::Contract(contract);", "a law that derives is read as a flow posts"),
    (RULES, "let watch = if book.laws[law].derives() { Watch::Occurrence(contract) } else { Watch::Contract(contract) };",
     "let watch = if book.laws[law].derives() { Watch::Occurrence(contract) } else { Watch::Occurrence(contract) };",
     "a law that judges is read when an occurrence is made"),
    (POST, "            self.fire_contract(m, &on);\n", "", "the laws a contract writes never fire as its flows post"),
    (POST, "self.fire(rules, &Occasion { amount: Some(m.out), ..*on });", "self.fire(rules, &Occasion { amount: Some(m.arrive), ..*on });",
     "a contract's law reads what arrived, not what left"),
    (CALC, "Ok(if self.compare(BinOp::Le, left, right)? { left } else { right })",
     "Ok(if self.compare(BinOp::Le, left, right)? { right } else { left })", "`up to` is the larger"),
    (COMPILE, "if !matches!(self.owner, Some(Owner::Contract(_))) {", "if false {", "a law that is not a contract's may derive"),
    (COMPILE, "trigger.is_some_and(|trigger| trigger != Trigger::Flow)", "trigger.is_some_and(|trigger| trigger == Trigger::Flow)",
     "a law that derives must not fire on a flow"),
    (COMPILE, "StepKind::When(_) | StepKind::Unless(_) | StepKind::Let(_))", "StepKind::When(_) | StepKind::Unless(_))",
     "a `let` in a law that derives is a step that judges"),
    (COMPILE, "let Some(first) = steps.iter().find(|step| derives(step)) else { return };",
     "let Some(first) = steps.iter().find(|step| derives(step)) else { return };\n        if first.loc == first.loc {\n            return;\n        }",
     "a law that derives is checked for nothing"),
]


def detect(source, work):
    """What the oracle says of a build of SOURCE: the engine's own dump of the flows the three spellings make, on the
    sample, and nothing when they agree. (The reports are the model's and the engine's flows added up; the dump is the layer
    the code under test is in.)"""
    from forecast import build as build_dump

    binary = build_dump(source, os.path.join(work, "dump"))
    return "killed by the engine dump" if dump(binary, os.path.join(work, "sample"), 3, quiet=True) else None


def mutate(tree, work, directory, only=None):
    """Each mutant must be caught by the oracle on the first pairs of DIRECTORY, or by a test that fails only with it."""
    from mutation import mutate as run_mutants

    work = os.path.abspath(work)
    sample = os.path.join(work, "sample")
    shutil.rmtree(sample, ignore_errors=True)
    os.makedirs(sample)
    for name in sorted(n for n in os.listdir(directory) if n.startswith("p"))[:150]:
        shutil.copytree(os.path.join(directory, name), os.path.join(sample, name))
    return run_mutants(tree, work, MUTANTS, detect, only)


def main(argv):
    if argv[1] == "gen":
        return gen(argv[2], int(argv[3]), int(argv[4]) if len(argv) > 4 else 1)
    if argv[1] == "run":
        return run(argv[2], argv[3], int(argv[4]) if len(argv) > 4 else 4) and 1
    if argv[1] == "dump":
        return dump(argv[2], argv[3], int(argv[4]) if len(argv) > 4 else 4) and 1
    if argv[1] == "compare":
        return compare(argv[2], argv[3], argv[4]) and 1
    if argv[1] == "mutate":
        return mutate(argv[2], argv[3], argv[4], {int(n) for n in argv[5].split(",")} if len(argv) > 5 else None)
    raise SystemExit(__doc__)


if __name__ == "__main__":
    sys.exit(main(sys.argv) or 0)
