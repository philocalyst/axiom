# Lane U: the unification pass

Read [`common.md`](common.md) first (it is the bar every lane is held to, and this lane is held to it hardest). Then
[`../PROPOSAL.md`](../PROPOSAL.md) §3 to §7, [`../DESIGN.md`](../DESIGN.md), [`../STATUS.md`](../STATUS.md) (the lane
table, the **Decisions** table, every "in numbers" section), and **every map in this directory** (`K*-map.md`): the maps
are where each earlier lane wrote down what it found duplicated and could not touch. Your worktree is
`/home/user/axiom/.claude/worktrees/lane-unify`, on branch `claude/great-wozniak-pnqn7x-v5-unify`.

You are the one Opus lane. The user's words, which are the whole of your brief: *"the big unification pass is the most
crucial thing. I'm okay with the new added lines, but it's crucial that we stay at or below target. It will require some
reworks, and it has to be done very carefully and with great care, because it establishes the code quality going forward. It
needs to TRULY be less is more, to the extreme: a lot of things will need to be reworked, but I know it can come out more
performant, more robust, better typed, more elegant, and just as featureful, and more. It will need to really crank it."*

## The ceiling

**27,000 code lines, counted by `python3 docs/v5/measure/quality.py crates`** (non-blank, non-comment lines, test files and
`#[cfg(test)]` modules excluded; every crate counts, a new crate counts, a macro's body counts). On 2026-10-04 the tree is
**53,419**: `model` 18,234, `engine` 12,463, `report` 6,804, `syntax` 5,634, `sync` 4,262, `core` 3,156, `cli` 2,350,
`session` 502. **Aim at 26,000** so the ceiling holds when later work adds back. Rough room by crate, to be replaced by your
own table: `model` ≈ 8,000, `engine` ≈ 6,500, `report` ≈ 3,800, `syntax` ≈ 3,000, `sync` ≈ 2,000, `core` ≈ 2,200, `cli` ≈ 1,000,
`session` ≈ 500. These are guesses: the plan decides.

The target is half the code. It will not come from trimming. It comes from finding that **three things are one thing**, writing
the one thing well, and deleting the three. That is the job. Everything below serves it.

## What counts as a unification, and what does not

**Counts:**

- *One concept written N times.* The same fold over postings in `flow`, `tax`, `budget`, `tally`; the same "table of rows with
  columns" built by hand in a dozen report files; the same collect-resolve-lower shape in each model pass; the same diagnostic
  assembled field by field; the same name table, `match` ladder or `impl` repeated per kind.
- *A parallel hierarchy.* AST node, `Book` record, `Run` fact and `Report` row for one thing, each with its own vocabulary and
  its own conversions: one vocabulary, borrowed along the pipeline, instead.
- *A hand-written mechanism a type already gives.* A cursor, a heap, a parallel `Vec` and an index, a `match` that is a table,
  a `String` built for what is a `Display`, a bool that is a type, a loop that is an iterator adaptor or a merge of two sorted
  columns, a validity check the borrow checker or a newtype could make unrepresentable.
- *A feature that is a special case of a more general one* (a view that is an instance of the pivot; a norm that is a law; a
  relator that is a contract; a forecast that is the fold): the general one grows a parameter and the special one goes.
- *Dead and half-built code*: everything the maps list as "accepted and read by nothing", unreachable arms, wrappers that
  forward.

**Does not count (and is rejected at review):**

- Code golf: fewer lines by denser lines, chained one-liners, clever macros, removing what the reader needed. `rustfmt` at
  `max_width = 120` is the only formatter and its output is the line count.
- Deleting a feature or a diagnostic. Every behaviour has a golden, a mistake book, a test or an oracle: they are the judges,
  and **the output stays byte-identical unless this brief or the Decisions table says it changes** (list every change with the
  book that shows it; an output that is *more right* is listed, reasoned and left for me to accept).
- Moving code to dodge the count (a new crate, a `build.rs`, a generated file, `include!`, an external dependency). Dependencies
  stay `memchr` and `fearless_simd` only.
- `macro_rules!` that writes what a generic, a table or a trait could. A macro is allowed where it replaces at least three
  times its own size with something that reads clearly; say so where it is defined.
- Coupling that the domain does not have, to save a parameter. A longer signature that is honest is better than a bundle.
- Weakening a test. Tests move with what they test. A deleted function's test moves to its replacement; nothing is dropped.
- More `unsafe`. It stays only in `core`'s tagless module (argued with `// SAFETY:` and a test per tag), and a new use is
  proposed in the plan with its benchmark, not slipped in.

## What the Rust should be (the user's standard, again)

Readable first, then clever where cleverness is a **data structure or a type**, never an expression. Arenas and ids; columns
for the passes that scan one field; small hot records with size assertions; sorted vectors with `partition_point` and merges in
place of maps and trees; `Steps`, `Dues`, `Facts`, `postings` (the `fearless_simd` kernel) used wherever a hand-rolled version
still stands; typestate and newtype ids so the borrow checker refuses the wrong program; iterators, not index loops; enums with
a tag column where a million of them are scanned; no `Arc`, `Mutex`, `Rc`, `RefCell`. Parallelism where a fold has independent
parts (per owner, per position) and only where a measurement says it pays. Every module opens with a `//!` that says its job
and why its data structure is the one it is. The code will be read by a stranger who wants an MCP server and a GUI on top of it:
the surface is `axiom-session`, and what a client would want to ask for is a typed value, not a rendered string.

## Phases, and where you stop

You work in phases and **each phase ends with a report and a stop** (your final message). I review the code, merge the branch into
`claude/great-wozniak-pnqn7x-v5`, and send you on with the next phase through a message, so your context stays yours. Do not run
past a stop. Between phases keep the branch merged with the v5 head (`git merge claude/great-wozniak-pnqn7x-v5`; I merge other
lanes meanwhile) and resolve what comes up.

### Phase 0: the plan (read-only: no build is needed, and none is wanted)

Read **all** the code, crate by crate, and the maps. Do not skim. You are the one who will say what the tree should be. Write
[`../UNIFY.md`](../UNIFY.md), committed and pushed, containing:

1. **The census.** Code lines per crate and per file, ranked, with what each file is for in a clause. The 40 largest functions.
   The ten concepts that appear in the most files.
2. **The concept inventory.** For each concept the tree writes more than once (start from the list below and add what you find;
   I have not read everything the way you will): the places it is written (file and line), what each copy does differently
   and **whether the difference is real** (most are not), and the **one formulation** that replaces them (the type or trait or
   function, as a signature and a paragraph, not a hope).
3. **The ledger.** One numbered entry per unification (`U1`, `U2`, ...): what goes, what it becomes, **lines deleted, lines
   added, net** (by reading, with the counts of the code it replaces), the behaviour it could change (none, or the list),
   the proof it needs (which golden, oracle, mutation test or benchmark), the risk, and what it depends on. Group them into
   **checkpoints** of one to four thousand lines each that can merge on their own and leave the tree green.
4. **The budget table.** Per crate: now, the ledger's deletions and additions, after. **It must sum to the ceiling or below, with
   the 1,000-line margin.** If the honest ledger does not reach it, say so plainly and name the levers (PROPOSAL §7 lists them: the
   habit forecast, sync's importers, the cheat-sheet or help text, the number of report views, spellings of the language) with
   what each costs the user, so I can decide. Do not pad the ledger with entries you do not believe in.
5. **What you take from the queued lanes, and what you leave.** K12b (cleanup), K4c (flows in columns), K3e (parcels in columns),
   K3f (debts as parcels), K7c (output unifications U1 to U6 of STATUS Waiting-on-you 14, **decided: built**), and L2/L3
   (positions under their agent, an optional counterparty, purposes without a direction root: language changes, which need my
   sign-off in the plan). Each has a brief in `lanes/`. Say for each whether you do it as part of the ledger, sequence it before
   or after, or leave it as written, and why. Two lanes are running while you plan (K6b: the post host, in
   `/home/user/axiom/.claude/worktrees/lane-k6b`; L1: the junction, in `.../lane-l1`, whose map `docs/v5/lanes/L1-map.md` is the
   plan for `syntax`) and one more (lane D: small decided behaviour changes, brief in `lanes/lane-D-decisions.md`) follows: your
   execution starts after those have merged, so plan the code as they leave it and say what you expect it to look like.
6. **The checks.** How each checkpoint is proven (you have, in `docs/v5/measure/`: the differential harness `diff/`, `fuzz.py`,
   `splits.py`, `claims.py`, `loans.py`, `derives.py`, `relators.py`, `forecast.py`, `addresses.py`, the session scripts; the goldens
   (`sh tests/golden.sh`), the mistakes corpus (`sh tests/mistakes/run.sh`), `bench/`), what is missing, and what you will write.
7. **The risks to features**: anything the ledger would lose. The answer must be nothing.
8. **The order**, and for the first checkpoint a short design (types and signatures) good enough that I can judge it before you
   build it.

Stop. Your report says the ceiling is reachable or why not, the three biggest unifications by lines, and the questions you need
me to decide.

### Phase 1 onwards: execute the ledger, a checkpoint at a time

- Before the first change, build the **baseline binary** from the checkpoint's starting commit (`cargo build --release`, copy the
  binary out of `target`) and keep it. It is the oracle: output **byte-identical** on every golden, every mistake book, every
  example at the dates and views `diff/` covers, 200 fuzz books, `splits.py`, the oracles above, `bench/` 100k and 1m
  (`check`: not slower, three runs, fastest reported with the load average; RSS not up). A difference is a bug unless it is on
  the list this brief or the Decisions table allows.
- Commit per unification, each green (`cargo fmt --check`, `cargo clippy` where the repo runs it, the workspace tests: the two
  known failures `a_prorata_place_realizes_only_the_lots_share_and_deferrals_merge_into_one_lot` and
  `a_context_forecast_keeps_historical_and_same_day_obligations_once` are lane D's, not yours), so a bad unification is one
  `git revert`. Count the lines (`quality.py`) after each and write the running total in the commit body.
- **Build the new thing first, move the readers onto it one at a time, delete the old when it has no reader.** A commit that adds
  the general version and leaves two copies standing is not done; the last commit of each unification deletes the copies.
- Every new type gets the tests of what it must never allow (the compile-fail doctest for the borrow, the property test for the
  invariant, a model-based test against a naive version where there is a data structure). Mutate what you write and see the
  tests fail.
- At the end of a checkpoint: the line table before and after per crate, the deletions by file, every changed output with the
  reason, the benchmarks, and the checkpoint's running total against the budget. Update `docs/v5/STATUS.md` (your section
  only) and push the branch (`git push -u origin claude/great-wozniak-pnqn7x-v5-unify`; never any other branch).

## Candidates I know of (not the ledger; yours is longer)

- `report` (6,804): fourteen views and `why` each their own table-building; K7b built the pivot and the `Target` walk and left U1
  to U6 (`STATUS.md`, Decisions 14): the report layer should be pivots and one renderer, and `why` a walk. The habit forecast
  (`expected.rs`, `variable.rs`, `recurrence.rs`, `bands.rs`) becomes a source of flows applied through the fold.
- `model` (18,234): the largest crate and the least examined. Lowering is a set of passes that each collect, resolve and lower
  with their own staging and diagnostics; `Staged`, `problem` and `Word::of` are the K0a groundwork that was meant to make them
  one shape. Ask where a pass is a table, where two passes walk the same AST, where a `Book` record exists only to be copied into
  a `Plan`, and where the diagnostics' construction is more code than the check.
- `engine` (12,463): `post.rs`, `plan.rs`, `ledger.rs`, `settle.rs`, `claims.rs`, `lots.rs` (K3e's six strategies as a ranking),
  `assets*.rs`, `occurrence/`, `promising.rs`, `recognition.rs`: ask what a position is, and whether one position type (a
  stepper over parcels) serves the lots, the assets, the tabs and the debts.
- `syntax` (5,634): L1 merges `transaction` and `statement`, `flow` and `journal`; what is left after it is yours.
- `sync` (4,262) and `cli` (2,350): `cli` should be a thin client of `axiom-session` (a command table as data); `sync`'s
  importers and apply path are the same fold over a different text.
- Everything the maps list as built, accepted and read by nothing (`match`, `deposit` before K3f, `Derivation` variants never
  constructed, `Loan` fields no one reads): delete or build, never keep.
- The large functions (`fnlen.py`): each over 60 lines is a missing abstraction, not a long function.

## Rules

- Common bar, with this lane's emphasis: **the line count is the proof of understanding.** If you cannot say what two things have
  in common in one sentence, you have not found the unification yet. If a unification adds more than it deletes, it needs a
  reason that is not the count (a speed, a type that makes a bug unrepresentable), and the plan says so.
- Read the code, not the maps: the maps are mine and my lanes', and they are sometimes wrong (K6's and K6b's corrected the
  briefs). When the code disagrees with a map, the code wins and you say so.
- Do not touch another running lane's worktree or branch. You may read them.
- You may use Sonnet helpers (`Agent` with `model: "sonnet"`) for research in Phase 0 and for mechanical moves later, **one at a
  time**, each in its own worktree (`/home/user/axiom/.claude/worktrees/lane-u-NAME`, branch
  `claude/great-wozniak-pnqn7x-v5-u-NAME`), told to read the code themselves; **you read every line they commit**, and you delete
  their `target/` directory when they finish (the disk is shared by every lane: keep your own `target/` the only large one).
- Commit trailer, exactly: `Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>` and
  `Claude-Session: https://claude.ai/code/session_01DZMgABaaMrSoCXHzmY1D6u`. A model's name appears nowhere else: not in a
  commit subject, a comment, a doc, a test, or the plan.
- Never push to any branch but your own. No pull request. At the end of every phase the worktree is clean and pushed.
