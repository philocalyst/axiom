# Confirmed-direction implementation ledger

This file tracks implementation evidence against `confirmed-direction.md`.
It is deliberately stricter than a feature roadmap: a gate is complete only
when its behavior is implemented, independently verified, and exercised by
the named acceptance tests. The alternative design in `other.md` may inspire
experiments, but it is not part of this contract.

## Constitutional invariants

| Invariant | Current evidence | Status |
|---|---|---|
| Evidence is immutable; corrections are new facts | `Workspace` commits exact source bytes; evidence/store corrections create new objects and retain history | Partial |
| Unknowns remain typed holes | Lossless CST preserves `_`/`?name`; V0 semantic lowering still admits only lot holes | Partial |
| No magical balancing or hidden accepted events | V0 journal is a pure projection and requires settlement evidence | Covered for V0 |
| Rules derive and never mutate accepted facts | V0 analysis is pure | Covered for V0 |
| Absence is not falsity | Logic uses scoped completeness evidence and refuses default negation under open-world/resource exhaustion | Covered in solver foundation |
| Ambiguity remains visible | Lot and quote alternatives remain visible | Covered for V0 |
| Conflicting evidence coexists | Position, settlement, quote, and decision conflicts survive | Partial |
| Occurrence, content, and external identity are distinct | Evidence/store/workspace preserve all three; some ontology facts still carry occurrence only | Partial |
| Quantity, cost, quote, value, basis, and proceeds differ | V0 distinguishes most fields; valuation is absent | Partial |
| Every nonzero quantity has a unit; zero is polymorphic | Enforced by exact/model/parser layers | Covered |
| Time roles remain distinct | Named time roles, intervals, uncertainty, recurrences, and calendars exist; source grammar is still narrow | Partial |
| Account categories are book interpretations | Nominal material, virtual-view, and book-account types now have a validated registry; parser/engine integration remains open | Partial |
| Actual and scenario worlds cannot leak | Named scenarios, accepted-root-bound realization tokens, and isolation tests exist | Covered in foundation |
| Every derived result is explainable | Canonical `Resolution` projects why/why-not/missing/conflicts/impact/inert repairs; CLI still exposes a V0 subset | Partial |
| One accepted event set feeds multiple books | Recognition projects one content-derived accepted world into multiple book policies | Covered in foundation |
| Product behavior is versioned policy, not kernel branches | FIFO and LIFO compile through one content-addressed selection-package evaluator | Partial |

## Delivery gates

### Gate 0 — semantic constitution

- [x] Exact arbitrary-precision integer, decimal, and rational arithmetic.
- [x] Explicit rounding modes and no floating-point authoritative path.
- [x] Orthogonal statement axes: polarity, world, force, phase, time,
  provenance, authority.
- [x] Four-valued support and `Truth × Multiplicity × Completion` results.
- [x] Scoped completeness claims and decisions.
- [ ] At least 75 independently executable edge-case fixtures spanning
  sections XIV A–I. The corpus names 134 cases and now has exactly 75
  registry-linked case-specific tests plus 12 grouped smoke suites. The count
  threshold is met, but review still classifies several cases as foundation
  boundary tests rather than complete domain semantics, so Gate 0 remains
  open.

### Gate 1 — executable reference semantics

- [x] Small, exhaustive reference evaluator.
- [x] Reference relations for candidate/selected lot, valuation, basis, gain,
  balances, obligation satisfaction, recognition, and availability.
- [x] Structured positive proof, negative proof, blocker, conflict, completion,
  and inert repair results; negative support requires scoped completeness.
- [x] Reference fixtures remain permanently runnable as a semantic oracle.

### Gate 2 — vertical slice

- [x] Two buys, unresolved disposal, two conditional exact gains.
- [x] Conflicting quotes remain visible and do not poison direct gain.
- [x] FIFO selection is content-addressed and explained.
- [x] Policy/decision disagreement blocks recognition.
- [x] Position and cash-settlement observations reconcile.
- [x] Journal is a derived monetary projection.
- [ ] Every answer has a proposition-specific independently checked proof.
  Lot allocation, inventory conservation, and recognized gain now use typed,
  independently recomputed certificates. Direct obligations, settlement
  histories, satisfaction allocations, and both remaining-balance families
  now do as well. Quote, position/reconciliation, cash-settlement, and journal
  answers now carry typed certificates whose values are recomputed by the
  checker; source-commit binding and the remaining answer families still need
  equivalent end-to-end treatment.
- [x] Source-order property tests.
- [x] Actual/scenario isolation test.
- [x] Reference/production differential tests cover generated FIFO/LIFO,
  decision, conflict, quote, settlement, position, partial-lot, and multi-lot
  cases through the production `Workspace` boundary.
- [ ] Real incremental quote-change invalidation test.
  Source-partition instrumentation now proves the intended valuation-only
  dependency shape and clean-run equivalence, but the authoritative engine
  still analyzes a source ledger as one unit.

### Gate 3 — finance-native solver kernel

- [x] Canonical typed IR and alpha-equivalent goal identity.
- [ ] Rollback unification with occurs checks and typed row support; a general
  refinement system remains open.
- [ ] Positive least-fixed-point evaluation and cycle diagnostics exist, but a
  general tabled candidate-search engine remains open.
- [x] Stratified negation and exact finite count/sum aggregation, with explicit
  rejection of negative and aggregate dependency cycles.
- [x] Resource bounds produce `Incomplete`, never refutation.
- [ ] Theory boundaries for exact arithmetic, units, time, relations, and
  contract state.
- [ ] Proof checker verifies every accepted certificate without invoking the
  main solver. Allocation, inventory-conservation, and recognition
  certificates are now checked without the solver, including their typed
  source edges and aggregate arithmetic. Logic count/sum currently emits a
  structurally checked trace; an attempted generic arithmetic certificate was
  rejected in review because it could not bind supplied row values and
  completeness claims back to source-ledger semantics.

### Gate 4 — language and incremental compiler

- [x] Lossless CST, formatter, spans, stable semantic identity, and round-trip
  properties.
- [ ] Typed HIR, modules, names, rows, refinements, annotations, and rule
  classes. A deterministic HIR foundation now covers validated names/modules,
  open and closed rows, refinements, phases, declarations, diagnostics, and
  typed-hole constraints; annotations, surface lowering, and compiler
  integration remain open.
- [ ] Generic typed holes survive parsing through query results.
- [ ] Incremental semantic database with precise dependency invalidation.
  Production elaboration and analysis are memoized through the database with
  exact source/package edges and clean-run equivalence; invalidation within a
  source file is still stage-granular rather than proposition-granular.
- [ ] Versioned package compiler, coherence checks, and lockfile.
  The executable/store/workspace package boundary now has one lossless UTF-8
  conversion, canonical body/dependency identity, and rejects unrepresentable
  manifest metadata. A deterministic exact/caret/tilde package resolver and
  independently verified content-addressed lockfile foundation now exist. A
  deterministic HIR compiler rejects diagnostic-bearing modules, checks
  exported-name coherence, and emits independently verifiable artifacts;
  persistence and Workspace integration remain open.
- [ ] LSP diagnostics, completion, and proof navigation foundation. A typed-HIR
  document index and checked proof-DAG navigator now provide the three core
  operations; transport, incremental document updates, and Workspace wiring
  remain open.

### Gate 5 — immutable store and reconciliation

- [x] Immutable content-addressed evidence, statement, decision, package,
  proof, commit, and close objects.
- [ ] Corrections/supersession, tombstones, unavailable/redacted/deleted
  states, and as-known-at queries. The immutable store now implements these
  primitives; production workspace/analysis integration remains open.
- [ ] Semantic branch/merge with unresolved conflicts preserved. Store merges
  retain divergent decisions, statement polarities, and evidence corrections;
  they now persist content-addressed conflict lifecycle records into commits,
  reject silent conflict drops, and block closes until an explicit resolution
  record is committed. Production analysis/collaboration integration remains
  open.
- [x] Raw import bytes, versioned adapter provenance, idempotent re-import,
  retained derivation history, split/merge links, and decision inbox.
- [ ] Signed, reproducible close objects pin the complete semantic context.
  Commit/close signing bytes exclude signatures, and the store now requires a
  caller-supplied verifier for signed insertion; key management and a complete
  signed-close workflow remain open.

### Gate 6 — personal and small-business alpha

- [x] Entities, roles, instruments, positions, rights, and encumbrances.
- [ ] Material accounts, virtual views, and book accounts remain distinct.
  A nominally typed registry validates their separate identities, contracts,
  queries, recognizer mappings, and book scoping; source-language and engine
  integration remain open.
- [x] Directional transfers, exchanges, issue/retire, and conservation checks.
- [x] Obligations and many-to-many payment satisfaction.
- [ ] Check/card/ACH state histories, reversals, refunds, and chargebacks.
  Rail-specific append-only transition validation now distinguishes ACH,
  card, and check lifecycles, including check re-presentation, and validates
  bounded targeted provisional credits, fees, corrections, reversals, refunds,
  and chargebacks. Source-language, package-defined lifecycle, and engine
  integration remain open.
- [ ] Cash and accrual recognizers, invoices, monthly close, journal export.
- [x] Budgets and forecasts remain isolated scenarios.

### Gate 7 — investments and contracts

- [x] Lots track remaining quantity, provenance, rights, adjustments, and
  per-book basis.
- [x] Partial and multi-lot allocation without reuse, with exact proportional
  basis/proceeds, FIFO/LIFO package order, and allocation provenance nodes.
  Typed allocation, conservation-chain, and recognition certificates are
  independently recomputed and adversarially checked; other result families
  remain open under Gate 2.
- [x] Quotes carry time, venue, side, source, confidence, and validity.
- [x] Exact path-preserving multi-currency valuation.
- [ ] Corporate actions, debt schedules, collateral, and policy packages.
  Exact split/merge/spinoff/dividend, amortization, payment, and collateral
  state foundations exist with conservation and quantum checks; they are not
  yet ledger syntax, stored objects, or recognition inputs.

### Gate 8 — collaboration and adapters

- [ ] Signed packages and decisions, access labels, and redacted proofs. Typed
  verified attestations, principal labels, and hidden-node proof commitments
  exist as a foundation; store/workspace enforcement remains open.
- [ ] Capability-limited component adapters that can emit observations only.
  The public collaboration adapter boundary currently exposes only immutable
  observation batches; a runtime capability sandbox remains open.
- [ ] Reproducible adapters, shared books, semantic collaboration conflicts,
  and package migrations.

### Gate 9 — planning and optimization

- [x] Bounded recurrence and uncertain scenario assumptions.
- [x] Liquidity graph with time/fee/capacity/risk/tax dimensions.
- [x] Pareto alternatives and inclusion-minimal exact infeasible-core
  diagnostics for the implemented liquidity and scenario constraints.
- [x] Approximate route and scenario candidates cross an exact verification
  boundary; there is not yet a general optimizer frontend.

### Gate 10 — scale and assurance

- [ ] Benchmark corpus and performance/invalidation metrics from section XVII.
  The runner covers all named shapes and reports unsupported semantics honestly;
  production Workspace cache/invalidation, normalization, and dependency-graph
  metrics, independent clean-recomputation equality, and isolated per-workload
  child-process peak RSS are
  measured. Serial/concurrent independent-workspace equivalence is measured
  separately and does not claim shared-engine parallelism. Several domain
  workloads and shared-engine parallel measurement remain open.
- [ ] Property, differential, incremental/full, and parallel/single-threaded
  equivalence suites.
- [ ] Parser, canonicalization, unification, cycles, proof, packages, time,
  units, merges, and adapter fuzz targets.
- [ ] Stable file/package formats and a mechanized specification of the small
  trusted checker boundary.

## Reproducible evidence snapshot

The current foundation checkpoint is exercised by:

- 337 library tests, 4 CLI tests, 8 independent reference/production
  differential tests, and 5 structured-reference outcome tests;
- 134 named constitutional cases with exactly 75 registry-linked independent
  case-specific tests and 12 grouped smoke suites; semantic review, not the
  raw count, keeps Gate 0 open;
- 14 deterministic heavy-ledger integration tests and two explicit 10,000-row
  release stress tests, including a full
  obligation/settlement/satisfaction network;
- an 11-workload section XVII benchmark runner with JSONL measurements,
  proof/dependency metrics, production Workspace cache/invalidation, and
  clean-recomputation equivalence plus process peak RSS; concurrent
  independent-worker equivalence is labelled distinctly from shared-engine
  parallelism;
- strict all-target Clippy and exact source-to-commit-to-analysis proof checks.

These numbers are evidence, not a completion claim. In particular, several
benchmark shapes are currently parsed evidence workloads rather than full
domain semantics, and shared-engine production parallelism remains open.

## Completion rule

The confirmed direction is complete only when every gate above is checked and
the evidence remains reproducible from a clean checkout. Passing the current
V0 suite proves the vertical slice, not the whole system.
