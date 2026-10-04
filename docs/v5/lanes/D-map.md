# Lane D map: where each decision lives, what it changes, and the order

Written before any code change, from reading the code at `46a6c07` (K6b merged) and running a baseline binary built from it
(`cargo build --release`, kept; goldens, mistakes and the tests reproduce at it). Line numbers are of that commit. Where the
brief and the code disagree the item says so first: **items 3 and 6 are partly built already, and item 6's `match` is gone.**

## The order, and why

| order | item | why here |
|---|---|---|
| 1 | 7 the year-end test | a test input; turns the known failure of `report` green; touches nothing else |
| 2 | 1 the prorata fixture | a test input and one sentence of LANGUAGE §9; turns the known failure of `engine` green |
| 3 | 10b the two debug-only tests | a fixture; needs a debug build once (`CARGO_INCREMENTAL=0`, `target/debug` deleted after) |
| 4 | 10a `currency USD` | one reader |
| 5 | 4 the missed claim's purpose | one line and a test |
| 6 | 6 words and docs | `AccrualAt::Due` test, LANGUAGE §1, §3, §6, §7; nothing in the output |
| 7 | 9b `unknown-address` | one function |
| 8 | 3 one warning per run | the diagnostic; moves goldens and mistakes (wording) |
| 9 | 9a the owner's range | a field of `Kind`, one line of `us.ax`, two checks |
| 10 | 2 the fee leg | the engine rule; moves `08-expat` |
| 11 | 5 mortgage interest | moves `05-family` and `07-landlord` tax numbers; touches a system and 12 journal lines |
| after L1 | 8 the books | `examples/` is rewritten by L1; stop and report if L1 has not merged |

Items 2 and 5 are last of the code items because they move numbers a reader will check. Items 3, 9a and 5 are the three
that need more than one commit-sized idea; each says where its design is.

## 1. Prorata basis, reading R1

**Lives at.** The rule is `engine/src/post.rs:415`, arm `(None, _, _, _) if unbased && !from.deferred => Qty::ZERO` of
`Ledger::price`; it stays as it is (K3c-map §5). The failing test is `engine/src/tests.rs:1096`
`a_prorata_place_realizes_only_the_lots_share_and_deferrals_merge_into_one_lot`: its `f.flow(2, checking, retirement,
6_300_00)` is a transfer from a taxed place into a `basis zero` place, which R1 makes a contribution with no basis. The HSA test
(`source_tests.rs:330` `hsa_basis_zero_contribution_and_against_medical_reimbursement`) wants exactly that and is unchanged.

**Change.** The test states `basis 6_300 USD` on flow 2 (`f.detail(flow, Detail { basis: Some(Qty(6_300_00)), .. })`, the
way `a_stated_basis_and_a_hold_override_what_the_route_says` states it at `tests.rs:2184`). Its assertions are unchanged:
the stated basis wins the first arm of the match (`(Some(basis), ..) => basis`), so the 6,300.00 arrives with its face as
basis and is plain money, the two zero-basis deferrals of 1,000 and 1,200 are one lot, and the sale realizes 388.24 only.
LANGUAGE §9 says `basis AMOUNT` overrides the arrival basis but not that a transfer from a taxed place into a `basis zero`
place arrives with none; one sentence is added to the "Transfers" bullet, naming the nondeductible IRA contribution.
`REMAINING.md:41` already says "explicit basis for nondeductible contributions".
**Output.** None: a test input and a sentence. No golden moves.

## 7. The year-end test

**Lives at.** `report/src/source_tests.rs:1115` `a_context_forecast_keeps_historical_and_same_day_obligations_once`, whose
system says `each year closing 12-31`; LANGUAGE §8 closes the 2026 year on 2027-12-31, so a `year-end-tax` due 2027-01-15
cannot exist in a forecast to 2027-03-01. The sibling is `source_tests.rs:1167`
`a_context_forecast_takes_the_closing_of_the_day_it_stands_on_once` (K5c), which says `each year`.

**Change.** The original's `closing 12-31` becomes `each year`. The two books and the two assertions are then the same text, so
the sibling asserts nothing the original does not: **they are merged into one**, the original's name kept (it is the test that
was failing), the sibling's doc comment (why `each year`) folded into the original's. No assertion is weakened.
**Output.** None outside the test. Net lines go down by about 50 (test lines).

## 10b. The two debug-only tests

**Lives at.** `engine/src/tests.rs:819` `kind_totals_use_descendants_and_the_subject_owners_places` and `:893`
`computed_kind_totals_index_every_kind_and_use_the_selected_kind`. Each calls `f.book()`, which numbers the holders for the
fixture's two kinds (`fixture.rs:481`), and then replaces `book.kinds` with a tree of six: `HolderIndex::number` has a
`debug_assert!` (`holders.rs:82`) that the thing is in its arena of the numbering, which it no longer is (the numbering still
counts two kinds). Release builds skip the assertion, and read the places at numbers shifted by four.

**Change.** The fixture, not the product: the fixture learns the kinds a test wants (an opt-in method that takes the extra
kinds and numbers the holders for them), and the two tests ask for theirs instead of replacing `book.kinds`. Confirmed first
by running both in a debug build (see the report). **Output.** None.

## 10a. `currency USD`

**Lives at.** `model/src/props.rs:395` `Args::currency` reads `self.name("a commodity")`, an `ExprKind::Name`, and a currency
is lexed as `ExprKind::Unit`, so `currency USD` on an entity is `error[property-type]: `currency` needs a commodity, this is a
commodity` (run on the baseline with `entity kid : person / currency EUR`: refused; no book can write the line). `Args::holds`
(`props.rs:465`) already reads units, inline.

**Change.** One reader, `Args::unit`, picks a commodity out of an `ExprKind::Unit` and resolves it; `currency` and `holds` both use
it (`holds` loses its inline copy, its messages are the same). **Output.** `currency USD` is accepted. The only text that
changes is the text of a *wrong* argument (`currency 5%`): it said "this is a commodity" of a unit; it says what it needs, "a
commodity such as `USD`". `docs/v5/measure/diff/cases3/decl-builtin-currency-*` (lane U's corpus, on its branch) will regrade:
`-extra` (`currency USD now`) was `property-type` because `USD` was refused, it is `property-argument` ("takes no more arguments
here") once `USD` is read.

## 4. A missed occurrence's claim carries the contract's purpose

**Lives at.** `engine/src/claims.rs:36`, `Ledger::claim_missed`: `header.flow.purpose = None;`, and the module doc (lines 3-4)
that says the claim has no purpose. `header` is the first flow of the occurrence the fold instantiates, so its purpose is the
contract's own (written on the template, the contract, or ranked from its party at lowering).

**Change.** The line goes; the doc says the claim carries the purpose, so recognition (`recognition.rs`) counts it as any claim:
when it is found in accrual books, when it is paid in cash books (the default). The K5c test that asserts the opposite,
`claim_tests.rs:585` `a_claim_the_monitor_made_has_no_purpose_for_a_law_to_count`, is *the behaviour this item changes*: it is
rewritten to say what is now true in cash books (a law that counts the purpose still counts nothing when the claim is made) and a
sibling says it counts in accrual books, on the day the miss is found. **Output.** To be measured at the commit: only a contract
with `due`, owed by the party, and a purpose that a law or a limit counts; probably no golden.

## 6. Words, docs and the grammar nobody reads

- **`Books::Accrual`'s doc** (`model/src/book.rs:325`) already says "when it is made (LANGUAGE section 7: "when invoiced")": K3d
  did it. **LANGUAGE §6** (`books cash|accrual`, line 487: "when claims are income or spending (default cash)") says nothing of
  when, and **§7's contract paragraph** (line 657) says "in accrual books an occurrence is income or spending on its due day":
  that is the stale one. Both are corrected. `AccrualAt::Due` (`engine/src/recognition.rs:26`) is a constant (`ACCRUAL_AT`) read
  in one function (`Counting::made`, line 176); the test needs the function to take the alternative as an argument
  (`made(at: AccrualAt)`, the caller passes the constant), then `Due` is tested directly: a claim made on 02-10 that falls due
  on 03-01 counts on 03-01, and counts on 02-10 under `Made`.
- **`claim`, `debt-claim`, `principal`.** LANGUAGE §1 ("Keywords are recognized by position and not reserved") gets the one place
  that lists them; the diagnostics a book that declares one gets are looked up and quoted (not guessed) in the sentence.
- **`?` beside `...`.** LANGUAGE §3 "Split flows" says it is `cannot-infer`: a sentence, with the fix.
- **The employer `match`: gone, nothing to do.** `Match` is not in the tree (`git log -S"Match" -- crates/model/src/book.rs`: removed
  by `4bec164`, "a contract's `also` and `share` are the laws they abbreviate"); the grammar has no production for it
  (`crates/syntax/src/contract.rs` reads schedule, `buy`, `due`, `grace` and the rest); a `match 50% up to 3%` line under a
  contract is read as a leg to a place called `match` (run on the baseline: `type-mismatch`, "expected an amount, but this is a
  number"). The `also` line in LANGUAGE §7 already is the spelling. STATUS's "grammar accepts and nothing reads" row for `match`
  is stale. No mistake book: nothing to refuse that was not already.
**Output.** None (docs, and a test).

## 9b. `unknown-address` suggests an address

**Lives at.** `model/src/reference.rs:197-232` `World::unknown_address` and `named_by` (234): the suggestion is the closest
*name* among the accounts the **first** leading word's entity fills, written after the words the user wrote. Run on the baseline
(`jordan` fills `jordan/bluefin/401k` and `jordan/roth-ira`; the line says `jordan/bluefin/roth-irc`): "did you mean
`jordan/bluefin/roth-ira`?", an address that is itself `unknown-address` (`bluefin` fills the 401k, not the IRA; the same line
with the right name says so: "there is no address `jordan/bluefin/roth-ira`").

**Change.** `named_by` yields the accounts with their names; the nearest name picks an account, and the suggestion is the written
words and that name when they mean the account (the `means` check the ambiguity diagnostic already uses), else the account's own
full address (`jordan/roth-ira`): something that can be pasted in place of what was written, by construction. The note ("the accounts
these words fill are called ...") is unchanged. **Output.** Mistake book 116 shows the old and the new; `100-unknown-address` (one
filler, the words mean the account) says the same as today.

## 3. One `missed-occurrence` per run

**Lives at.** `engine/src/monitor.rs:166-198`: `missed` already groups the unkept due days **per contract** (so the brief's "warns
for every missed due day" is not what the code does: a contract with 17 missed days says "was due 17 times" once and lists the
days), and `missed_by` words it. What it lacks is the **run**: two stretches of misses with a kept day between them are
one warning today, and the warning does not say since when nor offer the edit that ends a stretch. The record
(`Promise.kept == None`, one per due day) is untouched: `why`, `overdue` and the claim read it.

**Change.** `missed` sorts a contract's due days with whether each was kept (`Promise.kept`, claimed ones left out as today),
cuts them into maximal runs with `chunk_by`, and warns once for each run that was missed. The headline names how many and since
when (`` `rent` was due 4 times since 2026-01-01 and no occurrence was written (the last on 2026-04-01) ``); a run of one is
worded as today. The edit: a run that nothing kept after, in a contract with no `until` and a kept occurrence before it, gets
`.fix("... ends after its last kept day", at the end of the contract's line, "\n  until DATE")` where DATE is the later of that
occurrence's due and kept day (the diagnostics already carry such inserts: `problem::missing_role` writes `\n  slot value` at
`Loc::new(file, end, end)`). The existing two helps stay for each run, on the run's last due day.
**Output.** Every warning with more than one missed day changes its first line (it adds "since DATE"): `05-family-check` and
`07-landlord-check` (goldens), `105`, `109`, `112` to `115` (mistakes), and the `diff/cases` that say it; to be listed with counts
at the commit. New mistake books: a single miss, a run, two runs with a kept day between (116 onward).

## 9a. The owner's range

**Lives at.** `model/src/spelled.rs:76-125` `Free::Owner` ("takes any entity": `fits` is `true`, `takes` says "any entity") and
`free_slots` (285); `props.rs:805` makes `owner` under a **kind** an error ("`owner` is not a property of this kind"); explicit
`owner X` lines are read by `declare.rs:735` `Resolving::owners`, by name only. Run on the baseline: `account acme/529 : bank` with
`entity acme : employer` is accepted, and `acme` owns it. K3b-map §7 and §11.3 describe the gap and the design (a kind-narrowed
`owner`); `FIELD_WORDS` (`slots.rs:121`) forbids `has owner ...`, and the facts of a book are a write-only builder until they
are frozen, after the placement runs, so the range cannot be a fact. **The kind says**, so it is a field of the kind.

**Change.** A kind may say `owner KIND, KIND` (the same line the account writes with entities, now with kinds under a kind):
`Kind.owners`, the nearest non-empty one up the lineage is the range, none being "any entity" as today. `Free::Owner` takes the
range of the account's kind (`fits`, and `takes`: "a person or a household"); an explicit `owner X` line is checked by the same
predicate after the kinds' lines are read. A word or line that does not fit is `wrong-kind` with the edit the role-line machinery
already writes. `us.ax` says `owner taxpayer` once on `tax-deferred` (401k, IRA, HSA, 529 are person or household accounts).
**Output.** Nothing in the goldens, mistakes or examples (no corpus writes an employer, a party or an LLC as the owner of a
tax-deferred account; the diff run says so). Mistake book: `acme/529`, old behaviour (accepted, `acme` owns the 529) in this map.
If it needs more than 80 lines (a field and three struct literals, a reader, a check, a diagnostic) the report says why.

## 2. The fee leg of an exchange split

**Lives at.** The machinery exists for an **item**: `engine/src/statement.rs:338` `is_exchange_cost` (a `- AMOUNT #fees` item under
an exchange header), `statement.rs:94` `exchange_costs` (sums those into the header's `Detail.cost`, in the base currency),
`engine/src/ledger.rs:505` `post_journal` (finds the header's group and hands it to `post_computed`, which sets `detail.cost`),
and `engine/src/post.rs:441` `charge_costs` / `exchange_cost` / `realizes` (a sale's proceeds shrink by it, a purchase's basis
grows, unless the flow states `basis`). LANGUAGE §3 "Pairing" already says "legs and items whose purpose is a cost (`#fees`,
`#closing-costs`) are costs of the exchange"; K4b built it for items only (K4b-map §11.2 row 11). A split is a `Heading::Source` group
whose **legs** are flows (`Made.legs`); none of its legs is a header flow, so `is_exchange_cost` is false for all of them.
Measured on the baseline with a two-line book (Wise-style conversions, `buy.ax`): `us-checking 9_500.00 USD -> fx-fees 55.10 USD,
girokonto 8_100.26 EUR` gives the euros a basis of 9,444.90 USD (the README says 9,500.00).

**Change.** The one reader that decides it is `statement.rs`: a leg is a cost of its split's exchange when the split's source
pays (`group.side == FlowSide::Out`), another leg is the exchange (`flow.is_exchange()`), and the leg is not itself one and its
flow's purpose is a spending (the same test as the item's). `exchange_costs` takes the offsets of the costs, items and legs,
instead of items alone, and reads each one's amount the one way (what the group solved, else the flow's own). `post_journal`
hands the group to `post_computed` for the exchange leg of a split as it does for an exchange header. The fee leg stays the payment
it is (an expense in `register`, `flow` and `tax`): only the exchange's cost is added. The promise's `= AMOUNT` leg is **as
built**: nothing in the promise path changes.
**Output.** A test with the README's numbers (9,500.00 and 55.10: the euros' basis 9,500.00 USD, not 9,444.90; and the sale
side: the fee leg `fx-fees 4.77 EUR` comes off the proceeds of the 895.23 EUR sold). `08-expat`'s goldens move where a Wise
conversion's lots or gains are read (`gains`, `tax`, `available`, `balance --value`): to be measured at the commit. Differential
checks: `splits.py` (its `exchange` recipe has a computed cost item and one or two fee legs, which the baseline is held to), fuzz.

## 5. Mortgage interest

**Lives at.** `systems/src/us.ax:103` `purpose mortgage-interest : spending`, and the law at 266-272
`itemize-interest-and-gifts` (`when purpose is #mortgage-interest | #charity`, `count amount as itemized`). Nothing else reads the
purpose (grep: `us.ax`, 12 lines of `05-family/journal/2025/*.ax`, its README line 20, 12 of `explore-v5/06-family-addresses`,
and `examples/verify/verify05.py`). The rental law is `us/rental.ax:41` `rental-expenses`: a law of `kind rental-home`
counting `#repair | #insurance | #property-tax | #interest` of the asset. `kind home : personal-use` is `us.ax:116` and has no laws.

**Change.** A law of `kind home`: `on flow`, `when purpose is #interest`, `count amount as itemized`; the itemize law keeps
`#charity` only and the purpose goes. A flow `of` an asset fires the laws of the asset's kind, and a flow `of` a building is divided
among its parts by area (LANGUAGE §10), so a mixed-use building's interest is the rental share's rental expense and the personal
share's itemized deduction, adding up to the posting; a loan `for house` writes `#interest of house` and needs nothing
special. `05-family`'s twelve house lines `#mortgage-interest` become `#interest of house`; the car's `#interest` lines stay
(a vehicle is no home). `examples/verify/verify05.py` reads the tag: it follows. **Output.** `05-family-tax` and `-available`
and `07-landlord-tax` move: itemized deductions and tax lines, to be listed old to new at the commit, with the check that the rental and
itemized figures add to the interest of the asset. This commit edits `examples/`, which L1 rewrites; the edit is one word of twelve
lines and the commit says the command that redoes it (`sed`), so a merge after L1 is mechanical.

## 8. The books (after L1)

Waits for lane L1 to merge (`git log claude/great-wozniak-pnqn7x-v5`). Not started in this map's order; the report says so if L1
has not merged.

## How each commit is proved

Baseline binary built at the starting commit and kept. Per commit: `cargo fmt --all`, `cargo clippy --workspace --release -- -D
warnings`, `cargo test --workspace --release`; `sh tests/golden.sh` and `sh tests/mistakes/run.sh` compared with `git diff tests/`;
at the end the differential run (`docs/v5/measure/diff/` `run.sh` then `compare.sh`, `fuzz.py ... diff`, `splits.py`) against the
baseline, every difference listed with the item it belongs to; `check` on `bench/` 100k and 1m, three runs, fastest, with the
load average.

---

## As built (appended when the code items were done)

Eleven commits after the map, one for each item but 6 and 7 (which are also one each) and none for 8 (waiting for L1, which has
not merged into v5: `git log claude/great-wozniak-pnqn7x-v5` ends at `b04b597`). Where the code showed the map or the brief wrong:

| item | what the code showed | what was built |
|---|---|---|
| 7, 1 | as mapped | the report test says `each year` and the sibling is merged into it; the prorata test states `basis 6_300 USD`; LANGUAGE §9 says why. Both known failures pass |
| 10b | the debug failure is `holders.rs:82`, as mapped | `Fixture::book_with_kinds`; both tests pass in a debug build (`CARGO_INCREMENTAL=0`, `target/debug` deleted) |
| 10a | as mapped | `Args::unit`; `currency USD` accepted; `holds` uses it too |
| 4 | **one line was not enough**: what settles a claim reads its purpose from the journal line that made it (`recognition::claim_purpose`), and a claim the monitor makes has no line, so in cash books the payment would never have counted it | the line goes, and `claim_purpose` reads the contract template's header for a monitor-made parcel. A law counting the purpose counts the rent in accrual books on its **due day** (the claim keeps its occurrence's days), found 16 days later; in cash books on the day it is paid. The brief's "when it is found" is the day it is *made*, not the day it is *counted*. No golden moves |
| 6 | `Books::Accrual`'s doc was already right (K3d); LANGUAGE §7's contract paragraph was the stale one; **`match` is gone** from grammar and model | docs; `Counting::made(at)` and a test of `AccrualAt::Due`. `claim`, `debt-claim`: `duplicate-kind`; `principal`: `ambiguous-purpose` (a purpose declared in `std.ax`, not a built-in root) |
| 9b | the suggestion was unpasteable only when the other words do not reach the nearest account; the account's own address spells its custodian (`jordan/fidelity/roth-ira`) | the written words and the name when they mean it, else the account's address; mistake 116 |
| 3 | the monitor already warned once for each contract; what it lacked was the run, since-when and the edit | runs by `chunk_by`; `until DATE` for a run of several days, last of a contract with no end and not a loan; mistakes 117 to 119; 05-family-check, 07-landlord-check, 112 to 115 move (headline), 07's `paycheck` gains the edit |
| 9a | `me` (the undeclared root entity) is not a taxpayer: `101-ambiguous-address` broke until `me` always could own; the explicit `owner` line needed the same check | `Kind.owners`, `Book::may_own`, a kind-level `owner` line, `us.ax` `owner taxpayer` on `tax-deferred`; mistakes 120, 121 (new), 122 (the ambiguity 102 used to show), and 102 moves to `too-many` because its premise was the bug |
| 2 | no golden moves: `08-expat`'s conversions are v3 and do not reach the fold | `exchange_of` and `exchange_costs_of` in `statement.rs`; README numbers tested (9,500.00; 1,027.63 less 5.57); a leg that arrives is no cost |
| 5 | **the tax numbers of 05 and 07 do not move**: the home law counts what the old law counted; only 05-family-check's "laws enforced" goes 23 to 24. A building divided among parts of both kinds is **not built** (the engine walks `part of` upward and never divides a flow of the whole by area) | a law of `kind home`; `mortgage-interest` goes; 12 + 12 journal lines; `verify05.py` passes |

Proof, against the baseline binary built at `46a6c07`: goldens and mistakes byte-identical except the lists above; the diff harness
(552 outputs) differs in 31, all `missed-occurrence` headlines, two of them with the `until` edit and one of them (`promise-no-from`) a
contract with a kept day between two runs; `fuzz.py` with the headline blocks left out and `us.ax` line numbers normalized: 2,000 mutants,
0 panics, every difference one of the three (the headline, the law count 23 to 24 and `us.ax` line numbers, `owner` listed among a
kind's properties); `splits.py`: exchange 400, split 600 and statements 500 projects 0 differ, `equiv` 500 splits 0 not the plain
transfers; `check` on `bench/` 100k is byte-identical (1,022 errors both) and 1m is no slower (fastest of three, load 8 to 10:
4.72s to 4.89s against 4.86s to 4.95s; 0.44s to 0.47s against 0.44s to 0.46s at 100k). Workspace tests: 1,322 passed, 0 failed.

Item 8, prepared and not done: with `grace 30d` on `manager-fee` (tried on a scratch copy, not in `examples/`) `07-landlord`'s
`check` goes from 7 errors to 5.

## Item 8, as built (after L1 and K3f merged)

Six commits: `03-violations`, `07-landlord`, `11-sam`, `10-budgeter`, `09-shared`, `08-expat`. Every edit to an example is in its copy in
`tests/v4-syntax/examples/` too, and `crates/cli/tests/upgrade.rs` passes (5 tests): for the three ports the copy is the hand-ported
book in v4 spelling, written by a script over the old v3 text and the declarations by hand, and `fmt --upgrade` of it is the example
byte for byte, so the proof test needs no special case for 08, 09 and 10. Only the goldens of 07 (the grace), 08, 09 and 10 moved; the
old goldens of the three ports were each 600 lines of the v3 errors. `tests/mistakes` did not move.

What the engine and the language could not say, and what the books do instead (each is also in the README of its book):

| where | the gap | the book writes |
|---|---|---|
| 09 | LANGUAGE §3: "a flow to a party with `due`, or `for` a party, is a claim". Only `X owes me`, `me owes X` and a flow with `due` into a declared `receivable` place make one (K3c-map section 1.1) | `ben owes me 1_050.00 USD #rent due ... ^rent-2025-03`, a payment `checking <- ben ... ^rent-2025-03`; the loan to a friend is a flow and a claim |
| 09 | a tab has no name, so a value cannot be asserted of it (`statement-lowering`: "a value needs an account, asset, code or commodity subject") | the 43 month-end `by-cleo = 95.70 USD` statements are gone; `claims`, the README table and `verify09.py` are the reconciliation |
| 09 | a write-off is whole (`a full claim write-off cannot include recovery lines`); `#loan` is not a built-in purpose | forgiving 200 of 250 is Riley's payment and a gift of the same money; `purpose loan : transfer` is declared |
| 09 | a claim on an employer made with `owes me` is a flow *from* the employer, so the party kind's `pays wages` makes it wages; so is the payment | `#job-supplies` on the claim, `#reimbursement` on the payment |
| 09 | the grant's property is `grant-purpose` (`std.ax:152`); `purpose garden` on the entity is read as the party's own purpose and the `purpose` law says "`grant-purpose` is not set" | `grant-purpose garden`. **`02-household`, `03-violations` and `tests/mistakes/70-grant-wrong-purpose.ax` write `purpose education`, so their grant law has no purpose to test** |
| 08, 10 | a paystub written as a split through the owner counts as wages only what lands in an account (7,219.80 of a 10,416.67 stub; no `pretax`) | gross wages, then the withholding as outflows (the 05-family pattern); in euros through a clearing account, because the FBAR's `always` law and the currency disposals read every intermediate balance |
| 08 | a two-amount line whose price does not terminate stays v4 (`warning[v4-syntax]`) | the six-digit price that rounds to the statement's amount |
| 08 | tallies in a project law must be `value(tally(x), USD)` (type-mismatch otherwise); prices need `=` | written so; the `foreign-wages`, `de-tax` and `student-loan-interest` account kinds are purposes of the systems that count them |
| 10 | a contract paid `from` an envelope that holds less than it pays is projected without the cap, so `Committed` moves by 500.00 instead of 1,500.00 on the trip (net worth moves by the 3,000.00 either way); the weekly 24.50 of dining paid to `?` is no longer found recurring, so every forecast figure is 490.00 higher | README says both |

Found, not caused and not touched: `07-landlord` has 5 assertion errors at the v5 head (`deposit-bank`, `deposits`, `bills`) with and without `grace 30d`;
`examples/verify/verify11.py` fails at the head (`straight-line` is no longer in `std-sketch.ax`); the `outputs/` folders of 04 to 10 are recordings of the
legacy binary and say v3 (`expenses/fun`), so the READMEs name figures, not those files; `contracts` prints "0 kepts"; `check` and `budget` disagree about October's
fun in 10-budgeter (297.01 in the warning, 323.02 for the month: the warning's figure leaves out the last flow of the month, 26.01 on 10-24).
