# Lane C2: `core::facts`, one store of timelines with typed keys

Read [`common.md`](common.md) first, then [`../DESIGN.md`](../DESIGN.md) §1, §2.2, §3.1 and §3.2, which specify this
store. Then read what you build on, in `crates/core/src`: `tagless.rs` (the column of mixed values and its `Field`
trait), `dayset.rs`, and `timeline.rs`, which already paints values over days and is the semantics you extend. Your
worktree is `/home/user/axiom/.claude/worktrees/lane-c2`, on branch `claude/great-wozniak-pnqn7x-v5-c2`.

**Your crate:** `core`. New module `facts.rs`, its `mod`/`pub use` lines in `lib.rs`, and the two small changes to
existing core files listed below. Lanes K0a and K0b are editing other crates and a little of `core` (`calendar.rs`,
`id.rs`); do not touch those. Nothing outside `core` uses your module yet: lane K12 will, to put everything said about a
thing in it. So your tests are the only users, and they must be thorough. This is the same kind of lane as lane C, and
its modules are in your base: read `tagless.rs` and `trail.rs` as the standard of craft to meet.

## Why

The model keeps what is said about a thing in a dozen places (PROPOSAL §3, DESIGN §2.2): typed fields of five structs,
`Prop` rows read by a linear scan, a residence list, terms and budget timelines, and the engine's sampled `History`
that exists only because none of them can be integrated. K12 replaces them with one store. This lane builds the store
as a data structure, with no knowledge of the model: **holders are dense `u32`s, slots are dense `u32`s, and values are
whatever `Field` can hold.** K12 numbers the holders and slots and moves the readers.

## What to build

### 1. `Key<V>` and `SlotId`

- `SlotId(u32)`, and `Key<V: Field>`: a slot id and the type its values have, `PhantomData<fn() -> V>`. `Key::new` is a
  `const fn`, so that a key can be a constant or be made at run time from a slot's name once the model has declared it.
  Derive `Clone`, `Copy`, `PartialEq`, `Eq`, `Hash`, `Debug` by hand where a derive would demand `V: Trait`.
- A key's type is a claim the store cannot check by itself, since a slot's range lives in the model. `Facts::at` reads a
  payload as `V` and `debug_assert`s the tag as `Column::get` does. State in the module doc that a wrong claim gives a
  wrong value and never undefined behaviour, for the reason `tagless` gives, and that K12 checks every key against the
  slot's declared range when it builds the book.

### 2. `Builder`: paint statements in source order, then freeze

```rust
let mut facts = Facts::builder(holders);                     // holders: how many rows
facts.paint(holder, key, Days::new(from, to)?, value);       // `from … until …`, or `Days::new(from, Day::MAX)`
facts.paint_always(holder, key, value);                      // the declaration: holds from the beginning of time
let facts = facts.freeze();
```

- **Painting is `Timeline::paint`'s rule:** statements are applied in the order they are written, a later one
  overrides an earlier one over the days it covers, and what held before resumes after it. Adjacent steps that hold the
  same value merge.
- **Do not duplicate the painting.** Extract `Timeline`'s step surgery into one function over a `Vec<(Day, T)>` (or a
  slice-and-run form, whichever lets both use it without allocating per call), and have `Timeline<T>` and your builder
  call it. This is the one existing core file you may change besides `lib.rs`: its tests must pass unchanged. Compare
  values for merging by tag and payload bits; add `Payload::bits(self) -> u128` (private to the crate if it can be) or
  an equivalent to `tagless.rs`, additively, with a test. Say in your report how you handled the equality of two
  payloads, because `Ratio` is in lowest terms and `Qty` an `i64`, so equal values have equal bits, and say what would
  break that.
- **`freeze` is one counting sort.** Statements arrive in any holder order. Group them by `(holder, slot)` with one
  counting pass and one scatter, paint each group in arrival order, and write the result in CSR form. No
  `Vec<Vec<_>>` anywhere, built or kept.
- **A slot with no step for a holder has no entry.** Rows are small (a thing has a handful of slots set), so a row's
  slots are sorted and found by a short scan or a binary search, whichever your benchmark prefers; say which.

### 3. `Facts`: frozen, borrowed, read

```rust
pub struct Facts {
    rows: Vec<u32>,          // holder -> first entry; one more at the end
    slots: Vec<SlotId>,      // entry -> slot, sorted within a row
    steps: Vec<Run<Step>>,   // entry -> its steps (or two u32s: the layout is yours, and it is a commitment)
    days: Vec<Day>,          // step -> the day it begins
    values: Column,          // step -> the value
}
```

- `at<V: Field>(&self, key: Key<V>, holder: u32, day: Day) -> Option<V>`: what holds on `day`, or `None` if nothing is
  said yet. Two searches, no allocation.
- `steps(key, holder) -> Steps<'_, V>`: a borrowed stepper over what was painted, in order: `Iterator<Item = (Days, V)>`
  with a `len`, `ExactSizeIterator` and `DoubleEndedIterator`. It is the one representation that params, prices, terms
  and budgets will also use, so keep it independent of `Facts`: it borrows a `&[Day]` and a `&Column` slice and nothing
  else, and says in its doc that this is why.
- `days_where<V: Field>(&self, key, holder, within: Days, holds: impl FnMut(V) -> bool) -> DaySet`: the days of
  `within` on which the slot's value satisfies `holds`. An **integral**, not a sample: it walks the steps that meet the
  window once, and the days it returns are exact. This is what makes "the days I lived in a system" a plain read, and it
  deletes the engine's sampling of property conditions. Test that adjacent satisfying steps merge into one interval.
- **A slot that holds many values at once** (`lives` a person lives in several systems at once; an `owners` list) is a
  step whose value is a run of `V`s. Design this so that a step's value can be a set, read as a borrowed slice of a
  second column, without a second store. Offer `at_many`, or make `Many<V>` a `Field` over `Tag::Run`, whichever reads
  better; say which and why. A set is stored sorted, so equal sets are equal values and merge.
- **Inheritance is not here.** A kind's facts are the defaults of its things, and a lookup falls back up the kind's
  ancestors, a contiguous range of a pre-order tree. The tree is the model's. Offer `at_first` or a free function
  `inherited(facts, key, chain: impl IntoIterator<Item = u32>, day)` that tries each holder of a chain in turn and
  returns the first answer, so that K12 passes the ancestors and this module needs no tree. Test it with a chain.
- **Layout is a commitment.** Assert in code the sizes of every type you add (`const _: () = assert!(…)`). The step
  columns are the two flat ones above (a day column and a tagless column): a read touches 4 bytes of day and 17 of
  value.

### 4. What `Facts` must make easy for K12

Think of the callers, and write the doc for them. K12 will:
- number every holder (places, entities, commodities, kinds, assets, contracts) densely by concatenating the arenas, and
  hand you the total;
- fill millions of rows: a book of 10⁶ things that each say a few slots must freeze in well under a second and read in
  nanoseconds;
- read one slot of one holder on one day in the fold's hot loop, and read a whole slot's timeline in the reports;
- later add steps to a frozen store (a live edit). **Do not build that.** But keep `freeze` a function of the builder's
  contents alone, so that rebuilding is cheap, and say in the doc where a patch would go.

## Tests

- **A property test against a naive model**, as in lane C: random statements (holders, slots, day windows, values of
  several types, including `Many`) painted in a random order into the builder, and into a model that is a `Vec<Option<
  Payload bits>>` per `(holder, slot)` per day over a few years. After `freeze`, `at` agrees on every day of every
  `(holder, slot)`, `steps` agrees with the model's runs, and `days_where` agrees with the model's day list under random
  predicates. At least 2,000 random cases, and the generator is yours (`crate::testing::Rng` is in the base).
- The cases that bite: a paint that covers an earlier paint wholly; one inside it, which must resume the earlier value
  after; two that touch; an unbounded paint (`Day::MAX`); a paint before the first step; the same value painted twice
  (which merges); an empty row; a holder with every slot; a slot set for no holder.
- A `compile_fail` doctest that a `Key<Day>` cannot read a `Key<Ratio>`'s slot **if** you make that a type error; it
  cannot be, since a key is a claim. Do not write that test; write the one that the claim is debug-checked.
- Doc tests for the three calls a user makes: build and paint, `at`, `days_where`.

## Benchmark (behind `#[ignore]`, numbers in your report)

- **Build:** 10⁶ holders with about three slots each and about two steps per slot, in a random statement order. Time
  `freeze`, and compare with the obvious alternative, a `Vec<Vec<(Day, V)>>` per holder, built and then read.
- **`at`:** the same store, random `(holder, day)`, ns per read, against the same alternative. Report cache behaviour
  honestly: a cold random read is memory-bound, so also report a warm sweep. The point is that the CSR layout does not
  lose, and that its footprint is a small multiple of the data. Report bytes per step.
- **`days_where`:** a slot with 10⁴ steps, over a window of its whole length, against a daily sample.

## Not in this lane

- No changes to existing `core` files except `lib.rs`, `timeline.rs` (the shared painting) and an additive change to
  `tagless.rs` for payload equality.
- Nothing in other crates.
- No `fearless_simd`, no new dependency.
- No `Trailed`/undo integration: the store is frozen. A fold's mutable state is `trail`'s business.

## Report

The usual report from `common.md`, plus:
- the sizes of every type you add, and the bytes per step;
- the benchmark numbers above;
- for the module: its line count (non-test and whole file), and the one design decision you are least sure of;
- how you handled the equality of payloads, and the decision you made on `Many`.
