# K3a map: who asks for a tab, what the survey guessed, and what a tab may be beside a frozen tree

Written before the first code change of lane K3a, from the code at `b6bdf69`, and checked against what the code does (the
last section says how). Paths are in `crates/`; line numbers are those of `b6bdf69`.

A **tab** is the place that keeps what one party owes another: `Place { role: Role::Tab(party), owner, class, kind }`.
Its identity is `(party, owner, class)`; it has no source path and nothing can name it. Today the place tree is built once,
in pre-order, by `declare`, so every tab has to exist before lowering starts, and `lower::survey` predicts which will be
asked for.

## 0. What the brief says and what the code does

Four things in the brief do not match the code, and one thing in it is missing. Each decides something below.

1. **The survey predicts far more than lowering asks for.** `World::tab` has two callers: `lower_owes` (record.rs:1344)
   and `contract_loan` (contracts.rs:247). So of the five `Mention`s only two ever lead to a lookup: a `Claim`, and a loan's
   `Promise` (its `Debt` tab only). A `For`, a `Due`, an `Ends` and a promise's `Asset` tab are made and never asked for
   (section 2). Measured over 221 projects, 62% of the predicted tabs are never looked up.
2. **An unused tab is not invisible.** The `check` summary counts a place as used when `place.loc.is_some()`
   (report/lib.rs:343), and a predicted tab has the loc of its first mention. `why LINE` prints `declares place X` for any place
   whose loc overlaps the line (report/why/line.rs:245), so `why` of `checking -> ann 5 USD due 30d` says it declares the place
   `ann`. These are changes the lane makes on purpose, and lists (section 5).
3. **The survey is wrong where it is used.** It matches names by exact path (`account_owners.get("joint")` misses
   `assets/joint`, `entities.ids.get("ben")` misses `family/ben`) where lowering resolves them through scopes. Then the
   tab lowering asks for is not the tab predicted, and the build says `unregistered-tab`. A loan paid from `joint`, which is
   `assets/joint` owned by `pat`, does it today (the generated project `p0001` of `docs/v5/measure/tabs.py`).
   That is the diagnostic the brief calls "the model's own two phases disagree"; the lane removes it by removing the cause.
4. **`visit_endpoints` is not only for tabs.** The brief lists it among the walkers to delete. It does feed `find_tabs`, but
   its real use is `parties::mentions`: **an undeclared party exists because a journal names it** (LANGUAGE.md:453, "A party that
   is never declared can still be written"). `checking -> zorb 50 USD` works today, and `zorb` is an *implied entity*, made
   before the place tree and the entity tree freeze. The brief says `unknown-entity` for a name only a journal mentions
   "keeps firing, from lowering": it does not fire today for such a name, and making it fire would reject books that work.
   So the walk stays, reduced to what it is for (section 8). Making entities lazy as well is K3b: the entity tree has nested
   paths and a scoped name index whose ambiguity depends on the whole set.
5. **Nothing in `engine` or `report` creates or looks up a tab.** They read `Role::Tab` (section 1) and the order of ids.
   The brief's "readers of `Role` in `register.rs`" read `Role::Holding` and `Role::Outside`, not `Tab`.

## 1. Every place `Role::Tab` is made, asked for, or read

| | where | what |
|---|---|---|
| **made** | `declare/places.rs::tab_node` (298), called from `declare` (71-80) for each `TabDraft`; ids by `Tree::build` | the survey's tabs: `parents.push(None)`, so each is a **root**, after the issuers; `path` is the party's path, `kind` is the root `debt` or `asset` kind, `loc` is the first mention's |
| made (list) | `declare/holdings.rs::find_tabs` (157-215) from `JournalSurvey.mentions` | the `(party, owner, class)` keys, in survey order, `party != owner`, once each |
| made (map) | `declare/places.rs` (99-107) `Places.tabs`; `declare.rs:415-418` `World.tabs` | key to id |
| **asked** | `World::tab` (declare.rs:94): `tabs.get(key)`, else `unregistered-tab` | the only lookup |
| asked by | `lower/record.rs::lower_owes` (1344), key from the holder test below | a `X owes Y` statement, in the journal or in an `opening` |
| asked by | `lower/contracts.rs::contract_loan` (247), `(party, owner, Debt)` | a `loan` line of a contract |
| **a name for a loan's tab** | `declare.rs::contract_endpoints` (518-539) into `World.contract_endpoints`; read by `resolve.rs::special_end` (229) | a loan contract's name, used as a flow end, resolves to its debt tab even before the contract is lowered (a template may name a loan declared after it) |
| read | `said.rs::is_claim` (169) | every tab is a claim place |
| read | `lower/infer.rs::endpoint_purpose` (66) | the party a tab stands for gives the flow its purpose |
| read | `lower/statements.rs::claim_target` (496-497) | a write-off must name a transaction that has a flow into or out of a tab |
| read | `lower/record.rs::place_entity` (1500), `occurrence_item_ends` (1532) | the party of a tab, for an occurrence's items |
| read | `engine/eval.rs::matches` (994, 1013) | `place is KIND` and `place is ENTITY`: a tab is its party |
| tests | `model/tests/native_records.rs` (76, 113, 114) | |
| sync, report | none | `sync` reads `Role::Account` only; `report` reads `Holding` and `Outside` |

The holder test that decides a claim's key, in `lower_owes` (1329-1343) and, as `Entities.holds`, in `find_tabs`: an entity
*holds* when something is written as owned by it, or it is `me`; a held entity's own place is `Role::Holding`.

| | key `(party, owner, class)` |
|---|---|
| the creditor holds | `(debtor, creditor, Asset)` |
| else the debtor holds | `(creditor, debtor, Debt)` |
| neither | `(debtor, creditor, Asset)` |

## 2. What each `Mention` becomes, and whether lowering asks for it

`find_tabs` (holdings.rs:157). `owner_of(name)` is `account_owners.get(name)`: the owner of the account whose **path** is exactly
`name`. `me` is the book's owner. A key with `party == owner` is dropped.

| `Mention` (made by `survey`) | tabs made | asked for by lowering? |
|---|---|---|
| `Claim { subject, creditor }`, an `owes` statement (dated or in an opening) | the table above, 1 tab | **yes**, by `lower_owes`, with the same rule |
| `Promise { party, holding, loan_party }`, every contract (the party is `with` or the contract's own name) | `(party, owner_of(holding) or me, Asset)` and `(…, Debt)`: 2 tabs | only the `Debt` one, only when the contract has a `loan` line (`contract_loan`), with the owner of the place the holding resolves to |
| `For { other, ends }`, a `for WHOM` clause | `(other, owner_of(ends.from) or owner_of(ends.to) or me, Asset)` and `(…, Debt)`: 2 tabs | **no** |
| `Due { ends }`, a `due` clause | `(from, owner_of(to), Asset)` if `from` is an entity and `to` an account; `(to, owner_of(from), Debt)` the other way: up to 2 | **no** |
| `Ends { ends }`, the two ends of a contract schedule, a `now` change of terms, a leg, a clause with `for` or `due` | the same as `Due` | **no** |

Measured on 221 projects (60 generated by `tabs.py`, the 152 mistakes and 9 valid projects of K0a), by the first mention that
predicted each tab and whether lowering looked it up:

| first mention | predicted | looked up |
|---|---:|---:|
| `Claim` | 121 | 121 |
| `Promise` (also `Ends`, `For`, `Due`, `Claim` after it) | 196 | 25 (a loan's `Debt` tab, or a claim that came to the same key) |
| `For` | 55 | 1 (a claim came to the same key) |
| `Ends` (also `Due`) | 15 | 0 |
| asked and **not** predicted (`unregistered-tab`) | | 13 keys, in 11 of the 60 generated projects, none in K0a's |

So `Due`, `Ends`, `For` and a promise's `Asset` tab can go without a replacement: nothing reads them. What replaces the
`Claim` and loan `Promise` rows is the two lookups, which become find-or-create.

## 3. What is sized by the number of places while lowering runs

Searched for `places.len()`, `places.ids()`, a `Groups<Place, _>`, a `Vec` by place id, in `model`, `engine` and `report`.
Lowering runs from `lib.rs:95-105`: `declare`, `slots`, `props`, **`freeze_facts`**, …, `laws::register_native`,
`lower::contracts`, `lower::record`, **`freeze_facts`**.

| what | sized by | built | read while lowering? |
|---|---|---|---|
| `Book.holders` (`holders.rs`), the number of each thing for the facts | kinds, **places**, entities, commodities, assets laid end to end | `declare` | yes: every `Book::fact` of a place |
| `Book.facts` (`core::Facts`) | one row per holder; a read past the last **panics** | first `freeze_facts` (lib.rs:95) | yes: `is_claim`, `basis`, `holds`, `select`, of a place a flow ends at |
| `World.painter` (`core::facts::Builder`) | the same count, fixed at `Facts::builder(n)` (declare.rs:417) | `declare` | written by `end` statements after the first freeze |
| `Book.sites` | keyed by holder number | | no tab is in it |
| `Book.rules` (`Rules`: `on_in`, `on_out`, `on_gain`, `always`, `about`, five `Groups<Place, Rule>`), and `WrittenIn` inside it | `book.places.len()` (rules.rs:106, 164, 168) | `laws::register_native` → `rules::govern` (laws/mod.rs:180), **before** `record` | no: `model` never reads `book.rules`; the engine does |
| `Book.touching` (`Groups<Place, Id<Flow>>`) | `book.places.len()` (record.rs:187) | the end of `record`, after the last flow | read by reports only |
| `lookup.places`, `issuer_places` | names, commodities | | not by id |
| engine `Plan`, `Holdings`, `Sides`, `Owners`, `Traits`, `Totals`, report `history` offsets | `book.places.len()` | after `build` returns | no |

Two of these break if a place appears during lowering:

- **The numbering of holders.** `HolderIndex` lays the arenas end to end with the places *between* the kinds and the entities,
  so one more place moves every entity, commodity and asset number, and a place past the frozen store panics when read.
- **The rules.** A tab made after `govern` has no rules: it is not watched by its owner's laws, its kind's laws, the project's
  laws. A predicted tab was.

What moves after lowering, and why it is safe: only `rules::govern`. `Rules::of` reads `book.laws`, `kinds`, `contracts`,
`assets`, `places`, `also`; `lower::record` pushes none of these but flows, transactions, codes, selectors, details, programs
(checked: `laws.push` is called only by `laws/mod.rs`, `laws/budget.rs` and `lower/also.rs`, all before `record`), and `model`
never reads `book.rules`. `govern` gives no diagnostics; `rank`, which does, stays where it is, so the diagnostics come in the
same order. `touching` is already after.

## 4. Does anything ask a tab for its subtree, its path, or its parent?

Yes, all three, but never as a question that a tab answers differently from any other root leaf.

| question | asked by | of a tab |
|---|---|---|
| `Tree::lineage`, `parent` | `rules.rs:185` (every place), `engine/scope.rs:30` (every flow end's laws) | `[itself]`, no parent |
| `Tree::covers(root, p)` | `engine/eval.rs:1009, 1551`, `plan.rs:238`, `explain.rs:207` | true only for itself |
| `Tree::end`, `subtree` | `engine/eval.rs:1573`, `report/history.rs:418` (a place's offset in the history), `balance.rs` | `id..id+1` |
| `depth`, `roots`, `parent` | `report/balance.rs` (rows, net worth by class), `report/places.rs:29` | depth 0, a root |
| the path | `report/places.rs::path`, `flow` routes, every row label | the party's path; **no name finds a tab** (`lookup.places` has accounts and assets only: declare/places.rs:83-87) |

A tab is a root of its own with an empty subtree, so the tree answers every question by itself, and three engine modules and
the report ask it of tabs without knowing it is one. So a tab stays in the tree (candidate 3, "not in the tree", would put a
branch in each of those).

## 5. The order tabs are made in, and where it is visible

Today. `find_tabs` walks `survey.mentions` in order: per source (`Collected` order: systems first, then the project's files in
path order), per item in the file, a contract's mentions (the promise, then its schedule, body, deadline, `also` lines),
a statement's, a transaction's. A key keeps its first mention's place, and its `loc`. `Tree::build` keeps roots in input order,
so the tabs are the **last ids** of the tree, after the issuers, in that order.

With the lazy design. A tab is made the first time something asks. `lower::contracts` runs first (loans, in contract order,
and loans first because they are asked first), then `record` in `(day, source order)`. So the order is that of the journal in
time, where today's is that of the text.

Where an id order among tabs is visible, found by reading each consumer and by `balance` of a book whose text order and day
order disagree:

```text
2025-12-31 market -> checking 9_000 USD
2026-02-01 checking -> ann 5 USD due 30d       // predicts the Debt tab (ann, me), at this line; no claim asks for it here
2026-01-10 bob owes me 20 USD                  // the Asset tab (bob, me)
2026-03-01 me owes ann 7 USD                   // asks for the Debt tab (ann, me)
```

`balance` lists `ann` before `bob` today (the order of the text); lazily it lists `bob` first (the order of the days). `why`
of line 2 says `declares place ann` today, and of line 4 does not; lazily the other way round. The consumers:

| output | how | changes with the order? |
|---|---|---|
| `balance`, `balance --value`, `--monthly`, `--json` | rows are `book.places.ids()` (balance.rs:35) | **yes**, among tabs held |
| `lots` | rows are `run.holdings`, in place order | **yes**, among claim parcels |
| `claims` | stable sort by `(mine, due, made)` over holdings in place order | only for equal due and made days |
| the `check` summary | counts places with a loc, a flow or a holding | the **number** changes: an unused predicted tab is a counted place today |
| `why LINE` | `declares place X` for a place whose loc overlaps the line | **yes**: a predicted tab's loc is its first mention; a lazy tab's is the first statement that asked |
| `register`, `flow`, `available`, `gains`, `tax`, `contracts`, `forecast` | by flow, by day, by contract, summed | no |

The brief's rule: where lazy creation changes what is visible, make the order explicit. Which of these rows the generated
books change is measured in the lane's report; this map does not decide it.

## 6. How a tab lives beside the frozen pre-order tree

**Candidate 1, a trailing root, with one correction to the brief:** there is no tab root. Today each tab is its own root, and
`Tree<Place>` already ends with them. So the operation `Tree` needs is not "append a child to the last root" but **push a
root**: a new last root has `parent = NONE`, `depth = 0`, `end = id + 1`, and no other id, `end` or `depth` moves. The tree
`Tree::build` would make from the old items with the new one last is exactly the tree it makes.

**Candidate 2 (a second arena with its own id)** splits every `Id<Place>` consumer: the flows' `from`/`to`, `Holdings`, `Sides`,
`rules`, the report's rows. Section 4 shows three modules ask the tree about tabs by `Id<Place>`.

**Candidate 3 (not in the tree)** is refuted by section 4.

What must also grow when a tab is pushed (section 3): the **holder numbering** and the **facts store**.

- `HolderIndex` puts the places **last**: kinds, entities, commodities, assets, places. The arena that grows during lowering
  is the last, so growing it renumbers nothing. `HolderIndex::new` keeps its arguments; the layout is inside. The two tests
  that state the layout (`an_id_converts_to_the_holder_of_its_sort`) change with it, and say so.
- `Facts::grow(holders)` and `Builder::grow(holders)` add rows that have said nothing: a thing made after the store was frozen
  has said nothing, and a read of it finds its kind's defaults (`Book::fact` already falls back to the kind). The second
  `freeze_facts` then covers every tab.
- `Facts` keeps its contract that a read past the last holder panics: the store grows with the book, rather than reads going
  soft.
- `rules::govern` moves after `record`.
- The tab's `kind` is read from `book.roots.kinds`, not from the declaration's `Resolving`, which does not outlive `declare`.

Does anything else depend on the number of places at the first freeze? No (section 3).

## 7. The loan's name, which a template may use before the loan is lowered

`contract_endpoints` exists for a forward reference: `contract payments` has a leg `mortgage 20 USD` and `contract mortgage`,
declared after it, has the `loan`. `special_end` finds `contract_endpoints[mortgage]` and returns the debt tab; without it the
name reaches `seek_entity` and fails with `contract-endpoint`. This works in both orders today (checked on `fwd.ax` and
`rev.ax`, scratch books), and a contract that names its own loan relies on it as well, since `book.contracts[id]` is an empty
placeholder until `lower_contract` returns.

The survey gave it by predicting. Lazily it is a lookup that the first loop of `lower::contracts`, which already reserves every
contract id, can do: for a contract with a `loan` line, resolve its party and the owner of its holding (the same two
resolutions `lower_contract` makes) and ask `World::tab` for `(party, owner, Debt)`. No guess: the same call `contract_loan`
makes later, which then finds the tab.

## 8. What the endpoint walk is for, and what stays

`parties::mentions` (parties.rs:169) takes every name that `visit_endpoints` visits, with the loc of the first, and a set of
*roles*: the names written as a claim's debtor or creditor, a `for WHOM`, or a promise's party. `implied_parties` makes an
entity of each name that no declaration of a place, asset, kind, purpose, commodity or system answers to, and of a contract's
name only if it is also written as a party (`roles`). The context argument of the visitor, a 14-variant `EndpointContext`, is
read by exactly one thing outside the tests: the four contexts that make a role (`ClaimSubject`, `ClaimCreditor`, `ForParty`,
`ContractParty`). The rest are never matched. The survey's `Claim`, `For` and `Promise` mentions are read for the same
roles.

What the lane keeps: one walk that says, for each name, where it is first written and whether it is written as a party. A
two-valued `Role` replaces `EndpointContext`; a contract with no `with` has its own name as its party, which `collected.contracts`
says without walking. What it deletes: the survey, `Mention`, `Ends`, `JournalSurvey`, `find_tabs`, `TabDraft`, the tab half of
`places::declare`, and the survey's half of the roles. One consequence: a `for` in a contract that has no schedule (which the
survey never scanned and the visitor did) now makes a role, as it should; this matters only if the name is also a
contract's name.

## 9. How this map was checked

- A scratch copy of `b6bdf69` with an `eprintln!` in `World::tab` and in `find_tabs` (not committed) printed, for every run of
  `check`, which mention first predicted each tab and which keys lowering asked for: the tables in section 2.
- `docs/v5/measure/tabs.py gen DIR 60 1` writes projects of claims, loans and parties in orders where the text and the days
  disagree; the baseline binary raises `unregistered-tab` on 11 of the 60.
- The book in section 5, run through `balance`, `why` and `check`, gave the visible effects there.
- `docs/v5/measure/diff/cases2/tab-forward-loan.ax` and `tab-loan-first.ax`: a contract's leg ends at a loan's name, declared
  after it and before it; both print `checking → bank-co` on the baseline, and must keep printing it (section 7).
- `docs/v5/measure/diff/cases2/tab-implied-parties.ax`: parties that no declaration names (`zorb`, `frob`, `kidz`, `paypl`,
  `dana`) exist, and a claim against one of them works (section 0.4).
