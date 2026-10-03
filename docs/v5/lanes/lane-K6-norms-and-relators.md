# Lane K6: one rule IR, and an association is written once and true from both sides

Read [`common.md`](common.md) first. Then [`../PROPOSAL.md`](../PROPOSAL.md) §5 K6, [`../DESIGN.md`](../DESIGN.md) §2.3
(relators and projection), §2.5 and §2.7, and [`../research/ASSOCIATIONS.md`](../research/ASSOCIATIONS.md) §5 and §9
(what a relator carries, and the layers it is built in). Then the finished maps of the lanes before you (`K3b-map.md`:
what an address is; `K5b-map.md`: what a promise's occurrences are; `K12`'s section in `STATUS.md`). Your worktree is
`/home/user/axiom/.claude/worktrees/lane-k6`, on branch `claude/great-wozniak-pnqn7x-v5-k6`.

**Your crates:** `model` (`laws/`, `lower/also.rs`, `lower/infer.rs`, `rules.rs`, `purposes.rs`, the contract's `shares`
and `Match` in `book.rs`, `lower/contracts.rs`), and the engine's budget functions (`eval.rs`). Lane K5b/K5c touch the
promise side of `book.rs` and `occurrence.rs`: keep your edits to the template, `also`, `share` and `Match` types and
rebase onto theirs.

## What is wrong

Five things say "when a flow of this kind happens, derive another flow or a check", each with its own path:

| today | where | lines |
|---|---|---|
| `also ITEM \| FLOW [when E]` on a contract | `lower/also.rs`, `lower_alsos` | ~390 |
| `share 20% for studio` on a contract | `Contract.shares`, `Share`, engine | ~170 |
| `match 50% of [retirement] up to 6%` | `Match` (never set: dead) | |
| `budget food 900 USD monthly carries` | `laws/budget.rs`, the budget functions in `eval.rs` | ~550 |
| `sales-tax 8.625%` on a party kind | a path of its own | |
| a purpose a flow takes when it says none | `endpoint_purpose`, `taken_purpose`, `infer_for_flow` (`lower/infer.rs`), each ranking differently | |

and the laws themselves (`laws/compile.rs`, 1,288 lines) dispatch through nine tables in `rules.rs`. The user's complaint
(ASSOCIATIONS §1): a paycheck's withholding, both FICA halves, the deferral and the match are typed again on **every
contract**, and an employment seen from the employer's book is a second copy of the same legs with `when employer is owner`.

## What to build, in layers; each ends green and shippable

**Layer 1: one effect, one dispatch.** A law already has a trigger, `when`/`unless`, and effects. Add the one effect
`Derive(LegTemplate)`: derive a flow (an `Item` or a leg, in K4a's `Group` vocabulary, solved by K4b's `solve`) when the
trigger fires. Then:
- `also ... when E` on a contract is a law owned by that contract: `on flow when E derive ITEM | FLOW`;
- `share 20% for studio` is `on flow derive 20% of amount for studio`;
- `sales-tax` and `match` are the same;
- `budget` is `on flow warn total(month, carried) <= 900 USD`.
Delete `lower/also.rs`, `Share`, `Match`, `laws/budget.rs`, and the engine's budget functions **as far as the laws now
carry them**. The nine dispatch tables of `rules.rs` become one `Groups<(Trigger, Key), Rule>`. **Purpose inference uses
the same ranking as laws** (written, promise, party, party kind, commodity kind, account kind): one `classify` with a
rank, replacing the three inference functions.

**Layer 2: a relator's legs are written once on its kind, with roles as ends.** A kind with agent slots (a contract
kind: `employment`, `lease`, `management`) carries `also` legs whose ends are its **slot roles** (`employer`,
`employee`, `irs`):

```text
kind employment : contract
  has employee person
  has employer agent as with
  employer -> employee gross #wages
    - withholding(gross - deferred, employee.filing) -> irs #federal-tax
  also employer -> irs 7.65% of gross #payroll-tax
```

The fold **projects** each leg onto the book: an end that is one of the book's owners is that owner's position; any other
end is the outside; a leg with both ends outside does not touch the book. The household's book sees wages arrive at gross
and withholding leave as tax paid; the business's book sees the same kind from the other side, with the employer's own
half. **One kind, two books.** This is choreographic endpoint projection, restricted to one round, and it deletes every
`when employer is owner` and the second copy of every FICA line.

**Layer 3 (gated; do it only if layers 1 and 2 land inside budget):** `on start`/`on end` triggers on a relator's span;
`part` (positions that exist with the relator); `joins` memberships. These are ASSOCIATIONS §5.1 to §5.2 and are the least
settled part of the design (its own §10, item 6): **stop at the map for layer 3** and say what a run would need; do not
build it unprompted.

## Rules of this lane

- **No behaviour change for a book that does not use a relator kind.** Goldens, mistakes and tests byte-identical except
  what layer 1 lists: a purpose that the three inference functions assigned differently from the one ranking is a
  behaviour change; **list every one with a book that shows it** (the research counts purpose disagreements in
  `05-family` and `07-landlord`; they are the targets, so each should *improve*, and the report says so with the book).
- **Layer 2 is additive**: a new spelling that old books do not use. Its acceptance target: write `examples/05-family`'s
  paycheck and `07-landlord`'s lease/management in the relator spelling **as copies** (`examples/explore-v5/` or a test
  fixture, not the goldens' inputs), and show they check to the **same** balances, tallies and claims as the originals, in
  the household's book **and** in the employer's (a second book that uses the same kind with `employer` as an owner).
- A baseline binary from the starting commit; `fuzz.py ... diff`; a generator for desugared forms (`docs/v5/measure/`,
  seeded, in the style of `splits.py`): each sugar form beside its hand-written law, equal on `check`, `flow`, `tax`.
  Mutation-test it.
- Common bar. In particular: the effect and trigger are enums, the dispatch is one index, no `Box<dyn>`, no bool
  parameters, functions under 40 lines.

## Step 0: the map

`docs/v5/lanes/K6-map.md`, committed before code: for each of the five, its inputs and outputs in today's types, what
the engine does with it per flow, and where it differs from "a law that derives"; what the three purpose-inference
functions each rank and where they disagree **on the examples** (run them and count); the nine tables of `rules.rs`
and what keys them; what `Trigger` carries and what `Derive` needs of the law compiler's typing (units, `amount`,
`self`); how a contract's `with` and `for` are read for a leg's ends today and what `as with` would change; what
**projection** needs of the fold (an owner's set per book; K3's positions), and whether it is a lowering step (the kind's
legs are specialised to the owners at model time) or a fold step. Decide from the code and say why.

## Verification

At each commit that touches model or engine: `cargo fmt --all`; `cargo test --workspace --release --no-fail-fast`; the
mistakes corpus; `fuzz.py ... diff`; K4b's splits oracle (derived flows go through `solve`); the sugar generator.
`sh tests/golden.sh` at the end of each layer. Time `axiom check` on `bench/` 100k and 1m: no slowdown.

## Measure

Lines per crate; the histogram; deleted with file names. The target is the sum of the table above minus the desugaring
(~300) and `Derive` (~100): **about −1,200 lines** for layer 1. Layer 2 adds code (projection) and removes examples'
lines; report both.

## Not in this lane

- Addresses and filling slots by path: K3b. Rewriting every example to the new spelling and `fmt --upgrade`: L.
- Facts out, `why` as a provenance walk: K7. The promise's schedule: K5.
