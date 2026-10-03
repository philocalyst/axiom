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
    addresses.py bench SOURCE DESTINATION    a project of bench/gen.py with its accounts written as addresses

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
TODAY_LINE = "2026-12-30"
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
kind gift : plan
  has beneficiary saver optional
kind pot : asset
"""

# The slots each kind of account has beside the owner, in the order the kinds declare them: `gift` adds one to `plan`'s.
SLOTS = {"plan": ["sponsor"], "gift": ["sponsor", "beneficiary"], "pot": []}

SAVERS = ["ann", "bea", "cal", "dee"]
IDLE = ["zed"]  # an entity that fills no slot of any account: `zed/nest` is no address, and so a party the journal makes
KIDS = ["kai", "lou"]
HOUSEHOLDS = ["homeone", "hometwo"]
FIRMS = ["acme", "bluefin", "cobalt"]
BANKS = ["first", "second"]
ENTITIES = {**{e: "saver" for e in SAVERS + IDLE}, **{e: "kid" for e in KIDS}, **{e: "household" for e in HOUSEHOLDS},
            **{e: "firm" for e in FIRMS}, **{e: "bank-co" for e in BANKS}}
NAMES = ["plan", "gift", "pot", "nest", "fund", "cash", "529"]  # 529: a number by itself, so no reference is it alone

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
    def __init__(self, index, kind, name, owner, sponsor, beneficiary, custodian, opened, closed, co_owner=None):
        self.index, self.kind, self.name = index, kind, name
        self.owner, self.sponsor, self.beneficiary, self.custodian = owner, sponsor, beneficiary, custodian
        self.co_owner = co_owner  # `owner ann 60%, bea 40%`: flat books only, a path cannot say a share
        self.opened, self.closed = opened, closed
        self.path = name  # as written; set by `declare`
        self.lines = []

    @property
    def address(self):
        fillers = [self.owner, self.co_owner, self.sponsor, self.beneficiary, self.custodian]
        return [w for w in fillers if w] + [self.name]

    def is_open(self, day):
        """Whether it is open on `day`; on no day (None), whether it is ever open."""
        if self.opened and self.closed and self.closed < self.opened:
            return False
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
    free_all = ["owner"] + SLOTS[account.kind]
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
    if account.co_owner:
        roles = [f"owner {account.owner} 60%, {account.co_owner} 40%" if r.startswith("owner ") else r for r in roles]
    if account.name == "529" and not words:
        account.name = "nest"  # an account declared as a bare number would not parse
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


def flat_name(base, index):
    """A flat book's account is called by one word, and a word of digits alone is a number, not a name."""
    return f"{base}{index}" if not base.isdigit() else f"n{base}-{index}"


def draw_account(rng, index, style, taken):
    for _ in range(50):
        kind = rng.choice(list(SLOTS))
        name = rng.choice(NAMES) if style == "spelled" else flat_name(rng.choice(NAMES), index)
        owner = rng.choice(SAVERS + KIDS + HOUSEHOLDS + FIRMS[:1])
        sponsor = rng.choice(FIRMS) if "sponsor" in SLOTS[kind] and rng.random() < 0.6 else None
        beneficiary = rng.choice(SAVERS + KIDS) if "beneficiary" in SLOTS[kind] and rng.random() < 0.5 else None
        co_owner = rng.choice(SAVERS + KIDS) if style == "flat" and rng.random() < 0.2 else None
        if co_owner == owner:
            co_owner = None
        custodian = rng.choice(BANKS) if rng.random() < 0.6 else None
        opened = day_at(rng.randrange(0, 200)) if rng.random() < 0.35 else None
        closed = day_at(rng.randrange(150, 336)) if rng.random() < 0.25 else None
        if opened and closed and closed < opened and rng.random() < 0.8:
            continue  # now and then an account that is never open: it closes before it opens
        account = Account(index, kind, name, owner, sponsor, beneficiary, custodian, opened, closed, co_owner)
        key = (account.owner, account.co_owner, account.sponsor, account.beneficiary, account.custodian, name)
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


def meant_as_address(accounts, words):
    """Whether no name answered to a reference, and it still may be an address: two words or more that begin with an
    entity that fills a slot, or end in the name of an account. Anything else is a party the journal brings into being."""
    begins = words[0] in ENTITIES and fills_any(accounts, words[0])
    return len(words) >= 2 and (begins or any(a.name == words[-1] for a in accounts))


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
    if not first and not meant_as_address(accounts, words):
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


def typo(rng, words, number):
    """A name that no account has, or, now and then, an entity word that no entity is, or an entity that fills nothing.
    A name is made once for each line (`number`): a party the journal makes of a path is also known by its suffixes, and
    two lines that share one would be one party."""
    pick = rng.random()
    if pick < 0.4 or len(words) < 2:
        return words[:-1] + [f"{words[-1]}x{number}"]
    if pick < 0.6:  # neither word is anything: a party, and not an address
        return [rng.choice([IDLE[0], words[0] + "q"])] + words[1:-1] + [f"{words[-1]}x{number}"]
    at = rng.randrange(len(words) - 1)
    return words[:at] + [rng.choice([words[at] + "q", IDLE[0]])] + words[at + 1:]


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
        if len(words) == 1 and re.fullmatch(r"[0-9_.]+", words[0]):
            continue  # a number cannot be written as a name
        if rng.random() < 0.12 and len(words) >= 2:
            words = typo(rng, words, number)
        answer = resolve(accounts, words, day)
        if answer[0] == "one" and answer[1] is source:
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
        elif answer[0] == "party":
            tally["a party the journal makes: no name, and neither word of an address is an account's"] += 1
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
    entities = sorted({e for a in accounts for e in a.address[:-1]} | set(IDLE))
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
        if entry["answer"] in ("one", "party"):
            balances[accounts[entry["source"]].path] -= entry["amount"]
        if entry["answer"] == "one":
            balances[accounts[entry["target"]].path] += entry["amount"]
    targets = {}
    for target in range(6):
        pool = [rng.choice(accounts)] if accounts else []
        words = rng.choice(list(runs(pool[0].address))) if pool else []
        words = typo(rng, words, 1000 + target) if words and len(words) >= 2 and rng.random() < 0.2 else words
        if not words:
            continue
        answer = resolve(accounts, words, None)  # a report's target has no line, and so no day
        if answer[0] != "party":  # a party is a place too, if the journal mentions it: nothing to say of a target
            targets["/".join(words)] = [answer[0], answer[1].path if answer[0] == "one" else None]
    rungs = Counter(len(a.path.split("/")) - 1 for a in accounts)
    tally.update({f"words in the path: {n}": count for n, count in rungs.items()})
    facts = {"style": style, "accounts": [a.path for a in accounts], "journal": expect, "balances": balances,
             "tally": dict(tally), "targets": targets}
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
        if entry["answer"] in ("one", "party"):
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
    for text, (answer, account) in facts.get("targets", {}).items():
        wrong += check_target(binary, path, name, text, answer, account)
    got = balances(binary, path, name)
    for account, expected in facts["balances"].items():
        if abs(got.get(account, 0.0) - expected) > 0.001:
            wrong.append(f"balance of {account}: expected {expected}, said {got.get(account)}")
    return wrong


def check_target(binary, path, name, text, answer, account):
    """What `register TEXT` says: a report's target has no line, so the accounts it may mean are those ever open."""
    _, out, err = sh(binary, ["register", text, "-C", name], path)
    said = out + err
    if answer == "one":
        return [] if f"Register: {account}\n" in said else [f"register {text}: expected {account}, said {said[:80]!r}"]
    code = {"ambiguous": "error[ambiguous-place]", "unknown": "error[unknown-place]"}[answer]
    return [] if code in said else [f"register {text}: expected {code}, said {said[:80]!r}"]


def apply_fixes(path, facts, to):
    """The book with each ambiguous address replaced by the first the diagnostic offers; what the oracle expects of it."""
    lines = open(os.path.join(path, "main.ax")).read().split("\n")
    balances_ = {a: 1000 for a in facts["accounts"]}
    for entry in facts["journal"]:
        if entry["answer"] == "ambiguous" and entry["code"] == "ambiguous-address":
            number = entry["line"] - 1
            lines[number] = lines[number].replace(f"-> {entry['text']} ", f"-> {entry['fixes'][0]} ")
            entry["answer"], entry["target"] = "one", entry["candidates"][0]
        if entry["answer"] in ("one", "party"):
            balances_[facts["accounts"][entry["source"]]] -= entry["amount"]
        if entry["answer"] == "one":
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


def expected_address(words, free, owner_line, found, kind):
    """The address the account has once its words are placed, by the oracle's own placement: its owner, then what fills
    each slot of its kind in order, then its name. A word that is not placed is not in it."""
    options, every = placements(words, free)
    outcome = forced(words, free) if every and all(options) else None
    slot_of = {}
    for word, slot in zip(words, outcome or []):
        if slot:
            slot_of[slot] = word
    owner = slot_of.get("owner") or ("ann" if owner_line else "me")
    return [owner] + [slot_of[slot] for slot in SLOTS[kind] if slot in slot_of] + [kind]


def check_fills(binary, folder, path, kind, words, free, owner_line, found):
    """What the words filled, which the address says: a second account of the same name makes the name ambiguous, and the
    diagnostic that says so writes out the address of each of the two."""
    decoy_owner = next(e for e in SAVERS if e not in words[-1:])
    with open(os.path.join(folder, "main.ax"), "a") as book:
        book.write(f"account {decoy_owner}/{kind} : {kind}\n{TODAY_LINE} {kind} = 1 USD\n")
    lines = open(os.path.join(folder, "main.ax")).read().split("\n")
    mine = next(number for number, line in enumerate(lines, start=1) if line == f"account {path}")
    said = [d for d in diagnostics(binary, folder) if d["code"] == "ambiguous-address"]
    if len(said) != 1:
        return [f"{path} ({kind}): expected the name to be ambiguous, said {[d['code'] for d in diagnostics(binary, folder)]}"]
    text = next((l["text"] for l in said[0]["labels"] if not l["primary"] and l["line"] == mine), None)
    expected = "/".join(expected_address(words, free, owner_line, found, kind))
    return [] if text == f"`{expected}` is declared here" else [f"{path} ({kind}): expected the address {expected}, said {text}"]


def placement(binary, directory, count, seed):
    """The words before an account's name, against a brute-force placement of them in the slots of its kind."""
    os.makedirs(directory, exist_ok=True)
    rng = random.Random(seed)
    wrong, seen = [], Counter()
    for index in range(count):
        kind = rng.choice(list(SLOTS))
        n = rng.randrange(1, 5)
        words = [rng.choice(list(ENTITIES)) for _ in range(n)]
        owner_line = rng.random() < 0.3
        free = ([] if owner_line else ["owner"]) + SLOTS[kind]
        options, found = placements(words, free)
        path = "/".join(words + [kind])
        book = PRELUDE + "".join(f"entity {e} : {ENTITIES[e]}\n" for e in sorted(set(words) | {"ann"}))
        book += f"account {path}\n" + ("  owner ann\n" if owner_line else "")
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
        wrong += check_fills(binary, folder, path, kind, words, free, owner_line, found)
        shutil.rmtree(folder)
    for w in wrong[:10]:
        print("WRONG", w)
    print(f"{count} accounts, {len(wrong)} wrong; outcomes: {dict(seen)}")
    return len(wrong)


# ─── Mutants of the model ────────────────────────────────────────────────────────────────────────────────

MUTANTS = [
    # (name, file of crates/model/src, the text it changes, what the text becomes)
    # The index
    ("the order of the words is ignored", "addresses.rs", "if later == 0 {\n                return false;\n            }", "if later == 0 {\n                return true;\n            }"),
    ("each word is looked for from the first place", "addresses.rs", "from = later.trailing_zeros() + 1;", "from = 0;"),
    ("the line's day is ignored", "addresses.rs", "(Some(days), Some(day)) => days.contains(day),", "(Some(_), Some(_)) => true,"),
    ("an account that is never open is open on a day", "addresses.rs", "(None, Some(_)) => false,", "(None, Some(_)) => true,"),
    ("with no day, nothing is open", "addresses.rs", "(open, None) => open.is_some(),", "(_, None) => false,"),
    ("the name is not required", "addresses.rs", "let called = &self.called[Id::new(name.index() as u32)];", "let called = &self.called[Id::new(0)];"),
    ("an entity stands only where it last does", "addresses.rs", "fold(0, |bits, (at, _)| bits | 1 << at)", "fold(0, |_, (at, _)| 1 << at)"),
    ("the leftmost fillers are not preferred", "addresses.rs", "(set.count_ones(), !set.reverse_bits())", "(set.count_ones(), set.reverse_bits())"),
    ("the shortest address is the longest", "addresses.rs", "subsets.sort_by_key(|set| (set.count_ones(), !set.reverse_bits()));", "subsets.sort_by_key(|set| (u32::MAX - set.count_ones(), !set.reverse_bits()));"),
    ("an account owned in shares has its first owner only", "addresses.rs", "false => account.shares.iter().map(|share| share.entity).collect(),", "false => vec![account.shares[0].entity],"),
    ("the custodian is not in the address", "addresses.rs", "let all = owners.into_iter().chain(by_slot).chain(institution);", "let all = owners.into_iter().chain(by_slot);"),
    ("the custodian is the first word", "addresses.rs", "let all = owners.into_iter().chain(by_slot).chain(institution);", "let all = institution.into_iter().chain(owners).chain(by_slot);"),
    ("a kind's slots are in the order of the kind beneath", "slots.rs", "lineage.reverse();", ""),
    ("an account closes the day before it says", "addresses.rs", "Days::new(opened, book.fact(builtin::CLOSED, place).unwrap_or(Day::MAX))", "Days::new(opened, book.fact(builtin::CLOSED, place).map_or(Day::MAX, |day| Day(day.0 - 1)))"),
    ("an account opens the day after it says", "addresses.rs", "let opened = book.fact(builtin::OPENED, place).unwrap_or(Day::MIN);", "let opened = book.fact(builtin::OPENED, place).map_or(Day::MIN, |day| Day(day.0 + 1));"),
    # Reading a reference
    ("a name that found several accounts is final", "resolve.rs", "let spelled = places.iter().any(|&place| self.book.is_spelled(place));", "let spelled = false;"),
    ("the index is never asked", "resolve.rs", "if let Some(end) = self.address_end(home, word, day, Reached::Nothing) {", "if let Some(end) = None::<Result<End, Diagnostic>> {"),
    ("a word of an address that is no entity is a party", "reference.rs", "Found::Nothing if reached == Reached::Nothing && attempt(&fillers) => {", "Found::Nothing if false => {"),
    ("an entity that fills nothing begins an address", "reference.rs", "fillers.first().is_some_and(|&first| addresses.fills_any(first))", "!fillers.is_empty()"),
    ("no entity begins an address", "reference.rs", "fillers.first().is_some_and(|&first| addresses.fills_any(first))", "false"),
    ("a name no account has ends an address", "reference.rs", "|fillers: &[Id<Entity>]| called ||", "|fillers: &[Id<Entity>]| true ||"),
    ("an account's name does not make a reference an address", "reference.rs", "|fillers: &[Id<Entity>]| called ||", "|fillers: &[Id<Entity>]| false ||"),
    ("the journal makes a party of an address that ends in an account's name", "declare/parties.rs", "|| path.rsplit_once('/').is_some_and(|(_, last)| self.names.contains(last))", "|| false"),
    ("the journal makes a party of an address that begins with a filler", "declare/parties.rs", "path.split_once('/').is_some_and(|(first, _)| self.fillers.contains(first))", "false"),
    ("a settled reference ignores the days", "reference.rs", "if self.book.lookup.addresses.is_always_open(place) {", "if true {"),
    ("a settled reference is read when several accounts were found", "reference.rs", "let settled = home == Home::Project && reached == Reached::Nothing;", "let settled = home == Home::Project;"),
    ("a suggestion is no number's", "reference.rs", "if !text.contains('/') && numeric(&text) {", "if false {"),
    ("a suggestion ignores the names every account has", "reference.rs", "[only] => *only == place,", "[_] => true,"),
    ("a report's target ignores addresses", "book.rs", "Found::One(place) => return Ok(place),\n            Found::Several(places) => return Err(Miss::Ambiguous(places.into())),", "Found::One(_) => {}\n            Found::Several(_) => {}"),
    # Placing the words before the name
    ("every word goes in the first slot", "spelled.rs", "Placed::Forced(slot) => fills[usize::from(slot)].push(word),", "Placed::Forced(_) => fills[0].push(word),"),
    ("a word that could go two ways is placed too", "spelled.rs", "Placed::Ambiguous(set) => {\n                diags.push(spelling.ambiguous(&world.book, word, set));", "Placed::Ambiguous(set) => {\n                fills[set.trailing_zeros() as usize].push(word);\n                diags.push(spelling.ambiguous(&world.book, word, set));"),
    ("a line that names the owner leaves the owner free", "spelled.rs", ".then_some(Free::Owner);", ".then_some(Free::Owner).or(Some(Free::Owner));"),
    ("the owner takes only a household", "spelled.rs", "Free::Owner => true,\n            Free::Slot { slot: Slot", "Free::Owner => book.name(book.kinds[book.entities[entity].kind].name) == \"household\",\n            Free::Slot { slot: Slot"),
    ("a slot a line fills is free", "spelled.rs", "let slots = book.schema.entity_slots(&book.kinds, kind).into_iter().filter(|&(number, _)| !is_filled(number));", "let slots = book.schema.entity_slots(&book.kinds, kind).into_iter();"),
    ("the owner takes any number of words", "spelled.rs", "Free::Owner => true,\n            Free::Slot { slot, .. } => holds_one(slot.mult),", "Free::Owner => false,\n            Free::Slot { slot, .. } => holds_one(slot.mult),"),
]


def mutate(repo, directory, count, seed):
    """Each mutant of the model, built in a copy of the repository, run through the oracle: killed if anything differs."""
    work = directory + "-mutant"
    gen(directory, count, seed)
    killed, survived = [], []
    only = os.environ.get("MUTANTS")
    for name, file, old, new in MUTANTS:
        if only and name not in only.split(","):
            continue
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


# ─── The bench ───────────────────────────────────────────────────────────────────────────────────────────


def spell_bench(source, destination):
    """A project of `bench/gen.py` with its accounts written as addresses and its journal saying them by the custodian
    (`p1-bank/checking`), which no name of the account spells, so that every such flow end is read by the index. The
    accounts that have no custodian (cash, the 529) keep their names."""
    shutil.copytree(source, destination, dirs_exist_ok=True)
    accounts = open(os.path.join(destination, "accounts.ax")).read()
    renamed = {}

    def account(match):
        name, kind, custodian, rest = match.group(1), match.group(2), match.group(3), match.group(4)
        person, leaf = name.split("-", 1)
        if not custodian:
            return match.group(0)
        employer = re.search(r"  employer (\S+)\n", rest)
        if "beneficiary" in rest:
            return match.group(0)
        words = f"{person}/{employer.group(1)}/{leaf}" if employer else f"{person}/{leaf}"
        renamed[name] = f"{person}/{leaf}" if employer else f"{custodian}/{leaf}"
        kept = re.sub(r"  (owner|employer) \S+\n", "", rest)
        header = f"account {words}" + ("" if leaf == kind else f" : {kind}") + f" at {custodian}\n"
        return header + kept

    accounts = re.sub(r"account (p\d+-[\w-]+) : ([\w-]+) at (\S+)\n((?:  .*\n)*)", account, accounts)
    open(os.path.join(destination, "accounts.ax"), "w").write(accounts)
    pattern = re.compile(r"(?<![\w/#^.:-])(" + "|".join(sorted(map(re.escape, renamed), key=len, reverse=True)) + r")(?![\w/:-])")
    for folder, _, files in os.walk(destination):
        for name in files:
            if name.endswith(".ax") and name != "accounts.ax":
                path = os.path.join(folder, name)
                text = open(path).read()
                open(path, "w").write(pattern.sub(lambda m: renamed[m.group(1)], text))
    print(f"wrote {destination}: {len(renamed)} accounts written as addresses")


# ─── The family ──────────────────────────────────────────────────────────────────────────────────────────

def accept(binary):
    return subprocess.run([sys.executable, os.path.join(os.path.dirname(os.path.abspath(__file__)),
                                                          "family_addresses.py"), "prove", binary]).returncode


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
    if len(argv) >= 4 and argv[1] == "bench":
        spell_bench(argv[2], argv[3])
        return 0
    print(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
