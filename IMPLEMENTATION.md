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
  threshold is met. D18, F06, F10, H11, and I02 now exercise their named
  valuation, retroactivity, open-interval, scenario, and bounded-rule
  semantics directly; review of the remaining corpus still keeps Gate 0 open.

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
- [x] Persisted sale-ledger closes cite a sealed analysis artifact produced
  only from a checked `CommitAnalysis`. The artifact binds the exact source
  commit, deterministic analysis child, stored proof, policy roots, book,
  typed period, and the complete recognized-sale/journal result set. Generic
  proof envelopes, stale sources, field relabelling, blocked results, empty
  books, and sales outside the requested period cannot authorize this path.
  Close construction is transactional across analysis proof, analysis commit,
  sealed artifact, and close insertion: a late blocked-close failure leaves no
  orphan authority objects or incremental state behind.
  This deliberately narrow API does not yet claim general close semantics for
  non-sale economic systems or mixed-period ledgers.
- [x] Analysis proofs bind their immutable source commit with a typed proof
  operation rather than a `commit:<hash>` string convention. The checker
  requires one rooted binding covering every other terminal root, and the
  store requires its external address to name an actual commit.
- [x] Source commits can pin exactly one independently verified compiled
  package artifact as immutable context. Corrections and analysis children
  inherit it exactly, divergent merges reject it, and legacy policy-package
  replacement clears it. Package-authored forms now elaborate only through
  this source-commit boundary: exact committed bytes select package names
  inside the pinned artifact, and results retain the source commit, artifact
  ID/hash, package-qualified schema IDs, and canonical value hashes. There is
  no ambient registry or public unbound elaboration fallback. The sale engine
  still does not execute these generic values.
- [x] An explicit `SettlementStateV1` package capability crosses that boundary
  end to end. The compiler accepts only the exact closed eight-field schema and
  binds the capability into v2 package/artifact/schema identities without
  changing legacy v1 hashes. Projection rejects holes, non-canonical IDs and
  dates, non-positive exact amounts, inconsistent facts, and illegal
  rail-specific histories. A separate proof format independently re-decodes
  the source, checks ordered exhaustive coverage and transitions, binds every
  form to its evidence, artifact, package root, schema, occurrence, and value,
  enforces fixed resource limits, and is reachable from a typed derived commit.
  `SettlementStateV1Proof` remains the only authority for this package-authored
  path. A package may expose a bounded one-hop compact syntax for this exact
  capability: semantic form surfaces use new v4 package/artifact identity
  domains, elaboration retains the normalized syntax origin, and any compact
  capable form selects settlement proof v2. The v2 checker independently
  re-elaborates the exact source and compares template, target, complete field
  mapping, schema/value identity, and ordered exhaustive coverage. Direct-only
  capable sources keep byte-identical v1 proof identities. This is not a
  general package lifecycle program or an `analyze_commit` / obligation
  integration.
- [ ] Every answer has a proposition-specific independently checked proof.
  Lot allocation, inventory conservation, and recognized gain now use typed,
  independently recomputed certificates. Direct obligations, settlement
  histories, satisfaction allocations, and both remaining-balance families
  now do as well. Quote, position/reconciliation, cash-settlement,
  settlement/reconciliation, and journal answers now carry typed certificates
  whose values and typed source edges are recomputed by the checker;
  blocked-sale answers now bind exact quantity, proceeds, both units,
  reason, account, asset, and the typed sale observation. Source-commit
  binding and the remaining answer families still need equivalent end-to-end
  treatment.
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
  Package-authored `form` blocks now stay in the single lossless CST and
  elaborate exact Bool/Text/Integer/Decimal fields and primitive typed holes
  against a package-root-qualified compiled record schema. Duplicate,
  malformed, missing, unknown, and ill-typed fields retain source-local
  diagnostics; the resulting immutable record binds one schema ID and one
  canonical value hash. Document elaboration aggregates independent failures,
  reserves occurrence IDs even for otherwise invalid forms, rejects malformed
  headers and unaddressable package names, and resolves same-named schemas only
  through the exact artifact pinned by the source commit. SettlementStateV1
  forms additionally have an independent typed proof and commit-anchored
  persistence path. Packages can now define a bounded `FormSurfaceV1` that
  performs only total one-to-one field renaming onto a closed primitive record.
  Its definitions are package-root/artifact-hash bound, resource bounded, and
  revalidated at store boundaries; there are no defaults, expressions,
  inference, alias chains, or cross-package lookup. Capability-bearing targets
  other than `SettlementStateV1` are rejected until a dedicated,
  independently replayable proof version exists.
  Nested row-qualified holes, semantic authoring templates, aliases,
  refinements, phase
  wrappers, general proof certificates, and query-result transport remain
  open. `SettlementStateV1` is the sole capability exception: compact syntax
  is accepted only because its dedicated proof v2 replays the exact lowering;
  all future capabilities remain rejected without an equivalent typed replay
  contract.
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
  Workspace now compiles those artifacts through its typed incremental graph
  with exact lockfile, package-set, and module-content dependencies.
  Compiled artifacts can now be persisted as immutable store objects whose
  artifact hash and complete compiler-input roots are independently
  recomputed on insertion and read. Source commits can pin exactly one such
  artifact, and package-scoped export coherence now permits independent
  packages to reuse qualified names while rejecting duplicates within a
  package. An immutable resolver binds direct record schemas to the exact
  artifact, package root, qualified name, and complete row type. Equivalence
  to the separately represented executable policy-package objects is
  deliberately not claimed; that conversion proof remains open. The compiler
  now revalidates public HIR row order and uniqueness at its authority
  boundary, and schema-guided record binding normalizes exact integer-to-
  decimal widening so source spellings cannot fork semantic value identity.
- [ ] LSP diagnostics, completion, and proof navigation foundation. A typed-HIR
  document index and checked proof-DAG navigator now provide the three core
  operations. A checked authoring surface retains exact source commits, checks
  HIR spans and declaration spellings against them, and combines surface/HIR
  diagnostics with optional verified Workspace analysis proofs; complete
  surface lowering, transport, and incremental document updates remain open.

### Gate 5 — immutable store and reconciliation

- [x] Immutable content-addressed evidence, statement, decision, package,
  generic proof, typed settlement proof, commit, existing sale-close objects,
  and typed `SettlementCloseObject`/`SettlementCloseId` values. Settlement
  proof commits preserve the exact source artifact/package context and merge
  only as explicit typed roots. A settlement close stores the exact proof-child
  lineage, full policy, and reporting period; insertion, lookup, and full-store
  verification independently recompute the typed world, recognition, journal,
  and close. Workspace persistence is atomic. Settlement restatement names only
  the previous close and a corrected proof child; it inherits policy/period,
  requires a direct whole-source correction with identical source identity,
  rejects sibling histories, preserves legacy v1 close IDs, and validates a
  bounded v2 supersession chain without mutating historical closes.
- [ ] Corrections/supersession, tombstones, unavailable/redacted/deleted
  states, and as-known-at queries. The immutable store now implements these
  primitives; production workspace/analysis integration remains open.
- [ ] Semantic branch/merge with unresolved conflicts preserved. Store merges
  retain divergent decisions, statement polarities, and evidence corrections;
  they now persist content-addressed conflict lifecycle records into commits,
  reject non-ancestor merge bases, compare scoped decision selections and
  completeness branch snapshots, reject silent conflict drops, and block
  closes until an explicit resolution record is committed. Production
  analysis/collaboration integration remains open.
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
  and chargebacks. Package-authored SettlementStateV1 source forms now project
  and persist proven histories through an explicit versioned capability;
  executable package-defined lifecycle rules and recognition-engine integration
  remain open.
- [ ] General cash and accrual recognizers, invoices, monthly close, journal
  export. The narrow `SettlementWorld` projection now exposes typed
  `Observation` and `Cash` only: cash excludes histories whose final state is
  ineffective, requires explicit distinct endpoint-account mappings, and emits
  balanced immutable journal projections. Its monthly close is content-
  addressed, filters by the policy date, and persists atomically as a typed
  `SettlementCloseObject` only from an exact settlement-proof child; store
  boundaries independently recompute its world, recognition, journal, and
  close. Corrections create distinct close IDs while old closes remain valid;
  a typed restatement can link a corrected close to its immediate predecessor
  while inheriting the exact policy and period. This does not infer accrual or
  integrate with general obligations/`analyze_commit`.
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
  verified attestations, principal labels, hidden-node commitments, and a
  complete-source proof commitment exist as a foundation. Exact hidden hashes
  remain dictionary-testable, and authority for an expected source root still
  comes from the caller; confidentiality-safe commitments, persisted labels,
  and store/workspace enforcement remain open.
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
  The runner now has 13 workloads and reports unsupported semantics honestly;
  production Workspace cache/invalidation, normalization, and dependency-graph
  metrics, independent clean-recomputation equality, and isolated per-workload
  child-process peak RSS are
  measured. Serial/concurrent independent-workspace equivalence is measured
  separately and does not claim shared-engine parallelism. Currency exchange,
  corporate action, invoice/payment, ownership-role, package-upgrade, and
  recursive-logic workloads now report separately timed public-domain-API
  probes alongside honest source-projection scope labels. The one-row close
  workload reports only real Workspace revision/invalidation metrics; it does
  not claim a separate close semantic probe. The twelfth workload,
  `generic-form-elaboration`,
  exercises 1,000/10,000/100,000-form profiles through persisted compilation,
  source-commit artifact pinning, schema-bound document elaboration, canonical
  value rechecking, and a one-row correction. Large profiles are explicitly a
  corpus of independent 256-form authoritative batches, not one ledger; the
  release 10k/100k gate checks exact counts, provenance, and a 128 MiB RSS
  ceiling. Its revision timings are explicitly not labelled as
  incremental-engine metrics. The thirteenth settlement-state-proof workload
  likewise labels batch aggregates separately from singular proof identities
  and verifies revisions and atomic rejection in every batch. Its separate
  single-proof public typed persistence boundary accepts 3,309 rows and
  rejects 3,310 because the canonical proof bytes exceed the limit.
  `settlement_setup_ns` includes source
  load, package compilation/persistence, and artifact pinning;
  `document_elaboration_ns` is direct elaboration, while
  `settlement_document_projection_ns` includes Workspace elaboration plus
  settlement projection. Remaining domain workloads and shared-engine parallel
  measurement remain open. A separate 1,000-form release stress gate exercises
  capable-form projection, independent proof checking, commit anchoring, and
  full-store verification end to end.
- [ ] Property, differential, incremental/full, and parallel/single-threaded
  equivalence suites. A deterministic generated suite now exercises roughly
  1,500 parser, canonicalization, unification, cycle, time, unit, package,
  merge, and adapter cases through public APIs; fuzzing and full concurrency
  equivalence remain open.
- [ ] Parser, canonicalization, unification, cycles, proof, packages, time,
  units, merges, and adapter fuzz targets.
- [ ] Stable file/package formats and a mechanized specification of the small
  trusted checker boundary. Proof format v1 now has an executable byte-level
  specification, independent encoder coverage, literal hash vectors, and
  optional-ID conformance tests; decoding, migration, and mechanized proof
  remain open.

## Reproducible evidence snapshot

The current foundation checkpoint is exercised by:

- 388 library tests, 4 CLI tests, 8 independent reference/production
  differential tests, and 5 structured-reference outcome tests;
- 134 named constitutional cases with exactly 75 registry-linked independent
  case-specific tests and 12 grouped smoke suites; semantic review, not the
  raw count, keeps Gate 0 open;
- 14 deterministic heavy-ledger integration tests, 3 heavy economic-system
  workflows, 7 adversarial property tests, two explicit 10,000-row ledger
  release stress tests, one explicit 10,000/100,000 generic-form release
  stress gate, and one 1,000-form independently checked settlement-proof gate,
  plus 8 focused `settlement_books` tests and 6 focused settlement
  vertical-slice tests over a 26-row fixture (6 histories, 3 rails, 2
  instruments, and 3 monthly periods) with adversarial mapping, transition,
  identity, persistence, determinism, and correction checks, including a full
  obligation/settlement/satisfaction network, plus 9 generated assurance-family
  tests spanning roughly 1,500 cases;
- a 13-workload section XVII benchmark runner with JSONL measurements,
  proof/dependency metrics, production Workspace cache/invalidation, and
  clean-recomputation equivalence, six public-domain-API semantic probes, and
  process peak RSS; concurrent
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
