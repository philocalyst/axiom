# Lane K12b: the long functions, the dead code, and the three jobs of `props.rs`

Read [`common.md`](common.md) first. Then the sections of `STATUS.md` named "K12, in numbers" and "Known gaps", the
maps of the lanes that left things (`K5b-map.md` §11 to §13, `K3c-map.md` §9 to §12, `K4b-map.md` §10 and §11) and
`K12`'s deviations in its report (built-ins are model constants in `model::builtin`, not `std.ax` slots). Your worktree
is `/home/user/axiom/.claude/worktrees/lane-k12b`, on branch `claude/great-wozniak-pnqn7x-v5-k12b`.

**Your crates:** `model` (`props.rs`, `lower/record.rs`, `lower/contracts.rs`, `names.rs`, `declare/`, `fill.rs`), `engine`
(`ledger.rs`, `assets.rs`, `post.rs`'s plumbing, `settle.rs`), `core` (the clippy errors). **This lane is pure refactor:
no behaviour change anywhere.** It runs after K3b (names, declare) has merged, and beside K5c, K7a or K4c only if the
files do not overlap (say in the map which you touch).

## What is wrong, by the numbers

The functions the bar rejects (over 80 lines) and the ones that earn a justification, from `review.py` and `hist.py` at
the time of writing (re-measure first):

| function | lines | params | where |
|---|---:|---:|---|
| `lower_occurrence` | 403 | 8 | `model/lower/record.rs` |
| `lower_loan_origin` | 146 | 9 | `model/lower/record.rs` |
| `post_written_occurrence` | 121 | 2 | `engine/ledger.rs` |
| `contract_forecasts` | 157 | 7 | `report/forecast.rs` (K5c deletes it: not yours) |
| `arrive` | 79 | 2, one bool | `engine/post.rs` |
| `lower_contract` | 70 | 3 | `model/lower/contracts.rs` |
| `finish` | 45 | 0 | `engine/ledger.rs` |

and dead code the compiler already says so: `Rank::Alias` (`names.rs`), `Assets::into_states`, `ConsumptionGuard.before`
and `.before()` (`assets.rs`), more with `cargo build --release 2>&1 | grep -A4 '^warning'`. `cargo clippy --workspace
-- -D warnings` fails on eight errors in `core` (`day.rs`, `calendar.rs`, `num.rs`, `tree.rs`, `unit.rs`) that no lane has
touched: fix them, they are small.

K12's own leftovers (STATUS "What K12 left"):
- **`props.rs` (about 1,180 lines) does three jobs**: the grammar of the built-in properties, the staging of `has`
  values, and the system settings. Split it into three modules that each say their job, and move built-in properties onto
  the generic fill path where that deletes the `Args` readers (about 250 lines): measure first what that deletes;
  do it only if it is a net deletion and not a second reader style.
- Two reader styles side by side (typed `Key<V>` reads, and a dynamic `Value`): pick the typed one wherever the schema
  proves the type; say what stays dynamic and why.
- `Book.sites` is keyed by a bare tuple: name it.
- Weights are validated (`owners dana 60%, theo 40%`) but not stored: store them (a column beside the members), since K6
  needs them. This is the one addition.
- The facts are frozen twice because `end` statements say `closed` after lowering: freeze once, after the last
  statement, if the order allows it; if not, say why in the module doc.

K3b's own leftovers (its map §11, and my reading of the merged code; **the three places its author is least proud of are
yours**): the reference is read in **three entry points** that each fall back differently (`Book::place`, `World::seek_place`,
`World::end_on`/`address_end`): one function says "what does this word mean, on this day, from this home" and the three call
it; `declare/parties.rs::Addressed` decides **by source text, before the entities exist**, which mentions are addresses and
must agree with the real gate `Book::is_spelled` (derived from tree shape: root, `Account` role, has a `loc`, a `/` in the
path): make the party pass ask one definition of "spelled", stored once and read by both, or say why that cannot be; the
settled-reference memo (`settle_addresses`, `Addresses::once`) is a second cache of meaning that relies on call order, and
`special_end`/`found_end` carry `#[inline(always)]` to pay for it (+0.5% Ir): measure what the memo buys now and delete it if
the index is cheap enough without; `own()` turns an owner's place into a holding after declaration, and the oracle does not
model it. Also: `shortest_that` enumerates `2^n` subsets of an address's fillers (n up to 15): a bound or a smarter order;
`owner` has no range (`acme/529` places `acme` as owner without a `wrong-kind`); `unknown-address` suggests the closest name,
not the closest address. **Do not grow the model: this lane deletes.**

K3c's and K4b's own notes: `name_claims` returns a `bool` and writes `scratch.selectors`, the caller then chooses
`if named { &scratch.selectors } else { m.select() }`, twice; `Request` is built by hand in three places with mostly
default fields (give it a constructor per use). And the K4b list belongs to K4c, not to you.

## Rules

- **Byte-identical output**: goldens, mistakes, the K0a harness (`docs/v5/measure/diff/`), `fuzz.py ... diff`, every oracle
  (`splits.py`, `contracts.py`, `claims.py`), `cargo test --workspace --release`: the same two failures. A diff anywhere
  is a bug in your change, not a finding.
- **Split a long function by what the pieces mean, not by line count**: a piece has a name from the domain, owns the
  state it needs, and takes what belongs together. **No parameter bundles to hit a count**: if a function takes six
  things, find which belong together, which is a method of one of them, and which is a value in its own right. A bool
  parameter is an enum. If a 403-line function is a sequence of phases, each phase is a function or a type with one
  method per phase: say which in the map. Do not reach for a "context struct" that holds everything; that is the
  same function with a bigger signature.
- No test deleted or weakened. Tests that name a moved function move with it.
- Commit per function or per module, each building and passing, so that a bad one is one `git revert`.

## Step 0: the map

`docs/v5/lanes/K12b-map.md`, committed first: for each function above, its phases (what it does, in order), what state
each phase reads and writes, and the cut you will make; for `props.rs`, the three jobs with line ranges and who calls
each; the list of dead items with their evidence; what each rejected cut would have cost.

## Measure

Lines per crate, the histogram (functions over 40, over 80: the target is **none over 80 and `lower_occurrence`
gone as a name**), the clippy count (target 0 new, the `core` eight fixed). A refactor that grows the tree must say why.

## Not in this lane

- Behaviour. The K4b list (`put`, the two `Env`s, exchange legs by shape): K4c. `contract_forecasts`: K5c.
