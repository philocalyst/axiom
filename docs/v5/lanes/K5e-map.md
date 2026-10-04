# K5e map: a loan that began before the book opens its debt with what its schedule says is owed

Written before the first code change of lane K5e, from the code at `4379432` (K5d merged, with `03-triplex`'s `from`), and
checked against what the code does (the last section says how). Paths are in `crates/`; line numbers are those of `4379432`. The
vocabulary is K5d's ([`K5d-map.md`](K5d-map.md): the schedule, `Amortization::open_on`, `Said`, `Cause`) and K5b's (the monitor).
The baseline binary is built from `4379432` (`scratchpad/k5e/baseline-axiom`).

## 0. What the brief says and what the code does

Seven things in the brief do not match the code. Items 1 to 4 decide the design.

1. **An `opening` block does not make an `Origin::Derived` flow.** It makes `Origin::Written`, `Mode::Opening` flows
   (`lower_opening_leg`, lower/record.rs:575-633: `make_resolved_flow` sets `origin: Origin::Written`, flow.rs:418). No
   `Derivation` is an opening. So "the same kind of flow the model already makes for an `opening`" is the flow `lower_opening_leg`
   makes, and what says *it was implied* is a new `Derivation::Opening(contract)`, beside `Interest` and `Principal`, which are
   what K5d's payments say they come from. The readers that match `Derivation` have a `_` arm (`report/register.rs:401`), so the
   variant costs one word and one line there.
2. **"On the day before the first fact" cannot be the flow's day, and the first fact is not known when it would have to be
   placed.** Two facts of the code decide it:
   * **`Book.flows` is day-sorted by construction and read as such.** The fold's flow stream is a cursor over the arena
     (`Timeline::refresh`, engine/timeline.rs:365, `seek` :327 `partition_point`), `Book::first_fact` below reads the first
     flow, and `Flow.txn` ties a flow to the transaction that lowered it. A flow of day F-1 can only be placed *before* every
     flow of the book, which is before the lowering knows what the first fact is: the first fact is whatever the first record
     that lowers to something turns out to be (`flows`, an occurrence, an assertion, a split, a claim change), and a record
     can be rejected.
   * **A flow dated F-1 would move the first fact.** `first_fact` is the first flow's day, an opening's flows included (an
     `opening 2026-01-01` is the first fact of `11-sam`), so the implied opening would make the book begin a day earlier and the
     monitor, which starts its streams at that day (§1), would find the payment due on F-1 not kept.
   So the implied opening is dated **F, the first fact's day itself**, and says what is owed **when F begins** (after the payments
   due before F). It is lowered immediately after the first record that makes the first fact, so it follows that record's flows
   in the arena and keeps the arena sorted, and `first_fact` is F with it as without it. A payment due on F is the book's, and
   it posts after the opening: the two commute (a place's balance at the end of a day does not depend on the order of the flows of
   the day, and an assertion is judged after every flow of its day, timeline.rs:6-19). §7 says why the amount is the same as
   the brief's.
3. **The first fact's definition is in `engine`, which `model` cannot call.** `engine/timeline.rs:124` `first_fact(&Book)`,
   called once by `Plan::new` (plan.rs:145) and read by the monitor's start (`Plan::watch_from`, plan.rs:281). It reads nothing
   but the `Book`. So it is **moved** to `Book::first_fact` (model/book.rs) and the engine reads it from there: one definition,
   one place, asked by the lowering that needs it and by the plan that always did.
4. **The schedule does not exist when the opening is lowered, and what the opening needs of it does not need the journal.**
   `Promises::compile` runs after `lower::record` (model/lib.rs:125), and `Said::of` reads `book.touching` (the flows into the debt
   tab), which `record` builds in its last lines. But the balance when F begins is a function of events **before F**: the due
   days, the rates a statement says (a `now at` is a record dated before F or it is not before F), the resets, and nothing the
   journal does to the loan, because everything the journal does to a loan is a fact and F is the first. The precedent is
   `kept_by` (record.rs:677-708): lowering already asks a contract's promise as it stands (`Promises::alone`, promise.rs:175) while
   the waivers written after are not yet read. `Promises::owed_before(book, contract, day)` is that, for a loan, walking the same
   `walk` (amortization.rs:174) over the same dues, so the opening and the schedule that is compiled later have one arithmetic.
5. **`LOAN -X USD` is not how an opening writes a debt.** A debt is positive in an opening, as in a statement (LANGUAGE §5: `visa
   1_240.18 USD   owed`). Measured on the baseline (`scratchpad/k5e/p/sam`): `opening 01` with `mortgage 312_441.12 USD` makes
   `rocket 311,345.99 USD` owed on 2026-03-31, and `mortgage` as a name resolves to the debt tab *before* an account or a
   kind (`special_end`, resolve.rs:245-258), so the override is `mortgage 312_441.12 USD`.
6. **`explore-v5/03-triplex` is not byte-identical.** Its four `31 mortgage = ...` statements are held to the tab as well as to the
   schedule, and the tab starts at zero: `error[assertion]: first-federal holds -509.16 USD, not 355,671.89 USD` today (golden day
   2026-12-31: 66 errors). With the tab opened the four agree and go. `explore-v5/02-family` loses its three `rocket holds -N USD`
   the same way and keeps its own `loan-balance`s (its mortgage's first payment is one month earlier than its comment says: the
   `03-triplex` fault, K5d §13.3).
7. **A payment before the book began is no candidate for "missed".** `Amortization::explain` (promise/causes.rs:84-110) takes
   *every* payment due on or before the statement's day that no line keeps as `Cause::Missed`. For a loan that predates the book
   that is the 22 payments before it, none of which any line can keep (a line is a fact, and F is the first), so the cause could never name the
   missed payment of the book's own February. It is the same one-line fault as the monitor's rule (§1: a due day before the
   first fact is not missed), carried into the one place that re-derives it: `Said` gains the day the book begins and
   `explain` starts there.

## 1. The book's first fact: one definition

```text
Book::first_fact(&self) -> Option<Day>          model/book.rs   (moved from engine/timeline.rs:124)
    the earliest of: the first flow (an opening's included), the first written occurrence, the first assertion,
    the first split, the first claim change
Plan.first_fact            = book.first_fact()  engine/plan.rs:145
Plan::watch_from(today)    = first_fact.unwrap_or(today)   the monitor's start (Ledger::start, ledger.rs:88)
```

A book that has no fact (declarations only) is watched from the day the fold is run for: a contract's due days before the book is
all there is to say were not owed. `timeline::start` (timeline.rs:107, *the first real fact*, skipping `Mode::Opening`) is a
different question (what starts the laws that close periods) and keeps its place.

## 2. How an `opening` is lowered today, and the flow it makes

`record` (record.rs:161-215) lowers every dated record in `(day, written order)`: a `Txn`, a `Statement`, an `Opening`. For an
`opening` block `lower_opening_balances` (:531-571) opens a `Staged` (the rollback guard), makes one flow a line
(`lower_opening_leg`) and one `Txn` for the block (`journal_txn`, `TxnKind::Journal`, no contract, no program unless a line
computes a basis).

| a line | the flow |
|---|---|
| `visa 1_240.18 USD` (a place) | `Class::Debt` has `display_sign() == -1`: **from the place to the opening entity's place**, `out == arrive`, `Mode::Opening`, `Infer::Known` |
| `checking 6_062.55 USD` | `Class::Asset`: from the opening place to the place |
| purpose | `classify(from, to, None)`, as any flow; owner: the place's |
| `Origin` | `Written` |

The opening place is `book.entities[book.roots.opening].place`. Openings "are states, not flows: no law sees them" (LANGUAGE §5):
`post.rs` skips the laws for `Mode::Opening`, `timeline::start` skips them. A debt tab's `Said::paid_into` takes flows **into**
the tab, and an opening is **from** it, so it is not a prepayment.

## 3. How an origination line is detected, and the flow it makes

`lower_occurrence` (record.rs:715-741): a line `DATE NAME` whose contract has a loan with `loan.on == DATE` is the origination
(`lower_loan_origin`, :1122-1267, 146 lines, K5d's): one flow **from the debt tab to the owner's holding the schedule pays
from**, `Mode::Actual`, `Origin::Occurrence(contract)`, `TxnKind::LoanOrigin`. It moves the principal and the cash.

An origination line is a fact on the loan's own day, so **no loan with an origination line is before the book's first fact**
(`on < F` and a fact on `on` exclude each other): the brief's "no origination line" is the condition `on < F` itself and needs no
test of its own. A loan whose origination line is rejected is not originated and the book is wrong anyway.

## 4. Which examples have a loan that predates the book

Measured on a scratch test over the baseline tree (`scratchpad/k5e/measure.txt`, the book's `first_fact` and the schedule's
`open_on(F-1)`):

| example | loan | `on` | first fact | origination | predates | owed when the book begins |
|---|---|---|---|---|---|---|
| `05-family` | mortgage-payment, car-payment | 2025-12-31 | 2024-12-31 | no | no | |
| `07-landlord` | home-loan | 2024-12-18 | 2024-12-18 | **line** | no | |
| `11-sam` | mortgage | 2024-02-20 | 2026-01-01 | no | **yes** | 312,441.12 USD (the book's own comment says 312,441.12) |
| `v4-sketch` | mortgage | 2024-02-20 | 2026-01-01 | no | **yes** | 312,441.12 USD |
| `explore-v5/02-family` | mortgage | 2021-01-15 | 2026-01-01 | no | **yes** | 462,302.32 USD |
| | mortgage-2, car-loan | 2026-05-20, 2026-02-14 | 2026-01-01 | no | no (made in the book) | |
| `explore-v5/03-triplex` | mortgage | 2022-08-12 | 2026-01-01 | no | **yes** | 356,181.05 USD |
| `explore-v5/05-budgeter` | boots | 2026-03-06 | 2026-01-01 | no | no | |
| `explore-v5/06-family-addresses` | the two of `05-family` | 2025-12-31 | 2024-12-31 | no | no | |

Four examples change, and not goldens: `11-sam`, `v4-sketch`, `02-family`, `03-triplex`. `05-family` and `07-landlord`, the two goldens
with a loan, are unmoved by construction. **A loan made after the book began with no origination line**
(`02-family`'s `mortgage-2` and `car-loan`, whose `sf-fcu holds -417.39 USD, not 28,628.86 USD` is the same fault) also leaves its tab at
zero. It is not this lane's: an implied *origination* would have to say where the cash arrived, and the book's own journal may have
already (§5.6). Listed in the report.

## 5. The decisions

1. **When is a loan before the book?** `on < F`, F being `Book::first_fact()`, or **always** when the book has no fact. The loan
   opens on `begins = F`, or, for a book with no fact, on its own `on` day; its debt is `owed_before(begins)`: after every payment due
   before that day. One formula; the two cases differ by what the day is. For a loan with no fact in the book this
   is the origination K5d reads, made by the terms: `on == begins`, `owed_before(on)` is the principal, the debt opens with all of
   it and the book begins with the loan. The opening is a flow, so it is the first fact: the monitor then watches from `on`, and
   a book that declares a loan in 2024 and writes nothing warns once (`missed-occurrence`: due N times, last on ..., the grouping
   K5d made) that its payments are not kept. That is what `2024-02-20 mortgage` alone does today; it is the language working.
   (A book with no journal that would rather not be told writes nothing and the loan stays unopened: it only has to not declare
   a loan.) *Not built*: opening at `today - 1`, which the lowering cannot know.
2. **How big is the opening when payments were missed or prepaid before the book?** The schedule's balance: the lender's terms,
   which are what the language says a loan is (LANGUAGE §7). What the lender's own number is, the statement says, and a statement
   after F reconciles it (`loan-balance`, whose "a payment was missed" no longer counts the payments before F: §0.7). The note says what
   the number is and what to write to override it.
3. **An `opening` that names the debt tab wins.** `opening ... / mortgage 312_441.12 USD`: the name `mortgage` is the contract's
   name and resolves to the debt tab (§0.5). Decided **before the journal is lowered**, from the opening blocks' lines
   (`collected.openings`, whichever day, whether or not it is on F): a block that names the tab is the user's number and the
   implied one is **not made** (so there is never both). It is the *name* that is read: a line that reaches the tab another way
   (there is no other way: `special_end` answers first) would be read as it is lowered.
   An origination line wins by being a fact (§3). Neither is overridden and neither warns.
4. **The one diagnostic.** When the implied opening is made, `check` says so once per loan, `Severity::Note`, code `loan-opening`:
   the loan, the day, the amount and how it is reckoned, the line of the loan it comes from, and the fix **as an edit**: the opening
   block that would override it, inserted before the first record of the book (in the common case the book's first line is an
   `opening`, and a second `opening` on one day is valid). Not for a loan paid off before the book began (nothing is owed: nothing is
   opened, nothing said).
5. **A book with no fact** is §5.1: no fix is offered (there is no line to put it before), the note says so.
6. **A loan whose reset reads an index the book does not give** before F cannot say what is owed: the schedule `halted` (`Walked`, amortization.rs:167). No
   opening is made (no guess); the missing index is the diagnostic that already exists.
7. **A loan made in the book with no origination line** (§4) is as K5d left it. Said, not built.

## 6. The design

```text
model/book.rs                 Book::first_fact()                                      moved from engine/timeline.rs (-9 there)
model/promise.rs              Promises::owed_before(book, contract, day) -> Option<Qty>; amortize split so lowering can walk
model/promise/amortization.rs Amortization::owed_before(day); Said.begins (the day the book begins), set by Said::of
model/promise/causes.rs       explain: payments from said.begins only
model/journal.rs              Derivation::Opening(Id<Contract>)
model/lower/loan_opening.rs   NEW: which loans are unopened, the opening's flow and transaction, the note        ~110 lines
model/lower/record.rs         record(): the loop asks the unopened loans to open once the first fact is made     ~+10
engine/plan.rs, timeline.rs   Plan reads book.first_fact(); the function goes
report/register.rs            the word for the new derivation (contract_flow_word, contract_flow)                 +2
```

Nothing in `engine/{ledger,state,post,fire}.rs`, nothing in `engine/loan_balance.rs`. The fold sees one more `Mode::Opening` flow on day F.

`record()` stays one loop. Its hook costs one `Vec::is_empty` per record for a book with no unopened loan (every bench), and for a
book with one the check `first_fact()` per record until the book has begun (an O(1) read of the first flow, assertion, split and claim
change, and of the first transaction that is an occurrence only when `written_occurrences` is not empty).

## 7. Why the amount is the brief's

The brief says "the schedule's balance on the day before the first fact". `owed_before(F)` is the schedule's balance on F-1 (for F =
`on`, all of the principal: nothing is due on the day a loan is made). The opening is dated F and not F-1 for the arena's sake (§0.2),
but the number is the same, and the book's own payment on F is the first to leave it.

**It is the same number the schedule compiled later gives.** The walk is `walk` over the same dues and the same rates, with no
prepayments and no lines (which are facts, so are on or after F). A test compares `owed_before(F)` of the lowering with `open_on(F-1)`
of the compiled schedule for every loan of every example, and the oracle compares the tab on the day of the run, which is the
opening less what the book's lines then posted. The one input it cannot see is a waiver, a `now at` or an `ends` *dated on or after F* that
reaches back to a due day before F (a waiver names the nearest due day within its grace): the compiled schedule then
differs by that payment and the opening does not. It is a book that waives a payment from before it began; said, not handled.

## 8. The plan, and what proves each step

| step | what | proof |
|---|---|---|
| 0 | this map; the baseline binary | |
| 1 | the oracle first: `loans.py` gains books whose first fact is after the loan's day (the opening block of the book dated after it, no origination line), with and without an `opening` for the tab, statements, prepayments, resets, rates, missed payments; the reference says the opening, the tab and the causes | the reference's own `selftest`; the baseline *fails* them (negative tab, missed counts pre-book) |
| 2 | `model`: `Book::first_fact` (engine reads it), `owed_before`, `Said.begins`, the cause; unit tests | tests; the baseline's goldens unchanged |
| 3 | `model`: `lower/loan_opening.rs`, the hook in `record`, `Derivation::Opening`, the note | model tests; the oracle: 0 disagreements |
| 4 | goldens and examples, mistake books for the note (a book that makes it, a book whose opening wins, a book with no fact) | `git diff tests/` listed line by line |
| 5 | the mutation sweep of the new code; `fuzz.py diff`, `splits.py`, `claims.py`; the K7b histories oracle; timings on `bench/` | every mutant killed; zero differences on books with no loan |
| 6 | LANGUAGE §5 (the sentence "a loan's balance comes from its terms and needs no opening line" is now true), §7; this map's last section | |

## 9. How this map was checked

* The code read in full: record.rs:1-215, 508-741, 1122-1267, 1559-1590; staged.rs; resolve.rs:225-275; promise.rs:100-345;
  promise/{amortization,annuity,causes}.rs; engine/{timeline,plan,ledger}.rs (the first fact, the start, the streams);
  engine/loan_balance.rs; model/book.rs (`Contract`, `Txn`, `Flow`'s lookup), journal.rs (`Flow`, `Origin`, `Derivation`).
* §0.5 on the baseline binary (`scratchpad/k5e/p/sam`), §0.6 from its `check` output, §0.2's claim that a declarations-only book is
  watched from today on the baseline (`scratchpad/k5e/p/decl`: `✓ 0 flows`, a forecast whose tab goes negative).
* §4 by a scratch test in `crates/session/tests/` (deleted: it was never committed) that read each example's book and printed
  the loan, its first fact and its schedule's balance the day before.
