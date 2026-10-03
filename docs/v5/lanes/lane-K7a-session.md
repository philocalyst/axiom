# Lane K7a: the Session, the one surface an MCP server and a GUI need

Read [`common.md`](common.md) first. Then [`../DESIGN.md`](../DESIGN.md) §3.8 (the trail) and §3.10 (the session),
[`../PROPOSAL.md`](../PROPOSAL.md) §5 K7, and the code that is the surface today: `crates/report/src/lib.rs` (`Query`,
`Report`, `Section`, `Cell`, `Fact`: typed data already), `crates/report/src/json.rs`, `crates/cli/src/project.rs` and
`commands.rs` (what a command does), `crates/sync/src/lib.rs`'s `SourceProvider`. Your worktree is
`/home/user/axiom/.claude/worktrees/lane-k7a`, on branch `claude/great-wozniak-pnqn7x-v5-k7a`.

**Your crates:** a new module or crate for `Session` (decide in the map: `report` is the natural home if the CLI,
and later `mcp` and `gui` crates, depend on it; say what depends on what), and `cli` (`project.rs`, `commands.rs`) as a
client of it. Lanes K5c and K3b/K6 touch the engine, the forecast and the model: stay out of them.

## What is wrong

The user wants an MCP server and a GUI. Both need a **library surface** that today exists only as the CLI's
`commands.rs`: the CLI loads files, builds the book and plan, runs the fold, picks a view by name and prints text. Nothing
holds the loaded project as a value, so a second client re-implements the loading; nothing says what "apply this edit
and show me what changed" is; and every view re-derives what it needs from the run (`history.rs`'s replay,
`holdings_at`'s re-plan, `available`'s double fork). That last part is K7b's. This lane is the **surface**, built over
what is there, so that K7b/K4c can make it fast behind the same signatures.

## What to build

```rust
/// A loaded project: its sources, the book built from them, and the run of its fold. Nothing global; nothing prints.
pub struct Session<'s> { .. }

impl<'s> Session<'s> {
    pub fn open(sources: &'s dyn SourceProvider /* or an owned Sources: decide in the map */, options: Options) -> Session<'s>;
    pub fn diagnostics(&self) -> &[Diagnostic];
    pub fn query(&self, q: Query<'_>) -> Report<'_>;          // balance, flow, claims, tally, why, forecast ... as data
    pub fn what_if(&self, edit: &Edit) -> Report<'_>;          // read-only: the answer if the edit were applied
    pub fn apply(&mut self, edit: Edit) -> Applied;            // re-parse the file, rebuild, re-fold; what changed
}
pub enum Edit { Replace { file: FileId, range: Range<u32>, text: String }, Append { file: FileId, text: String } /* .. */ }
pub struct Applied { diagnostics_added: .., diagnostics_removed: .., /* what a client needs to refresh */ }
```

- **`apply` and `what_if` may rebuild everything** in this lane; the signature is what matters, and the cost is K4c's
  and K7b's to remove (undo to the checkpoint before the edit's day, re-fold, stop when a later checkpoint's state
  agrees). Say in the map what the incremental version needs of the fold, per DESIGN §3.8, and do not build it.
- **The borrow checker makes `what_if` safe**: it takes `&self` and returns data borrowed from a *temporary* session or
  an owned report, never a view of a state that can change; `apply` takes `&mut self`, so no `Report<'_>` outlives the
  state it was read from. Make the lifetimes say it and show a test that does not compile in a doc comment
  (`compile_fail`).
- **A report is data in both directions**: `Report` serializes (the existing `json.rs` writer) and the **edit** a client
  sends is a typed value, not a command line. A client that wants to **add a transaction** gets a helper that builds the
  `Edit::Append` from a typed `NewTransaction` (date, flows, purpose, code, with a formatter from `syntax`, not string
  concatenation in the client).
- **The CLI becomes a client**: `commands.rs` builds a `Session` and calls `query`; `project.rs` is the
  `SourceProvider` for files on disk. The CLI's behaviour does not change (goldens byte-identical).
- **Libraries never print; nothing is global**; every value is typed and immutable once built. A `Session` is `Send`
  if its parts are; say whether it is `Sync` and why (the fold's state is not shared).
- An **MCP-shaped example** as a doc-test or an example binary in `crates/report/examples/` (or wherever the map puts
  `Session`): open `examples/05-family`, run `query(Balance)`, `what_if` a new transaction, `apply` it, `query` again
  and print the JSON of the change. Not an MCP server: the proof that one is a thin loop over this API.

## Rules of this lane

- **No behaviour change** in any command: goldens, mistakes, the three known failures, byte-identical.
- No test deleted or weakened. The new code has tests: an `apply` that fixes a diagnostic removes it; a `what_if` does
  not change `query`; an `apply` of an edit that does not parse reports and leaves the session as it was (it must not
  panic or half-apply: the borrow checker and a rebuild-into-a-new-value make this free; show it).
- Common bar: functions under 40 lines, no bool parameters, no parameter bundles, no `Arc`/`Mutex`/`Rc`/`RefCell`.
  A `Session` must be buildable without any global state, so a GUI can hold several.

## Step 0: the map

`docs/v5/lanes/K7a-map.md`, committed before any code: what `commands.rs` does for each command (load, build, plan,
fold, view, render) and which parts are the session's; what `project.rs` does that is **file IO** (stays in the CLI
or a `fs` provider) and what is the book's; every place a view re-folds or re-plans (`history.rs`, `projection.rs`,
`holdings_at`, `available`'s two forks): name each, with its cost, so K7b has its list; the lifetimes in `Report<'b>`
and `Book<'s>` and what a `Session` that owns its text must look like (self-referential? decide with an owner
arena and a borrowed book, not `unsafe`: say how); where `Edit` is applied (the source provider, the AST, the
formatter), and what `syntax` exposes for locations.

## Verification

`cargo fmt --all`; `cargo test --workspace --release --no-fail-fast`; `sh tests/golden.sh` and `sh tests/mistakes/run.sh`
(byte-identical); `fuzz.py ... diff`; time `axiom balance`/`check` on `bench/` 100k and 1m: no slowdown from going
through `Session`.

## Measure

Lines per crate before and after; the histogram. This lane **adds** (about +400) and deletes the CLI's duplicated
loading; report both. State what K7b can now delete.

## Not in this lane

- Steppers, pivots, provenance `why`, deleting the views' re-folds: K7b. The trail, an incremental `apply`: K4c/K7b.
- An MCP server or a GUI crate. This is what they will be written against.
