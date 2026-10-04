# UNIFY: the plan of lane U

Phase 0 of the unification lane: what the tree is, what it writes more than once, what each of those becomes, and what that
adds up to. Written from the code at `25566a5` (K5e merged), with the K6b and L1 worktrees read but not touched. Every count
below was made on the code; where a number is an estimate it says so, and how it was made. Paths are in `crates/`.

## 0. The answer

**The ceiling is not reachable by unification with the features and the output as they are.** The honest ledger below (49
entries in six checkpoints) deletes about **9,900 net lines**. Applied to the tree U will start from (K6b, L1 and lane D merged:
about **54,350** by `quality.py`, **56,300** counted truly, see 1.1), and with K3f's own +70, it lands at about **46,450 lines**
(about 44,500 by today's `quality.py`, which hides 1,948 lines of code). The levers the brief and PROPOSAL §7 name, and five more
I found, bring it to about **38,500**, and every one of those but three removes something the user has. 27,000 is not a number
this feature set reaches; section 4 says what it would take, feature by feature, so the choice is yours.

Why the proposal's 27,000 does not hold any more, in its own lanes' numbers (STATUS "in numbers"):

| lane | planned | delivered | why it differed, in the lane's words |
|---|---:|---:|---|
| K0a groundwork | −2,000 | −430 | the splits that followed added signatures and context structs |
| K3a no survey | −600 | −332 | the implied-party walk stays; the replacement code is real |
| K4a one vocabulary | −1,200 | −481 | the two quantity transliterations were smaller than assumed |
| K12 kinds, slots, facts | −2,000 | +517 | the sampling machinery stays; `props.rs` still 1,182 lines |
| K4b one solve | −1,500 | +858 | the three "copies" were not one algorithm; the statement path did not exist |
| K5b fold on promises | −900 | −209 | the monitor is +177 |
| K5c forecast is the fold | −900 | +98 | the second driver was about 330 lines, not 1,200 |
| K6 norms and relators | −1,200 | +773 | the sugars were not copies of anything |
| features since the proposal | 0 | +4,400 | K5a +966, K3b +859, K5d +725, K3d +414, K7a +363, K3c +312, K5e +207, K6b ≈ +550 |

The proposal's per-crate column (model 6,950, engine 6,550, report 3,400) was a clean-room estimate of a smaller feature set;
the lanes that were meant to deliver it ran, built the kernels it named, and the tree grew. What is left to unify is real and
is below, but it is a sixth of the tree, not a half: the functions are short already (mean 12.5 lines), the duplication that
remains is in *shapes* (how a line is lowered, how a property is read, where an asset part's basis lives) and the bulk of every
crate is breadth of features and the quality of their diagnostics.

The three biggest unifications, by lines:

1. **U7, one lowering of a line that moves value** (−1,130): a journal transaction, an opening, a written occurrence, a loan's
   origination, an `owes`, a contract's template, an `also`/`derive` line and a basis statement are each a group (a header, its
   legs, its items, a tail) lowered by its own copy of the same skeleton (`lower/record.rs`, `lower/contracts.rs`,
   `laws/compile/line.rs`, `lower/statements.rs`, `lower/loan_opening.rs`). One `Lowering::group`.
2. **U21, an asset part's basis is its parcels'** (−910): `engine/assets.rs` keeps a second copy of each part's basis beside the
   parcels that carry it, and six prepare/apply guards (`ConsumptionGuard`, `CarryGuard`, `AssetBasisBatchGuard`,
   `PartBasisAdjustment`, `CarryLotBatchAdjustment`, `AssetPartAddition`) plus eight `ParcelBasisMismatch` checks keep the two in
   step. The parcels are the one store; a part's basis is read from them.
3. **U4, one reader of a property line** (−870, byte-identical): `contract … loan AMOUNT on DATE at RATE over SPAN [for ASSET]`,
   `resets`, `prepay`, `grace`, `covers`, `rising`, `indexed`, `area`, `deposit`, `share`, `from`/`until`, the built-in
   properties of `props.rs`, a contract's `input`s, a loan's `now at`, and the lines of `format` and `sync` declarations are each
   parsed by hand, argument by argument, with a diagnostic per mismatch. One `Signature` table read by one reader.

Close behind: **U1, the lowering site** (−840): 150 model functions take `world`, `home`, `file` and `diags` as four parameters
(930 signature lines); they are the steps of lowering one source site, and become methods of it.

What I take from the queued lanes (section 5): **K12b** whole; **K3e**'s relief-as-ranking, identity key and asset parts (the
column layout only if its benchmark pays); **K4c**'s K4b list and its `FlowRef` (the column layout after U, held to its
benchmark); **K7c**'s six (K7c-1 to K7c-6, decided); **K3f** runs as its own lane beside C1-C3 and merges before C4 (a behaviour
change with its own oracle); **L2/L3** wait for your sign-off and are written against the one lowering of C2.

The questions I need you to decide are in section 9. The first is the count itself: `quality.py` stops reading a file at its
first inline test module, which hides 1,948 lines of code that U must touch (section 1.1).

---

## 1. The census

### 1.1 What is counted, and what is hidden

`docs/v5/measure/quality.py` drops a `#[cfg(test)] mod x {` block by **truncating the file at it**. Six files have an inline
test module in the middle and code after it; that code is not counted:

| file | counted | real | hidden |
|---|---:|---:|---:|
| `model/src/law.rs` | 44 | 542 | 498 |
| `model/src/book.rs` | 622 | 996 | 374 |
| `model/src/journal.rs` | 160 | 529 | 369 |
| `core/src/calendar.rs` | 165 | 493 | 328 |
| `model/src/declare.rs` | 498 | 703 | 205 |
| `model/src/problem.rs` | 347 | 444 | 97 |
| `sync/src/world.rs` | 502 | 576 | 74 |
| `sync/src/peg.rs` | 290 | 293 | 3 |
| **total** | **53,419** | **55,367** | **1,948** |

(Counted by skipping each test module to its matching brace instead of truncating the file at it, the change question 1
proposes for `quality.py`. `pub struct Amount` in `book.rs`, `Value` in `law.rs`, `Flow` in `journal.rs` and `Window` in
`calendar.rs` are among the hidden code.) The ledger rewrites five of these files, so the hidden lines surface the day U
touches them, and an honest plan counts them from the start. **I plan against the true count.**
Decided (section 10): `quality.py` now skips a test module to its matching brace, and the baseline is **55,367** at `a21eee9`.
Moving a test module to dodge or to cause the count is not something U does either way.

### 1.2 By crate

| crate | `quality.py` | true | K6b, L1, D (expected) | entering U (true) |
|---|---:|---:|---:|---:|
| `model` | 18,234 | 19,777 | +140 (K6b) | 19,917 |
| `engine` | 12,463 | 12,463 | +270 (K6b) | 12,733 |
| `report` | 6,804 | 6,804 | +140 (K6b) | 6,944 |
| `syntax` | 5,634 | 5,634 | +80 (L1: junction, `legacy.rs` ≈110, one header production) | 5,714 |
| `sync` | 4,262 | 4,339 | 0 | 4,339 |
| `core` | 3,156 | 3,484 | 0 | 3,484 |
| `cli` | 2,350 | 2,350 | +300 (L1: `upgrade.rs` against the book) | 2,650 |
| `session` | 502 | 502 | 0 | 502 |
| `systems` | 14 | 14 | 0 | 14 |
| **total** | **53,419** | **55,367** | **+930** | **≈56,300** |

K6b measured at its worktree's head against its base `990ddb5`: engine +234, model +121, report +126 (+481, with its
`offspring.rs` still uncommitted); the column above rounds that up for what is left of it. L1's figures are its map's own
(`legacy.rs` about 110, the upgrade in `cli`); lane D's brief asks for no growth.

### 1.3 By file, ranked

What each file is for is its own module comment's first sentence: the tree documents every one of its 229 files.
Counts are `quality.py`'s, with the hidden lines of 1.1 beside the file.

#### `model`: 18,234 lines in 64 files

| lines | file | what it is for (its own `//!`, first sentence) |
|---:|---|---|
| 1,567 | `lower/contracts.rs` | S5 contract declarations are lowered in two passes. |
| 1,475 | `lower/record.rs` | Native S5 journal records. |
| 1,131 | `laws/compile.rs` | Compiling one law. |
| 985 | `props.rs` | Properties: the lines written under kinds, accounts, entities and commodities. |
| 880 | `sync_lower.rs` | Lower S5 sync declarations into the one typed model schema used by the runtime. |
| 807 | `lower/statements.rs` | Statements that say something on a day without being a flow: a value, a measure, a return filed, a split, an … |
| 622 (+374 hidden) | `book.rs` | The book: everything the sources declare and record, resolved and typed. |
| 549 | `lower/flow.rs` | Making a flow from what was written: its ends, its quantities, its tail and the items under it. |
| 498 | `slots.rs` | Slots: what the things of a kind have, how many of each, and what each takes. |
| 498 (+205 hidden) | `declare.rs` | Native S5 declarations: create the immutable Book trees and name indexes. |
| 380 | `resolve.rs` | Turning written names into ids, with a diagnostic when it cannot be done. |
| 368 | `laws/mod.rs` | Laws: written in systems, kinds, accounts, entities and the project, compiled to typed nodes. |
| 350 | `laws/budget.rs` | Native purpose-budget declaration and dated terms lowering. |
| 347 (+97 hidden) | `problem.rs` | What the model says is wrong, for the problems that come in families. |
| 295 | `laws/compile/line.rs` | What a line that derives says: the `FLOW` or `ITEM` after an `also` or a `derive`, read. |
| 291 | `solve.rs` | What a split gives each of its legs, solved once. |
| 290 | `params.rs` | Typed parameter tables: dated and named values such as `limit[year]`. |
| 277 | `promise.rs` | A promise compiled to a term, and the schedules its terms fall due on. |
| 275 | `lower/contracts/relator.rs` | What a contract of a kind is: the slots it fills, and the legs its kind writes once. |
| 274 | `values.rs` | Constants: the literal values written in properties, params and laws. |
| 271 | `rules.rs` | Governance: which laws watch which place, worked out once. |
| 261 | `declare/places.rs` | The place tree: somewhere to put a flow for every account, asset, party and issuer, frozen once. |
| 252 | `lower/tail.rs` | The tail of a line: the clauses after its amount, read into what the line says about itself. |
| 250 | `spelled.rs` | Accounts written with the things that fill their slots before the name: `jordan/bluefin/401k`. |
| 236 | `declare/parties.rs` | Parties: the entities a book has, those written and those a journal implies, and who owns what. |
| 225 | `fill.rs` | Filling slots: what a property line gives the slot it names, checked once. |
| 224 | `addresses.rs` | Addresses: how the words of a reference find the account they mean. |
| 211 | `taxonomy.rs` | Trees of names written `NAME : PARENT`: the kinds and the purposes. |
| 206 | `promise/annuity.rs` | A loan as a state, and the four things that happen to it. |
| 200 | `promise/amortization.rs` | A loan's schedule: what is owed after every payment and prepayment the book states, worked out once. |
| 191 | `balance.rs` | What a statement's split, or header with items, comes to, and whether it can. |
| 182 | `reference.rs` | Reading a written reference as an address, and saying so when it is none or several. |
| 179 | `promise/schedule.rs` | The days one stream of occurrences falls due, and what a day asks of them. |
| 173 | `lower.rs` | Native S5 journal lowering: the journal and the contracts become flows. |
| 160 (+369 hidden) | `journal.rs` | What the journal records: flows grouped into transactions, balance assertions, measures, settlement events, … |
| 159 | `declare/mentions.rs` | The names the sources write where a party can stand, and where each is first written. |
| 154 | `lower/loan_opening.rs` | A loan made before the book began opens its debt with what its terms say is owed when the book begins. |
| 152 | `split.rs` | One vocabulary for a split: what a header, its legs and the items under them say. |
| 151 | `laws/compile/derive.rs` | `derive`: the step of a law that makes a flow, and the lines of a contract that are such laws. |
| 146 | `names.rs` | Name tables: a thing is known by every `/`-boundary suffix of its path. |
| 138 | `kinds.rs` | Kinds: what things are. |
| 134 | `declare/holdings.rs` | What the owners hold: accounts and assets. |
| 129 | `sync.rs` | What a book says about sync (LANGUAGE §14): where facts from outside come from, how their records read, and the … |
| 126 | `lower/infer.rs` | What a flow is for when its line does not say, and what wins when its sources disagree. |
| 125 | `said.rs` | What a book says of a thing: the values of slots, read back. |
| 121 | `laws/order.rs` | The order laws run in: a law that reads `tally(x)` runs after every law that counts into `x`, whichever file … |
| 116 | `laws/types.rs` | The typing rules of law expressions: which operands an operator accepts, and what it says when they do not fit. |
| 108 | `promise/reckon.rs` | What a due day is worth and what it is for: the factor an occurrence on that day takes, and the days it is … |
| 104 | `laws/vars.rs` | What a law's trigger tells it: the variables it may read, and their types. |
| 100 | `purposes.rs` | Purposes: what flows are for. |
| 99 | `sources.rs` | The sources, arranged: which files are systems, which are the project, and the tree of systems they define. |
| 89 | `collect.rs` | One look at every item of every source, sorted into typed buckets. |
| 86 | `lib.rs` | Syntax trees to a `Book`: names resolved, kinds linked, laws compiled and type-checked, transactions elaborated … |
| 75 | `promise/causes.rs` | Why a statement of what a loan owes disagrees with its schedule. |
| 72 | `declare/commodities.rs` | Commodities, and the base currency every amount is counted against. |
| 67 | `holders.rs` | The things a book says things about, numbered. |
| 66 | `lower/staged.rs` | Writes to the journal's arenas that are undone unless they are kept. |
| 62 | `prices.rs` | Prices: quotes by commodity pair and day, and converting through them. |
| 59 | `scope.rs` | Who can see what. |
| 54 | `errors.rs` | The words diagnostics share: a word as written, the ways to say it, and the near miss that fixes it. |
| 53 | `builtin.rs` | The language's own slots: what its property lines say, as facts. |
| 46 | `promise/residual.rs` | What is still owed of one stream of a promise. |
| 44 (+498 hidden) | `law.rs` | Compiled laws. |
| 19 | `paths.rs` | Trees of `/`-separated paths. |

#### `engine`: 12,463 lines in 38 files

| lines | file | what it is for (its own `//!`, first sentence) |
|---:|---|---|
| 1,413 | `eval.rs` | Law evaluation. |
| 1,094 | `lots.rs` | Where value rests, and how it leaves: one `Slot` per `(place, commodity)`. |
| 956 | `post.rs` | Moving one flow's value: relief at the source, realization, arrival at the target, with the laws that watch … |
| 826 | `explain.rs` | What the fold says when something is wrong: a law that fails, an assertion that does not hold, lots that cannot … |
| 782 | `occurrence.rs` | A contract's occurrence made into flows. |
| 667 | `fire.rs` | Firing laws: which rules run for an occasion, and what becomes of what they find. |
| 662 | `totals.rs` | Running sums: window totals per subject, and tallies. |
| 551 | `ledger.rs` | The state machine. |
| 519 | `assets.rs` | Canonical, part-aware state for assets. |
| 409 | `lib.rs` | The timeline: a book folded through time. |
| 371 | `plan.rs` | The plan: everything the fold decides before it begins, once. |
| 326 | `calc.rs` | Arithmetic and comparison on law values. |
| 313 | `timeline.rs` | The order of the fold. |
| 309 | `statement.rs` | A statement's split, solved when its first flow lands. |
| 301 | `infer.rs` | The solve pass: amounts written `? USD`. |
| 220 | `facts.rs` | What a law says that never changes, read off its syntax tree once. |
| 217 | `promising.rs` | An occurrence falls due: a line of the journal keeps it, or, past the day a ledger stands on, the promise is … |
| 191 | `loan_balance.rs` | A statement of what a loan owes, held to the loan's schedule. |
| 185 | `state.rs` | What a Ledger carries besides the plan and its timeline. |
| 185 | `monitor.rs` | What the fold has yet to settle of every promise, and what a run says of them. |
| 182 | `histories.rs` | What every position held, on every day: balances as steps, recorded by the fold as it goes. |
| 171 | `motion.rs` | A flow as the fold sees it: oriented, with its quantities solved, on the day it takes effect. |
| 168 | `occurrence/derive.rs` | What the laws of a promise derive from one of its occurrences. |
| 164 | `owners.rs` | Effective ownership: who owns each entity and place in the end, and in what shares. |
| 159 | `assets_runtime.rs` | Ledger hooks that keep canonical asset basis and parcel basis in lockstep. |
| 144 | `recognition.rs` | When a claim counts as income or spending: LANGUAGE §7, "A claim's purpose is its recognition". |
| 140 | `settle.rs` | A payment from a party settles the claims on it: LANGUAGE §7. |
| 128 | `reconcile.rs` | Balance assertions, checked at the end of their day. |
| 107 | `events.rs` | Settlement events: `2026-02-06 #check-1041 settled`. |
| 100 | `temporal.rs` | Sparse histories for the law functions that inspect a value over time. |
| 92 | `claims.rs` | Making a claim of what a party owed and nothing kept, and forgiving a claim: `^code waived`. |
| 86 | `checkpoint.rs` | Checkpoints: a fold's state, small enough to keep at a report boundary or month end. |
| 83 | `show.rs` | Words for values: every place the engine prints one goes through here. |
| 70 | `evaluate.rs` | Reading an expression of a record against one of its flows. |
| 70 | `traits.rs` | What the places, entities and commodities say that the fold asks, resolved once. |
| 60 | `budget.rs` | Calendar segments for dated purpose budgets. |
| 28 | `scope.rs` | Whose value a flow moves. |
| 14 | `sides.rs` | The sign in which each place's balance is shown. |

#### `report`: 6,804 lines in 43 files

| lines | file | what it is for (its own `//!`, first sentence) |
|---:|---|---|
| 548 | `register.rs` | `register`: one place's flows, dated, with a running balance. |
| 532 | `flow.rs` | `flow`: income, spending, capital and transfer, grouped by purpose or party. |
| 450 | `json.rs` | Machine-readable renderers for reports and diagnostics. |
| 370 | `forecast.rs` | `forecast`: where the books are heading. |
| 314 | `available.rs` | `available`: how much can actually be spent, what is coming in, and what it would cost to reach the rest. |
| 310 | `balance.rs` | `balance`: what every place holds, as a tree, on one day or at each month end. |
| 287 | `why.rs` | `why`: the provenance of a figure. |
| 261 | `why/line.rs` | `why FILE:LINE`: what is written on one source line, and everything it caused. |
| 223 | `why/purpose.rs` | `why #PURPOSE`: the rules, budget and flows classified by a purpose. |
| 210 | `contracts.rs` | `contracts`: promises, their current terms and the next time due. |
| 210 | `budget.rs` | `budget`: what each purpose has spent against its dated allowance. |
| 198 | `table.rs` | Small constructors so views read as what they show, not how it is built. |
| 194 | `lib.rs` | Views over a run. |
| 190 | `lens.rs` | One answer, for every view, to "whose is this, what is it worth, and how liquid is it". |
| 184 | `tax.rs` | `tax`: what the laws tallied and what they say is owed, for one year. |
| 183 | `why/contract.rs` | `why CONTRACT`: each change in terms and the occurrences it promised. |
| 170 | `claims.rs` | `claims`: what others owe, and what is owed to them, still open on a day. |
| 138 | `forecast/trace.rs` | Running the forecast: the fold continued past today, and the position read at each month end. |
| 135 | `context.rs` | A reusable, coherent view of one engine run. |
| 124 | `why/asset.rs` | `why ASSET`: its parts, basis changes and flows that concern it. |
| 120 | `why/place.rs` | `why PLACE`: what it holds and how, how close it is to every limit, which laws govern it, what touched it lately. |
| 116 | `lots.rs` | `lots`: parcels with their basis, and what they would fetch at the latest price. |
| 105 | `forecast/expected.rs` | What is expected to happen again: plans, and the rhythms history shows. |
| 102 | `gains.rs` | `gains`: every disposal in a year, in the shape of Form 8949: what was sold, when it was acquired and sold, … |
| 102 | `pivot.rs` | What the postings count, as a table with a row for whatever a view groups by and a cell for each period. |
| 100 | `forecast/bands.rs` | Monte Carlo bands: how far variable spending could pull the forecast down. |
| 86 | `forecast/recurrence.rs` | Regular flows: finding a rhythm in history, and the calendar rule that projects a plan or a rhythm forward. |
| 86 | `why/law.rs` | `why LAW`: where it applies, what it says, how often it ran, what it caused. |
| 79 | `limits.rs` | `limits`: every cap and budget a person lives under, before anything breaks. |
| 79 | `why/entity.rs` | `why ENTITY`: whose money it is, what governs it, what is held for it, what it owes and is owed. |
| 78 | `headroom.rs` | Headroom: what every limit had counted, and what it allowed. |
| 72 | `history.rs` | What happened, as posted: the journal's flows with their solved quantities and settlement, in the order the … |
| 70 | `forecast/variable.rs` | How much other spending varies: the spending contracts and habits do not explain. |
| 60 | `resolve.rs` | Turning what someone typed into ids, or into a diagnostic that helps. |
| 54 | `why/system.rs` | `why SYSTEM`: its laws, and what each counted or owed for the people living under it. |
| 54 | `why/code.rs` | `why ^CODE`: the flows a code marks and the events that changed their state: a check cleared, a claim waived. |
| 42 | `calendar.rs` | Cutting time into consecutive calendar months or years. |
| 38 | `synth.rs` | Flows the journal never wrote: planned ones, and hypothetical ones. |
| 30 | `balances.rs` | What every place held at the end of each of several days, for one owner scope: a read of the run's histories. |
| 27 | `why/text.rs` | `why "description"`: the written flows with that exact description. |
| 27 | `why/taxline.rs` | `why NAME` for a tally or obligation: every effect by that name, and the flows and gains behind them. |
| 25 | `closings.rs` | The days `each year closing MM-DD` laws judge a year. |
| 21 | `places.rs` | Questions views ask about places. |

#### `syntax`: 5,634 lines in 19 files

| lines | file | what it is for (its own `//!`, first sentence) |
|---:|---|---|
| 843 | `ast.rs` | The syntax tree. |
| 543 | `lex.rs` | Tokens: the words, numbers, dates and punctuation of one line. |
| 450 | `style.rs` | `axiom fmt`: the house style of a journal (LANGUAGE §2, §3). |
| 388 | `flow.rs` | Flows: the `SOURCE -> TARGET` header, its ends and tail, the indented legs of a one-side split (which an … |
| 319 | `source.rs` | What feeds a book (LANGUAGE §14): `sync` sources, the `format`s of their records, and the `pattern`s that … |
| 310 | `structure.rs` | A file's structure: items in column 0, dispatched on their first word, and the indented blocks under them. |
| 296 | `contract.rs` | Contracts (LANGUAGE §7): `contract NAME [: KIND] [with PARTY]`, its schedule and the template an occurrence … |
| 294 | `decl.rs` | Declarations and the other block items: `account`, `entity`, `asset`, `purpose`, `commodity`, `kind`, `budget`, … |
| 293 | `statement.rs` | Statements (LANGUAGE §5): `DATE SUBJECT VERB …`. |
| 286 | `parser.rs` | The parser's state, and the small operations every grammar rule shares. |
| 284 | `amount.rs` | Amounts: `84.20 USD`, `empty`, and the mistakes people make writing them. |
| 254 | `expr.rs` | Expressions: precedence climbing into the file's post-order arena. |
| 204 | `malformed.rs` | Diagnostics for tokens that are not tokens: impossible dates, glued amounts, currency symbols, unterminated … |
| 203 | `law.rs` | Laws: a trigger, then steps that run top to bottom. |
| 175 | `lib.rs` | Source text to a borrowed syntax tree. |
| 149 | `dates.rs` | Dates as written: in full, or short of what their context gives. |
| 140 | `lines.rs` | Splitting source into lines. |
| 102 | `journal.rs` | Dated lines: a flow, or a statement about its first end, and `opening` blocks. |
| 101 | `refs.rs` | Typed indices into a file's tables. |

#### `sync`: 4,262 lines in 20 files

| lines | file | what it is for (its own `//!`, first sentence) |
|---:|---|---|
| 632 | `format.rs` | Runtime readers for the model's canonical format declarations. |
| 502 (+74 hidden) | `world.rs` | A statement's way into the journal (LANGUAGE §14): which records are already written, who the rest are, which … |
| 402 | `planner.rs` | The model-native, no-write planning boundary used by `axiom sync`. |
| 391 | `recognize.rs` | Runtime recognition over the book's borrowed entities, accounts and flat model pattern programs. |
| 391 | `tagged.rs` | Tagged statements: OFX version 1 (SGML, where a value runs to the next `<` and only aggregates close), OFX … |
| 381 | `write.rs` | Where a line goes, and how it is written (LANGUAGE §10): into the file its day belongs to, in day order, dated … |
| 290 (+3 hidden) | `peg.rs` | Runtime for the model's compiled pattern programs (LANGUAGE §14). |
| 205 | `sink.rs` | Sources that print Axiom: invoices, prices, the rows of a param. |
| 169 | `reconcile.rs` | Which records are already written. |
| 141 | `binding.rs` | The existing journal state the statement reconciler borrows. |
| 124 | `amount.rs` | Amounts as banks write them, in a CSV cell or an OFX tag. |
| 104 | `lib.rs` | Sync: how a book stays current without being typed (LANGUAGE §14). |
| 95 | `paths.rs` | Project-confined paths for local sync inputs and outputs. |
| 91 | `csv.rs` | CSV as banks write it: quoted fields with doubled quotes, CRLF, a byte-order mark. |
| 87 | `command.rs` | Running a source's command: from the project root, with `{since}`, `{today}`, `{year}` and `{units}` filled in, … |
| 86 | `apply.rs` | Writing a plan: the one place sync touches the project's files. |
| 67 | `diff.rs` | What would be written, as a unified diff. |
| 47 | `cell.rs` | Borrowed cells shared by the row and tagged readers. |
| 43 | `unknown.rs` | The memos nothing recognized, grouped, each group with the `known-as` line that would recognize it: what … |
| 14 | `date.rs` | ISO dates when a format has no explicit compiled `core::DateLayout`. |

#### `core`: 3,156 lines in 22 files

| lines | file | what it is for (its own `//!`, first sentence) |
|---:|---|---|
| 475 | `facts.rs` | Everything said about a thing, as steps on days: one store, rows of timelines. |
| 357 | `num.rs` | Exact numbers. |
| 289 | `dues.rs` | The days a schedule falls due, as an ordered set that can be counted and indexed. |
| 232 | `tagless.rs` | A column of mixed values: a byte of tag beside sixteen bytes of payload. |
| 213 | `id.rs` | Typed indices into typed arenas. |
| 171 | `day.rs` | Calendar days. |
| 165 (+328 hidden) | `calendar.rs` | The calendar vocabulary: ranges of days, months and years, an amount spread over days, and the days a schedule … |
| 157 | `tree.rs` | Hierarchies in pre-order. |
| 154 | `diag.rs` | Diagnostics: what went wrong, where, and what to do about it. |
| 138 | `trail.rs` | Dense state with an undo log: going back in time as cheap as going forward. |
| 138 | `dayset.rs` | Finite unions of day intervals: the days a condition holds. |
| 132 | `postings.rs` | Intersecting sorted lists of ids: how the words of an address find the things they name. |
| 104 | `placement.rs` | Forced placement: the slot each word of a declaration must go in, if every way of placing the words agrees. |
| 85 | `groups.rs` | Values grouped by a typed key, stored contiguously. |
| 68 | `par.rs` | Data parallelism over borrowed slices. |
| 66 | `sparse.rs` | Range minimum and maximum in constant time: a sparse table. |
| 40 | `lib.rs` | The vocabulary every other crate speaks: exact quantities, calendar days and the ranges, months and schedules … |
| 39 | `timeline.rs` | A value that changes on days. |
| 39 | `hash.rs` | A fast, non-cryptographic hasher for interned keys and small integers. |
| 33 | `sym.rs` | Interned names. |
| 31 | `glob.rs` | Matching names against patterns. |
| 30 | `unit.rs` | What an amount is counted in, for type checking. |

#### `cli`: 2,350 lines in 16 files

| lines | file | what it is for (its own `//!`, first sentence) |
|---:|---|---|
| 429 | `commands.rs` | What each command does: load the project, open a session on it, and show what was asked for. |
| 385 | `args.rs` | The command line: which commands and options exist, and how arguments become a `Command`. |
| 376 | `table.rs` | Reports as tables. |
| 224 | `render/snippet.rs` | One file's share of a diagnostic, as rows: the source lines its labels point at with the marks under them, or … |
| 191 | `style.rs` | Colour, and the only code that knows what an ANSI escape looks like. |
| 131 | `render/mod.rs` | Diagnostics, drawn. |
| 116 | `fmt.rs` | `axiom fmt`: lay out only the requested source files using the syntax crate's formatter. |
| 105 | `project.rs` | Finding a project on disk and reading its sources. |
| 72 | `render/labels.rs` | The marks under one source line: an underline for every label, and the connectors that carry each label's text … |
| 70 | `render/findings.rs` | Many diagnostics, presented as few: in the order a reader fixes them, one report for each cause, and counted. |
| 65 | `main.rs` | `axiom`: the command line. |
| 63 | `render/page.rs` | The frame around a diagnostic: the gutter of line numbers, the corners that open each file's panel, and the `= … |
| 54 | `help.rs` | The usage screen, drawn from the command and option tables. |
| 39 | `sync.rs` | The CLI adapter for model-native sync planning. |
| 29 | `text.rs` | Small helpers for prose. |
| 1 | `render/json.rs` | The CLI uses the report crate's public, source-provider based JSON renderer. |

#### `session`: 502 lines in 6 files

| lines | file | what it is for (its own `//!`, first sentence) |
|---:|---|---|
| 176 | `sources.rs` | The texts a project is read from, by file number: what a diagnostic's `Loc` points into. |
| 110 | `edit.rs` | What a client sends a session and what it gets back: an edit as a typed value, the reasons one is refused, and … |
| 93 | `session.rs` | The session: a project read, built and folded, that answers queries and takes edits. |
| 75 | `transaction.rs` | A transaction as a typed value, and the line that says it. |
| 33 | `texts.rs` | The owner of every text a session has read: the reason a session can have a text that changes at all. |
| 15 | `lib.rs` | A loaded project as a value: the texts it was read from, the book built from them, and the run of its fold, … |

#### `systems`: 14 lines in 1 files

| lines | file | what it is for (its own `//!`, first sentence) |
|---:|---|---|
| 14 | `lib.rs` | The standard library of economic systems, written in Axiom and embedded. |

### 1.4 The 40 largest functions

By `docs/v5/measure/fnlen.py` (signature to closing brace, test modules excluded; its two hits on `cli/src/table.rs` `ink`
are a trait's method declarations the script misreads, and are left out). The last column says what becomes of each.

| # | lines | where | function | becomes |
|---:|---:|---|---|---|
| 1 | 403 | `model/lower/record.rs:747` | `lower_occurrence` | a written group (U7) merged once with its template (U28) |
| 2 | 152 | `engine/fire.rs:368` | `carry` | a carry moves basis between parcels, nothing else (U21) |
| 3 | 146 | `model/lower/record.rs:1154` | `lower_loan_origin` | a group with derived quantities (U7) |
| 4 | 109 | `engine/post.rs:822` | `dispose_sold_asset` | relief of the asset's parcels and the one `realize` (U21, U22) |
| 5 | 87 | `engine/lots.rs:1251` | `prepare_part_carry_additions` | goes: no second store to keep in step (U21) |
| 6 | 80 | `report/available.rs:95` | `spendable_section` | stays; its count of what has no price moves to the view (U42) |
| 7 | 79 | `model/lower.rs:44` | `inputs` | a `Signature` row (U4) |
| 8 | 79 | `engine/post.rs:523` | `arrive` | part creation leaves it (U21) |
| 9 | 78 | `model/declare.rs:467` | `book` | `..Book::default()` for the 30 empty arenas (U17) |
| 10 | 75 | `model/lower/record.rs:1403` | `lower_owes` | a group (U7) |
| 11 | 75 | `model/lower/contracts.rs:1232` | `escalation_property` | two `Signature` rows (U4) |
| 12 | 74 | `model/laws/compile/line.rs:68` | `tail` | the one tail reader with the `ALSO` clause set (U2) |
| 13 | 72 | `model/declare/places.rs:218` | `origin` | stays (the place tree) |
| 14 | 69 | `model/lower/record.rs:1303` | `claim_ends` | a group's ends (U7) |
| 15 | 68 | `model/lower/contracts.rs:1308` | `contract_area` | a `Signature` row and a measure check (U4) |
| 16 | 67 | `report/available.rs:245` | `Reach::of` | stays (the what-if withdrawal; lever L-f) |
| 17 | 67 | `engine/post.rs:935` | `match_pending_carries` | the carry queue against parcels only (U21) |
| 18 | 66 | `model/declare/parties.rs:223` | `declare` | the one meaning of a word (U11, K12b's `Addressed`) |
| 19 | 66 | `engine/totals.rs:436` | `History::record` | one recognized series (U29) |
| 20 | 65 | `model/sync_lower.rs:96` | `validate_pattern_calls` | stays (PEG call depth) |
| 21 | 65 | `engine/occurrence.rs:354` | `header` | one `Env` (U25), merged group (U28) |
| 22 | 65 | `engine/eval.rs:632` | `node` | stays |
| 23 | 63 | `model/lower/flow.rs:471` | `lower_items` | the group's items, one for journal and template (U7) |
| 24 | 62 | `engine/fire.rs:544` | `enforce` | stays |
| 25 | 61 | `model/lower/tail.rs:152` | `FlowCx::read` | the one tail reader (U2) |
| 26 | 61 | `model/lower/contracts.rs:184` | `lower_contract` | the contract skeleton (U10) |
| 27 | 60 | `model/laws/mod.rs:333` | `fits` | a column of the one trigger table (U12) |
| 28 | 60 | `engine/ledger.rs:528` | `post_computed` | exchange costs said by the group (U25) |
| 29 | 60 | `engine/explain.rs:782` | `mismatch` | stays (the balance suspects) |
| 30 | 59 | `model/lower/record.rs:607` | `lower_opening_leg` | a group (U7) |
| 31 | 59 | `engine/fire.rs:220` | `enter_months` | stays |
| 32 | 58 | `model/lower/statements.rs:755` | `lower_basis` | a group from the unknown party (U7) |
| 33 | 57 | `model/lower/flow.rs:370` | `make_resolved_flow` | `FlowDraft` (U7) |
| 34 | 56 | `report/balance.rs:236` | `push_place` | stays; K7c-1 touches its notes (U35) |
| 35 | 56 | `model/sync_lower.rs:39` | `declare` | the walk moves to `collect` (U6) |
| 36 | 55 | `model/lower/contracts.rs:313` | `loan_fields` | a `Signature` row (U4) |
| 37 | 55 | `model/declare/holdings.rs:29` | `declare_accounts` | stays |
| 38 | 55 | `engine/eval.rs:1288` | `budget_limit` | stays |
| 39 | 54 | `report/forecast.rs:245` | `expected_section` | the habit rows read promise streams (U36) |
| 40 | 54 | `report/available.rs:38` | `from_ledger` | stays |

3,546 functions, mean 12.5 lines, seven over 80 lines and one over 200. **The size is not in long functions**: after U the three
over 100 are gone and nothing in the ledger depends on splitting a function by line count.

### 1.5 The concepts that appear in the most files

Counted with `grep -l` over the non-test sources (the pattern for each is in parentheses):

| concept | files | the unification that makes it one |
|---|---:|---|
| a diagnostic assembled by hand (`Diagnostic::error(`…) | 72 | U16, U49 (diagnostics as data where the wording is fixed) |
| a thing's name (`book.name(…path/name/symbol)`) | 63 | U40 (`Book::name_of(Thing)`) |
| an amount literal read (`literal_amount`, `.num()`) | 41 | U3, U4 |
| a window or period of days (`Window::`, `Period::`, `Periods::`) | 37 | U29, U13 (`law::Window` is `Option<Period>`) |
| "which thing" matched arm by arm (`Subject::`, `Holder::`, `Object::`) | 32 | U40 (`Thing`), U31 |
| a purpose looked up or worded | 32 | U11, U40 |
| a flow's two ends and its route | 29 | U7 (`FlowDraft`), U24 (`FlowRef`), U37 |
| a table row of typed cells (`Cell::`) | 27 | U37, U38, U43 |
| whose money: owner scope (`lens.owns`, `governs`) | 26 | U38, U39 |
| an enum said in words (`*_words`, `word()`, `noun()`, `phrase()`) | 23 | U12, U40 |
| a value in the base currency (`convert`, `base_value`, `lens.value`, `prices.rate`) | 19 | U14 |
| values grouped by a key (`Groups::build`, `bucket`) | 15 | U31, U46 |
| the clauses of a line's tail (`ClauseKind::`) | 12 | U2 |

---

## 2. The concept inventory

Each concept the tree writes more than once: where, what each copy does differently and whether the difference is real, and
the one formulation that replaces them. Signatures are sketches at the level the first checkpoint's design (section 8) is held
to; lifetimes are elided where they add nothing. Line numbers are at `25566a5`.

### 2.1 Lowering one source site

**Where.** 150 functions of `model` take `world: &mut World`, `diags: &mut Vec<Diagnostic>` and, nearly always, `home: Home` and
`file: &ast::File`: every function of `lower/contracts.rs` (55 of them), `lower/statements.rs`, `lower/record.rs`,
`lower/flow.rs`, `lower/tail.rs` (`resolve_object`, `written_purpose`, `written_waive`), `laws/compile/line.rs` (`tail`,
`read_line`, `implied_flow`, `implied_end`, `lower_selectors`), `laws/budget.rs`, `props.rs`, `params.rs`, `sync_lower.rs`.
Their signatures alone are **930 lines** (measured from `fn` to the opening brace), six lines each on average, and most call sites
wrap. Three structs already carry part of it: `FlowCx` (`lower/flow.rs:49`: file, home, day, txn, loc, roots, code index),
`TermsCx` (`lower/contracts.rs:656`), `Stated` (`lower/statements.rs:42`), and the law compiler's `Compiler`
(`laws/compile.rs:307`) holds `world`, `diags`, `file` and `home` and is the one place the pattern is done right.

**What differs.** Nothing but which of the four a function happens to need: every one of them lowers something written in one
source site into the world, and reports to the one list.

**The one formulation.**

```rust
/// One source site being lowered into the world: what it may see, and where what is wrong with it goes.
pub(crate) struct Lowering<'w, 'a, 's> {
    pub world: &'w mut World<'s>,
    site: &'a Site<'a, 's>,
    diags: &'w mut Vec<Diagnostic>,
}
impl<'w, 'a, 's> Lowering<'w, 'a, 's> {
    pub fn file(&self) -> &'a ast::File<'s>;
    pub fn home(&self) -> Home;
    pub fn word(&self, name: ast::Name<'s>) -> Word<'s>;
    pub fn say(&mut self, problem: Diagnostic);
    pub fn or_say<T>(&mut self, found: Result<T, Diagnostic>) -> Option<T>;
    // the lookups every pass makes, in the site's scope
    pub fn entity(&mut self, name: ast::Name<'s>) -> Option<Id<Entity>>;
    pub fn purpose(&mut self, written: ast::Purpose<'s>, reach: Reach) -> Option<Purposed>;
    pub fn commodity(&mut self, name: ast::Name<'s>) -> Option<Id<Commodity>>;
    pub fn end(&mut self, end: ast::End<'s>, on: Option<Day>) -> Option<ResolvedEnd>;
}
```

A pass is a method of the site it lowers: `fn contract(&mut self, written: &ast::Contract<'s>) -> Option<Contract>`, not a
free function of six arguments. `Compiler` becomes a `Lowering` plus the run of nodes it builds. This is not a bundle to hit a
count: the four parameters are one thing in the domain (the lowering of one site), and the law compiler shows it is the
shape the passes want. U1.

### 2.2 What a line says about its flow: the tail

**Where.** Eight readers of the same `ClauseKind`, each a loop over `file[clauses]` with its own subset and its own refusal:

| reader | at | takes | refuses the rest with |
|---|---|---|---|
| `FlowCx::read` | `lower/tail.rs:152` | all 12 | `until-position` for `until` |
| `lower_term_tail` | `lower/contracts.rs:1002` | code, purpose, description, waive | nothing: "left unread, as it always has been" |
| `laws::compile::line::tail` | `laws/compile/line.rs:68` | code, purpose, description, waive, `for` whom, since, due on, literal basis | `also-tail`, `also-relative-due`, `computed-also-basis` |
| `measure_tail` | `lower/statements.rs:433` | for whom, purpose, description, code, against | `measure-tail` |
| `waiver_tail` | `lower/statements.rs:613` | until, code (once), description, purpose (refused) | `statement-lowering`, `unreachable!` |
| `ending_codes` + `ending_description` | `lower/statements.rs:704,719` | code, description | `unreachable!` |
| `assertion_gap` | `lower/statements.rs:358` | via, waive, description, code | `unreachable!` |
| `lower_claim_change` | `lower/statements.rs:462` | description | `statement-lowering` |

and three results to hold what they read: `Tail` (9 fields, with a 35-line field-by-field `merge`), `TermTail`, `Metadata`, plus
`MeasureTail` and `WaiverTail`. The `unreachable!` arms exist because **the parser already refuses those clauses**: the set a
statement takes is written twice, once in `syntax/src/statement.rs` (`takes`) and once here.

**What differs, and whether it is real.** The *set* of clauses a line takes is real and differs by record; the reading of each
clause is identical (a code is interned and pushed, a purpose is `written_purpose`, a description is `quoted_text`, a waive is
`written_waive`). Two differences are real and stay as data: a relative `due` is refused on an `also` line, and a computed
basis is a root on a flow and refused on an `also`.

**The one formulation.**

```rust
/// What a line says of the flow it makes: one type from the lowering to the fold's cold record.
#[derive(Clone, Copy, Default)]
pub struct Says {
    pub purpose: Option<Purposed>,
    pub description: Option<Text>,
    pub payee: Option<Id<Entity>>,
    pub recognized: Option<Days>,
    pub waive: Option<Waive>,
    pub detail: Detail,
    pub basis: Option<Basis>,          // Stated(Qty) | Computed(NodeId)
    pub price: Option<Price>,
}
/// The clauses a kind of line takes, and what it says of one it does not.
#[derive(Clone, Copy)]
pub struct Clauses { takes: u16, refused: Refusal }
impl Clauses {
    pub const FLOW: Clauses; pub const TERM: Clauses; pub const ALSO: Clauses; pub const MEASURE: Clauses;
    pub const WAIVER: Clauses; pub const ENDING: Clauses; pub const VALUE: Clauses; pub const WRITE_OFF: Clauses;
}
impl Lowering<'_, '_, '_> {
    /// Reads every clause of a tail the line takes; the codes go to the pool and come back as a run.
    pub fn tail(&mut self, clauses: ast::Many<ast::Clause>, takes: Clauses, at: TailAt) -> (Run<Sym>, Says);
}
impl Says { pub fn under(self, parent: Says) -> Says }   // a child's tail over its parent's: the 35-line merge
```

`syntax/src/statement.rs` reads the same `Clauses` consts for what the parser keeps off a statement, so the set is written
once. `Says` is also what `split::Says`, `book::Derived`'s metadata and `occurrence.rs`'s `Stamp` copy field by field today
(2.10). U2, U17.

### 2.3 A written amount and a written quantity

**Where.** Seven functions turn `ast::Amount` (a literal or the root of a compiled expression) into a model amount:
`stated_amount` (`lower/flow.rs:230`), `resolve_amount` (`lower/flow.rs:563`), `template_amount` (`lower/contracts.rs:935`),
`derived_amount` (`laws/compile/derive.rs:123`), `assertion_amount` (`lower/statements.rs:319`), `basis_cost`
(`lower/statements.rs:835`) and `implied_amount` (`laws/compile/line.rs:291`); two turn `ast::Quantity` into a `Part`:
`resolve_quantity` (`lower/flow.rs:189`) and `template_quantity` (`lower/contracts.rs:883`). `ast::Sign` is mapped to
`split::Sign` three times (`lower/flow.rs:522`, `lower/contracts.rs:973`, `laws/compile/line.rs:186`) and `ast::Cadence` to
`book::Cadence` once (`lower/contracts.rs:728`), with `syntax::Cadence` a copy of `core::Cadence` (`syntax/src/ast.rs`).

**What differs.** Real: a missing root is said in a template (`template-root`) and silently dropped in a journal flow
(`stated_amount`: "the diagnostic is dropped here, as it always was"), and a template reads a bare percentage as a share.
Not real: everything else, and the enum maps (the model's `Sign` and `Cadence` have the syntax's variants exactly).

**The one formulation.**

```rust
impl Lowering<'_, '_, '_> {
    /// A written amount: its literal, in `fallback`'s unit if it names none, or the node that computes it.
    pub fn amount(&mut self, written: ast::Amount, fallback: Id<Commodity>, roots: &Roots) -> Result<Expr, Unsaid>;
    /// What a quantity written at one side of a line takes of its group, and the amount its flow carries meanwhile.
    pub fn quantity(&mut self, written: ast::Quantity, side: FlowSide, fallback: Id<Commodity>, roots: &Roots) -> Option<Taken>;
}
/// Why a written amount is none: said already, or for the caller to say or drop (the journal's flows drop it).
pub enum Unsaid { Said, Missing(Diagnostic) }
```

The model uses `ast::Sign`, `ast::Cadence` (= `core::Cadence`), `ast::Policy` and `ast::Period` directly: one vocabulary,
borrowed along the pipeline. U3.

### 2.4 The arguments of a property line

**Where.** A property line (`NAME ARG …` with nested lines) is parsed generically by `syntax/src/decl.rs` (`prop`) and then
**read by hand, argument by argument**, in five places:

- `lower/contracts.rs`: `loan_fields` (313), `loan_principal` (378), `loan_asset` (408), `loan_prepay` (440), `loan_resets`
  (485), `reset_schedule` (510), `reset_index` (560), `reset_limits` (590), `reset_keyword`, `is_percent`, `percent_ratio`,
  `invalid_reset`, `positive_loan_term`, `contract_days` (1070), `span_property` (1122), `grace_property` (1143),
  `has_property`, `relative_property` (1171), `coverage_property` (1201), `escalation_property` (1232), `contract_area` (1308),
  `contract_deposit` (1385), `deposit_amount`, `holding_name`, `shares` (1536), `read_share_line`, `share_rate`,
  `measured_numerator`, `share_owner`, `add_share`: **55 hand-built diagnostics**, about 650 code lines;
- `props.rs`: the `Args` reader (261-503: `next_id`, `wrong`, `arg`, `done`, `word`, `name`, `day`, `span`, `text`, `count`,
  `percent`, `entity`, `place`, `currency`, `citizens`, `basis`, `books`, `purpose`, `share`, `part_of`, `policy`, `holds`,
  `residence`), the `BUILTINS` table (102) and `is_builtin_line` (1043, the same 25 names again);
- `lower.rs:44` `inputs` (79 lines): a contract's `input NAME [UNIT]` lines;
- `lower/statements.rs:578` `lower_rate_change`: `now at PERCENT`;
- `sync_lower.rs`: `FormatReader::read_line`/`read_field`/`column`/`read_spec` (591-714) and `source_format`/`source_sink`/
  `source_feed` (857-946): the lines of `format` and `source` declarations.

`props.rs`'s `Args` is already the general reader (typed `day()`, `span()`, `percent()`, `word(&[…])`, with the generic
`property-argument`/`property-type` diagnostics); the contract, input, rate and sync readers are the same thing written
again with bespoke messages.

**What differs, and whether it is real.** The *shapes* differ (a loan is `AMOUNT on DATE at PERCENT over SPAN [for ASSET]`, a
reset `SPAN from DATE to PARAM + PERCENT [cap PERCENT] [life PERCENT]`) and are data. The *checks after reading* are real
and stay: a principal and an area are positive, a deposit's holding is an account of the owner that accepts its unit, shares
total at most 100%, a reset cannot precede the loan, a measured share needs an area in the same unit. The *wording* of the
shape errors differs per line and is pinned by nothing but the model's own tests (codes only: `contract-loan-resets`,
`contract-area-positive`, `contract-deposit-positive`, `contract-deposit-holding`, `contract-share-measure`; no mistake book
and no golden prints one). U4 keeps every message byte-for-byte as a column of the table; lever L-d is the generic wording.

**The one formulation.**

```rust
/// What a property line is: its words and values in order, said once as data.
pub(crate) struct Signature {
    pub name: &'static str,
    pub code: &'static str,           // the diagnostic family its shape errors carry
    pub shape: &'static [Slot],
    pub once: Once,                   // Once::Only (said twice is an error) | Once::Repeats
    pub lines: &'static [Signature],  // the nested lines it takes (a loan's `prepay`, `resets`)
}
pub(crate) enum Slot {
    Word(&'static str, Said),         // a keyword, and what is said when it is not there
    Value(Want, Said),
    Optional(&'static [Slot]),
    Repeat(&'static [Slot]),
}
pub(crate) enum Want { Day, Span, Percent, Fraction, Amount, Measure, Name, Entity, Place, Asset, Commodity, Param,
                       System, Purpose, Text, Count(u8), OneOf(&'static [&'static str]), IndexPlusMargin }
pub(crate) struct Said { pub message: &'static str, pub label: &'static str }   // the wording the line has today
/// A line read against its signature: the values in order, each where it is written.
pub(crate) struct Args<'a, 's> { line: &'a ast::Prop<'s>, values: Vec<(Arg, Loc)> }
impl Lowering<'_, '_, '_> {
    pub fn args<'a>(&mut self, line: &'a ast::Prop, signature: &'static Signature) -> Option<Args<'a, '_>>;
}
```

`LOAN`, `RESETS`, `PREPAY`, `GRACE`, `COVERS`, `RISING`, `INDEXED`, `AREA`, `DEPOSIT`, `SHARE`, `FROM`, `UNTIL`, `INPUT`,
`RATE`, the 25 built-ins and the format and source lines are rows; the contract becomes "read its lines by their
signatures, then check what they mean". The built-ins' readers (`says!`, `says_all!`) become a column of the row that says
which key a value is painted to, which is K12b's "built-in properties on the generic fill path", measured: it deletes. U4.

### 2.5 Filling a slot

**Where.** `fill.rs` fills the slots a kind declares from a property line (kinds, words and typed values; counts; weights) and
`props.rs::missing_roles` says what nothing filled. `lower/contracts/relator.rs` fills the slots of a contract's kind
(`fillers`, `fits`, `Misfit`, `unfilled`, `many`) with its own range check and five diagnostics (`relator-slot-unknown`,
`-twice`, `-kind`, `-value`, `-missing`).

**What differs.** A contract's fillers are stored on the contract (`Contract.fillers`), a thing's in the facts; the range check
is the same check (`Range::Kinds` covering the entity's kind). The relator diagnostics are pinned by
`model/tests/relator.rs` (codes) and worded for contracts.

**The one formulation.** `fill::fill` takes the target's vocabulary (`Filling::Thing` or `Filling::Role`, which says the codes
and the words) and returns `Filling`; the relator keeps its diagnostics' text as that vocabulary and loses its own range check
and count. Small (−50) because the wording stays. U5.

### 2.6 Walking the sources

**Where.** `collect.rs` (K0a: "one look at every item of every source, sorted into typed buckets") is the walk. Six passes walk
again: `declare/mentions.rs` (flows, tails, contracts, statements, `also`s), `laws/mod.rs:291` `counted` (every law step),
`laws/budget.rs::Found::read` (every item), `slots.rs::values_written`, `lower/loan_opening.rs` `named_by_openings` and
`originations`, `declare.rs::scopes` (every entity's `lives` and every `now lives`).

**What differs.** What each wants of the walk; the walk is one. **One formulation:** `Collected` gains the lists those passes
want (`mentions`, `tallies`, `budgets`, `lives`, `originations`), filled in its one pass. U6.

### 2.7 A line that moves value: one group lowering

**Where.** Every written line that makes flows is a group (K4a's `Group<H, F, I>`: a header, legs, items) with a tail, and each
kind of line lowers it with its own copy of one skeleton (`Staged::open` → roots → `compile_template` → a context struct →
tail → `make_resolved_flow`/`lower_items` → `balance::settle` → `keep_program` → `journal_txn` → push → `commit`):

| line | lowered by | lines |
|---|---|---:|
| a journal transaction (named ends, a split, a statement's legs) | `lower_txn` (`record.rs:274`), `lower_flows` (326), `lower_named_flow` (356), `lower_split_flow` (431), `make_flow` (`flow.rs:244`), `lower_items` (`flow.rs:471`) | ≈420 |
| an opening | `lower_opening` (540), `lower_opening_balances` (563), `lower_opening_leg` (607) | ≈170 |
| a written occurrence of a contract | `lower_occurrence` (747) | 403 |
| a loan's origination | `lower_loan_origin` (1154) | 146 |
| `owes` | `lower_owes` (1403), `claim_ends` (1303), `claim_program` (1375) | ≈180 |
| a contract's schedule (its template) | `lower_terms` (`contracts.rs:682`), `template_header` (737), `template_legs` (796), `TermsCx::flow` (852), `lower_header_item` (961) | ≈250 |
| an `also` / `derive` line | `read_line`, `implied_flow`, `implied_end`, `implied_item` (`laws/compile/line.rs:153-308`) | ≈150 |
| an asset's basis statement | `lower_basis`, `arrival_flow` (`statements.rs:755,878`) | ≈90 |
| a loan made before the book | `push_opening` (`loan_opening.rs:154`) | ≈40 |

and a `Flow` literal of twenty fields is written out in five of them (`make_resolved_flow`, `TermsCx::flow`, `push_opening`,
and twice more in `record.rs`).

**What differs, and whether it is real.** Real: *what the group is lowered against* (a journal transaction on its day, a
contract's terms with no day, a law's derive template with roles for ends), *who the ends are* (written, the schedule's
holding and the party, a role's position, the unknown party, the opening entity) and *what the quantities may be* (a
template's `Derived` and `Interest`, an occurrence's inputs). Not real: the skeleton, the end resolution, the quantity
resolution (2.3), the tail (2.2), the item lowering (`lower_items` and `lower_header_item` are one function over two item
payloads), the flow construction and the program keeping.

**The one formulation.**

```rust
/// What a group is lowered against.
pub(crate) enum Against<'a> {
    Journal { day: Day, txn: Id<Txn> },
    Terms { contract: Id<Contract>, anchor: Day, inputs: &'a [Input] },
    Derive { positions: Positions<'a> },
}
/// A flow being made: everything but what the fold reads off the group.
pub(crate) struct FlowDraft { pub from: ResolvedEnd, pub to: ResolvedEnd, pub out: Amount, pub arrive: Amount,
                              pub infer: Infer, pub mode: Mode, pub says: Says, pub loc: Loc }
impl Lowering<'_, '_, '_> {
    /// One written group: its header, legs and items, each line's tail, resolved against `against`.
    pub fn group<H: Header>(&mut self, written: Written<'_, H>, against: Against<'_>, roots: &Roots) -> Option<Lowered<H>>;
    /// Keeps a lowered journal group: its flows, its program, its transaction; nothing of it if any of it failed.
    pub fn keep(&mut self, lowered: Lowered<JournalHeader>, record: RecordKind) -> Option<Id<Txn>>;
}
```

`H` is what the header is (a journal header with both ends, a source-only split, an opening line, an occurrence's override,
a template's schedule): the part of each copy that is real. U7, U9, U10.

### 2.8 A statement's subject

`statement_target` and `named_target` (`statements.rs:165,182`), `end_target` (675), `basis_asset` (815) and `claim_target`
(491) each resolve a statement's subject with their own precedence: a value asks asset, then a loan's debt, then an end; an
ending asks contract, then asset, then end. The precedences are real (a contract's name is its debt in an assertion and the
contract in an ending) and become one `Subject` resolver returning everything the word could be, from which each verb picks.
U8.

### 2.9 Names to ids

**Where.** `resolve.rs` writes the same three functions per namespace: `seek_X` (one, none, or several), `X` (the error with a
did-you-mean), `ambiguous_X`: kinds (111-121), entities (126-159), purposes (164-182), places (187-211), params (399-451),
commodities (44), systems (453). `Book::place`/`Book::entity` (`book.rs:1167,1192`), `World::end_on` (`resolve.rs:224`) and
`World::address_end` (`reference.rs:54`) are the three entry points K12b names, with `settle_addresses`
(`reference.rs:95`) a second cache of meaning that `special_end`/`found_end` pay for with `#[inline(always)]`. A path answers to
its `/`-suffixes in four places: `names.rs:73` `suffixes`, `declare.rs:550` `add_path_spellings`, `:562`
`strict_path_suffixes`, `:360` `push_name_claims`.

**What differs.** Real: places resolve by address on a day, params by system qualifier, kinds through the system scope. Not
real: the seek/error/ambiguity triple, and the suffix enumeration.

**The one formulation.**

```rust
pub(crate) trait Named: Sized {
    const NOUN: Noun;
    fn index(book: &Book) -> &Scoped<Self>;
    fn path(book: &Book, id: Id<Self>) -> Sym;
    fn loc(book: &Book, id: Id<Self>) -> Option<Loc>;
}
impl World<'_> {
    pub fn seek<T: Named>(&self, home: Home, word: Word) -> Seek<T>;
    pub fn find<T: Named>(&self, home: Home, word: Word) -> Result<Id<T>, Diagnostic>;
    /// What a word written as a flow's end means, on a day, from a home: the one reading the three entry points ask.
    pub fn meaning(&self, home: Home, word: Word, on: Option<Day>) -> Result<End, Diagnostic>;
}
```

and `paths::suffixes(path)` the one enumeration. U11.

### 2.10 What a flow says, and a flow as read

`Says` (2.2) replaces `Tail`, `TermTail`, `Metadata`, `split::Says`, `MeasureTail`, the metadata half of `book::Derived` and
the copying `Stamp::on` (`engine/occurrence.rs`). A flow is then *read* in five shapes: `Flow` (20 fields, `journal.rs`),
`FlowView` (the view with a detail from either arena), `RuntimeFlow` (a flow with a runtime detail and ordinal), `Motion` (21
fields, `engine/motion.rs`, most of them copied from the view) and `Posting` (`report/history.rs`). K4c's `FlowRef<'_>` (a
`Copy` struct of borrows with accessors) is the one read shape: `FlowView` and `RuntimeFlow`'s view become it, `Motion` keeps
only what the fold adds (cause, day, solved amounts, orientation, ordinal) beside a `FlowRef`, and `Posting` is a `FlowRef` with
its `Posted`. U17, U24.

### 2.11 Triggers, said seven times

`law::Trigger` is matched in `laws/compile.rs:395` (from the syntax), `laws/vars.rs:27,41` (`When::of`, `phrase`),
`laws/order.rs:20` (`occasion`), `laws/mod.rs:333` (`fits`: which owners, its message and help), `report/why.rs:324`
(`trigger_words`), `report/json.rs:381` (`write_trigger_words`) and `cli/src/table.rs:382` (`write_trigger`), the last three
identical. **One formulation:** one table `TRIGGERS: [(Trigger, Words, Occasion, Fits)]` read by `Trigger::words()`,
`Trigger::occasion()`, `Trigger::fits(owner)`, and the syntax's `ON_TRIGGERS` reads its words. U12, U40.

### 2.12 The law compiler's small duplicates

`Compiler { … }` is written out three times (`compile.rs:245,282,340`); `owner_amount_ty` and `value_amount_ty` (835, 857) map
an owner to its currency twice; the words `month`/`year`/`ever` are parsed three times (`total`, `window_word`, `keyword`);
the built-in fields are listed three times (`field` at 766, `unknown_field` at 902, `slots.rs:121` `FIELD_WORDS`); a law is
synthesized by hand four times (`derive.rs::share`, `budget.rs::lower_budget`, `Effect::Carry`'s constant node,
`amount_node`), and a budget's formula is compiled into an arena of its own and then copied node by node into the law's
(`budget.rs::offset_op`, 35 lines). `laws/mod.rs::register` buckets laws by owner into four `Vec<Vec<_>>`. **One
formulation:** `Compiler::new(lowering, owner, when)`, one `FIELDS` table, `Window::parse`, a `Nodes` builder for
synthesized laws, compiling a budget formula into the law's arena, and `Groups::build` for `register`. U13.

### 2.13 The value of an amount in another commodity

**Where.** Three paths: `model/src/prices.rs` (`Prices::rate`/`direct`/`latest`, 44-72), `model/src/book.rs`
(`convert`/`convert_for` → `conversion_path` → `spot_rate_use`/`latest_quote`/`param_rate_use`, 1248-1400), and the report's
`Lens::exact` (`report/lens.rs:168`) which calls `prices.rate` and scales with six extra digits. The engine's `base_value_on`
(`post.rs:1125`) goes through `book.convert`.

**What differs.** Real: the report rounds once at the end of a sum (six extra digits), the engine per amount; `convert_for`
honours a system's rate policy (spot or a param). Not real: the two lookups of the latest quote and the inverse/via
fallbacks (traced to give the same rate). **One formulation:** `Book::rate(from, to, day, policy) -> Option<RateUse>`, and both
`convert` and `Lens::exact` ask it. U14.

### 2.14 Diagnostics assembled by hand

448 sites build a `Diagnostic` field by field (model 281, syntax 79, sync 39, engine 37, cli 7, report 3, session 2): 1,181
lines of builder chains, and about as many again in `diags.push(` / `return None;` around them (each chain counted from
`Diagnostic::` to its last `)`). `model/src/problem.rs` (K0a) is the catalog for the families; the rest are one-offs. **One
formulation:** a diagnostic whose wording is fixed is a `const` row (`Problem { code, message, label, help }`), said with
`lowering.say(LOAN_DATE, at)`; one whose wording has arguments stays a function in `problem.rs`. What this saves is the
ceremony, about two lines a site, and only after U4 and U7 have deleted the sites they own. U16, U49.

### 2.15 Where value rests, and how it leaves (K3e)

**Where.** `engine/src/lots.rs`: `relieve` dispatches to `relieve_in_order` (533) or `relieve_scanning` (656), which use
`take_plain` (566), `take_exact` (577), `take_run` (599), `take_priciest` (625), `take_dearest` (637), `gather` (694),
`whole_claims` (886), `by_policy` (900), `basis_per_unit` (910), `interchangeable` (915), `allocate` (919), with `Colour`,
`Candidate`, `Ranked`, a cursor (`first`), a lazily built heap and a sweep. A parcel's identity is recomputed at every landing
and compared by a hand-written `PartialEq` (`Identity`, 54-115).

**What differs, and whether it is real.** The order each policy takes in is real and must stay byte-exact (K3c: `exact` is the
parcels of one transaction adding up, else the oldest); the cursor, the heap and the scan are three mechanisms for "take in rank
order", kept for speed.

**The one formulation (K3e's).** `rank: fn(Policy, &Request) -> impl Ord` over a parcel's hot fields, `take(ranked, need, share:
Share)` (all of one before the next; pro rata across a tie by weight), and an identity *key* hashed once at landing, merging by
an integer compare. The heap and cursor stay only if the benchmark at 100k parcels says the ranking cannot match them. U19,
U20.

### 2.16 An asset part's basis, kept twice

**Where.** `engine/src/assets.rs` keeps per part `cost` and `basis`; the parcels that carry the asset (`Parcel.part`) carry the
same basis. Every change goes through a guard that prepares both and applies both: `prepare_consumption`/`ConsumptionGuard`
(386, 588), `prepare_carry`/`CarryGuard` (429, 619), `prepare_basis_additions`/`AssetBasisBatchGuard` (467, 631),
`lots.rs::prepare_part_basis_adjustment`/`PartBasisAdjustment` (1195, 963), `prepare_part_carry_additions`/
`CarryLotBatchAdjustment` (1251, 993), `assets_runtime.rs::AssetPartAddition` (41), and `anchor_and_total` with
`holdings.part_basis(anchor) != total => ParcelBasisMismatch` eight times (`assets_runtime.rs`, `post.rs::dispose_sold_asset`).
`post.rs` builds a `Part` three times (`new_acquisition_part` 606, `add_acquisition_part` 680, `add_improvement_part` 725),
each with its own `asset-cost` diagnostic.

**What differs.** Nothing that is not the invariant "the two stores agree", which the guards exist to keep. The part table's
own information is real: which parts an asset has, their kind (acquisition or improvement), their cost, when recorded, their
service day, the disposal, and what laws consumed and carried (already recorded as `Adjustment`s).

**The one formulation.** A part's basis is read from its parcels (`Holdings::part_basis`, which exists, at `lots.rs:1168`);
`Part { id, flow, kind, recorded, day, cost }` loses `basis`; consumption and carry change the parcels only, through one
`Holdings::adjust(part, delta, Weight)` that checks before it writes (no guard types); a part's own share of the anchor's
basis, which depreciation per part reads, is `cost − consumed(part) + carried(part)` from the run's adjustments, kept as two
running sums beside the part. This is K3c §4's "smaller cut", costed: −910. No golden, mistake book or test pins an
`asset-state` diagnostic (checked); the eight mismatch checks become unrepresentable. U21.

### 2.17 Realizing what leaves

`post.rs::realize` (453) and `dispose_sold_asset` (822) build, per relieved slice, the same `Gain`, the same `Realized` and the
same `on gain` occasion, then fire `Watch::Gain`. One `realize(slices, from, to, unit, proceeds)`. A `Parcel` literal of nine
fields is written in `arrive` and `add_acquisition_part` and `rebase`. U22.

### 2.18 The records of a fold

`Run` (`engine/lib.rs:160`, 25 fields), `Recorded` (622, the same lists borrowed), `Applied` (643, the same lists as ranges) and
`state::Record` (the owned lists, with `marks`/`since` writing `Applied` field by field and `finish` moving `Record` into `Run`
field by field). Two records of what a flow settled (`Record::settled` and `Record::settlements`, `settle.rs:173-174`; K12b).
**One formulation:** `Records` (the owned lists) with `view() -> &Records`, `marks()`/`since()` over one `[usize; N]`, and
`Run { records: Records, … }`. U23, U26.

### 2.19 Streams of due days

`engine/monitor.rs` keeps a heap of `Waiting` streams (one `Residual` per contract schedule) to find what was missed;
`engine/promising.rs` keeps a heap of `Ahead` streams (one `Residual` per contract schedule) to post what falls due in a
forecast; `report/forecast/recurrence.rs` keeps a third walker (`Schedule::days` over `core::calendar::due`) for habits. The
start loops are the same (`promises.by_contract()` × `[Regular, Standing]`). **One formulation:** `Streams<R>` (K12b's), a heap
of `(next due, stream)` over `Residual`, which a habit becomes too (2.24). U27, U36.

### 2.20 A written occurrence and its template, merged twice

The model's `lower_occurrence` (403 lines) lowers a written occurrence against its contract's template: it binds inputs,
matches the written legs to the template's by ends, applies the override amount and price, and builds `WrittenOccurrence` with
groups. The engine's materializer (`engine/occurrence.rs`) matches them again at fold time (`written_leg_for_template`,
`same_flow_ends`, `legs_at`, `items_at`, the ordinal arithmetic) to make the occurrence's flows. **One formulation:** the model
lowers the written lines as a group of their own (U7) with no matching; the engine's `materialize` is the one merge. U28.

### 2.21 What was recognized into a window

`engine/totals.rs` keeps two structures for one question: `Windows` (rolling month, year, the year just closed, ever, and a list
of accruals ahead, 86-163) for the laws' `total(…)`, and `History` (block prefix sums over day facts with a slow path for
ranges, 408-541) for budgets that carry. `Flowed`, `DayFact` and `BlockPrefix` are three `{incoming, outgoing}` pairs with
their own `side`/`side_mut` by `Dir`. **One formulation:** one recognized series per key (`History`'s prefix blocks, with a cursor
for the current month and year so the hot path stays O(1)), and `[Qty; 2]` indexed by `Dir`. U29.

### 2.22 A balance over time

The fold samples the values of `peak`, `low` and `days` expressions into `temporal.rs` (`TemporalHistory`) at every change date
(`ledger.rs` `sample_temporal*`, `plan.rs` `temporal_queries`/`change_dates`, `eval.rs` `temporal`/`extreme`/`day_count`), while
K7b's `histories.rs` records every position's balance as steps and offers `Steps::extremes` over `core::sparse`, which **nothing
reads**. For the common root (`self.balance`, a place's or entity's balance, `value(balance, UNIT)`) the steps answer
`peak`/`low` directly and `days` is `Steps::days_where`. **One formulation:** the temporal functions read the run's steps where
their root is a balance, and sample only other roots. U30.

### 2.23 Places within a subject

`plan.rs` builds `entity_places` (`Groups<Entity, Place>`), `asset_places`, `kind_places` (a `Map<Kind, Box<[Place]>>`) and uses
`book.places.covers` for places; `totals.rs::places_within` builds `Groups<Place, Subject>`, the inverse. One `Within` index
(subject → places, place → subjects) serves `Plan::inside`, `places_of`, `kind_places` and the totals' `Watch`. U31.

### 2.24 The habit forecast

`report/forecast/expected.rs` finds habits in history and makes `Expectation`s with a `recurrence::Schedule` of their own; the
forecast applies their flows through `trace.rs::apply_habit` beside the promises the ledger posts (`Ledger::promise`, K5c).
Decision 11: the habit forecast stays and "becomes a source of flows applied through the same fold (K5c's interface), and lane U
shrinks it". **One formulation:** a habit is compiled to a `promise::Stream` (a cadence, a day of the month, a template
flow) and posted by `Promising` like a contract's occurrence; "What recurs" reads the streams. U36.

### 2.25 Enums said in words

21 functions return a `&'static str` for an enum and as many matches are written inline: `State` (`register.rs:187`,
`register.rs:614`, `why.rs:78`), `PurposeRoot` (`flow.rs:31`, `why/purpose.rs`), `Cadence` (`contracts.rs:1005`,
`forecast.rs::describe_contract`, `recurrence::describe`), `EventState`, `Provenance` (`lower/infer.rs:142`, `why/line.rs`),
`Action`, `Sign`, `FlowSide`, weekdays, the trigger (2.11). **One formulation:** `trait Words { const WORDS: &'static [(Self,
&'static str)]; fn word(self) -> &'static str; fn parse(&str) -> Option<Self> }`, as `Policy::WORDS` already is, read by the
parser, the model, the report and the CLI. U40.

### 2.26 Which thing

`Holder` (5 sorts, the facts), `Subject` (4, the laws), `Object` (3, a purpose's), `Owner` (8, a law's), `StatementTarget`,
`EndTarget`, `NativeTarget`, report's `Target`: each matched arm by arm for a name, a location, an owner, a kind. The sum types
are real (each admits a different set); the per-arm lookups are not: `Book::name_of`, `loc_of`, `owner_of`, `kind_of` over a
`Thing` that each converts into. U40.

### 2.27 Rows of flows in the report

`register.rs` builds four registers (place, entity, asset, contract) with different columns; `why.rs::flows_table`,
`why/asset.rs::about`, `why/contract.rs::derived_section` and `why/text.rs` build four more tables of flows. K7c-2 and K7c-3
(decided) make the entity, asset and contract registers the place register's columns and route `why asset:`/`why contract:`
through the shared table. **One formulation:** `FlowTable { columns: &[FlowColumn] }` with `FlowColumn::{Date, Purpose, Route,
Amount, State, Note, Source, Activity}` filled from a `Posting`. U35, U37.

### 2.28 Another owner's money

"`X` belongs to `Y`, whose money this is not." is written in `register.rs:62,91`, `why/place.rs:28`, `why/asset.rs:17`,
`why/contract.rs:18`, and "is outside this owner's scope" in `register.rs:125`, `why/entity.rs:23`. One
`Lens::refuse(title, owner) -> Option<Report>`. U39.

### 2.29 A cell said as text

`report/src/json.rs` (`write_plain` 237, `plain_visible` 279, `starts_with_punctuation`, `write_period_words`,
`write_trigger_words`, `write_percent`, `StackText`) and `cli/src/table.rs` (`write_cell` 243, `cell_visible` 342,
`starts_with_punctuation`, `write_period`, `write_trigger`, `write_percent`, `StackText`) are the same function twice, about 170
lines. **Differences:** the CLI colours (an ink per cell, red for a negative amount, dim for a source) and pads a unit to the
column's width; JSON writes a `Count` without grouping. Both are real and are the sink's. **One formulation:**
`Cell::write_plain(&self, out: &mut impl CellSink, sources)` in `report`, with `CellSink: fmt::Write { fn ink(&mut self, Ink);
fn count(&mut self, n: usize) }`. U43.

### 2.30 Writing what the language reads

An amount in the language's digits is written by `sync/src/world.rs::money`, `session/src/transaction.rs::amount`,
`lower/loan_opening.rs::note` (`to_string().replace(',', "_")`) and the formatter. A flow line is written by
`sync/src/world.rs` (as text, then **read back by splitting on whitespace** in `moved_by`, 213-238) and
`session/src/transaction.rs::NewTransaction::line`. **One formulation:** `Amount::written(scale, unit)` once, and sync keeps
the typed line it writes (what each end moves) instead of reparsing its own text. U44.

### 2.31 Confining a write to the project

`cli/src/fmt.rs::ensure_inside`, `sync/src/apply.rs::contained`/`nearest_existing`, `sync/src/paths.rs::confined`: three
canonicalize-and-compare. One `paths::inside(root, path)`. U45.

### 2.32 The days a schedule falls due

`core::calendar::due` with `Landings` and `first_cadence_at_or_after` walks a cadence; `core::dues::Dues` counts and indexes the
same days (its `Walked` shape calls `due`); `report/forecast/recurrence.rs::Schedule::days` walks them a third time for habits.
`syntax::Cadence` is a copy of `core::Cadence`. **One formulation:** `Dues` is the one, `due` its private walker, the habit a
`Dues` (2.24). U46, U36.

### 2.33 Two derive hosts (after K6b)

K6's `engine/occurrence/derive.rs` derives flows and items when an occurrence is made; K6b's `engine/offspring.rs` derives flows
from a flow that has posted (a queue, a lineage, a cycle guard, `RuntimeTxn::Derived`). Both read `Effect::Derive` and a
`Derived` template. Whether they are one host depends on what K6b merges; the ledger holds −150 for the template reading they
share and decides after K6b. U34.

### 2.34 Code nothing calls

Found by reading and by listing every function no non-test code names: `core/src/trail.rs` (138, no user anywhere:
K5c's report says why it was not used), `sync/src/diff.rs` `Change::diff` (67; the CLI writes its own diff), `engine/assets.rs`
`nearest_acquisition` with `nearest_from` and `shift` (about 75, tests only), `engine/explain.rs::basis_shortfall`,
`engine/motion.rs::Motion::from_view`, `engine/eval.rs::Context::for_purpose`, `engine/assets.rs::into_states`,
`model/names.rs` `Rank::Alias` (K12b), `report/history.rs` `Change` (an enum of one variant), `engine/histories.rs`
`Extremes` (re-exported, read by nothing until U30 reads it). U33.

---

## 3. The ledger

One entry per unification. **Lines** are by reading: what goes is counted on the code at `25566a5` (the functions and types
named, with their signatures, call-site wrapping and the diagnostics they build); what is added is the new type, its reader and
the rows or methods it needs, written out in the same style as the code around it, then counted. Estimates are rounded to five
or ten (a file that goes whole is counted exactly); the error on an entry is about a fifth of its deletions, in both directions.
Columns: deleted / added / **net**, per crate.

A checkpoint is a run of green commits that merges on its own; its size is its net deletion, between one and four thousand
lines for each of the six (the lines it touches, deleted plus added, are in the table that closes this section). Every entry
keeps the output byte-identical unless it says otherwise, and the only entries that say otherwise are K7c's (decided,
Decisions 14).

### Checkpoint C1: one site, one tail, one property reader (`model`, `syntax`): −2,310

**U1. The lowering site.** *Goes:* the four parameters `world`, `home`, `file`, `diags` of 150 functions (930 signature
lines) and the argument lists that pass them on (about 260 wrapped call lines); `FlowCx`'s, `TermsCx`'s and
`Stated`'s copies of the file and the home with their `file()`/`home()` accessors; `laws::Compiler`'s own four. *Becomes:*
`Lowering<'w, 'a, 's>` (2.1, designed in section 8) and the passes as its methods. *Lines:* model −1,190 / +350 / **−840**.
*Behaviour:* none (no logic moves; the diagnostics are pushed to the same list in the same order). *Proof:* the Book dump
(section 6, to write) identical on every example, golden, fuzz and bench book; mistakes; `diff/`. *Risk:* low; the borrow of
`World` while a `Staged` mark is open is the one thing to get right, and the design gives `Lowering::staged` a closure for it.
*Needs:* L1 merged (the junction rewrites the header lowering this touches).

**U2. One tail.** *Goes:* the eight clause readers of 2.2 (`FlowCx::read`, `lower_term_tail`, `laws::compile::line::tail`,
`measure_tail`, `waiver_tail`, `ending_codes`, `ending_description`, `assertion_gap`'s loop) and the five result types (`Tail`
with its 35-line `merge`, `TermTail`, `Metadata`, `MeasureTail`, `WaiverTail`); the `unreachable!` arms; in `syntax`, the
statement's own list of the clauses it keeps (`statement.rs` `takes`). *Becomes:* `Says`, `Clauses` (one `const` per kind of line,
read by the parser too) and `Lowering::tail`. *Lines:* model −420 / +170 / **−250**; syntax −40 / +20 / **−20**.
*Behaviour:* none: the refusals keep their codes and words as the `Refusal` of each `Clauses` row (`until-position`, `also-tail`,
`also-relative-due`, `computed-also-basis`, `measure-tail`, `statement-lowering`); a term tail keeps reading nothing it did
not read ("left unread" is the `TERM` row's `Refusal::Unread`). *Proof:* Book dump; mistakes; `derives.py`; the parser's tests.
*Risk:* low. *Needs:* U1.

**U3. A written amount and quantity.** *Goes:* `stated_amount`, `resolve_amount`, `template_amount`, `derived_amount`,
`assertion_amount`, `basis_cost`'s amount half, `implied_amount`; `resolve_quantity`, `template_quantity`; the three
`ast::Sign` → `split::Sign` maps, `written_cadence`, `syntax::Cadence` (a copy of `core::Cadence`) and the model's copies of
`Sign` and `Cadence`. *Becomes:* `Lowering::amount`, `Lowering::quantity`, `Unsaid` (2.3); the model reads the syntax's enums.
*Lines:* model −330 / +150 / **−180**; syntax −45 / +25 / **−20**. *Behaviour:* none: the journal's flow still drops a missing
root's diagnostic (`Unsaid::Missing` dropped by that one caller), the template still reads a bare percentage as a share (the
`fallback` the template passes). *Proof:* Book dump; `splits.py`; `contracts.py`; `loans.py`. *Risk:* low. *Needs:* U1.

**U4. One reader of a property line.** *Goes:* the contract property readers of 2.4 (`loan_fields`, `loan_principal`,
`loan_asset`, `loan_prepay`, `loan_resets`, `reset_schedule`, `reset_index`, `reset_limits`, `reset_keyword`, `is_percent`,
`percent_ratio`, `invalid_reset`, `loan_term_error`, `contract_days`, `span_property`, `grace_property`, `has_property`,
`relative_property`, `coverage_property`, `escalation_property`, `contract_area`'s reading, `contract_deposit`/
`deposit_amount`/`holding_name`'s reading, `read_share_line`, `share_rate`): about 650 code lines and 330 of hand-built
diagnostics; `lower.rs::inputs` (79); `lower_rate_change`'s reading; `props.rs`'s `is_builtin_line` and the parts of `Args` the
signature reader replaces (`next_id`, `wrong`, `arg`, `done`); `sync_lower.rs`'s `FormatReader::read_line`/`read_field`/
`column`/`read_spec` and `source_format`/`source_sink`/`source_feed`'s argument reading. *Becomes:* `Signature`, `Slot`, `Want`,
`Said`, `Check` and one reader returning `Args` (2.4, designed in section 8); a row per line (about 40 rows: the 25 built-ins, 15
contract, input, rate, format and source lines); what a line *means* stays code (a lender who is not the borrower, a deposit's holding,
shares at most 100%, a measured share's area). `props.rs` splits into the three modules K12b asks for as a consequence (the grammar is
the rows). *Lines:* model −1,390 / +520 / **−870**. *Behaviour:* none: every message and label is a `Said` column copied from
today's code, and the order in which a line's errors are found is the row's order (section 8 shows the loan, whose shape error
comes after its principal's, and the reset, whose words are each said). *Proof:* the declared-lines corpus (section 6, to
write: one book per error path of every property line, about 120, run against the baseline binary before U4 starts);
`contracts.py`, `loans.py`; model tests (codes). *Risk:* medium: the corpus is the only thing that pins these messages, so it is
written and run on the baseline first. *Needs:* U1, U3. With generic wording (question 3, lever L-d) the `Said` columns go:
−1,390 / +430 / −960.

**U5. Filling a slot.** *Goes:* `relator.rs`'s `fillers`, `fits`, `Misfit`, `unfilled`, `many` and its range check. *Becomes:*
`fill::fill` with the target's vocabulary (`Filling::Thing` or `Filling::Role`: the codes and words each says). *Lines:* model
−170 / +120 / **−50**. *Behaviour:* none (the `relator-slot-*` codes and words are the `Role` vocabulary). *Proof:*
`relators.py`; `model/tests/relator.rs`; the declared-lines corpus. *Risk:* low. *Needs:* U4. Generic wording: −100.

**U6. One walk.** *Goes:* the walks of 2.6 (`declare/mentions.rs`'s, `laws::counted`, `budget::Found::read`,
`slots::values_written`, `named_by_openings`/`originations`, `declare::scopes`, `sync_lower::declare`'s). *Becomes:* `Collected`
lists filled in its one pass. *Lines:* model −140 / +60 / **−80**. *Behaviour:* none (each list keeps source order, which is the
walk's). *Proof:* Book dump; mistakes (the first-mention order of an implied party is what `mentions` decides). *Risk:* low.
*Needs:* nothing.

### Checkpoint C2: one lowering of a line that moves value (`model`): −1,450

**U7. One group lowering.** *Goes:* the per-line copies of the skeleton in 2.7: `lower_txn`, `lower_flows`,
`lower_named_flow`, `lower_split_flow` with `Split`/`SplitEnd`/`SplitLegs`/`MadeLeg`, `make_flow`, `make_resolved_flow`,
`lower_items`; `lower_opening*`; `lower_occurrence` (403) with `OccurrenceGroupDraft`, `occurrence_item_ends`,
`merge_detail_pool`, `template_side`; `lower_loan_origin` (146); `lower_owes`, `claim_ends`, `claim_program`;
`lower_terms`, `template_header`, `template_legs`, `TermsCx::flow`, `lower_header_item`, `split_loan_payment`'s construction;
`read_line`, `implied_flow`, `implied_end`, `implied_item`; `lower_basis`/`arrival_flow`; `push_opening`'s flow; five `Flow`
literals of twenty fields. About 1,750 lines of the table of 2.7 (all but the matching of a written occurrence to its
template, which U28 takes) and 130 of literals. *Becomes:* `Against`, `FlowDraft`, a
`Header` trait with one impl per kind of header (the part of each copy that is real: who the ends are and what a quantity may
be), `Lowering::group` and `Lowering::keep`. *Lines:* model −1,880 / +750 / **−1,130**. *Behaviour:* none. The order of the
flows, items and diagnostics of a group is the written order in every copy today, and stays it; a written occurrence's lines
are lowered by `group` like any other, and its matching against the template stays as it is until U28, so the Book holds
the same occurrence in the same arenas. *Proof:* Book dump on every book we have (the occurrence's arenas are the place
it could differ); mistakes; `splits.py`, `contracts.py`, `loans.py`, `derives.py`, `claims.py`; L1's `fmt --upgrade` outputs.
*Risk:* **high**, the largest single change of the plan: it lands in seven commits, one kind of line at a time onto
`Lowering::group` (journal, opening, owes, basis, loan origination, template and derive, occurrence last), each green, and the
copies go only in the last. *Needs:* U1, U2, U3; L1 merged.

**U8. A statement's subject.** *Goes:* `statement_target`, `named_target`, `end_target`, `basis_asset`, `claim_target` and
their precedence code. *Becomes:* one `Subject` resolver returning what the word could be, from which each verb picks with
its own precedence (a value: asset, a loan's debt, an end; an ending: contract, asset, end). *Lines:* model −190 / +80 /
**−110**. *Behaviour:* none (the precedences are data now). *Proof:* Book dump; mistakes; `loans.py` (a value on a loan).
*Risk:* low. *Needs:* U1, U11 if it lands first (either order works).

**U9. A statement verb as a row.** *Goes:* `lower_statement`'s dispatch and the opening each verb writes by hand (match the
subject, else `unsupported_statement` with its own message, resolve, push): `lower_event`, `lower_split`, `lower_filed`,
`assert_value`, `lower_reading`, `lower_quote`, the waiver, ending, measure, claim-change and write-off openings;
`unsupported_computed_value`. *Becomes:* a `VERBS` table (the verb, the subject sort it takes, the refusal's words, the
function that lowers what is particular to it). *Lines:* model −230 / +120 / **−110**. *Behaviour:* none. *Proof:* Book dump;
mistakes. *Risk:* low. *Needs:* U8.

**U10. A contract, lowered once.** *Goes:* `lower.rs`'s second walk of a contract's body for its roots
(`contract_roots`, `push_payment_roots`, `changed_term_roots`, `push_body_roots`, `push_amount_root`, `compile_roots`: about
80 lines, which U7's group compiles as it meets them); `empty_contract`'s literal (`Contract::new` and `Default`);
`resolve_commodity`/`resolve_endpoint`/`schedule_owner`/`node_doc` (wrappers of the site's lookups); the re-finding of what the
first pass found in `lower_contract`. *Becomes:* `Lowering::contract`, reading its lines by signature (U4) and its terms as
groups (U7). *Lines:* model −220 / +120 / **−100**. *Behaviour:* none (the roots are compiled in the same order: the order the
body is written in). *Proof:* Book dump (the node arena's order is the thing to watch); `contracts.py`. *Risk:* medium (node
order); the Book dump compares node arenas by structure, not by index, if the order turns out to differ harmlessly, and that is
said. *Needs:* U4, U7.

### Checkpoint C3: names, laws, values and diagnostics (`model`, `report`): −1,190

**U11. Names to ids.** *Goes:* the per-namespace `seek_X`/`X`/`ambiguous_X` triples of `resolve.rs` (kinds, entities,
purposes, places, params, commodities, systems), `Book::place`/`Book::entity`'s own fallbacks, `World::end_on`'s and
`World::address_end`'s two readings and the `settle_addresses` memo with the `#[inline(always)]` it needs (K12b), the four
suffix enumerations. *Becomes:* `Named`, `World::seek`/`find`/`meaning`, `paths::suffixes` (2.9); K12b's one definition of
"spelled" read by `declare/parties.rs::Addressed` and `Book::is_spelled`. *Lines:* model −420 / +110 / **−310**.
*Behaviour:* none; did-you-mean candidates are ranked by the one function each triple calls today. *Proof:* mistakes (the
suggestions); `addresses.py`, `family_addresses.py`; Book dump; `bench/` (the memo goes only if `check` is not slower; if it
is, it stays and the entry is −250). *Risk:* medium (speed). *Needs:* U1.

**U12. Triggers.** *Goes:* four of the seven matches of `law::Trigger` in `model` (`compile.rs`'s, `vars.rs`'s `When::of` and
`phrase`, `order.rs::occasion`, `laws/mod.rs::fits`). *Becomes:* `TRIGGERS` and `Trigger::{words, occasion, fits}`; the report's
and CLI's three read `words` in U40. *Lines:* model −150 / +90 / **−60**. *Behaviour:* none. *Proof:* mistakes (the
`fits` messages); goldens. *Risk:* low. *Needs:* nothing.

**U13. The law compiler's duplicates.** *Goes:* 2.12's list: three `Compiler` literals, `owner_amount_ty`/`value_amount_ty`,
three parses of `month`/`year`/`ever`, three lists of the built-in fields (`field`, `unknown_field`, `slots.rs::FIELD_WORDS`),
four hand-synthesized laws, `budget.rs::offset_op` and the budget formula's own arena, `register`'s four bucketings.
*Becomes:* `Compiler::new(lowering, owner, when)`, one `FIELDS` table, `Window::parse`, a `Nodes` builder, the budget compiled
into its law's arena, `Groups::build`. *Lines:* model −330 / +105 / **−225**. *Behaviour:* none (node order in a synthesized
law is the builder's, which is the order the hand-written code pushed). *Proof:* Book dump (laws); goldens with laws
(`tax`, `limits`); mistakes. *Risk:* low. *Needs:* U1.

**U14. The value of an amount in another commodity.** *Goes:* `Prices::rate`/`direct`/`latest` and `book.rs`'s
`conversion_path`/`spot_rate_use`/`latest_quote`/`param_rate_use` as two lookups; `Lens::exact`'s own rate. *Becomes:*
`Book::rate(from, to, day, policy) -> Option<RateUse>`; `convert` and `Lens::exact` ask it and keep their own rounding.
*Lines:* model −150 / +60 / **−90**; report −25 / +10 / **−15**. *Behaviour:* none (traced to the same rate on every path,
including the inverse and via-base fallbacks). *Proof:* goldens with `--value`; `diff/`; fuzz. *Risk:* low. *Needs:*
nothing.

**U15. Smaller copies in properties and trees.** *Goes:* `unknown_property` beside `unknown_native_property`;
`native_system_currencies` beside `system_rates` (the same once-per-system loop); `diagnose_asset_cycles`'s own parent-pointer
cycle finder beside the taxonomy's. *Becomes:* one of each; cycles through `core::tree`. *Lines:* model −150 / +65 / **−85**.
*Behaviour:* none. *Proof:* mistakes (cycles, unknown properties). *Risk:* low. *Needs:* U4.

**U16. Diagnostics with fixed wording as data.** *Goes:* the builder chain at each site whose wording has no argument (after
U4 and U7 have deleted the sites they own, about 350 remain in `model`). *Becomes:* `const` `Problem` rows in `problem.rs`
said with `lowering.say(ROW, at)`; a diagnostic with arguments stays a function. *Lines:* model −600 / +350 / **−250**.
*Behaviour:* none (the rows are the strings, moved). *Proof:* mistakes; `diff/cases`; the declared-lines corpus. *Risk:* low.
*Needs:* U1, U4, U7.

**U17. What a flow says, carried whole; the Book's empty arenas.** *Goes:* `split::Says`, the metadata half of `book::Derived`
and the field-by-field copies into them; `declare.rs::book`'s thirty empty arenas written out. *Becomes:* `Says` (U2) carried
whole; `..Book::default()`. *Lines:* model −220 / +70 / **−150**. *Behaviour:* none. *Proof:* Book dump. *Risk:* low.
*Needs:* U2.

**U18. K12b's small items.** *Goes and becomes:* `Book.sites`'s tuple key named; one reader style where the schema proves
the type; the facts frozen once if the order allows (or the module doc says why not); `shortest_that` bounded; `name_claims`'s
`bool` and the three hand-built `Request`s (a constructor per use). *Adds:* the weights of a membership stored (K6 needs them:
K12b's one addition, +30). *Lines:* model −35 / +30 / **−5**. *Behaviour:* none. K12b also names two items that would change
what a book says (`owner` written without a range, as in `acme/529`; `unknown-address` suggesting the closest name rather
than the closest address): U does not change them, and lists them for lane D or a decision (question 11). `core`'s eight
`clippy` errors go in the first commit of phase 1, so that `core` is held to `clippy -D warnings` from then on. *Proof:* tests;
mistakes. *Risk:* low. *Needs:* U11.

### Checkpoint C4: where value rests and how it leaves (`engine`, K3e): −1,295

K3f (a debt is a parcel, Decisions 13) merges before C4 starts, so the one relief below serves claims, debts and positions.

**U19. One relief, a ranking (K3e item 3).** *Goes:* `relieve_in_order`, `relieve_scanning`, `take_plain`, `take_exact`,
`take_run`, `take_priciest`, `take_dearest`, `gather`, `whole_claims`, `by_policy`, `basis_per_unit`, `interchangeable`,
`allocate`, with `Colour`, `Candidate`, `Ranked`, `Shares`, `Selection`. *Becomes:* `rank(Policy, &Request) -> impl Ord` over a
parcel's hot fields, `take(ranked, need, Share)`, and a table of the policies (rank key, how a tie shares). The cursor, the
heap and the sweep stay as the ranking's implementation for FIFO, LIFO and HIFO **if** K3e's benchmark at 100k parcels says the
plain ranking cannot match them (then −120 instead of −220). *Lines:* engine −560 / +340 / **−220**. *Behaviour:* none; the
order of `exact` (the parcels of one transaction adding up, else the oldest) is byte-exact. *Proof:* `claims.py` (1,500 books
and its mutants), `splits.py`, the K3e model-based test (to write: a naive `Vec<(qty, basis, key)>` per policy on seeded random
landings and reliefs, with selectors, ties and merges; mutants of the rank and the take must fail it), `lots.rs`'s unit tests
moved; a sale-heavy book (100k lots, 50k sales) and a prorata book (to write, with `bench/gen.py`); `bench/` 100k, 1m.
*Risk:* medium (speed, and the order of `exact`). *Needs:* K3f merged.

**U20. Identity as a key (K3e item 2).** *Goes:* `Identity`'s hand-written `PartialEq` and `Hash`, `identity(parcel, money)`
recomputed at each landing and compared structurally, the `money: bool`. *Becomes:* a key hashed once at landing; merging is an
integer compare, a collision checked once against the parcel's fields. *Lines:* engine −110 / +70 / **−40**. *Behaviour:* none.
*Proof:* as U19; a test that two parcels with colliding keys and different fields never merge. *Risk:* low. *Needs:* U19.

**U21. An asset part's basis is its parcels' (K3e item 4, K3c §4's smaller cut).** *Goes:* `Part.basis` and every write to it;
`prepare_consumption`/`ConsumptionGuard`, `prepare_carry`/`CarryGuard`, `prepare_basis_additions`/`AssetBasisBatchGuard`
(`assets.rs`), `prepare_part_basis_adjustment`/`PartBasisAdjustment`, `prepare_part_carry_additions`/`CarryLotBatchAdjustment`
(`lots.rs` 963-1337), `AssetPartAddition` and the lockstep hooks (`assets_runtime.rs`), `PendingCarry`/`part_slots`, the eight
`ParcelBasisMismatch` checks, three `Part` builders with three `asset-cost` diagnostics (`post.rs`), most of `fire.rs::carry`
(152) and `match_pending_carries` (67). *Becomes:* 2.16's formulation: the parcels are the one store of basis,
`Holdings::adjust(part, delta, Weight)` checks before it writes, a part keeps its own `consumed` and `carried` sums for what
depreciation per part reads, one `Part` constructor. *Lines:* engine −1,150 / +240 / **−910**. *Behaviour:* none on any book
we have: the `asset-state` diagnostics the guards raise when the two stores disagree become unrepresentable, and no golden,
mistake book or test raises one (checked). *Proof:* goldens with assets (`why ASSET`, `gains`, `lots`, depreciation in
`tax`); `claims.py` (parts are on its corpus); a property test that `part_basis(anchor)` equals the old `Part.basis` on every
step of 1,500 generated books, run on a branch that still has both before the old one is deleted. *Risk:* medium; the
property test on the transition branch is the guard. *Needs:* U19, U20.

**U22. Realizing what leaves, once.** *Goes:* the second construction of `Gain`, `Realized` and the `on gain` occasion in
`dispose_sold_asset`; the `Parcel` literals in `arrive`, `add_acquisition_part`, `rebase`. *Becomes:* one
`realize(slices, from, to, unit, proceeds)`; `Parcel::landed(...)`. *Lines:* engine −110 / +45 / **−65**. *Behaviour:* none.
*Proof:* goldens (`gains`); `claims.py`. *Risk:* low. *Needs:* U21.

**U23. What a flow settled, recorded once; a write-off's reversal, read (K12b, K3d's leftovers).** *Goes:* `Record::settled`
beside `Record::settlements` and `Frame::settled`; the report's `flow.rs::forgiven_by` and `Counting::forgiving`, which work out
again what `claims.rs::take_back` did in the fold. *Becomes:* a settlement recorded once on the flow (`Posted` carries it); `Run`
carries what was taken back and the report reads it; a flow with no claim place at either end skips the claim rule at the
call site (K12b's 1.3%). *Lines:* engine −60 / +40 / **−20**; report −55 / +15 / **−40**. *Behaviour:* none (the three
records disagree today on one kind of flow, a flow out of a claim place; K3d's map says which reading the output uses, and that
one is kept). *Proof:* `claims.py`; goldens (`claims`, `flow`). *Risk:* low. *Needs:* K3f.

### Checkpoint C5: flows, records, streams and series (`engine`, `model`, K4c's list): −1,755

**U24. A flow as read: `FlowRef` (K4c item 1, its reader shape only).** *Goes:* `FlowView`, the view half of `RuntimeFlow`,
the fields `Motion` copies from the view, the field-by-field reads of `book.flows[id]` that take a flow apart. *Becomes:*
`FlowRef<'_>` (`Copy`, borrows, accessors by name) from `Flows::get(id)` over today's `Vec<Flow>`; `Motion` keeps only what the
fold adds (cause, day, solved amounts, orientation, ordinal). K4c's columns can later change what is behind the accessors
without touching a reader. *Lines:* engine −150 / +80 / **−70**; model −90 / +50 / **−40**. *Behaviour:* none. *Proof:* fuzz
`diff`; `splits.py`; `bench/` (a `FlowRef` must not cost the fold: `check` at 1m not slower). *Risk:* low. *Needs:* C2 (the
flows are built in one place).

**U25. K4b's list (K4c item 4).** *Goes:* the second `put` (`balance::put` and `statement.rs::solve_group` each write the
solver's answer back), the second `Env` (`statement.rs::Reads` and `occurrence.rs::Reads`, with different `lands`), exchange legs
recognised by a flow's shape (`is_exchange`) in `Statement::asked`, `balance::moved` and the fold, `exchange_costs`'s tuple and
`is_exchange_cost`'s index arithmetic, `amount_of`'s five parameters. *Becomes:* one `put` through the fold's resolved map; one
`Env` with `lands` a value; the group *says* which legs exchange (a `Draw` field the lowering sets once, from the units);
`exchange_costs` and `amount_of` over `FlowRef`. *Lines:* engine −250 / +110 / **−140**; model −80 / +40 / **−40**.
*Behaviour:* none. *Proof:* `splits.py`; the K5a oracle; fuzz `diff`. *Risk:* low. *Needs:* U24, U7.

**U26. The records of a fold.** *Goes:* `Recorded` (the lists borrowed), `Applied` (the lists as ranges), `state::Record`'s
field-by-field `marks`/`since`/`finish`. *Becomes:* `Records` with `view()`, `marks()`/`since()` over one array of lengths, and
`Run { records, … }`. *Lines:* engine −200 / +80 / **−120**. *Behaviour:* none. *Proof:* tests; goldens; session scripts (they
read `Run`). *Risk:* low. *Needs:* U23.

**U27. Streams of due days (K12b, K5c's leftover).** *Goes:* the two heaps of `Residual` streams in `monitor.rs` (`Waiting`) and
`promising.rs` (`Ahead`) with their two start loops; `Ledger::promise_through`, an API for one reader. *Becomes:* `Streams<R>`,
a heap of `(next due, stream)`. *Lines:* engine −160 / +90 / **−70**. *Behaviour:* none (ties between streams on one day keep
today's order: each heap's key becomes the `Streams` key, unchanged). *Proof:* goldens (`contracts`, `forecast`); `forecast.py`;
`docs/v5/measure/promises/`. *Risk:* low. *Needs:* nothing in U.

**U28. A written occurrence and its template, merged once.** *Goes:* the model's matching of a written occurrence's legs to
its template's (by ends, with the override's amount and price; the part of `lower_occurrence` U7 left), and the engine's
re-reading of what the model matched (`ledger.rs::post_written_occurrence`, 121 lines; `occurrence.rs`'s `legs_at`, `items_at`
and their ordinal arithmetic over the model's pre-matched layout). *Becomes:* the Book holds a written occurrence as the group
U7 lowered; `materialize(template, written)` in the engine is the one merge. *Lines:* model −100; engine −190 / +110 / **−80**:
**−180**. *Behaviour:* none at the run: the Book's written occurrence changes shape (the dump shows it, and says so), the flows
it materializes do not. *Proof:* `docs/v5/measure/internals/` (the materialized dump, identical); goldens (`07-landlord` and
every example with a written occurrence); `contracts.py`; fuzz `diff`. *Risk:* medium (the ordinal of a derived flow is what
`why` and the codes index by). *Needs:* U7. Model and engine change in one commit.

**U29. One recognized series.** *Goes:* `Windows` (rolling month, year, closed year, ever and the `Reaching` heap of accruals
ahead) beside `History` (block prefix sums with a slow path); `Flowed`, `DayFact`, `BlockPrefix` (three `{incoming, outgoing}`
pairs each with `side`/`side_mut`); `record`/`record_slot`'s two pushes. *Becomes:* one series per key (prefix blocks with a
cursor for the current month and year, so the hot path stays constant time) and `[Qty; 2]` indexed by `Dir`. *Lines:* engine
−330 / +130 / **−200**. *Behaviour:* none. *Proof:* goldens with laws (`tax`, `limits`, `budget`); a model test (to write)
against naive window sums on random books; `bench/` (this is the fold's hot path: `check` at 1m not slower, three runs).
*Risk:* **medium-high** (speed); if the cursor cannot match `Windows` at 1m, `Windows` stays as the series' cache and the
entry is −90. *Needs:* nothing in U.

**U30. A balance over time reads the run's histories.** *Goes:* `temporal.rs` (`TemporalHistory`), `ledger.rs`'s
`sample_temporal*`, `plan.rs`'s `temporal_queries`/`change_dates` for roots that are balances, `eval.rs`'s separate
`extreme`/`day_count` paths for them. *Becomes:* `peak`/`low`/`days` over a balance read `Steps::extremes` and
`Steps::days_where` (`histories.rs`, built by K7b and read by nothing); other roots keep the sampler. *Lines:* engine −260 / +60 /
**−200**. *Behaviour:* none. `value(balance, UNIT)` changes on a quote's day as well as a flow's: it reads the steps merged with
the quote days, and if the oracle finds a day that differs it keeps the sampler (−120). *Proof:* a temporal oracle (to write:
evaluate every `peak`/`low`/`days` day by day on random books and compare); goldens with residence laws. *Risk:* medium. *Needs:*
nothing in U.

**U31. Places within a subject.** *Goes:* `plan.rs`'s `entity_places`, `asset_places`, `kind_places` and the use of
`book.places.covers` for places; `totals.rs::places_within` (the inverse). *Becomes:* one `Within` index (subject to places,
place to subjects). *Lines:* engine −120 / +50 / **−70**. *Behaviour:* none. *Proof:* goldens with laws; fuzz `diff`.
*Risk:* low. *Needs:* nothing.

**U32. A loan's disagreement as data (K12b, K5d's leftover).** *Goes:* `loan_balance.rs`'s prose built in the engine
(191 lines, "prose in code"), and `Cause::Short` papering over the difference between the book's payment and the schedule's.
*Becomes:* the structured disagreement in `Run` (statement, schedule, day, the candidate causes); the report words it, with
today's words. *Lines:* engine −230 / +40 / **−190**; report +50. *Behaviour:* none in the output. `loans.py` reads the
structure through `docs/v5/measure/loans/` instead of parsing a note. *Proof:* `loans.py` and its mutants; goldens with loans.
*Risk:* low. *Needs:* U26.

**U33. Code nothing calls.** *Goes:* 2.34's list: `core/src/trail.rs` (138), `sync/src/diff.rs` (67), `nearest_acquisition`/
`nearest_from`/`shift`, `basis_shortfall`, `Motion::from_view`, `Context::for_purpose`, `into_states`, `Rank::Alias`,
`report/history.rs`'s one-variant `Change` (into U37). (K12b's `ConsumptionGuard.before` goes with its guard in U21.) `Extremes`
stays: U30 reads it. *Lines:* engine −120, core −138, sync −67, model −10: **−335**. *Behaviour:* none. *Proof:* it builds;
tests that tested only the dead code go with it, and are listed. *Risk:* none to features; `trail` was offered to K4c's `Staged`
and K5c declined it, so if K4c's columns want it later it comes back from history. *Needs:* nothing.

**U34. Two derive hosts (after K6b).** *Goes:* the template reading `engine/occurrence/derive.rs` and K6b's
`engine/offspring.rs` both do for `Effect::Derive` and `Derived`. *Becomes:* one, decided on K6b's merged code. *Lines:* engine
−220 / +70 / **−150**, held until K6b's map says what it left. *Behaviour:* none. *Proof:* `derives.py`; K6b's own oracle.
*Risk:* the estimate (K6b is running). *Needs:* K6b merged.

### Checkpoint C6: the views and the outer crates (`report`, `cli`, `sync`, `core`, `session`, `syntax`): −1,925

**U35. K7c's six: STATUS's U1 to U6 (decided, Decisions 14).** *Goes and becomes:* K7c-1 `balance --value` without the "N flows
have no price" note (−45); K7c-2 the entity, asset and contract registers as the place register's columns (−130); K7c-3 `why
asset:` and `why contract:` through the shared flows table (−45); K7c-4 `why #purpose`'s limits and budgets through the limits
and budget rows (−60); K7c-5 `flow --by party` as the periods table (−40); K7c-6 one `why` layout. *Lines:* report −420 / +100 /
**−320**. *Behaviour:* **changes the bytes of the views named, as decided**; three goldens move with K7c-6, each listed with the
reason in the commit. *Proof:* every other golden and command byte-identical (`session/allcmds.sh` over the examples);
`whys.py`. *Risk:* low. *Needs:* nothing.

**U36. The habit forecast is a source of flows (Decisions 11).** *Goes:* `recurrence.rs`'s `Schedule` and its day walker,
`trace.rs::apply_habit` and its plumbing, `expected.rs`'s projection of an `Expectation` into flows, the forecast's habit rows
built beside the promise rows. *Becomes:* a habit compiled to a `promise::Stream` (a cadence, a day, a template flow) posted
through K5c's interface (`Ledger::promise`) like a contract's occurrence; "What recurs" reads the streams; the bands keep their
draws. *Lines:* report −420 / +140 / **−280**. *Behaviour:* none: the month-end positions, "What recurs" and the p10/p50/p90
bands are the same numbers (a habit's flow keeps its place among the contract flows of its day, the place `apply_habit`
gives it now, through the stream's key; the bands draw in the same order). *Proof:* `forecast.py`; goldens with `forecast`; `docs/v5/measure/forecasts/`. *Risk:* medium
(the order on a shared day). *Needs:* U27, U46.

**U37. Rows of flows.** *Goes:* `why.rs::flows_table`, `why/asset.rs::about`, `why/contract.rs::derived_section`,
`why/text.rs`'s table and the registers' row building (after K7c-2 and K7c-3 have made the registers one). *Becomes:*
`FlowTable { columns: &[FlowColumn] }` filled from a `Posting`. *Lines:* report −300 / +120 / **−180**. *Behaviour:* none beyond
U35's. *Proof:* goldens; `whys.py`; `report_mutants.py`. *Risk:* low. *Needs:* U35, U24.

**U38. The view site.** *Goes:* `run: &Run` beside `lens` in 75 report signatures and at 66 call sites, and `Lens`'s four
lifetimes written out at each. *Becomes:* `View<'v, 's>` (the plan, whose, the day and the run), the report's analogue of U1;
the views and their helpers are its methods. *Lines:* report −260 / +110 / **−150**. *Behaviour:* none. *Proof:* goldens;
session scripts. *Risk:* low. *Needs:* nothing.

**U39. Another owner's money; a typed target.** *Goes:* "`X` belongs to `Y`, whose money this is not." written five times and
"is outside this owner's scope" twice (2.28); `register.rs`'s own resolution of `contract:`/`asset:`/`entity:` beside
`why::Target`'s. *Becomes:* `View::refuse(title, owner) -> Option<Report>`; one target resolution. *Lines:* report −110 / +30 /
**−80**. *Behaviour:* none. *Proof:* goldens; `whys.py`. *Risk:* low. *Needs:* U38.

**U40. Enums in words; which thing.** *Goes:* the 21 word functions and the inline matches of 2.25 (`State` three times,
`PurposeRoot`, `Cadence` three times, `EventState`, `Provenance`, `Action`, `Sign`, `FlowSide`, weekdays) and the per-arm name,
location and owner lookups of 2.26. *Becomes:* `trait Words` (as `Policy::WORDS` already is), read by the parser, the model and
the report; `Thing` with `Book::name_of`/`loc_of`/`owner_of`. *Lines:* report −200 / +90 / **−110**; model −70 / +30 / **−40**.
*Behaviour:* none. *Proof:* goldens; mistakes. *Risk:* low. *Needs:* U12.

**U41. Periods tables through the pivot.** *Goes:* `flow.rs`'s hand-built income and spending totals per period (54), its
measures grid (369-412) and the period column headings written twice (138, 384); `register.rs::measure_section` (a pivot by
hand). *Becomes:* `Pivot` with a measure key. *Lines:* report −110 / +50 / **−60**. *Behaviour:* none. *Proof:* goldens
(`flow`); session scripts. *Risk:* low. *Needs:* U35 (K7c-5 touches the same table).

**U42. What has no price.** *Goes:* each valuing view counting its unpriced amounts by hand and wording its note (60 lines name
`unpriced` in `available`, `balance`, `claims`, `lots`, `pivot`, `lens`). *Becomes:* a tally the `View` keeps as it values, and
`Section::unpriced(noun)` with each view's words as data. *Lines:* report −90 / +30 / **−60**. *Behaviour:* none. *Proof:*
goldens with `--value`; `diff/`. *Risk:* low. *Needs:* U38.

**U43. A cell said as text.** *Goes:* the CLI's copy of the report's cell writer (`write_cell`, `cell_visible`,
`starts_with_punctuation`, `write_period`, `write_trigger`, `write_percent`, `StackText`). *Becomes:*
`Cell::write_plain(&self, out: &mut impl CellSink, sources)` in `report`; the terminal's sink inks and pads, the JSON sink writes a
count bare. *Lines:* cli −200 / +30 / **−170**; report +20. *Behaviour:* none. *Proof:* goldens (text); the JSON commands of
`diff/` and `session/allcmds.sh`; `climutate.py`. *Risk:* low. *Needs:* U40.

**U44. Writing what the language reads.** *Goes:* four writers of an amount in the language's digits (`sync/world.rs::money`,
`session/transaction.rs::amount`, `loan_opening.rs::note`, the formatter's) and sync reading back its own flow line by
splitting on whitespace (`world.rs::moved_by`). *Becomes:* `Amount::written(scale, unit)`; sync keeps the typed line it writes.
*Lines:* sync −110 / +30 / **−80**; session −15; model −5; core +10. *Behaviour:* none. *Proof:* sync's tests; session
scripts; `fmt` goldens. *Risk:* low. *Needs:* nothing.

**U45. Confining a write to the project.** *Goes:* `fmt.rs::ensure_inside`, `apply.rs::contained`/`nearest_existing`,
`paths.rs::confined`. *Becomes:* `paths::inside(root, path)`. *Lines:* sync −40 / +15 / **−25**; cli −25 / +10 / **−15**.
*Behaviour:* none (the symlink refusals keep their tests). *Proof:* `sync_refuses_to_write_through_a_symlinked_parent` and
the `fmt` tests. *Risk:* low (security-relevant: each refusal keeps a test). *Needs:* nothing.

**U46. The days a schedule falls due; values grouped by a key.** *Goes:* `calendar::due` with `Landings` and
`first_cadence_at_or_after` beside `Dues`; `Groups::build` beside `groups::bucket`. *Becomes:* `Dues` the one walker; one
grouping. `DateLayout` (130 lines, read only by `sync`) moves to `sync`: net zero, and counted only so the per-crate table is
true. *Lines:* core −250 / +45 / **−205**; sync +130. *Behaviour:* none. *Proof:* `core`'s tests; goldens with contracts;
`forecast.py`. *Risk:* low. *Needs:* nothing.

**U47. Sync's memos read by the record reader.** *Goes:* `read_memos`, `read_row_memos`, `header_slot`,
`first_row_is_a_record`, `take_row_memo`, `read_tagged_memos`, `take_tagged_memo` (`format.rs` 440-627), which repeat the record
reader's walk (`rows`, `in_header`, `is_a_record`, `tagged`) to collect one field. *Becomes:* the reader with a plan for the
memo field only. *Lines:* sync −200 / +70 / **−130**. *Behaviour:* none: the memo reading still stops at its first problem (a
`Harvest` that stops is a value of the reader), and its diagnostics keep their words. *Proof:* sync's tests;
`check_reads_local_memos_and_never_runs_declared_commands`; the CLI's memo goldens. *Risk:* low. *Needs:* nothing.

**U48. The CLI's copy of sync's memo groups.** *Goes:* `MemoSuggestion` and `of_unrecognized`, a field-by-field copy of
`axiom_sync::unrecognized`'s groups. *Becomes:* the CLI writes sync's groups. *Lines:* cli −60 / +20 / **−40**. *Behaviour:*
none. *Proof:* the CLI's tests; the memo goldens. *Risk:* low. *Needs:* U47.

**U49. The syntax's diagnostics as data.** *Goes:* the builder chains of the syntax's 79 diagnostic sites whose wording is
fixed, and the words of the first-word dispatch written beside each production. *Becomes:* `const` rows (PROPOSAL §7's "a
diagnostic catalog in syntax as well", costed there at none). *Lines:* syntax −300 / +180 / **−120**. *Behaviour:* none.
*Proof:* mistakes (the parser's whole corpus); `diff/cases`. *Risk:* low. *Needs:* L1 merged, U16 (the row type).

### The ledger in one table

| checkpoint | entries | deleted | added | **net** | touched | by crate |
|---|---|---:|---:|---:|---:|---|
| C1 site, tail, property reader | U1-U6 | 3,725 | 1,415 | **−2,310** | 5,140 | model −2,270, syntax −40 |
| C2 one group lowering | U7-U10 | 2,520 | 1,070 | **−1,450** | 3,590 | model −1,450 |
| C3 names, laws, values, diagnostics | U11-U18 | 2,080 | 890 | **−1,190** | 2,970 | model −1,175, report −15 |
| C4 relief and parts (K3e) | U19-U23 | 2,045 | 750 | **−1,295** | 2,795 | engine −1,255, report −40 |
| C5 flows, records, streams, series | U24-U34 | 2,715 | 960 | **−1,755** | 3,675 | engine −1,410, model −190, core −138, sync −67, report +50 |
| C6 views and the outer crates | U35-U49 | 3,200 | 1,275 | **−1,925** | 4,475 | report −1,220, cli −225, core −195, syntax −120, sync −105, model −45, session −15 |
| **all** | 49 | **16,285** | **6,360** | **−9,925** | 25,645 | |

The six biggest entries: U7 −1,130, U21 −910, U4 −870, U1 −840, U35 −320, U11 −310. Half the net is in model (−5,130), which
is where the reading found the most copies of one thing.

---

## 4. The budget

### 4.1 Per crate

Counted truly (1.1). "Entering U" is the tree after K6b, L1 and lane D (1.2). The last column is PROPOSAL §7's v5 estimate,
for comparison.

| crate | `quality.py` today | true today | K6b, L1, D | entering U | ledger deletes | ledger adds | **after U** | PROPOSAL v5 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| `model` | 18,234 | 19,777 | +140 | 19,917 | 8,575 | 3,445 | **14,787** | 6,950 |
| `engine` | 12,463 | 12,463 | +270 | 12,733 | 4,220 | 1,555 | **10,068** | 6,550 |
| `report` | 6,804 | 6,804 | +140 | 6,944 | 1,990 | 765 | **5,719** | 3,400 |
| `syntax` | 5,634 | 5,634 | +80 | 5,714 | 385 | 225 | **5,554** | 4,350 |
| `sync` | 4,262 | 4,339 | 0 | 4,339 | 417 | 245 | **4,167** | 2,550 |
| `core` | 3,156 | 3,484 | 0 | 3,484 | 388 | 55 | **3,151** | 1,500 |
| `cli` | 2,350 | 2,350 | +300 | 2,650 | 285 | 60 | **2,425** | 1,850 |
| `session` | 502 | 502 | 0 | 502 | 25 | 10 | **487** | – |
| `systems` | 14 | 14 | 0 | 14 | 0 | 0 | **14** | – |
| **total** | **53,419** | **55,367** | **+930** | **56,297** | **16,285** | **6,360** | **46,372** | **~27,000** |

K3f, its own lane between C3 and C4, is not in these columns: by its brief it deletes `owed_by_you`, the `payable` gate and
`makes_debt` (about 70 lines) and adds the debt side of `settle.rs`, its recognition and `deposit` (about 140): **+70**, so
**about 46,450 in all**. By today's `quality.py`, which would go on hiding the 1,948 lines of 1.1, that is about 44,500.

**This does not meet the ceiling.** The ceiling is 27,000, aimed at 26,000 with the margin; the ledger lands about 20,450
lines above the aim. I have not padded it: every entry is one I believe in and can say in one sentence, and the ones whose
size depends on a benchmark say what they come to if the benchmark says no (U11, U19, U29, U30: about 350 lines less in all).

### 4.2 The levers

What each saves after the ledger, and what it costs the person using Axiom. The first six rows are the five the brief names (with
PROPOSAL §7's; the number of views is two rows); the rest are the ones the reading found. None is in the ledger.

| lever | saves | where | what it costs the user |
|---|---:|---|---|
| **L-a** count `sync`'s importers outside the budget (PROPOSAL §7) | −2,300 | `sync` −1,980 (`format`, `tagged`, `peg`, `csv`, `cell`, `amount`, `date`, `recognize`), `model` −320 (their half of `sync_lower.rs`) | nothing at run time. It changes what is counted, not what exists: the readers become a crate of their own outside the ceiling |
| **L-b** the habit forecast leaves (PROPOSAL §7; reverses Decisions 11) | −430 | `report` (after U36) | no "What recurs", no p10/p50/p90 bands: the forecast is what the book promises, and nothing it guesses |
| **L-k** help text (the brief's "cheat sheet or help text") | −350 | `model` −150, `cli` −90, `syntax` −60, `engine` −50 | every diagnostic loses its `help:` line (158 sites); `axiom help` lists the commands without saying what each does |
| **L-c** the number of report views: `why` keeps only the provenance walk (PROPOSAL §7) | −850 | `report` | the per-target pages go: a place's limits and governing laws, a purpose's budget, a contract's term changes become one list of what caused what |
| **L-f** the number of report views: `available` without its what-if | −300 | `report` (`Reach`) | `available` no longer says what reaching the rest would cost (a sale, an early withdrawal's penalty) |
| **L-e** spellings of the language: the v3 hints, the v4 reader and `fmt --upgrade`, once `08`-`10` are ported by hand (Decisions 5) | −550 | `syntax` −250, `cli` −300 | a v4 book no longer upgrades itself; a v3 form gets a plain parse error instead of the hint that names the v5 form |
| **L-d** generic wording for the shape of a declared line (question 3) | −400 | `model` (U4, U5 and the relator's and contract's remaining shape errors) | a wrongly written `loan`, `resets`, `area`, `share` or relator slot says "expected PERCENT after `cap`" in one form instead of its own sentence; the codes stay; no golden or mistake book moves |
| **L-g** asset parts collapse into the asset's basis | −900 | `engine` −700, `model` −100, `report` −100 | an improvement loses its own depreciation and service day; `why ASSET` shows one basis; a sale cannot say which part's basis left |
| **L-h** relators | −450 | `model` | contract kinds that fill slots and write their legs once (`contract job : employment with acme`) go; each contract writes its legs |
| **L-i** addresses | −1,100 | `model` −870 (`addresses`, `reference`, `spelled`, the address half of `parties` and `resolve`), `core` −236 (`postings`, `placement`) | a reference must be a place's path; `chase checking`-style addresses and spelled accounts (K3b, K12) go |
| **L-j** `peak`, `low`, `days` over balances only | −300 | `engine` (the sampler U30 keeps) | a law cannot ask the peak of something that is not a balance (a sum of two, a value in another unit) |
| **all eleven** | **−7,930** | | **about 38,500** |

The cheap ones are L-a (a counting decision), L-d (the wording of errors few people see, with the code kept) and L-e (once
the examples are ported): together −3,250, **about 43,200**. Every other lever takes away something a person uses, and I
would not pull any of them to meet a number.

### 4.3 What 27,000 would take

From about 38,500 with every lever, 27,000 needs about 11,500 more and the aim of 26,000 about 12,500. Only whole features
are that size. Roughly, by the files each lives in (not a plan, the size of what the number asks):

| feature removed | about |
|---|---:|
| `axiom sync` whole (planner, reconcile, write, sinks, apply, recognition; `model/sync.rs` and the rest of `sync_lower.rs`) | −2,800 |
| diagnostics reduced to a code and a location (the sentences the mistakes corpus checks, `malformed.rs`, the snippet renderer) | −2,000 |
| the forecast (after L-b: `forecast`, `trace`, `synth`, the ahead half of `promising.rs`) | −1,200 |
| budgets and limits (`laws/budget.rs`, the budget evaluation, `budget`, `limits`, `headroom`) | −1,100 |
| loan terms beyond a fixed schedule (resets, prepay, grace, indexed, rising, covers; annuity, amortization, `loan_balance`, `loan_opening`, causes) | −1,000 |
| `why` (after L-c) | −900 |
| claims and their recognition (tabs, `settle.rs`, `claims.rs`, `recognition.rs`, the `claims` view) | −900 |
| window totals beyond the month and the year (accruals ahead, tallies' history) | −600 |
| effective ownership and shares | −500 |
| tax views and filed returns (`tax`, `gains`, `filed`) | −450 |
| **all** | **−11,450**, about 27,050, still without the margin |

So 27,000 is a different product: Axiom without sync, a forecast, budgets, loans beyond a fixed schedule, claims and the
diagnostics people rely on. PROPOSAL's 27,000 was a clean-room estimate of a smaller feature set (section 0); the user's
Decision 2 says "behaviour and features stay", and with features staying the honest number is about 46,450, or about 43,200
with the three cheap levers. Question 2 asks you to choose.

---

## 5. The queued lanes

| lane | what U takes | what U leaves | when | why |
|---|---|---|---|---|
| **K12b** cleanup | **all of it**: the long functions (`lower_occurrence`, `lower_loan_origin` in U7; `post_written_occurrence` U28; `lower_contract` U10; `finish` U26; `arrive`'s `bool` U21/U22); `props.rs`'s three jobs and the built-ins on one reader (U4); the three entry points, `Addressed` and the memo (U11); `Book.sites`, reader styles, weights stored, frozen once, `shortest_that`, `name_claims`, `Request` (U18); three records of a settlement and the write-off's reversal (U23); `Streams<R>`, `promise_through` (U27); `loan_balance.rs` (U32); dead code (U33); the eight `clippy` errors in `core` (the first commit) | two items that change what a book says (`owner` without a range, `unknown-address`'s suggestion): to lane D or a decision (question 11) | inside C1-C5 | each item is one of U's unifications or sits in a file U rewrites; a separate lane would edit the same functions twice |
| **K4c** flows in columns | item 4 (K4b's list: U25) and item 1's reader shape (`FlowRef`: U24) | the column layout (hot and cold columns, `Flows::push`, the tagless quantity of item 2, `Staged` as one mark of item 3) | its own lane after C5, against `FlowRef`, held to its benchmark (RSS down a quarter, `check` at 1m not slower) | the unifications delete lines; the layout is a performance change with its own proof, and behind `FlowRef` it touches no reader |
| **K3e** parcels in columns | items 2, 3 and 4 (identity key U20, relief as a ranking U19, asset parts U21) | item 1 (parcels in columns), and item 5 as K3e says | C4; the columns after, as their own lane, only if their benchmark pays | as K4c |
| **K3f** debts as parcels (Decisions 13) | nothing | all of it | its own lane, beside C1-C3 (its files, `settle.rs`, `claims.rs`, `post.rs`'s gate, `ledger.rs::all`, `Class`, `Sides` and four report files, are not code C1-C3 rewrites), merged **before C4** | it changes outputs (`07-landlord`, `10-budgeter`) and brings its own oracle; U's checkpoints are byte-identical against a baseline binary, and mixing the two would lose that proof. Before C4, so the one relief of U19 serves debts too |
| **K7c** output unifications (Decisions 14, built) | **K7c-1 to K7c-6**, all of them (U35) | nothing | C6, first | decided; they change only the views named, and U37 to U42 build on them |
| **L2/L3** positions under their agent; an optional counterparty; purposes without a direction root | nothing | all of it: their briefs are not written yet (STATUS) | after C2 and C3 at the earliest, written against `Lowering::group` (U7) and `World::meaning` (U11) | they change the language, which needs your sign-off (question 6); after C2 and C3 each is a change in one place instead of five. They delete model lines (STATUS); the ledger does not count them |
| **K6b**, **L1**, **D** (running and following) | nothing | all of it | before C1 | U starts from their merged tree. I expect K6b to add about 550 lines (`engine/offspring.rs`, the post host; U34 then decides one derive host), L1 about 380 (the junction and one header production in `syntax`, `legacy.rs`, `upgrade.rs` in `cli`), lane D nothing net |

---

## 6. The checks

**On every commit**, against the baseline binary built from the checkpoint's starting commit: `cargo fmt --check`,
`cargo clippy` (`core` clean with `-D warnings` from phase 1a; the rest of the workspace has 238 warnings today, 168 of
them `result_large_err` for a `Diagnostic` in an `Err`: counted per crate in each checkpoint report; C1 broke "never up, and
none in code U writes", see section 11),
`cargo test --workspace --release` (no known failures once lane D has merged), the goldens (`sh tests/golden.sh`: 60
outputs), the mistakes corpus (`sh tests/mistakes/run.sh`: 115 books, 108 files and seven projects), the differential harness
(`docs/v5/measure/diff/run.sh` on both binaries, then `compare.sh`: 156 cases of mistakes and odd shapes, 33 valid projects
read by every view), `fuzz.py … diff` on 200 books, and `quality.py` with the running total in the commit body.

**Per checkpoint**, the oracles of what it touches, and at the end `bench/` (`sh bench/run.sh 100k 1m`, `sh bench/profile.sh
100k`: wall, user CPU, RSS, instructions; three runs, the fastest, with the load average):

| checkpoint | oracles and harnesses | what must not move |
|---|---|---|
| C1, C2, C3 | the Book dump (to write) on every book we have; `splits.py`, `contracts.py`, `loans.py`, `derives.py`, `relators.py`, `addresses.py`, `family_addresses.py`, `purposes.py`; the declared-lines corpus (to write) | the Book, arena by arena; every diagnostic's bytes and order |
| C4 | `claims.py` (1,500 books and its mutants), `splits.py`, `tabs.py`; the K3e relief model test, the sale-heavy and prorata books and the part-basis transition test (to write); `docs/v5/measure/parcels/` | every relief order; `check` at 1m and on the sale-heavy book not slower, RSS not up |
| C5 | fuzz `diff`, `splits.py`, `forecast.py`, `loans.py` (reading the structure U32 adds), `docs/v5/measure/promises/` and `internals/` (the materialized dump); the window-totals model test and the temporal oracle (to write) | the fold's output; `check` at 1m not slower (U24, U29, U30 are on the hot path) |
| C6 | the goldens (three move with K7c-6, listed), `session/allcmds.sh`, `whys.py`, `dates.py`, `fuzzcmds.py`, `report_mutants.py`, `climutate.py`, `forecast.py`, sync's tests | every view's bytes, text and JSON, except K7c's |

**What is missing, and what I will write** (each in the commit before the code it proves, run on the baseline first):

1. `quality.py` matching a test module's braces (question 1): the first commit of phase 1.
2. **The Book dump** (C1): `docs/v5/measure/internals/` gains a dump of every arena of a `Book` in id order as text, with
   expression nodes compared by structure, run over the examples, goldens, the books of the mistakes corpus that lower,
   `diff/cases2`, 200 fuzz books and `bench/` 100k. Lowering's unifications (C1-C3) are proven by this: same sources, same
   Book.
3. **The declared-lines corpus** (C1, before U4): `docs/v5/measure/diff/cases3/`, one small book per error path of every
   property line (about 120, generated by reading each `return None` of the readers U4 replaces), read by `run.sh`. Today no
   golden or mistake book prints a contract property's shape error.
4. **K3e's model-based relief test** and two bench books (C4): a naive implementation of every policy compared on seeded
   random landings and reliefs; a sale-heavy book (100k lots, 50k sales) and a prorata one from `bench/gen.py`.
5. **The part-basis transition test** (C4, U21): on a branch with both stores, `part_basis(anchor)` equals `Part.basis` after
   every step of 1,500 generated books; then the old store goes.
6. **A window-totals model test** (C5, U29) and **a temporal oracle** (C5, U30): naive day-by-day sums and extremes on random
   books, compared with the fold.
7. **Mutants** (`mutation.py`) for every new type: the signature reader, the tail, the group lowering, the ranking, the
   series, `Streams`, the cell sink. A mutant that survives every oracle and test gets a test that names what it checks.

---

## 7. The risks to features

**None: no entry removes a feature, and only U35 changes an output, as decided.** The places where a unification could lose
something by mistake, and what stops it:

| where | what could be lost | what stops it |
|---|---|---|
| U4, U5 | the exact sentence of a declared line's shape error, and which error a line with two mistakes reports | the `Said` columns copied from the code; the row's order is the check order; the declared-lines corpus run on both binaries |
| U7, U10, U28 | an occurrence's arenas, a derived flow's ordinal, the order of a contract's compiled nodes | the Book dump (U7, U10); `internals/`'s materialized dump (U28, where the Book's shape changes on purpose); seven commits for U7, one kind of line each |
| U11 | a did-you-mean candidate; `check`'s speed if the memo goes | the mistakes corpus; `bench/` (the memo stays if it pays) |
| U19, U20 | the order `exact` relieves in; a merge of two parcels that differ | `claims.py` and its mutants; the model test; a collision test |
| U21 | a part's basis on any day | the transition test; the `asset-state` diagnostics it removes are an internal invariant's, raised by no book |
| U29, U30 | a window total at a boundary; an extreme on a quote day | the model test and the temporal oracle; each entry keeps the old structure where the benchmark or oracle says so |
| U36 | the order of a habit's flow and a contract's on one day; the bands' draws | `forecast.py`; the forecast goldens |
| U43 | the terminal's colours and padding | the text goldens; `climutate.py` |
| U45 | a refusal to write outside the project (security) | each refusal keeps its test, and one is added per path the three copies took |
| U33 | `core::trail`, which nothing uses | it is history if K4c wants it |

The levers of section 4.2 each remove a feature; none is in the ledger, and none will be pulled without your decision.

---

## 8. The order, and the first checkpoint

### 8.1 The order

1. K6b, L1 and lane D merge (running and following). Nothing of U starts before: C1 and C2 rewrite the header lowering L1
   changes, and C5's U34 reads what K6b leaves.
2. **The count** (two commits): `quality.py` matches braces and STATUS's baseline is restated; `core`'s eight `clippy` errors
   (K12b's) are fixed.
3. **C1**, merge. K3f runs as its own lane beside C1-C3.
4. **C2**, merge. From here L2/L3 can be written against one lowering.
5. **C3**, merge. K3f merges before C4.
6. **C4**, merge. K3e's columns, if ever, as their own lane after.
7. **C5**, merge. K4c's columns as their own lane after.
8. **C6**, merge.

Model first: half the net is there, and the later checkpoints read the Book it builds (U24 and U25 need C2's one flow builder,
U28 needs U7). Relief before flows, so that K3f lands on a stable engine and C4 is self-contained. The report last, because it
reads `FlowRef`, `Streams` and `Records`.

### 8.2 The first checkpoint, C1: types and signatures

```rust
// model/src/lower/site.rs: one source site being lowered (U1)
pub(crate) struct Lowering<'w, 'a, 's> {
    pub world: &'w mut World<'s>,
    site: &'a Site<'a, 's>,
    diags: &'w mut Vec<Diagnostic>,
    stage: Option<Marks>,                     // where the journal's arenas ended when the innermost stage opened
}
impl<'w, 'a, 's> Lowering<'w, 'a, 's> {
    pub fn new(world: &'w mut World<'s>, site: &'a Site<'a, 's>, diags: &'w mut Vec<Diagnostic>) -> Self;
    pub fn file(&self) -> &'a ast::File<'s>;
    pub fn home(&self) -> Home;
    pub fn word(&self, name: ast::Name<'s>) -> Word<'s>;
    pub fn say(&mut self, problem: Diagnostic);
    pub fn or_say<T>(&mut self, found: Result<T, Diagnostic>) -> Option<T>;
    /// Runs `lower`; everything it wrote to the journal's arenas is taken back unless it returns `Some` (today's
    /// `Staged`, whose drop is every early return, as a closure: the borrow of the world cannot outlive it).
    pub fn staged<T>(&mut self, lower: impl FnOnce(&mut Self) -> Option<T>) -> Option<T>;
    /// The flows and codes written since the innermost stage opened.
    pub fn written(&self) -> Written;
}
```

`World` keeps its lookups (`seek_*`, `commodity_of`, `literal_amount`); `Lowering` adds the ones that need the site
(`entity`, `purpose`, `commodity`, `end` in 2.1). A pass is a method: `impl Lowering<'_, '_, '_> { fn contract(&mut self,
written: &ast::Contract<'_>) -> Option<Contract> }`. The law compiler's `Compiler` holds a `Lowering` in place of its four
fields. A function that needs only the book takes `&Book`, as now: `Lowering` is for what lowers one site, not a way to pass
the world everywhere.

```rust
// model/src/lower/tail.rs: what a line says of its flow (U2)
#[derive(Clone, Copy, Default)]
pub struct Says { /* purpose, description, payee, recognized, waive, detail, basis, price: as 2.2 */ }
#[derive(Clone, Copy)]
pub struct Clauses { takes: ClauseSet, refused: Refusal }      // ClauseSet: a u16 of ClauseKind bits
#[derive(Clone, Copy)]
pub enum Refusal { Said(&'static Problem), Unread }            // `until-position`, `also-tail`, …; a term tail's silence
pub enum TailAt { Dated(Day), Undated, Also }                  // a relative `due` needs a day; an `also` refuses it
impl Clauses { pub const FLOW: Clauses; pub const TERM: Clauses; pub const ALSO: Clauses; pub const MEASURE: Clauses;
               pub const WAIVER: Clauses; pub const ENDING: Clauses; pub const VALUE: Clauses; pub const WRITE_OFF: Clauses;
               pub fn takes(self, kind: ClauseKind) -> bool; }            // what `syntax/src/statement.rs` reads too
impl Lowering<'_, '_, '_> {
    pub fn tail(&mut self, clauses: ast::Many<ast::Clause>, takes: Clauses, at: TailAt) -> (Run<Sym>, Says);
}
impl Says { pub fn under(self, parent: Says) -> Says; }        // a leg's tail over its header's
```

```rust
// model/src/lower/amount.rs: a written amount and quantity (U3)
pub enum Unsaid { Said, Missing(Diagnostic) }   // said already, or for the caller to say (a journal flow drops it)
impl Lowering<'_, '_, '_> {
    pub fn amount(&mut self, written: ast::Amount<'_>, fallback: Id<Commodity>, roots: &Roots) -> Result<Expr, Unsaid>;
    pub fn quantity(&mut self, written: ast::Quantity<'_>, side: FlowSide, fallback: Id<Commodity>, roots: &Roots)
        -> Option<Taken>;
}
// and the model reads `ast::Sign`, `ast::Policy`, `ast::Period`, `core::Cadence` (which `syntax` re-exports) directly.
```

```rust
// model/src/props/signature.rs: one reader of a property line (U4)
pub(crate) struct Signature {
    pub name: &'static str,
    pub once: Once,                          // Only(&'static str: the noun `problem::twice` says) | Repeats
    pub family: Option<Family>,              // the code and sentence every label of this line shares (a reset's)
    pub empty: Option<Words>,                // what a line with no arguments says, when it says something of its own
    pub shape: Shape,                        // Each: every word says its own label | Whole { after: u8, said: Words }
    pub slots: &'static [Slot],
    pub lines: &'static [&'static Signature],
    pub other_line: Option<Words>,           // a nested line it does not take
}
pub(crate) enum Slot {
    Word(&'static str, Said),                // a keyword; `Said::SHAPE` when the row's shape error covers it
    Value(Want, Check, Said),
    Optional(&'static [Slot]),
    Keyed { keys: &'static [(&'static str, Want, Check, Said)], unknown: Text, twice: Text },  // any order, each once
}
pub(crate) enum Want { Day, Span, Percent, Amount, Name, Entity, Place, Asset, Commodity, Param(Dim), Purpose, Text,
                       Count, OneOf(&'static [&'static str]), IndexPlusMargin }
pub(crate) enum Check { None, Positive, NotNegative, NotBefore(Earlier), Declared }  // Earlier: a slot of this line or its parent's
pub(crate) struct Said { pub missing: Text, pub wrong: Text, pub range: Text }
pub(crate) enum Text { Shape, Label(&'static str), Own(Words) }  // the row's shape error | a label under `family` | a sentence
pub(crate) struct Family { pub code: &'static str, pub message: &'static str }
pub(crate) struct Words { pub code: &'static str, pub message: &'static str, pub label: &'static str }
pub(crate) struct Args<'a, 's> { line: &'a ast::Prop<'s>, values: Vec<(Arg, Loc)> }   // in slot order
impl<'w, 'a, 's> Lowering<'w, 'a, 's> {
    /// Finds `signature`'s line among `props` (none, one, or the `twice` error), reads it slot by slot in the row's order,
    /// says the first thing that is wrong exactly as the code it replaces said it, and returns the values.
    pub fn property(&mut self, props: ast::Many<ast::Prop<'s>>, signature: &'static Signature) -> Option<Option<Args<'a, 's>>>;
}
```

Two rows, written out, show what byte-identical wording costs (every `own` and `label` text is today's, verbatim):

```rust
const LOAN: Signature = Signature {
    name: "loan", once: Once::Only("loan"), family: None,
    empty: Some(words("contract-loan", "a loan needs its principal, start date, rate, and term",
                      "write `loan AMOUNT on DATE at RATE over SPAN`")),
    // today's order: the principal is read and checked before the count and the words are
    shape: Shape::Whole { after: 1, said: words("contract-loan", "the loan definition has missing or extra fields",
                                                "write `loan AMOUNT on DATE at RATE over SPAN [for ASSET]`") },
    slots: &[
        Value(Want::Amount, Check::Positive, Said { missing: Text::Shape,
            wrong: own("contract-loan-principal", "a loan principal must be a literal amount", "write an amount such as `3_000 USD`"),
            range: own("contract-loan-principal", "a loan principal must be positive", "this amount is not positive") }),
        Word("on", Said::SHAPE),
        Value(Want::Day, Check::None, Said::wrong(own("contract-loan-date", "a loan start needs a date", "write the date after `on`"))),
        Word("at", Said::SHAPE),
        Value(Want::Percent, Check::NotNegative, Said::wrong_or_range(
            own("contract-loan-rate", "a loan rate must be a nonnegative percentage", "write a rate such as `5.875%`"))),
        Word("over", Said::SHAPE),
        Value(Want::Span, Check::Positive, Said::wrong_or_range(
            own("contract-loan-term", "a loan term must be a positive span", "write a term such as `30y`"))),
        Optional(&[Word("for", Said::all(own("contract-loan-asset", "a financed asset follows `for`", "write `for ASSET` here"))),
                   Value(Want::Asset, Check::Declared, LOAN_ASSET)]),       // two more sentences, the second with the name
    ],
    lines: &[&RESETS, &PREPAY],
    other_line: Some(words("contract-loan-property", "this nested loan property is not supported", "remove or correct this property")),
};
const RESETS: Signature = Signature {
    name: "resets", once: Once::Only("reset rule"),
    family: Some(Family { code: "contract-loan-resets", message: "a loan reset rule needs an interval, date, index and margin" }),
    empty: None, shape: Shape::Each,
    slots: &[
        Value(Want::Span, Check::Positive, Said { missing: label("write `resets 1y from DATE to PARAM + PERCENT`"),
            wrong: label("the reset interval must be a positive span"), range: label("the reset interval must be a positive span") }),
        Word("from", Said::all(label("write `from DATE` after the reset interval"))),
        Value(Want::Day, Check::NotBefore(Earlier::Parent(LOAN_ON)), Said { missing: label("write the first reset date after `from`"),
            wrong: label("write a full date for the first reset"), range: label("the first reset cannot precede the loan") }),
        Word("to", Said::all(label("write `to PARAM + PERCENT` after the reset date"))),
        // `PARAM + PERCENT`: the margin's three labels, then the index looked up and its unit checked, in today's order
        Value(Want::IndexPlusMargin, Check::NotNegative, RESET_MARGIN),
        Keyed { keys: &[("cap", Want::Percent, Check::NotNegative, RESET_LIMIT), ("life", Want::Percent, Check::NotNegative, RESET_LIMIT)],
                unknown: label("only `cap PERCENT` and `life PERCENT` follow the margin"),
                twice: label("write each reset limit once") },
    ],
    lines: &[], other_line: None,
};
```

`Want::Amount` resolves the unit as it reads (today the unit is looked up before the sign is checked, and the reader keeps that
order). What the loan *means* stays code after the read: the lender is not the borrower (`contract-loan-party`), the debt tab.
The rows are longer than a generic reader's because the sentences are today's; with generic wording (L-d) the `Said` columns
become one per `Want`, the rows a line each, and U4 is −960 instead of −870.

```rust
// model/src/collect.rs: one walk (U6)
pub(crate) struct Collected<'a, 's> {
    /* today's buckets, and: */
    pub mentions: Vec<Mention<'a, 's>>,      // where a party can stand, in source order (declare/mentions.rs)
    pub tallies: Vec<Counted<'a, 's>>,       // every law step that counts (laws::counted)
    pub budgets: Vec<&'a ast::Item<'s>>,     // budget::Found::read
    pub lives: Vec<Lives<'a, 's>>,           // every `lives` and `now lives` (declare::scopes)
    pub originations: Vec<Origination<'a, 's>>,  // loan_opening
}
// model/src/fill.rs: one filling (U5)
pub(crate) enum Filling { Thing, Role(&'static RoleWords) }   // which codes and words a misfit is said with
pub(crate) fn fill(book: &Book, slots: &Slots, line: &Args, filling: Filling) -> Result<Filled, Misfit>;
```

### 8.3 C1, commit by commit

1. The Book dump and the declared-lines corpus (tools only; run on the baseline binary).
2. U1 in six commits: `Lowering` with `staged`, and `Compiler` holding one; `lower/contracts.rs`; `lower/statements.rs`;
   `lower/record.rs`, `flow.rs`, `tail.rs`, `infer.rs`; `laws/`; `props.rs`, `params.rs`, `sync_lower.rs` and the declare passes.
   The last deletes the copies in `FlowCx`, `TermsCx` and `Stated`.
3. U6: `Collected`'s new lists, the six walks reading them, the walks deleted.
4. U2: `Says`, `Clauses`, `Lowering::tail` with the journal's flows on it; the statements; the term and `also` tails; the
   parser reading `Clauses`; the five old types deleted.
5. U3: `amount` and `quantity` with their readers moved; the enum copies deleted.
6. U4: the reader and the built-ins (`Args` becomes its result; `props.rs` split into grammar, staging and settings); `input`
   and `now at`; `loan`, `resets`, `prepay`; the schedule properties; `area`, `deposit`, `share`; `format` and `source`; the
   leftovers deleted.
7. U5: the relator on `fill::fill`.

Every commit: Book dump identical, declared-lines corpus identical, mistakes, goldens, `diff/`, tests. At the end: model
19,917 to about 17,650 and syntax 5,714 to about 5,675 (true count), the line table per crate and file, `bench/` 100k and 1m
(C1 is not on the fold's path; the numbers are recorded so C2 has its baseline).

---

## 9. Questions for you

1. **The count.** May the first commit of phase 1 make `quality.py` match a test module's braces? It restates today's 53,419 as
   55,367: the 1,948 hidden lines are code (section 1.1), and U rewrites five of the files that hide them.
2. **The ceiling.** 27,000 is not reachable with the features and the output kept: the ledger reaches **about 46,450** (true
   count; about 44,500 by today's `quality.py`). With the three cheap levers (L-a, L-d, L-e) about 43,200; with all eleven about
   38,500; 27,000 needs whole features to go (4.3). Which ceiling, and which levers?
3. **Generic wording (L-d).** May a declared line's shape errors (contract properties, relator slots) take one generic form per
   kind of value, keeping their codes? No golden or mistake book prints them; −90 in U4, −50 in U5, −400 in all.
4. **K3f** as its own lane beside C1-C3, merged before C4: agreed?
5. **K4c's and K3e's column layouts** after U as their own lanes, held to their benchmarks, with U taking their unifications
   (`FlowRef`, K4b's list, the ranking, the key, the parts): agreed?
6. **L2/L3.** Their briefs are not written. Do you sign off on the language changes, and may they be written against C2 and C3?
7. **`Lowering` and `View`.** Each holds what one site of lowering (or one view of a run) is, the way the law compiler's
   `Compiler` already does; neither is a bundle made to cut a parameter count. Acceptable?
8. **U36.** A habit becomes a promise stream posted through K5c's interface, with "What recurs" and the bands reading the
   streams: is that the shape Decisions 11 meant by "a source of flows applied through the same fold"?
9. **U30.** `peak`, `low` and `days` over a balance read the run's histories (`Steps::extremes`, built by K7b and read by
   nothing), and the sampler stays for every other root. Agreed?
10. **U46.** `DateLayout` moves from `core` (the vocabulary every crate speaks) to `sync` (its one reader): net zero. Agreed?
11. **K12b's two behaviour items** (`owner` written without a range; `unknown-address` suggesting the closest name rather than
    the closest address): lane D, or not at all? U does not change them.

---

## 10. Decided (the coordinator's answers, 2026-10-04)

The plan is approved as the execution plan, C1 to C6, with a seventh checkpoint added. What changes in the sections above:

- **The count** (question 1): `quality.py` skips an inline test module to its matching brace (comments, strings, raw strings
  and char literals do not count, a lifetime is not a char). The baseline is **55,367** at `a21eee9`: model 19,777, engine
  12,463, report 6,804, syntax 5,634, sync 4,339, core 3,484, cli 2,350, session 502, systems 14.
- **The ceiling** (question 2): 27,000 stays the target; the coordinator carries the gap to the user. Nothing that removes
  something the user has is pulled: **L-a, L-b, L-c, L-f, L-g, L-h, L-i, L-j and L-k stay out** (the user asked for addresses,
  relators, the habit forecast and gorgeous diagnostics; L-a is a counting trick).
- **L-d is in** (question 3), on one condition: a declared line's shape error is generated from its `Signature` and is at least
  as helpful as today's. It names the property, the position, what the shape expected and what was found, and its `help:` line
  is the signature itself as the example, so the table is also the documentation. Codes are unchanged. U4 and U5 therefore
  take the generic form (−960 and −100), the `Said` columns of 8.2 become one generated message per slot, and the
  declared-lines corpus is the list of every message that changes, before and after, for review.
- **L-e** is decided last, as the final entry of C7, and only if the tree is still above the ceiling then.
- **The landing to plan against**: the ledger with L-d, about **46,050** counted truly (45,500 if L-e is pulled at the end of
  C7). The job from here is to beat it.
- **K3f** (question 4): its own lane beside C1-C3, merged before C4; the coordinator launches it when K6b has merged.
  **K4c's and K3e's layouts** (question 5): their own lanes after U, held to their benchmarks. **L2/L3** (question 6): signed
  off as PROPOSAL §6 describes them (positions under their agent, an optional counterparty, purposes without a direction root,
  the `account` keyword going); their briefs are written after C2/C3 against `Lowering::group`.
- **`Lowering` and `View`** (question 7): acceptable as single domain concepts, each the context of one phase that really
  travels together (the law compiler's `Compiler` is the model), never a grab-bag; each struct's doc says so.
- **U36, U30, U46** (questions 8 to 10): yes. **K12b's two behaviour items** (question 11) are bug fixes and go to lane D.

New requirements:

1. **Every checkpoint report** gives, per ledger entry, the lines planned against the lines delivered, and a revised landing
   for the whole plan. For each entry, look for what else became deletable once it landed (follow-on deletions) and take it in
   the same checkpoint.
2. **C7, round two**, after C6: the census and the concept inventory run again on the unified tree (the first round removes the
   duplication that can be seen; the second finds what it was hiding), its ledger written and executed. Its plan is part of
   C6's report. L-e is its last entry, if the ceiling is still not met.
3. **The ceiling is a gate on the whole lane**: at the end of each checkpoint, the report says how far the tree is from 27,000
   and what the next biggest un-taken unification is.

Phase 1a, the instruments, runs now (K6b, L1 and lane D are still running): the count; `core`'s eight `clippy` errors; the
declared-lines corpus (`docs/v5/measure/diff/cases3/`) and its run on a baseline binary built from the v5 head; the sale-heavy
and prorata bench books (`bench/gen.py`) and K3e's model-based relief test (a test module; `lots.rs`'s code unchanged); the
mutation lists the instruments can already be held to. The Book dump waits for C1: it needs L1's `Debug` derives and K6b's
`Book` fields.

Phase 1a done (2026-10-04): the count restated (55,367); `core` clean under `clippy -D warnings` (+9 lines: `Add<Span>`
for `Day`, `Neg` for `Dec`, a type name; 55,376); the declared-lines corpus (208 cases, 68 codes, all raising what they
claim on a baseline binary of `b3b98fc`); the relief model test and its fourteen mutants, all killed; the relief books.
The corpus found that `currency USD` cannot be written (the built-in's reader asks for a name, a currency is lexed as a
unit): a bug for lane D or for U4, whose `Want::Commodity` reads units.

## 11. C1, delivered (2026-10-04)

From the v5 head `bce9735` (57,109) to `e65eb8a` (56,146): **−963**, of which the entries **−1,019** and two additions
(the Book dump's `Debug` derives +21, the `purpose-of-its-own` hint +26) and phase 1a's `core` (+10).

| entry | planned (with L-d) | delivered | why it differs |
|---|---:|---:|---|
| U4 one reader of a property line | −960 | **−437** | the built-ins' reader (`Args`) became the one reader instead of a `Signature` table; `format` and `source` lines are strings, not expressions, and keep their reader (−100 of the plan); the plan counted lines U1 also counted |
| U3 a written amount and quantity | −200 | **−82** | `written_amount`/`written_part` replace six readers; the template's share and the journal's dropped root stay as a caller's choice, not a type |
| U2 one tail | −270 | **−150** | one `read_tail` with a `Line` for the flow, term, `also`, measure and ending; a value's tail (`via` is a place) and a waiver's (its code is its name) mean other things; the parser's own clause table stays (sharing it moves refusals between parser and lowering) |
| U1 the lowering site | −840 | **−350** | the four parameters were two: `world` and `diags` always travel together, so the world keeps its diagnostics (−259); `home` and `file` already travel in the passes' own contexts (`Site`, `Written`, `Stated`, `FlowCx`, `Placement`); a statement is lowered in its context (−91); 24 signatures still pass `home` and `file` apart (about −40, left for C2, which rewrites those passes) |
| U5 the relator on `fill::fill` | −100 | **0** | on reading, the relator shares one line with `fill::thing` (the kind test); its fills are names, not values, and its five errors are its own codes and words: carrying both vocabularies through `fill` adds what it removes |
| U6 one walk | −80 | **0** | the walks left are short queries already on `Collected` (`values_written`, `scopes`, `named_by_openings`, `originations`); `mentions` keeps its own walk for its order (first mention across kinds of item) |
| **C1** | **−2,450** | **−1,019** | |

Follow-ons taken in C1: `Tail::merge` as one struct; `implied_end` and `written_part` return their problem (the readers that
said into a list to be thrown away); `loan_endpoint` takes back what it said; a claim's two parties read in one line; a
statement's unsupported case is a method of its context; `contracts`' "0 kepts" (the count under its column); two unused
imports; the declared-lines corpus claims what the v5 head raises (`currency`, fixed there) and what U4 says once.

What C1 changed, all of it: 107 outputs of `diff/` (86 declared-line errors in other words, codes the same; three share
cascades said once, two of them by their cause, `contract-share-unit`, not their first; 18 `contracts` tables), 104 Books
(the same diagnostics, the measures' codes in the pool, one ending's description interned where written, and the hint),
three goldens/mistakes (the hint). Every other output is byte-identical to `bce9735`.

**Revised landing.** C1 delivered 42% of its plan, the K lanes' own ratio (section 0). The ledger's estimates counted the lines
an entry touches as the lines it deletes; what unifies needs its own lines for the cases it still tells apart. Applied to what
is left: C2 about −800 (of −1,450; U7's signature savings are taken), C3 −650 (of −1,190), C4 −900 (of −1,295; U21 removes a
store and its guards, the likeliest to hold), C5 −1,000 (of −1,755), C6 −1,100 (of −1,925), C7 perhaps −700: the tree lands
at about **51,000**, not 46,050. The distance to 27,000 is **29,146** today and about 24,000 then.

`bench/` (C2's baseline): `check` at 1m 5.01 s against 4.95 s on `bce9735` (median of five interleaved runs), 672 MB both; the
rest within the machine's noise. Found on the way, for their owners: the bench books exit 1 on both binaries (1,022 errors at
100k) and write `purpose pN-edu` on their grants (`bench/nativeize.py`); 02-household and 03-violations write `purpose
education` on the scholarship, so 03's expected `purpose` violation fires because `grant-purpose` is unset; 10-budgeter's
`check` warns October's `#fun` at 297.01 USD (the total at its second firing) where `budget` says 323.02, the window total
counted twice, which U29 (one recognized series) is to make one; the engine's dead code since the K merges
(`Assets::into_states`, the guards' `before`, `part_straight_line`) is U21's and U33's.

`clippy`: `core` clean with `-D warnings`; the workspace has 277 warnings against 245 on `bce9735`. C1 removed 15 (five
`too_many_arguments`, the unused imports, `core`'s eight from phase 1a) and added 47 `result_large_err`: the one reader of
property lines returns `Result<_, Diagnostic>` as all of `model` does (`args.rs` 22, `lower/contracts/lines.rs` 25, the
rest in `written_amount`, `written_part` and `implied_end`). That breaks section 6's promise of none in code U writes. The cure
is one change in `core`, not 264 boxes: a `Diagnostic` that holds its parts behind one `Box` (with `Deref` to them, so every
reader of `.code` and `.labels` stays as it is) makes every `Err` one pointer wide and the lint silent everywhere. It is an
entry for C6 (U43's neighbourhood), or for now if the coordinator wants the count down before C2.
