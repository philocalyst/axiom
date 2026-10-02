# Lane C: the core primitives the kernels are made of

Read [`common.md`](common.md) first, then [`../DESIGN.md`](../DESIGN.md) §3 and §4, which are the specification. Your
worktree is `/home/user/axiom/.claude/worktrees/lane-c`, on branch `claude/great-wozniak-pnqn7x-v5-c`.

**Your crate:** `core`, and only new modules in it, plus their `mod`/`pub use` lines in `lib.rs`. Lanes K0a and K0b are
editing other crates and a little of `core` (`calendar.rs`, `id.rs`), so do not touch existing `core` files except
`lib.rs`. Nothing outside `core` uses your modules yet: the kernel lanes will. So your tests are the only users, and
they must be thorough.

This is the most demanding lane for craft. These six modules sit under every kernel, run in every hot loop, and will be
read by everyone who touches the engine. Each one must be small, obviously correct, documented by its `//!` with the
invariant it keeps and why its layout is the one it is, and proven by:
- unit tests;
- a property test against a naive model (write the generator yourself: no dependencies; a small xorshift is enough);
- for the hot paths, a benchmark in the test suite behind `#[ignore]`, with its numbers in your report.

## 1. `tagless`: a column of mixed values (DESIGN §3.1)

- `Tag` (`#[repr(u8)]`), `Payload` (a 16-byte `#[repr(C)] union`, every field `Copy`), `Column { tags, payloads }`.
- `trait Field: Copy` with `const TAG: Tag` and the conversions. Implement it for the scalar types core already has:
  - `bool`;
  - `Ratio`;
  - `Qty` paired with a commodity id (decide how core names that pair without depending on the model);
  - `Day`, `Days`, `Span`, `Sym`;
  - raw `u32` ids;
  - a `(u32, u32)` run.
- `Column::push<V: Field>`, `get<V: Field>` (the one unchecked read: `debug_assert_eq!` on the tag, and a `// SAFETY:`
  argument), `try_get<V>` (checked, returns `Option`), and an iterator of `(Tag, Payload)`.
- `const` asserts: `size_of::<Payload>() == 16` and `size_of::<Tag>() == 1`.
- Every other `unsafe` you need stays in this module, with its own `SAFETY` comment. State in the module doc why the
  union is sound: every field is `Copy`, and the tag discipline is enforced by the only writer.
- Tests: a round trip for every `Tag`; a property test of random typed pushes read back with `try_get`; and in debug, a
  wrong-type `get` must panic (`#[should_panic]`).

## 2. `dayset`: finite unions of day intervals (DESIGN §3.2)

- **The representation:** sorted, disjoint, non-adjacent `Days` intervals. Normalise on construction, so equal sets are
  equal values.
- **Two forms:**
  - an owned `DaySet` for building;
  - a borrowed `DaySlice<'a>` for reading runs stored in a shared arena.

  The kernels will store many sets in one arena, so design for that: `Run<Days>` into a `Vec<Days>`.
- **Operations:**
  - `union`, `intersection` and `difference`, as linear merges;
  - `len()` in days;
  - `contains(day)` by binary search;
  - `within(window)`;
  - `earliest_reaching(n, window_len) -> Option<Day>`: the first day on which the trailing `window_len` days contain
    at least `n` days of the set. That is the 330-in-365 and 183-day tests, by a two-pointer sweep.
- **Tests:** each operation against a naive bitmap model over random sets inside a few years. The model is a
  `Vec<bool>` per day.

## 3. `sparse`: range minimum and maximum in O(1) (DESIGN §3.9)

- A sparse table over a slice of `Copy + Ord` values: build in O(n log n) into one flat `Vec`, not `Vec<Vec<_>>`, then
  answer `max(range)` and `min(range)` with two overlapping power-of-two blocks. The log is `ilog2`, with no loop.
- Generic over the order (max or min) with a type parameter, not a `bool`.
- Also: `peak_within(days: &[Day], values, window: Days)`, which maps a day window to an index range by
  `partition_point` and then queries. A balance history is `(day, value)` steps, so the value on a day is the last step
  at or before it. Get that boundary right, and test it.
- Tests against a linear scan over random arrays and random windows.

## 4. `postings`: sorted-set intersection, galloping (DESIGN §3.3)

- `intersect(a: &[u32], b: &[u32], out: &mut Vec<u32>)`: a merge that gallops (exponential search, then binary
  search) when one list is much longer.
- `intersect_all(lists: &mut [&[u32]], out)`: shortest first, pairwise, into a reused buffer.
- Then the **one place you may use `fearless_simd`**: a block-compare kernel for the case where both lists are of
  similar length. Compare a block of 4 or 8 of `a` against a broadcast of `b[j]`, or the shuffle-based kernel in
  Lemire, Boytsov and Kurz, "SIMD compression and the intersection of sorted integers", SPE 2016.
  - Dispatch once, with a scalar fallback.
  - Keep it only if your benchmark shows at least 1.3× on lists of 10⁴–10⁶ with similar lengths. Otherwise delete it,
    and report the numbers.
  - If you keep it, add `fearless_simd = "1"` to `[workspace.dependencies]` and to core's `Cargo.toml`.
- Tests against `BTreeSet` intersection on random lists, including empty lists, disjoint lists and identical lists.

## 5. `placement`: forced placement of words into slots (DESIGN §3.4)

- **Input:**
  - `cand: &[u16]`: for each word (at most 8), the bit set of slots it may fill (at most 16);
  - `single: u16`: the slots that take at most one word.
- **Output:** for each word, either the one slot it lands in under *every* valid placement (forced), or the set of
  slots it could land in (ambiguous). Plus a typed failure:
  - no placement exists, which is too many words for the slots;
  - a word with no candidates.
- **Algorithm:** unit propagation, then a depth-first search over words that ORs where each word landed into a
  per-word mask. A word is forced iff its mask has one bit. Keep it allocation-free: `[u16; 8]` on the stack.
- **A typed result:** `enum Placed { Forced(u8), Ambiguous(u16) }` per word, and `enum Unplaceable { NoCandidate(u8),
  NoPlacement }`.
- **Tests:**
  - the four worked rows of ASSOCIATIONS §4.3, encoded as bitmasks;
  - a property test against brute-force enumeration over random small instances.

## 6. `trail`: dense state with an undo log (DESIGN §3.8)

- `Trailed<T: Copy, L: Log<T>>`:
  - `cells: Vec<T>`;
  - `set(i, v)` and `get(i)`;
  - `push(v) -> u32`, which appends; undo truncates appends.
- `trait Log<T>`: `record(at, old)`, `mark() -> Mark`, `undo_to(mark, &mut cells)`.
  - Two impls: `Undo<T>`, the log, and `()`, which records nothing. With `()`, `mark` and `undo` are unavailable.
  - Make that a **type error**, not a runtime panic: put `mark`/`undo` on a second trait that only `Undo<T>`
    implements, or use another arrangement you can justify.
- `Mark` is opaque, and is only valid on the trail that made it. Can a mark from one trail be applied to another?
  Prevent it with a branded lifetime (the `GhostCell` trick: an invariant lifetime brand) if it stays readable. If not,
  use a cheap runtime check, and say which you chose and why.
- `Fork<'t, T>`: an RAII guard that holds `&mut Trailed`, derefs to it, and undoes to its mark on drop.
  `Fork::keep(self)` commits, by forgetting the guard as `Staged` does.
- Several trailed arrays usually move together: parcels, tallies, residuals. Provide a way to mark and undo a group of
  them as one, without `dyn` in the hot path: a macro, or a tuple impl of a `Trail` trait up to 6. Pick the one that
  reads better.
- **Tests:** random sequences of set, push, mark, undo and fork against a model that clones the whole vector at each
  mark.
- **Benchmark:** the cost of `set` with `()` must equal a plain `Vec` write. Show it, or show the assembly is identical
  (`cargo asm` is not available, so a benchmark is enough).

## Not in this lane

- No changes to existing `core` files except `lib.rs`.
- Nothing in other crates.
- No new dependencies except `fearless_simd`, and only if §4's benchmark earns it.

## Report

The usual report from `common.md`, plus:
- **Benchmark numbers** for `postings` (scalar and, if built, SIMD), `sparse` and `trail`.
- **For each module:** its line count, and the one design decision you are least sure of.
