# K3b map: what a path of words means today, where a name is resolved, and what an address needs beside it

Written before the first code change of lane K3b, from the code at `0089678`, and checked against what the code does (the
last section says how). Paths are in `crates/`; line numbers are those of `0089678`.

An **address** is the path of the things that fill an account's slots, then its name: `jordan/bluefin/fidelity/401k`. A
**reference** is any subsequence of an address that ends in the name and denotes one account open on the line's day.
A declaration's words before its name fill the account's slots by forced placement. The brief asks whether the model can
do this with no change to the grammar. It can, and section 1 shows why. The one piece that needs the grammar is nesting,
and section 7 says what it would cost and why this lane does not stop for it.

## 0. What the brief says and what the code does

Five things in the brief do not match the code. Each decides something below.

1. **A path is already one token, and already a tree.** `jordan/bluefin/401k` is one `Tok::Name` (the lexer joins segments
   with `/`, and a segment may start with a digit), in a declaration, a flow end, a statement subject and a property
   argument alike. As an account's name it already makes three places: `jordan`, `jordan/bluefin` and the account. So the
   "words are fillers first, tree segments second" order of the brief is a decision about what the *declaration* means
   (section 8.1), because for a reference the tree reading is only suffixes (section 1).
2. **`jordan/401k` does not fail today; it makes a party.** Nothing declares it, so `implied_parties` (declare/parties.rs:118)
   makes an entity of it, under `jordan` in the entity tree, and the flow goes to that entity. A typo in the new spelling
   would silently open a party. That is the behaviour the new spelling has to stop, and it is in the pass that runs
   *before* places and entities exist (section 8.5).
3. **`owner` is not a slot, and neither is `at`.** `owner` is a built-in line read at declaration time into
   `Place.owner` and `Place.shares` (declare.rs:710), and `has owner …` is refused (`FIELD_WORDS`, slots.rs:121). `at`
   fills `Role::Account { institution }`, which nothing reads (K3a map §10). Of the five relation lines the brief names,
   only `employer` and `beneficiary` are slots, and `coverage` takes words, not things (section 4). The design's
   `owner` and `with` are two slots every position has; here they are two fields the address reads.
4. **There is no `as with` marker, and the `has` grammar cannot say one.** `has NAME TAKES [MULT] [by WEIGHT]`
   (syntax/decl.rs:88). A path word cannot fill a custodian by type, because no kind says which slot is the custodian.
   `at` is the one existing way to say it (section 7).
5. **The line's day is not passed to a name.** `World::end(home, word)` has no day (resolve.rs:207). The journal's
   `FlowCx` has one (lower/flow.rs:53), and so does `Stated` through its statement. "Open on the line's day" is a new
   parameter on three callers, not a filter on an existing one.

## 1. What `jordan/bluefin/401k` is today

Checked on scratch books against the baseline binary (section 9).

| stage | what it does with `jordan/bluefin/401k` |
|---|---|
| lexer (syntax/lex.rs:345, `name`) | one `Tok::Name`. `/` joins segments when a name byte follows; `401k` is a name byte sequence. `family/529` the same. A path with an uppercase letter is `Malformed::Word` |
| parser, a declaration (syntax/decl.rs:30) | `account NAME [: KIND] [at NAME]`. The name is one path. The kind is `name_like`, so `401k` and `529` are kinds. A bare word after the header (`family/529 riley`) is `expected-end-of-line`, which the design drops anyway |
| parser, a reference | a flow end, a statement subject and a property argument (`ExprKind::Name`) are each one name |
| `declare_accounts` (declare/holdings.rs:26) | one `AccountDraft` per account: path, class (from the kind), kind (the written one, else the root `asset`), owner (the `owner` line, else `me`), `institution` (`at`) |
| `places::declare` (declare/places.rs:42) | `keys()` adds **every prefix** of the path as a place, so the account sits under `jordan/bluefin` under `jordan`. A prefix with an account beneath it is indexed, so `jordan` is also the name of a place |
| `Names<Place>` (names.rs:95) | the full path (`Rank::Path`) and each suffix after a `/` (`Rank::Suffix`): `jordan/bluefin/401k`, `bluefin/401k`, `401k`. A suffix shared by two accounts is ambiguous, whatever day it is |
| reference `jordan/bluefin/401k`, `bluefin/401k`, `401k` | the account, when unique. This is a *contiguous trailing* subsequence of the words |
| reference `jordan/401k` | **no place**. `found_end` finds none, `commodity_end` none, then `entity()`, which finds the implied entity `jordan/401k` that `implied_parties` made of the mention |
| reference `jordan` | `ambiguous-end`: "`jordan` could mean account `jordan` or entity `jordan`", from the prefix place |

The last row is a defect of reading a leading word as a tree segment when it names a person. In `balance`, `family/checking` and
`family/savings` roll up under a `family` row, and a flow written `family -> …` (the household as a party) is
`ambiguous-end`. The address reading makes each leading word a filler, so the account is a root and no prefix place
exists for it (section 8.2).

**The answer to the decisive question.** The model can do the new spelling with no change to the grammar. Everything the
brief spells, `account jordan/bluefin/401k : 401k`, `jordan/401k` in the journal, `jordan/401k employer bluefin` as
the rung that names a role, `account jordan/401k : 401k` with every line written as today, is a token the grammar already
accepts. What changes is meaning, in `declare` and `resolve`. What the grammar does *not* offer is in section 7.

## 2. Every place a name is resolved

`World::end` is the entry for anything that is a flow's end. It tries four things in order: `special_end` (`?`, a loan
contract's name, a contract's name, an asset's name), `found_end` (`Names<Place>` and the visible entities together),
`commodity_end` (an issuer's place), then `entity()` (a party). `World::place`/`seek_place` look in `Names<Place>` alone.
`World::entity`/`seek_entity` look in the scoped entity table, which holds the implied parties too.

| what is written | file, function | looked up as |
|---|---|---|
| a flow's end, in a transaction, an opening line, an occurrence | lower/flow.rs `resolve_end` (437), the selectors under it (449) | `World::end`; has `cx.day` |
| a statement's subject (`checking = 5 USD`, `hsa end`, a balance) | lower/statements.rs `named_target` (193) | an asset, a loan's debt, then `World::end`; has the statement's day |
| the place a gap went `via` | statements.rs `assertion_gap` (358), 653 | `World::end` |
| the object a clause names (`for`, `of`, `via`, `into`) | lower/tail.rs (107-112, 176, 187) | an entity, or `end`, or a place by reach |
| an `also` line's end, and its selectors | lower/also.rs (56, 279, 373) | `World::end` or an entity |
| a contract's party, its holding and its `with` | lower/contracts.rs (132, 1024, 1535) | an entity; the holding by `end` |
| a claim's debtor and creditor | lower/record.rs (1332) | an entity |
| a law's `on` place, a selector in a law | laws/mod.rs (128, 132), laws/compile.rs (434, 679-691, 787) | a place, an entity, or `end` |
| a budget's `funded from … into` | laws/budget.rs (355-356) | a place |
| a value of a slot that takes things | fill.rs `thing` (148-150) | an entity, or a place by `World::place` |
| an expression of type place or entity | values.rs (218-219) | an entity, a place |
| a property's argument (`owner`, `member`, `via`) | props.rs (383, 388, 442); declare.rs `owners` (727) | an entity; `via` a place |
| the declared thing itself | props.rs `native_target` (948) | `World::place` by the account's own path |
| a statement's subject, when it is a property change | props.rs `subject_candidates` (994) | `Names<Place>.find` and the entity table |
| a sync account and a bearer | sync_lower.rs (362-364, 413, 926) | an entity or a place |
| a CLI target (`register jordan/401k`) | report/resolve.rs:18, `Book::place` (book.rs:1421) | `Names<Place>`, then an entity's place |
| a kind | kinds.rs `find`, `World::kind`/`seek_kind` | the scoped kind table; never a place |

Of these, **only the journal's** (the first four rows) have a day in hand, and only they are lines. The rest name a
thing as a declaration or a setting does, with no day. So address resolution takes `Option<Day>`-like input in two
forms (section 8.4): a day, for a line of the journal; and no day, for a setting or a CLI target, which sees every account
that is ever open.

## 3. What `holders.rs` and `scope.rs` do

`holders.rs` numbers the things a book says things about (kinds, entities, commodities, assets, places) end to end, so the
facts store has one dense row per holder. Places are last because they are the one arena that grows after the numbering
(a claim tab, K3a). `Addresses` keys nothing by holder number: it is by `Id<Place>`, which is the place's pre-order
number, and a place is declared before the numbering is made, so the index is built once after the facts are frozen
and never grows. A tab is not an account and is not in it.

`scope.rs` says what a home (the project, `std`, a system) sees. Places have no scope: `Names<Place>` is not a `Scoped`.
Entities and kinds do, so a word of an address resolves as an entity *in the home of the line that wrote it*, and a kind
name as a kind in that home. The index itself is not scoped. It is made of the entities' own interned names, and the
resolution of each written word to an entity happens at the call site, which has the home.

## 4. How a declaration reads the words after the name, and what each relation line is

`decl` reads the name, then `: KIND` (`name_like`), then, for an account, `at NAME`, then the indented lines: `law`,
`also`, `known-as`, `has`, and everything else is `property` (`NAME ARG*` with lines under it). That is all a declaration
has to carry words. The words of a declared account are therefore: the words of its path, the kind, the `at` name, and the
arguments of its property lines.

| line | what it is today | slot of which kind |
|---|---|---|
| `owner X [N%], …` | a built-in line. `Resolving::owners` (declare.rs:710) reads it before any place exists, into `Place.owner` and `Place.shares`. `owner` also decides whether the entity *holds* (`Entities.holds`, declare/parties.rs:229), which makes its own place `Role::Holding` instead of `Role::Outside` | the `owner` of every position. Not a schema slot |
| `at X` | `AccountDraft.institution`, then `Role::Account { institution }`, which no reader consumes | the `with` of every position. Not a schema slot |
| `employer X` | a slot, `has employer employer optional` on `401k` (systems/us/401k.ax:28), range the kind `employer`, filled by `fill::thing` and said into the facts | `employer` of `401k` |
| `beneficiary X` | a slot, `has beneficiary person` on `529-plan` (us/529.ax:30), exactly one | `beneficiary` of `529-plan` |
| `coverage W` | a slot that takes **words**, `one of self-only \| family` on `hsa` (us/hsa.ax:29) | not a filler: a word is not a thing |
| `owner family` on `dcfsa`, `escrow`, `mortgage`, `car-loan`, `card` | the owner line | `owner` |

These are the only entity slots in `std` and `us`. `K12` types them, counts them and says a missing one (`missing-role`), but
the schema is built *after* the places are (`slots::declare` runs after `declare::declare`, lib.rs:101), and nothing
about a place's tree shape or its owner can wait for it. Section 8.3 says how the placement passes that.

## 5. What the facts and the schema know, to be reused

- **The schema** (`slots.rs`): `Schema::effective(kinds, kind)` yields every slot of a kind, nearest declaration first, each
  name once; `Slot { name, range: Kinds | Words | Value, mult }`; `View::Kinds` gives the range's kinds, and
  `Book::is_a(kind, of)` is the test a word's kind passes. Placement needs exactly these, so it reads them and keeps no copy.
- **The facts** (`core::facts`): what a role line said is a value of a slot of a place, on the days it holds. The index
  reads `employer`, `beneficiary` and the like from there, one read per slot per account, once, after the first freeze
  (lib.rs:103). `OPENED` and `CLOSED` are two of the language's own slots (builtin.rs): a place's days of being open come
  from them, with no new storage.
- **`core::placement`** is complete and tested (`place(cand, single) -> Placement`) and needs nothing added: at most eight
  words, sixteen slots, unit propagation then a search that records where each word can land.
- **`core::postings`** has `intersect_all(&mut [&[u32]], out)`, shortest list first, galloping where the lists are far
  apart and the SIMD block kernel where they are long (C3). Lists must be strictly increasing, which is why an account
  that holds one word in two slots has *one* entry for that word, with a bit per position (section 8.6).
- **`Groups<K, V>`** (core::groups) is the compressed row: `Groups<Word, u32>` holds the posting lists, and a second
  `Groups<Word, u16>` built from the same pairs holds, beside each id, where in the address the word stands.
- **`problem.rs`** is the catalog: `unknown(Noun, word, nearest)` and `ambiguous(Noun, word, &[Candidate])` give
  `unknown-address` and `ambiguous-address` when `Noun::Address` exists, the fix as an edit of the written word.

## 6. The cost of a lookup today, and of the intersection

Today a flow end is `World::end`: `special_end` does an interner lookup of the text for `contract_endpoints`, then
`book.contract`, `book.asset` and `book.commodity` each look the text up again (each a hash of the whole text); then
`found_end` does one for `Names<Place>` and one for the entity table; then `entity()` another. A hit is found in about six
hashes of one string. From `bench/perf/callgrind-inclusive-100k.txt` (a 100k-flow `check`): `World::end` is **9.9%** of all
instructions, `Interner::get` 9.1% inclusive, and with about two ends a flow that is about 900 instructions an end.

An address reference adds, once, for a path that nothing else claims: one interner lookup per word, one `Groups` row per
word, `intersect_all` over `k` short lists, and a filter of what is left by day. Lists are as long as the number of
accounts a filler fills a slot of, which is a handful (a household's accounts), at most thousands (`me` owns them all, so
`me`'s list is the whole book: a gallop from the shortest list, `k log(n/k)`). Neither path is on the route of a book that
does not use the new spelling: a name found by `Names<Place>` never reaches the index, so **the hot path of today's books
gains no instruction** except one branch in `found_end`'s miss and ambiguity arms.

## 7. What the grammar would need, and why this lane does not stop for it

**No grammar change is needed for the address spelling.** Two things the design says are grammar changes. This lane builds
neither, and the rest does not depend on them.

1. **Nesting.** `entity fidelity : broker` with `alex/401k` under it, to fill the custodian by the container. Today a line
   under an entity is a *property* (`decl_line`, decl.rs:83), so `alex/401k` under `entity fidelity` is a property named
   `alex/401k`. The smallest change: in an `entity` block, a line whose first word is a path that is not a built-in
   property name, and is followed by `: KIND`, a `has` slot, or nothing but an end of line, is a nested declaration of an
   account (`Decl { what: Account, at: Some(container) }`). The ambiguities it introduces, each an example:
   - `owner family` is a property and `family/529` a nested account, but `family` alone, with no `/`, is either a property
     with no argument or an account named `family`; the rule needs the `/` or a `: KIND`;
   - a book that has a property and an entity of one name (`purpose`, `known-as`) reads the built-in first, so a nested
     account cannot be named like one;
   - `riley/529` under `entity fidelity` and `account riley/529 at fidelity` mean the same thing, so `at` is the
     existing spelling of the same slot, and nesting adds a second way to say it.
   Because `at` already says it, the acceptance copy writes `at fidelity`.
2. **The `as with` marker on a `has` line** (`has custodian broker as with`), so a kind may say which slot a path word fills
   as the custodian, and its range. Without it the custodian is not a placement target: **a path word never fills `with`**.
   If it did, every owner word would fit two slots (any entity can be an owner and a custodian) and no declaration would
   place. `with` is filled by `at` only, and reads as a word of the address all the same.

Both are L's, together with the kind-narrowed `owner` (`has owner person | household`, forbidden today by `FIELD_WORDS`).
Until `owner` has a range, a path word can fill `owner` whatever it is: `acme/529` places `acme` as the owner. The check
the design wants (`wrong-kind`: an employer is not a person or household) comes with that range, and is not built here.

## 8. The design this map commits to

### 8.1 The gate: when the words of a path are fillers

An `account` declaration is **address-spelled** if its path has two words or more and **every word but the last names one
entity visible from the declaration's home**. Anything else (`assets/bank/checking`, where `assets` is no entity) keeps its
tree reading, unchanged. The corpora have no account whose leading words are all entities (examples, mistakes, the diff
cases, the bench, and the unit tests: 0 of 33 projects, counting every entity name written anywhere in a directory), so
the gate changes no book we have. The one class it can change is an old account whose path begins with an entity's name
(`account chase/checking : deposit` with `entity chase`): it used to be owned by `me` and grouped under `chase`, and is
now owned by `chase`, by the forced placement the design asks for. That class is silent, and a report says it.

If the declaration has no `: KIND` and the last word names a place kind, the kind is that kind (`alex/401k` is a 401k).
With a `: KIND`, any name goes (`family/checking : deposit`).

### 8.2 The tree shape

An address-spelled account is a **root of the place tree** whose path is the whole written path, with no prefix places.
Its `Names<Place>` entries are the ones every path has (the path and each suffix), so every reference that resolved
by suffix still does. This removes the `family`/`jordan` rows and the ambiguity of section 1's last row for it.

### 8.3 Placement is a pass after the schema, and what it fixes up

The words cannot be placed when the accounts are made, because the slots' ranges are not yet declared. The gate and the tree
shape need entities only, so they are decided in `declare_accounts`. The placement itself is a pass in `props::declare`,
after the role lines have been said (so it knows which slots a line already filled) and before `missing_roles` (which
must not say a slot missing that a word fills):

- the **free slots** of an account are `owner` (if there is no `owner` line) and each slot of its kind whose range is kinds
  and which no line filled; `with` is never free (section 7);
- each leading word's candidates are the free slots its entity's kind fits (`Book::is_a`); `owner` takes any entity;
- `core::placement::place` decides. A word `Forced` into a slot fills it: `owner` sets `Place.owner` and empties
  `Place.shares`, a declared slot is said into the facts exactly as a role line says it; a word `Ambiguous` is
  **`ambiguous-placement`**, naming its slots and the role word that settles it; a word with no candidate or no
  placement is `wrong-kind` and `too-many`, the diagnostics K12 already has for a role line, built from the same words;
- an owner that is now the owner of an account **holds**: its own place becomes `Role::Holding`, as `Entities.holds` would
  have made it had the owner been written as a line. This is one small fix-up of the place the entity already has, and it
  is the only thing the pass changes that was made earlier.

### 8.4 Resolution: old first, then the index, then as before

For a flow end (`World::end_on(home, word, day)`) and a setting (`World::end(home, word)`, no day):

1. `special_end`, then `found_end` as today. If it finds exactly one place or a party, that is the answer: **a name that
   resolved before resolves to the same thing**, and nothing declared later changes it.
2. If `found_end` says several places, and any of them is address-spelled, or, **in a book that writes some account as an
   address** (section 8.7), finds nothing and the path has two words or
   more and it begins with a filler (an entity some account has in a slot) or ends in the name of an account (a
   misspelt first word of `jordan/bluefin/401k` is still an attempt at it): the **address resolution**: resolve the
   words as entities in the line's home, intersect their posting lists with the name's, filter by order and by open on the
   day. One account: the answer. Several: **`ambiguous-address`** with each candidate's full address and its
   **shortest unique address** as the edit. None: **`unknown-address`**, with the closest address of what the leading words
   fill, and a note when a candidate exists but is not open that day. A reference with no day sees every account.
3. Otherwise, as today.

**Open on the line's day** means `OPENED <= day` and not past `CLOSED`, read from the facts at build time into one
`Days` per account. An `end` statement says `closed` after lowering and is not seen by a line before it: a line sees what the
declarations say, which is the line's day deciding, as a relator's `from` and `until` do.

### 8.5 The implied-party pass must know what an address is

(Only in a book that writes some account as an address, section 8.7.)

`implied_parties` runs before entities exist and makes a party of every undeclared name a journal mentions. A mention such
as `jordan/chekcing` would become a party. So the pass is given one more set of names it does not make parties of: a
mention of two words or more whose first word is a **filler word**, a word that some account declaration writes as a leading
path word, an `at` name or an argument of a non-built-in line, **or whose last word is the name of an account**. It
over-approximates (a word that turns out to fill no slot only means the mention is read as an address attempt and is
`unknown-address` instead of a party). The corpora have no mention of that shape that is not already declared, so it
changes no book we have; it is measured, not argued (section 10).

### 8.6 `Addresses`

```rust
pub(crate) struct Addresses {
    fills: Groups<Entity, u32>,   // for each entity, the accounts it fills a slot of, by place number
    stands: Groups<Entity, u16>,  // beside each: one bit for each position the entity has in that address
    called: Groups<Called, u32>,  // for each name, the accounts called it
    address: Groups<Place, Part>, // each account's address in order, the name last: for a diagnostic and a shortest unique
    open: Vec<Option<Days>>,      // by place number: the days it is open; none if it closes before it opens
    once: Map<Sym, Id<Place>>,    // the references a journal writes that no day or home can change, worked out once
}
```

A filler is the entity its word is in the line's home (so a scoped or nested entity is the one the line sees), and the
posting list of an entity is the accounts it fills, with the bit of each position it holds; the name's list is the accounts
called it. Order is checked greedily on the bits (the lowest unused position past the last). An account that holds an
entity in two slots has one entry with two bits. At most 16 words an address. The words that can be written are the
entities whose paths have no `/` (DESIGN §2.4: "entities and assets keep flat names"); a nested entity can fill a slot by a
role line and does not appear in an address.

### 8.7 The book's gate: no spelled account, no address

A book in which no account is written with the entities that fill its slots before its name reads every reference as it
always did, byte for byte. The index is not asked, `settle_addresses` does nothing, the CLI and the settings take no
address, and the implied-party pass keeps no mention from being a party. This is what "additive" means for the two rules
of 8.4 and 8.5 that are not about a spelled account: without it, a party written as a path that ends in an account's name
(`clients/escrow`, with `account bank/escrow`) or begins with an entity that fills a slot (`jordan/landlord`, with `owner
jordan` on some account) would be `unknown-address`, where it was an implied party. The gate is one flag of `Addresses`
(`is_used`, any place `is_spelled`) and one of the implied-party pass (`Addressed.used`, any account whose leading
words are all written entities, by text). In a book that does write one, an account written the old way has an address
too (`bluefin/jordan-401k`), and a mention of that shape is an address and no party.

## 9. What this lets L delete

The `owner` line of every account whose owner is a path word (12 of the 16 relation lines under accounts in
`05-family`, 12 in `09-shared`, 2 in `04-freelancer`), the
`employer X` and `beneficiary X` lines that a word places, and every account name that carries a relation (`jordan-401k`,
`riley-529`, `joint-checking`). It does not delete `at`, until the `as with` marker and nesting exist.

## 10. How this map was checked

- Four scratch books on the baseline binary (a copy of `0089678`'s `axiom`): the account `jordan/bluefin/401k : 401k at
  fidelity` and the references `jordan/bluefin/401k`, `bluefin/401k`, `jordan/401k`, `401k`, `jordan`; a bare word in a
  header; and `family/529 riley`. They gave the table of section 1: `jordan/401k` became the implied entity `jordan/401k`
  and made `401k` ambiguous; `jordan` was `ambiguous-end` with the prefix place; the balance tree had a `jordan` row.
- `grep` of every resolver named in section 2, and `bench/perf/callgrind-inclusive-100k.txt` for section 6.
- A script over `examples/`, `tests/mistakes/`, `docs/v5/measure/diff/` and `bench/` (33 projects, every `entity` name
  written in each directory, every `account` path of two words or more): 0 accounts whose leading words are all entities,
  and `crates/*/src` has three tests with a `/` in an account name (`personal/checking`, `business/checking`,
  `bank/checking`), none of whose leading words is an entity.

## 11. What the lane built, where it departs from section 8, and what it measured

### 11.1 Built

- `model/src/addresses.rs`: `Addresses`, the index of section 8.6, resolved by intersection in order and by day.
- `model/src/spelled.rs`: the gate (`leading`, `Book::is_spelled`) and the placement pass (`place_words`) over
  `core::placement`.
- `model/src/reference.rs`: a written reference read as an address, `settle_addresses`, and the two diagnostics
  `unknown-address` and `ambiguous-address` (each candidate with the shortest address that means only it).
- `model/src/problem.rs`: `ambiguous-placement`, and `wrong-kind` and `too-many` for a word before a name, built from the
  words the role-line versions of them use.
- Small edits: `resolve.rs` (the line's day reaches `found_end`; `seek_place` and `Book::place` take an address),
  `declare/*` (a spelled account is a root; an account's kind from its last word; the party pass keeps mentions that are
  addresses from being parties), `lower/flow.rs` and `lower/statements.rs` (the day is passed), `slots.rs`
  (`entity_slots`, root-first), and one line of `report/places.rs` (a spelled account's row is its whole path).
- `tests/mistakes/100` to `104`: a case for each diagnostic, with the output the tool gives.
- `docs/v5/measure/addresses.py`: the generator and the brute-force oracle, with the mutants; `family_addresses.py`: the
  acceptance copy; `examples/explore-v5/06-family-addresses`: that copy, which checks to the balances, tallies, claims
  and limits of `examples/05-family` (which is not edited). `diff/cases2/old-party-path.ax`: the old shape the gate keeps.

### 11.2 Where it departs from section 8

1. **The index is keyed by entity, not by the symbol of a word.** A filler is the entity its word is in the line's home
   (a scoped or nested entity is the one the line sees), and a word that is no entity cannot be a filler, so a posting
   list for a word that is not an entity would only hold nothing. The name's list is its own (`called`).
2. **The gate of 8.7 is new.** Section 8.4 and 8.5 as first written changed the meaning of a party written as a path in a
   book that has no spelled account. The corpora do not hold the shape, so no golden said so; a case written for it
   (`old-party-path.ax`) does, and the gate keeps it.
3. **"Ends in an account's name" is a second reason to read a reference as an address** (8.4, 8.5), beside "begins with
   a filler": a misspelt first word of `jordan/bluefin/401k` is a mistake in an address and no party.
4. **`settle_addresses`** works out once, before the journal is lowered, what each reference of two words or more means
   when no day or home can change it (one account, open on every day). A hit is one hash of the text. It is the cost
   of the new spelling at 100k flows brought from +12.6% to +1.75% (1m: +1.90%).
5. **The shortest address is taken by the whole reading**: the fewest fillers, the leftmost among equals, the name always,
   and never a word of digits alone (a number is no name), judged by what the names every account has and the index
   say together, not by the index alone.
6. **A setting or a report has no line and so no day**: a reference of an account that is open on no day is unknown, and
   one that two accounts share but only one is ever open means that one (`Book::place`, `seek_place`).
7. **`opened` and `closed` are read from the facts when the index is built.** An `end` statement says `closed` after
   lowering, and a line before it does not see it, as a relator's `from` and `until` are read.

### 11.3 Not built, and not right yet

- Nesting (`entity fidelity` with `alex/401k` under it) and the `as with` marker (section 7): the custodian is still `at`.
- A kind-narrowed `owner`: until `owner` has a range, `acme/529` places `acme` as the owner and says nothing. Section 7.
- The CLI's own message for a target two accounts share is the old one (`ambiguous-place`, by suffix); a setting says
  `ambiguous-address` only where the names found none.
- `unknown-address` suggests the closest name only, not the closest address.
- The party pass decides by the text of the sources, before the entities exist: an over-approximation, said in 8.5.
- Entities whose path has a `/`, and addresses of more than 15 words, are not in the index.
- An account whose path begins with an entity's name used to be owned by `me` and grouped under that name; in a book it is
  now owned by that entity. Section 8.1. No corpus holds one.
- The owner's own place becomes a holding when it owns a spelled account (`own()` in `spelled.rs`), the one fix-up of a
  place made earlier. The oracle does not model it; a unit test does.

### 11.4 Measured

Numbers are of the tree after the last commit unless said otherwise.

- **Lines** (`briefs/loc.py`, before to after): core 3461 to 3472, model 17961 to 18819, report 7088 to 7091; the other
  crates unchanged; total 52079 to 52951 (+872). The design asked for +250 and the oracle's support in the model is not
  in it: nothing has been deleted yet (section 9 is L's).
- **Functions** (`hist.py crates`): 41-80 lines 137 to 136 functions, nothing else moved by more than the new functions
  (1908 to 1954 of 1-10 lines, 666 to 681 of 11-20, 487 to 497 of 21-40). 70 new functions, the longest 32 lines
  (`Addresses::build`); the three functions the lane grew past 38 lines are 40 (`resolve_end`, `declare`) and 38
  (`found_end`).
- **Lookup cost** (callgrind instructions of `check` on `bench/gen.py` projects, baseline `0089678` against this tree):
  100k flows 1,855,756,956 to 1,857,625,340 (+0.10%); 1m flows 17,148,503,376 to 17,163,680,209 (+0.09%). The same
  projects written as addresses (`addresses.py bench`): 100k 1,888,223,514 (+1.75%), 1m 17,474,812,742 (+1.90%).
- **Tests**: `cargo test --workspace --release`: 1046 passed, 4 failed. Three are the known failures. The fourth,
  `claim_tests::a_code_on_a_line_item_of_a_payment_names_the_claim_that_item_settles`, fails the same on `0089678`.
- **Goldens**: `tests/golden.sh` changes four files, `04-freelancer-{available,balance,check,claims}.txt`, and the
  baseline binary prints the same: they were stale at `0089678` (K3c settles the invoices by their payments). None of
  them is touched here. `tests/mistakes` and the K0a harness (`diff/compare.sh`, 185 cases and the new one) change
  nothing; `fuzz.py ... 1000 diff`: 0 panics, 0 differences (every example is rejected by `check` for the v3 syntax it
  carries, so the fuzz compares what is said of it).
- **Oracle** (`addresses.py all`, books of both sorts, some with a tight pool of entities that stand in every slot):
  400 books at seed 77, 0 wrong. It read 7,533 references: 5,463 one account (3,037 by the names every account has, 2,426
  by the index, 101 of those because the line's day ruled a sibling out), 378 ambiguous (each re-read after the shortest
  address its diagnostic offers), 1,563 unknown (719 of them an account that is open on another day) and 129 parties; 20
  of the books write no account as an address. The same at seeds 5, 7 and 8 over 300 books each, 0 wrong. `placement`:
  words before a name against a brute-force placement of them, 400 of them, 0 wrong (73 placed, 50 ambiguous, 99 wrong
  kind, 178 too many).
- **Mutants**: see the end of this section.
- **Acceptance**: `family_addresses.py prove target/release/axiom` runs the eleven commands of the goldens over
  `05-family` and over the copy: the same 141 diagnostics (the v3 syntax every example still carries) in the same lines,
  and the same balances, claims, tallies, limits and tax, with each account called what the copy calls it. The copy
  says each account once, and drops the 13 relation lines under its accounts (`owner`, `employer`, `beneficiary`) that
  its words place, and the names that carried a relation (`jordan-401k`, `riley-529`, `joint-checking`); the two
  `owner` lines under assets stay, an asset not being an address.
