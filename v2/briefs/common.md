## Context

Axiom is a typed, plain-text personal-finance ledger language, implemented in Rust
under `v2/`. It is a cargo workspace with these crates:

- `core`: numbers, dates, ids, trees, groups, diagnostics and `par`;
- `syntax`: parser, producing the AST;
- `model`: AST to `Book`;
- `engine`: `Book` to `Run`, the timeline fold;
- `report`: the views;
- `cli`: the `axiom` binary, rendering, `sync`;
- `systems`: embedded `.ax` standard library.

We are in the **v4 rework**: from a chart of accounts to agents, resources, events and
promises. There are no income, expense or equity accounts. Parties are the other end of
flows, purposes say what flows are for, assets are identified things whose history is
their basis, contracts are promises, claims live on parties, and terms change by
statements in the journal. `v2/LANGUAGE.md` is the normative spec and
`v2/examples/v4-sketch/` its worked example.

The user's bar, in their words:
- "ruthlessly review and rework all of the code … reworking preexisting abstractions
  and making it far cleaner and simpler and stronger … accept nothing but truly perfect
  and beautifully abstracted code";
- "code quality is dropping: simpler and stronger abstractions and invariants and
  increases in performance still are unpursued, and the syntax could be simpler still";
- "Error messages could yet be improved … leverage our mechanisms to a far greater
  degree to improve them even more and really show the might of our architecture";
- "a system operated at this scale for many businesses will be mostly automated".

The orchestrator reviews every line and sends back anything that falls short. Be
ambitious about the *formulation*: find the abstraction that makes three special
cases one. Be conservative about *breadth*: stay inside your lane.

## Read first

- `v2/LANGUAGE.md` and `v2/DESIGN.md`, in full. They are the spec and the source of
  truth. Where the code disagrees, the spec wins, within your lane.
- `v2/examples/v4-sketch/`, with its README: the target surface.
- `theory.md` (the theories the design rests on) and `v2/examples/explore-v5/FINDINGS.md` (what five
  ledgers could not say) in `v2/briefs/`, where your brief points at them.
- The code audits in `v2/briefs/`, for the findings your brief names:
  `audit-core-syntax-model.md` and `audit-engine-report-cli.md`. Their line numbers
  are from the v4 branch; find the code by name on yours.
- `v2/tests/mistakes/REPORT.md` §12–§14, the diagnostic style guide, if your lane
  writes diagnostics. `limabean.md` in `v2/briefs/` is the benchmark we beat.
- Your crate, in full, before you change anything.

## Code-quality rules

- **Readable first.** Use descriptive names, small functions, early returns and `match`
  on enums. Use no macros unless they remove real repetition. **No code golf**: fewer
  lines must come from better abstractions, never from density.
- **Borrow the source and reference by id.** Use `&'s str` or `Sym` for names and typed
  `Id<T>` for references. Nothing allocates per number or per date, or per flow on a
  hot path.
- **Encode invariants in types.** An enum is better than a bool pair, a newtype better
  than a raw integer, and a state that cannot exist should be unrepresentable. Prefer
  data structures that make the common case O(1) and the rare case correct. Facts that
  never change are computed once, not re-derived per use.
- **Concurrency.** Use `core::par` (`map`, `map_each`, `join`, `for_each_mut`) for
  parallelism. No `Arc`, `Mutex`, `Rc` or `RefCell`. Stable Rust only. Make
  concurrency safe by construction: immutable phase outputs (`File`, `Book`,
  `Plan`, `Run`, views) are `Sync` and shared by reference. Assert it statically
  (`const _: () = { fn is_sync<T: Sync>() {} … };`). Disjoint mutable state is
  split with `split_at_mut` or chunking, never locked.
- **Dependencies.** Only `memchr` and `fearless_simd` (1.0). Use SIMD only where a
  measurement shows it pays, and report the number.
- **Interfaces.** Views are data (sections, rows, typed cells, facts). Text and
  JSON are renderers over that data, so an editor or a GUI is one more renderer.
  No view computes anything while rendering.
- **Size.** The whole workspace must end v4 at or below **24,000** non-test lines;
  it is about 21,500 today. Every lane's additions are paid for by deletions as far
  as they can be. Your brief gives your share.
- **Diagnostics.** Errors are `Diagnostic`s with labels, notes and help, in the style of
  `tests/mistakes/REPORT.md` §14 and LANGUAGE §11: the headline states the fact in the
  book's words, labels point at causes, the fix is an edit. Never `panic!` on user
  input.
- **Comments.** They explain *why*, not what. Put doc comments on public items.
- **Tests.** Keep existing tests passing, adapting them where the spec changed
  behaviour. Add small, focused tests for new behaviour; don't write sprawling test
  suites. No clippy runs are needed.
- **Counting lines.** Use
  `python3 v2/briefs/loc.py v2`,
  which counts non-test, non-comment lines per crate. A size target is never a reason
  to golf.

## Working rules

- Work in your worktree. Touch **only the files your brief lists**. If another crate's
  interface needs to change, don't change it: describe the change in your final report.
- Before every commit:
  - `cd v2 && cargo check --workspace --tests` and `cargo test --workspace --release`
    must be green, unless your brief says which tests are expected to break;
  - run `sh tests/golden.sh` and read `git diff tests/golden`. Each change there must be
    one your brief asks for, and your report explains it.

  Commit the regenerated goldens.
- Commit in logical steps with clear messages, so an interruption loses little. End
  each with the attribution lines your own harness gives you for commits (its
  `Co-Authored-By` line and the `Claude-Session` line). Name no model anywhere else in a
  commit.

  Do not push.
- Your final message is a concise report:
  - what you built, and the abstractions you chose and why;
  - what you deleted;
  - line counts before and after;
  - performance numbers where relevant;
  - every golden change with its reason;
  - interface changes you need from other lanes;
  - anything you left undone;
  - the final commit hash.
