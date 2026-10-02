# Lane K12: kinds, typed slots and one store of facts

Read [`common.md`](common.md) first. Then read [`../DESIGN.md`](../DESIGN.md) §1 (the verdicts), §2.1–2.2, §3.1–3.2
and §5. Then read [`../research/ASSOCIATIONS.md`](../research/ASSOCIATIONS.md) §1–3. Your worktree is
`/home/user/axiom/.claude/worktrees/lane-k12`, on branch `claude/great-wozniak-pnqn7x-v5-k12`.

**Your crates:** `model`, `syntax` (the `has` line grammar only), `systems` (the `.ax` declarations), and the readers in
`engine`, `report` and `sync` that you must move to the new store. Lane C's primitives (`core::tagless`, `core::dayset`, `core::placement`) are in your base, and lane C2's `core::facts`
(the store itself, with `Key<V>`, `Builder`, `Steps`, `days_where`) is merged into it or will be before you reach step 5,
with notice from the orchestrator. Use them, and do not re-implement them.

## Why

The user's complaint: *"the jordan-401k is proof that the typing is still a little weak; it should be easy and
declarative to set up associations."* R5 found the cause in three lines:
- `has employer entity` (`us/401k.ax`);
- `has beneficiary entity` (`us/529.ax`);
- `has coverage name` (`us/hsa.ax`).

`entity` accepts any entity, and `name` accepts any word. Behind them, the model stores what is said about a thing in a
dozen places:
- the typed fields of `Kind`, `Place`, `Entity`, `Commodity` and `Asset`;
- `Prop` rows with a `since`, read by a linear scan (`book.rs::prop`) and written by rebuilding a boxed slice
  (`props.rs::put`);
- the `Residence` list;
- the terms and budget timelines;
- the engine's sampled `History` (`engine/temporal.rs`), which exists only because the model's values are not
  integrable.

This lane makes kinds declare **typed, counted slots**, and puts **everything said about a thing in one store of
timelines** read through typed keys. Associations (addresses, relators, `joins`, projection) come in K3 and K6 and
build on exactly this.

## The rule of this lane

Behaviour is preserved with exactly two exceptions, both listed in your report:
1. **The residence test.** `temporal_days_count_an_inclusive_residence_before_the_first_flow` must pass: integrating
   facts instead of sampling them fixes it. That leaves 3 known failures.
2. **Typing the shipped slots catches real errors.** Where a stricter slot type (step 4) catches a genuine error in an
   example, fix the example. List each one with the diagnostic it now gets, before and after.

No other test expectation, golden or mistake output changes.

## Steps

Each step is one or more commits, each building and passing.

### 1. `Taxonomy<T>`: one tree builder

`kinds.rs::declare_sites`, `purposes.rs::declare_sites` and `paths.rs` each build a scoped name tree from `NAME :
PARENT` lines, with their own duplicate detection, parent resolution with suggestions, and cycle cutting (`cycles` is
written twice). Write the one builder from PROPOSAL §5 K1:

```rust
pub struct Taxonomy<T> { pub tree: Tree<T>, pub index: Scoped<T> }
pub trait Node: Sized { const NOUN: Noun; fn root(name: Sym) -> Self; fn declared(at: Declared<'_, '_>, names: &mut Interner<'_>) -> Self; }
```

Kinds and purposes both use it.
- Diagnostics go through K0a's `problem` catalog.
- Report the line count of the old builders and of the new one.

### 2. Slots: `has NAME RANGE [MULT] [by WEIGHT]`

Extend the `has` line in `syntax` and the model's `Has` into a `Slot`:

```rust
pub struct Slot { pub name: Sym, pub range: Range, pub mult: Mult, pub weight: Option<Weight>, pub loc: Loc }
pub enum Range { Kinds(Run<Id<Kind>>), Words(Run<Sym>), Value(Ty) }       // `kind | kind`, `one of a | b`, a value type
pub enum Mult { One, Optional, Some, Many }
```

- **Storage.** Slots live in one arena, and a kind holds `Run<Slot>`: no per-kind `Vec`.
- **Inheritance.** A kind's effective slots are its ancestors' plus its own. A subkind may repeat a slot only to
  **narrow** it: a sub-range, a tighter multiplicity. Anything else is `slot-widening`.
- **`Ty::Entity` and `Ty::Name` are deleted.**
  - A kind range replaces `entity`.
  - `one of` replaces `name`.
  - A book that writes `has x entity` or `has x name` gets an error with a fix: `has x agent`, or `has x one of …`
    listing the words its things use. Check the corpus, and say whether any book in `examples/` or `systems/` needed
    the fix.
- **Defaults** (`= …`) and the `as with`/`as for` markers are not this lane's: parse neither yet. K3 and K6 add them
  with addresses and relators.

### 3. Filling slots, checked once

A property line `NAME VALUE[, VALUE …]` on a thing fills the slot of that name on the thing's kind. It is checked at
that moment, and never again downstream:
- **The range:** `wrong-kind`, naming what was found and what the slot takes.
- **The count:** `too-many`, or `missing-role` for a required slot left empty at the end of declarations.
- **Weights,** for `by` slots: `owners dana 60%, theo 40%`.

The diagnostics are new entries in the catalog. Make them gorgeous, with an example in the help.
- **`wrong-kind`** names the slot's range in words ("takes a person or a household"), and offers the closest declared
  thing of a fitting kind as a fix.
- **`missing-role`** points at the declaration's name, and offers the role line as an edit.

### 4. Type the shipped slots

In `systems/src/us/*.ax` and `std.ax`:
- `has employer entity` becomes a slot whose range is the `employer` kind;
- `has beneficiary entity` becomes `person`;
- `has coverage name` becomes `one of self-only | family`;
- `has filing name` becomes the closed set of filing statuses the `us` laws compare against. Read them to find the set.

Keep the slot names that the laws read, so no law changes in this lane. K6 renames `employer` to `sponsor` when the
relator lands. Run every example, fix what is genuinely wrong, and list it.

### 5. `Facts`: one store, typed keys

`core::facts` is the store (lane C2): a `Builder` that paints statements in source order and freezes by one counting
sort, a frozen `Facts` read by `at`, `steps` and `days_where`, and `inherited` for the fall back up a chain. Read its
module doc first. You do not build the store. You:
- **Number the holders.** The model still keeps separate arenas for places, entities, commodities, assets, kinds and
  contracts. Number them by concatenating them: a `HolderIndex` from a per-sort offset, so that `Id<Place>` and
  `Id<Entity>` map to a dense `u32` and back. K3 merges the arenas; you do not.
- **Resolve the engine's keys by name, once.** The engine reads a built-in slot (`opened`, `lives`, `restricted`, …)
  through a typed `Key<V>`. Do not hard-code slot ids as constants: a number in the engine that has to agree with the
  order of declarations in `std.ax` is a bug waiting for an edit. Declare the built-in slots in `std.ax`, and when the
  book is built look each up by name into one `Slots` struct of typed keys (`pub opened: Key<Day>`, …), checking that the
  declared range is the type the key claims. A mismatch is an internal error naming the slot, reported at build time and
  tested. The engine borrows `&Slots` beside the facts.
- **Build the store** from every declaration and statement, in source order, and freeze it into the book.

Then move, one family per commit, every value that changes on a day or is said about a thing into `Facts`:
- **`Prop` rows** (declared `has` properties), `book.rs::prop`, `props.rs::put`, `PropTable` and `Props`;
- **the built-in properties** in `props.rs::BUILTINS`:
  - `opened` and `closed`;
  - `liquidity`, `select` and `holds`;
  - `lives` and `citizen`;
  - `currency`, `books`, `member` and `precision`;
  - and the kind flags `restricted`, `deferred`, `claim`, `basis`, `purpose`, `pays`, `takes`, `sales-tax` and
    `share`.

  Each becomes a slot that `std.ax` declares on the root kind it belongs to. The engine reads it through a `Key<V>`
  constant. Its `Args` reader becomes the parser of its value type. The `Assign` enum goes. A kind's facts are the
  defaults of its things, and lookup falls back up the kind's ancestors (a pre-order range);
- **`Residence`** becomes the `lives` slot's timeline;
- **the fields of `Kind`, `Place`, `Entity`, `Commodity` and `Asset`** that only cache a property go. Their readers in
  `engine`, `report` and `sync` call `facts.at(KEY, holder, day)`. Keep a field only where it is identity (`name`,
  `kind`, `loc`) or structure (a place's `role`), and justify each one you keep.

Params, prices, terms timelines and budget timelines use the same `Steps` representation, but stay in their own tables.
A price is keyed by a commodity pair and a param by a row key, not by a holder's slot. Do not force them into `Facts`.

### 6. Integrate, do not sample

- `Facts::days_where(key, holder, within, holds) -> DaySet` integrates the steps exactly.
- **Delete what sampled property conditions:**
  - the law functions `days(COND, window)` read it, through the law evaluator;
  - `engine/temporal.rs`'s sampling of *property* conditions goes;
  - so do `plan.rs::temporal_queries`' property dates and `add_prop_dates`.
- **Keep `peak`/`low` over balances.** They still need the sampled `History` until K7's position steppers. Keep that
  path, smaller.
- **The residence test.** Yearly schedules start from the first step of any fact, which fixes
  `temporal_days_count_an_inclusive_residence_before_the_first_flow`.

### 7. Measure

`props.rs` is 1,931 lines. Report how much of it survives, and the per-crate deltas. The target is a net reduction of
**about 2,000 lines**, with `Facts`, `Taxonomy` and slots included in the count.

## Not in this lane

- Addresses and paths: K3.
- `as with`/`as for`, defaults and bracket selectors: K3 and K6.
- Relators, `joins`, parts and projection: K6.
- Merging the arenas into one `Thing` arena: K3.
- Position histories, and `peak`/`low` without sampling: K7.
