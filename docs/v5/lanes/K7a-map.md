# K7a map: what a command does today, who borrows what, and what a Session that owns its text has to be

Written before the first code change of lane K7a, from the code at `bdc25f9` (K5c merged), and checked against what the code does
(the numbers were measured on this commit: the last section says how). Paths are in `crates/`; line numbers are those of `bdc25f9`.
The sources are [`lane-K7a-session.md`](lane-K7a-session.md), DESIGN §3.8 and §3.10, PROPOSAL §5 K7, `report/src/lib.rs`,
`report/src/context.rs`, `cli/src/commands.rs`, `cli/src/project.rs` and `cli/src/sync.rs`.

The CLI is already a pipeline with a session in the middle of it, by accident: `commands.rs::run` loads, parses, builds, runs and
renders, and a private `Session` struct in the same file (`commands.rs:405`) carries `book`, `run`, `sources` and the diagnostics from
the end of that pipeline to the printer. This lane makes the middle of the pipeline a library value that something other than the
command line can hold.

## 0. What the brief says and what the code does

Eleven things in the brief do not match the code, and each decides something below.

1. **`SourceProvider` is not in `sync`.** It is `report/src/lib.rs:223`, and it is a *catalogue of positions* (`locate(path, line)` and
   `describe(loc)`), not a reader of files. `sync`'s trait is `SourceRegistry` (`sync/src/planner.rs:33`: `read`, `text`,
   `generated`), an append-only registry of the inputs a sync reads. What the CLI calls `Sources` (`cli/src/project.rs:217`) is the
   text store both of them are views of, and **it is the thing a `Session` has to own or borrow**; it has no IO in it.
2. **`projection.rs` is gone** (K5c deleted it) and **`history.rs` does not replay the engine**: it scans `run.posted` (§3). The
   places that fold again are `Context::ledger_at`, `available` and the free-function path of `lib.rs`; `Plan::new` is built in five
   places. §3 names every one, with its cost.
3. **The parser never fails.** `syntax::parse` recovers item by item and always returns a `File` and a list of diagnostics (the two
   exceptions, `file-too-large` and `piece-too-large`, return an empty file and one error). So "an edit that does not parse" cannot
   mean "no tree": it has to mean *an edit that makes the file say something that does not parse*, which §5 turns into a rule.
4. **`Book<'s>` borrows only its names, and `Plan<'b, 's>` borrows the whole `Book`.** `Book`'s one borrow is `names: Interner<'s>`
   (`core/src/sym.rs:11`: a map and a vector of `&'s str`); everything else is ids in arenas. `Plan` holds `&'b Book<'s>`, so **no
   value can own a book and a plan**, and `Context<'b, 's>` (plan, run, checkpoint) cannot sit beside the book it was made from. §4
   is the consequence, and it changes the brief's picture of `Session` in two places (no stored plan; `what_if` by closure).
5. **`what_if(&self, edit) -> Report<'_>` cannot return data borrowed from a temporary session**: a temporary dies at the end of the
   function. The brief says "or an owned report"; a `Report<'s>` is made of `&'s str` names and is not owned. The way to hand out data
   borrowed from a temporary, safely, is to hand out the *scope*: `what_if(&self, edit, |after| ...)` (§6).
6. **`apply` cannot be `&mut self` over a borrowed, immutable text** unless the new text has somewhere to live that outlives the old
   book. §4 chooses an append-only arena of texts and says what it costs (memory for each applied edit, not for each hypothesis).
7. **`Query` has no owner scope; the context has.** `--for ENTITY` is `Context::new(book, options, whose)`'s third argument, and the
   fold does not depend on it (`Whose` only filters views). A session answers for any owner, so `query` takes it per call.
8. **`check` and a report command fold differently today**, on purpose: `check` calls `axiom_engine::run` (`Plan::new` and a fold) and
   then `axiom_report::summary`, which builds a second `Plan::new` (`report/src/lib.rs:339`); a report goes through
   `Context::new` (`Plan::new`, a fold that also forks the ledger at today and keeps a `Checkpoint`). The comment at
   `commands.rs:70` says why `check` does not pay for the checkpoint. Measured here: **the fork and the checkpoint cost 1% of the fold;
   the second `Plan::new` costs 4% of a 1m `check`** (§3). A session that keeps the checkpoint costs `check` that 1%; it does not
   remove the second plan, because `check` asks for the diagnostics (which folds, with a plan that is then dropped) before it asks for
   the summary (which needs a plan again): §14.
9. **"Three known failures" is two.** On `bdc25f9` exactly two tests fail:
   `source_tests::a_context_forecast_keeps_historical_and_same_day_obligations_once` (K5c's map §0.5: the year-end test cannot pass as
   written) and `tests::a_prorata_place_realizes_only_the_lots_share_and_deferrals_merge_into_one_lot` (STATUS "Waiting on you" 3).
10. **There is no formatter for a typed transaction anywhere.** `syntax::format` lays out an already-written file; `sync/src/write.rs`
    inserts text a command printed, by day, into the file the day belongs to (`Layout::file_for`). Nothing builds a flow line from a
    typed value. §7 puts the writer beside the session and says why not in `syntax`.
11. **The trail is not what makes a live edit slow or fast: `build` is.** On a 1m-flow book `build` takes 2.2 s of a 4.3 s `check`,
    the fold 1.7 s. DESIGN §3.8's undo-to-a-checkpoint removes at most the fold; the model is rebuilt from every file's tree on every
    edit. §9 says what an incremental `apply` needs, and that the first thing it needs is not in DESIGN §3.8.

## 1. What `commands.rs` does, command by command

Every command starts `Project::find` (walk up to an `axiom.ax`, or a lone `.ax` file) and `Project::load` (list, sort, read every
`.ax` file under the root on every core, check UTF-8, add the embedded systems the project does not override). `Help` and `Version`
need nothing of this.

| command | parse | build | plan | fold | view | render |
|---|---|---|---|---|---|---|
| `fmt` | `Sources::parse` | no | no | no | `File::format` per selected file | `fmt::apply`: the diff list; **writes the files** |
| `sync` | `parse_files` | `build` | `Plan::new` inside `engine::run` | `engine::run`, no checkpoint | `sync::plan` (a `SourceRegistry` over the files and an append-only `auxiliary` list), then `Fate::apply` **writes the files** through `axiom_sync::apply` | `render_sync` |
| `check` | `parse_files` | `build` | `Plan::new` inside `engine::run`, again inside `report::summary` | `engine::run`, no checkpoint | `check_memos`: reads the local files a declared `read` names (**IO**, `Project::read_local`) for the unknown-memo hints; `summary` | `Session::check` |
| every report (`balance`, `register`, ...) | `parse_files` | `build` | `Context::new`: `Whose::resolve`, then `Plan::new` | one fold that forks the ledger at today and keeps a `Checkpoint` | `Context::report_with_sources` | `JsonRenderer` or `TableRenderer`, with the book's errors above it |
| a report with an unknown `--for` | same | same | `Context::new` fails first; then `engine::run` | `engine::run` only to have the book's errors to show | none | `Session::refuse` |

What is the session's, and what is not:

- **The session's:** the parse of every file (on every core), the build, the collection of what they say (`parsed` then `built`
  diagnostics, in that order), the plan and the fold, the owner scope, the answer (`Report`), the summary, and the text store all of
  it is an answer *about*.
- **Not the session's:** finding the project and reading it from disk (`Project`); `fmt` (it needs trees, not a book; it writes
  files); `check_memos` and `sync::plan` (they read files and run commands the book *declares*: IO a GUI would do differently); the
  renderers; the exit code; `--all`; colour; the day (`system_today` is the CLI's clock, the session is given `Options`).
- **Order matters for `--json`.** `check`'s diagnostics are, in this order: the parser's (file by file), the model's, the ones
  `check_memos` found, then the run's (`commands.rs:78`, `:423`). `check --json` prints them in that order, so a session that
  hands back the first two and the last as separate slices lets the CLI put its own in the middle without changing a byte.

## 2. What `project.rs` does

| | what | IO? | where it goes |
|---|---|---|---|
| `Project::find`, `marker_above`, `failure` | the folder that holds `axiom.ax`; a lone `.ax` file is a project | canonicalize, `is_file` | **stays in the CLI** |
| `Project::load`, `read`, `find_sources`, `collect`, `display` | list, sort and read the files (every core), UTF-8 | yes | stays |
| `Project::read_local` | a sync input, confined beneath the root through symlinks | yes | stays (sync's) |
| `SourceFile` | one text, its path, whether it is embedded, a lazily built table of line starts | no | **moves to the session crate** |
| `Sources` | the files by `FileId`, then an append-only `auxiliary` list; `assemble` (embedded systems, project overrides, the 65,536-file limit), `get`, `find`, `describe`, `locate`, `parse`, `project_paths`, `all_paths`; `impl SourceProvider` | no | **moves** |
| `Sources::append_auxiliary_to`, `parse_files` | `pub(super)` helpers that exist only so that the text a book borrows (`files`) and the text a sync adds (`auxiliary`) are two `Vec`s the borrow checker can tell apart | no | **deleted**: an arena makes them one thing (§4) |

So `project.rs` is two things in one file: a *provider of files on disk* (what the brief wants it to be) and a *text store* that every
client needs. The second moves; the first stays, and an MCP server or a GUI brings its own (a GUI has buffers, not folders).

## 3. Every place a view folds again or builds a plan again, with its cost

Measured on the generated books of `bench/gen.py` (`--flows 100k`: 96,019 flows; `--flows 1m`: 982,013 flows), release build, the
fastest of two runs, on a four-core machine with a load of about 3 to 5: figures are good to about ±0.1 s at 1m. "+" is the
difference from the same book's `check`, which is a load, a build and a fold and nothing else (100k: 0.44 s; 1m: 4.6 s).

| site | what it does | cost (100k / 1m) |
|---|---|---|
| `Plan::new` in `report::report` (`lib.rs:242`), `report::summary` (`:339`), `Context::new` (`context.rs:33`), `claims::holdings_at` (`claims.rs:56`), `forecast` tests (`:538`) | the events, the solve, the laws' facts, the owners: O(book) tables, built from scratch | 31 ms / 175 ms (4% of `check`) |
| `check` | `engine::run` builds a plan, `summary` builds a second | **+31 ms / +175 ms wasted** |
| `Context::new`'s fold | one fold, one `Ledger::fork` at today, one `Checkpoint` (a clone of the world) | +6 ms / +18 ms over the plain fold (1%) |
| `Context::ledger_at(day)` for `claims --at`, `lots --at`, `available --at` (`context.rs:129`) | a day at or after the checkpoint resumes it (one clone of its world); a day before it is `plan.start` and a fold from the first fact to the day, then `advance_to_closing` | `claims --at <past>`: +0.12 s / +0.25 s; `lots --at <past>`: +0.05 s / +0.48 s |
| `available::from_ledger` (`available.rs:49`) | a `fork` + `advance(horizon)` for the baseline, then **a fork, `apply_runtime` and `advance(horizon)` for every slow holding of every owner** (`:292`) | `available` (today): +0.04 s / +0.75 s; `available --at <past>` (the fold from the start, and the forks): **+0.57 s / +12.3 s** |
| `forecast::view` (`forecast.rs:57`, `forecast/trace.rs:42`) | `plan.resume(checkpoint)`, `advance(today)`, `reach(horizon)`, every habit flow applied, `advance` to every month end up to `until` | `forecast --paths 20`: +0.08 s / +1.0 s |
| `claims::holdings_at` (`claims.rs:50`), `available::view_with_lens` (`:37`), `Past::Journal` (`forecast.rs:39`) | the **free-function path** (`report::report`): `Plan::new`, `plan.start`, a fold to the day, or the journal folded again by the forecast | the CLI never takes it; every `report` test does |
| `history::Snapshots::replay` (`history.rs:207`, 116 lines, the longest function in `report`) | **not a fold**: two passes over `run.posted`, a dense `days x (place, commodity)` grid, `accumulate`, splits, then `lens.place_qty` per cell | `balance --at`, `--monthly`, `--value`, `register`, `flow`: +0.02 to 0.05 s / +0.04 to 0.37 s |

Two readings of the table. **The fold is not what a query repeats; the *state at a day before today* is**, and it is repeated by
`ledger_at` (a fold from the start) and, much more, by `available`'s forks: 12 s of a 17 s `available --at` at 1m. And **a plan is
built on every entry**: 175 ms at 1m, per query, because `Plan<'b, 's>` borrows the book and so cannot be kept next to it (§4).
Both are K7b's and K4c's to remove (step marks instead of folds; views that read the run); this lane only has to not make them worse.

## 4. The lifetimes, and what a Session that owns its text has to look like

```text
text ──read──▶ File<'s> ──build──▶ Book<'s> ──Plan::new──▶ Plan<'b,'s> ──fold──▶ Run, Checkpoint ──views──▶ Report<'b>
 owner         borrows text        borrows text            borrows the Book        own everything            borrows the Book
                                   (names only)            (whole)                 (no lifetime)             (names)
```

- `File<'s>` and `Source<'s>` borrow `&'s str`; `build` returns a `Book<'s>` and the trees are dropped (`commands.rs:41`).
- `Plan<'b, 's> { book: &'b Book<'s>, .. }`, `Ledger<'p, 'b, 's>` (borrows the plan), `Lens<'b, 's, 'w, 'p>`.
- `Run`, `Checkpoint`, `Diagnostic`, `Whose`: no lifetime. `Context<'b, 's>` owns a `Plan`, a `Run` and a `Checkpoint`.
- `Report<'b>` is `Cell::Name(&'b str)` and friends: it borrows the book **for the length of one view**, not the plan, the context or
  the run (`Context::report(&self) -> Result<Report<'b>, _>`).

A `Session` that owns its text, its book and a plan would be self-referential three times over. Safe Rust has four ways out, and the
brief rules out `unsafe`:

| way | what it costs |
|---|---|
| ids instead of references: a `Book` that owns its names, a `Plan` that is handed the book each call | a change to `core::Interner`, `Book`, `Plan` and every `book.name(..)` caller: model and engine, which K3b and K6 are rewriting |
| rebuild what borrows, on every query | a parse, a build and a plan per query |
| an owner outside, borrowed by the session | the client declares two values and the borrow checker orders them |
| `Box::leak` | memory never returned |

The cheapest that keeps every value immutable and `Send` is the third, with two refinements that make it a *session* rather than
three locals in the CLI's `main`:

1. **Text lives in an append-only arena, `Texts`.** `Texts::keep(&self, file) -> &SourceFile` adds a text through a *shared*
   reference (a vector of doubling buckets of `OnceLock<SourceFile>`, std, no `unsafe`, no lock, one atomic counter, the same
   `OnceLock` `SourceFile` already uses for its line table) and nothing is ever moved or dropped until the arena is. So a `&'t SourceFile`
   stays valid while more texts are added, which is exactly what `apply` needs: **the new book borrows a new text from the same arena
   as the old book borrowed the old one, and the old book and the old text can be dropped, or kept, independently.**
2. **`Sources<'t>` is a table of `&'t SourceFile` by `FileId`, with the arena it adds to.** Cloning it copies pointers. A
   `what_if` clones it and swaps one entry; a sync adds its auxiliary texts to a clone. This replaces the `files`/`auxiliary` pair
   of `Vec<SourceFile>`: they were two vectors only so that the borrow checker would let the book borrow one and the sync append to
   the other.

So:

```rust
pub struct Texts { .. }                                    // the owner: append-only, Sync
pub struct Sources<'t> { texts: &'t Texts, files, auxiliary }   // FileId -> &'t SourceFile; a SourceProvider
pub struct Session<'t> { sources: Sources<'t>, book: Book<'t>, book_diagnostics: Vec<Diagnostic>,
                         options: Options, folded: OnceLock<Folded> }
```

and `Folded` is what a fold leaves that does not borrow the plan (`Run`, `Checkpoint`, the effects prefix): **lifetime-free, so the
session keeps it; the plan is not kept.**

**The plan is built per call, and the fold is lazy so that the first call needs only one.** `Session::query` builds
`Plan::new(&self.book)`, and the first call that needs the fold (a query, `diagnostics`, `summary`) folds it with *that* plan and
stores the `Folded` in the `OnceLock`. Eager in `open`, then a plan again in `query`, would build two plans for a CLI that asks one
question: +4% at 1m, +7% at 100k (§3). Lazily the CLI builds one, as today. For a client that asks many questions it is one `Plan::new`
a question (31 ms at 100k) until K7b makes views read the run and the plan stops being needed; **the cost is stated, not hidden.**

What the borrow checker then guarantees (these are the doc tests):

- `query(&self, ..) -> Report<'_>` borrows the session; `apply(&mut self, ..)` needs it alone. **No `Report` outlives the state it was
  read from**, and the compiler says so (`E0502`).
- `what_if<R>(&self, edit, ask: impl FnOnce(&Session<'_>) -> R) -> Result<R, Refused>`: the hypothetical session is a local of
  `what_if`; `ask` is higher-ranked over its lifetime and `R` cannot mention it, so **a report of a state that is gone cannot be
  returned** (rustc: "lifetime may not live long enough", no error code). Nothing is added to the arena by a hypothesis.
- `apply` builds the next session as a *new value* from the edited table and replaces `*self` only on success: a refusal returns
  before anything is assigned, so there is no half-applied state to guard against.

**What it costs, said plainly.** (a) Each *applied* edit leaves the previous text of that one file in the arena until the arena is
dropped: bytes, not books (the book is dropped on `*self = next`). A long-lived server that applies thousands of edits to a large
file should reopen from a fresh `Texts` now and then; `Texts` can say how many texts it holds. (b) A struct that holds a `Texts` and
the `Session` over it is self-referential, so a GUI keeps the arena in a longer-lived frame (the worker thread that serves the
session, or a leaked box for a process-long project) and a session in it. The brief asks for "a `Session` a GUI can hold several
of": it can; each has its own `Texts`, nothing is global, and `Session` is `Send` and `Sync` (§6).

## 5. Where an edit is applied, and what `syntax` exposes

Nothing applies a located edit today. The three things that look like it:

- `Diagnostic::help[].edit: Option<(Loc, String)>` (`core/src/diag.rs:79`): a fix, a byte range of one file and its replacement. The
  renderer draws it (`render/snippet.rs:93`), `json.rs:137` prints it as `fixes`. **No code applies it.**
- `axiom_sync::Change { path, before, after }` and `axiom_sync::apply(root, &[Change])`: the whole new text of a file, staged in a
  sibling and renamed over the target (`sync/src/apply.rs`). It is the way an applied edit reaches the disk, and a client that
  wants one saves a session's changed file through it. The session never touches the disk.
- `syntax::format(src, &File) -> String`: a file's whole text laid out again.

`syntax` exposes: `Loc { file: FileId(u16), start: u32, end: u32 }`, byte offsets; `parse(file, src, Folder) -> (File, Vec<Diagnostic>)`
per file and independent of every other (a large file is cut at item boundaries and parsed in pieces); `Folder::of(path)` (a path's
year and month complete a short date); `format`. So an edit is applied **to one file's text, by bytes**, and the one file is parsed
again (alone) to learn what the edit did to it.

`Edit` is therefore:

```rust
pub enum Edit {
    Replace { at: Loc, text: String },        // a diagnostic's own fix is one
    Append  { file: FileId, text: String },   // lines at the end of a file
}
```

(`Loc` is a `FileId` and a byte range: the brief's `Replace { file, range, text }` in the type the whole system already uses to point.
`Edit::fix(&Help)` makes the first from the second, so "apply this diagnostic's fix" is one call.)

**An edit is refused, and nothing changes, when** it names a file that is not loaded, or one that ships with Axiom (an embedded system
is not the client's to edit); when its range is not a span of the text (past the end, backwards, or inside a character); or when it
**makes the file parse worse**: the one file is parsed again and refused if its new parse has a diagnostic of error severity that the old parse of
that file did not (compared by severity, code and message, as a multiset). The last is the reading of "an edit that does not parse"
that survives §0.3: a book that already has syntax errors can still be edited, an edit that adds one is a refusal, and an edit
that makes the *model* complain (an unknown account) is applied, because the model's complaints are exactly what `Applied` reports.
`Refused` is a value, with a `Display`; nothing is printed.

`apply` returns `Result<Applied, Refused>` where `Applied { file, added, removed }` are the diagnostics (all of them, parse, model and
run) that the edit introduced and cleared, by the same multiset comparison. The brief says `-> Applied`; refusal has to be
representable.

## 6. The surface, and what depends on what

A new crate, **`axiom-session`** (`crates/session`), above `report` and beside `sync`:

```text
core ◀ syntax ◀ model ◀ engine ◀ report ◀ session ◀ cli          (and later: mcp, gui)
                           ▲                          │
                           └──────── sync ◀───────────┘
```

It depends on `core`, `syntax` (to parse and to check a refused edit), `model`, `engine` and `report`. `report` stays what its first
line says, *views over a run, a pure function of the book and its run*, and does not learn about syntax or edits. The brief called
`report` "the natural home"; a crate of its own keeps the lifecycle (own, parse, build, fold, edit) out of the views, and
`crates/*` is already the workspace's member list.

```rust
impl<'t> Session<'t> {
    pub fn open(sources: Sources<'t>, options: Options) -> Session<'t>;          // parse, build; the fold is lazy
    pub fn sources(&self) -> &Sources<'t>;                                        // a SourceProvider, and the catalogue the renderers draw from
    pub fn book(&self) -> &Book<'t>;
    pub fn run(&self) -> &Run;                                                    // folds on first use
    pub fn book_diagnostics(&self) -> &[Diagnostic];                              // what reading and building found
    pub fn run_diagnostics(&self) -> &[Diagnostic];                               // what folding found
    pub fn diagnostics(&self) -> impl Iterator<Item = &Diagnostic> + Clone;       // both, in the CLI's order
    pub fn summary(&self) -> Summary;
    pub fn query(&self, query: &Query<'_>, whose: Option<&str>) -> Result<Report<'_>, Diagnostic>;
    pub fn what_if<R>(&self, edit: &Edit, ask: impl FnOnce(&Session<'_>) -> R) -> Result<R, Refused>;
    pub fn apply(&mut self, edit: Edit) -> Result<Applied, Refused>;
}
```

`Session` is `Send` and `Sync`: `&'t Texts` (a `OnceLock` array behind an atomic counter), the `Book` (`Plan<'_, '_>` holds `&Book` and
the engine already asserts `Plan: Sync`), `Vec<Diagnostic>`, `Options` and `OnceLock<Folded>` (`Run` and `Checkpoint` are `Sync`, asserted
in `engine/src/lib.rs:122`). It is a const assertion in the crate, so a field that stops being `Sync` fails the build. Two threads may
`query` the same session: the first to need the fold does it, the other waits (`OnceLock::get_or_init`); the fold's state is never
shared mutably, because the fold finishes before anything reads it and a query forks the ledger from the stored checkpoint.

A `Report` is data in both directions: it serializes (`report::json`, which the example uses), and **the edit a client sends is a
typed value**: `NewTransaction { day, from, to, amount: Money, purpose, description, codes }`, whose `line()` writes
`2026-03-01 checking -> grocer 12.50 USD #food "weekly shop" ^receipt-9` (tokens checked, the string escaped, the tail in the
language's order) and whose `append_to(file)` is the `Edit::Append`. It is checked by parsing the line back and by the same
"does not parse worse" rule as every edit, and a test shows `syntax::format` leaves it unchanged. It lives beside the session and not
in `syntax` because the sentence it writes is L1's to change (`<-`, arrows leading legs): L1 will move it into the formatter it
rewrites, and today a second sentence written in `syntax` would be one more thing for L1 to port.

## 7. The CLI becomes a client

- `project.rs` keeps `Project` and loses `SourceFile` and `Sources`; `Project::load(&self, texts: &'t Texts) -> Sources<'t>` puts what it
  reads in the arena.
- `commands.rs`: `Texts::default()`, `Project::find/load`, `fmt` as before; then `Session::open`. `Sync` plans over a clone of the
  session's `Sources` (the auxiliary texts it registers go there, so the session is untouched); `check` adds `check_memos`'s findings
  between `book_diagnostics()` and `run_diagnostics()`; a report is `session.query(query, whose)`, and an unknown `--for` is the
  `Err` of the same call (the book's errors are `session.diagnostics()`, which folds). The private `Session` struct (`commands.rs:405`)
  is renamed for what it does, *presenting*.
- `sync.rs`'s `Catalog` (a `SourceRegistry` over `files` and `auxiliary`) becomes a thin registry over a `Sources`.
- What is deleted from the CLI: the second parse/build/run orchestration (`commands.rs:35-82`), `Sources::append_auxiliary_to`,
  `Sources::parse_files`, `source_by_id`, `LocalFiles`' bookkeeping of the `parsed` count, and `check`'s second `Plan::new`.

## 8. The baseline this lane is held to

`cargo test --workspace --release --no-fail-fast` on `bdc25f9`: see §12. Goldens (`sh tests/golden.sh`) and mistakes
(`sh tests/mistakes/run.sh`) reproduce the committed files byte for byte before the first change. The comparison harness is
`docs/v5/measure/diff/run.sh` (468 files) and a script that runs every command, as text and as `--json`, on every example,
with `--for`, `--at`, `--all`, `--relaxed`, `fmt --check` and `sync --dry` (1,592 files): both are run with a binary built from
`bdc25f9` and again after every commit.

## 9. What an incremental `apply` needs (DESIGN §3.8), and what this lane does not build

`apply` here rebuilds everything: parse every file, build the book, fold from the start. The incremental version has to take the five
costs of §3 and the one in §0.11 in turn:

1. **Retained trees.** `build` takes every file's tree. Re-parsing one file is cheap (`parse` on a month's file is milliseconds); what
   the session needs is to keep the other files' `File<'t>` (they borrow arena text, so they can be kept: the arena is the reason
   they can) instead of parsing them again. 213 ms at 1m.
2. **An incremental `build`: the floor.** `build` is 2.2 s at 1m and 257 ms at 100k and **not incremental in any part**. A journal
   edit lowers one file's lines; the declarations, the laws and the contracts are untouched, but `build` has no way to say so. Until
   it has, a live edit costs at least the build, however fast the fold. (This is what DESIGN §3.8 does not say.)
3. **Stable ids across an edit.** `Run.posted` is parallel to `Book::flows` and flows are sorted by day, then declaration order: an
   edit on day D can only change the ids of flows on or after D, **provided every other arena (places, entities, kinds, laws) is
   unchanged**, and an edit that declares something changes them. The session needs to know which kind of edit it applied: a journal
   line, or a declaration. K3a's lazily created places are appended, which keeps the prefix valid.
4. **Checkpoints kept, as marks.** `Checkpoint` is a clone of the world (6 ms at 1m; 144 month ends at 12 years would be 0.8 s and 144
   worlds of memory). DESIGN §3.8's trail makes it a mark: `Trailed` is in `core::trail` already, the fold's state is not behind it yet
   (K4c). The engine has the rest: `Plan::resume(&Checkpoint)`, `Ledger::advance_by_month(until, month_end)` ("an editor refolding after
   an edit stops at the first month end whose digest matches the old fold's") and `Checkpoint::digest`; nothing calls them but tests.
5. **A new `Run` from the old one's prefix.** The records the fold keeps (effects, gains, violations, headroom, `promises`) are indexed
   by id and appended in day order, so the old run's records before the checkpoint's day are the new run's, and what follows the
   digest match is the old run's, re-indexed by the edit's change in the flow count.
6. **Views that read the run, not the plan or the replay** (K7b), or an incremental `Plan` (K4c's `Events`).

This lane builds none of it. `Session::apply` has the signature that survives it.

## 10. Design decisions, and how each could be wrong

- **The arena grows by one text per applied edit.** If a server applies edits for days to one 60 MB file, it holds every version.
  The fix is `Session::reopen(&mut self, fresh: &'t Texts)`, which copies the current texts into a fresh arena; it is not built
  because a client can do the same with `Sources`, and no client exists yet to say what it wants.
- **A plan per query.** 31 ms at 100k, 175 ms at 1m, until K7b. If an MCP server answers many cheap queries on a large book this is
  the dominant cost; the alternative (a plan stored in the session) needs `Book` to own its names and `Plan` not to borrow it.
- **`what_if` by closure** is less convenient than a returned value, and cannot be held across calls (a GUI that previews a fix while
  the pointer is over it holds the closure's result, which is owned data). A returned `Session<'t>` would put each hypothesis in the
  arena for good.
- **Refusing on a *worse* parse** is a policy. A GUI that applies each keystroke would want it off; it can keep its own buffer until
  the text parses, and a policy enum is one parameter if it turns out to be needed. The default that cannot make the book worse in a
  way the language can already see is the safer one for a program that proposes edits (an MCP tool).
- **`Edit::Append` puts a line at the end of a file**, not in day order. `sync/src/write.rs` (`Layout::file_for`, `scan`) knows where a
  dated line goes; making it an `Edit::Insert { day }` is a later lane's, and it is the same `apply`.

## 11. Verification plan

Per commit: `cargo fmt --all`; `cargo clippy --workspace --release -- -D warnings` (the baseline already fails in `core`, unchanged:
STATUS "Known gaps"); `cargo test --workspace --release --no-fail-fast`; `sh tests/golden.sh` and `sh tests/mistakes/run.sh` with `git diff --stat tests/` empty; the two
comparison scripts of §8 against the baseline binary (`diff -r`: empty); `fuzz.py BASE NEW examples SEED COUNT diff`.
At the end: `check`, `balance` and the views of §3 on the 100k and 1m books, three runs, fastest, load average reported; `loc.py` and
`hist.py` before and after; every new test mutated and shown to fail.

## 12. The test baseline

`cargo test --workspace --release --no-fail-fast` on `bdc25f9`: **1,060 passed, 2 failed, 19 ignored** (the two of §0.9; the ignored
are benchmarks and the oracle runs). Every later commit is held to: the same failures, and no passing test fewer.

## 13. What was built

Six commits on `bdc25f9` (`git log --oneline bdc25f9..`):

| commit | what |
|---|---|
| K7a-map | this file, before any code |
| `axiom-session`: the text store | `crates/session`: `Texts` (the arena), `Sources`, `SourceFile` moved out of `cli/project.rs`; `Project::load(&Texts)`; the `files`/`auxiliary` pair, `append_auxiliary_to`, `parse_files` gone |
| `report`: `Folded` | `Folded` (run and checkpoint, no lifetime), `Context<'b, 's, F = Folded>`, `Context::over`, `summary_of`, `json::diagnostic` |
| `Session` and the CLI as its client | `open`, `query`, `diagnostics` (and the two halves), `summary`, `run`; `commands.rs` opens a session and presents it |
| edits | `Edit`, `Refused`, `Applied`, `what_if`, `apply`, `NewTransaction`; the example; the doc tests |
| this section | what was built, measured and left |

Non-test lines (`briefs/loc.py`), before and after: **cli 2,507 to 2,338 (-169), report 6,965 to 6,997 (+32), session 0 to 500; total
51,968 to 52,331 (+363)**, against the brief's "about +400". Why, honestly:

- **session +500:** about 150 of it is the CLI's text store moved (`SourceFile` and `Sources`, so cli lost the same), 33 the arena, 93 the
  session, 110 `edit.rs`, 75 `transaction.rs`, 13 the crate root, and about 25 more on `Sources` (`empty`, `auxiliary`, `files`, `axiom_file`,
  `with`, `reading`, `texts`).
- **cli -169:** the text store (about -150), `append_auxiliary_to` and `parse_files`, `source_by_id`, `LocalFiles`'s `parsed` count, and
  the parse/build/run orchestration in `run` (about -60 together), less the `Presenter`'s fields and the `check` and `sync` glue (about
  +40).
- **report +32:** `Folded` (+20) and the generic `Context` (+8), `summary_of`, `json::diagnostic` (+7).
- **The brief's "deletes the CLI's duplicated loading" is smaller than hoped**: what is duplicated is not loading but the
  *orchestration* of it (parse, build, fold, choose between `Context` and `engine::run`), and that was 60 lines. The text store was one
  copy, which moved.

Function lengths (`hist.py`): 3,242 to 3,290 functions; 1 to 10 lines 1,940 to 1,979; 11 to 20: 668 to 675; 21 to 40: 492 to 495; **41 to 80: 133
to 132**; over 80: 9 to 9, the same nine. No function this lane wrote is over 40 lines. `Context::report` is 42 (it was 41: an exhaustive match,
one arm for each of the 14 queries, which is what it is for); `run` in `commands.rs` is 38.

Tests: **1,091 passed, 2 failed (the two of §0.9), 19 ignored**, against 1,060, 2 and 19: five moved from `cli` to `session` and 31
written (26 unit tests, 3 doc tests, 2 `compile_fail`). No test deleted or weakened; the ones that moved or were touched changed in
plumbing only (a `Texts` to keep texts in).

## 14. Where the build parts from the plan above

1. **`check` still builds two plans when the book has no errors.** §0.8 hoped a session would remove the second. It cannot: `check` asks for
   the diagnostics first (to know whether to print a summary), which folds with a plan that is dropped, and the summary then builds
   another. A report builds one, as before: the CLI makes the query first, so the fold is made with the query's plan and the diagnostics read
   it. (The first version of the CLI read the diagnostics first and built two plans for every report; the timings caught it.)
2. **`diagnostics()` is an iterator**, with `book_diagnostics()` and `run_diagnostics()` as the slices behind it, so that `check` can put
   the diagnostics of a sync source's data files between them and keep `--json` in its order.
3. **`Applied::between` is public.** A client that asks `what_if` needs to know what the hypothetical session found that this one did not,
   and `apply` returns the same value.
4. **`Project` stays in the CLI and the example has its own 15-line loader.** An MCP server will want `Project`; moving it is a `git mv`
   and a `Texts` parameter, and it is not this lane's.
5. **No `Session::reopen`.** §10 said it would not be built. `Texts::len()` says how many texts it holds.
6. **One flow shape in `NewTransaction`**: `DAY FROM -> TO AMOUNT` with a purpose, a description and codes. A split with legs and a
   flow with `@ PRICE` or `for` are the same writer extended; they are not written.

## 15. Measured

Release build, the generated books of §3, a four-core machine shared with another lane (**load average 3 to 5.5 throughout**, so single
runs are good to about ±10%): the figures are from 8 to 12 *interleaved* runs of the two binaries (`ab.py`), the baseline being a binary built
from `bdc25f9`.

| | baseline min / median | with the Session min / median |
|---|---|---|
| 100k `check` | 0.468 / 0.499 s | 0.434 / 0.511 s |
| 100k `balance` | 0.472 / 0.492 s | 0.447 / 0.495 s |
| 1m `check` | 4.646 / 4.805 s | 4.721 / 4.908 s (+1.6% / +2.1%; user CPU 4.407 / 4.410 s) |
| 1m `balance` | 4.540 / 4.993 s | 4.387 / 4.742 s |

The other views (fastest of three, two rounds, not interleaved): 100k `balance --monthly` 0.461 / 0.466 s, `available` 0.466 / 0.454 s,
`forecast` 0.488 / 0.478 s; 1m `balance --monthly` 4.648 / 4.573 s, `available` 4.972 / 4.986 s, `forecast` 5.502 / 5.427 s. Peak RSS
680,052 / 680,004 KB (1m `check`): the retained checkpoint costs nothing a measurement can see. **There is no slowdown from going through
`Session` beyond the 1% of the fold that the checkpoint costs `check`.**

What a client that holds a session pays (`Session` on the same books, one process):

| | 100k | 1m |
|---|---|---|
| `open` (parse and build) | 301 ms | 2.58 s |
| the first query (the fold, and its plan) | 172 ms | 2.05 s |
| the next query (a plan, and the view) | 16 to 18 ms | 174 to 202 ms |
| `apply` of one flow (build, fold, and the fold of the old book to compare) | 398 ms | 4.85 s |
| the query after an `apply` | 17 ms | 198 ms |

## 16. How it was checked

The scripts are `docs/v5/measure/session/`: `allcmds.sh` (the 1,592 outputs), `fuzzcmds.py`, `ab.py` (interleaved timing), `mutate.py` and `mutants.py`.

- **No output changed.** `sh tests/golden.sh` and `sh tests/mistakes/run.sh`: `git diff tests/` empty after every commit. The 1,592
  outputs of `allcmds.sh` (every command, text and `--json`, on every example, with `--at`, `--for`, `--all`, `--relaxed`,
  `fmt --check`, `sync --dry`, a missing project, and two one-file projects) and the 468 of `docs/v5/measure/diff/run.sh`: **identical to the
  baseline binary, byte for byte, after every commit.** `fuzz.py ... diff` on 400 mutated examples: 0 differ. A new script
  (`fuzzcmds.py`) mutates an example and compares 15 commands through both binaries, stdout, stderr and exit status: 250 mutants, 3,750 comparisons,
  0 differ.
- **Mutants.** 26 changes to the new code, one at a time (the arena's slot arithmetic and counter; the table's replacement and its
  paths; the span check, the tail and the newline of an edit; the multiset and the identity of a diagnostic; the swap of added and
  removed; the JSON; the syntax and embedded-file refusals; forgetting to assign the new session; keeping a hypothesis's text; the order
  of the diagnostics; the owner; the written line's order, padding, escaping, token and sign). **24 were killed at once; 2 survived and showed
  two missing assertions** (a diagnostic's code is part of its identity; `Applied::file` names the edited file, not file 0): both were
  added, and both mutants were killed. Every mutant is named in the harness.
- **The `compile_fail` doc tests.** Rustdoc on stable does not check an error code, so the two bodies were compiled by hand: the first
  gives `E0502` ("cannot borrow `session` as mutable because it is also borrowed as immutable", at `apply`, with the report "later used
  here"), the second "lifetime may not live long enough" (the closure's return type `Result<Report<'2>, _>` against `&'1 Session<'_>`).
  Each has a passing twin that is the same code with the one offending line moved or changed.

## 17. What K7b can now delete, and what it can build on

- **The free-function path of `report`**: `report`, `report_with_sources`, `views` (about 60 lines of `lib.rs`: a second dispatch
  of the 14 queries), `claims::holdings_at` and `available::view_with_lens` (each builds a plan and folds), and the `Past::Journal`
  variant with its branch in `forecast/trace.rs`: **about 90 non-test lines**, and the tests that run a query both ways and compare
  (`context_views_match_the_legacy_views...`, `source_tests.rs`: about 60 call sites of `crate::report`) become one test of
  `Session::query`. `Context::new`, the owning constructor, goes with them.
- **A plan per answer**, once views read the run: `Session::with_plan` and the `Plan` in `Context` are the last reasons a session
  builds one (31 ms at 100k, 175 ms at 1m, per query).
- **`Context::ledger_at` and `available`'s forks**, as steppers: §3 is the list; the session is where they hang.
- **`Applied` computed by comparing two folds**: with the trail, the diff is what the re-fold changed.
- **To build on:** `Session::apply` has the signature that survives §9; `Sources::with` and `Texts::keep` are where a retained tree
  per file would live (§9.1); `Edit` is already what a code action, a sync `Change` (whole-file `Replace`) and an LSP edit are.

## 18. What is not finished, and the three places I am least proud of

Not finished: an incremental `apply` (§9: it is a rebuild, 4.85 s at 1m); `Edit::Insert { day }` (sync's `Layout` knows where a dated
line goes); `NewTransaction` for splits, prices and `for`; moving `Project` to a library so an MCP server does not write its own loader;
`Session::reopen` for a server that applies for days; `why` and the other K7b queries as `Query` variants (the session answers what
`Query` has).

1. **A plan per answer, and a lazy fold.** `with_plan` is the honest consequence of `Plan` borrowing `Book`, and it works, but a `OnceLock`
   filled by "whichever answer comes first, with its plan" is a thing a reader has to be told. `diagnostics()` can fold; `summary()` after
   it builds a second plan. It is correct and measured, and it is a wart until K7b makes views read the run.
2. **The arena grows and the owner is the client's.** A `Texts` keeps every applied edit's old text until it is dropped, `what_if` is a
   closure because a returned hypothesis would have to be kept there too, and a GUI that wants to hold a `Session` in a struct must
   leak or scope its `Texts`. The alternatives (§4) were worse, but this is the part of the surface a GUI author will meet first.
3. **`apply`'s notion of "worse" and of "the same diagnostic".** Refusing an edit that adds a syntax error, and identifying a
   diagnostic by severity, code and message, are policies I chose and argued, not facts. A change to a balance changes every assertion
   message in a book (`apply` of one flow to the 100k book says 1 added and 1 removed; the 1m book 49 and 49), so `Applied` is noisy where
   messages carry numbers. It also folds both books to say it. A stable identity (the diagnostic's anchor mapped through the edit) is
   the better answer and is more code than this lane.
