## Audit of engine, report and cli: highest-leverage improvements

I only read the code; no files were changed. Everything below cites code I read, relative to `/home/user/axiom/.claude/worktrees/lane-v4/v2/crates/`.

### Size
| crate | files excl. `tests.rs`/`*_tests.rs` | also excluding inline `#[cfg(test)] mod tests` and `fixture.rs` |
|---|---|---|
| engine | 6,689 | ≈5,640 |
| report | 5,310 | ≈4,970 |
| cli | 3,020 | ≈2,430 |

**Ten longest functions (non-test):**
1. `engine/src/explain.rs:502` `mismatch`: 120
2. `report/src/available.rs:69` `spendable_section`: 64
3. `report/src/register.rs:49` `section`: 63
4. `report/src/lots.rs:15` `view`: 62
5. `report/src/why/entity.rs:18` `report`: 58
6. `report/src/available.rs:199` `Reach::of`: 51
7. `report/src/available.rs:254` `reach_section`: 50
8. `engine/src/timeline.rs:120` `deadlines`: 50
9. `report/src/budget.rs:20` `view`: 49
10. `engine/src/post.rs:102` `relieve`: 48 (tied with `why/system.rs:15`)

### Findings, ranked by leverage

**1. The views re-solve and re-fold the whole journal.**
- The CLI already runs the fold once (`cli/src/commands.rs:40`).
- Three views then build a fresh `Ledger::new` anyway: `report/src/available.rs:37-39`, `claims.rs:52-59` (used by `lots`) and `forecast/projection.rs:42-43`.
- Each `Ledger::new` repeats `events::read`, `infer::solve`, `fire::{repeats,caps,readers}` and `Totals::new` (`engine/src/ledger.rs:75-105`).
- `Solved` is documented as "Never changes" (`ledger.rs:39`), yet it is owned and deep-cloned on every `fork` (`ledger.rs:120`). Its maps and vectors are copied once per `available` withdrawal.
- Because `Solved` is owned, `fire.rs:183-191` has to `mem::take` the readers to get past the borrow checker.
- `Record::forked` also clones the solved `amounts` map (`state.rs:116`).
- The only thing tying `Solved` to `Options.today` is the precomputed deadline list (`timeline.rs:120-169`).

Proposed shape:
```rust
pub struct Plan<'b,'s> { book: &'b Book<'s>, events: Events, solved: Map<Id<Flow>,Amounts>, laws: Box<[LawFacts]>, readers: Readers, unsolved: Map<..> }
pub struct Ledger<'p,'b,'s> { plan: &'p Plan<'b,'s>, clock: Clock, world: World, record: Record, scratch: Scratch }
impl Plan<'_, '_> { pub fn start(&self) -> Ledger<'_, '_, '_> }
```
- Deadlines become lazy: a heap of `(next_due, rule)` inside `Clock`.
- The CLI builds one `Plan` and hands the report a checkpoint at `today`; views fork it instead of re-folding.
- Expected change: about −25 lines. `available` and `forecast` fold once instead of twice, and forks stop cloning static tables.

**2. Laws are re-parsed from their syntax tree in eight places.**
- `law.rs:44` `cap`.
- `fire.rs:39-68` `Reads::of`/`require`, run again on every new headroom window (`fire.rs:286`) and on every violation (`fire.rs:324`).
- `fire.rs:85-118` `window_read`/`readers`, and `totals.rs:357-364` `reads_total`.
- In the report: `headroom.rs:27` and `:73-77` (`is_floor`, called inside the sort comparator at `limits.rs:43-47`, which also allocates two `String`s per comparison), `why.rs:191-211`, and `explain.rs:450`.

Compute the facts once per law in `Plan`:
```rust
pub struct LawFacts { shortcut: Option<Shortcut>, window: Option<Window>, totals: Option<Widened>, steps: Box<[StepFacts]> }
pub struct StepFacts { reads: Option<Reads>, bound: Option<Bound>, warn: bool }   // Bound::{Cap, Floor}
pub enum Shortcut { Cap(Cap), FloorOfNothing }
```
- `Headroom` carries `bound`, so the report stops walking the syntax tree.
- Expected change: about −40 lines.

**3. Which subjects contain a place is recomputed on every flow.**
- `Totals::watched_sides` (`totals.rs:281-284`) and `record` (`totals.rs:298-309`) each call `inside` for the same subjects. For an entity subject, `inside` walks the entity tree's lineage (`scope.rs:21-35`).
- But `through[p]` is *exactly* the watched subjects that contain `p`. So "contains `here` but not `there`" is just a difference of two short lists:
```rust
fn crossed(&self, here: Id<Place>, there: Id<Place>) -> impl Iterator<Item = Subject> + '_ {
    self.through[here].iter().copied().filter(move |s| !self.through[there].contains(s))
}
```
- `eval::held` (`eval.rs:563-571`) filters a place subject's subtree with `inside`, which is always true inside that range.
- For an entity subject, `held` scans every slot in the book. It should use a `Groups<Entity, Place>` built once.
- Expected change: about −10 lines, and no lineage walks left on the per-flow path.

**4. `Snapshots` is a dense grid of days × places × commodities** (`history.rs:151-181`, `256-258`, `278-287`, `310-320`).
- The engine's own `lots.rs:1-7` explains that a place holds one or two commodities; this grid ignores that.
- `summary()` (`lib.rs:179-182`) allocates and scans the full grid just for the one-line `check` summary.
- Index only the `(place, unit)` pairs that occur, sorted. Places are in pre-order, so a subtree is still one contiguous range:
```rust
struct Snapshots { days: Vec<Day>, pairs: Vec<(Id<Place>, Id<Commodity>)>, cells: Vec<Held> /* days × pairs */ }
```
- Expected change: about 0 lines; memory and scan cost shrink by roughly the number of commodities.

**5. The display sign is found by parsing the place's path string.**
- `v3_root` (`model/book.rs:669-671`) does prefix matching on the path. It is called at `engine/scope.rs:61`, `eval.rs:577` (every `balance` evaluation), `explain.rs:513`, `balance.rs:196`, `register.rs:52` and `places.rs:41,54`.
- Through `places.rs:54` it runs twice per posting in `flow.rs:101`, and again in `variable.rs:61` and `budget.rs:74`.
- It is inconsistent: `why/entity.rs:29` uses `class.display_sign()`, and `journal.rs:278` documents `Class::display_sign`.
- Fix: one table resolved once, `Sides(Box<[PathRoot]>)` with `sign()`, `side()` and `is_root()`. This gives one seam to delete with v3. Expected change: about −5 lines.

**6. Flag combinations that the types allow but the logic doesn't.**
- `Violation { warn, waived, priced }` (`engine/lib.rs:302-312`) allows 8 combinations of which about 4 make sense. It is built from a `(bool, bool, bool)` tuple (`fire.rs:334,338,355`).
- Consumers work out "does it block" themselves: `available.rs:227`.
- `Effect { owe: Option<Owed>, priced: bool }` (`lib.rs:287-290`): being priced implies an obligation, but only by convention.
```rust
pub enum Verdict { Blocks, Warns, Waived(Waiver), Priced { waived: bool } }
pub enum Consequence { Count, Owe(Owed), Penalty(Owed) }
```
- Expected change: about −5 lines.

**7. Calendar windows and day-spreading exist twice, once per crate.**
- Engine: `totals.rs:27-77` (`window_of`, `share`, `by_year`).
- Report: `apportion.rs` is the same telescoping formula as `totals::share`. Also `calendar.rs`, `headroom.rs:96-121`, `explain.rs:265-274` (a third `YYYY-MM` formatter) and hand-built closes at `timeline.rs:140-157`.
- Put them in core:
```rust
pub struct Window { period: Period, first: Day }   // containing, days, next, exactly(Recognition), Display
pub struct Spread { amount: Qty, over: Recognition } // fn within(&self, Recognition) -> Qty
```
- Expected change: about −60 lines.

**8. Well-known names are looked up by string inside loops.**
- `lens.rs:106-110` calls `book.kind("currency")` on every liquidity check: per holding, per pending posting (`available.rs:107`), per flow end (`projection.rs:99,119`).
- `projection.rs:68` calls `names.get("maturity")` per holding per checkpoint.
- `eval.rs:416` calls `names.get("born")` on every `.age` evaluation, which is on the engine's hot path.
- `budget.rs:73` compares a law's name to `"budget"`; `why.rs:169` and `taxline.rs:13` compare strings for every effect.
- If a name is missing, `kind()` computes a fuzzy suggestion over all names (`names.rs:112`).
- Fix: resolve once into `Lens { currency, .. }` and `Plan.born`, and compare `Sym`s. Expected change: about 0 lines.

**9. Code that only v3 uses, and v4 types nothing uses.**
- `.basis` ends are marked "v3 only" (`journal.rs:170`). Code that exists only for them: `motion.rs:39-60`, `post.rs:107-111,283-285,322-338`, `explain.rs:726-743`, `history.rs:73-108` (`Change::Rebased`), `register.rs:90,182-185`, `places.rs:17-25`, `expected.rs:187-191`, `variable.rs:57-58`, `projection.rs:95-97`, `infer.rs:212-215`.
- Plans (`forecast/expected.rs:88-117`) drive the forecast. The v4 equivalents, `Contract::due_days` (`book.rs:501`) and `Run.promises`, go unused.
- `Run.assets`, `promises` and `adjustments` (`lib.rs:89-94`) are always empty (`ledger.rs:238-241`) and nothing reads them. There are also seven `unreachable!("{V3}")` arms.
- Fix: move the bridge into one `bridge` module per crate so deleting it is mechanical. About 130 lines go when v3 does.

**10. The report repeats its "unpriced" bookkeeping and hard-codes column counts.**
- The same count-the-unpriced-amounts pattern is written seven times (`claims.rs:160-183`, `lots.rs:47-73`, `available.rs:73-129`, `flow.rs:141-185`, `history.rs:248-251`, `balance.rs:48-53,282`), each with its own wording of the note.
- Total rows are padded with magic column counts: `Row::padded(.., 7|9|6|4)` at `claims.rs:180`, `lots.rs:70`, `available.rs:288`, `why.rs:229`, `entity.rs:65`, `system.rs:46`.
```rust
#[derive(Default)] pub struct Priced { total: Qty, missing: usize }  // add(lens, Amount) -> Option<Qty>; note(what)
impl Section<'_> { pub fn total(&mut self, lead: impl IntoIterator<Item = Cell>) /* pads to columns.len() */ }
```
- Expected change: about −30 lines.

**11. `explain::mismatch` does three jobs in 120 lines** (`explain.rs:502-621`): it gathers the flows since the last check, diagnoses the likeliest mistake, and renders the message.
- Split into `fn since(..) -> Since { real, pending }`, `Suspect::find(..)` (testable without a `Book`) and `Suspect::advise(d)`.
- Expected change: about +6 lines, in exchange for three pieces that can be tested separately.

**12. The four per-place rule tables are listed by hand six times** (`fire.rs:74-77,108`, `totals.rs:263-264`, `headroom.rs:24`, `why/place.rs:104`, `why/entity.rs:37`).
- Add `Rules::per_place() -> [&Groups<Place, Rule>; 4]` and `Rules::all()` to the model.
- Expected change: about −10 lines.

### Performance of the hot path

**A journal flow with no laws** (for example `checking → groceries`) is already allocation-free: `Scratch` is reused. What it still costs:
- **Timeline:** a minimum over five heads, plus `skip_unreal`, which does an `events.state` hash once any event exists (`timeline.rs:255`).
- **Amounts:** `amounts()` probes `record.amounts` with a hash *before* checking `flow.infer` (`ledger.rs:291`). A `Known` flow can never be in that map.
- **`count`:** walks `through` twice, calling `inside` each time.
- **Relief:** a slot-chain walk in `ask_ties`, then `record.ambiguous.contains` (`post.rs:128`), which is evaluated even when relief is not ambiguous. Then the `relieve` chain walk and its fast path.
- **`price`:** two i128 `share` calls per slice.
- **`arrive`:** one more chain walk.
- **Total:** about 2–3 hashes, 3–4 chain walks, no heap allocation.
- **Exception:** `fire` allocates its `done` vector on every call when `repeats` is set (`fire.rs:130-138`).
- **Commodities other than the base:** each conversion does 2–4 binary searches over the whole quote table (`model/prices.rs:47-70`). The same value `base_value(m, m.out)` can be computed up to three times per flow (in `count`, `proceeds`, and `realizes`/`exchange_cost`).

**With laws, the cost is re-deriving static facts.**
- Each rule costs `applies` (two `inside` calls) plus a run of the evaluator.
- The std `overdraft` law (`systems/src/std.ax:26-28`, `always`, at both ends of every bank flow) is the most frequent evaluation. Each run does:
  - the path-string parse (`eval.rs:577`);
  - a subtree walk with a redundant `inside` filter;
  - `sum_in_base`;
  - a hash upsert into `record.headroom` (`fire.rs:278`);
  - a hash `failing.remove` (`fire.rs:226`).
- Those headroom readings are then thrown away by `limits.rs:24`.

**Structural cuts, largest first:**
1. **A `FloorOfNothing` shortcut** in `LawFacts` (finding 2), next to the existing `Cap`: overdraft becomes one slot read and one comparison, with no evaluator run and no headroom recorded.
2. **Membership from precomputed lists** (finding 3), for totals, entity balances and `applies`. This removes every lineage walk from the per-flow path.
3. **Per-flow memos:** `Worth { out, arrive }` in `Scratch`, and a per-commodity price memo keyed by day (the fold's days only move forward).
4. **Skip hash probes the types can rule out:** a `Known` flow has no solved amount, and a flow without codes has no event.

Items 1–2 target what the earlier callgrind report in `bench/REPORT.md` measured as about 9% of `check` (`eval::scan` 5.2%, `fire` 2.1%, `watched_sides` 2.0%). Its profile names `Balances::at` rather than today's `Snapshots`, so it predates recent code; the percentages are indicative. I did not run any benchmarks myself.

### Critical files for implementation
- `/home/user/axiom/.claude/worktrees/lane-v4/v2/crates/engine/src/ledger.rs`
- `/home/user/axiom/.claude/worktrees/lane-v4/v2/crates/engine/src/fire.rs`
- `/home/user/axiom/.claude/worktrees/lane-v4/v2/crates/engine/src/totals.rs`
- `/home/user/axiom/.claude/worktrees/lane-v4/v2/crates/engine/src/eval.rs`
- `/home/user/axiom/.claude/worktrees/lane-v4/v2/crates/report/src/history.rs`
