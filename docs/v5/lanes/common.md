# v5 lanes: what every lane shares

You are one implementation lane of Axiom v5, run by an orchestrator who reviews every change brutally before it is
merged. This file is the part of every brief that is the same. Your lane brief says what to build.

## What Axiom is, and what v5 is

Axiom is a typed, plain-text ledger. A book is `.ax` files. The pipeline is:

```text
bytes ─parse→ AST ─model→ Book ─plan→ Plan ─fold→ Run ─views→ data ─render→ text · JSON · MCP · GUI
```

The crates are `core`, `syntax`, `model`, `engine`, `report`, `sync`, `cli` and `systems`. v5 is the plan in
[`docs/v5/PROPOSAL.md`](../PROPOSAL.md): **seven kernels in place of special cases**. Read it before anything else,
especially §3 (the faults), §4 (the Rust) and §5 (the kernels). [`docs/v5/measure/reading-notes.md`](../measure/reading-notes.md)
is the file-by-file map of today's code, with line numbers. `LANGUAGE.md` and `DESIGN.md` are the v4 reference
that v5 changes. `briefs/theory.md` is the research v4 rests on.

The checkout is `/home/user/axiom/.claude/worktrees/<your-lane>`, on a branch of
`claude/great-wozniak-pnqn7x-v5`. Work only in your worktree.

## The bar

The user's standard, in their words: *"really really excellent, beautiful, iterated code: highly beautiful and highly
clever Rust, highly cache friendly, using the borrow checker, with minimal branching and parallelism + fearless_simd.
Data structures deserve special attention, and unsafe tagless enums are exciting. Highly well abstracted, because we
want an MCP server and a GUI eventually."* Concretely:

### Readable first

- **Name from the domain.** Use the language's words: position, parcel, leg, promise, residual, behaviour. Avoid
  `data`, `info`, `handle`, `process`, `do_x`.
- **Every module opens with a `//!` doc** that says its job and *why its data structure is the one it is*.
- **Comments say why, never what.** No commented-out code, no TODOs without an owner.
- **Functions:**
  - usually under 30 lines;
  - 60 needs a reason the reviewer accepts, and over 80 is rejected;
  - at most 5 parameters, otherwise a borrowed context struct (`&Cx<'_, '_>`);
  - no `bool` parameters: use a two-variant enum that says what it means.
- **Iterate.** Write it, read it back as a stranger would, and rewrite it shorter and clearer. The first version is
  never the one you hand in.

### Data structures get special attention

- **Arenas and ids.**
  - Everything lives in an `Arena<T>` (or a pre-order `Tree<T>`) and is referred to by a `u32` `Id<T>`.
  - Variable-length parts are `Run<T>` ranges into one flat arena, or `Groups<K, V>` (compressed rows).
  - No `Vec<Vec<_>>`, no per-node `Box`, no pointer chasing in hot loops.
- **Lay data out for the pass that reads it.**
  - When a pass scans one field of many items, store that field in its own column (struct of arrays).
  - Keep hot structs small, and assert it: `const _: () = assert!(size_of::<Leg>() <= 32);`.
- **Prefer dense arrays indexed by ids.** Use `Map` (FxHash) only for sparse lookups, never inside a per-event loop.
  Use sorted vectors with `partition_point` or a merge where others reach for a tree or a map.
- **Tagless enums are welcome**, in one place. A *tag column* (`Vec<Tag>`, one byte each) beside a *payload column*
  of a `union` (16 bytes, all fields `Copy`) makes a stream of mixed values scan at memory speed.
  - It lives in `core` (for example `core::tagless`), behind a safe API whose constructors keep tag and payload in
    agreement.
  - Every `unsafe` block carries a `// SAFETY:` comment naming the invariant. `debug_assert!` checks the invariant,
    and a unit test exercises every variant.
  - Every other crate keeps `#![forbid(unsafe_code)]`.
  - No other `unsafe` anywhere without the orchestrator's agreement.

### The borrow checker does the bookkeeping

- **Each phase produces immutable data that later phases borrow:**
  - `File<'s>` borrows the source;
  - `Book<'s>` borrows names from it;
  - `Plan<'b, 's>` borrows the `Book`;
  - a view borrows the `Run`.

  Never clone immutable data to get around a borrow. Restructure instead.
- **Staged writes are RAII guards that roll back on `Drop`** unless committed (`Staged`, PROPOSAL K4). Never
  hand-written rollbacks.
- **No `Rc`, `Arc`, `Mutex` or `RefCell`**, ever.

### Minimal branching

- **Data-driven tables:** `const` arrays of `(word, variant)` and lookup by index.
- **Exhaustive matches on enums** instead of chains of `if`. Make invalid states unrepresentable, so there is
  nothing to check.
- **Branch-free arithmetic** where it stays readable, such as a sign as a multiplier, or `min`/`max`/`clamp`.
- **Validate once at the boundary**, then trust the types. No phase re-validates what an earlier phase guarantees.

### Parallel and SIMD

- **Parallelism goes through `core::par`** (scoped threads, ordered results, no locks). Parallelize independent work:
  files, owners whose books never touch, views, forks for `available` and the forecast.
- **`fearless_simd` 1.0 is allowed** for byte scanning and numeric kernels, when a benchmark in your report shows
  it pays (at least 1.3× on that path). Keep a scalar path, and dispatch once at the top, not per call.

### Abstracted for an MCP server and a GUI

- **Library crates never print.** Every crate boundary is a typed, immutable value.
- **Views, diagnostics and explanations are data with stable locations**, so an editor, an MCP tool or a GUI can
  render them and act on them (code actions are edits).
- **Keep the query surface small and typed:** a session holds the book, answers queries and takes edits. Do not
  build that surface unless your brief asks. Do not make it harder to build.

### Dependencies and toolchain

`memchr` and `fearless_simd` only. Stable Rust (1.94 here; the workspace says 1.90).

### Diagnostics stay gorgeous

Every diagnostic states the fact in the book's words, points at causes, and gives the fix as an edit. Build them
through the diagnostic catalog once it exists (lane K0), never by hand in a new site.

## Working rules

- **Verify before every commit you hand in:**
  1. `cargo fmt --all`
  2. `cargo clippy --workspace --release -- -D warnings` (allow only what the brief allows)
  3. `cargo test --workspace --release`

  Then run `sh tests/golden.sh` and `sh tests/mistakes/run.sh`, and check `git diff --stat tests/`.
- **The baseline:** 734 tests pass, 4 fail, 8 are ignored. The 4 failures are named in `REMAINING.md`. Keep every
  passing test passing.
  - Never delete, skip, ignore or weaken a test.
  - Never change an expected value to make a test pass unless your brief says the behaviour changes, and then say
    so in the commit.
- **Goldens:** a golden output changes only when your brief says the behaviour changes. Each change is listed and
  justified in your report.
- **Measure the size** with `python3 briefs/loc.py .` before and after, and report it per crate. The workspace has a
  `rustfmt.toml` (width 120, `use_small_heuristics = "Max"`), so `cargo fmt --all` keeps counts comparable. Never
  change it. The v5 baseline is 49,428 lines.
- **Measure quality** with `python3 docs/v5/measure/hist.py crates` (the function-length histogram) and
  `python3 docs/v5/measure/fnlen.py crates` (the longest functions), before and after.
- **Commits:**
  - small and coherent, each building and passing;
  - messages in plain prose: what changed and why, then the evidence;
  - end every message with these two lines:

    ```
    Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
    Claude-Session: https://claude.ai/code/session_01DZMgABaaMrSoCXHzmY1D6u
    ```
- **Do not push, and do not merge.** The orchestrator merges.

## Your report

When done, reply with:
1. **What you built**, as a list of commits.
2. **Before and after:** lines per crate (loc.py), the function-length histogram, tests passed/failed/ignored, and the
   golden and mistake diffs (none, or each one justified).
3. **The data structures you chose and why**, with their `size_of`.
4. **What you could not do**, or did differently from the brief, and why.
5. **The three places you are least proud of**, so the review starts there.
