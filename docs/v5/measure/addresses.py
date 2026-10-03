#!/usr/bin/env python3
"""A generator of books of accounts with random slot fillers, and the oracle that holds the CLI to what an address is.

    addresses.py gen DIR N [SEED]            write N projects into DIR (p0000/main.ax ...), each with the expectations
                                             the oracle derives from the book it wrote (p0000/expect.json)
    addresses.py run BINARY DIR [JOBS]       run BINARY over them and check every line against the oracle
    addresses.py all BINARY DIR N [SEED]     gen, then run
    addresses.py placement BINARY DIR N [SEED]
                                             the words before an account's name against a brute-force placement
    addresses.py mutate REPO DIR N [SEED]    build each mutant of the model (MUTANTS below) into a copy of REPO, run the
                                             oracle with it, and say which the oracle kills
    addresses.py accept BINARY               the family's accounts written as addresses against the book written as names

What an address is (docs/v5/lanes/K3b-map.md section 8). An account's address is the entities that fill its slots, owner
first and custodian last, and then its name. A reference is the entities it names, in order, and the name it ends in.
It is read in two steps. First the names every account has, its written path and each trailing run of it: a name
that finds one account means it, on any day; a name that finds several, and one is written as an address, goes to the
second step with the line's day; a name that finds none goes on if it has two words and begins with an entity that
fills something. The second step is the accounts whose address holds the words in order and that are open that day.
One is the answer, several is `ambiguous-address` with each one's shortest address that means only it, none is
`unknown-address`.

The oracle is this file's own reading of that, brute force: it never shares a data structure or a rule's code with the
model. For every journal line it knows which account each end means, or that the end is ambiguous (and which accounts, and
what to write instead of each) or unknown, and so which balances every account ends with. `run` checks the diagnostics of
every line, the balances, and the fixes: a book in which each ambiguous reference is replaced by the first address its
diagnostic offers must say nothing, and put the flow where the oracle says.

Two sorts of book are drawn. In a *spelled* book each account is written with the entities that fill its slots in its
path as far as they can be placed (`ann/acme/ira : plan`), and by role lines for the rest, so its rung of the ladder
varies; names repeat across owners, as `401k` does. In a *flat* book each account has a unique name and every filler is a
role line (`account ann-nest : plan`, `owner ann`), as the old spelling writes it: such an account has an address too.

It is not vacuous, and says so: `gen` and `run` count the references that were one account, ambiguous or unknown, by
which step decided, and the lines that a sibling opening later keeps unambiguous on an earlier day.
"""
import itertools
import json
import os
import random
import re
import shutil
import subprocess
import sys
from collections import Counter
from concurrent.futures import ThreadPoolExecutor

TODAY = "2026-12-31"
FIRST_DAY = 20260101

PRELUDE = """\
use std
base USD
kind saver : entity
kind kid : saver
kind firm : entity
kind bank-co : entity
kind plan : asset
  has sponsor firm optional
  has beneficiary saver optional
kind pot : asset
"""

SAVERS = ["ann", "bea", "cal", "dee"]
KIDS = ["kai", "lou"]
HOUSEHOLDS = ["homeone", "hometwo"]
FIRMS = ["acme", "bluefin", "cobalt"]
BANKS = ["first", "second"]
ENTITIES = {**{e: "saver" for e in SAVERS}, **{e: "kid" for e in KIDS}, **{e: "household" for e in HOUSEHOLDS},
            **{e: "firm" for e in FIRMS}, **{e: "bank-co" for e in BANKS}}
NAMES = ["plan", "pot", "nest", "fund", "cash"]
KIND_NAMED = {"plan", "pot"}

# What each entity's kind fits, as a slot of `plan`: owner takes any entity, sponsor a firm, beneficiary a saver or a kid.
FITS = {"owner": lambda kind: True, "sponsor": lambda kind: kind == "firm",
        "beneficiary": lambda kind: kind in ("saver", "kid")}


def day_text(day):
    return f"{day // 10000:04d}-{day // 100 % 100:02d}-{day % 100:02d}"


def day_at(offset):
    """The day `offset` days after the first of 2026 (28-day months keep it simple and valid)."""
    return FIRST_DAY + (offset // 28) * 100 + offset % 28


# ─── The oracle's reading of placement ───────────────────────────────────────────────────────────────────────


def placements(words, free):
    """Every way to put each word in one of the free slots that fit it, one word to a slot. A brute-force enumeration."""
    options = [[s for s in free if FITS[s](ENTITIES[w])] for w in words]
    found = []
    for choice in itertools.product(*options):
        if len(set(choice)) == len(choice):
            found.append(choice)
    return options, found


def forced(words, free):
    """For each word, the slot every placement puts it in, or None if they disagree. [] when no word fits or none places."""
    options, found = placements(words, free)
    if any(not o for o in options) or not found:
        return None
    return [found[0][i] if len({p[i] for p in found}) == 1 else None for i in range(len(words))]


# ─── A book ──────────────────────────────────────────────────────────────────────────────────────────────


class Account:
    def __init__(self, index, kind, name, owner, sponsor, beneficiary, custodian, opened, closed):
        self.index, self.kind, self.name = index, kind, name
        self.owner, self.sponsor, self.beneficiary, self.custodian = owner, sponsor, beneficiary, custodian
        self.opened, self.closed = opened, closed
        self.path = name  # as written; set by `declare`
        self.lines = []

    @property
    def address(self):
        fillers = [self.owner, self.sponsor, self.beneficiary, self.custodian]
        return [w for w in fillers if w] + [self.name]

    def is_open(self, day):
        """Whether it is open on `day`; on no day (None), whether it is ever open: always, in the oracle's books."""
        return day is None or (self.opened is None or self.opened <= day) and (self.closed is None or day <= self.closed)

    @property
    def spelled(self):
        return "/" in self.path


def slots_of(account):
    """The words an account is filled by, each as (the slot the generator meant it for, the entity)."""
    pairs = [("owner", account.owner)]
    pairs += [(slot, w) for slot, w in (("sponsor", account.sponsor), ("beneficiary", account.beneficiary)) if w]
    return pairs


def path_choices(account):
    """Every subset of the account's words that the placement takes from the path to the slots they were meant for, the
    rest going on role lines, largest first: the rungs of the ladder this account can be written at."""
    free_all = ["owner"] + (["sponsor", "beneficiary"] if account.kind == "plan" else [])
    pairs = slots_of(account)
    found = []
    for size in range(len(pairs), -1, -1):
        for chosen in itertools.combinations(pairs, size):
            lines = {slot for slot, w in pairs if (slot, w) not in chosen}
            free = [s for s in free_all if s not in lines]
            result = forced([w for _, w in chosen], free)
            if result is not None and result == [slot for slot, _ in chosen]:
                found.append(chosen)
    return found


def declare(account, style, rng):
    """The lines that declare the account, in the style of the book, and the path it is written at."""
    if style == "flat":
        chosen = ()
    else:
        choices = path_choices(account)
        chosen = choices[0] if rng.random() < 0.75 else rng.choice(choices)
    words = [w for _, w in chosen]
    roles = [f"{slot} {w}" for slot, w in slots_of(account) if (slot, w) not in chosen]
    account.path = "/".join(words + [account.name])
    omit_kind = account.spelled and account.name == account.kind and rng.random() < 0.7
    out = [f"account {account.path}" + ("" if omit_kind else f" : {account.kind}")
           + (f" at {account.custodian}" if account.custodian else "")]
    out += [f"  {role}" for role in roles]
    if account.opened:
        out.append(f"  opened {day_text(account.opened)}")
    if account.closed:
        out.append(f"  closed {day_text(account.closed)}")
    return out


def draw_account(rng, index, style, taken):
    for _ in range(50):
        kind = rng.choice(["plan", "pot"])
        name = rng.choice(NAMES) if style == "spelled" else f"{rng.choice(NAMES)}{index}"
        owner = rng.choice(SAVERS + KIDS + HOUSEHOLDS + FIRMS[:1])
        sponsor = rng.choice(FIRMS) if kind == "plan" and rng.random() < 0.6 else None
        beneficiary = rng.choice(SAVERS + KIDS) if kind == "plan" and rng.random() < 0.4 else None
        custodian = rng.choice(BANKS) if rng.random() < 0.6 else None
        opened = day_at(rng.randrange(0, 200)) if rng.random() < 0.35 else None
        closed = day_at(rng.randrange(150, 336)) if rng.random() < 0.25 else None
        if opened and closed and closed < opened:
            continue
        account = Account(index, kind, name, owner, sponsor, beneficiary, custodian, opened, closed)
        key = (account.owner, account.sponsor, account.beneficiary, account.custodian, name)
        if key not in taken:
            taken.add(key)
            return account
    return None


class Reference:
    """A word of text that names an end of a flow, and what the oracle says it means on a day."""

    def __init__(self, words):
        self.words = words

    @property
    def text(self):
        return "/".join(self.words)


def tree_order(account):
    """Where the place tree puts an account among roots: by path, `/` the smallest byte."""
    return account.path.replace("/", "\0")


def step_one(accounts, text):
    """The names every account has: its written path, and each trailing run of it. The best rank only."""
    full = [a for a in accounts if a.path == text]
    return full or [a for a in accounts if a.path.endswith("/" + text)]


def fills_any(accounts, entity):
    return any(entity in a.address[:-1] for a in accounts)


def in_order(address, words):
    at = 0
    for w in words[:-1]:
        try:
            at = address.index(w, at) + 1
        except ValueError:
            return False
    return address[-1] == words[-1] and at <= len(address) - 1


def resolve(accounts, words, day):
    """('one', account) | ('ambiguous', [accounts], code) | ('unknown',) | ('party',): what the end means on a day."""
    text = "/".join(words)
    first = step_one(accounts, text)
    if len(first) == 1:
        return ("one", first[0])
    if len(first) >= 2 and not any(a.spelled for a in first):
        return ("ambiguous", first, "ambiguous-place")
    if not first and not (len(words) >= 2 and words[0] in ENTITIES and fills_any(accounts, words[0])):
        return ("party",)
    if any(w not in ENTITIES for w in words[:-1]):
        return ("unknown",)
    found = sorted((a for a in accounts if a.is_open(day) and in_order(a.address, words)), key=tree_order)
    if not found:
        return ("unknown",)
    return ("one", found[0]) if len(found) == 1 else ("ambiguous", found, "ambiguous-address")


def shortest(accounts, account, day):
    """The fewest fillers, leftmost among equals, then the name, that the whole reading takes to this account alone."""
    fillers = account.address[:-1]
    for size in range(len(fillers) + 1):
        for chosen in itertools.combinations(range(len(fillers)), size):
            words = [fillers[i] for i in chosen] + [account.name]
            if len(words) == 1 and re.fullmatch(r"[0-9_.]+", words[0]):
                continue  # a word of digits alone is a number: it cannot be written as a name
            answer = resolve(accounts, words, day)
            if answer[0] == "one" and answer[1] is account:
                return "/".join(words)
    return "/".join(account.address)


# ─── The journal ─────────────────────────────────────────────────────────────────────────────────────────


def runs(address):
    """Every run of an address that ends in its name: each subset of the fillers, in order."""
    fillers = address[:-1]
    for size in range(len(fillers) + 1):
        for chosen in itertools.combinations(range(len(fillers)), size):
            yield [fillers[i] for i in chosen] + [address[-1]]


def typo(rng, words):
    return words[:-1] + [words[-1] + rng.choice("xz")]


def journal(rng, accounts, style):
    """Lines `DAY SOURCE -> TARGET AMOUNT`, the source an account's written path, the target a reference of some rung."""
    lines, expect, tally = [], [], Counter()
    days = sorted(rng.sample(range(0, 336), rng.randrange(12, 28)))
    for number, offset in enumerate(days, start=1):
        day = day_at(offset)
        sources = [a for a in accounts if a.is_open(day)]
        if not sources:
            continue
        source = rng.choice(sources)
        target = rng.choice([a for a in accounts if a is not source] or [source])
        words = rng.choice(list(runs(target.address)))
        if rng.random() < 0.08 and len(words) >= 2:
            words = typo(rng, words)
        answer = resolve(accounts, words, day)
        if answer[0] == "party" or (answer[0] == "one" and answer[1] is source):
            continue
        entry = {"line": None, "day": day, "source": source.index, "text": "/".join(words), "amount": number,
                 "answer": answer[0]}
        ignoring_days = resolve(accounts, words, None)
        if answer[0] == "one":
            entry["target"] = answer[1].index
            by_names = len(step_one(accounts, "/".join(words))) == 1
            tally["one, by the names every account has" if by_names else "one, by the index"] += 1
            if not by_names and ignoring_days[0] != "one":
                tally["one, because the day ruled a sibling out"] += 1
        elif answer[0] == "ambiguous":
            entry["code"] = answer[2]
            entry["candidates"] = [a.index for a in answer[1]]
            entry["fixes"] = [shortest(accounts, a, day) for a in answer[1]]
            tally[f"ambiguous: {answer[2]}"] += 1
        else:
            entry["code"] = "unknown-address"
            tally["unknown-address" + (", though an account has it on another day" if ignoring_days[0] != "unknown" else "")] += 1
        lines.append(f"{day_text(day)} {source.path} -> {'/'.join(words)} {number} USD")
        expect.append(entry)
    return lines, expect, tally


# ─── Writing, running, checking ───────────────────────────────────────────────────────────────────────────


def project(seed, index):
    rng = random.Random(seed * 1_000_003 + index)
    style = rng.choice(["spelled", "spelled", "flat"])
    taken, accounts = set(), []
    for i in range(rng.randrange(3, 9)):
        account = draw_account(rng, i, style, taken)
        if account is not None:
            accounts.append(account)
    # Two accounts written alike would be one declared twice: the written path decides.
    text = [PRELUDE]
    entities = sorted({e for a in accounts for e in a.address[:-1]})
    text += [f"entity {e} : {ENTITIES[e]}" for e in entities]
    seen = set()
    for account in list(accounts):
        lines = declare(account, style, rng)
        if account.path in seen:
            accounts.remove(account)
            continue
        seen.add(account.path)
        account.lines = lines
        text += lines
    for number, account in enumerate(accounts):
        account.index = number  # what the journal says an account is: its place among those that were kept
    opening = ["opening 2026-01-01"] + [f"  {a.path} 1_000 USD" for a in accounts]
    lines, expect, tally = journal(rng, accounts, style)
    book = "\n".join(text + opening + lines) + "\n"
    first = book.split("\n").index(opening[0]) + 1
    numbers = [first + len(opening) + i for i in range(len(lines))]
    for entry, number in zip(expect, numbers):
        entry["line"] = number
    balances = {a.path: 1000 for a in accounts}
    for entry in expect:
        if entry["answer"] == "one":
            balances[accounts[entry["source"]].path] -= entry["amount"]
            balances[accounts[entry["target"]].path] += entry["amount"]
    rungs = Counter(len(a.path.split("/")) - 1 for a in accounts)
    tally.update({f"words in the path: {n}": count for n, count in rungs.items()})
    facts = {"style": style, "accounts": [a.path for a in accounts], "journal": expect, "balances": balances,
             "tally": dict(tally)}
    return book, facts, accounts


def gen(directory, count, seed):
    os.makedirs(directory, exist_ok=True)
    total = Counter()
    for index in range(count):
        book, facts, _ = project(seed, index)
        path = os.path.join(directory, f"p{index:04d}")
        os.makedirs(path, exist_ok=True)
        with open(os.path.join(path, "main.ax"), "w") as out:
            out.write(book)
        with open(os.path.join(path, "expect.json"), "w") as out:
            json.dump(facts, out)
        total.update(facts["tally"])
        total[f"book: {facts['style']}"] += 1
    return total


def sh(binary, args, cwd):
    run = subprocess.run([binary, *args, "--today", TODAY, "--color", "never"], cwd=cwd, capture_output=True,
                         text=True, timeout=120)
    return run.returncode, run.stdout, run.stderr


def diagnostics(binary, path, name="main.ax"):
    _, out, err = sh(binary, ["check", "-C", name, "--json"], path)
    return [json.loads(line) for line in (out + err).splitlines() if line.startswith("{")]


def balances(binary, path, name="main.ax"):
    _, out, _ = sh(binary, ["balance", "-C", name, "--json"], path)
    facts = json.loads(out)["facts"] if out.startswith("{") else []
    return {f["of"]: float(f["value"]) for f in facts if f["concept"] == "balance"}


def line_of(diagnostic):
    return next(label["line"] for label in diagnostic["labels"] if label["primary"])


def check(binary, path, facts, name="main.ax"):
    """What differs between what the CLI says and what the oracle expects of one project."""
    found, wrong = diagnostics(binary, path, name), []
    by_line = {}
    for d in found:
        if d["code"] != "flow-shape":
            by_line.setdefault(line_of(d), []).append(d)
    for entry in facts["journal"]:
        say = by_line.pop(entry["line"], [])
        if entry["answer"] == "one":
            if say:
                wrong.append(f"line {entry['line']} `{entry['text']}`: expected one account, said {[d['code'] for d in say]}")
        elif len(say) != 1 or say[0]["code"] != entry["code"]:
            wrong.append(f"line {entry['line']} `{entry['text']}`: expected {entry['code']}, said {[d['code'] for d in say]}")
        elif entry["answer"] == "ambiguous" and entry["code"] == "ambiguous-address":
            fixes = [f["replacement"] for f in say[0]["fixes"]]
            if fixes != entry["fixes"]:
                wrong.append(f"line {entry['line']} `{entry['text']}`: expected the addresses {entry['fixes']}, said {fixes}")
    for line, say in by_line.items():
        wrong.append(f"line {line}: said {[d['code'] for d in say]}, expected nothing")
    got = balances(binary, path, name)
    for account, expected in facts["balances"].items():
        if abs(got.get(account, 0.0) - expected) > 0.001:
            wrong.append(f"balance of {account}: expected {expected}, said {got.get(account)}")
    return wrong


def apply_fixes(path, facts, to):
    """The book with each ambiguous address replaced by the first the diagnostic offers; what the oracle expects of it."""
    lines = open(os.path.join(path, "main.ax")).read().split("\n")
    balances_ = {a: 1000 for a in facts["accounts"]}
    for entry in facts["journal"]:
        if entry["answer"] == "ambiguous" and entry["code"] == "ambiguous-address":
            number = entry["line"] - 1
            lines[number] = lines[number].replace(f"-> {entry['text']} ", f"-> {entry['fixes'][0]} ")
            entry["answer"], entry["target"] = "one", entry["candidates"][0]
        if entry["answer"] == "one":
            balances_[facts["accounts"][entry["source"]]] -= entry["amount"]
            balances_[facts["accounts"][entry["target"]]] += entry["amount"]
    facts["balances"] = balances_
    os.makedirs(to, exist_ok=True)
    with open(os.path.join(to, "main.ax"), "w") as out:
        out.write("\n".join(lines))
    return facts


def run_project(binary, path):
    facts = json.load(open(os.path.join(path, "expect.json")))
    wrong = check(binary, path, facts)
    again = None
    if any(e["answer"] == "ambiguous" and e["code"] == "ambiguous-address" for e in facts["journal"]):
        fixed = apply_fixes(path, json.loads(json.dumps(facts)), path + "-fixed")
        again = [f"after the fixes: {w}" for w in check(binary, path + "-fixed", fixed)]
        shutil.rmtree(path + "-fixed")
    return path, wrong + (again or []), facts


def run(binary, directory, jobs=3):
    paths = sorted(os.path.join(directory, n) for n in os.listdir(directory) if n.startswith("p") and "-" not in n)
    with ThreadPoolExecutor(jobs) as pool:
        results = list(pool.map(lambda p: run_project(binary, p), paths))
    failed = [(p, wrong) for p, wrong, _ in results if wrong]
    for path, wrong in failed[:6]:
        print(f"WRONG {path}:")
        for w in wrong[:6]:
            print("   ", w)
    seen = Counter()
    for _, _, facts in results:
        seen.update(facts["tally"])
        seen[f"book: {facts['style']}"] += 1
    print(f"{len(paths)} projects, {len(failed)} wrong; what the books held:")
    for what, count in sorted(seen.items()):
        print(f"  {what:<44} {count}")
    return len(failed)


# ─── Placement ───────────────────────────────────────────────────────────────────────────────────────────


def placement(binary, directory, count, seed):
    """The words before an account's name, against a brute-force placement of them in the slots of its kind."""
    os.makedirs(directory, exist_ok=True)
    rng = random.Random(seed)
    wrong, seen = [], Counter()
    for index in range(count):
        kind = rng.choice(["plan", "pot"])
        n = rng.randrange(1, 5)
        words = [rng.choice(list(ENTITIES)) for _ in range(n)]
        free = ["owner"] + (["sponsor", "beneficiary"] if kind == "plan" else [])
        options, found = placements(words, free)
        path = "/".join(words + [kind])
        book = PRELUDE + "".join(f"entity {e} : {ENTITIES[e]}\n" for e in sorted(set(words))) + f"account {path}\n"
        folder = os.path.join(directory, f"q{index:04d}")
        os.makedirs(folder, exist_ok=True)
        open(os.path.join(folder, "main.ax"), "w").write(book)
        codes = Counter(d["code"] for d in diagnostics(binary, folder))
        if any(not o for o in options):
            expected = Counter({"wrong-kind": 1})
        elif not found:
            expected = Counter({"too-many": 1})
        else:
            outcome = forced(words, free)
            expected = Counter({"ambiguous-placement": outcome.count(None)}) if None in outcome else Counter()
            if kind == "plan" and None not in outcome:
                # the beneficiary, if a word fills it, is not missing; if none does, it is optional: nothing is said
                pass
        seen[next(iter(expected), "placed")] += 1
        if "missing-role" in codes:
            del codes["missing-role"]
        if codes != +expected and not (not expected and not codes):
            wrong.append(f"{path} ({kind}): expected {dict(expected)}, said {dict(codes)}")
        shutil.rmtree(folder)
    for w in wrong[:10]:
        print("WRONG", w)
    print(f"{count} accounts, {len(wrong)} wrong; outcomes: {dict(seen)}")
    return len(wrong)


# ─── Mutants of the model ────────────────────────────────────────────────────────────────────────────────

MUTANTS = [
    # (name, file, old text, new text)
    ("order ignored", "addresses.rs", "if later == 0 {\n                return false;\n            }", "if later == 0 {\n                return true;\n            }"),
    ("order from the lowest place only", "addresses.rs", "from = later.trailing_zeros() + 1;", "from = 0;"),
    ("days ignored", "addresses.rs", "(Some(days), Some(day)) => days.contains(day),", "(Some(_), Some(_)) => true,"),
    ("never open when closed", "addresses.rs", "(None, Some(_)) => false,", "(None, Some(_)) => true,"),
    ("no day sees only open accounts", "addresses.rs", "(open, None) => open.is_some(),", "(_, None) => false,"),
    ("the name is not required", "addresses.rs", "lists.push(&self.called[Id::new(name.index() as u32)]);", "lists.push(&self.called[Id::new(0)]);"),
    ("the last filler of an entity only", "addresses.rs", "fold(0, |bits, (at, _)| bits | 1 << at)", "fold(0, |_, (at, _)| 1 << at)"),
    ("first shortest is the last filler", "addresses.rs", "(set.count_ones(), !set.reverse_bits())", "(set.count_ones(), set.reverse_bits())"),
    ("the shortest never shorter than all", "addresses.rs", "subsets.sort_by_key(|set| (set.count_ones(), !set.reverse_bits()));", "subsets.sort_by_key(|set| (u32::MAX - set.count_ones(), !set.reverse_bits()));"),
    ("the owner is the first of the shares only", "addresses.rs", "false => account.shares.iter().map(|share| share.entity).collect(),", "false => vec![account.shares[0].entity],"),
    ("custodian dropped", "addresses.rs", "let all = owners.into_iter().chain(by_slot).chain(institution);", "let all = owners.into_iter().chain(by_slot);"),
    ("custodian first", "addresses.rs", "let all = owners.into_iter().chain(by_slot).chain(institution);", "let all = institution.into_iter().chain(owners).chain(by_slot);"),
    ("slots in the other order", "slots.rs", "lineage.reverse();", ""),
    ("a name that found several is final", "resolve.rs", "if places.iter().any(|&place| self.book.is_spelled(place)) {", "if false {"),
    ("addresses never asked", "resolve.rs", "if let Some(end) = self.address_end(home, word, day, Reached::Nothing) {", "if let Some(end) = None::<Result<End, Diagnostic>> {"),
    ("an unknown word of an address is a party", "reference.rs", "Found::Nothing if at > 0 => return Err(self.unknown_address(word, &fillers, None)),", "Found::Nothing if at > 0 => return Ok(None),"),
    ("every first word is an attempt", "reference.rs", "reached == Reached::Several || fillers.first().is_some_and(|&first| book.lookup.addresses.fills_any(first))", "true"),
    ("no first word is an attempt", "reference.rs", "reached == Reached::Several || fillers.first().is_some_and(|&first| book.lookup.addresses.fills_any(first))", "reached == Reached::Several"),
    ("placement takes the first slot", "spelled.rs", "Placed::Forced(slot) => fills[usize::from(slot)].push(word),", "Placed::Forced(_) => fills[0].push(word),"),
    ("an ambiguous word is placed", "spelled.rs", "Placed::Ambiguous(set) => {\n                        diags.push(ambiguous(world, spelling, word, &free, set));", "Placed::Ambiguous(set) => {\n                        fills[set.trailing_zeros() as usize].push(word);\n                        diags.push(ambiguous(world, spelling, word, &free, set));"),
    ("the owner is not free when a line says it", "spelled.rs", ".then_some(Free::Owner);", ".then_some(Free::Owner).or(Some(Free::Owner));"),
    ("owner takes only a household", "spelled.rs", "Free::Owner => true,\n            Free::Slot { slot: Slot", "Free::Owner => book.name(book.kinds[book.entities[entity].kind].name) == \"household\",\n            Free::Slot { slot: Slot"),
    ("a filled slot is free", "spelled.rs", "let slots = book.schema.entity_slots(&book.kinds, kind).into_iter().filter(|&(number, _)| !is_filled(number));", "let slots = book.schema.entity_slots(&book.kinds, kind).into_iter();"),
]


def mutate(repo, directory, count, seed):
    """Each mutant of the model, built in a copy of the repository, run through the oracle: killed if anything differs."""
    work = directory + "-mutant"
    gen(directory, count, seed)
    killed, survived = [], []
    for name, file, old, new in MUTANTS:
        path = os.path.join(repo, "crates", "model", "src", file)
        text = open(path).read()
        if old not in text:
            print(f"SKIPPED {name}: the text it mutates is not in {file}")
            continue
        open(path, "w").write(text.replace(old, new, 1))
        try:
            built = subprocess.run(["cargo", "build", "--release", "-q"], cwd=repo, capture_output=True, text=True)
            if built.returncode:
                print(f"DOES NOT BUILD {name}\n{built.stderr[-400:]}")
                continue
            binary = os.path.join(repo, "target", "release", "axiom")
            shutil.copy(binary, work)
            broken = run(work, directory, 2) + placement(work, directory + "-placement", count, seed)
        finally:
            open(path, "w").write(text)
        (killed if broken else survived).append(name)
        print(f"{'KILLED  ' if broken else 'SURVIVED'} {name}", flush=True)
    print(f"\n{len(killed)} killed, {len(survived)} survived: {survived}")
    return len(survived)


# ─── The family ──────────────────────────────────────────────────────────────────────────────────────────

# What each account of examples/05-family is called by names, and how the copy writes it.
FAMILY = [
    # (old name, new path, kind written, custodian, role lines)
    ("joint-checking", "family/checking", "deposit", "chase", []),
    ("joint-savings", "family/savings", "deposit", "chase", []),
    ("escrow", "family/escrow", None, "lender", []),
    ("alex-401k", "me/acme/401k", None, "fidelity", []),
    ("jordan-401k", "jordan/bluefin/401k", None, "fidelity", []),
    ("hsa", "me/hsa", None, "fidelity", ["coverage family"]),
    ("dcfsa", "family/dcfsa", "dependent-care-fsa", "acme", []),
    ("riley-529", "family/riley/529", "529-plan", "fidelity", []),
    ("mortgage", "family/mortgage", None, "lender", []),
    ("car-loan", "family/car-loan", "loan", "honda-finance", []),
    ("card", "family/card", "credit-card", "chase", []),
]


def shortest_stable(old, text_of):
    """The shortest reference of an account that is unique among all accounts ever declared."""
    raise NotImplementedError


def accept(binary):
    from family_addresses import main  # the acceptance copy is written by its own script; see there
    return main(binary)


def main(argv):
    if len(argv) >= 5 and argv[1] == "gen":
        total = gen(argv[2], int(argv[3]), int(argv[4]) if len(argv) > 4 else 1)
        print(f"wrote {argv[3]} projects to {argv[2]}; what they hold:")
        for what, count in sorted(total.items()):
            print(f"  {what:<44} {count}")
        return 0
    if len(argv) >= 4 and argv[1] == "gen":
        total = gen(argv[2], int(argv[3]), 1)
        for what, count in sorted(total.items()):
            print(f"  {what:<44} {count}")
        return 0
    if len(argv) >= 4 and argv[1] == "run":
        return 1 if run(argv[2], argv[3], int(argv[4]) if len(argv) > 4 else 3) else 0
    if len(argv) >= 5 and argv[1] == "all":
        gen(argv[3], int(argv[4]), int(argv[5]) if len(argv) > 5 else 1)
        return 1 if run(argv[2], argv[3]) else 0
    if len(argv) >= 5 and argv[1] == "placement":
        return 1 if placement(argv[2], argv[3], int(argv[4]), int(argv[5]) if len(argv) > 5 else 1) else 0
    if len(argv) >= 5 and argv[1] == "mutate":
        return 1 if mutate(argv[2], argv[3], int(argv[4]), int(argv[5]) if len(argv) > 5 else 1) else 0
    if len(argv) >= 3 and argv[1] == "accept":
        return accept(argv[2])
    print(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
