# Lane K4c: flows in columns, a quantity that is a tag and a payload, and the cleanup K4b left

Read [`common.md`](common.md) first. Then [`../DESIGN.md`](../DESIGN.md) §3.1 (`tagless`), §3.5 (`Events`: one IR, in
columns), §3.8 (the trail), [`../PROPOSAL.md`](../PROPOSAL.md) §4 (the Rust we want), `crates/core/src/tagless.rs`
and `facts.rs` (the best-documented unsafe in the tree: your standard), and the maps of the lanes before you:
`K4a-map.md`, `K4b-map.md` (sections 10 and 11: what it left), `K5b-map.md`. Your worktree is
`/home/user/axiom/.claude/worktrees/lane-k4c`, on branch `claude/great-wozniak-pnqn7x-v5-k4c`.

**Your crates:** `model` (`journal.rs`'s `Flow`, `book.rs`'s `Book.flows`, `lower/staged.rs`, `balance.rs`), `engine`
(everything that reads a flow: `ledger.rs`, `post.rs`, `occurrence.rs`, `statement.rs`, `evaluate.rs`, `motion.rs`),
`core` (`tagless.rs` gains what a flow's quantity needs). Lanes K3c (`lots.rs`, `post.rs`'s arrival, `explain.rs`) and
K6 (`laws/`, `lower/also.rs`) may run beside you: **the change is mechanical at every reader, so land it as a few
small commits that each build and pass, and merge `claude/great-wozniak-pnqn7x-v5` into your branch before each push of
the lane**.

## What is wrong

`Flow` is **192 bytes** (`size_of::<Flow>() <= 200` is asserted in `journal.rs`) and a book is a `Vec<Flow>`. The fold
reads, per flow, `day`, `from`, `to`, `out`, `arrive`, `select` and a couple of tags: about 80 bytes of the 192; the rest
(`payee`, `owner`, `purpose`, `description`, `origin`, `header_codes`, `codes`, `loc`, `waive`, `detail`) is read by a
report or a diagnostic. So a scan over a million-transaction book pulls about 2.4 times the memory it uses, and holds
it: `axiom check` on `bench/` at 1m has an RSS of 664 MB. There are 187 sites that name `book.flows` or `.flows[`.

And a flow's quantity is two `Amount`s (`out`, `arrive`) and an `Infer` (`Known | Target{end, balance} | All | Unknown`)
that says whether they are what the flow moves or a stand-in until the fold can say: a tag and a payload, held as an
enum beside the amounts it qualifies. K4b made that the solver's `Resolved { amount, infer, mode, exact }`.

K4b also left a list (its map §10 and its report; I reviewed it): two `put`s (`model/balance.rs::put` and
`engine/statement.rs::solve_group` each write what `solve` found back to the flows); two `Env`s (`statement.rs::Reads` and
`occurrence.rs::Reads`) with different `lands`; exchange legs recognised by the **shape** of the leg's flow
(`flow.is_exchange()`) in `Statement::asked`, `balance::moved` and the fold, so the solver and the model agree through
units and not through the group; `exchange_costs`'s tuple parameter and `is_exchange_cost`'s index arithmetic (a page of
checked adds to say "the flow at offset n of this transaction"); `amount_of` with five parameters.

## What to build

1. **`Flows`: the book's flows in columns.** Hot columns (`day`, `from`, `to`, the quantity, `select`, `txn`), a cold
   record for the rest, both indexed by `Id<Flow>`. `Flow` stays as the **owned value** that lowering builds and the
   tests write (a builder, `Flows::push(flow)`); readers get a `FlowRef<'_>` (a struct of borrows, `Copy`) from
   `Flows::get(id)`, with accessors by name, and the fold's hot loop reads the columns directly
   (`flows.hot(range)` giving slices). The map decides the hot/cold split **from what each reader reads** (count it),
   not from this paragraph. A write is a method (`Flows::set_amounts(id, out, arrive)`), never `&mut Flow`.
2. **A flow's quantity is a tag and a payload**: `{Known(Amount), Target(end, balance), All, Unknown(unit), Computed}` as
   one tagless column entry, read through a typed accessor that returns the enum (a `match` the optimiser can see
   through) and, in the fold's loop, branches on the one-byte tag only when it is not `Known`. Extend `core::tagless`
   (the `Tag`s and `Field` impls it needs, with the invariant, the `// SAFETY:` argument and a round-trip test for each
   new tag, as `tagless.rs` does); **unsafe only in `core`**, never in `model`/`engine`. If the map shows a safe encoding
   as fast as the unsafe one, **use the safe one** and say so with the benchmark: unsafe has to earn its place.
3. **`Staged` is one mark over `Flows`** (truncate every column to a length) instead of five hand-kept marks; if the
   trail (`core::trail`) fits it without cost to `check`, use it, else stay with marks and say why.
4. **The K4b list**: one `put` (the solver's answer written once, from one function, through `Flows::set_amounts`/the
   fold's resolved map: say which); one `Env` reading with `lands` chosen by a value, not by a second struct; the group
   *says* which legs exchange (a `Draw` field the lowering sets, from the units, once) so the solver and the fold stop
   re-deriving it from a flow; `exchange_costs` and `is_exchange_cost` through `FlowRef`; `amount_of` takes a
   `FlowRef` and a root. Delete what is dead after.

## The proof and the measure

- **Behaviour unchanged**: goldens, mistakes, the three known failures; K4b's splits oracle (`splits.py`) and the K5a
  oracle; `fuzz.py ... diff`; `cargo test --workspace --release`. Byte-identical output is the bar for every commit.
- **The performance claim is the lane's reason, so measure it first**: before any change, record `axiom check`
  wall time, user CPU, RSS and callgrind instruction counts on `bench/` at 100k and 1m (`sh bench/run.sh 100k 1m`,
  `sh bench/profile.sh 100k`), and the size of a flow. Then a microbenchmark of the fold's inner scan over
  `Vec<Flow>` against the hot columns on one million flows (the way lane C measured `tagless` and `postings`). **Target:
  RSS down by at least a quarter and `check` at 1m not slower; if the columns are not at least neutral, say so, keep
  the layout only if it is simpler, and report the numbers.** The machine is shared: run each measurement at least
  three times and report the fastest, with the load average.
- Size assertions in code: `const _: () = assert!(size_of::<FlowRef<'_>>() <= ..)`, the hot record, the quantity
  payload (16 bytes).

## Rules of this lane

- Common bar. Function parameters: if a function takes six things, find which belong together. **No parameter
  bundles to hit a count.** No bool parameters. Functions under 40 lines.
- `unsafe` only in `core::tagless` (or a new `core` module), each block with a `// SAFETY:` comment that states the
  invariant and who maintains it, and a test that would fail if it broke (including a `miri`-style round trip where
  `cargo miri` is not available: say what you did instead).
- No test deleted or weakened; a test that names `Flow`'s fields moves to the accessors.
- Do **not** change what a flow *means*: `Infer`'s four cases keep their semantics; K4b's solver is not touched except
  where the list above says.

## Step 0: the map

`docs/v5/lanes/K4c-map.md`, committed before any code: for **every field of `Flow`**, the readers (file, function, count),
split hot (read per flow in the fold) and cold (a diagnostic or a report); every `&mut Flow` or field write
(`flow.out = ..`) and what it means; where a `Flow` is **cloned** (K4b's `Reads::flow` clones one per read: cost); what
`RuntimeFlow` adds (K5c deletes its forecast use: do not design for it); what `Staged` truncates and in what order; the
size of `Flow` by field with the padding; and the measurements above. Decide the column split from the counts.

## Not in this lane

- Parcels in columns (`lots.rs`): a lane after K3c. The promise terms: K5b. The trail for forks and the forecast: K5c.
- The deleted-by-design `RuntimeFlow` paths of the forecast: K5c.
