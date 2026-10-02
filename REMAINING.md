# Axiom: remaining work at the local stopping point

Stopped on **2026-10-02** at the user's request. The active checkout is
`axiom-local`, branch `cutover/promote-workspace`; verified product commit
`1be566d4b1ddfbde419076dddb65ffc1e8f98a7c`. The workspace compiles. The final
release suite has **734 passed, 4 failed and 8 ignored**. This checklist records
unfinished work; it does not claim migration completion.

Read the [stopping-point writeup](docs/local-rework/STOPPING-POINT-2026-10-02.md)
for exact failures, current financial output, preserved worker changes and
resumption order. [LANGUAGE.md](LANGUAGE.md) and [DESIGN.md](DESIGN.md) remain the
specification. The full original inventory and lane briefs are preserved in the
[historical checklist](docs/local-rework/history/REMAINING-2026-09-30.md) and
[briefs](briefs/README.md); historical branch status is not current verification.

## Verified and checkpointed

- [x] Promote the native workspace from `v2/` to the repository root (`86ac2b0`).
- [x] Remove the older root implementation and `v2-previous` in this isolated
  checkout, retaining Git history and removal inventories (`07c8a8e`, `3294182`).
- [x] Remove the unused `Book.plans` / journal-plan route (`76d787e`).
- [x] Integrate reviewed Luna slices for native lowering, typed purposes and
  reports, owner-aware APIs, sparse histories and kept-occurrence posting.
  The [commit sequence](docs/local-rework/COMMIT-SEQUENCE.tsv) records scope.
- [x] Run a fresh workspace check, complete release test run, formatting check,
  actual family/landlord probes and independent Source 04/05/07 arithmetic checks.
  Failures are retained and listed below.
- [x] Stop the workers; preserve clean commits, dirty patches and untracked files;
  move worker worktrees to `verification/retired-worktrees` outside this repository.
- [x] Write a current handoff and acceptance gaps without modifying the original
  Downloads repository, recovery evidence or retained financial expectations.

## First: repair the known failures

- [ ] Residence-only books must start their yearly schedule. Engine regression
  `temporal_days_count_an_inclusive_residence_before_the_first_flow` expects one
  check and currently gets none.
- [ ] Reconcile BasisZero funding with after-tax prorata contributions. Engine
  regression `a_prorata_place_realizes_only_the_lots_share_and_deferrals_merge_into_one_lot`
  gets zero plain basis instead of 5,188.24. Native IRA rules require explicit
  basis for nondeductible contributions; resolve this compatibility deliberately.
- [ ] Resume the forecast closing prefix once. Report regression
  `a_context_forecast_keeps_historical_and_same_day_obligations_once` lacks the
  10 USD year-end tax; its two fee obligations are present.
- [ ] Stop canonical loan forecasts after repayment. Report regression
  `native_loan_forecast_stops_after_the_typed_principal_is_repaid` emits five
  1,000 USD payments instead of three.
- [ ] Repair contract template purpose classification and occurrence recognition
  against actual full books. Current family check has 128 purpose disagreements
  and 13 assertions; landlord has 3 purpose disagreements, 12 occurrence-date
  errors and 29 assertions. Independent arithmetic passing is insufficient.

## Native runtime and API acceptance

- [ ] Complete the canonical monitor: expected/missed occurrences, persisted
  promise/open-claim state, checkpoint/hash identity and linked settlements.
  `Run.monitor_complete` is still false.
- [ ] Implement the scheduled ClaimChange handler, currently a no-op. Review
  retained `80a99b4` and `86e29ba` before integrating filtered waivers, due changes,
  literal credits and parcel selection. Close credit/claim dates, waiver recovery
  behavior and computed-credit diagnostics with native source assertions.
- [ ] Implement strict code → exact → oldest claim settlement and pro-rata ending
  recognition/refunds. Close measures and ending timeline events.
- [ ] Wire canonical deposits/refunds and LoanState. Replace the retained loan
  worker's incorrect ACT/365 convention with the nominal cadence used by Source
  07 and Sam, using real scheduled windows for prorated resets/prepayments.
- [ ] Review/integrate the corrected FX memo and explicit `value(... at POLICY)`
  consumer at `ee14432`; run engine regressions on the promoted root. Complete
  actual runtime owner-policy evidence and currency tallies, not only Book APIs.
- [ ] Complete chained `else` reparations, paid/deadline semantics, filed-return
  contributors and amendment diagnostics; remove remaining legacy semantic seams.
- [ ] Finish report view parity, canonical historical/same-day obligations and
  native Book/Run sync reconciliation, dry diffs and idempotence acceptance.

## Financial examples, diagnostics and corpus

- [ ] Close Source 05 classification, HSA reimbursement and closed-return parity.
  Latest tax output exits 1; earlier near-parity figures are historical.
- [ ] Close Source 06 wash/carry and Source 07 loan, deposit and roof-improvement
  basis parity without lowering any independent financial expectation.
- [ ] Finish Source 08 WIP: preserve FX rounding and FBAR bank peaks. Then close
  Source 08–11 actual runtime financial proof and grade every other example.
- [ ] Map all 73 original behaviors to exact native source tests/assertions in
  `crates/model/TEST-MIGRATION.md`; reconcile stale counts and pending rows.
- [ ] Regrade all 60 goldens and 125 mistakes at the new native checkpoint,
  document justified changes, remove runner error masking and portable-timeout gaps.
- [ ] Complete native benchmark-generator feature parity and direct emission.
  The current generated 1M fold has 2,505 errors and is not an acceptance result.

## Data structures, borrowing and parallelism

- [ ] Replace `core::par` per-chunk vectors and unbounded result backlog with
  bounded safe work/output storage; preserve ordering and panic propagation.
- [ ] Prevent nested CLI/parser loops from multiplying threads. Benchmark one
  huge file, balanced files and mixed huge/tiny workloads with identical checksums.
- [ ] Implement and measure flat report cell storage and borrowed row views;
  reduce repeated occurrence counting and duplicated runtime metadata.
- [ ] Establish correct end-to-end allocation, retained-memory, peak-RSS and
  wall-time baselines. Isolated sparse-history gains do not prove a tenfold
  whole-program improvement. No current whole-program performance gate is closed.

## Final acceptance and cleanup

- [ ] Pass all four known regressions and actual financial checks, then complete
  release tests, required lint checks, goldens, mistakes and native benchmark gates.
- [ ] Resolve repository-wide formatting differences: final `cargo fmt --check`
  reports diffs in 117 files. Avoid treating a whitespace pass as behavioral proof.
- [ ] Finish stale path/command and API cleanup. Historical `v2/` references may
  remain as provenance; active instructions must use the promoted root.
- [ ] Resolve the original source-size target with readable, behavior-preserving
  changes. Current non-test Rust count is 58,493 versus the historical 24,000
  target; no exception was agreed.
- [ ] Close the original acceptance inventory only with supporting evidence and
  update PLAN accordingly. The post-v4 LSP remains a separate unassigned roadmap.

All implementation workers were Luna, with Sol reviewing and integrating. No
further implementation is running. Worker branches and WIP are preserved for a
future continuation; nothing was silently merged to make this list look finished.
