# Local v4 cutover

> Stopping checkpoint, 2026-10-02: product commit `1be566d`. Compilation passes;
> the release suite has 734 passed, four failed and eight ignored. Full family
> and landlord checks still fail. See [the current handoff](STOPPING-POINT-2026-10-02.md)
> and [current remaining work](../../REMAINING.md). Dated entries below retain
> their original observations and are superseded by the final checks.

This is new implementation guided by the recovered requirements. It does not
restore the lost H16002 patches. The original checkout, recovery objects and
cached evidence remain unchanged.

The completed bounded rework is preserved on `rework/recovered-client-boundaries`
at `134f2a3c58b27d90a1fb4de0f0a3b0beb340615f`. This continuation starts there on
`cutover/promote-workspace` in the isolated `axiom-local` repository.

## Requirements and provenance

The current request authorizes finishing the cutover, removing legacy source,
promoting `v2/`, cleaning the local workspace and API, completing the migration
requirements in `REMAINING.md`, and investigating structural allocation and
memory improvements. The earlier request specifies Luna implementation workers
with Sol orchestration and review. Six existing Luna workers continue in separate
local worktrees; their work is reviewed and integrated incrementally.

The specification is the promoted root `LANGUAGE.md` and `DESIGN.md`. The recovered migration
checklist was `v2/REMAINING.md` at the starting commit; its original inventory is
preserved in [the historical checklist](history/REMAINING-2026-09-30.md). Its historical
branch descriptions and performance numbers are evidence of earlier work, not
verification of this implementation. Its referenced lane briefs and audits supply
acceptance details. The cached prompt inventory and source proofs are recorded in
`SCOPE.md` and `recovered-prompts.json`.

| Scope | Explicit source | Acceptance evidence required |
|---|---|---|
| Native v4 parser and formatter | REMAINING §§3, 4.2, 4.10; LANGUAGE | Syntax tests, sketch parsing, malformed input, formatting idempotence |
| Declarations, names, dimensions, norms, formats and patterns | REMAINING §4.3; M4a brief | Declarations build, refusal table, shared compiled sync types |
| Journal, references, contracts, claims, assets and ownership | REMAINING §4.4; M4b brief | Native fixtures for each verb and contract mode, independent sketch arithmetic |
| Purpose, budget, asset and promise execution | REMAINING §4.5; E4b brief | Populated Run state and meaningful end-to-end fixtures |
| Conversion, norms, filed returns and diagnostics | REMAINING §4.6; E4c brief | Unit evidence, repaired chains, return amendments, regraded diagnostics |
| Reports, typed rendering and CLI | REMAINING §4.2; R4 brief | Every view, strict JSON, owner scope, native forecasts, real fmt |
| Sync runtime and Book/Run binding | REMAINING §§4.1, 4.7; SY brief | Shared declarations, reconciliation, dry diff, idempotence, grouped unknown memos |
| Systems, examples and refusal corpus | REMAINING §4.8; Y4 brief | All examples, verify scripts, corrected 11-sam figures, justified golden changes |
| Legacy removal and root promotion | Current request; REMAINING §4.9 | Single root workspace, no v3 bridges or previous implementation tree |
| API and memory structure | Current request; REMAINING §§4.4, 4.10, 6, 7 | Compact records, shared metadata, borrowed views, allocation counts and measured RSS/time |
| Final closure | Current request; REMAINING §§4.9, 5, 9 | Release suite, corpus, performance, size, current PLAN and honest completion status |

## Implementation and review order

1. Integrate the recovered S5 syntax as a reviewed dependency.
2. Settle model public types and borrowed accessors before engine/report/sync ports.
3. Integrate native declarations and journal lowering, then engine semantics and
   consumers with focused native tests.
4. Run the migrated systems, examples and independent arithmetic checks, then
   regrade the refusal corpus and regenerate goldens with explanations.
5. Promote the verified workspace and remove superseded source in this isolated
   repository. Update paths and run all checks from the root.
6. Measure the new structure against retained local baselines and record actual
   gains and limits. Complete REMAINING and PLAN from verification results.

Promotion waits for the implementation dependency chain so active patches retain
consistent paths. Original repositories and recovery evidence are outside the
cleanup scope. No push, publication or backend change is part of this work.

## Measurement limits

The CLI already dropped the syntax tree after model building at the starting
commit. That existing behavior is not a new memory improvement. Candidate gains
are build-time peak memory, per-flow allocations and metadata duplication,
immutable shared pools, sparse fold state and borrowed report/sync data.

The request for order-of-magnitude improvements is a performance goal. Allocation
counts, retained bytes, structure sizes, wall time and peak RSS must establish
which operations improve and by how much; a local structural win does not imply
a tenfold reduction in whole-process memory.

The historical 24,000-line cap and the separately listed post-v4 LSP roadmap have
been presented as optional scope preferences. Until clarified, the migration is
implemented comprehensively, with readability and the cap treated as a target;
the LSP remains a separately identified roadmap item. Neither is represented as
an agreed exception or as completed work.

## Integration record

| Integration commit | Change | Verification |
|---|---|---|
| `df76d70` | Promote recovered S5 v4 parser/AST; reject legacy chart account blocks; document raw run/into comments | Lane and coordinator: 81 syntax tests passed; one pre-existing ignored doctest |
| `1b337eb`, `1320743` | Bound packed piece indices, tighten parser contracts and fold parallel results in source order | Coordinator: 84 syntax tests passed; one ignored doctest. Allocation candidate measured in PARSER-EXPERIMENTS; ordinary release timing gate remains pending |
| `1c54f5c`, `b613f3f` | Pooled journal metadata, borrowed flow views, runtime detail association, canonical sync types and M4a/M4b handoff | Coordinator: 50 core and 84 syntax tests passed. Full model build remains red in the old v3 lowering pipeline |
| `67c2302` | Reject absent parameter key tuples and dates before their first row | Reviewed arithmetic correction and regression cases; model tests await native lowering |
| `914f4aa` | Flow-owner purpose rules, dated properties, asset scopes and sparse rolling totals | Lane: 124 engine tests passed before adopting the schema dependency; native integrated engine checks pending |
| `1a2c09f`, `e9cf758` | Sam native source fixture and independent financial oracle with source-derived inputs | Lane: all 12 Axiom files parse, 144 items, no diagnostics. Coordinator: verify11 passes. Engine agreement remains pending |
| `c37881b`, `9e9345a`, `046c068` | Native settings/scopes, forward kind hierarchy and dimensional parameter lowering | Reviewed source checkpoints. Model tests remain blocked by old build wiring and contract API migration |
| `ba75dde`, `86fd5d8` | Cumulative depreciation, borrowed pooled flow metadata, exact partial-lot quantities and transferred lot codes | Reviewed source and regression cases. Integrated engine tests await the native model build |
| `983277a`, `25e112a`, `fb2af99` | Copyable text handles backed by borrowed source or a book-owned decoded string arena; explicit uninstantiated template identity | Reviewed source. Consumer lifetime migration and native template execution still pending |
| `7c12884` | Custom property declarations, dimensional values, shared kind defaults and layered dated overrides | Reviewed source and focused regression cases. Native build wiring and integrated tests pending; review corrections remain assigned |
| `340ee96` | Correct Sam's cumulative depreciation oracle from source dates | Coordinator: independent Python oracle passes. Engine agreement still pending |
| `0816097`, `3c30b35` | Scoped native report metadata, explicit register targets, grouped contract descriptions and owner-scoped source queries | Reviewed source. Book-borrow lifetime migration and native report tests pending |
| `6a80014`, `35399af`, `f430513`, `3a50d56` | Canonical sync declaration lowering, readers, typed original-currency captures and bounded matching | Reviewed source. Text adaptation, remaining reader/output corrections, Book/Run monitor binding and integrated tests pending |
| `2dfe40a`, `be3a4d6` | Native declaration trees, ownership shares, surveyed claim tabs, place-first endpoints; build now reaches custom properties, parameters and contracts | Reviewed source. Workspace check still fails in legacy model adapters; native law and journal execution are not yet wired |
| `e4bc7db`, `6932fb9` | Pooled implied-flow metadata and explicit journal versus contract-occurrence runtime identity | Reviewed source. Engine propagation and integrated model tests pending |
| `88c5dcc`, `2b21863`, `d918dae` | One forecast-history allocation with stable category indices | Coordinator: actual integrated bands module passes all five release tests, including every sample against the previous nested-row sampling loop; full report tests still blocked by model |
| `563a3a6`, `f98abeb` | Native declaration laws, forward override metadata, contract governance and typed S5 expressions/effects | Reviewed compiler checkpoints. Coordinator workspace check reaches 89 model migration errors; native builder wiring and execution remain pending. Dynamic amount typing is being tightened to the normative unit rules |
| `a511cd2` | Iterative memoized effective ownership, with cycle and overflow diagnostics | Reviewed source and nested-share/cycle regression cases; integrated engine tests await the native model |
| `dab2b66`, `01bc510`, `da1f56d` | Named sync selection, dry option and auxiliary reader sources held separately from borrowed parsed input | Reviewed API and borrow-lifetime regression case; CLI tests await model and sync adapter completion |
| `b2f2e4b` | Validate generated sync items with a full date before emitting the destination's short date | Coordinator: actual integrated core/syntax, CSV, tagged, amount, memo-cell and writer modules pass 28 release tests; three timing tests ignored. Both heading-dependent date regressions pass |
| `c1d1eb0`, `0f17b1b`, `7d28acf` | Strict dimensional inference, owner currency, typed selector keys and lexicographic law specificity | Reviewed compiler checkpoints; runtime and native corpus evidence remains pending |
| `d822b11`, `8a59c0c` | Confine local source paths, borrow mutable read callbacks and join split CAMT text | Coordinator: path and primitive suite reached 31 passing tests; the next exact-source review run found an indentation-dependent nested CAMT memo failure (32 passed, one failed, three timing tests ignored). The correction is assigned; this row is not green |
| `6bb370a`, `2b4bd5e`, `d90f913`, `686311b` | Native property defaults, borrowed endpoint survey, deterministic implicit parties and asset-first endpoint refusal | Reviewed source and regression cases. Native model execution evidence awaits journal wiring |
| `67ee82a`, `01476f5` | Typed runtime transaction provenance, dimensional evaluation, conserved owner allocations and borrowed promise/claim ranges | Reviewed source and regression cases. Runtime pools remain empty until native monitor population; engine tests have not passed on the integrated native model |
| `5e611a4` | Remove 2,831 lines of obsolete S3 collection, flow and folder-date lowering; wire native law and sync declarations | Coordinator: workspace check now reaches seven model errors (three old forecast adapter errors, two contract root-shape errors, two sync lowering errors). Core and syntax pass 134 release tests, with one pre-existing ignored doctest |
| `63f744e` | Distinguish calendar-year selector keys from numeric expressions | Lane: 85 syntax tests pass. Coordinator's rerun on the integrated revision is pending |

The six old completed implementation worktrees were clean before removal. Their
branches and commits remain available; exact paths and heads are in
`verification/cutover/removed-worktrees.json` outside the repository. Current
cutover lanes use private worktrees. Cleanup has not touched original repositories
or recovery evidence.

The obsolete ignored v1 build directory in this isolated checkout was removed
after checking its exact path and confirming it was not a symlink: 610 files,
222,276,721 bytes. The removal inventory is
`verification/cutover/removed-build-artifacts.json` outside the repository. The
active native build directory and all source/evidence repositories were retained.

Acceptance rows close only after verification. This document is an active work
record, not a completion claim; later dated checkpoints supersede earlier
compiler and test observations.

## Allocation baseline

The coordinator's isolated System-allocator probe records each phase separately;
its source and logs are under `verification/cutover/` outside this repository.
The probe counts successful allocation/reallocation calls, requested bytes,
retained live bytes and peak live bytes. Instrumented timing is not a production
performance measurement. Input text is loaded before each phase snapshot.

At the preserved `134f2a3` baseline, the generated 1M-flow project used 64,785,844
input bytes across 169 sources. Parsing recorded 7,147 allocation calls and
193,195,392 requested bytes. Model building recorded 52,785 calls and
915,523,757 requested bytes, with peak live bytes of 770,481,965. Dropping the AST
left 472,825,856 live bytes. Planning recorded 6,862 calls; folding recorded
1,628 calls. The baseline Flow header was 200 bytes, Txn 88 and syntax Item 40.

These are baseline observations, not gains. A native run on the same generated
financial workload, with correctness verified, is still required to compare
allocation counts, retained memory and whole-process RSS/time.

## Native integration checks, 2026-10-01

The journal builder is now wired after native declarations, contracts and law
registration. The obsolete Book/Contract forecast adapter has been deleted;
forecast consumers must use the engine's canonical occurrence materializer.
Typed law and template nodes use `Arena<Node>` with `Option<Ty>`: an invalid
node is represented explicitly instead of by a second parallel poison vector.

| Commit | Reviewed change | Actual verification |
|---|---|---|
| `44070b1` | Preserve nested CAMT memo text across indentation, reuse decoded memo storage and fix sync declaration lifetimes | Exact production primitive modules: 34 release tests passed, three timing tests ignored, including the independently discovered nested-memo regression |
| `ac23af5` | Remove the obsolete Book/Contract forecast adapter | Source review; replacement engine materialization remains pending |
| `f2f983e`, `5ecf04f`, `b2918f0` | Typed node arenas, explicit invalid types, selector-year roles and engine facts adaptation | Integrated core/syntax: 135 release tests passed; one pre-existing ignored syntax doctest |
| `e7d533a`, `00c0a73` | Native journal lowering and builder wiring | Actual model library compiles; workspace consumers still require migration |
| `3ddf356` | Intern native journal codes directly and share Also-tail metadata lowering | Actual native model integration tests: three passed. Household and Sam inventory no longer panics |
| `3090f71`, `1633263` | Native diagnostics and caret codes; diagnostics borrow their Plan and bound contributor selection to three entries | Source/API review; integrated engine execution remains pending |
| `48a770b` | Distinguish purpose caps from place-direction caps | Source review; dynamic dated budget execution and report consumption remain pending |
| `9d2138d`, `e094144` | Native engine integration, explicit monitor-completeness state and unit-safe cap shortcut | Actual engine library compiled before the new budget function dependency. Independent native transfer and inferred-amount fixtures both produce checking 75.00 and savings 25.00 USD. Full engine tests still require migration; earlier source-only success is not a full-suite claim |
| `e68ee88`, `64281c4`, `0aea933`, `02128d9`, `9175444` | Native touching index, sparse computed assertion programs, computed dated terms, filed tally records and settlement events | Coordinator: 136 core/syntax release tests passed, one pre-existing ignored syntax doctest; six native-record integration tests passed |
| `b039051` | Cross-namespace declaration collision and jointly resolved endpoint ambiguity diagnostics | Coordinator: three exact-source names regressions passed against the actual model; package test migration remains pending |
| `eca9704`, `4e401b6`, `b840ce1` | One source registry for native sync planning, staged account deltas, exact source-offset diagnostics and native runtime API adaptation | Lane: actual sync library compiles on this dependency state. Coordinator: exact production primitive suite compiles, but two of 42 tests fail because target text is read twice; correction assigned. This is not a green sync-suite result |
| `300ee6f` | Evaluate computed assertions at reconciliation and exclude zero placeholders from inference | Focused lane tests pass. Review requires preserving the asset subject and strengthening the successful-evaluation assertion; pending |
| `83d030a` | Native declaration Also, budget/law links, computed limits and scoped law override resolution | Coordinator: three native-law integration tests passed. Review corrections for invalid Also tails, deterministic IDs and fully dated budget terms remain pending. Workspace check currently fails at the engine's unimplemented BudgetLimit arm |
| `28a9079` | Focused native model test API adaptation | Test-only review; no package green claim. A proposed wholesale replacement still has 73 behavior groups pending/partial and is not accepted as completed migration |

The independent inventory builds the actual native model and shipped systems;
it contains no substitute financial engine. After code interning was corrected,
Sam builds 22 flows, 24 transactions, ten contracts and 22 laws, while reporting
135 errors. Those errors include remaining unsupported statement verbs,
contract date/share handling and older system declarations. These are real
remaining requirements, not acceptable completion results. The logs are under
`verification/cutover/native-model-inventory-interning-fixed.log` outside the
repository.

The integrated Flow header is 192 bytes, compared with 200 at the preserved
baseline; Txn remains 88 bytes and syntax Item 40. This four-percent header
reduction is a measured structural observation, not an order-of-magnitude
whole-process memory claim. Larger candidates being implemented are staged
sync deltas instead of copying all account history, a source registry that
borrows cached text, canonical owner-aware views and bounded provenance scans.

## Current integration checkpoint, 2026-10-01 11:46 UTC

The Mac executor briefly disconnected, then recovered on the same host. Work
resumed from the persisted branches. Root promotion and full verification have
not happened. No recovery source was changed, and neither original checkout has
uncommitted changes.

| Commits | Reviewed change | Coordinator verification |
|---|---|---|
| `bd46f33` | Read a sync target once; preserve explicit monitor-completeness state | Exact production sync primitive harness: 42 tests passed, three timing tests ignored. Full native planner acceptance remains pending |
| `21b7bf9`, `7e69057` | Remove obsolete model name resolution and PathRoot/v3_root adapters | Model package compiled and tested on the integrated native dependency chain |
| `013afcb`, `5ea30b4` | Unique earlier transaction references, then chronological code index; whole-asset opening and contract waiver/end timelines | Actual native-record integration suite: ten release tests passed. Waiver/end clause review corrections and runtime asset disposal remain pending |
| `b574f8f`, `45a530f` | Inventory legacy model behaviors and add native declaration fixtures | Package at `45a530f`: 63 release tests passed. Many behavior rows remain pending or partial; deletion of stale tests does not close those rows |
| `8783fb3`, `95b0703`, `61effa8`, `d39ae3d` | Plan-backed report views, canonical runtime-flow APIs and owner-scoped available/forecast calculations | Lane report library compiles on its checkpoint. Native report fixture migration, future occurrence materialization and independent owner-scoped financial checks remain pending |
| `e504307`, `e4a3d07` | Local read-only memo suggestions and grouped JSON check hints | API/source review. Integrated CLI tests await consumer and fixture migration |
| `9f4a2cc`, `19591e7` | Typed asset-part state, checked cost/basis adjustments, indexed identity and temporal boundaries | Lane exact-source asset harness: eight tests passed. Asset table is not yet wired into the canonical ledger fold |
| `cc872c7`, `4f80a4c`, `8dc8245` | Fully dated budget terms, inherited omitted settings, deterministic IDs and atomic refusal of malformed initial budgets/Also lines | Package before final refusal additions: 66 release tests passed. Lane final native-law suite: six passed. Runtime budget migration is the current workspace compile blocker |
| `4c2f893`, `aee7de8` | Build stable grouped tables with two borrowed passes and no pair/order copies | Coordinator: core/syntax 136 release tests passed, one pre-existing ignored doctest; integrated benchmark verifies every bucket against the previous builder |

The integrated grouped-table benchmark used 100,000 values/2,000 keys and
1,000,000 values/20,000 keys. It reduced allocation calls from four to two.
Requested and peak additional bytes fell from 1,608,004 to 408,004 at the smaller
scale, and from 16,080,004 to 4,080,004 at the larger scale. Retained result bytes
are unchanged. This run measured 369.034 to 191.965 microseconds per small build
and 4.061597 to 2.173166 milliseconds per large build (about 1.9 times faster).
These are operation-level measurements, not a whole-process memory or latency
claim. Production callers borrow staging vectors; cloning an owned Vec iterator
would reintroduce the copy and is avoided.

Logs are saved outside the repository under `verification/cutover/`, including
`groups-integrated-bench.log`, `native-core-syntax-groups-tests.log`,
`native-model-integrated-budget-tests.log` and
`native-workspace-consumer-check.log`. The last workspace check stops at three
engine errors caused by the new BudgetTerms/BudgetTotal dependency. Financial
monitoring, shared occurrence/journal expression evaluation, part-aware assets,
native standard systems and corpus migration still need execution evidence.
The source count at `8dc8245` was 48,420 non-test lines; the historical 24,000-line
target is not met. At this checkpoint, legacy root source and `v2-previous/`
remained; the latter was removed at the later cleanup checkpoint below.

## Dependency verification, 2026-10-01 12:20 UTC

`bcdf414` integrates dated carrying-budget execution, claim/opening-claim and
asset-basis lowering, source-located ending events, the minimal surveyed tab
lookup, native standard/US systems, the paired sync staging buffer and asset
state in checkpoint digests. `8b35933` disambiguates example account/party and
contract/purpose names without changing amounts.

The coordinator's actual model/system run passed 56 model unit tests, seven
native-law tests, fourteen native-record tests and the embedded-system parser
test (78 total). The actual sync library passed 80 release tests with four
timing tests ignored, including separate run-source output and registered
diagnostic provenance. Package-level sync verification remains blocked by 38
compile errors in the seven legacy end-to-end fixtures. Seventeen meaningful
world behavior tests removed by an interim fixture migration are being ported
back; the interim one-test replacement is not accepted as coverage closure.

The workspace library check now reaches the report and finds seven consumer
errors in asset-part and BudgetTerms rendering. The report worker has a matching
consumer correction under verification. Engine package tests currently stop at
one missing `Book.endings` fixture initializer; the preceding full engine run
had 123 passing tests, 26 failures and one ignored test. Those 26 source fixtures
still need native ports with their financial assertions retained.

The independent model inventory on `bcdf414` reports fourteen errors for
02-household and 44 for Sam. This is a real model build with the shipped systems;
it performs no financial fold. The example name corrections reduce those counts
on the worker's subsequent inventory to thirteen and 36. Remaining model errors
include contract occurrence/change statements, the measured-share area
denominator, literal `@` price forms and commodity payout endpoints. The source
count before name-only corrections is 49,494 non-test lines. No root promotion,
full corpus pass, whole-process performance comparison or checklist closure is
claimed by this checkpoint.

## Native consumer and sync checkpoint, 2026-10-01 12:50 UTC

The integrated tree at `1e6d8ec` includes measured contract-area shares, effective
ownership in report views, dated budget segments with checked carried sums,
successive property finalization without lost rows, the registered-source CLI
sync adapter, native occurrence input bindings, and bounded calendar seeking.
The coordinator's release model/system check passed 57 model unit tests, seven
native-law tests, seventeen native-record tests and one system parser test (82
total). The subsequent core/model check passed 52 core tests and nineteen
native-record tests, including occurrence grace, input ordering and ambiguous
schedule refusal.

The actual full sync package passed 94 library tests and all seven native
end-to-end tests; five explicit timing/stress fixtures remained ignored. The
restored world tests use a parsed Book and an engine Run. Two former behaviors
still await the canonical monitor: invoice-code attribution and due occurrence
variance. One settlement test still seeds a settlement code after binding;
that adapter gap is assigned for correction and is not claimed closed. The
paired record/reading buffer now survives sorting and reconciliation without
unzip copies. A scale test seeds a synthetic million-entry account index on a
native-bound world; its timing measures that runtime pipeline, not a million
source flows parsed and folded.

The report ownership integration test passed on the actual root tree and
verifies cumulative cent conservation across a 60/40 shared account. Review
found that several other views still used the primary `Flow.owner` instead of
the account's effective shares; their correction and further source tests are
in progress. The workspace check now reaches the CLI and reports four adapter
errors. A worker has matching corrections under verification. Report fixture
migration and the canonical future monitor remain incomplete.

The full engine package executes after its fixture constructor corrections:
134 passed, 27 failed, one ignored. Twenty-six failures are legacy source
fixtures or their native semantic port requirements. The additional carrying
budget test expected omitted `carries` to turn carrying off, contradicting
LANGUAGE section 3's inheritance rule. The corrected source oracle retains
carrying in March; no new syntax or reset-by-omission rule is introduced.

Coordinator allocation instrumentation verifies identical ordinary calendar
results over 100,000 schedules: heap calls fall from 1,300,000 to zero and
requested bytes from 10,400,000 to zero. This first instrumented run also finds
an ordinary-case latency regression (36.044 to 76.569 milliseconds), which is
under correction; the allocation result does not justify accepting that
regression. The unbounded-anchor run terminates without heap allocation. Logs
and the before/after source probe are in `verification/cutover/` outside this
repository. These are operation-level measurements, not whole-process RSS.

One additional clean obsolete client checkout was removed without force,
retaining its branch and commit. Two inactive dirty report worktrees were
relocated intact under `verification/retired-worktrees/`, with separate staged
and working patches and a manifest. Source repositories and recovery evidence
remain unchanged. The pre-occurrence source count was 49,986 non-test lines;
the historical size target remains unmet. Root promotion, legacy source
deletion, full financial corpus verification and checklist completion are
still pending.

## Native execution checkpoint, 2026-10-01 13:35 UTC

The coordinator integrated exact written-occurrence identity (`b7d86bc`), the
CLI fixture correction (`bf465f9`), removal of the old Sync session and its
public adapters (`dfc8901`, `3b3ce4e`), and the faithful native benchmark
generator (`151452e`). Workspace checking passed after the Sync API removal.
The full Sync package then passed 94 unit and seven end-to-end tests, with five
explicit timing/stress tests ignored. Pending settlement codes now come from
the native Book and already settled flows are not replayed.

Computed journal execution and its native source fixtures were integrated in
`3d832ea`, `2b84da2`, `77fed62`, `99cf234`, and `052dce3`. The engine now borrows
call-local details instead of allocating one-entry detail arenas. Native sale
and purchase fee items retain their posted spending flow while contributing
their cost to the exchange. The full engine release suite passed 165 tests,
with four failures and one ignored test. The failures are recognition of a
range in its first month, a future year's budget breach, recognition after
applying a future flow to a fork, and the still unwired asset improvement hook.
They are implementation obligations; source tests keep their financial
assertions. The native paid-for split retains one grouped source, header and
leg due dates, and debtor metadata, but its current model-only structure test
does not establish runtime claim behavior.

The core/model release suite at `052dce3` passed 53 core, 57 model unit, seven
native-law and 21 native-record tests. The CLI passed all 54 unit tests; its
integration suite passed eleven and failed three on remaining source fixtures.
A worker reports corrections for those fixtures, awaiting root integration and
rerun. Report unit fixture migration and the canonical materializer/monitor are
still pending. No empty promise/claim vectors are interpreted as monitor
completion.

Commodity issuers now have separate typed identities (`83f6480`): VTI and BND
never share a single fund-kind endpoint. Review also identified a general
income-flow ownership defect: an outside source's default owner can replace
the receiving account's actual owner. Its correction and source-level tests
are assigned before financial verification. Per-owner nonlinear law effects
must be weighted before evaluating the law; scoped report rendering alone is
insufficient.

The calendar correction in `ea674f2` retains zero allocation over 100,000
ordinary schedules, with the same checksum as the old implementation. The
coordinator's allocator-instrumented run measured 38.744 ms before and 40.905 ms
after; this is not a demonstrated wall-time speedup. Seeking from Day::MIN
also terminates with zero heap allocation. A separate structural review found
that carried-budget history reads repeatedly scan all earlier facts; indexed,
same-day aggregated range reads are under implementation with arithmetic and
allocation comparisons required before acceptance.

The native benchmark converter keeps the original deterministic simulation,
daily quotes, lot trades, FX, payroll splits, pending checks, growth, grants,
unknown amounts and balance assertions. A coordinator-generated 10k workload
has 10,009 flows and 9,970 journal lines. Its first actual CLI execution reports
102 errors and six warnings, primarily unposted grouped payroll, inherited
property lookup, recognition and inference. This is useful failure evidence,
not a correctness or performance pass. The original financial assertions are
not waived or removed to manufacture parity.

At this checkpoint the native source count is 51,074 non-test lines, including
18,435 in model, 8,333 in engine and 8,216 in report. The historical 24,000-line
target remains unmet. Native examples, mistakes and goldens still need their
full source and financial migration. Root promotion and legacy removal have
not yet occurred, and REMAINING is not marked complete.


## Reviewed execution and cleanup checkpoint, 2026-10-01 14:31 UTC

At `f30a116`, the coordinator's release engine and CLI checks passed 179 engine,
54 CLI unit and 14 CLI integration tests. Two engine tests are explicitly ignored:
the existing stress test and the manual history measurement. Budget recognition
assertions retain their exact warning counts and financial readings. A fixture
helper now identifies generated budget laws by `Book.budgets` rather than the
old generic law name.

The core/syntax/model/systems/sync check passed 53 core, 86 syntax, 57 model unit,
seven native-law, 28 native-record, one systems-source, 94 sync unit and seven
sync end-to-end tests. Five sync timing tests and one syntax doctest are ignored.
The report check at `72da508` passed 65 tests and failed 31. Native forecast and
monitor wiring, remaining source ports, budget output and several report
behavior corrections remain active requirements. A green engine unit suite does
not establish financial agreement for the full examples or corpus.

`44a24a8` adds typed asset acquisition, improvement and sale hooks. Improvements
add basis parts with separate service clocks while retaining one physical unit
on the acquisition parcel. Focused purchase/improvement/sale and guarded
consume/carry tests pass. Per-part law effects, sale-day depreciation and grouped
capital sale costs still require integration. Review found that a Less item must
retain its gross header and post a reversed item; subtracting both would reduce
cash twice. The canonical occurrence materializer (`3be5072`) is integrated but
has not yet populated the native promise and claim monitor.

`30a065d` preserves landlord sale-cost evidence as a Less item and ends the
landlord's transferred lease at disposal, based on the preserved original plan.
The independent verifier agrees on depreciation 10,178.42 USD, net proceeds
404,531.25 USD, gain 23,659.67 USD, tax 13,015.59 USD and final cash 173,866.60 USD.
It reads native source and imports no Axiom implementation; engine agreement is
still required. The benchmark converter now preserves opening holdings and
qualified tuition separately from other education spending. Benchmark CLI runs
are end-to-end: current Sync folds the engine even with no sync jobs. Their
subtraction is not phase isolation.

The exact history operation probe passes at 100,000 and 1,000,000 facts, checking
both incoming and outgoing sums for every query against the retained linear
scan. The index coalesces same-day facts and uses wide checked block prefixes.

| Facts | Facts/day | Indexed capacity / old capacity | 100 indexed queries / old scans |
|---|---:|---:|---:|
| 100,000 | 1 | 3,211,264 / 2,400,000 bytes | 54.167 µs / 4.275375 ms |
| 100,000 | 100 | 25,088 / 2,400,000 bytes | 10.375 µs / 5.5775 ms |
| 1,000,000 | 1 | 25,690,112 / 24,000,000 bytes | 366.125 µs / 47.83075 ms |
| 1,000,000 | 100 | 401,408 / 24,000,000 bytes | 17.125 µs / 50.416417 ms |

Dense histories use about 60–96 times less index capacity in these cases, while
unique-date histories use more capacity. These are capacity and query-operation
measurements, not whole-process peak memory or ordinary CLI latency. The actual
probe is `native-budget-history-scale.log` outside the repository. The regenerated
native 1M fixture retains 1,000,094 simulated flows and 996,625 simulated journal
lines; opening blocks and native spellings give 1,014,718 physical lines. A correct
fold and financial parity are prerequisites to the whole-workload comparison.

`3294182` removes all 54 tracked files of the unused `v2-previous` implementation
(17,660 lines). Exact file hashes and the preserving commit are in
`verification/cutover/removed-legacy-previous.json`. Legacy root source removal
and native root promotion remain pending. Current implementation workers retain
private working trees, and original repositories and recovery evidence remain
unchanged. This checkpoint does not close REMAINING, the corpus or the size cap.


## Root promotion checkpoint, 2026-10-01 15:37 UTC

`86ac2b0` promotes the native crates, Cargo workspace, language specification,
benchmarks, examples and tests to the repository root. It removes all 82 tracked
files of the older root implementation, including its source, tests, fixtures
and superseded design documents. The preserving commit is `07c8a8e`; exact
hashes and sizes are recorded outside the repository in
`verification/cutover/removed-legacy-root.json`. The 54-file `v2-previous`
implementation was already removed at `3294182`. Original repositories and
recovery evidence remain unchanged. The root README now describes the native
commands, and offline Cargo metadata resolves the promoted workspace.

Private implementation worktrees retain their former paths. Reviewed commits
apply to the promoted paths through Git rename detection; `27e1251` verifies
this with the percentage split materializer. External native probes now resolve
`axiom-local/crates` rather than the removed `axiom-local/v2/crates` path.

`4783128` adds typed percentage split legs, written body overrides, exact
association of late payments with scheduled occurrences, party payees in both
contract directions, and initial loan terms. Its worker native-record check
passed 31 tests. `27e1251` materializes percentages against the gross active
header, so 6% of a 4,600 USD payroll is 276 USD. `a59144e` corrects occurrence
ordinals, escalation of literal amounts, Rest legs before explicit legs,
Carve/Less distinction and same-unit split conservation. `07c8a8e` evaluates
depreciation over the scheduled period and exposes the terminal mid-month
calculation for disposal. Integration checks are running; grouped written
body replacement, native loan execution and the claim monitor remain active.

At `92c1c86` the release model and systems checks passed 57 model unit, seven
native-law, 28 native-record and one systems-source tests. These checks establish
lowering behavior, not full financial agreement for the examples. Root promotion
is complete, while REMAINING closure is still contingent on native execution,
reports and the financial corpus passing their required checks.
