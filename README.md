# Axiom

Axiom is a local-first, proof-producing economic ledger. The source ledger is
the final source of truth; journals, balances, recognition, explanations, and
policy listings are deterministic views over it.

The first vertical slice keeps the authoring surface deliberately small:

```text
book tax-us

buy buy/one on 2026-01-04
  10 ABC into brokerage
  for 200 USD
  fee 1 USD

sell sell on 2026-09-20
  10 ABC from brokerage
  for 500 USD
  lot ?lot

use lots/fifo for tax-us
```

Every non-zero quantity carries a unit. In V0, only a lot selector may remain
open (`?lot` or `?`); it stays visible until a policy or an explicit decision
resolves it. Derived results never rewrite the ledger.

## Commands

```text
axiom check   book.axm
axiom journal book.axm
axiom why     book.axm gain:sell
axiom packages book.axm
axiom --help
```

`check` reports the observed source state first, then unresolved lot choices,
conflicts, and useful next actions. A clean ledger exits with status 0;
unresolved recognition exits non-zero. Conflicting quotes are review attention,
not a blocker for a directly stated sale. Parse and usage failures are also
non-zero and identify the source line where possible.

`journal` prints the monetary recognition projection when recognition is
available. An entry is called balanced only when its settlement names an
account; missing settlement information leaves the journal partial and
blocked. The authored sale remains the record of the asset movement. An
unresolved lot or decision remains explicit rather than being silently
balanced.

`why` explains a goal (for example `gain:sell`) in terms of its source,
observations, policy, and decisions. `packages` lists the fixed V0 source
vocabulary and the policy packages used by the ledger.

Output is stable and intended for both a person at a terminal and a checked-in
review. The same source and package set produce the same result on repeated
analysis. Where order has economic meaning, a policy must state it explicitly.

## Design

The semantic pipeline is:

```text
source ledger -> observations -> resolution -> recognition -> journal/proof
```

The journal is a compact authoring and interoperability surface, not the
underlying ontology. Accounts, positions, obligations, transfers, evidence,
time, and policies remain distinct so the same accepted economic fact can feed
multiple books. V0 intentionally exposes only the small source vocabulary
above; broader extension mechanisms remain design material, not implemented
language features.

See [`DESIGN.md`](DESIGN.md) for the executable implementation contract,
[`confirmed-direction.md`](confirmed-direction.md) for the full architectural
direction, and [`other.md`](other.md) for an alternative design explored as
inspiration rather than as an implemented contract.
