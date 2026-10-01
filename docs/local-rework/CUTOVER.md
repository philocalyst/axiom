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

The six old completed implementation worktrees were clean before removal. Their
branches and commits remain available; exact paths and heads are in
`verification/cutover/removed-worktrees.json` outside the repository. Current
cutover lanes use private worktrees. Cleanup has not touched original repositories
or recovery evidence.

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
