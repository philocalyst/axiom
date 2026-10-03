# v5 status

Where the rewrite stands, and what is waiting on a decision. Read [`DESIGN.md`](DESIGN.md) for the design and
[`lanes/common.md`](lanes/common.md) for the standard every lane is held to. The integration branch is
`claude/great-wozniak-pnqn7x-v5`; each lane works on its own `-v5-*` branch and is merged here after review.

## Lanes

| lane | what | state |
|---|---|---|
| **C** core primitives | `tagless`, `dayset`, `sparse`, `postings`, `placement`, `trail` | **merged** (`8704e75`) |
| **K0a** model groundwork | `Staged`, one `problem` catalog, `Word::of`, one collect pass, dead code, short functions | **merged** (`5f99b98`) |
| **K0b** outer groundwork | owners, JSON writers, sync dates, sync apply, `RuntimeRange`, short functions | **merged** (`daf104e`) |
| **C3** `postings` SIMD | the `fearless_simd` block-compare kernel, kept only if it is 1.3× | **merged**: kept, 1.8-2.7× |
| **C2** `core::facts` | the store of timelines: `Key<V>`, painting `Builder`, frozen CSR `Facts`, `days_where` as an integral, sets as `Many` | **merged** (after K0a) |
| **K12** kinds, slots, facts | typed slots, `Taxonomy`, numbering the holders, moving every reader to `core::facts` | **merged** (`e5a3554`) |
| **K4a** one split vocabulary | `Quantity`, `Part`, `Expr`, `Group<H,F,I>`, one `Program` replace the two parallel Template*/Journal* families | **merged** (`d1daf1f`) |
| **K3a** positions that need no prediction | tabs created lazily; delete the survey, `find_tabs`, `contract_endpoints`, `unregistered-tab` | **merged** |
| **K4b** one `solve` | `solve` over an `Env` makes a split for the model (constant folding, static conservation check), the promise fold and, new, the statement fold | **merged** (`456b2dd`) |
| **K5a** a promise is a term | `core::Dues` (due days counted by arithmetic), `model::promise` (`Term`, `Schedule`, `Residual`, `Annuity`), compiled once beside the old code, proven equal to an independent reference | **merged** (`92e80c1`) |
| **K3c** claims and parts | `exact` is a relief policy, a flow's codes name claims, write-off is relief, a payment from a party settles its claims, a tab is a claim by its kind. Asset parts: **no**, with evidence | **merged** (`0089678`) |
| **K5b** the fold reads the promise | a contract's terms stored once, the old schedule walkers and sync's dead `dues` deleted, the monitor (`missed-occurrence`), `grace` as LANGUAGE §7 says | **merged** (`c8c1695`) |
| K3b addresses | `Addresses`, declaration words fill slots by forced placement | running (map first; stops at the map if a grammar change is needed) |
| **K5c** forecast | the forecast is the fold past today (`Ledger::promise`: a heap of due days, one `Residual` per stream); a missed `Due` the party owes is a claim | **merged** (`bdc25f9`) |
| K3d claims, recognition | `books cash\|accrual`, a split payment settles by what the party pays, debts as parcels | brief written |
| K4c flows in columns | `Flow` (192 bytes) as hot columns and a cold record, a quantity as a tag and a payload, K4b's cleanup list | brief written |
| K6 norms and relators | one rule IR (`Derive`), relators written once and projected per book | brief written |
| K7a the `Session` | the library surface an MCP server and a GUI are written against; the CLI becomes a client | brief written |
| K7b facts out | steppers, pivots, provenance `why`; the views stop re-folding | brief written (after K7a, K3d) |
| K3e parcels in columns | `lots.rs`, `assets*.rs` (~2,500 lines): hot columns, an identity key, relief as a ranking plus a way of taking, asset parts if the smaller cut is a net deletion | brief written (after K3d, K4c) |
| K5d loans | a loan is a state machine with four inputs; a payment says `#interest` and `#principal`; resets, prepay, `for ASSET`, a statement reconciles the schedule; `deposit` if K3d's debts-as-parcels landed (`match` is K6's) | brief written (after K5c) |
| L1 the junction | one line grammar, `<-` and `@`, legs lead with arrows, `fmt --upgrade` ports every example; syntax only: the lowered book is identical | brief written (after the kernels) |
| L2/L3 language, semantic | positions under their agent, debts as promises, optional counterparty, purposes without a direction root | after L1 and K6 (brief not yet written) |

Test baseline before any lane: 734 passed, 4 failed, 8 ignored. Lane C on top: 777 passed, the same 4 failed, 13
ignored (the new ones are benchmarks). The four failures are the ones `v2/REMAINING.md` names.

## Lane C, in numbers

| | |
|---|---|
| lines added | +683 non-test in `core` (2,508 total) |
| `unsafe` | one macro, nine expansions, argued sound for every bit pattern |
| tagless | a count of one kind of value over the tag column alone is 6.5× faster than over an enum |
| sparse table | a range query is 4.2 ns at any width, against 64 µs for a scan of 65,536 |
| `postings` merge | 2.1–2.7× faster than the classic merge, galloping wins from a skew of 8 |
| `Trailed<_, ()>` | writes exactly like a `Vec` |

## Waiting on you

1. **`fearless_simd` source access.** The registry source stayed unreadable to the lanes (a permission refusal) and docs.rs is
   blocked by the network policy; nothing was worked around. The API was recovered from our own scratch prototypes, and
   lane C3 built the `postings` kernel from them: no `unsafe`, 1.8-2.7× the scalar merge. If you would like lanes to be
   able to read the crate, allow `~/.cargo/registry/src/*/fearless_simd-*`.
2. **The budget ceiling.** The design lands at about 27,000 lines, with a floor of about 24,500 and levers to about
   20,000 (PROPOSAL §7). The tree is at about 51,970 non-test lines: the lanes so far built structure (K12, K4b, K5a add
   code; K4a, K3a delete) and the deletions are ahead of us (K5b, K5c, K3c, K6, K7). Say if you want the levers pulled.
3. **Prorata basis semantics** (K3c): whether a prorata sale carries basis per unit or by exact share. K3c describes the two
   readings and what each changes, and decides neither.
4. **Two semantic choices K4b left open** (the only ones it could not settle from LANGUAGE.md):
   - *A fee leg of an exchange split* (`girokonto 900 EUR ->` with `fx-fees 4.77 EUR` and `us-checking 1_027.63 USD`).
     Today it is a payment of its own; `examples/08-expat`'s README says it is a **cost of the exchange** (basis 9,500.00
     USD, not 9,444.90). Which?
   - *A promise's `= AMOUNT` leg.* Built as: it moves the gap to the balance and the header's remainder is what is left
     (a 100.00 promise to `savings = 5_030 USD` at 5,000 moves 30.00, the remainder is 70.00). The other reading: the
     leg is not carved from the header at all (the header pays its end in full and the leg moves its gap on top).
5. **The examples' numbers moved.** K4b made the statement path mean what LANGUAGE §3 says (it created or dropped money
   before: see its table). 17 of 60 goldens change, all in `04`, `08`, `09`, `10`, which hold split statements. Every
   example still carries 36 to 461 errors (v3 syntax) and drops the statements that fail, so neither build matches the
   READMEs' hand-verified figures; K4b's own numbers are validated by 5,000 generated splits equal to the plain
   transfers they say they are. The examples themselves need migrating (lane L).

6. **K5b's grace change moves `07-landlord`.** LANGUAGE §7 says a due day is kept by the nearest occurrence within its
   `grace`, default **half the schedule's own cadence**; the code used the longest cadence of the contract (a month).
   `2025-12-29 manager-fee 200.00 USD` is 24 days after its due day, on a contract that ends 2025-12-31, so it now keeps
   nothing (`contract-occurrence-date`), does not post, and the 12-31 assertion on `rental-bank` fails by 200.00 USD
   (an extra error: 44 to 46). Options: write `grace 30d` on the contract, move the line, or accept. The book is not edited.
7. **The monitor's start.** It starts a stream at the book's first fact, so a contract with no `until` that outlives its
   subject (a loan on a house sold in 2025) warns `missed-occurrence` for every due day after; the monitor cannot tell a
   missing `until` from a forgotten payment. Writing `until` is the fix; whether the warning is too eager is a call.
8. **Claim recognition and two reserved words.** `books cash|accrual` is read by nothing; LANGUAGE §7 says default cash and
   income "when invoiced" in accrual books while `Books::Accrual` and §6 say "when it is due". K3d takes §7's (when
   invoiced) as one enum variant so the other is a line. And `claim` and `debt-claim` are now built-in kind words (a book
   that declared `kind claim` gets `duplicate-kind`): keep them or choose less common names.

9. **The forecast now pays what the book promises (K5c).** The old second ledger capped every contract flow with
   `within_means`, so a forecast paid only what the escrow held; the fold posts a promised occurrence whole, as it posts a
   kept one. `examples/05-family`'s forecast changes on 6 of 54 runs (the `Net worth` column only: `Committed` is the same):
   from 2026-02-14 the escrow holds 1,440.00 USD on 03-31 and the 04-10 property tax is 3,300.00 and the 06-20 insurance
   1,870.00, so the last row of the default horizon goes from +7,801.94 to -2,528.06. The old figure hid 3,730.00 USD the book
   promises. I read this as the forecast becoming right; say if the capping was a feature. No golden changed.
10. **The year-end forecast test cannot pass as written.** `a_context_forecast_keeps_historical_and_same_day_obligations_once`
    says `each year closing 12-31` and asserts a `year-end-tax` due 2027-01-15, but LANGUAGE §8 closes the 2026 year on
    2027-12-31, so it cannot exist in a forecast to 2027-03-01 (`why year-end-tax` says "Ran 0 times"). With `each year` the
    baseline gives the three asserted rows. K5c left the assertions untouched and added a passing sibling with `each year`.
    Options: change the original book's `closing 12-31` to `each year` (a test-input fix) or delete it as superseded.
11. **K5c's claim, and the habit forecast.** The claim a missed party-owed `due` makes is posted the day *after* the later of
    the occurrence's reach and its deadline (so a line within reach still keeps the occurrence and "oldest first" cannot
    settle the wrong claim); a brief reading made it on the day `due 5d` passes: one line in `monitor.rs::expect`. And the
    habit forecast (`expected.rs`, `variable.rs`, `recurrence.rs`, `bands.rs`: 368 lines, about 550 with what only they feed)
    is untouched: if it moves out the forecast is promises only (no "What recurs", no p10/p50/p90, a flat outlook where
    spending is not in a contract); if it moves to its own crate the report takes it as a list of flows to apply, the
    interface K5c left (`Ledger::promise_through` goes with it).

## What the grammar accepts and the engine does nothing with

Found by K5a's map (`K5a-map.md` §0, §1, §6) and K4b's: **written, checked, and read by nothing.** None of this is a
regression; it is what v4 left. K5d is the lane that makes them real, and each is a product decision about whether to.

| feature | what happens | where it stops |
|---|---|---|
| `deposit AMOUNT [into HOLDING]` on a contract | lowering checks the amount and the holding and reports diagnostics, then nothing reads `Contract.deposit` | no flow, no claim on the landlord |
| `match ...` (an employer's match) | `Match` is never set | the match is an `also` line, by hand |
| `loan ... resets EVERY from DATE to PARAM + PERCENT` | `Loan.resets` is written and read by nothing | the rate never resets |
| `loan ... prepay shortens \| recasts` | `Loan.prepay` is read by nothing | a flow to the contract is an ordinary flow |
| `loan ... for ASSET` | `Loan.asset` is read by nothing | no `of ASSET` on the interest |
| a loan's interest and principal | `Derivation::Interest` and `Principal` are never constructed; there is **no amortization**: a loan's payment is one level amount, and "Loan balance" is the debt tab's holdings, which only the journal moves | `#interest` and `#principal` do not exist as flows |
| `grace SPAN` on a contract | lowered, read by nothing: matching uses a full cadence (LANGUAGE §7 says its `grace`, default half a cadence) | K5b implements it as written |
| `due SPAN else ITEM` | lowered, validated, carried; no reader (the monitor does not exist) | K5b makes the overdue list, K5c the claim |
| `?` beside `...` in a split | `cannot-infer`; the remainder takes the whole total meanwhile | K4b limitation |

## K5c, in numbers

| | |
|---|---|
| what it is | `Ledger::promise` gives the ledger one `Residual` per stream (`engine/promising.rs`), a min-heap of due days and a sorted table of the days a line wrote; the fold takes promised occurrences at `Moment::after_flows` and posts them through the function that posts a kept one, tells the monitor, and records a `Planned`. The report reads `Recorded::planned`: no second numbering, no second `today`. A missed `due` the party owes the owner is a claim in the tab the owner keeps with that party (`Book::claim_of` replaces `paid_into`); `Term::Due.after` is used |
| deleted | `contract_forecasts`, `view_from/_with/_with_lens`, `project_runtime*`, `Promises::expected`, `RuntimeFlow::source`, the driver in `forecast/projection.rs` (now `trace.rs`: the reading code only) |
| lines | **+98 against a target of -900**: gross 590 deleted, 1,099 added (`promising.rs` 531, about 300 of them tests). The second driver was about 330 lines, not 1,200; no function was orphaned by the deletions. The trail was **not** used: `core::trail` is a log of `Copy` cells and the fold's state (`lots.rs` 2,015, totals 1,011, assets 1,175) is not that shape, so one clone at today replaces two or three (map §7) |
| speed | callgrind on `bench/100k`: `check` 1,860M to 1,867M, `forecast` 2,184M to 2,179M (a forecast costs 3.6% less beyond `check`) |
| behaviour | no golden or mistake changed; the forecast changes in the cases of Waiting on you 9; 59 of 600 generated promise projects differ in `forecast` (an overdraft is noted per occurrence; a leg reading a balance is read once), 40 of 600 differ where a party-owed deadline contract makes a claim |
| proof | a forecast oracle (400 projects: the forecast from earlier today equals the same book with the occurrences written down; 375 with occurrences, all agree on both layers); 30 mutants, none survives; K5b's promise oracle needed its reference taught the deadline (18 of 1,500 first disagreed; **an oracle edited to agree with the engine: reviewed, the reference reads the deadline from the terms as the spec says**) |
| left | claim recognition (K3d); the owner's debts stay plain balances; `else ITEM` read by nothing; `Ledger::promise_through` is an API for one reader; `Monitor` and `Promising` each keep a heap of streams (a generic is possible); `fuzz.py ... diff` shows nothing about valid books because every example already has `check` errors |

## K3c, in numbers

| | |
|---|---|
| what it is | `Policy::Exact` (a claim place relieves by the claim whose open amount is the flow's, the parcels of one transaction adding up); a flow's own codes name the claims it settles; `^code waived` is relief, recorded in `Run.written_off`; a tab has a built-in `claim` / `debt-claim` kind, so readers ask the place and not its role; **a payment from a party settles the claims on it** and a returned payment reopens them |
| a hole it found | **paying a claim did not settle it** (LANGUAGE §7, as written, was unimplemented): `ann owes me` 300, 200, 300 and `ann -> checking 300 ^i1` left all three claims open and the money counted twice. `04-freelancer`: 61,300.00 of paid invoices counted twice, one write-off ignored; Coming in 108,000.00 to 42,900.00 before K4b's statements landed |
| asset parts | **not parcels**: an improvement adds basis and no quantity, so a parcel per part is a zero-quantity lot and `lots.rs` would branch on "is this a part"; `PendingCarry` and `part_slots` also serve securities. The smaller cut for a later lane: a part's basis is `cost + carried - consumed` from `Run.adjustments` (the three guards go), `PendingCarry` and `part_slots` move to the lots, `Disposal` is a derivation of the position history (K7) |
| prorata | the failing test is **not about the sale**: `post.rs:278` gives no basis to money arriving in a `basis zero` place from a source that is not `deferred`. Two readings (map §5): **R1** keep it and write a basis on the test's flow; **R2** delete it, following LANGUAGE §9's text: the failing test passes, the HSA test fails, four goldens move (penalties and taxable income of `04` and `05`). Your decision; the line is untouched |
| lines | claims +312, assets 0 (`assets.rs` 871 lines stay: the verdict). 38 tests added, none deleted |
| proof | a claims oracle (1,500 books in 7 families, a Python reference of §7, 33 mutants: 27 killed by the oracle, 5 by unit tests, 1 by a test added for it); `verdict` 1,500 held |
| left | recognition (K3d); a debt is still a plain balance (K3d); a payment written as a split settles only what reaches the owner, so seven `04-freelancer` invoices keep the processor's fee as a remainder and `overdue` says `fernhill still owes 130.80 USD` of an invoice that was paid (K3d); a payment in another commodity is an exchange and settles nothing |

## K5b, in numbers

| | |
|---|---|
| what it is | `Contract` holds its `Terms` once and a `waived` timeline: K5a's invariant is the type. The fold, the lowering and the reports read `Promises`; `engine/monitor.rs` walks a `Residual` per stream beside the journal with a min-heap of miss days. A due day no line kept is a `missed-occurrence` warning (one per contract); `monitor_complete` is true; `open_claims` is filled |
| deleted | `Contract::{occurrences, due_days, amount_on*, recognition*, terms_on*}`, `nearest_occurrence`, `loan_payment`, the ordinal count, `sync/promise.rs`, `World.dues`, the dead `ForecastError` variants and `ForecastFeature`, `sync-monitor-incomplete`, `TermsState` |
| lines | **−209 against a target of −900**: the monitor is +177 and the type change touched every reader. The three long functions stay (`lower_occurrence` 403, `post_written_occurrence` 121) |
| speed | `check promise-no-from.ax`: **8.9 s to 0.008 s**; 100k `check` and `forecast` unchanged, 1m within noise (an out-of-line `covers` cost 18% mid-lane and was inlined) |
| behaviour | the native loan forecast test passes (a loan expects its payments and no more); `grace` read (07-landlord moves: see Waiting on you 6); 05-family and 07-landlord gain `missed-occurrence` warnings; a walked schedule with no `from` counts from 1970 |
| proof | the promise oracle against an independent reference: 0 failures over 45,181 windows, 30,926 ordinals, 339,756 probe days; every difference from the old rule has a cause (none unexplained); 55 mutants, 0 survived |
| left | lowering still matches a line on a one-contract compile at statement time (the fold validates the match); `Term::Due.after` is read by nothing (K5c uses it or deletes it); the `Late` cell and `why contract` late rows have no unit test of their own |

## K4b, in numbers

| | |
|---|---|
| what it is | `solve(header, legs, items, remainder, env)` in `model/solve.rs`: one algebra of a group over an `Env` (`LiteralEnv` at model time, the fold's reads at run time). `balance::settle` solves a statement whose amounts are written out and says what cannot add up (`split-imbalance`); `engine/statement.rs` solves an open group at its first landing; `engine/occurrence.rs` makes a promise's occurrence by the same `solve` |
| lines | **+858 net against a target of −1,500** (model +651, engine +206; `ledger.rs` −1,004). The brief was wrong in two ways: the three "copies" were not one algorithm, and the **statement path did not exist in the fold** (a statement's legs did not debit its source: split statements created money, 1,100 against 800 on a probe), so it had to be built |
| functions over 80 lines | 16 to 12; `post_journal` 249 to 29, `materialize_group` 324 to 65 |
| proof | 5,000 generated splits equal to the plain transfers LANGUAGE §3 says they are; 250,000 groups solver-vs-old-resolver; per-recipe books, 0 unclassified differences; 25 acceptance tests written from §3's own examples (19 before the code); mutants killed by those tests and the oracle |
| speed | `check` 1m: 4.17 to 4.14 s; callgrind +0.13%; unchanged |
| left | two `Env`s with different `lands`; exchange legs recognised by the flow's shape; a fee leg is a payment, not an exchange cost; the `bench/` projects never exercise splits |

**Behaviour changes** (K4b-map §11.2 has all thirteen with LANGUAGE.md line numbers): a split's legs debit its source;
`...` is the remainder; a total after the arrow or none (the sum of the legs) is read; items are carved from the header (a
paystub's `32.10` and `12.00` no longer debit 164.10 of a 120.00); `- 6%` and `2% of amount` compile and are of the
header; `all` as a leg or a header means what §3 says; a split that does not add up is `split-imbalance`, said at the
header and the legs, and nothing posts (488 of 2,000 generated books); a leg in another commodity is the exchange of the
remainder instead of creating currency (+96,000.00 on a probe).

## K5a, in numbers

| | |
|---|---|
| what it is | `core::Dues`: a schedule's due days as a set that is counted and indexed (`nth`, `before`), tiled schedules by arithmetic in O(log n); `model::promise`: `Term` (16 bytes), `Schedule` (44), `Residual` (24), `Annuity`, compiled once at the end of `build`; **nothing in the product reads them yet** |
| lines | **+966 now** (core +288, model +678) against **about 450 that K5b and K5c delete**, and `Terms` stored once removes more. The structure is larger than what it replaces until then; what it buys is below |
| proof | an independent reference; 1,500 generated projects, 2,329 contracts from 77 forms: 45,181 windows, 31,739 ordinals, 339,756 kept lines, 85,858 residual steps, **0 failures**; 50 mutants of the old code (46 killed, 4 argued equivalent) and 37 of the new (all killed) |
| what the old code gets wrong | eleven defects, each shown on a book (`K5a-map.md` §7): the ordinal is O(n) (a contract with no `from` costs 9 s per kept line); a loan forecast never stops; a due day is lost after a waiver on `on last`; `weekly on 15` lists a day four times; history and forecast number occurrences differently; matching sees the schedule as lowering painted it so far; `grace` is read by nothing |
| speed | no-`from` ordinal: 4.4-6.4 s to about 9 µs |
| left | not every schedule is arithmetic: longer-than-cadence `on`, clamping pairs like `on 30, last`, mixed kinds walk (O(n)); `Payment` clones the template (the book holds each promise twice until K5b); the compile's invariants are `debug_assert!`s: K5b makes them types |

## K0a, in numbers

| | |
|---|---|
| model lines | 17,034 → 16,604 (−430). The brief's −2,000 target was missed: the catalog and the single collect pass saved about 390, and the splits that followed added signatures and context structs. The gain is in function length: functions over 80 lines in `model` went from 38 to 4, and the four left are K3 and K4's |
| goldens | unchanged. Mistakes 26, 60 and 98 changed in wording only (`duplicate-*` codes unified, listed in the lane's commit 5271942) |
| panics | K0a turned some parser-guaranteed diagnostics into `assert!`/`unreachable!`. 2,700 mutated corpus and example files through the old and new binaries: none panics in either |
| budget on a rerun | a lane that changes the parser (K12) may violate those invariants. The mutation fuzzers are in `docs/v5/measure/` territory: rerun them after K12 |

## What K0b found, routed

- `check` on `examples/02-household` takes 24.6 s, from `Ledger::sample_temporal_through` sampling daily. K12 deletes it.
- `totals::History::read` is 6.5% of a 100k `check`, mostly an edge-block scan over about 62 facts. A prefix sum per day
  removes it (about 5%). A `fearless_simd` kernel on structure-of-arrays columns gave only −1.6%, below the bar, so it
  was not kept. Belongs to K7's position steppers.
- A smaller or interned `Diagnostic` in `core` would remove the boxed-error aliases the groundwork needed.
- The CLI and report render cells twice. K7.

## K3a, in numbers

| | |
|---|---|
| deleted | `lower::survey`, `find_tabs`, `TabDraft`, `Mention`, `JournalSurvey`, `EndpointContext`, `contract_endpoints` as a prediction, the `unregistered-tab` diagnostic |
| built | `Tree::push_root`, `Facts::grow`, `Builder::grow` (things that come to be after a freeze), holders numbered with places last, `World::tab` finds-or-makes, `rules::govern` after `record`, `Book::listing` for an explicit tab order |
| lines | model −351, total −332 against a −600 target: the implied-party walk stays (about 130 lines) and the replacement code is real |
| a real bug fixed | a loan paid from an account written by a short name (`joint` for `assets/joint`) gave `unregistered-tab` (11 of 60 generated projects) or, when another mention registered the key, split one debt across two tabs. The same books written with whole paths now report identically (0 differ, 125 before) |
| visible changes, listed | `check` counts the places that exist; `why LINE` names the first claim that asked; tab order is explicit in `balance`/`lots`/`claims`/`available`; a self-loan gets `contract-loan-party`; one golden (`04-freelancer-balance`) lists its tabs by name |
| proof | `tabs.py` (900 generated projects), K0a's harness, `splits.py`, 2,000 fuzzed mutants, 11 oracle mutants killed |
| for K3b/K3c | `Role` does not block `(owner, with, kind)`: delete `institution`, make `Holding` a unit, add `with` to `Place`, give tabs a `claim` kind. The implied-party walk is what K3b deletes; it must decide what an ambiguous suffix means when a second entity is created after the first was resolved |

## K4a, in numbers

| | |
|---|---|
| deleted | 13 types (`TemplateAmount`, `TemplateQuantity`, `TemplateFlow`, `TemplateLeg`, `TemplateItem`, `TemplateItemParent`, `JournalQuantity`, `JournalGroup`, `JournalItem`, `WrittenGroup`, `JournalEnd`, `TemplateProgram`, `JournalProgram`) |
| built | `Expr`, `Quantity` (what a side may be), `Part` (what a leg takes: `Of`, `Share`, `Rest`), `Group<Header, Flow, Item>` as `Promised` and `Made`, one `Program`. A header cannot be a share or a remainder by type |
| lines | **−481 against a target of −1,200**: the two quantity transliterations were smaller than assumed and `split.rs` adds about 250 with docs. `materialize_group` 600 → 324 lines; the bulk is in the three resolvers K4b merges |
| proof | a generator of split-heavy books (2,000 books, 55,722 commands) and an internals dump (1,543 promises, 22,636 forecast occurrences): zero differences. The oracle was mutation-tested: 38 mutants, 32 killed, 6 argued equivalent |
| found | `TemplateItemParent::Leg` was never constructed; `WrittenGroup.out/arrive` never set; `flow_roots` written and never read; a template index always 0 |
| tooling | `docs/v5/measure/splits.py`, `docs/v5/measure/internals/`, `K4a-map.md`. `fuzz.py` on `examples/` at `--today 2026-06-01` is vacuous for engine changes: every mutant is rejected by `check`, so use the splits oracle and the goldens |

## K12, in numbers

| | |
|---|---|
| what it is | kinds declare typed, counted, weighted slots (`has NAME RANGE [MULT] [by WEIGHT]`) in one `Schema`; a property line fills a slot and is checked once; kinds and purposes share one `Taxonomy` builder; every property is a fact in `core::facts`; the fold reads place, entity and commodity traits from dense arrays resolved at plan build |
| deleted | `Assign`, `Prop` rows, kind defaults, about twenty cached fields. `Kind` 192 to 64 bytes, `Place` 152 to 88, `Entity` 176 to 88, `Commodity` 96 to 36 |
| lines | **+517 net against a target of −2,000.** The sampling machinery stays for balance conditions until K7 (only residence is integrated), `props.rs` is still 1,182 lines, and the new files carry their tests |
| tests | 882 passed, 3 failed (the residence test passes now), 16 ignored |
| goldens, mistakes | byte-identical. Fuzzed 1,000 mutated example projects against the pre-K12 binary and diffed output: 13 differ, all in the intended diagnostics below |

**Diagnostics K12 changed on purpose:** `property-value` and `duplicate-property-value` on a built-in property are now
`too-many`; a required slot left empty is `missing-role` (`rental-home` has no `land` or `in-service`); a word outside a
closed set is `wrong-word`; the taxonomy's help text reads "write `: income`, …"; `has` is no longer listed among a
thing's properties. `days(self.lives is X, window)` counts the whole window as the book states it, future steps
included; the sampled count projected today's state forward.

**What K12 left, for a K12b cleanup:**
- `props.rs` does three jobs (the grammar of the built-ins, the staging of `has` values, the system settings), and
  built-in properties are not parsed by the generic fill path: that would delete the `Args` readers (about 250 lines).
- Two reader styles side by side (typed keys, and a dynamic `Value`), and `Book.sites` keyed by a bare tuple.
- Weights are validated but not stored (`owners dana 60%, theo 40%` keeps the members): K3 and K6 need them.
- The facts are frozen twice, because `end` statements say `closed` after lowering.

## Lane C3, in numbers

| | |
|---|---|
| kernel | broadcast block compare, `u32x8`, a 2 KB packing table, about 80 lines, no `unsafe` |
| speed | 0.95-1.05 ns per id against 2.0-2.8 scalar, at 10⁴ to 10⁶ ids, 1% and 50% shared (median of five runs, worst single run 1.39×) |
| the bar | 1.3×: cleared |
| gap | the AVX-512 path was measured on the old host (3-4×) but not with the final code: this host reports AVX2 |
| side effect | galloping now starts at skew 32 instead of 8, because the block merge beats it below that |

## Lane C2, in numbers

| | |
|---|---|
| lines | +507 non-test in `core`; `facts.rs` is 757 lines before its tests, 439 of them code |
| size | 26.5 bytes a step, 135 MB for 5.1M statements over a million holders; nested vectors: 61 bytes a step, 4.0M allocations |
| build | 275-550 ms on 4 cores for 5.1M statements; 1.4-1.7 s for the nested layout |
| `days_where` | 9-97x faster than sampling every day, and exact |
| a read | warm sweep 15.5-17 ns (nested 22-25); **cold random read 55-70 ns (nested 47-52): the layout loses 15-25% cold**, so per-event reads must be resolved at plan build |
| tests | 31 unit, 3 doc, a 2,550-book property test against a per-day model, ~20 hand-made mutants all caught |

## Operational note

Sonnet's session limit was hit once (about 08:40 to 10:40 UTC): lanes C2 and C3 died mid-work and were resumed from their
transcripts. Their worktrees kept their uncommitted changes. Keep at most three lanes running.

## Known gaps, not hidden

- `cargo clippy --workspace -- -D warnings` already fails on the baseline: eight errors in `core` files nobody in v5 has
  touched (`day.rs`, `calendar.rs`, `num.rs`, `tree.rs`, `unit.rs`). Nothing new was added by any lane.
- `trail`: a mark kept across an undo to an earlier mark, once the trail has grown past it again, cannot be told from a
  good one. The module says so. A list of checkpoints must drop the marks it undoes past.
