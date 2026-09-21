# Axiom V0 implementation contract

This repository implements one complete semantic slice, not a miniature
general-purpose theorem prover. The canonical text ledger flows through:

```text
exact source bytes -> immutable source commit -> observed economic facts
                   -> resolution -> recognition -> journal/proof
```

The source ledger is the final source of truth. Everything else is a pure,
reproducible view.

## Authoring grammar

Blank lines and lines beginning with `;` or `#` are ignored.

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

quote quote/one on 2026-09-20
  1 ABC = 52 USD

observe position brokerage 10 ABC
observe settlement sell 500 USD into checking

use lots/fifo for tax-us
decide sell lot buy/two
```

`buy`, `sell`, `quote`, `observe`, `use`, and `decide` are the deliberately
small V0 vocabulary. V0 intentionally stops at this fixed source surface; no
generic extension mechanism is part of this contract. Only the lot selector
may be open: `?name` names a lot hole and `?` is an anonymous lot hole. Other
quantities, units, and accounts must be concrete before the CLI lowers the
source ledger. Every non-zero number has a unit.

## Shared semantic contract

- `exact.rs`: arbitrary precision, exact decimal/rational arithmetic; no floats.
- `model.rs`: parsed economic facts and typed IDs. Keep occurrence identity
  separate from normalized content hashes.
- `package.rs`: canonical, domain-separated hashes for policy definitions.
  FIFO and LIFO use the same compiled selection-program evaluator; V0 does
  not expose a generic extension mechanism.
- `surface.rs`: the single lossless CST and formatter boundary.
- `parser.rs`: strict semantic lowering with line-oriented diagnostics.
- `proof.rs`: content-addressed proof DAG with independent integrity and exact
  arithmetic checks.
- `engine.rs`: deterministic analysis. Never select among multiple candidates
  without a policy or decision. Policy/decision disagreement is a conflict.
- `render.rs`: calm human output, derived journal, and source-level explanation.
- `workspace.rs`: immutable exact-source commits, correction history,
  strict elaboration, and commit-bound analysis proofs.

The central result axes are independent:

```text
truth:       observed | reconciled | unresolved | conflicting
multiplicity:none | unique | multiple
completion:  complete | partial | blocked
```

The CLI must make the common case simple:

```text
axiom check examples/fifo.axm
axiom journal examples/fifo.axm
axiom why examples/fifo.axm gain:sell
axiom packages examples/fifo.axm
axiom --help
```

## Acceptance gate

The initial fixture has two eligible lots, exact conditional gains of 299 USD
and 199 USD, conflicting quotes, an observed position, and an observed
settlement with an account. FIFO selects the first lot. A decision selecting
the second lot while FIFO is active creates a conflict and blocks recognized
gain without erasing the observed sale, position, or settlement. Quote
disagreement remains visible attention and does not block direct recognition.
