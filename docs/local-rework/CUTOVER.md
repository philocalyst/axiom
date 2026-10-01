# Local v4 cutover

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

The specification is `v2/LANGUAGE.md` and `v2/DESIGN.md`. The recovered migration
checklist is `v2/REMAINING.md` as preserved at the starting commit. Its historical
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

All remaining acceptance rows are pending until verified. This document is an
active work record, not a completion claim.

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
