# Axiom v5: the design the lanes build

[`PROPOSAL.md`](PROPOSAL.md) is the review and the case for v5: what is wrong with the cutover, and why seven kernels.
This file is what the lanes build. It folds in the research lane (R5:
[`research/REPORT.md`](research/REPORT.md), [`ASSOCIATIONS.md`](research/ASSOCIATIONS.md),
[`THEORY.md`](research/THEORY.md) and seven ledgers) and the orchestrator's verdict on each of its proposals. Where the
two files disagree, this one wins.

The design rests on one sentence:

> **Everything the book declares is a thing of a kind; everything said about a thing is a slot on a timeline; every
> movement is a leg between two positions; every promise is a term reduced by the fold; and the fold's state is a
> trail, so going back in time is as cheap as going forward.**

Five structures carry it. Each has one good algorithm.

| structure | holds | algorithm |
|---|---|---|
| `Kinds` (K1) | kinds, their slots, parts and laws | pre-order tree; slot lookup walks a kind's ancestors, an id range |
| `Facts` (K2) | every slot of every thing, as steps on days | CSR of (thing, slot) → steps; values in a tag column beside a 16-byte payload column |
| `Addresses` | how journal words find things | posting lists, intersected by galloping merge; forced placement by bitmask |
| `Events` (K4) | every leg, solved where it can be | struct of arrays; constant folding at model time; an affine split check |
| `Trail` (K3, K5, K7) | the fold's mutable state | undo log: a mark is a checkpoint, a fork, a staged write and a forecast boundary at once |

---

## 1. Verdicts on R5

R5's work is strong. It found the root of the `jordan-401k` complaint in three lines of `us/*.ax` (`has employer entity`,
`has beneficiary entity`, `has coverage name`), classified all 105 FINDINGS, and wrote seven books in the proposed syntax.
Its research was cut short by blocked hosts, and it says so. Here is what v5 takes from it.

| R5 | verdict | why, and what changes from R5's text |
|---|---|---|
| **A1** typed slots: range, count, closed world | **adopt** | The answer to the complaint. `entity` and `name` stop being slot types. |
| **A2** filling by nesting, path, role word and default; addresses as day-aware subsequences | **adopt, cut one channel** | Bare words in a header (`family/529 riley`) are a fourth way to say what the path already says. They go: `family/riley/529` places `riley` by the same rule. Four channels remain: nesting, path, role line and default. |
| A2 forced placement ("a word is placed only if every placement agrees") | **adopt** | It is the right rule: it never guesses. It is computed by bitmask enumeration (§3.4). |
| **A3** relator kinds that carry legs, laws and lifecycle; `joins`; parts | **adopt** | R5's two sketches of employment disagree (`household.ax` uses `joins`, `small-business.ax` puts `plan` and `deferral` on the employment). v5 takes `joins`, because a membership has its own election, its own position and its own end (§2.3). |
| A3 relator legs written per perspective (`when employer is owner`, every FICA leg twice) | **replace** | A relator's legs are written once, in role terms, and each book **projects** them: a leg touches the book only where one of its ends is the book's own (choreographic projection, THEORY T3). The `when employer is owner` lines and their duplicates go (§2.3). |
| A3 `verb "{employee} works at {employer}"` | **reject** | A mixfix grammar declared in the book is a parser inside the parser. The contract form says the same thing, and the pay contract *is* the employment. |
| **A12** roles derived on a day; `dependents`; `DaySet` | **adopt** | `children 1`, which never ages, becomes `count(dependents where age < 17)`. |
| A12 kinds as a set (`entity oakcraft : vendor, client`) | **reject, for a stronger rule** | In UFO a kind is rigid and a role is anti-rigid. `client` is a role a thing plays *because* a relator exists (an engagement), not a second kind. Oakcraft is an `org`; its engagement makes it a client; its purchases make it a vendor. The party-kind purpose rule reads the relator, so one thing can be both without multiple classification. |
| **A4** weighted many-slots; `-> owners by share`; K-1 by day | **adopt** | One syntax replaces `owner X 60%, Y 40%` and `share 120 SQFT for studio`. |
| **A6** two calendars: pay period and pay date, business-day roll, `for the tax year` | **adopt** | It goes into K5's `Every` (a schedule has a period and a payment rule). |
| **A9** `kind stock-in-trade` with `sells` | **adapt** | R5 derives cost of goods by a law (`on gain derive -> basis #cost-of-goods`) that writes a leg to no party. That turns a gain into two flows *after the fact*. The REA reading is simpler: a sale of stock-in-trade *is* an exchange, whose given side, at basis, is `#cost-of-goods` and whose taken side is `#sales`. So the commodity kind classifies its own disposal: `sells #sales at #cost-of-goods`. No gain is realized and no leg is derived (§2.5). |
| **A8** claims in kind with consideration; bullet principal; set-off | **adopt** | A claim is a parcel of any commodity (`144 CLUB_BOX`), with basis equal to the consideration. Deferred revenue, gift cards, written options and the club are one shape. Set-off is the Pacioli normal form, a view, not a keyword. |
| **A10** `into`: an exchange between two of my positions | **adopt** | `Leg { from, to }` already has both ends. |
| **A5** opening tallies and pending carries | **adopt** | `opening` populates the tally store and the carry list. |
| **A7** interest as a term of a position | **adopt** | `Term::Accrue`. |
| **A11** options: `Term::Choose` | **adopt** | The forecast takes `default` until an event chooses. |
| PROPOSAL defect: `Term::At` missing | **fix** | Added (§3.7). |
| PROPOSAL defect: §6.3 drops the `income` root, and `30% of #income` loses its base | **fix** | `income` and `spending` stay as **derived classes**: the set of flows whose direction makes them so. They are not roots of the purpose tree; they are valid wherever a purpose set is read (`30% of income`). |
| T2: move-only `Parcel` with a `Drop` guard and a `consumed` flag | **reject the guard, keep the point** | Parcels live in columns (§3.6), not as objects, so there is nothing to drop. Conservation is enforced where it can fail: the static split check (§3.5), `#[must_use]` on every relief result, and no wildcard arm over `Fact`. |
| T4: Z-sets, month checkpoints, persistent maps (`im`, `rpds`) | **adapt, without the dependencies** | Checkpoints are trail marks (§3.8). No persistent map is needed, so no dependency. |
| T5 Datalog, T6 Pacioli, T7 Catala, T8 ACTUS/Marlowe, T9 Allen, T10 XBRL, T11 lenses, T12 CRDT | **as R5 says** | Semantics and oracles, not runtimes. `ledger-semantics` (Lean 4) becomes a property-test oracle when K7 lands. |
| `SmallVec`, `Vec<Slot>` and `Vec<(Ref, Weight)>` in the type sketches | **reject** | No dependency, and no per-node vectors: `Run<T>` into one arena. |

What R5 leaves open, and v5 leaves open with it:
- **Two bases**: books on accrual, a return on cash with other depreciation.
- **Documents out**: W-2, 941, K-1, 2555 and FBAR forms.
- **Order-level sales tax**: a rate set by the ship-to address.
- **Content**: `pt`, `us/feie` and `us/s-corp` are not written.

Section 8 says how each would fit without a new kernel.

---

## 2. The model

### 2.1 Things, kinds and slots (K1)

A **thing** is anything declared: a person, an org, a position, an asset, a commodity, a contract, a plan or a membership.
Every thing has exactly one **kind**, and kinds form one tree under five roots, the **sorts**:
- `agent`;
- `position`;
- `asset`;
- `commodity`;
- `contract`.

The sort is fixed by the root a kind descends from, and is the only thing the engine branches on.

A kind declares **slots**:

```text
SLOT  := has NAME RANGE [MULT] [by WEIGHT] [as with | as for] [= DEFAULT]
RANGE := KIND (| KIND)* | one of WORD (| WORD)* | VALUE-TYPE
MULT  := (nothing: exactly one) | optional | some | many
```

- **The range is checked once.** The model checks it when the slot is filled, and nothing downstream checks again.
- **Multiplicity is counted, and the world is closed.** A required slot that stays empty is an error, and so is a
  slot filled twice.
- **A subkind may repeat a slot only to narrow it.** It may name a sub-range, a tighter count, or a new default.

Every built-in property v4 hard-codes becomes a slot that `std.ax` declares on a root kind:
- `owner`, `opened` and `closed` on `position`;
- `lives`, `citizen`, `born`, `filing` and `currency` on `agent`;
- `precision` on `commodity`;
- and so on.

The engine reads the few it needs through typed keys (§3.2). That deletes the per-field parsers in `props.rs`:
`Assign` and its five `set` impls. It also deletes the fields of `Place`, `Entity` and `Kind` that each hold one
property.

### 2.2 Facts: one store for everything said about a thing (K2)

A slot's value is a **timeline**:
- the declaration is its first step;
- `DATE THING now SLOT VALUE` adds a step;
- `until` restores the value before.

Every value that changes on a day lives here, under one key, (thing, slot):
- properties;
- residence;
- terms;
- prices and params;
- owners' shares;
- a plan's match;
- the budget.

Reading a slot on a day is a binary search. Counting the days a condition holds is an integral over the steps, with no
sampling. The days themselves are a `DaySet`. A **default** (`= employment[employee owner].employer`) is evaluated once,
on the day the thing begins, and stored as its first step. History does not move when the employment does. R5's
warning stands: a default is a guess about the past, and when it finds nothing or more than one, the error is
`unfilled-default`, with the role word as the fix.

### 2.3 Relators: what an association implies

A **relator** is a thing with two or more agent slots: an employment, a plan membership, a lease, a management agreement,
an engagement. It may carry:

| | word | replaces |
|---|---|---|
| participants | `has` slots | free property lines, `at`/`with` plumbing, `member`, `client_of` |
| positions that exist with it | `part` | the hand-opened escrow; v4's `part of` |
| the legs it implies | `also` | per-contract legs; `Match`; `lower/also.rs` |
| its own life | `law … on start` / `on end` | hand-typed final pay, forfeiture, deposit settlement |
| what it schedules | its contract body | unchanged: a promise (K5) |

**Projection.** A relator's legs are written once, with roles as their ends:

```text
kind employment : contract
  has employee person
  has employer agent as with
  has joins    plan many                                   // each `joins` makes a membership
  employer -> employee gross #wages                        // the body: what each occurrence pays
    - withholding(gross - deferred, employee.filing) -> irs #federal-tax
    - 7.65% of gross -> irs #payroll-tax
  also employer -> irs 7.65% of gross #payroll-tax
  also employer -> irs futa(gross, total(#wages of employee, year)) #futa
```

The body keeps v4's split semantics. The header is what the payer pays, and each `-` item redirects part of it to
another end. So the withholding is the employee's money, attributed to the employee and paid by the employer.

The fold **projects** every leg and item onto the book:
- an end that is one of the book's owners is that owner's position;
- any other end is the outside;
- a leg with both ends outside does not touch the book.

What each book sees:
- **The household's book.** Wages arrive at gross; withholding and FICA leave as tax paid; net lands in checking. The
  employer's two `also` lines have both ends outside, so they vanish.
- **The business's book.** Wages leave at gross. The items are paid by the business to the IRS on the employee's
  behalf, which is v4's `for`. The `also` lines post the employer's own FICA half and FUTA.

The kind is written once and is true from both sides. That is choreographic projection, Montesi's *endpoint
projection*, restricted to a single round. It removes every `when employer is owner` and the second copy of every FICA
line.

**Membership.** `joins PLAN [ELECTION]` inside an employment makes a thing whose kind the plan names:

```text
kind 401k-plan : plan
  has sponsor   employer                                  // filled by nesting: the plan is declared under its sponsor
  has custodian broker
  has match     tiers = none
  has vests     schedule = immediate
  member 401k-membership
kind 401k-membership : membership                         // employment × plan, with its own election and position
  has deferral  percent = 0%
  part account : 401k                                     // owner = the employee; sponsor and custodian = the plan's
  pay - deferral * gross -> account #deferral   when eligible   // `pay -` adds an item to the employment's body: the employee's money
  also employer -> account plan.match(deferral, gross) #match  when eligible
  law vesting on end
    derive account -> plan.sponsor (1 - plan.vests(service)) * match-in(account) #forfeiture
```

The paycheck is then one line: `contract alex-pay : employment with acme`, then `employee alex`, the amount and
schedule, and `joins 401k-plan deferral 10%`. Withholding, FICA, the deferral, the match and the plan's position follow
from the kinds.

### 2.4 Addresses are definite descriptions

A thing's **canonical address** is the path of its agent and asset slot fillers, in the kind's slot order, then its name:
`jordan/bluefin/fidelity/401k`. A journal reference is **any subsequence that denotes exactly one thing open on the
line's day**. This is Russell's ι: `jordan/401k` means *the* 401(k) with Jordan among its fillers.
- **None** is `unknown-address`, offering the nearest name.
- **Several** is `ambiguous-address`, listing each candidate's shortest unique address. That list is also the code
  action.

Declarations use the same path: the words before the name fill slots by forced placement (§3.4).
- **Nesting** fills the slot marked `as with`.
- **A kind's name stands for the name** (`alex/401k`). A different name needs a kind: `family/checking : deposit`.

Entities and assets keep flat names. A relator's slots read as fields: `alex-pay.employer`.

R5's ladder holds: `alex/401k` → `jordan/bluefin/401k` → `jordan/401k sponsor bluefin` → every slot written. The book
gets more precise exactly where precision stops being free.

### 2.5 Commodities classify their own disposal

```text
kind stock-in-trade : commodity
  sells #sales at #cost-of-goods         // an exchange giving this away: the taken side is #sales, the given side at basis #cost-of-goods
  select fifo
```

**Relief does not change:** parcels leave FIFO, and their basis is the cost. What changes is classification. The given
side is valued at basis and tallied under the cost purpose, and the taken side is tallied under the sales purpose. No
gain is recognized and no leg is derived.
- **A return** puts the parcel back at the basis it left with (provenance).
- **Shrinkage** is an assertion `=` whose gap is `via #shrinkage`.
- **A landed cost** (`#landed-cost of ^pi-1140`) is a `rebase` of that purchase's parcels, pro rata to cost.

### 2.6 Claims are parcels of any commodity

A claim is a parcel held in a position with the debtor, so the claim can be of any commodity:
- `lantern-row owes market 144 CLUB_BOX monthly over 12m received 2_880.00 USD` makes a claim of 144 boxes with
  basis 2,880;
- each delivery relieves 12 of them, and recognizes 240 as `#subscriptions`.

The same shape covers:
- a gift card;
- a written option (`brokerage owes market 1 LMNT_C75_0417 received 235.00 USD`);
- deferred revenue.

Settlement is relief: by code, then exact amount, then FIFO. That is v4 §7 as written. Aging is the parcel's day.
**Set-off** is the Pacioli normal form of a pair's claims: a view, never a keyword.

### 2.7 Income and spending are derived classes

Purposes are directionless (PROPOSAL 6.3). `income` and `spending` are not roots of the purpose tree. They are two
derived sets, the flows whose direction from the owner makes them so. They can be read anywhere a purpose set is read:
`budget reserve 30% of income`, `tally(spending, month)`. That fixes the regression R5 found (fl-b07).

---

## 3. The data structures

Every layout below is a commitment: the lanes may improve it, never regress it. Sizes are asserted in code.

### 3.1 `core::tagless`: a column of mixed values

The fold and the facts store read long streams of values of mixed type: amounts, days, ids, ratios, symbols. An enum
`Value` is 24–32 bytes and branches on every read. `tagless` stores the same stream as two columns:

```rust
/// One byte per value: what the payload column holds at the same index.
#[repr(u8)] #[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tag { Empty, Bool, Num, Amount, Day, Span, Ratio, Sym, Thing, Days, Run }

/// Sixteen bytes, every field `Copy`. Which field is live is the tag at the same index.
#[derive(Clone, Copy)] #[repr(C)]
pub union Payload { bool: bool, num: Ratio, amount: (Qty, Id<Commodity>), day: Day, days: Days, sym: Sym, thing: u32, run: (u32, u32) }

/// A column of tagged values. Constructors write tag and payload together; readers that know the type from the
/// schema (a `Key<V>`) read the payload directly, and `debug_assert!` the tag.
pub struct Column { tags: Vec<Tag>, payloads: Vec<Payload> }

pub trait Field: Copy { const TAG: Tag; fn put(self) -> Payload; /* SAFETY contract: */ unsafe fn get(p: Payload) -> Self; }
impl Column {
    pub fn push<V: Field>(&mut self, v: V) -> u32;
    pub fn get<V: Field>(&self, at: u32) -> V;       // the one `unsafe` read, behind `debug_assert_eq!(self.tags[at], V::TAG)`
    pub fn value(&self, at: u32) -> Value;           // the checked, branching read, for diagnostics and views
}
const _: () = assert!(size_of::<Payload>() == 16);
```

- **The invariant** is `tags[i]` names the live field of `payloads[i]`. `push` is the only writer, so it holds by
  construction.
- **The safety argument** is that `get::<V>` is called only where the schema has already proved the type: a slot's
  declared range, checked when the slot was filled.
- **The tests** cover every `Tag` with a round trip. A property test writes random typed values and reads them back
  checked.
- **Who uses it:** the facts store (§3.2) and the leg quantities (§3.5).

### 3.2 `Facts`: compressed rows of timelines, with typed keys

```rust
/// A slot of a kind, and the type its values have, known at compile time for the slots the engine reads.
pub struct Key<V> { slot: SlotId, of: PhantomData<fn() -> V> }
pub const OWNER: Key<Id<Thing>> = …;   pub const LIVES: Key<Many<Id<System>>> = …;   pub const OPENED: Key<Day> = …;

/// Every slot of every thing, as steps. Rows are things (CSR offsets); within a row, slots are sorted, and each
/// slot's steps are a run of `(Day, value)` sorted by day. Values are a tagless column.
pub struct Facts {
    rows: Vec<u32>,              // thing → first entry
    slots: Vec<SlotId>,          // entry → slot, sorted within a row
    steps: Vec<Run<Step>>,       // entry → its steps
    days: Vec<Day>,              // step → from
    values: Column,              // step → value
}
impl Facts {
    pub fn at<V: Field>(&self, key: Key<V>, thing: Id<Thing>, day: Day) -> Option<V>;   // two binary searches
    pub fn timeline<V: Field>(&self, key: Key<V>, thing: Id<Thing>) -> Steps<'_, V>;    // a borrowed stepper
    pub fn days_where(&self, …, within: Days, holds: impl Fn(V) -> bool) -> DaySet;    // an integral, not a sample
}
```

- **Built once, frozen, then borrowed.** Facts is built by one counting sort over all declarations and statements, then
  frozen. The fold borrows it: `Plan<'b, 's>` holds `&'b Facts`.
- **A kind's facts are its things' defaults.** Lookup falls back up the kind's ancestors, which are a contiguous
  pre-order range, so the fallback is a short loop over a slice.
- **`DaySet` is `Run<Days>` into one arena of disjoint sorted intervals.** Union, intersection and difference are linear
  merges. `len()` is a sum. `earliest_reaching(n, window)` (the FEIE test, the 183-day rule) is a two-pointer sweep.
- **Absorbs:**
  - `props.rs` (1,931 lines);
  - `params.rs`, `values.rs` and `prices.rs`;
  - the `Residence`, `Timeline<Terms>` and `Timeline<BudgetTerms>` fields;
  - `engine/temporal.rs` and `sample_temporal*`.

### 3.3 `Addresses`: posting lists

```rust
/// For each word (a `Sym`), the things whose canonical address contains it, sorted by id: a search engine's
/// inverted index. `slot_of[i]` says which slot of the thing the word filled, so a subsequence can be checked in order.
pub struct Addresses { postings: Groups<Sym, (Id<Thing>, u8)>, opened: Vec<Days> }

impl Addresses {
    /// The things a path denotes on `day`: the galloping intersection of its words' posting lists (shortest first),
    /// filtered by order and by `opened`. Exactly one is a resolution; none or several is a diagnostic with
    /// candidates.
    pub fn resolve(&self, path: &[Sym], day: Day) -> Resolution;
}
```

- **Intersection is a merge.** Intersecting sorted `u32` lists is a merge that gallops: exponential search into the
  longer list. That costs O(k log(n/k)) for lists of length k ≤ n, and it is the one place in the model where
  `fearless_simd` can pay.
- **One resolution per distinct path.** The journal repeats addresses, so resolutions are memoized per (path, the span
  of days on which the answer is constant). Each distinct path is resolved once.
- **It replaces:**
  - `Scoped<T>` lookups for positions;
  - the survey and tab prediction (K3);
  - the `fidelity-brokerage` renames.

### 3.4 Forced placement: a bitmask search

A declaration's words go into the slots of its kind. There are at most 8 words and 16 slots.

```rust
/// cand[w]: the slots word w's kinds admit, as a bit set. cap[s]: 1 for `one`/`optional`, unbounded for `some`/`many`.
/// Enumerate every placement by depth-first search over words, keeping per-word unions of where it landed.
/// A word whose union is a single bit is forced; any other word is `ambiguous-role`, naming the bits.
fn place(cand: &[u16], cap: u16 /* bits of single-capacity slots */) -> Result<[u8; 8], Placement>;
```

- **Size.** Eight words over sixteen slots is at most 16⁸ leaves in theory, and in practice a handful: each word has
  one or two candidates.
- **Speed.** Unit propagation first (a word with one candidate claims its slot), then the search, so most declarations
  never branch.
- **No ambiguity is possible in the representation.** A placement is an array, so no slot holds two words by accident.

### 3.5 `Events`: one IR, in columns (K4)

```rust
pub struct Events {
    day: Vec<Day>, verb: Vec<Verb>, legs: Vec<Run<Leg>>, tail: Vec<Id<Tail>>, at: Vec<Loc>,   // per event
    from: Vec<Id<Position>>, to: Vec<Id<Position>>, why: Vec<Purposed>, qty: Column,          // per leg
}
pub enum Q { Known(Amount), Expr(NodeId), Rest, Target(Amount), Unknown(Id<Commodity>), All(Option<Id<Commodity>>) }
```

- **The fold's hot loop** reads `from`, `to` and `qty` for a contiguous run of legs: three dense columns.
- **`Q` is stored tagless in `qty`.** Literal transactions, nearly all of them, are solved at model time by `solve` with
  a `LiteralEnv` (constant folding), and only `Known` survives. The fold re-solves only legs whose tag is `Expr` or
  `Rest`.
- **The split check is static.** Every leg of a template is an affine form `a·x + b` in the header amount. A split
  conserves iff, per commodity, `Σa = 1` and `Σb = 0`, or one leg is `Rest`. This is `solve`'s linear algebra used to
  check instead of solve, and it runs once at model time.
- **Zero postings are trimmed**, as Numscript does.
- **Elaboration writes through `Staged`** (landed in K0a), and later through the trail (§3.8).

### 3.6 Parcels (K3): columns, relieved by a lazy heap

```rust
/// Every parcel ever opened, in columns. A position's open parcels are a run of indices kept in relief order;
/// HIFO keeps today's lazy `BinaryHeap<Ranked>` and compares basis per unit by cross-multiplication.
pub struct Parcels {
    qty: Vec<Qty>, basis: Vec<Qty>, acquired: Vec<Day>, held_since: Vec<Day>,
    origin: Vec<Id<Event>>, part: Vec<Option<PartId>>, commodity: Vec<Id<Commodity>>,
}
```

- **`Parcel` today is about 80 bytes and is scanned whole.** Relief reads `qty`, `basis` and one ordering column, so in
  columns relief touches about a quarter of the bytes.
- **A claim is a parcel in a position with the debtor.**
- **An asset is a position holding one parcel per part.**
  - Depreciation is `rebase`.
  - A wash sale is `rebase` on the matched lot.

  The `AssetState` store goes.
- **Settlement is relief:** `[^code]`, then the new `Exact` policy, then FIFO.

### 3.7 Promises (K5): terms, residuals and a deadline heap

```rust
pub enum Term {
    Done, Pay(Id<LegTemplate>), All(Run<Term>), Every { schedule: Id<Schedule>, body: TermId },
    Due { grace: Span, blame: Role, body: TermId, otherwise: TermId }, At { day: DayExpr, body: TermId },
    If { cond: NodeId, then: TermId, otherwise: TermId }, Let { name: Sym, value: NodeId, body: TermId },
    Annuity(Id<Loan>), Accrue { rate: NodeId, on: Balance, pay: Id<Schedule> },
    Choose { by: Role, until: DayExpr, options: Run<(Sym, TermId)>, default: TermId },
}
pub struct Schedule { period: Cadence, paid: Option<Cadence>, roll: Roll }    // A6: the pay period and the pay date
```

- **A promise's state is a residual:** the term still owed, as a cursor into a post-order arena.
- **The monitor reduces residuals as events arrive.** This is residuation (Andersen et al., CSL).
- **A min-heap of (deadline, promise) fires `Due` timeouts**, with `otherwise` and blame.
- **The forecast is the same fold past `today`,** with `Choose` taking its `default`.
- **The occurrence ordinal is a binary search** over the schedule's days, not a count from the contract's start.
- **The four "which occurrence is due" implementations become one.**

### 3.8 The trail: one mechanism for staging, forks, checkpoints, forecasts and live edits

The fold's mutable state lives in dense arrays:
- the parcel columns;
- tallies;
- residuals;
- totals.

Every write to them goes through a **trail**, the Warren Abstract Machine's undo log:

```rust
/// Dense state with an undo log. `mark()` is O(1); `undo(mark)` restores every cell written since, in reverse.
pub struct Trailed<T: Copy, L: Log<T> = Undo<T>> { cells: Vec<T>, log: L }
pub trait Log<T> { fn record(&mut self, at: u32, old: T); fn mark(&self) -> Mark; fn undo(&mut self, to: Mark, cells: &mut Vec<T>); }
pub struct Undo<T> { entries: Vec<(u32, T)>, appended: Vec<u32> /* lengths at each push, to truncate appends */ }
impl<T> Log<T> for () { #[inline(always)] fn record(&mut self, _: u32, _: T) {} … }   // a one-shot `check` pays nothing
```

One structure, five uses. Each of them is a separate mechanism today.

| use | today | with the trail |
|---|---|---|
| staged elaboration | `Staged` truncates five arenas | a mark; drop undoes |
| `available` and what-if | the ledger forked twice, by copy | mark, fold the hypothetical, read, undo |
| the forecast | `contract_forecasts`, a second driver | mark at `today`, fold past it, read, undo |
| `--at DATE` views | `Plan::new` and a whole fold per query | marks at month ends are checkpoints; undo back to the nearest, fold forward to the date |
| a live edit (MCP, GUI) | rebuild everything | undo to the checkpoint before the edit's day, re-fold, stop early once a later checkpoint's state hash agrees |

- **Zero cost when unused.** The log is a type parameter. The CLI's one-shot `check` uses `()`, which monomorphizes to
  nothing. A session uses `Undo<T>`.
- **The borrow checker makes a fork safe.** A `Fork<'r>` guard holds `&mut Run` and undoes on drop, so no view can read
  a hypothetical after it is gone.
- **What it replaces:**
  - the "five folds" of PROPOSAL F4;
  - `projection.rs`;
  - `holdings_at`'s re-plan;
  - the double fork in `available`;
  - THEORY T4's persistent maps.

### 3.9 Facts out (K7)

- **Position histories are steppers recorded during the fold:** one `(day, balance)` row per change, per position and
  commodity.
- **`peak` and `low` over a window use a sparse table** built over a history on first use: O(n log n) to build, O(1) per
  query. This covers FBAR with no sampling.
- **Views are pivots over the posting stream:** rows by a dimension, columns by period. The linear ones (balances,
  tallies, flows by purpose, claims) take deltas (Z-sets, THEORY T4). After a live edit, an open view patches itself
  from the delta between the checkpoint and the re-fold.
- **`why` is a provenance walk:** posting → leg → event → statement → line. It is not a page per target.

### 3.10 The session: the one surface an MCP server and a GUI need

```rust
pub struct Session<'s> { sources: Sources<'s>, book: Book<'s>, run: Run<Undo> }
impl Session<'_> {
    pub fn diagnostics(&self) -> &[Diagnostic];
    pub fn query(&self, q: Query) -> View;                 // balance, flow, claims, tally, why, forecast … as data
    pub fn what_if(&mut self, edit: &Edit) -> View;        // a fork: mark, apply, fold, read, undo
    pub fn apply(&mut self, edit: Edit) -> Applied;        // re-parse the file, rebuild what changed, re-fold from a checkpoint
}
```

- **Lanes do not build `Session`.** K7 builds it.
- **Every lane keeps it buildable:** libraries never print, every value is typed and immutable once built, and nothing
  holds global state.

---

## 4. Algorithms at a glance

| problem | algorithm | cost |
|---|---|---|
| kind and purpose trees, `is` | pre-order tree; a subtree is an id range | O(1) `is` |
| a slot on a day | two binary searches in CSR rows | O(log n) |
| days a condition holds | integral over steps; `DaySet` merges | O(steps) |
| FEIE/183-day windows | two-pointer sweep over a `DaySet` | O(intervals) |
| address resolution | galloping intersection of posting lists, memoized per path | O(k log(n/k)) once per path |
| filling slots | unit propagation, then bitmask search | tiny, bounded |
| literal splits | constant folding at model time | once |
| split conservation | affine check `Σa = 1, Σb = 0` | once per template |
| relief | FIFO run; HIFO lazy heap by cross-multiplication; prorata exact shares | O(log n) per pick |
| settlement | code, then exact, then FIFO | as relief |
| promise monitoring | residuation over a post-order term arena; a deadline min-heap | O(log n) per event |
| occurrence ordinal | binary search in the schedule's days | O(log n) |
| ownership | one walk, owners before the owned, composed rows in an arena (landed in K0b) | O(edges) |
| peak/low | sparse table | O(1) per query |
| forks, forecast, `--at`, live edits | trail marks and undo | O(writes since the mark) |
| parallelism | files parsed in parallel; owners whose books never touch folded in parallel; views in parallel | `core::par` |

---

## 5. What the language becomes, beyond PROPOSAL §6

Additions, each paying for itself by what it removes:

| added | removes |
|---|---|
| `has NAME RANGE MULT [by W] [= DEFAULT]` with kinds as ranges | `entity`/`name` slot types; `employer`, `coverage`, `beneficiary`, `member`, `client_of` as fields; the per-field parsers |
| paths fill slots (`jordan/bluefin/401k`) and address things | `account … at`; `owner` lines; names that carry relations; the survey |
| `joins PLAN [ELECTION]`, `member KIND`, `part NAME : KIND` | per-paycheck legs; `matching`; the hand-opened escrow; `part of` |
| `on start`/`on end` triggers | hand-typed final pay, forfeiture and deposit settlement |
| relator legs in role terms, projected per book | `when employer is owner` and every leg written twice |
| `sells #P at #C` on commodity kinds | a capital gain on merchandise; a hand-typed cost of goods |
| `received AMOUNT` on a claim | deferred revenue and option writing as special cases |
| `into POSITION` | the one-subject-position limit of `@` |
| `opening … tally …` and `carry …` | the back-filled prior year |
| `interest R on the daily balance, paid CADENCE` | twelve typed interest lines a year per account |
| `in arrears paid DAYS`, business-day roll | hand-moved weekend dates |
| `income`, `spending` as derived classes | the regression of `% of #income` |

Removed outright: the `account` keyword; `at PARTY`; `kind receivable` and `kind payable`; the `income`/`spending`
purpose roots; `member`; `children N`; `owner X 60%` and `share N for X` as two syntaxes; free property lines.

---

## 6. Lanes, revised

K0a and K0b are running: groundwork, no behaviour change. Then:

| wave | lane | builds | exit |
|---|---|---|---|
| 1 | **K1 kinds and slots** | `Kinds` with slots, ranges, multiplicity, weights, defaults, parts; `std.ax` declares the built-in properties as slots; `Taxonomy<T>` for kinds and purposes; forced placement | every v4 property reads through a slot; `us/*.ax` slots typed; no behaviour change in goldens |
| 1 | **K2 facts** | `core::tagless`; `Facts` with `Key<V>`; `DaySet`; params, prices, residence, terms and budgets as facts; delete sampling | fixes `temporal_days_count…`; absorbs `props.rs` |
| 2 | **K3 positions and addresses** | `Addresses` with posting lists; positions as things; claims as parcels of any commodity; settlement = relief; parcels in columns; delete the survey and `AssetState` | ClaimChange and settlement work; prorata basis decided |
| 2 | **K4 events** | `Events` in columns; `Q` tagless; `solve` shared; static split check; `Staged` → trail | the family and landlord occurrence errors |
| 3 | **K5 promises** | terms, residuals, deadline heap, `Schedule` with pay dates; the trail; forecast = fold past today | both forecast failures; `monitor_complete` gone |
| 3 | **K6 norms** | relators' legs and laws with projection; `joins`, membership, `on start`/`on end`; `also`/`share`/`sales-tax`/`budget`/`match` desugared; `sells` | purpose counts in family and landlord |
| 4 | **K7 facts out and the session** | steppers, sparse table, pivots, provenance `why`, `Session` | goldens regraded once, every change justified |
| 4 | **L language** | the junction, positions under agents, paths, debts as promises, derived income/spending, `fmt --upgrade` | every example upgraded mechanically |

- **Two at a time.** Lanes in one wave run in parallel only where their crates do not overlap. This machine has four
  cores, so two lanes build at once.
- **Each lane gets a brief** that cites this file, PROPOSAL §3–5 for the faults it removes, and `lanes/common.md`.

---

## 7. Budget

The design adds the associations, relators, claims in kind, the trail and the session. It removes what each replaces.
Against PROPOSAL §7 (about 27,000 lines from 49,428):
- **Additions:** slots and placement +400, `Addresses` +250, the trail +200, `tagless` +150, `DaySet` +150,
  projection and membership +300, `Session` +250. About +1,700.
- **Further removals:** the per-field property parsers (`props.rs` alone is 1,931), `Match`, `lower/also.rs`, the
  `Entity`/`Place` relation fields and their resolvers, `projection.rs`, the forecast's own driver, and three of the
  five folds. About −2,500 beyond what §7 already counted.

The ceiling stays about **27,000**. The examples shrink by much more: R5 counts 25 written relations in `05-family`
becoming 3, and every paycheck becoming one line.

---

## 8. Open, and where each would fit

- **Two bases** (book accrual, tax cash with other depreciation): a `basis` slot read per system. A tally is counted
  under each system that reads it, so the second basis is a second tally of the same flows. It needs no new kernel and
  is not designed.
- **Documents out** (W-2, 941, K-1, 2555, FBAR): views with a layout, through the `Session` query surface. This is
  content.
- **Order-level sales tax:** a sync document's lines carry the ship-to system. It needs a sync format and nothing in the
  core.
- **Defaults are guesses about the past:** they stay explicit (`unfilled-default`). `axiom fix` can write the guess down.
- **A bare address is stable only until a sibling opens:** resolution by the line's day contains it. `axiom fix`
  rewrites references to their shortest stable form.
