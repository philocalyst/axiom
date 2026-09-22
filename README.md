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

The same surface records a direct obligation and its payment without journal
machinery or a second configuration language:

```text
obligation invoice/a
  debtor customer
  creditor vendor
  performance transfer 100 USD
  due 2026-10-01

settlement payment/a
  kind ach
  from customer
  to vendor
  amount 100 USD
  state issued
  state presented
  state settled

satisfy allocation/a
  obligation invoice/a
  settlement payment/a
  amount 100 USD
  state applied
```

The amount unit is the settlement instrument; it is written once. Partial and
many-to-many allocations use more `satisfy` blocks. Settlement history stays
ordered source evidence, and only a valid settled instrument reduces an
obligation. Derived balances never become a second ledger.

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

`why` explains a goal (for example `gain:sell`, `obligation:invoice/a`, or
`satisfaction:allocation/a`) in terms of its source, observations, policy, and
decisions. `packages` lists the fixed V0 source vocabulary and the policy
packages used by the ledger.

Output is stable and intended for both a person at a terminal and a checked-in
review. The same source and package set produce the same result on repeated
analysis. Where order has economic meaning, a policy must state it explicitly.

The CLI loads the exact source bytes into an immutable content-addressed
commit before elaboration. Analysis proofs are bound to that source commit, so
changing even source trivia creates a new auditable source identity without
silently rewriting prior history.

Package-authored forms have one deliberately narrow semantic extension point:
an exported record may explicitly carry the versioned `SettlementStateV1`
capability. A capable form is decoded only through the compiled artifact pinned
by its source commit, validated as an ordered ACH/card/check history, checked a
second time by an independent typed proof checker, and anchored by a derived
commit. Record shape or schema names alone never grant this authority.

### Settlement vertical slice

`SettlementStateV1Proof` remains the dedicated typed authority for this path.
`SettlementWorld` is a checked, read-only projection with exactly two typed
interpretations: `Observation` and `Cash`; it performs no accrual inference,
does not execute arbitrary package lifecycle programs, and is not wired into
general `analyze_commit` or obligation analysis.

Cash recognition includes only histories whose final state is `settled`. It
requires explicit, distinct endpoint-to-account mappings and produces balanced,
immutable journal entries with typed proof edges. A monthly close is a
content-addressed projection filtered by the policy date (settlement date for
cash, occurrence date for observation), then persisted as a typed
`SettlementCloseObject` under a distinct `SettlementCloseId`. It can be
created only from the exact persisted settlement-proof child; the stored policy
and period are rechecked by independently recomputing the typed world,
recognition, journal, and close at insertion, lookup, and full-store
verification. Workspace persistence is atomic. Corrections create distinct
close IDs while old closes remain checkable; this path has no supersedes or
restatement linkage.

## Assurance workloads

```text
cargo test --locked --offline --all-targets
cargo test --locked --offline --test heavy_ledgers \
  stress_ten_thousand_evidence_rows -- --ignored
cargo test --locked --offline --release --test heavy_ledgers \
  stress_ten_thousand_obligations_settlements_and_satisfactions -- --ignored
cargo test --locked --offline --release --test settlement_proof \
  stress_thousand_settlement_forms_project_prove_anchor_and_verify -- --ignored --exact
cargo test --locked --offline --test heavy_settlement_close
cargo run --release --locked --offline --bin axiom-bench -- --self-test
cargo run --release --locked --offline --bin axiom-bench
```

The benchmark emits JSON Lines on stdout and a human summary on stderr.
Unsupported domain shapes are marked `shape_only`; unavailable production
incremental and platform-resource metrics are `null`, never synthetic values.
On Linux and macOS, peak resident memory is measured with `getrusage` and
labelled as process-lifetime RSS. Independent-worker measurements compare
serial and concurrent clean workspaces; they do not claim a shared parallel
engine, whose legacy schema fields remain `null`.

## Design

The semantic pipeline is:

```text
exact source bytes -> immutable commit -> observations -> resolution
                   -> recognition -> journal/proof bound to the commit

package form -> pinned artifact capability -> settlement event graph
             -> independent typed proof -> derived proof commit
```

The journal is a compact authoring and interoperability surface, not the
underlying ontology. Accounts, positions, obligations, transfers, evidence,
time, and policies remain distinct so the same accepted economic fact can feed
multiple books. V0 intentionally exposes a small built-in vocabulary for
trades, direct obligations, settlements, and satisfactions. Community packages
can now define the authoring schema for the fixed `SettlementStateV1`
capability. Arbitrary executable economic behavior and package-defined
recognition remain architectural direction, not implemented authority; the
typed settlement world above is the deliberately narrow exception.

See [`DESIGN.md`](DESIGN.md) for the executable implementation contract,
[`confirmed-direction.md`](confirmed-direction.md) for the full architectural
direction, and [`other.md`](other.md) for an alternative design explored as
inspiration rather than as an implemented contract.
