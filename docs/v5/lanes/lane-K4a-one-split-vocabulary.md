# Lane K4a: one vocabulary for a split

Read [`common.md`](common.md) first. Then [`../PROPOSAL.md`](../PROPOSAL.md) §3 F2 and §5 K4, and
[`../DESIGN.md`](../DESIGN.md) §3.5. Your worktree is `/home/user/axiom/.claude/worktrees/lane-k4a`, on branch
`claude/great-wozniak-pnqn7x-v5-k4a`.

**Your crates:** `model` (`journal.rs`, `book.rs`, `lower/`), `engine` (`ledger.rs`, `post.rs`, `explain.rs`, `eval.rs`) and
the two readers in `report` (`contracts.rs`, `forecast/expected.rs`, `why/line.rs`). Lane K12 works at the same time on
`model/props.rs`, `kinds.rs`, `purposes.rs`, `declare/`, `book.rs` (the `Kind`, `Place`, `Entity` fields) and the engine's
property readers. Keep your edits to `book.rs` inside the template and quantity types, so that the two merge cleanly.

## What is wrong

A statement that moves value says three things: a **header** (one end, an amount), **legs** (the other ends, each with a
quantity), and **items** (signed amounts carved out of, added to or taken off the header or a leg). The model has two
parallel type families for it:

| | for a contract's template | for a statement, and a written occurrence |
|---|---|---|
| a quantity | `TemplateQuantity` (9 variants) | `JournalQuantity` (8 variants) |
| the whole | `TemplateFlow { flow, out, arrive, legs, items }` | `JournalGroup { header, source, side, total, legs, leg_quantities, items }` |
| a leg | `TemplateLeg { flow, side, quantity }` | a `u32` index into the flows, and a parallel `leg_quantities` |
| an item | `TemplateItem` (11 fields) | `JournalItem` (6 fields) |
| expressions | `TemplateProgram { nodes }` | `JournalProgram { program, flow_roots, groups }` |
| an occurrence's own say | `OccurrenceTail`, `WrittenOccurrence`, `WrittenGroup` | |

and the engine reads both in `ledger.rs`, in three functions that each resolve the split a different way
(`materialize_group`, `post_journal`, `post_written_occurrence`). The two quantity enums differ by `Percent` and by whether
the root is stored inside the variant, and `engine::ledger::template_quantity` and `written_quantity` are 80-line
transliterations of each other (read them: `ledger.rs`, around lines 1067 to 1220).

This lane makes them **one** vocabulary. It does not change what any split means: K4b does that.

## Rules of this lane

- **No behaviour change.** Goldens and mistakes are byte-identical, the four known test failures stay the same four, and
  every test that passes passes.
- **No test deleted or weakened.** Tests that name the old types change to the new ones with the same assertions.
- A step that cannot be finished without a behaviour change stops there: say so in the report. Do not change behaviour
  to make a type fit.

## Step 0: the oracle, then the map

Before you change a line, build what lets you prove nothing changed.

1. **Build the baseline binary** from your starting commit and keep a copy (`cp target/release/axiom /tmp/axiom-k4a-base`).
2. **A split generator.** `docs/v5/measure/diff/` (lane K0a's differential harness) has a runner, `run.sh` and
   `compare.sh BASELINE NEW`, over 156 mistake cases and 9 valid projects. Read it, and run it once. It does not stress
   splits. Write `docs/v5/measure/splits.py`: a generator of small valid books (a seeded random choice, deterministic) of
   statements and contracts that use every quantity form:
   - headers with an amount, `?`, `all`, `=` (a target), `...` (the rest), a percent of the header;
   - legs of each kind, written both ways round;
   - items: `+` and `-` carved from the header and from a leg, with purposes, in the header's unit and in another unit;
   - a computed amount (an expression over `input`, a `param`, or another amount);
   - contract templates with escalation, a loan's derived payment, an `input`, a written occurrence that overrides an
     amount, adds a leg, or omits an input;
   - an exchange with a cost item, and an asset sale.

   It writes N projects into a directory; `compare.sh`-style, it runs `check`, `balance`, `register` of every place, `flow`
   and `why` on each flow, through two binaries, and fails on any difference in stdout or exit code. Aim for 2,000
   projects, and make sure it is not vacuous: report how many of them print no diagnostics and move money, and how many
   hit each quantity form (count by form in the generator itself).
3. **The map.** Write `docs/v5/lanes/K4a-map.md` and commit it before the first code change: for each of the types in the
   table above, where it is built (file, function), where it is read, and which variants can appear where. In particular:
   which of `Percent`, `Rest`, `Whole`, `Derived`, `Unknown` and `All` can ever reach the engine's occurrence path, and
   which are resolved by lowering. This map is what the design in step 1 rests on, and a reviewer will read it.

## Step 1: one `Quantity`

One enum for what a leg or header quantity can be, in `model`. Decide the variants from the map, and:
- a variant that cannot appear in some phase is **unrepresentable there**: do not leave an `Err(InvalidTemplate)` for the
  engine to find at run time. Two types where one is a subset of the other (the resolved-by-lowering forms and the forms
  that survive to the fold), or a phase parameter, are both fine. Pick the one that reads better, and explain it in the
  module doc;
- an expression root is a `Option<NodeId>` held beside the amount in some variants and absent in others: write it as it
  is, once (an `Expr` that is `Literal(Amount)` or `Computed(NodeId)`, which `TemplateAmount` already is).

Delete `TemplateQuantity` and `JournalQuantity` and the code that converts between them.

## Step 2: one group

One `Group` (header, legs, items) for a contract's template, a statement with computed amounts and a written occurrence's
override, in `model`. The legs are values in the group, not indexes into the flows beside a parallel quantity array. Items
carry what they need and no more (`TemplateItem` has 11 fields, `JournalItem` 6: the template's extra ones are the item's
flow data, which the group's flows own). Delete `TemplateFlow`, `TemplateLeg`, `TemplateItem`, `JournalGroup`,
`JournalItem` and `WrittenGroup`. Keep a distinct type only where the map shows a real difference, and say what it is.

## Step 3: one program

`TemplateProgram` and `JournalProgram { program, flow_roots, groups }` become one `Program`: the expression arena, and the
per-flow roots. `FlowExpressions` is sparse roots keyed by a flow's offset: look at whether a root stored with the leg it
belongs to removes the `partition_point` lookup in `post_journal`. Do it if it does.

## Step 4: the engine reads one thing

`engine/ledger.rs`'s `template_quantity` and `written_quantity` become one function. `materialize_group`,
`post_journal` and `post_written_occurrence` keep their algorithms for now (K4b merges them), but they read the one
vocabulary, and the code that existed only to adapt one family to the other goes. The report's three readers do the same.

## Step 5: measure

Per crate lines before and after (`python3 briefs/loc.py .`); the function-length histogram; types deleted, with their
`size_of` and the new types'. The target is a net reduction of **about 1,200 lines** across `model`, `engine` and `report`.
If you land well under that, say where the lines were that you expected.

## Verification, at each commit that touches model or engine

`cargo fmt --all`; `cargo test --workspace --release --no-fail-fast` (4 known failures: see `docs/v5/STATUS.md`);
the splits oracle on 2,000 projects against the baseline binary; `sh tests/mistakes/run.sh` (fast). `sh tests/golden.sh`
is slow (the household example takes 24 s per command until K12 lands), so run it at the end of each step. Then
`git diff tests/` must be empty.

## Not in this lane

- One algorithm for resolving a split: K4b.
- Events in columns, `Q` as a tagless value, `Staged` as a trail: K4c.
- Deleting the materializer and the forecast's second driver: K5.
- Properties and kinds: K12.
