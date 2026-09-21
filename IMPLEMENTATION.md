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
| Account categories are book interpretations | Documented, not modeled | Missing |
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
  sections XIV A–I. The corpus names 134 cases and currently has 12 grouped
  smoke suites; a shared suite is not counted as proof of every mapped case.

### Gate 1 — executable reference semantics

- [x] Small, exhaustive reference evaluator.
- [x] Reference relations for candidate/selected lot, valuation, basis, gain,
  balances, obligation satisfaction, recognition, and availability.
- [ ] Structured positive proof, negative proof, blocker, conflict, and repair
  results.
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
  independently recomputed certificates; the remaining answer families still
  need equivalent certificate types.
- [x] Source-order property tests.
- [x] Actual/scenario isolation test.
- [x] Reference/production differential tests cover generated FIFO/LIFO,
  decision, conflict, quote, settlement, position, partial-lot, and multi-lot
  cases through the production `Workspace` boundary.
- [ ] Real incremental quote-change invalidation test.

### Gate 3 — finance-native solver kernel

- [x] Canonical typed IR and alpha-equivalent goal identity.
- [x] Rollback unification with occurs checks and typed row/refinement support.
- [x] Tabled candidate search with positive fixed points and cycle classes.
- [ ] Stratified negation and finite complete aggregation.
- [x] Resource bounds produce `Incomplete`, never refutation.
- [ ] Theory boundaries for exact arithmetic, units, time, relations, and
  contract state.
- [ ] Proof checker verifies every accepted certificate without invoking the
  main solver. Allocation, inventory-conservation, and recognition
  certificates are now checked without the solver, including their typed
  source edges and aggregate arithmetic.

### Gate 4 — language and incremental compiler

- [x] Lossless CST, formatter, spans, stable semantic identity, and round-trip
  properties.
- [ ] Typed HIR, modules, names, rows, refinements, annotations, and rule
  classes.
- [ ] Generic typed holes survive parsing through query results.
- [ ] Incremental semantic database with precise dependency invalidation.
  Production elaboration and analysis are memoized through the database with
  exact source/package edges and clean-run equivalence; invalidation within a
  source file is still stage-granular rather than proposition-granular.
- [ ] Versioned package compiler, coherence checks, and lockfile.
  The executable/store/workspace package boundary now has one lossless UTF-8
  conversion, canonical body/dependency identity, and rejects unrepresentable
  manifest metadata; compilation and lockfile resolution remain open.
- [ ] LSP diagnostics, completion, and proof navigation foundation.

### Gate 5 — immutable store and reconciliation

- [x] Immutable content-addressed evidence, statement, decision, package,
  proof, commit, and close objects.
- [ ] Corrections/supersession, tombstones, unavailable/redacted/deleted
  states, and as-known-at queries.
- [ ] Semantic branch/merge with unresolved conflicts preserved.
- [x] Raw import bytes, versioned adapter provenance, idempotent re-import,
  split/merge links, and decision inbox.
- [ ] Signed, reproducible close objects pin the complete semantic context.
  Commit/close signing bytes exclude signatures, and the store now requires a
  caller-supplied verifier for signed insertion; key management and a complete
  signed-close workflow remain open.

### Gate 6 — personal and small-business alpha

- [x] Entities, roles, instruments, positions, rights, and encumbrances.
- [ ] Material accounts, virtual views, and book accounts remain distinct.
- [x] Directional transfers, exchanges, issue/retire, and conservation checks.
- [x] Obligations and many-to-many payment satisfaction.
- [ ] Check/card/ACH state histories, reversals, refunds, and chargebacks.
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

### Gate 8 — collaboration and adapters

- [ ] Signed packages and decisions, access labels, and redacted proofs.
- [ ] Capability-limited component adapters that can emit observations only.
- [ ] Reproducible adapters, shared books, semantic collaboration conflicts,
  and package migrations.

### Gate 9 — planning and optimization

- [x] Bounded recurrence and uncertain scenario assumptions.
- [x] Liquidity graph with time/fee/capacity/risk/tax dimensions.
- [ ] Pareto alternatives and infeasible-core diagnostics. Pareto routes are
  implemented; minimal infeasible cores are not.
- [ ] Approximate optimizer outputs are verified against exact constraints.

### Gate 10 — scale and assurance

- [ ] Benchmark corpus and performance/invalidation metrics from section XVII.
  The runner covers all named shapes and reports unsupported semantics honestly;
  production Workspace cache/invalidation, normalization, and relation-size
  metrics, independent clean-recomputation equality, and process peak RSS are
  measured; parallel determinism and several domain workloads remain.
- [ ] Property, differential, incremental/full, and parallel/single-threaded
  equivalence suites.
- [ ] Parser, canonicalization, unification, cycles, proof, packages, time,
  units, merges, and adapter fuzz targets.
- [ ] Stable file/package formats and a mechanized specification of the small
  trusted checker boundary.

## Reproducible evidence snapshot

The current foundation checkpoint is exercised by:

- 246 library tests, 4 CLI tests, and 3 independent reference/production
  differential tests;
- 134 named constitutional cases with 12 grouped smoke suites; 58 cases are
  explicit implementation inventory, and the remaining mappings still need
  independent case-specific assertions before Gate 0 can close;
- 13 deterministic heavy-ledger integration tests and an explicit 10,000-row
  stress test;
- an 11-workload section XVII benchmark runner with JSONL measurements,
  proof/dependency metrics, production Workspace cache/invalidation, and
  clean-recomputation equivalence plus process peak RSS; unavailable parallel
  metrics remain explicitly `null` rather than simulated;
- strict all-target Clippy and exact source-to-commit-to-analysis proof checks.

These numbers are evidence, not a completion claim. In particular, several
benchmark shapes are currently parsed evidence workloads rather than full
domain semantics, and production-scale memory/parallel metrics remain open.

## Completion rule

The confirmed direction is complete only when every gate above is checked and
the evidence remains reproducible from a clean checkout. Passing the current
V0 suite proves the vertical slice, not the whole system.
