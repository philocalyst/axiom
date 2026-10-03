# Lane K3f: a debt is a parcel, and paying a bill settles it

Read [`common.md`](common.md) first. Then [`K3d-map.md`](K3d-map.md) **§6 (the design), §0.4 and §3** (what recognition does, which
this lane mirrors with the direction `Out`), [`K3c-map.md`](K3c-map.md) §0.5, §6, §10 (a tab; relief by `exact`/`code`/`oldest`;
`settle.rs`) and LANGUAGE §7 (claims: "a bill received"). Your worktree is `/home/user/axiom/.claude/worktrees/lane-k3f`, on
branch `claude/great-wozniak-pnqn7x-v5-k3f`. **This lane starts after K3d and K6 have merged**, and runs alone with K7b at most.

**Your crates:** `engine` (`settle.rs`, `claims.rs`, `post.rs`'s gate, `ledger.rs`'s `all`), `model` (`book.rs`'s `Class` and
`holds_parcels`, `Sides`), `report` (`claims.rs`, `balance.rs`, `available.rs`, `forecast/trace.rs`).

## What is wrong

`me owes pge 142.50 USD ^b1` is a plain balance on a `Debt`-class tab: `Class::Debt.holds_parcels()` is false, so a payment to
the party relieves nothing, `claims` says "Nothing is owed either way", and `available` counts a paid bill twice
(`docs/v5/measure/diff/cases2/claim-debt-tab.ax`: 857.50 against 665.00 when the `payable` gate is dropped). `owed_by_you`
(`claims.rs:95`) rebuilds a debt from the flows touching the place, by code, and the `payable` gate of `claims.rs:88` and
`Book::makes_debt` exist only because a debt is not a parcel.

## What to build (K3d-map §6, verified against the code first)

`holds_parcels` becomes a fact of the place (`class == Asset || it is a claim`, no new byte in `PlaceTraits`); a Debt tab holds
positive parcels of what is owed, made by the flow *out of* the tab (`me owes pge`), and a flow from the owner's money to the
party **relieves them** by the same `exact`/`code`/`oldest` order (`settle.rs` mirrored: `tab_of(party, owner, Class::Debt)`);
`Sides` gives a tab the sign that makes its parcels read as liabilities; `claims` lists a debt by `is_claim` and its class;
`owed_by_you`, the `payable` gate and `Book::makes_debt` **go**. Recognition mirrors K3d's rule with the direction `Out`: a bill
with a purpose is spending when made in accrual books and when paid in cash books, a bill's write-off (forgiven by the party)
reverses in accrual. Credit cards and loans stay plain balances: they are not claims on a party (say how the model tells them
apart, from the tab's kind).

## The proof

`claim-debt-tab.ax` and a family of probe books (a bill paid in full, in parts, by code, returned, forgiven, with a purpose in
cash and in accrual books; a card and a loan unchanged); K3c's and K3d's claims oracle extended to debts (the owner's side),
mutation-tested (every mutant killed by the oracle or a named test); goldens and mistakes byte-identical except what the
examples with a debt change (`07-landlord`'s `me owes summit-roofing`, `10-budgeter`'s bills: list each with a reason);
`fuzz.py ... diff`, `splits.py`; `axiom check` on `bench/` 100k and 1m, three runs, no slowdown.

## Rules

Common bar. No unsafe, no bool parameters, functions under 40 lines, no parameter bundles. The lane **deletes** `owed_by_you`,
the gate and `makes_debt`: report net lines honestly. No test deleted or weakened.

## Step 0: the map

`docs/v5/lanes/K3f-map.md`, committed first: every reader of a Debt-class place (K3d-map §6 lists them: `balance.rs:86`,
`claims.rs:88-95`, `trace.rs:120,153`, `available.rs`, `Sides::of`) and what each needs of a parcel; how a card or a loan is told
from a bill; the examples that have a debt, measured on a scratch build.

## Not in this lane

Asset parts, prorata (the user's decision), amortization of a loan (K5d).
