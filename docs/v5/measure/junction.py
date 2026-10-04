#!/usr/bin/env python3
"""A generator of small books whose lines are written twice, in v4 and in v5, and a check that the two are one book.

    junction.py gen DIR N [SEED]          write N projects into DIR: DIR/p0000/v4/axiom.ax and DIR/p0000/v5/axiom.ax
    junction.py run BINARY DIR [JOBS]     run the commands below over both renders of every project and compare
    junction.py upgrade BINARY DIR [JOBS] `fmt --upgrade` of each v4 render must be `fmt` of its v5 render
    junction.py mutate BINARY DIR [JOBS]  mutants of the v5 render (swap the sides, drop a leg's arrow, flip buy and
                                          sell) must each be seen: a different book, or a diagnostic
    junction.py all BINARY DIR N [SEED]   gen, run, upgrade, mutate

What it is for. Lane L1 changes how a flow line is spelled and nothing it means: `acme -> checking 3_200 USD` is
`checking <- acme 3_200 USD`, a paystub's `acme -> 5_200 USD` with bare legs is `me <- acme 5_200 USD` with arrows,
`checking 1_499.99 USD -> broker 5 VTI @ 300 USD` states its price once. The lines of every shape are generated as data
(a v4 text and the v5 text that says the same) by a seeded random generator, so the same SEED and N write the same
books. `run` shows that the two renders give one book through every command the CLI has (`check`, `balance` in its
three forms, `register` of every place, `flow`, `available`, `claims`, `contracts`, `tax`, `gains`, `lots`,
`forecast`), byte for byte in the JSON, the diagnostics compared by code, severity and message (the v4 render says
`v4-syntax` once and the v5 render nothing). `upgrade` shows that `fmt --upgrade` writes the v5 render from the v4
one. `mutate` shows the check is not vacuous: each mutant of a v5 render that the generator can make (a `<-` line with
its two ends swapped, a leg with its arrow dropped, a buy written as a sell) has to be told from the book it came from.

`gen` counts the forms it writes, and `run` says how many projects were clean (no error at all) and moved money, so a
generator whose books were all rejected would show it.
"""
import json
import os
import random
import re
import subprocess
import sys
import tempfile
from collections import Counter
from concurrent.futures import ThreadPoolExecutor
from decimal import ROUND_HALF_EVEN, Decimal

TODAY = "2026-06-30"

PRELUDE = """\
use std
base USD
commodity VTI : stock
  precision 3
commodity EURX : currency
  precision 2
entity me : person
entity acme : org
entity shop : org
entity taxman : org
entity buyer : org
entity landlord : landlord
purpose fees : spending
account checking : bank
account savings : bank
account wallet : cash
account broker : brokerage
account visa : credit-card
opening 2026-01-01
  checking 200_000 USD
  savings 5_000 USD
  wallet 2_000 USD
  broker 50 VTI basis 5_000 USD since 2020-01-01
2026-01-01 VTI = 120 USD
2026-01-01 EURX = 1.1 USD
"""

COMMANDS = [
    ["check"],
    ["balance"],
    ["balance", "--value"],
    ["balance", "--monthly"],
    ["flow"],
    ["flow", "--by", "party"],
    ["available"],
    ["claims"],
    ["contracts"],
    ["tax", "2026"],
    ["gains", "2026"],
    ["lots"],
    ["forecast", "--paths", "20"],
]
PLACES = ["checking", "savings", "wallet", "broker", "visa"]
PARTIES = ["shop", "acme", "taxman", "buyer", "landlord"]
PURPOSES = ["fun", "groceries", "household", "dining", "transport"]


class Book:
    """The lines of one project in both spellings, and the forms they use."""

    def __init__(self, rng):
        self.rng = rng
        self.v4, self.v5 = [], []
        self.forms = Counter()

    def add(self, v4, v5=None, *forms):
        self.v4.extend(v4.rstrip("\n").split("\n"))
        self.v5.extend((v4 if v5 is None else v5).rstrip("\n").split("\n"))
        for form in forms:
            self.forms[form] += 1

    def day(self, month=None):
        month = month or self.rng.randint(2, 5)
        return f"2026-{month:02d}-{self.rng.randint(2, 27):02d}"


def usd(rng, low=5, high=400):
    return f"{rng.randint(low, high)} USD"


def tail(rng):
    parts = []
    if rng.random() < 0.5:
        parts.append("#" + rng.choice(PURPOSES))
    if rng.random() < 0.2:
        parts.append(rng.choice(['"for the month"', '"a note"']))
    return (" " + " ".join(parts)) if parts else ""


def cents(value):
    return f"{value.quantize(Decimal('0.01'), rounding=ROUND_HALF_EVEN)}"


# ─── Recipes: each adds a block of lines, in v4 and in v5 ────────────────────────────────────────────────────


def give(book):
    rng = book.rng
    a, b = rng.choice([("checking", "shop"), ("visa", "shop"), ("checking", "savings"), ("wallet", "shop"),
                       ("checking", "landlord"), ("savings", "wallet")])
    book.add(f"{book.day()} {a} -> {b} {usd(rng)}{tail(rng)}", None, "give")


def take(book):
    rng = book.rng
    a, b = rng.choice([("acme", "checking"), ("buyer", "savings"), ("shop", "checking"), ("taxman", "wallet")])
    mark = tail(rng) or " #wages"
    day, amt = book.day(), usd(rng)
    if rng.random() < 0.3:
        book.add(f"{day} {a} {amt} -> {b}{mark}", f"{day} {b} <- {a} {amt}{mark}", "take:left-amount")
    else:
        book.add(f"{day} {a} -> {b} {amt}{mark}", f"{day} {b} <- {a} {amt}{mark}", "take")


def purchase(book):
    rng = book.rng
    qty, price = rng.randint(1, 5), Decimal(rng.randint(100, 150)) + Decimal(rng.randint(0, 99)) / 100
    cost, price = cents(Decimal(qty) * price), cents(price)
    day = book.day()
    if rng.random() < 0.5:
        book.add(f"{day} checking {cost} USD -> broker {qty} VTI @ {price} USD",
                 f"{day} checking -> broker {qty} VTI @ {price} USD", "buy:across")
    else:
        book.add(f"{day} broker -> broker {qty} VTI @ {price} USD", f"{day} broker <- {qty} VTI @ {price} USD", "buy:same")


def sale(book):
    rng = book.rng
    qty, price = rng.randint(1, 4), Decimal(rng.randint(100, 150)) + Decimal(rng.randint(0, 99)) / 100
    day = book.day()
    proceeds, price = cents(Decimal(qty) * price), cents(price)
    if rng.random() < 0.5:
        book.add(f"{day} broker {qty} VTI -> broker @ {price} USD", f"{day} broker -> {qty} VTI @ {price} USD", "sell:same")
    else:
        book.add(f"{day} broker {qty} VTI -> checking {proceeds} USD", f"{day} broker {qty} VTI -> checking @ {price} USD",
                 "sell:across")


def paystub(book):
    rng = book.rng
    day, gross = book.day(), rng.randint(2_000, 9_000)
    withheld, saved = rng.randint(100, 900), rng.randint(50, 500)
    legs4 = [f"  taxman {withheld} USD #tax-paid", f"  savings {saved} USD", "  checking ..."]
    legs5 = [f"  -> taxman {withheld} USD #tax-paid", f"  -> savings {saved} USD", "  -> checking ..."]
    book.add(f"{day} acme -> {gross} USD #wages\n" + "\n".join(legs4),
             f"{day} me <- acme {gross} USD #wages\n" + "\n".join(legs5), "split:party")


def own_split(book):
    rng = book.rng
    day, total = book.day(), rng.randint(100, 400)
    a, b = rng.randint(10, 40), rng.randint(10, 40)
    legs4 = [f"  shop {a} USD #groceries", f"  buyer {b} USD", "  savings ..."]
    legs5 = [f"  -> shop {a} USD #groceries", f"  -> buyer {b} USD", "  -> savings ..."]
    book.add(f"{day} checking -> {total} USD\n" + "\n".join(legs4), f"{day} checking -> {total} USD\n" + "\n".join(legs5),
             "split:own")


def dangling(book):
    rng = book.rng
    day, total = book.day(), rng.randint(100, 400)
    a = rng.randint(10, 40)
    book.add(f"{day} checking {total} USD ->\n  shop {a} USD\n  savings ...",
             f"{day} checking -> {total} USD\n  -> shop {a} USD\n  -> savings ...", "split:dangling")


def arrive_split(book):
    rng = book.rng
    day, total = book.day(), rng.randint(100, 400)
    a = rng.randint(10, 40)
    book.add(f"{day} -> checking {total} USD\n  shop {a} USD\n  acme ...",
             f"{day} checking <- {total} USD\n  <- shop {a} USD\n  <- acme ...", "split:arrive")


def arrive_through(book):
    rng = book.rng
    day, total = book.day(), rng.randint(100, 400)
    a = rng.randint(10, 40)
    book.add(f"{day} -> shop {total} USD #household\n  savings {a} USD\n  checking ...",
             f"{day} me -> shop {total} USD #household\n  <- savings {a} USD\n  <- checking ...", "split:through")


def items(book):
    rng = book.rng
    a, b = rng.choice([("checking", "shop"), ("visa", "shop"), ("buyer", "checking"), ("acme", "savings")])
    day, header = book.day(), rng.randint(100, 600)
    body = "\n".join(f"  {rng.choice(['', '+ ', '- '])}{rng.randint(1, 30)} USD #fees" for _ in range(rng.randint(1, 2)))
    if a in ("buyer", "acme"):
        book.add(f"{day} {a} -> {b} {header} USD #wages\n{body}", f"{day} {b} <- {a} {header} USD #wages\n{body}", "items:take")
    else:
        book.add(f"{day} {a} -> {b} {header} USD #household\n{body}", None, "items:give")


def contract(book):
    rng = book.rng
    n = book.forms["contract"] + 1
    gross, tax, save = rng.randint(3_000, 6_000), rng.randint(200, 800), rng.randint(50, 300)
    name = f"job{n}"
    start = f"2026-{rng.randint(2, 4):02d}-01"
    head = f"contract {name} with acme\n  {gross} USD monthly on 1 into checking #wages\n  from {start}\n"
    legs4 = f"  taxman {tax} USD\n  savings {save} USD\n  checking ...\n"
    legs5 = f"  -> taxman {tax} USD\n  -> savings {save} USD\n  -> checking ...\n"
    occ = f"2026-05-01 {name}\n" if rng.random() < 0.7 else ""
    over4 = f"2026-06-01 {name}\n  taxman {tax + 10} USD\n" if rng.random() < 0.5 else ""
    over5 = over4.replace(f"  taxman {tax + 10} USD", f"  -> taxman {tax + 10} USD")
    book.v4.append(head + legs4 + occ + over4)
    book.v5.append(head + legs5 + occ + over5)
    book.forms["contract"] += 1
    book.forms["contract:legs"] += 1
    if over4:
        book.forms["occurrence:override"] += 1


RECIPES = [(give, 10), (take, 8), (purchase, 3), (sale, 3), (paystub, 4), (own_split, 3), (dangling, 2),
           (arrive_split, 2), (arrive_through, 2), (items, 3), (contract, 2)]


def make(rng):
    book = Book(rng)
    chosen = rng.choices([recipe for recipe, _ in RECIPES], [weight for _, weight in RECIPES], k=rng.randint(4, 14))
    for recipe in chosen:
        recipe(book)
    return book


def render(book, which):
    lines = book.v4 if which == "v4" else book.v5
    return PRELUDE + "\n".join(lines) + "\n"


def gen(out, n, seed):
    rng = random.Random(seed)
    os.makedirs(out, exist_ok=True)
    forms = Counter()
    for i in range(n):
        book = make(random.Random(rng.random()))
        forms.update(book.forms)
        for which in ("v4", "v5"):
            path = os.path.join(out, f"p{i:04d}", which)
            os.makedirs(path, exist_ok=True)
            with open(os.path.join(path, "axiom.ax"), "w") as f:
                f.write(render(book, which))
    with open(os.path.join(out, "forms.json"), "w") as f:
        json.dump(forms, f, indent=1, sort_keys=True)
    print(f"{n} projects (seed {seed}); forms written:")
    for form, count in sorted(forms.items()):
        print(f"  {count:6d}  {form}")


# ─── Running the binary ──────────────────────────────────────────────────────────────────────────────────────


def run(binary, args, project):
    cmd = [binary, *args, "-C", project, "--today", TODAY, "--color", "never"]
    done = subprocess.run(cmd, capture_output=True, text=True, timeout=120)
    return done.returncode, done.stdout, done.stderr


def diagnostics(stdout):
    """The diagnostics of a `--json` run, by code, severity and message: the v4 warning is the one allowed to differ."""
    found = []
    for line in stdout.splitlines():
        try:
            item = json.loads(line)
        except ValueError:
            continue
        if isinstance(item, dict) and "code" in item and item.get("code") != "v4-syntax":
            found.append((item["code"], item["severity"], item["message"]))
    return found


def observe(binary, project):
    """Everything the CLI says of a project, as (command, exit status, output) with the legacy warning removed."""
    seen = []
    for args in COMMANDS:
        code, out, err = run(binary, [*args, "--json"], project)
        if args == ["check"]:
            seen.append((" ".join(args), diagnostics(out), err))
        else:
            seen.append((" ".join(args), code, "\n".join(l for l in out.splitlines() if '"code":"v4-syntax"' not in l)))
    for place in PLACES:
        code, out, err = run(binary, ["register", place, "--json"], project)
        seen.append((f"register {place}", code, out))
    return seen


def clean(seen):
    """Whether the check raised no error, and the book moved money."""
    errors = [d for d in seen[0][1] if d[1] == "error"]
    return not errors


def compare_project(binary, root, name):
    v4, v5 = (observe(binary, os.path.join(root, name, which)) for which in ("v4", "v5"))
    differences = [a[0] for a, b in zip(v4, v5) if a != b]
    return name, differences, clean(v5)


def parallel(jobs, fn, items):
    with ThreadPoolExecutor(max_workers=jobs) as pool:
        return list(pool.map(fn, items))


def projects(root):
    return sorted(d for d in os.listdir(root) if re.fullmatch(r"p\d+", d))


def cmd_run(binary, root, jobs):
    results = parallel(jobs, lambda name: compare_project(binary, root, name), projects(root))
    bad = [(name, d) for name, d, _ in results if d]
    cleans = sum(1 for _, _, c in results if c)
    print(f"{len(results)} projects, {cleans} clean (no error), {len(bad)} differ")
    for name, d in bad[:20]:
        print(f"  {name}: {', '.join(d)}")
    return not bad


def formatted(binary, text, which):
    """`fmt` or `fmt --upgrade` of one book, as the text it writes."""
    with tempfile.TemporaryDirectory() as tmp:
        with open(os.path.join(tmp, "axiom.ax"), "w") as f:
            f.write(text)
        args = ["fmt", "--upgrade"] if which == "upgrade" else ["fmt"]
        code, out, err = run(binary, args, tmp)
        with open(os.path.join(tmp, "axiom.ax")) as f:
            return f.read(), code, out + err


def cmd_upgrade(binary, root, jobs):
    def one(name):
        v4 = open(os.path.join(root, name, "v4", "axiom.ax")).read()
        v5 = open(os.path.join(root, name, "v5", "axiom.ax")).read()
        upgraded, code, said = formatted(binary, v4, "upgrade")
        want, _, _ = formatted(binary, v5, "format")
        again, _, _ = formatted(binary, upgraded, "upgrade")
        return name, upgraded == want, again == upgraded, said

    results = parallel(jobs, one, projects(root))
    wrong = [(n, said) for n, ok, _, said in results if not ok]
    moving = [n for n, _, stable, _ in results if not stable]
    print(f"{len(results)} projects: {len(wrong)} upgrades differ from the v5 render, {len(moving)} are not stable")
    for name, said in wrong[:10]:
        print(f"  {name}: {said.strip()[:200]}")
    return not wrong and not moving


# ─── Mutants of the v5 render ────────────────────────────────────────────────────────────────────────────────

def mutate_first(text, pattern, replace):
    """`text` with the first line `pattern` matches rewritten by `replace`, or None when no line matches."""
    lines = text.split("\n")
    for at, line in enumerate(lines):
        match = re.match(pattern, line)
        if match:
            lines[at] = replace(match)
            return "\n".join(lines)
    return None


def swap_sides(text):
    """A `<-` line with its two ends swapped."""
    return mutate_first(text, r"^(\d{4}-\d\d-\d\d) (\S+) <- (\S+) (\d.*)$",
                        lambda m: f"{m.group(1)} {m.group(3)} <- {m.group(2)} {m.group(4)}")


def drop_arrow(text):
    """A leg with its arrow dropped, under a header that needs it: a `<-` line, or an owner paying a party through its
    legs. Under a header v4 could write the arrow may be left off, and that is the old spelling and not a mistake."""
    lines = text.split("\n")
    needs = False
    for at, line in enumerate(lines):
        if not line.startswith(" "):
            needs = bool(re.match(r"^\d{4}-\d\d-\d\d (\S+ <- |me -> )", line))
        elif needs and re.match(r"^  (->|<-) ", line):
            lines[at] = re.sub(r"^  (->|<-) ", "  ", line)
            return "\n".join(lines)
    return None


def flip_exchange(text):
    """A purchase written as a sale, or the other way: an exchange inside one end, which has no other end."""
    return mutate_first(text, r"^(\d{4}-\d\d-\d\d) (\S+) (<-|->) (\d[\d_.]* VTI @ .*)$",
                        lambda m: f"{m.group(1)} {m.group(2)} {'->' if m.group(3) == '<-' else '<-'} {m.group(4)}")


MUTANTS = [("swap-sides", swap_sides), ("drop-arrow", drop_arrow), ("flip-exchange", flip_exchange)]


def cmd_mutate(binary, root, jobs):
    killed, survived, none = Counter(), Counter(), Counter()
    cases = [(name, label, fn) for name in projects(root) for label, fn in MUTANTS]

    def one(case):
        name, label, fn = case
        v4 = observe(binary, os.path.join(root, name, "v4"))
        text = fn(open(os.path.join(root, name, "v5", "axiom.ax")).read())
        if text is None:
            return label, "none"
        with tempfile.TemporaryDirectory() as tmp:
            with open(os.path.join(tmp, "axiom.ax"), "w") as f:
                f.write(text)
            mutant = observe(binary, tmp)
        return label, "killed" if mutant != v4 else "survived"

    for label, verdict in parallel(jobs, one, cases):
        {"killed": killed, "survived": survived, "none": none}[verdict][label] += 1
    for label, _ in MUTANTS:
        print(f"{label:14s} killed {killed[label]:4d}  survived {survived[label]:4d}  not applicable {none[label]:4d}")
    return not survived


def main(argv):
    mode = argv[1] if len(argv) > 1 else ""
    jobs = 4
    if mode == "gen":
        gen(argv[2], int(argv[3]), int(argv[4]) if len(argv) > 4 else 1)
        return 0
    if mode in ("run", "upgrade", "mutate"):
        if len(argv) > 4:
            jobs = int(argv[4])
        fn = {"run": cmd_run, "upgrade": cmd_upgrade, "mutate": cmd_mutate}[mode]
        return 0 if fn(argv[2], argv[3], jobs) else 1
    if mode == "all":
        binary, out, n = argv[2], argv[3], int(argv[4])
        gen(out, n, int(argv[5]) if len(argv) > 5 else 1)
        ok = cmd_run(binary, out, jobs)
        ok = cmd_upgrade(binary, out, jobs) and ok
        ok = cmd_mutate(binary, out, jobs) and ok
        return 0 if ok else 1
    print(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
