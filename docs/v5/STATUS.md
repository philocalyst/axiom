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
| **K3b** addresses | an account is written with the entities that fill its slots (`jordan/bluefin/401k`); `Addresses` is an inverted index resolved by posting-list intersection on the line's day; forced placement fills the slots; three new diagnostics | **merged** (`8e33a9d`) |
| **K5c** forecast | the forecast is the fold past today (`Ledger::promise`: a heap of due days, one `Residual` per stream); a missed `Due` the party owes is a claim | **merged** (`bdc25f9`) |
| **K3d** claims, recognition | a split payment settles by what the party pays in all; `books cash\|accrual` read once and one rule (`engine/recognition.rs`) says when a claim counts, asked by the fold and every reader; the default is cash; a write-off takes back each line for its own purpose. **Debts as parcels: not built** (design in K3d-map §6) | **merged** (`3f17468`) |
| K4c flows in columns | `Flow` (192 bytes) as hot columns and a cold record, a quantity as a tag and a payload, K4b's cleanup list | brief written |
| **K6b** the post host | a law of a kind, purpose, entity or account derives a flow from a posted one (cash back, a processor's fee), with a cause, a record, a cycle guard and returns that reverse | **merged** (`0b4bfe1`) |
| **K6** norms and relators | one purpose ranking (`classify`); the nine tables of laws are one `Rules` index keyed by `Watch`; a law may `derive` a flow or an item and a contract's laws fire with its occurrences; a contract's `also` and `share` are the laws they abbreviate; `kind X : contract` and `contract NAME : KIND` write a relator's legs once, true from both books (Layer 3 stops at the map) | **merged** (`17da806`) |
| **K7a** the `Session` | the library surface an MCP server and a GUI are written against; the CLI becomes a client | **merged** (`368e5e8`) |
| **K7b** facts out | the fold records every position's balance as steps (`engine/histories.rs`), so `balance --at/--monthly/--value` is a binary search; the postings count through one pivot; `why` asks one question of a target; the free-function path is gone | **merged** (`990ddb5`) |
| K3f debts as parcels | a bill you owe is a parcel on a Debt tab, a payment to the party settles it; `owed_by_you`, the `payable` gate and `makes_debt` go | **merged** (`fe06d05`) |
| K3e parcels in columns | `lots.rs`, `assets*.rs` (~2,500 lines): hot columns, an identity key, relief as a ranking plus a way of taking, asset parts if the smaller cut is a net deletion | brief written (after K3d, K4c) |
| **K5d** loans | a loan is one schedule walked once when the promises compile (`Annuity::step` over Pay, Prepay, Reset, Rate: pure, 40-byte state); a payment is a split of `#principal` to the debt tab and `#interest` (of the asset a `for` names) to the lender; `resets`, `prepay shortens\|recasts`, a rate written `DATE LOAN now at PERCENT`; a statement of the loan's balance is held to the schedule with the likely cause named. `deposit` not built (needs K3f) | **merged** (`34adb26`) |
| **K5e** a loan that began before the book | a loan made before the book's first fact, with no `opening` of its debt and no origination line, opens its debt tab on the first fact's day with what its schedule says is owed (`Book::first_fact` is now one definition; a payment due before the book began is no payment that was missed) | **merged** (`25566a5`) |
| **L1** the junction | one line grammar, `<-` and `@`, legs lead with arrows, `fmt --upgrade` ports every example; syntax only: the lowered book is identical | **merged** (`aeb624b`; the model check `error[junction-subject]` is its own last commit `9d375c8`, one `git revert`) |
| L2/L3 language, semantic | positions under their agent, debts as promises, optional counterparty, purposes without a direction root | after L1 and K6 (brief not yet written); lane U's plan may take them in (they delete model lines) |
| D the user's decisions | the small behaviour changes decided above: prorata R1, the exchange fee, the monitor's one warning, the claim's purpose, mortgage interest, `match` out, the examples | **merged** (`96f3e09`): items 1 to 7, 9 and 10; item 8 (the books: `07-landlord` `grace 30d`, `03-violations`, `11-sam`, porting `08`/`09`/`10`) **merged** (`Merge lane D, the books`): `03-violations`, `07-landlord` (`grace 30d`), `11-sam` corrected; `08-expat`, `09-shared`, `10-budgeter` ported to v5 |
| **U unification** | **the pass that brings the tree to the ceiling: one formulation per concept, every behaviour kept.** Plan `UNIFY.md` (49 unifications, C1 to C6, a second round C7). **C1 merged** (`48519a6`: -1,019 of -2,450 planned, 42%) and **C2 merged** (`f1baf39`: -214 of -1,450, 14%; `Diagnostic` behind one pointer takes clippy from 277 to 58 warnings): 57,109 to 55,938 lines. Lane U's own verdict (`UNIFY.md` section 13): **unification ends near 52,000; under 40,000 is a choice of features** (about 12,000 lines of sync, why/explain, the forecast, the v3/v4 readers, relators/addresses, diagnostics quality); under 27,000 removes user-written laws, contracts and loans, lots and basis too. **Running in parallel: lane U (Opus) C3 in `model`; lane U-E (Opus, `lane-unify-c4`) C4 in `engine`; lane U-C6 (Sonnet, `lane-unify-c6`) the report/cli/sync/core entries of C6 (K7c = U35, the view site, rows of flows, the cell writer, sync's memos)** | running |

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

## Decisions

The user's instruction: *"pick the most intuitive/capable option on all"*, and *"the unification pass is the most crucial
thing: I am okay with the new added lines, but it is crucial that we stay at or below target"*. Every question below was
decided on those two grounds (what a person writing the book would expect first, then what lets the book say more); the
numbers refer to the questions as the lanes asked them, kept further down. **Who builds each** is the right-hand column.

| # | question | decision | built by |
|---|---|---|---|
| 1 | `fearless_simd` source access | still the user's permission to give; nothing waits on it | no one |
| 2 | budget ceiling | **27,000 non-test code lines is a ceiling, counted by `docs/v5/measure/quality.py` (53,419 on 2026-10-04)**, aimed at 26,000; levers pulled as needed; behaviour and features stay | lane U |
| 3 | prorata basis (K3c R1/R2) | **R1**: a transfer from a taxed place into a `basis zero` place is a contribution with no basis (an HSA funded from checking is pre-tax, as people write it); a nondeductible IRA contribution says `basis AMOUNT`. The failing test's fixture gains a stated basis on its flow 2 | lane D |
| 4 | a fee leg of an exchange split; a promise's `= AMOUNT` leg | **the fee is a cost of the exchange** (it adds to the basis of what is bought, comes off the proceeds of what is sold), as `08-expat`'s README says; the `=` leg **as built** (it moves the gap, the header's remainder is what is left) | lane D (fee); none |
| 5 | the examples' numbers and errors | accepted; `03-violations`' stale "expect" comment is corrected, `11-sam` declares the purposes it uses; **`08`, `09`, `10` are ported to v5 by hand over `fmt --upgrade`** until `check` has no error that is the port's | lane D after L1 merges |
| 6 | grace moves `07-landlord` | the book writes `grace 30d` on the manager-fee contract; the default stays half the cadence (LANGUAGE §7) | lane D |
| 7 | the monitor's start and its eagerness | the monitor starts where it starts; **a run of consecutive missed due days of one contract is one `missed-occurrence`** (how many, since when, and the edit `until DATE` that ends it) | lane D |
| 8 | recognition and the reserved words | **a claim counts when it is made** (§7's "invoiced"; `AccrualAt::Due` stays one line); the doc of `Books::Accrual` and LANGUAGE §6 are corrected to say so; **`claim`, `debt-claim` and `principal` stay reserved** and LANGUAGE lists them | lane D |
| 9 | the forecast pays what the book promises | accepted: it is the fold | none |
| 10 | the year-end test | the original book's `closing 12-31` becomes `each year` (the spelling LANGUAGE §8 defines); its sibling stays only if it asserts something else | lane D |
| 11 | a missed occurrence's claim; the habit forecast | **the day after** (as built); **the claim carries the contract's purpose**, so accrual counts it when it is found and cash when it is paid. **The habit forecast stays** (what recurs, p10/p50/p90): it becomes a source of flows applied through the same fold (K5c's interface), and lane U shrinks it | lane D (purpose); lane U (the forecast) |
| 12 | recognition on `04-freelancer` | accepted (cash: gross receipts 74,800.00) | none |
| 13 | debts as parcels | **built** (K3f); `deposit` is Part B of it | lane U's plan decides: absorb or sequence |
| 14 | `--at` baseline wrongness; K7b's output unifications | the fold is right; **U1 to U6 are all built** (about -320 lines; U6's three goldens move) | lane U |
| 15 | the mortgage-interest deduction | **(c)**: the itemized deduction reads `#interest of ASSET` for a home-kind asset, and the asset's shares decide the personal part, so one interest posting never counts twice and any loan `for` a home works; `05-family`'s house interest is written `#interest` | lane D |
| 16 | a loan's last payment, `principal` | accepted as built (ACTUS) | none |
| 17 | what the grammar accepts and nothing reads | `match` **leaves the grammar** (its diagnostic names the `also` line that does it); `deposit` is built with K3f; `?` beside `...` stays `cannot-infer` and LANGUAGE says so | lane D (`match`); K3f |

**The gap, and what I am doing about it (2026-10-04, after lane U's plan, `UNIFY.md`).** Lane U read all 55,367 true code lines and
found that **27,000 is not reachable by unification with the features and the output kept**: its ledger (49 unifications in six
checkpoints, each byte-identical against a baseline binary) deletes 16,285 lines and adds 6,360, **net about -9,900**, landing at
**about 46,450** (44,500 by the old counter, which hid 1,948 lines of code: the count is restated). The proposal's 27,000 was a
clean-room estimate of a smaller feature set; the lanes since added about 4,400 lines of features (loans, addresses, recognition,
the session, the post host). I did not accept that as the end: lane U also does a **second round (C7)** over the unified tree, the
per-checkpoint report says planned against delivered, and **I pulled only what removes nothing you have**: generic-but-helpful
shape errors for declared lines (L-d, signature-derived, about -400), and, last and only if still above, the v3/v4 readers
(L-e, about -550). **Not pulled, because each removes something you use** (the cost is in `UNIFY.md` section 4.2): sync's importers
counted outside the budget (-2,300, a counting trick), the habit forecast (-430), the `why` pages (-850), `available`'s what-if (-300),
asset parts (-900), relators (-450), addresses (-1,100), help text (-350). With every one pulled it would be about 38,500; 27,000
needs whole features (sync, the forecast, budgets, loans beyond a fixed schedule, claims, rich diagnostics, about -11,450 more).
**This is the one decision the report to you must put first: features or the number.**

Lane D is one Sonnet lane of small, separate commits (`docs/v5/lanes/lane-D-decisions.md`). Lane U is one Opus lane in
phases, each ending in a merge (`docs/v5/lanes/lane-U-unify.md`).

## The questions the lanes asked

Kept as written: the reasons behind each decision above. None of these is waiting any more except 1.

1. **`fearless_simd` source access.** The registry source stayed unreadable to the lanes (a permission refusal) and docs.rs is
   blocked by the network policy; nothing was worked around. The API was recovered from our own scratch prototypes, and
   lane C3 built the `postings` kernel from them: no `unsafe`, 1.8-2.7× the scalar merge. If you would like lanes to be
   able to read the crate, allow `~/.cargo/registry/src/*/fearless_simd-*`.
2. **The budget ceiling.** The design lands at about 27,000 lines, with a floor of about 24,500 and levers to about
   20,000 (PROPOSAL §7). The tree is at about 57,000 true code lines on 2026-10-04 (55,086 by the old counter that stops reading a file at its first inline test module; K7b: -63, K5d: +725, K5e: +207, K6b: +568, D: +93, K3f: +70, L1: +980): the lanes so far built structure (K12, K4b, K5a add
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
5. **The examples' numbers moved, and most of their errors were one bug (K6).** K4b made the statement path mean what LANGUAGE §3
   says. I had put the examples' 36 to 461 `check` errors down to v3 syntax; **most were `purpose-disagreement`**: where a
   party's kind, a party, an account kind and a written purpose named different purposes the code dropped the flow with an
   error instead of taking the best ranked (LANGUAGE §2: first match wins), and the dropped flows made the assertions fail. K6's
   one ranking brings back 306 of the 308 such flows: **`04-freelancer` and `05-family` now `check` with zero errors** (449
   and 864 flows; net worth 70,222.47 and 46,463.25 USD), `06-investor` 36 to 3, `07-landlord` 46 to 18, `02-household` 12 to 1.
   **`08-expat`, `09-shared` and `10-budgeter` (454, 419, 281 errors) are v3 books** no ranking reaches: they declare chart
   accounts (`account income/salary-us : wages`, refused as `chart-account`), reach them with `via income/salary`
   (`unknown-place`) and write `/ party` the old way round (`v3-party`); lane L's `fmt --upgrade` ports them.
   `03-violations` line 60 pays a grocer `#rent` on purpose: the written purpose now outranks the grocer's, the flow posts, and
   the example's "expect: purpose conflict" comment is stale (the book is not edited). `11-sam` has 11 errors (it uses
   purposes `match` and `escrow` that nothing declares).

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

12. **Recognition, and what it did to `04-freelancer` (K3d).** `books` defaults to **cash**, as LANGUAGE §7 says, and gross receipts of 2025
    are now **74,800.00 USD from 27 sources, the README's own figure** (it was 158,672.40 from 42: every invoice counted when
    made and again when paid); federal income tax owed 25,970.50 to 2,073.22, SEP-IRA `available` 3,166.80 to 2,919.24; the seven
    invoices that showed a processor's fee as a remainder are settled (`fernhill still owes 130.80 USD` for an invoice paid in
    full is gone) and `claims` goes 12,467.10 to 11,800.00. The flip to cash is one line (`5da3aee`) with its goldens in the
    next commit, so it can be dropped. **§7 and the doc of `Books::Accrual` disagree** (income "when invoiced" against "when it
    is due"; §7's own contract paragraph says due): K3d built "when made" as an enum (`AccrualAt::{Made, Due}`, one function,
    `Due` untested) and corrected the doc. Other examples' `flow` moves too (`11-sam`, `explore-v5/01-agency`: payments no
    longer count as Unclassified, write-offs take back their purposes). **K5c's missed-occurrence claim still has no purpose**:
    giving it one is one line in `claims.rs::claim_missed` and one K5c test, and then accrual counts the occurrence on the day
    the miss is found and cash when paid.
13. **Debts are still plain balances.** A bill you owe is not a parcel, so a payment to the party does not settle it
    (`claim-debt-tab.ax`: `available` 857.50 against 665.00 when the `payable` gate is dropped). K3d stopped at the design
    (its map §6); it is the next claims lane and belongs before K3e (parcels in columns).

14. **Where the old `--at` balance was wrong (K7b), and a decision about output unifications.** The replay behind `balance --at`,
    `--monthly`, `--value` and `register` added up *flows*, so it could not see what the fold does to a place that is not a flow's
    end: **177 (book, position) rows across 21 books** where the baseline contradicted its own final state, in `K7b-baseline-wrong.tsv`
    with the day, the position, both numbers and the cause (a kept occurrence's template flows never counted: 94; a claim settled,
    forgiven or made by the monitor, tabs never relieved: 70; an asset counted twice, opening line plus holding: 13). First differing days:
    `02-household` checking 2026-01-01 **9,200.00 to 5,750.00**; `04-freelancer` brightwave 2025-02-12 -9,600.00 to -6,400.00;
    `07-landlord` lender 2025-02-01 3,842.30 to 5,651.89. The fold is right every time (an exact oracle advances the ledger a day at a time;
    the baseline's own plain `balance` on the last day gives the fold's number). No golden moved; 20 of 796 commands differ and only balance
    forms. K7b offered **five output unifications for a possible K7c** (about -320 lines in all; report would still be about 6,425 against the
    brief's 5,500): U1 `balance --value` without the "N flows have no price" note (-45), U2 entity/asset/contract registers as the place
    register's columns (-130), U3 `why asset:`/`why contract:` through the shared flows table (-45), U4 `why #purpose` limits/budgets through the limits
    and budget rows (-60), U5 `flow --by party` as the periods table (-40); each changes the bytes of the views it names (none changes a golden but
    U6, one `why` layout, which changes three). Say if you want any.

15. **A loan's interest now counts, and the mortgage-interest deduction is yours to decide (K5d).** `07-landlord`'s loan is `for house`, so its
    interest is `#interest of house`, which the rental law already counted: tax 2025 `rental-expenses` 11,495.08 to 28,682.62 (17,187.54 of
    interest over 11 payments: the figure the README's independent verifier gives), `rental-net` 13,229.92 to -3,957.62, `total-tax`
    16,315.89 to 12,534.62, **owed 1,735.89 becomes a refund of 2,045.38**; liabilities 16,917.95 to 14,200.00 and net worth 152,248.65 to
    154,966.60 (the principal of every payment now leaves the debt), `check` 18 errors to 7 (its eleven month-end statements agree with the
    schedule to the cent). The itemized **mortgage-interest deduction reads `#mortgage-interest`, which no loan writes**; options:
    (a) a loan `for` a home-kind asset writes `#mortgage-interest` instead (then the rental law stops counting it unless `rental.ax` changes too);
    (b) `purpose mortgage-interest : interest` in `us.ax`, so one posting counts for both; (c) the deduction reads `#interest of ASSET` for a
    home-kind asset and the asset's shares (03-triplex: 64.06% rentals) decide the personal part. The examples disagree today (`05-family`
    books the house's interest as `#mortgage-interest` and the car's as `#interest`). Not invented.
16. **A loan's last payment is what is left, as ACTUS says** (K5d): the old engine paid the level payment again and left cents on the debt;
    the legs add up to the old payment on every payment but the last (2,286 payments of 12 loans asserted). The differences are -3.23 and
    -0.02 on `05-family`'s mortgage (2053) and car (2028), +0.80 on `11-sam`'s (2054): no output reaches those dates. `purpose principal :
    transfer` is now a built-in name (a book declaring its own `principal` purpose collides, as `claim` did for kinds).

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

## Lane U, in numbers

C3, names, laws, values and diagnostics: **55,770**, −168 from the v5 head `f1baf39` (55,938); model 18,710 (−198), core
3,532 (+30: the cycle finder moved there), report one line changed. Built where an entry deletes about 40 lines or a concept:
U13 −225/**−113** (one `Compiler` constructor; the owner's currency read once; `Window::named`; a budget's computed limit
compiled onto its law's arena, so the offset copy and `BudgetLimit` go), U14 −105/**−30** (one spot lookup of the quotes:
`Book::rate`, which `convert` and `Lens::exact` ask), U15 −85/**−25** (one cycle finder, `core::tree::cycles`). Not built,
with their map counts (UNIFY §14): U11 −310/0, U12 −60/0, U16 −250/0, U17 −150/0, U18 −5/0: **−168 of −1,190** (14%).
Diagnostics as rows prototyped on `problem.rs`: the family grows by 35 lines, so not built, and section 13's −500 is withdrawn.
The evaluator read closely: `eval.rs`'s `Machine` is already the one walk (`fire.rs` is policy, `totals.rs` the window store
of U29, `calc.rs` arithmetic), so section 13's −400 is withdrawn too. Found: `Book::convert_for` and its five types (a
system's `rates` policy, 177 code lines) are called only by `model`'s tests; a decision. Byte-identical to `f1baf39` in
`diff/` (868 outputs), all 577 Books, goldens, mistakes and the declared-lines corpus, and in two books written for the
budget formula and the asset cycles; the release suite green; clippy 58 warnings (as at C2's end; `core` clean). With C1 and C2, 28% of plan: the tree
lands near **53,500**; distance to 27,000: **28,770**.

C2, one lowering of a line that moves value: **55,938**, −205 from the v5 head `9590f36` (56,143); model 18,908 (−212), core
3,502 (+8), session 501 (−1). Planned against delivered: the `Diagnostic` behind one `Box` (first, the coordinator's) +9,
clippy 277 to 58 warnings; U7 −1,130/**−198** (`Recording`, the context of a dated record, with its skeleton and its steps as
methods; the journal, the occurrence and the loan's origination on it; the contract's template and the law's `also` line keep
their own lowering); U8 −110/**0** and U9 −110/**0** (the resolvers and openings are as short as a table of them); U10
−100/**−16**: **−214 of −1,450** (14%). Byte-identical to `9590f36` in `diff/`, all 577 Books, goldens, mistakes and the
declared-lines corpus; the release suite green. With C1, 32% of plan: the ledger's rest lands the tree near **53,500**;
distance to 27,000: **28,938**. UNIFY §13 says, for the user, what gets under 40,000 and what under 27,000.

C1, one site, one tail, one property reader: **56,143**, −966 from the v5 head `bce9735` (57,109); model 19,120 (−972),
syntax 6,438 (−4), core 3,494 (+10, phase 1a). Per entry, planned (with L-d) against delivered: U4 −960/**−437**, U3
−200/**−82**, U2 −270/**−150**, U1 −840/**−350** (the world keeps its diagnostics, −259; a statement is lowered in its
context, −91), U5 −100/**0** and U6 −80/**0** (neither pays on reading; UNIFY §11 says why): **−1,019 of −2,450**. Added: the
Book dump's derives (+21) and the `purpose-of-its-own` hint (+26, the coordinator's `grant-purpose` finding). Against
`bce9735`: 107 `diff/` outputs (86 declared-line errors in other words with the same codes, three share cascades said once,
18 `contracts` tables without "kepts"), 104 Books (those diagnostics, measure codes in the pool, one ending's interning, the
hint), three goldens/mistakes (the hint, in 02-household and 03-violations too: they write `purpose education` on the
scholarship grant); everything else byte-identical; the declared-lines corpus 208/208 on both; the release suite green.
`bench/` against `bce9735`: `check` at 1m 5.01 s (median of five interleaved runs) against 4.95 s, 672 MB both; every other
command within the machine's noise (±15% both ways), memory the same. The bench books exit 1 on both binaries (1,022 errors
at 100k, 764 of them `assertion`), and their grants write `purpose pN-edu` (the hint now says so 24 times):
`bench/nativeize.py` is behind the language. Revised landing: about **51,000** (UNIFY §11); distance to 27,000: **29,143**.

Phase 1a, the instruments (`claude/great-wozniak-pnqn7x-v5-unify`; the plan is `UNIFY.md` there). **The count is restated:**
`quality.py` used to stop reading a file at its first inline test module, so the 1,948 lines of code after one were not counted
(`law.rs` 498, `book.rs` 374, `journal.rs` 369, `calendar.rs` 328, `declare.rs` 205, `problem.rs` 97, `sync/world.rs` 74,
`peg.rs` 3); it now skips the module to its matching brace. **Baseline: 55,367** at `a21eee9` (53,419 by the old counter):
model 19,777, engine 12,463, report 6,804, syntax 5,634, sync 4,339, core 3,484, cli 2,350, session 502, systems 14.

The instruments, built: `core` is clean under `clippy -D warnings` (the workspace beyond it has 238 warnings, 168 of them
`result_large_err`); **the declared-lines corpus** (`measure/diff/declared.py`, `cases3/`: 208 books, every error path of
the readers U4/U5 replace, 68 codes, all raising what they claim on a baseline binary of `b3b98fc`); **the relief model
test** (`engine/src/lots/tests/relief_model.rs`: 3,000 seeded holdings, every policy, ties, selectors, merges; today's
relief agrees everywhere) with **fourteen mutants, all killed** (`measure/u/relief_mutants.py`); **the relief books**
(`bench/gen.py --relief`: 100k lots and 50k sales, `check` 0.69 s and 128 MB with HIFO; pro rata 2k and 1k, 0.27 s;
load 5.6). Found: `currency USD` cannot be written (the reader wants a name, a currency is a unit). Running total
**55,376**.

## K5e, in numbers

| | |
|---|---|
| what it is | `model/lower/loan_opening.rs`: a loan whose `on` is before `Book::first_fact()` (the one definition now; it left the engine's timeline), with no `opening` line naming it and no `DATE NAME` line originating it (even a rejected one), opens its debt tab with `Promises::owed_before`, the schedule's balance before that day (the lowering has the terms, the rates already said and the reset indices; nothing the journal does to the loan precedes the first fact). The flow is out of the debt tab against the opening entity, `Mode::Opening`, `Origin::Derived(Derivation::Opening(contract))`, lowered right after the record that makes the first fact; **dated on the first fact's own day, not the day before** (a flow before the first fact cannot be placed in the day-sorted arena, and one dated the day before would move the first fact and the monitor's start; the amount is still the balance before that day). `note[loan-opening]` once per loan names the loan, the day, the amount and the line that overrides it (`mortgage 312_441.12 USD` in an `opening`: a debt is written positive). A book with no fact opens each loan on the day it was made (and the monitor then watches from there: one `missed-occurrence`). `Amortization::explain` no longer names a payment due before the book as missed |
| lines | **+207 non-test** (model +213, engine -8, report +2); `loan_opening.rs` +154 (the map priced it at 110: the rejected-origination pre-scan, a `Begins` enum, and the edit placed as a line of the opening) |
| behaviour | no golden or existing mistake moved (four new mistake books 112 to 115); `11-sam` and `v4-sketch` print the debt they own (311,345.99 owed on `rocket` after three kept payments; net worth is negative because the condo has no price in the book); `explore-v5/02-family` 134 to 130 errors and `03-triplex` 66 to 65 |
| proof | 7,500 generated books whose first fact is after the loan's day (first fact an opening, a line, a flow or a statement; a user opening that agrees or differs; rates said before the book; resets; both prepay modes; missed payments; 67 loans paid off before the book) agree with the independent reference, 0 differ (the baseline differs on 4,050); K5d's 4,500 books still agree; 31 mutants, 30 killed (16 by the oracle, 14 by tests), 1 equivalent; fuzz 3,000 mutants of examples 04 to 10: 0 differ; `splits.py` 27,690 commands 0 differ; `claims.py` identical; `check` on `bench/` 100k 0.318 to 0.310 s, 1m 2.860 to 2.882 s |
| left | **a loan made in the book with no origination line** (`02-family`'s `mortgage-2` and `car-loan`: the tab stays at zero): proposal `warning[loan-not-originated]` with the origination line as the edit, not an implied flow, because where the cash arrived is a fact only the book knows; two inputs the lowering's walk cannot see (a late line that is itself the first fact and keeps a payment due before it; a waiver dated on or after the first fact for a due day before it); the opening follows the record that makes the first fact on the same day (right because a day's balances do not depend on the order of its flows, an ordering argument and not a structural guarantee); `Promises::owed_before` and `Promises::compile` are two walks of one loan sharing `walk`; **`Insertion` reads bytes** to word an edit (a code action belongs in the diagnostic's own vocabulary so an editor, the MCP server and the GUI can place it: not built) |

## K5d, in numbers

| | |
|---|---|
| what it is | `Annuity::step(State, Event) -> (State, Paid)` for Pay, Prepay (shortens: the number of payments a payment needs is the loan's own recurrence stepped, no new rounding site; recasts: the payment refigured), Reset (`index + margin` held by the caps) and Rate (a statement's number): pure, total, no allocation, 40-byte state asserted, the four rounding sites named in the module doc and each tested. **The schedule is walked once, at `Promises::compile`**, over every event the book states in day order (reset, rate, payment, prepayment), into a flat pool of 32-byte entries that the occurrence, the monitor, the reports and the statement check all read, so the fold, the monitor and the forecast cannot disagree; `Residual` shrank. A payment is a K4b split: header `#principal` to the debt tab, leg `#interest` to the lender, principal the remainder. `value LOAN = X` is held to the schedule: `error[loan-balance]` names both numbers, the payments around the day and, **when exactly one candidate explains the gap to the cent**, the cause (a missed payment, a short one, an extra counted as principal, an unrecorded prepayment) with the edit that mends it; a tie or no cause says so. `DATE LOAN now at 6.25%` was silently dropped (and `unknown-property` on a contract named like a kind): now `Contract.rates`, and `contract-rate-change` on a contract with no loan |
| the finding | a loan's payment never touched its debt tab: `07-landlord`'s twelve statements all failed (11 of its 18 errors) |
| lines | **+725 non-test** against a planned +350 (engine +209, model +468, report +48): `loan_balance.rs` 191, `amortization.rs` 187, `annuity.rs` 115, `causes.rs` 75; it deleted 13 lines, because the old path was a 91-line `Annuity` and a cursor with nothing to remove |
| speed | `check` on `bench/` (no loans) 100k 0.358 to 0.362 s, 1m 3.811 to 3.912 s (nine more interleaved runs: 3.906 vs 3.827 s fastest, 4.245 vs 4.107 s median): noise; RSS 665 MB both |
| behaviour | see Waiting on you 15 and 16: only the six `07-landlord` goldens moved; `05-family` check and balance identical (its `contracts` view shows the schedule's balance, `forecast` balances move monthly); `explore-v5/03-triplex` gained four `loan-balance` errors because its contract says "first payment 2022-10-01" in a comment and wrote no `from`: **fixed in the book (one line, `from 2022-10-01`); its four statements now agree to the cent** and `check` has 66 errors; `11-sam` and `v4-sketch` print **negative liabilities** (their loan predates the book and nothing opens the debt: lane K5e) |
| proof | `loans.py`, an independent ACTUS annuity in Python integers over 8 cadences: 4,500 projects, 0 disagreements on every payment's interest, principal and balance, prepayments, month-end balances, what kept lines post and what the forecast promises, the monitor's missed days and the 1,704 `loan-balance` diagnostics with the cause named; the K5b promise oracle ported to the schedule: 800 projects, 0 failures; fuzz 5,000 mutants of books with no loan, 0 differences; `splits.py` 74 of 1,000 projects differ, all with a loan; `claims.py` byte-identical; **62 of 80 mutants run**: 39 killed by the oracle, 14 by named tests, 9 survived of which 7 are now killed by tests written for them, 1 equivalent, 1 dead code deleted; **19 mutants (causes, the occurrence's reading, the lowering split, reconcile, loan_balance) are written and not run** |
| left | a loan that predates the book (K5e); `deposit` (Part B: needs K3f); the mortgage-interest choice (Waiting on you 15); **`engine/loan_balance.rs` is 191 lines of prose in code and the oracle learns the cause from the wording of a note** (the structured disagreement should travel in `Run` and the report should word it: K12b); a short payment is the book's and not the schedule's (`Cause::Short` papers over it: `contracts` shows 404,691.86 for a mortgage with no payment kept) |

## K6b, in numbers

| | |
|---|---|
| what it is | one `post` that posts a flow, fires the laws that watch it, and posts what they derived by the same path: derived flows are queued **by value** in a `Brood` stack drained by a loop (`post` never calls `post`), each with a `Lineage` (a fixed `[Id<Law>; 8]` plus a length, passed by value; the only way to a longer one is `Lineage::then`, so the depth bound and the cycle check are one call and cannot be left out). A law does not watch its own offspring (`Stopped::Own`, silent: the card example in LANGUAGE would be a cycle of one otherwise); `derive-cycle` and `derive-depth` name each law in order and the flow that began the chain, once per `(lineage, law)`. A returned flow posts what it derived backwards, in posted order, and derives nothing. `also` and `on flow ... derive` are laws under every owner (kind, account, entity, purpose, asset); `Table::Touching` / `Watch::Touching`; `Stand::{Flow, Subject, At}` for the ends of a derived flow; `Cause::Derived(Id<Offspring>)`; `Derived::flow_from` is the one flow constructor of both hosts (K6's occurrence host and this) |
| lines | **+568 non-test** (engine +250, model +188, report +130); `offspring.rs` 306; about 705 more lines are Rust tests; `derived.py` about 700 lines of Python |
| report | a derived flow is a posting after the flow it came from (`all_postings`; `postings` stays the journal's): register, flow, balance, tax and `why` show it, and its origin says "derived by the `also` of kind `card` from FILE:LINE" |
| behaviour | goldens and the 213 existing mistake outputs byte-identical (five new mistake books 105 to 109: `derive-cycle`, `derive-depth`, `derive-posted`, `law-never-fires`, `selector-owner`). K6's three `also` probes (`g-also-account`, `-entity`, `-kind`) now fail `unknown-purpose` for `#fees` declared nowhere: the `also-inert` warning is gone because the `also` now fires. An `on flow` law under an account that holds several commodities needs `value(amount, USD)`. One fuzz mutant differs (a hand-written `on flow` law under an entity: was `law-trigger`, is now a law whose amount has no one unit) |
| proof | `derived.py`: 600 projects (an `also` book, the same as hand-written laws, the same with the derived lines written out, and the `also` book with every occurrence kept), an independent Python reference of firing order: CLI layer 0 differ, dump layer 0 differ (25,961 flows posted, 4,462 derived after today, 1,588 derived from promised occurrences); 208 cycles and 51 depth stops equal to the reference's counts; **40 mutants, 37 killed, 3 survived of which 2 are equivalent and 1 now has a test** |
| cost | a book with no kind-level law pays about 50 instructions per flow (+0.29% on 100k, callgrind) and 2 to 4% more user time; wall time inside noise (100k 0.412 against 0.444 s, 1m 4.195 against 4.220 s) |
| left | a derived flow settles nothing (no claim); `explain.rs` names journal flows only, so a derived flow counts toward a limit without being named; `why ^code` and `why "description"` list lines, not derived flows; a written line replacing a derived one (LANGUAGE §10) and a party `share` claim are not built; `derived.py`'s mask (notes, codes, source lines) is where a derived-against-written difference could hide; `Derivatives::after` collects a `Vec` per journal posting in `all_postings` (a report-only allocation lane U can take); `stopped_chain` matches the same enum three times (a method on `Unbounded`) |

## Lane D, in numbers

| | |
|---|---|
| what it is | the user's decisions built, one commit each: prorata R1 (the test states `basis 6_300 USD`, LANGUAGE §9 says why); the exchange fee (`exchange_costs_of`: the `Less` items of an exchange header and the spending legs beside the exchange leg of a split cost the exchange; it replaced `is_exchange_cost`'s 30 lines of checked arithmetic); the monitor's one warning per run of missed days (how many, since when, `until DATE` as an edit for the last run of a contract with no end that is not a loan); a monitor-made claim carries the contract's purpose (it needed more than one line: what settles a claim reads its purpose from the line that made it, and a monitor-made claim has none, so `recognition::claim_purpose` reads the occurrence's template header); `Kind.owners` (`owner taxpayer` on a kind, `me` always may own) so `acme/529` and an `owner acme` line on a 401(k) are refused; `unknown-address` suggests an address; `currency USD` can be written (`Args::unit`); the two kind-total tests pass in debug builds; the year-end test says `each year` and its sibling is merged into it; the itemized deduction reads `#interest of ASSET` of a home (`05-family`'s 12 house-interest lines become `#interest of house`) |
| lines | +93 non-test (engine +25, model +68); item 9a +60, item 3 +35 |
| behaviour | 05-family and 07-landlord `check` goldens (the missed-occurrence headlines say "since DATE"; 05's law count 23 to 24); mistakes 112 to 115 (headline only), 102 now says `too-many` (two employers before a 401(k) can no longer be owners; 122 keeps `ambiguous-placement` demonstrated); new mistake books 116 to 122; `tax 2025` byte-identical for 05 and explore-v5/06 (the home law counts what the old law counted: itemized 48,885.07, mortgage interest 24,077.32) |
| what the code showed the brief got wrong | `match` was already gone from the grammar (nothing done); `Books::Accrual`'s doc was already right, §7's contract paragraph was the stale text; "rental and personal share add up" is not buildable here (the engine walks `part of` upward and never divides a flow of a whole building by area: a mixed building writes the interest of each unit); the exchange fee counts only if the split's source pays, another leg exchanges and the leg's purpose is a spending: **`08-expat`'s `fx-fees` leg needs a `#fees` purpose when item 8 ports it**, or the README's 9,500.00 will not appear |
| proof | goldens and mistakes byte-identical but for the lists; `diff/`: 31 of 552 differ, all missed-occurrence; fuzz 2,000 mutants 0 panics, differences only the headline, the law count and `owner` listed among a kind's properties; `splits.py` exchange 400, split 600, statements 500 projects 0 differ, `equiv` over 500 splits (75 with fee legs) 0 not the plain transfers; workspace 1,322 tests, 0 failed |
| left | item 8; item 9a's design (a side table because the facts builder is write-only when placement runs: lane U rewrites those passes); item 2 not checked on the real `08-expat` |

## K3f, in numbers

| | |
|---|---|
| what it is | a bill (`me owes pge 142.50 USD ^b1`) is a parcel the owner owes: a **negative** parcel of a debt place that says `claim`, made in `relieve_balance` by `Ledger::owe`; a payment is recognised in `settle.rs` as `paid_by_party`, `paid_to_party` (the owner's money into the place of a party the owner owes) or `paid_into_debt` (into a declared payable), and relieves it in the same order as every claim (code and selectors, the exact amount, the oldest). Relief of a debt reuses `lots.rs` through `Slot::owes` and `mirror()` (the slot is negated, relieved as a positive holding and negated back: about 25 lines, no second relief). Recognition mirrors K3d with the direction `Out` (`claim_dir`: Debt out, Asset in; a bill with a purpose is spending when made in accrual books and when paid in cash books; a forgiven bill reverses in accrual). A loan and a credit card stay plain balances: **told apart by the tab's kind** (bill tab: root `debt-claim`, says claim; loan tab: root `debt`; declared card and loan kinds are `debt`, no claim); `World::tab` is keyed `(party, owner, kind)`; a returned payment into a declared place opens its bill again and a credit note into it is its spending refunded; a law reads `open(^b1)` as what is owed. **Deleted:** `owed_by_you`, the `payable` gate of `claims.rs`, `Book::makes_debt`, `Class::holds_parcels` |
| lines | **+70 non-test** (engine +123, model -3, report -50): U's budget exactly |
| behaviour | one golden: `07-landlord-balance` (the roof's invoice, 14,200.00, was counted twice, as a `summit-roofing` balance and as a liability beside the checking that paid it: liabilities 14,200.00 to 0.00, net worth 154,966.60 to 169,166.60); `10-budgeter` does not move (its bills are contracts); mistakes byte-identical; across all examples and ten reports at two days only `01-agency` (irs balance 11,893.50 to 84.00, net worth -20,082.10 to -8,272.60, `available` gains "Due within 30 days", `claims` lists open bills) and `07-landlord` move; the probes: `claim-debt-tab.ax` claims one row ^b2 50.00 (was "Nothing owed"), `tab-implied-parties.ax` lists `me owes zorb 12` as owed by you |
| proof | `claims.py` 600 books (107 with debts: paid, in parts, by code, returned, forgiven, payable, loan, deposit): 0 failures against the references (the baseline fails exactly the 107); 94 mutants, 43 rerun, 32 killed by the oracle, 5 by tests, 6 survived and answered, final none survives; fuzz 1,000 0 differ; `splits.py` 300 projects 8,413 commands 0 differ; 1,336 tests; **perf: 100k 0.462 to 0.458 s, 1m 4.772 to 4.441 s, callgrind -0.5%** (the first version of the gates cost +0.4% instructions because the bench's flows are on Debt-class card places: the second reads traits only for Debt ends) |
| left | a payment to someone who is lender and biller at once (a loan schedule's interest and principal are flows to the bank's place, so with an open bill from the same bank they settle the oldest bill first: needs the contract's name on the flow); `lots.rs` has a slot that is negative for one kind of place (revisit in K3e/U's C4: `Slot` grew an unaligned bool); `claim_dir` is a function of tab class (a third class needs a third arm); missing-bill monitoring (`claim_tab`) is still party-side only; **`deposit` (Part B) is not built** (the brief did not ask) |

## L1, in numbers

| | |
|---|---|
| what it is | one production reads a dated line (`transaction` and `statement` merged, `journal.rs` folded into `statement.rs`), the word after its subject looked up in one table keyed by the token (`PUNCTUATION`, `WORDS`); `<-` is a take, `<- A @ P` a buy, `-> A @ P` a sell, legs lead with their arrow, `me <- acme 5_200 USD` with `->` legs is a split through an owner (`Course` on the AST: the junction and the owner in one word); the formatter has one set of columns; v4 text reads unchanged with **one** `warning[v4-syntax]` per file counting four forms (a bare leg, two amounts, an amount before an arrow with only legs, a subject-less `->`); `axiom fmt --upgrade` (in `syntax` behind a `Registry` trait that `cli` supplies from the book: the map had it in `cli`) writes a v4 book the v5 way, **checks that the book says the same before it writes**, refuses what it cannot place (`upgrade-owner`) and a two-amount line whose price nobody would write (`upgrade-price`, with the shortest price that agrees), and writes the rest; the model reads one thing of a junction: `error[junction-subject]` for a `<-`, purchase, sale or split through an owner whose subject is a party (a v4 `->` from a party is unchanged) |
| lines | **+980 total (the tree grew)**: syntax +804 (`upgrade.rs` +316, `legacy.rs` +78, the grammar and its diagnostics +410), cli +137, model +39 (the check). `Txn` is 136 bytes (was 128 in v4; the first AST made it 160) |
| examples | every example, fixture and generator written v5, line counts identical, bytes +6 to +18% (arrows on legs, columns); **19 two-amount lines written by hand with the price the upgrade names** (listed in `6d03832`'s message: `06-investor`, `11-sam`, `v4-sketch`, `explore-v5/04-nomad`); `tests/v4-syntax/examples/` keeps the v4 text of each example, which the proof test upgrades and compares |
| behaviour | goldens: 28 files change **only in the source lines a diagnostic quotes**, plus one real change (`03-violations` gains `warning[v4-syntax]` for a two-amount line it keeps as v4); mistakes regraded in one commit (quoted lines; `expected-arrow` says "`->` or `<-`"; `many-to-many` gains the help that names the split through an owner; `missing-legs` becomes `exchange-no-price` with a fix); new books 116 to 126 (lane D's, which collided, are renumbered 127 to 133) |
| proof | `crates/cli/tests/upgrade.rs`: each of examples 01 to 11 as v4 upgrades to the example as written now, and `check`, `balance` (three forms), `flow`, `claims`, `tax` say the same through both, upgrade idempotent; `junction.py` (seeded generator rendering every line shape in v4 and v5): 120 books 0 differ, 0 upgrades differ, **mutants swap-sides 112, drop-arrow 90, flip-exchange 58 killed, 0 survived**; I re-ran it on the merged binary with another seed: 60 books 0 differ, same mutants killed; fuzz 1,200 mutants 0 panics (204 differ in five known classes, all the warning or the new wording); `splits.py` 8,318 commands 0 differ |
| speed | the parser is **not** instruction-neutral: +8.4% on the counting bench (small inputs about +10%, 30,000 items +0.4%); `check` of 100k flows +0.9% instructions; 1m wall within noise (4.77 against 4.72 s, user +1.2%); RSS 681 to 689 MB (+1.1%) |
| left | syntax did **not** shrink and the parser still makes three passes over a split's legs and about three punctuation lookups per flow line (lane U's C6 should take them); `File::arrow_after` finds the model check's arrow by re-lexing the source (storing it would cost a word per flow); fixtures upgraded by a scratch heuristic (it got one wrong, found by the model check) and test helpers that now tolerate `v4-syntax` can hide one; the model check does not cover `also` lines or a contract's templates; **item 8 of lane D (the books) must edit `tests/v4-syntax/examples/` as well as `examples/`, or the proof test fails** |

## Lane D, the books, in numbers (and what porting `08` to `10` found)

| | |
|---|---|
| what it is | `03-violations`' stale comment; `07-landlord`'s `grace 30d` on the manager-fee contract; `11-sam` declares `purpose match : contribution` and `purpose escrow-deposit : transfer` (its contract line is `also -> escrow 410 USD #escrow-deposit`: `#escrow` collides with the account `escrow`); `08-expat`, `09-shared`, `10-budgeter` ported from v3 to v5 by hand (their v4 copies in `tests/v4-syntax/examples/` hold the hand-ported text, so the upgrade proof test needs no special case). No crate code changed; the workspace has 1,381 tests, 0 failed; the goldens that moved are 07 (the missed-occurrence lists carry the run) and 08, 09, 10 (each was about 600 lines of v3 errors) |
| the READMEs | `10-budgeter` and `09-shared`: every README figure agrees with the book and with the independent `verify10.py` / `verify09.py` (10: net worth 10,653.04, tax 447.92; 09: claims 4,100.00 and 100.00 on the two days, wages 40,192.89, net worth 22,917.53); `08-expat`: federal, California and FBAR figures agree (FBAR 38,927.16, stacking tax 3,463.56), ten figures move by 1 to 3 cents (a German pay is one flow converted once at the day's rate, not legs converted one by one: 6 x 5,400 EUR is exactly 37,564.56), and `available` is 70,259.47, exactly what the README's own sentence lists (its 68,663.47 came from the old engine; the 1,596.00 is unexplained). The exchange fee works: `lots girokonto` shows basis 9,500.00 against value 9,444.90 (`wise-fees` carries `#fees`). `gains 2025`'s total is -39.68 (README: -3.27; with the fee excluded from the basis +27.34: neither reproduces it), with 18 extra zero-gain lines for what leaves the clearing account the day it arrives |
| **what the language says and the engine does not (carry these into the final report)** | (1) `-> party due` (LANGUAGE §3) makes no claim: only `X owes me`, `me owes X` and a flow with `due` into a declared receivable place do; (2) a value cannot be asserted of a tab (09's 43 month-end `by-cleo = ...` lines are gone: `claims` is the reconciliation); (3) a write-off is whole only (Riley's forgiven 200 of 250 is her payment plus a gift of the same money); (4) `#loan` is not a built-in purpose; (5) **a paystub written as a split through the owner counts as wages only what lands in an account** (7,219.80 of a 10,416.67 stub, no pre-tax), so 08 and 10 write the gross wage and the withholding as outflows, the `05-family` pattern, and 08's German pay passes through an empty `de-clearing` account because the FBAR law and the currency disposals read every intermediate balance (without it FBAR is 40,801.89): a candidate for L2/L3; (6) a contract paid `from` a fund projects without capping at what the fund holds, so `10-budgeter`'s `Committed` moves 500.00 where its README says 1,500.00 (written as 350 from the fund and 1,500 from checking it moves exactly 1,500.00): the README's economics are right and this is the consequence of Decision 9, say if the cap should come back for a `from` fund |
| small bugs found | `contracts` prints "0 kepts"; `10-budgeter`: `check` warns "297.01" for October fun while `budget` says 323.02 (the warning leaves out the month's last 26.01 flow); `purpose education` on a grant is read as the party's own purpose and leaves the grant law with no purpose ("`grant-purpose` is not set"): 02-household, 03-violations and `tests/mistakes/70-grant-wrong-purpose.ax` write it that way, the std property is `grant-purpose`; `examples/verify/verify11.py` fails at the head (`straight-line` is gone from `std-sketch.ax`); `07-landlord` has 5 assertion errors at the head (`deposit-bank`, `deposits`, `bills`: `deposit` is not built) with and without the grace; `11-sam` has 9 errors none about purposes it declares; `outputs/` folders of 04 to 10 are legacy recordings (the READMEs name figures instead); unused imports in `report/src/why/contract.rs` and `cli/src/table.rs` from the K5d merge |

## K7b, in numbers

| | |
|---|---|
| what it is | **A**: `engine/histories.rs`, a recorder in the fold at the one choke point (`Holdings::entry`/`scale`, drained once per fact by `Ledger::record_balances`): per position (a place's holding of one commodity) the days its balance changed and what it was from each, laid out as columns (`days`, `balances`, an offset per position, a counting sort at freeze; places are in pre-order so the positions beneath a place are one run and a subtree's balance is a slice sum); `Steps::extremes` builds a sparse table for a window's peak or low (no consumer yet). **B**: `report/pivot.rs`: the postings count through one grid (`Counted`), the purpose and party flow views and `why #purpose` are its rows. **C**: `why` is one `Target` and one walk; what a line caused is read in one pass. The free-function path (`report`, `report_with_sources`, `views`, `holdings_at`, `available::view_with_lens`, `Context::new`, `Past::Journal`) and `Snapshots::replay` (116 lines) are deleted |
| speed (in-process, `crates/session/examples/scrub.rs`) | `balance --at` 6.24 to 0.053 ms at 100k (117x) and 78.2 to 0.48 ms at 1m (164x); `--value` 112x and 189x; `--monthly` 15x and 31x. **Through a `Session` it is 1.5x** (21.4 to 14.1 ms at 100k, 254 to 160 ms at 1m): `Session::query` builds a `Plan` per answer, 14.6 ms or 161 ms, which is now the floor. The lever is known (`Standing`: what `plan.sides`/`known` read) and not built. One-shot CLI: `check` +1.1% instructions (the recorder), RSS +0.4% at 100k and -0.02% at 1m; the histories are 1.4 MB at 100k and 11.9 MB at 1m |
| lines | engine +218, report -281, **tree -63**; `report` is 6,746 against the brief's 5,500 (246 over the 6,500 I accepted); no 5,500 path exists inside `report` without changing bytes. 38 function definitions deleted; functions over 80 lines: 8 to 6 (`Snapshots::replay` 116 and `why/asset::report` 106, split) |
| behaviour | see Waiting on you 14: balance forms only, where the old replay contradicted the fold; 60 goldens and 213 mistakes byte-identical |
| proof | an exact fold oracle (the ledger advanced a day at a time) holds on every example and probe book; a session oracle (52 projects, 11,922 days); 200 fuzz books, 127,442 days, 628 replay-wrong positions, none unexplained; `dates.py`: 6,928 commands, 614 differ, all balance forms of 16 books of the TSV, 0 unexplained; `whys.py` 14,568, 0 differ; 30 mutants of the recording (28 killed at once, 3 only by a baseline-failing test: tests added and re-killed; 1 survivor after the unpriced list became a list: killed by a pinned test) and 12 of the pivot (11 killed, 1 survivor: a test added); **the 6 mutants of `why` are written and not run** |
| left | `claims`, `lots` and `available --at` still re-fold (`Context::ledger_at`: state at a day is parcels, not balances; needs K3e or month-end checkpoints); `Session` builds a plan per answer; `Steps::extremes` has no product consumer; `register` stays a statement (a running balance as of the cutoff is not `at(day)`); no cause index and no `Cause::Time { law, period }`; the cache of unpriced flows (`OnceLock` in `Folded`) is right only because `Lens::value` never reads whose (one test and one mutant guard it) |

## K6, in numbers

| | |
|---|---|
| what it is | **the ranking**: `classify` (`lower/infer.rs`): the written purpose, then each end's (the party's own, its kind's, the commodity issuer's), then an account kind's `takes` as a rewrite; the best rank wins; two sources of one rank naming unrelated purposes are still `purpose-disagreement`. **the index**: eight tables and a list become one `Groups<u32, Rule>` read through `Rules::at(Watch)`. **`derive`**: a law step that derives a flow or an item, typed in the law compiler and hosted by the occurrence (`engine/occurrence/derive.rs`: items are solved with the group by `solve`, flows join the occurrence), so a forecast sees them with no second path; a contract's laws fire with its occurrences (they never did). **the sugars**: `also` and `share` on a contract are the laws they abbreviate; `lower/also.rs`, `Also`/`AlsoOn`/`Implied`, `Match`, `Terms.shares` and the dead `Derivation` variants are gone. **relators**: `kind employment : contract` with slots and `also` legs whose ends are roles, `contract alex-pay : employment` with slot lines; each book that owns a contract of the kind has the legs it touches, decided at lowering (the owners are known then): `explore-v5/07-relators/` shows the paycheck, lease and manager of `05-family`/`07-landlord` printing what the originals print and the employer's book printing the employer's half from the same kind (payroll tax 7.65% of 46,500.00 = 3,557.28) |
| the finding | **four of the five "derive" mechanisms had no run-time path**: `also`, `share`, `match`, `sales-tax` were lowered, validated and read by nothing (the cutover dropped their engine half), and a contract's own laws never fired |
| lines | **+773 non-test** (core +6, engine +196, model +532, syntax +37) against a brief that hoped for -1,200 for layer 1: layer 1 +332 (step 4 -129, ranking +12, dispatch +104, derive +345), layer 2 +429 (`relator.rs` 329). The sugars were not copies of anything, `budget` was already a law, and the dispatch grew where it was planned to shrink by 60 |
| behaviour | the ranking regraded 41 goldens (`04`/`05` clean, see Waiting on you 5); the sugars move only `05-family`'s `forecast` (`alex-pay` 5,750.00 to 5,980.00, `mortgage-payment` 2,487.48 to 3,192.48: no golden command reads it, `outputs/forecast.txt` is a stale snapshot); a contract's law now fires (`contract-law-never-fires.ax` warns once); new warnings `also-inert` (a kind's, entity's, purpose's or account's `also` derives nothing yet) and `contract-share-party` |
| proof | `derives.py`: 400 triples of a law, the `also`/`share` lines that abbreviate it and the same by hand, 48,137 engine rows, 0 differ; `relators.py`: 500 cases each seen in two books, 41,329 flows, 0 differ; fuzz, K4b `splits.py` (300 projects, 8,269 commands) and K0a compare differ only in the listed notes; mutants: 29 of the derive code (one equivalent deleted, three survivors each given a test) and 20 of the relators (**1 not killed**: `Positions::of` taking the last of two fillers, equivalent until `as with`); the sweep was stopped, survivors verified by hand |
| left | `share` is smaller than LANGUAGE says (the header's share of each occurrence, not "12% of every flow"; a share for a party is the flow it bears, with a warning and no claim); `sales-tax` not built (a claim, not a flow); `relator.rs` re-reads the kind's AST per contract and resolves each leg's ends twice, `Positions` rides beside `Placement` as a second input; **the "sugars on" commit is not last** (`4bec164`; Layer 2 is built on it, so reverting it alone does not apply and separating them is about an hour of history surgery: not done); `+ N% of amount` as a template item under a contract header fails at the baseline (`InvalidProgram`); `flow` and `register` do not list what a kept occurrence made; Layer 3 (`on start`/`on end`, `part`, `joins`) and `as with` are for K6b and lane L |

## K3d, in numbers

| | |
|---|---|
| what it is | **A**: a payment is the flows of one statement out of one party's place (same unit, real on the day) that reach the owner or pay someone beside a flow that does, so a processor's fee leg settles; "exactly the flow's" is judged on what the party still pays from that leg on (`Request::exact`); a payment to a third party alone settles nothing; a payment out of a claim place is also a settlement. **B**: `engine/recognition.rs`, 137 lines: `Counting { books, purpose, day, recognized, due, dealing }.pieces()` with `Dealing::{Ordinary, Making, Settling, Forgiving}`, a `Piece` being the part of a flow that counts toward one purpose; the fold (`count_purposes`, `fire_purpose`, `explain`) and the readers (`flow::for_each_counted`) ask it, `budget` and `tax` follow the fold with no edit; `Run.settlements`; cash counts a claim when settled as the purpose it was made with, accrual when made and its settlement nothing; a write-off in accrual reverses (totals only: a law cannot subtract) |
| a bug the oracle found | an itemized write-off used the first line's purpose for every parcel (fixed, with a test) |
| lines | **+414 non-test** (engine +380, report +27, model +7) plus 610 test lines. The rule did not make the readers smaller: four private walks and the dead `Lens::purpose_direction` went (-32), the one shared walk is +55. `post` went from 42 to 32 lines |
| speed | callgrind `check` +1.3% on a book with no claims (`Counting::pieces`, `makes_claim`, a non-inlined `record_purpose` per purposed flow); 100k +3%, 1m +1.4% wall (first version was +2.7% before pre-filters) |
| proof | the claims oracle (600 projects, references written from §7, cash default): 0 failures; against the start commit 324 differ as the references say, 0 unexplained; 46 mutants at the final commit, 43 killed (30 by the oracle, 13 by tests), 3 survived and judged equivalent |
| left | **debts as parcels**; the three places its author is least proud of: three records of what a flow settled (`Record::settled`, `Record::settlements`, `Frame::settled`) that disagree on one kind of flow (a settlement should be recorded once on the flow and a return should read it back); a write-off's reversal written twice (`claims.rs::take_back` in the fold, `flow.rs::forgiven_by` in the reader; `Run` should carry it); the rule costs 1.3% on books with no claims; a limit that broke does not name a claim-place settlement in cash books |

## K7a, in numbers

| | |
|---|---|
| what it is | a new crate, `axiom-session`: `Session<'t>` is a loaded project as a value (`open`, `query`, `diagnostics`, `summary`, `run`), `what_if(&self, edit, ask)` hands a closure the session the edit would make, `apply(&mut self, edit) -> Result<Applied, Refused>` builds the next session as a value and assigns it only when it exists; `Edit` and `NewTransaction` are typed values (a line of the language written by `syntax`, not concatenated). The CLI is its first client |
| the borrow checker | no value can own a text, a book built from it and a plan over that book (`Book<'s>` borrows names, `Plan<'b,'s>` borrows the whole book), so: the text lives in `Texts`, an **append-only arena of `OnceLock` slots in doubling buckets** (no `unsafe`; `#![forbid(unsafe_code)]`), the session owns the book, the fold's results are kept lifetime-free (`Folded`) and the plan is built per answer. `query(&self)` returns a report tied to the session and `apply(&mut self)` needs it alone, so **no report outlives the state it was read from** (a `compile_fail,E0502` doc test with its passing twin); `what_if` takes a closure because a returned hypothesis would dangle (a second `compile_fail`). A `Session` is `Send + Sync` (asserted by a `const _`) and several coexist |
| lines | +363 non-test total: session +500 (about 150 of it the CLI's text store, moved), cli -169, report +32; against the brief's about +400. "Deletes the CLI's duplicated loading" was smaller than hoped: the duplication was the orchestration (about 60 lines) |
| speed | 1m `check` +1.6% min / +2.1% median (the retained checkpoint, about 1% of the fold), 100k within noise, RSS 680 MB both. A held session at 100k: `open` 301 ms, first query 172 ms, later 16 to 18 ms; at 1m first 2.05 s, later about 180 ms (a `Plan::new` each). **`apply` is a full rebuild** (398 ms at 100k, 4.85 s at 1m): `build` is 2.2 s of the 4.3 s `check` at 1m and no part of DESIGN §3.8's trail touches it |
| behaviour | no output changed: 1,592 outputs of every command on every example (text, `--json`, `--at`, `--for`, `--all`, `--relaxed`, `fmt --check`, `sync --dry`) byte-identical after every commit; 468 diff cases; `fuzz.py` 400 mutants and a new `fuzzcmds.py` (250 mutants x 15 commands) 0 differ |
| proof | 31 tests added, none deleted; 26 mutants of the new code, 24 killed at once and **2 survived and showed two missing assertions** (added, re-killed) |
| left | an incremental `apply` (needs `build` to be incremental: the model's, not the fold's); `Edit::Insert { day }`; `NewTransaction` for splits, `@ price` and `for`; `Project` stays in the CLI so an MCP server writes its own loader (the example has 15 lines); `check` builds two plans when the book has no errors; **`apply` refuses an edit that adds a syntax error and identifies a diagnostic by (severity, code, message)**: two policies argued, not facts (a stable identity is better and more code); a `Texts` keeps every applied edit's old text |
| K7b can delete | `report`, `report_with_sources`, `views`, `holdings_at`, `available::view_with_lens`, `Past::Journal` (about 90 lines and 60 test call sites), `with_plan` once views read the run, `ledger_at` and `available`'s forks |

## K3b, in numbers

| | |
|---|---|
| what it is | `jordan/bluefin/401k` is one token the model already read as a path; K3b reads it as an **address**: the entities that fill an account's slots in order, then its name. `model/addresses.rs` is an inverted index (posting lists by entity and by name, intersected by galloping from the shortest list with `core::postings`, then checked for order and for being open on the line's day); `spelled.rs` is the gate and the placement pass (`core::placement`: a word is placed only where every way of placing all the words puts it); `reference.rs` reads a reference and says `unknown-address` / `ambiguous-address` with the shortest address for each candidate; `ambiguous-placement`, `wrong-kind`, `too-many` for the words before a name. No grammar change. A book that writes no account that way reads exactly as before (a gate: `Addresses::is_used`) |
| lines | **+859 non-test (model +845), nothing deleted**, against a design that asked for about +250. The new spelling does not pay for itself in lines yet: lane L's deletions are about 100 lines of model code (`institution`, `unknown-institution`, half of `missing_roles`) and 33 `owner`/`employer`/`beneficiary` lines in six examples (the acceptance copy of `05-family` drops 13 of its 15 relation lines and the names that carried a relation). What it buys is a relation said once that the name cannot contradict, which is what the `jordan-401k` complaint was |
| speed | callgrind `check`: old spelling +0.15% (100k), +0.13% (1m); the same flows written as addresses +1.7% / +1.9% (a first version was +12.6% before the settled-reference memo) |
| behaviour | no golden or mistake changed (5 new mistake books, 100 to 104); one semantic change in an old shape: an old account whose path **begins with an entity's name** used to be owned by `me`, and is now owned by that entity (no corpus holds one) |
| proof | an address oracle: 400 generated books, 7,533 references read, 0 wrong (5,463 one account, 378 ambiguous, 1,563 unknown, 129 parties); a separate placement oracle against brute force, 400 cases, 0 wrong; 33 mutants, 31 killed, **2 not killed** (a suggestion that is no number; a commodity end that is not an address attempt) and 3 more deleted as redundant code rather than killed; a copy of `05-family` in the new spelling checks to the same 141 diagnostics, balances, claims, tallies, limits and tax |
| left | nesting (`entity fidelity` with `alex/401k` under it) and `as with` are not built (map §7: lane L); the custodian is still `at`; the three places its author is least proud of are in K12b's brief (the party pass decides by source text; the settled memo and `inline(always)`; `is_spelled` and `own()`) |

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
