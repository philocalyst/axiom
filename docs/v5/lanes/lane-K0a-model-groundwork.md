# Lane K0a: model groundwork

Read [`common.md`](common.md) first; it is part of this brief. Your worktree is
`/home/user/axiom/.claude/worktrees/lane-k0a`, on branch `claude/great-wozniak-pnqn7x-v5-k0a`.

**Your crates:** `model`, plus the small additions to `core` and `syntax` named below. Lane K0b works on `engine`,
`report`, `sync` and `cli` at the same time. Do not edit those crates beyond what a signature change you make forces.
If you do touch them, say so in your report.

**The rule of this lane:** no behaviour changes. Every test that passes now still passes, the four known failures
still fail the same way, and `sh tests/golden.sh` and `sh tests/mistakes/run.sh` leave `git diff tests/` empty.
This lane reshapes the model so that the kernel lanes (K1–K7) can land on it. It is not a rewrite: change how the code
is organised, not what it decides. **Target: about −2,000 lines in `model`** (from 17,034), with the
function-length histogram moving toward main's (main has 2% of function lines in functions over 80 lines; the model
has far more).

Work in this order, one or more commits per step. Each commit builds and passes.

## 1. `Staged`: one RAII guard instead of 25 rollbacks

`lower/record.rs` ends in `fn rollback(world, flows, codes, selectors, details, programs)`, a hand-written
transaction over five arenas. It is called 25 times, and each caller first saves five `len()`s.

Build the guard PROPOSAL §5 K4 sketches:

```rust
/// Writes to the book's journal arenas that are undone unless committed. Lowering one statement opens a
/// `Staged`, pushes freely, and calls `commit` once it is sure; every early return rolls back by itself.
pub(crate) struct Staged<'w, 's> { world: &'w mut World<'s>, marks: Marks, state: Outcome }
```

- `Marks` records the lengths of the arenas a statement can grow. Derive the list from what `rollback` truncates
  today. Check whether `txns` and `input_values` belong in it as well (a statement that fails after pushing them
  leaves them behind today: if so, keep that behaviour and say so in your report).
- `Staged` derefs to `World` (or exposes `world()`), so the lowering code reads the same.
- `commit(self)` consumes the guard. `Drop` truncates if not committed. No `bool` field: use a two-variant enum, or
  `ManuallyDrop`/`mem::forget` if that is cleaner (justify it).
- Delete `rollback` and every saved-length tuple.

## 2. `Problem`: a diagnostic catalog for the model

The model builds about 310 `Diagnostic`s by hand. The same families recur: `duplicate-*` (about 30 sites),
`unknown-*` (about 13) and `ambiguous-*` (about 12), plus unit mismatches, wrong kinds and property type errors.
`CodeIndex::resolve` and `resolve_claim` differ only in wording.

- Add `model/src/problem.rs`: a small enum of the recurring shapes, with each variant's fields borrowed from the
  book. For example, `Duplicate { noun, name: Word, first: Loc }`, `Unknown { noun, word: Word, nearest:
  Option<&str> }`, `Ambiguous { noun, word, candidates }`, `WrongKind { … }` and `UnitMismatch { … }`, plus one
  `fn diagnostic(self) -> Diagnostic`.
- The code string, message, labels and fix stay **byte-identical** to today's, because the mistakes corpus and the
  goldens check them. Where two sites word the same problem differently, keep both wordings for now (a variant field
  can carry the noun), and list each pair in your report so the orchestrator can choose one later.
- Move every site that fits a variant. Leave one-off diagnostics as they are: the catalog is for families, not for
  wrapping every call.
- Report how many sites moved and the line delta.

## 3. `file.word(x)`

`Word { text: x.0, loc: file.loc(x.0) }` is written 105 times in the model. `Word` is the model's
(`model/src/errors.rs`), and `File::loc` is syntax's. Add one constructor in the model, either `Word::of(file, x)`
or a small extension trait that gives `file.word(x)`, whichever reads better at the call sites, and use it
everywhere.

## 4. One collect pass

The model walks every item of every file about 30 times, with `for item in &file.items { let ItemKind::X(id) =
item.kind else { continue } … }` (in `declare.rs`, `sync_lower.rs`, `props.rs`, `lower.rs`, `params.rs`, `kinds.rs`,
`purposes.rs` and `lower/contracts.rs`). Main had `collect.rs`: one pass that sorted items into typed buckets.
The orchestrator has a copy of main's at
`/tmp/claude-0/-home-user-axiom/d0ff8c60-72e7-58b1-a59f-e3f4cba55dfe/scratchpad/main-v2/v2/crates/model/src/collect.rs`.
Read it for the idea, but the AST has changed since: write it fresh for today's AST.

- One pass per file produces a `Collected<'a, 's>`: per item kind, a `Vec<Written<'a, 's, T>>` (the item, its
  site and its node, all borrowed), in source order.
- Files are independent, so collect them through `core::par` and concatenate in file order.
- Every later walk iterates its bucket. The 30 `let … else { continue }` loops go.
- Keep the order in which things are declared and diagnosed exactly as today. Goldens and mistake outputs list
  diagnostics in order.

## 5. Dead code and validation the parser already does

Each of these is dead or unreachable today. **Prove it before you delete it:**
- for an unreachable diagnostic, a test (or an existing mistake case) where the parser rejects the input first;
- for an unused parameter or value, the compiler.

The list:
- `let _ = loc;` in `lower/record.rs` (`resolve_quantity`), and the computation of `loc` above it if nothing else
  uses it;
- the `if` with identical branches near the end of `lower/record.rs` (the Amount/Pending/Target arms);
- the unused `_survey` and `_party` parameters in `lower/contracts.rs`, and `_diags` in `push_empty_txn`;
- the diagnostics `duplicate-waiver-description`, `duplicate-claim-writeoff-description`,
  `duplicate-end-description`, `assertion-tail`, `measure-tail` and `until-position`. `clauses()` rejects repeated
  clauses and `takes(verb, clause)` rejects clauses a verb does not take, so these should never fire;
- the eight `let x_pairs = x; let x = Groups::build(…); drop(x_pairs);` blocks in `rules.rs`, which become a small
  helper or a loop over a table;
- `errors.rs::iso`, which duplicates `Day`'s `Display`.

Anything else you find that is provably dead goes too. List it.

## 6. One implementation per helper

- **Literal to amount:** `literal_amount`, the `resolve_literal` closure (which swallows diagnostics with `.ok()`:
  keep that behaviour, but make it visible in the signature), `resolve_amount` ×2 and two inline copies in
  `laws/mod.rs`. Make one function.
- **`lower_tail`:** there is a copy in `lower/contracts.rs` that returns a 6-tuple and silently drops seven clause
  kinds. Make one, returning a named struct. Keep the drop behaviour for contract tails (it is a behaviour), but make
  it explicit: the contract caller ignores the fields it does not use.
- **`resolve_object`:** two copies. Make one.
- **Calendar to core:** `book.rs` has `anniversary_on`, `add_months`, `add_span`, `calendar_window`,
  `previous_window`, `quarter_window`, `covered_span` and `move_days`. These duplicate `Day::add`,
  `Window::containing`, `Window::after` and `Days::moved` in `core`, with a `ForecastError::Overflow` threaded
  through. Move what core lacks into `core::calendar`, delete the rest, and call core.
  - Overflow can only happen near `Day::MIN`/`Day::MAX`. If the only source is the `Day::MIN` anchor that contracts
    use, keep the error for now (K5 removes the anchor) and say so.
- **`index_at`** in `book.rs` duplicates `Param::row`. Make one.

## 7. Then iterate

With the pieces above in place, take the five longest model functions (`python3 docs/v5/measure/fnlen.py crates`):
`lower_occurrence`, `lower_txn`, `lower_alsos`, `lower_owes` and `purposes::declare_sites`. Split each into named
steps. Lane K4 rewrites `lower_occurrence`, `lower_owes` and `lower_loan_origin` into one event IR, so do not invest
in them beyond what `Staged` and the catalog give for free. `lower_txn`, `lower_alsos` and `declare_sites` survive
longer: make each a short driver over well-named steps, with a borrowed context struct where parameter lists exceed
five (`lower_items` takes 17 and `make_resolved_flow` 14).

## Not in this lane

- No new language features and no new kernel types beyond `Staged`, `Problem` and `Collected`.
- Do not touch the survey (`lower::survey`, `visit_endpoints`). K3 deletes it.
- Do not touch `Taxonomy`. K1 builds it.
