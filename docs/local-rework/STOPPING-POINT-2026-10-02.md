# Local stopping point, 2026-10-02

The user asked to stop, write down what remains and close this session. Work is
checkpointed in `axiom-local` on `cutover/promote-workspace`. The verified source
commit is `1be566d4b1ddfbde419076dddb65ffc1e8f98a7c`; the subsequent documentation
commit records this handoff. This is a compiling development checkpoint with
known correctness failures, not a completed native migration.

Sol coordinated and reviewed six `gpt-6-luna` implementation lanes using xhigh
reasoning. All workers have stopped. No push, publication or backend change was
made. The original Downloads repository and the September 30 recovery evidence
were left unchanged. No denied remote-history or native-UI access was retried.

## What landed

The native workspace formerly under `v2/` is now the sole root Cargo workspace.
`86ac2b0` promoted it, `07c8a8e` removed the old root implementation, and
`3294182` removed `v2-previous`. Removal inventories are preserved outside the
repository in `verification/cutover/`. Git retains the removed source.

Reviewed integrations include native declarations and journal lowering, pooled
transaction metadata and borrowed flow access, typed purposes and owner scopes,
asset and budget histories, typed report cells and CLI JSON, compiled sync
declarations, and native financial example ports. Later corrections include:

| Commits | Result |
| --- | --- |
| `7ac30ae`, `34d9fc5`, `e95abe5` | Sparse written-occurrence and temporal indexes; owner currency parameter keys |
| `25bd4d3`, `8ba80f4`, `a0b1f1a`, `1a357e7` | Part-specific asset sampling, residence query compilation, temporal cursors and post-mutation sampling |
| `d590125`, `dcdb851`, `7e1985d` | Typed whole-transaction claim changes and deposits; validated default deposit holding |
| `338acd9`, `542c6e5`, `91d2238` | Idle-owner budget rows, typed purposes, explicit unpriced aggregates, future-window note and exact asset consumption dates |
| `42f003d` | Prior-year California payment classification and prevention of a second payroll-benefit deduction |
| `3f02d79`, `3e09afe` | Typed owner-aware conversion API and written-quote precedence |
| `15bdc18`, `30ada83`, `65ae13f` | Stack-only purpose inference, source-located conflicts and compatible ancestor/descendant refinements |
| `76d787e` | Removed the unused `Book.plans` and journal-plan route |
| `fc58a78` | BasisZero HSA funding, linked reimbursement purpose and atomic transfer sampling; prorata compatibility remains failing |
| `1be566d` | Kept contract occurrences materialize and post once, in source order; absent optional inputs omit only their dependent items |

The last slice integrates Luna commits `7859e7a`, `1918628` and `059f793`.
`059f793` supersedes the temporary all-or-nothing missing-input guard: LANGUAGE
says to omit the item that reads an absent input. A focused payroll regression
posts 6,173 USD once. Actual full examples still expose template classification
and recognition failures; that regression alone does not establish corpus parity.

The complete new integration sequence since the previous bounded green baseline
is [COMMIT-SEQUENCE.tsv](COMMIT-SEQUENCE.tsv). Original requirement provenance is
in [SCOPE.md](SCOPE.md) and [recovered-prompts.json](recovered-prompts.json).
The current checklist is [REMAINING.md](../../REMAINING.md). Its full historical
inventory is retained in [the archived checklist](history/REMAINING-2026-09-30.md).

## Final verification

Commands ran on the connected Mac with Rust 1.98.1, locked offline dependencies
and `CARGO_INCREMENTAL=0`. Evidence is under
`../verification/cutover/` relative to the repository root.

| Check | Result | Artifact |
| --- | --- | --- |
| `cargo check --workspace --locked --offline` | Pass, existing warnings remain | `stopping-workspace-check.log` |
| `cargo test --release --workspace --no-fail-fast --locked --offline` | **734 passed, 4 failed, 8 ignored** | `stopping-workspace-tests.log`, `stopping-workspace-tests.summary.json` |
| `cargo fmt --all -- --check` | Fails: diffs in 117 files | `stopping-format-check.log`, `stopping-format-check.summary.json` |
| `git diff --cached --check` before the source commit | Pass | Source checkpoint commit |
| Independent `verify04.py`, `verify05.py`, `verify07.py` | All exit 0 with retained financial expectations | `stopping-verify04.stdout`, `stopping-verify05.stdout`, `stopping-verify07.stdout` |
| Actual family CLI check, closing date 2026-04-15 | Fails: 141 errors, 3 notes | `stopping-05-family-check.stdout` |
| Actual landlord CLI check, 2025-12-31 | Fails: 44 errors | `stopping-07-landlord-check.stdout` |

`stopping-financial-probes.json` records source commit, binary SHA-256, exact
commands, exit codes, diagnostic counts and current family tax rows. The source
tree tested before the commit is exactly the committed product tree. No product
changes followed it. Clippy, a fresh complete golden/mistake regrade, and current
native benchmark acceptance were not completed at this stopping point.

The four test failures retain their assertions:

| Test | Observed failure | Next investigation |
| --- | --- | --- |
| Engine `temporal_days_count_an_inclusive_residence_before_the_first_flow` | No check fires; expected one inclusive residence check | Start relevant yearly schedules from residence facts in a book without flows |
| Engine `a_prorata_place_realizes_only_the_lots_share_and_deferrals_merge_into_one_lot` | Plain basis is 0; expected 5,188.24 | Reconcile the new BasisZero funding rule with after-tax prorata contributions. The old fixture lacks explicit basis; native IRA docs require basis for nondeductible contributions. Resolve that deliberately and retain the numerical regression |
| Report `a_context_forecast_keeps_historical_and_same_day_obligations_once` | Historical fee and same-day fee are present; 10 USD year-end tax is absent | Resume the closing prefix and emit its obligation once |
| Report `native_loan_forecast_stops_after_the_typed_principal_is_repaid` | Five 1,000 USD payments instead of three | Consume canonical loan principal and cap/stop occurrences after repayment |

These failures are independent of the unintegrated worker checkpoints below.
The BasisZero/prorata failure surfaced after the HSA rule changed; it must not be
hidden by claiming that the focused HSA test closes all basis behavior.

## Financial and corpus work still required

Current family errors comprise **128 purpose disagreements and 13 assertions**.
For example, an employer-to-health-insurer contract item has both wage and
insurance endpoint classifications. Classification conflicts reject source
movements and make downstream balances and taxes unreliable. The current tax
command exits 1: AGI 223,668.93, SALT 18,131.60 and itemized deductions 46,108.92
are not accepted outputs. Earlier closer tax figures in the progress log refer
to an older checkpoint and are superseded by this probe.

Current landlord errors comprise **3 purpose disagreements, 12 occurrence-date
errors and 29 assertions**. Payroll withholding legs conflict with employer
purpose, and occurrence recognition still rejects journal dates. December
checking is 99,790.60 against the retained 173,866.60 assertion. The source payroll
unit regression passing does not fix those full-book conditions.

Preserve the independent financial expectations while repairing the actual
source/runtime path. In particular:

- Complete Source 05 classification, HSA reimbursement and closed tax parity.
- Complete Source 06 wash/carry and Source 07 loan, deposit and roof-improvement
  basis proof. The earlier Source 07 gap was the 14,200 USD invoice allocation:
  retained roof basis contribution 14,070.91 must reach the asset. Recheck these
  values on the next runtime checkpoint rather than treating older output as current.
- The retained Source 08 WIP compiles and checks privately, but wages and interest
  are 2 cents below its original expectations. FBAR peaks are 40,801.89 instead of
  38,927.16 because gross payroll temporarily inflates the bank balance before
  deductions. A direct employer split meets current purpose conflicts. Net worth
  remains 113,626.85. Do not lower the oracle or invent clearing events to hide this.
- Finish Source 08–11 native runtime financial proof, grade 01–03 and every other
  example, map the 73 original behavior rows to real native assertions, and rerun
  all 60 goldens and 125 mistakes with justified output changes.
- Correct golden-runner error masking and the nonportable mistake-runner timeout;
  update the native generator and reject performance results from incorrect folds.

The old green `134f2a3c` checkpoint had 432 passing tests, two ignored tests and
byte-identical goldens/mistakes. It is retained on
`rework/recovered-client-boundaries`; those results do not certify this cutover.

## Runtime, API and report work still required

`Run.monitor_complete` remains false. Kept occurrences and their flow/detail
pools are recorded, but a complete canonical monitor still needs persistence,
checkpoint/hash identity, expected/missed occurrences, open claims, settlement
ordering, deposits/refunds, loan state, measures and ending events. ClaimChange
is scheduled but its engine handler remains a no-op. The richer parser checkpoint
must be integrated with a real consumer before claiming credit/waiver support.

Finish strict code → exact → oldest claim settlement, filtered write-offs,
credit/due amendments and claim dates, and specify/implement waiver recoveries
and computed credit diagnostics. Close pro-rata ending/refund behavior and
explicit early deposit refund semantics without fabricating cash movements.

LoanState needs the **nominal cadence** convention used by Source 07 and Sam,
with actual scheduled-window fractions for dated resets and prepayments. The
retained worker's ACT/365 daily convention misses these oracles. Source 07's
first monthly interest is 1,569.38 at 6.75%/12 despite its December origination.
Sam's 320,000 USD, 5.875%, 360-month payment is 1,892.92; after 22 payments its
January 2026 interest/principal/balance are 1,529.66 / 363.26 / 312,077.86.

Owner conversion APIs are integrated, but the corrected per-Machine conversion
memo and explicit policy consumer remain unintegrated and need root tests. Cache
immutable rate facts, not amount-specific overflow or zero identities. Complete
runtime policy evidence for every tally and check quote/residence precedence.

Norm repair still takes only the first `else` reparation. Complete chained
repairs, paid/deadline behavior, filed-return contributors and amendment
diagnostics. Audit remaining v3 property/source/law seams. Complete report view
parity and canonical obligations/loans; structural report arena work is pending.
Existing sync tests pass, but native Book/Run binding, shared claim/contract
reconciliation and full declared-source acceptance still need closure evidence.

## Memory, data structures and parallelism

Some bounded improvements are measured, but no tenfold whole-program gain is
established. Sparse budget history on a 1M-flow, 100-flow/day workload retained
401,408 bytes versus 24,000,000 bytes (59.8× less); its isolated query improved
from 50.416 ms to 17.125 µs. A one-flow/day distribution adds about 7% retained
capacity, so the gain depends on density. Calendar enumeration removes 1.3M
allocation calls in its 100k-case probe, while wall time does not improve.
Two-pass Groups construction reduces requested bytes by about 4×. These are
operation measurements, not process RSS or full financial acceptance.

The main unresolved structural work is:

- Replace per-chunk `Vec<R>`, chunk vectors and the unbounded ordered-result
  backlog in `core::par` with safe bounded work/output storage. Bound in-flight
  results, preserve source order and panic behavior, and prevent nested CLI/parser
  parallel loops from creating a quadratic number of threads. This was not started.
- Use flat section/cell arenas and borrowed report row views where measured useful;
  avoid per-row heap containers and duplicated metadata. The implementation was
  not started. Cell payloads may still dominate memory after row-header savings.
- Address repeated occurrence counting from contract start, which can make a long
  schedule quadratic. Complete runtime state first, then measure indexed cursors.
- Validate same checksums and financial results on one huge input, many balanced
  inputs and mixed huge/tiny inputs. Record allocations, retained bytes, threads,
  wall time and peak RSS against the retained baseline.

The generated native 1M-fold probe produced 2,505 errors; its phase allocation
figures are diagnostic, not performance acceptance. The reported model allocation
reduction cannot establish a correct whole-program gain. Process RSS measurement
was unavailable in that probe. The current non-test source count is **58,493**
(`stopping-loc.txt`); the historical 24,000-line target is unmet. No exception was
agreed. Keep readable Rust, borrowed immutable state and safe ownership; do not
remove behavior or compress code merely to meet a count.

## Preserved worker work

Stopped worker worktrees now live under `../verification/retired-worktrees/`.
Moves preserved every HEAD and the SHA-256 of each dirty patch. The manifest,
status, binary patches and untracked-file snapshots are under
`../verification/cutover/stopping-workers/`; move records are in
`stopping-worktree-moves.json`. Branches remain local and can be inspected without
blindly merging their different baselines.

| Branch / archived directory suffix | Head / retained change | Review state |
| --- | --- | --- |
| `cutover/client-report-root` / `axiom-cutover-client-report-root` | `fe84326`; LoanState at `60328c5` | Fixture changes integrated; loan convention fails independent oracles, do not integrate unchanged |
| `cutover/context-native` / `axiom-cutover-context-native` | `499a16b` plus **19 dirty Source 08 files**, 65,199-byte patch | Earlier Source 05 slice integrated; Source 08 WIP is incomplete and preserved |
| `cutover/engine` / `axiom-cutover-engine` | `ee14432` corrected rate memo and explicit `value(... at POLICY)` | Model library 62/62 passed privately; engine test blocked by stale lane dependencies, unintegrated |
| `cutover/engine-current` / `axiom-cutover-engine-current` | `059f793` | Reviewed occurrence slice integrated as `1be566d`; full monitor remains pending |
| `cutover/journal-current` / `axiom-cutover-journal-current` | `80a99b4` typed filtered waivers, due changes and literal credits | 53 native records and six focused claim tests passed privately; runtime absent, unintegrated |
| `cutover/journal-native` / `axiom-cutover-journal-native` | `804eead` plus one dirty `declare.rs` change | Older dependency lane; patch preserved, not an extra verified integration |
| `cutover/sync-native` / `axiom-cutover-sync-native` | `e241cf7`; claim parcel ordinals/write-off helper `86e29ba` | HSA/temporal slices integrated; strict linked-purpose correction and claim helper unintegrated |

Three earlier retired worktrees also retain dirty historical work. Their patches
and untracked files were included in the final snapshot. Nothing was force-deleted
to make the workspace appear clean. The active repository itself is committed;
build outputs and evidence remain available for reproduction.

## Suggested resumption order

1. Read current `LANGUAGE.md`, `DESIGN.md`, `REMAINING.md` and this handoff.
   Repair full-book purpose classification/occurrence recognition, then the four
   preserved regressions. Re-run actual Source 05/07 output, not only their oracles.
2. Review the retained parser/claim helpers and loan/FX work against the promoted
   root. Integrate in bounded slices with canonical state and meaningful tests.
3. Complete the runtime monitor, norms/returns and all financial/corpus acceptance.
4. Measure and implement borrowed structures and bounded parallelism on correct
   inputs; finish formatting, source/API cleanup and the acceptance matrix.

The missing H16002 source and diff payloads are still an evidence gap. All of this
work is new local implementation guided by surviving messages. The post-v4 LSP
roadmap remains a separate, unassigned future item. The task stops here because
the user requested a checkpoint, not because the remaining requirements passed.
