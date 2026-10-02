# Lane K4b: one solve

Read [`common.md`](common.md) first. Then [`../PROPOSAL.md`](../PROPOSAL.md) §3 F2 and §5 K4 (read the paragraph on
constant folding twice), [`../DESIGN.md`](../DESIGN.md) §3.5, and the K4a map
[`K4a-map.md`](K4a-map.md) and `crates/model/src/split.rs`: K4a made the vocabulary this lane builds on. Your worktree is
`/home/user/axiom/.claude/worktrees/lane-k4b`, on branch `claude/great-wozniak-pnqn7x-v5-k4b`.

**Your crates:** `model` (`lower/flow.rs`, `lower/record.rs`, `split.rs`), `engine` (`ledger.rs`, `post.rs`, `infer.rs`,
`explain.rs`). Lane K3a works at the same time on `model/lower.rs`, `declare/` and `Role`: stay out of those.

## What is wrong

A split says: a header amount, legs that take a share, a fixed amount, a target, `all`, or the rest, and items that carve
out of, add to or take off the header. Resolving it (what does each leg come to, in what unit, does it conserve) is one
algorithm, and the code has **three** copies of it, in three phases:

| where | resolves | when |
|---|---|---|
| `model/lower/flow.rs::lower_items` and `lower_split_flow`, `lower_named_flow` | a statement's header, legs and items whose amounts are literal | at model time |
| `engine/ledger.rs::post_journal` | the same statement, when an amount is **computed** (an expression), or an exchange has a cost item | at fold time, for each flow, re-reading the statement's `Program` roots |
| `engine/ledger.rs::materialize_group` | a contract's template, with the escalation of the day, and an occurrence's own overrides | at fold time, for each occurrence |

K4a gave them one vocabulary (`Quantity`, `Part`, `Expr`, `Group`). This lane gives them **one function**.

## What to build

```rust
/// What a group's expressions are evaluated against: nothing at model time, the fold's state at run time.
pub trait Env {
    fn eval(&self, root: NodeId, scale: Ratio) -> Result<Option<Amount>, Fault>;   // None: an input is missing
    fn held(&self, place: Id<Place>, unit: Id<Commodity>) -> Qty;                    // for `all` and `=`
}

/// Header, legs, `...`, carve/add/less items and percent-of-header, resolved once: every leg's amount, in order.
pub fn solve(group: &Group<..>, env: &impl Env) -> Result<Solved, Fault>;
```

- `LiteralEnv` evaluates nothing and fails on a root; the model calls `solve` with it for every group whose amounts are all
  literal, and stores the solved amounts. **That is constant folding**: for nearly every transaction the fold then does no
  solving at all, only posting.
- `FoldEnv` is the engine's: `post_journal` and `materialize_group` call `solve` with it, for the groups the model left
  unsolved (a computed amount, a target, `all`, an input).
- The static conservation check goes with it (DESIGN §3.5 and THEORY T2): per commodity, the shares of a split sum to one
  and its constants to zero, or one leg is the rest. A split that cannot conserve is a model-time error, not a posting that
  fails on some day.

Delete the three copies. Keep each phase's wrapper (the model's lowering of the AST into a group, the fold's reading of a
flow or an occurrence into a `Group` and its posting of the result) and nothing else.

## Rules of this lane

- **Behaviour is preserved, with one authorised exception**: the static conservation check may report, at model time, a
  split that the fold today posts unbalanced or only fails at run time. List every case it catches in your report with the
  diagnostic, and whether any example or test book hits it. If one does, stop and say so before changing the book.
- Goldens, mistakes, the splits oracle (`docs/v5/measure/splits.py`, K4a's: use it against a baseline binary built from
  your starting commit) and `fuzz.py ... diff`: no difference except what you listed.
- No test deleted or weakened; the same three known failures. Tests that name a deleted function move to `solve`.

## Step 0: the map

Write `docs/v5/lanes/K4b-map.md` and commit it before any code change. For each of the three copies: its inputs and
outputs in K4a's vocabulary; each step it performs, in order (resolve the header, subtract legs, resolve `...`, apply each
item's sign, percent of the header, `all`, `=`, exchange cost, escalation, an omitted input); where the three **differ** in
any of those steps, and whether the difference is a behaviour or an accident. Anywhere they differ **as behaviour**, the
one function takes the difference as a parameter or an input, not as a second code path: say what it is.

The map is where you find out whether "one function" is true. If the three differ more than K4a's map suggests, say so,
and what the smallest honest unification is.

## Step 1: `solve`, tested alone

Write it in `model` (it needs `Group` and `Fault`; the engine already depends on `model`). Test it on its own before any
caller uses it: unit tests for each part, and a property test against the existing resolvers on generated groups (build
them from the same generator the splits oracle uses, or from `split.rs` values directly). Run old and new on at least
200,000 groups and require equal results, **including equal errors**. Report how many reached each form.

## Step 2: the model calls it

`lower_items`, `lower_split_flow` and `lower_named_flow` call `solve` with `LiteralEnv`. What cannot be solved there (a
computed amount, `=`, `all`, an input) stays unsolved in the group, for the fold.

## Step 3: the fold calls it

`post_journal` and `materialize_group` call `solve` with `FoldEnv`. Delete their private resolution. Measure
`axiom check` on `bench/` at 100k and 1m (`sh bench/run.sh 100k 1m`) before and after: constant folding means the fold
should do less, so a slowdown is a finding. Also report callgrind instruction counts for `check` at 100k
(`sh bench/profile.sh 100k`).

## Step 4: measure

Lines per crate before and after, the function-length histogram, and the longest function left in `ledger.rs`. The
target is about **−1,500 lines**: `materialize_group` is 404 lines and `post_journal` 265 after K4a.

## Not in this lane

- Events in columns, `Q` as a tagless value, `Staged` as a trail: K4c.
- Deleting the occurrence materializer's role in forecasting, the promise monitor, `contract_forecasts`: K5.
- Positions, tabs, `Role`: K3a.
