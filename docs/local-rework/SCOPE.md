# Local rework from recovered Axiom requests

This is new work started on 2026-10-01. It does not recreate or claim to restore
the lost h16002 patches. No recent remote source or diff payload survived.

## Evidence and baseline

The exact Axiom task is `01a0f031-c1c8-7a41-8f31-8847241a38dd`, titled
“Clone Axiom repository”, on H16002 at `C:/Users/rmccrar6/Downloads/axiom`.
The recovery report identifies activity from 08:26 to 11:50 UTC on September 30.
The six cached prompts have no timestamps. Their cached array order is preserved
in [recovered-prompts.json](recovered-prompts.json), with an original source path,
SHA-256 and exact JSON pointers. The editor route names
`v2/crates/report/src/forecast.rs`; it proves a viewed path, not edits.

Source evidence remains unchanged under
`/Users/mileswirht/Documents/Codex/2026-09-30/task/recovery/`:

- `evidence/history/recent-remote/remote-thread-ui-cache.json`: exact user text.
- `evidence/history/recent-remote/REPORT.txt` and `key-source-proofs.json`: remote
  identities, activity, missing history and failed supported retrievals.
- `evidence/history/axiom-targeted-report.json`: 484 older completion summaries,
  dated September 21, 22 and 26; all 30 checked original hash objects survive.
  These are historical claims, not requests from the missing remote turn.
- `evidence/axiom/REPORT.md`: divergent preserved branches and baseline tests.

Selected baseline: `12d5d1889fe9bc699a20c32a2056aa571d142d4b`, the exact branch
named in cached prompt 1. It has the coherent v3 toolchain, v4 public types,
engine Plan/Ledger restructure and preserved v4 briefs. Prior recovery tests
reported 399 passed and two ignored. The divergent report, sync and v4 branches
are reference material only. The v4 syntax alternative has 85 recorded model
compilation errors; it is not silently substituted for the coherent baseline.

The integration repository is an independent local clone. The first bounded
rework is preserved on `rework/recovered-client-boundaries`; the native cutover
continues on `cutover/promote-workspace`. Worker worktrees belong to that clone.
The original Downloads repository and recovery repositories, refs, bundles and
evidence are read-only inputs. No backend changes, push or publication are in
scope. Native Codex UI access and previous recovery denials remain respected.

## Explicit requested scope

Pointers below abbreviate `/prompt-history/01a0f031-c1c8-7a41-8f31-8847241a38dd/`.
The later instruction to use Luna for implementation and Sol for orchestration
and review governs this new work; it supersedes prompt 1's older division of work.

| Requirement | Exact provenance | Local implementation/verification target |
|---|---|---|
| Clone the named repository | Prompt 0 | Independent local clone from preserved exact objects |
| Continue from the Claude main and `v2/REMAINING.md` | Prompt 1, quoted branch and brief | Incremental changes on its green baseline; retain migration gaps explicitly |
| Experiment with cleaner, better typed formulations; no code golf | Prompt 1 | Typed model schedules, report context and client/source/rendering APIs |
| Explore fewer allocations, less reference counting and true lock-free algorithms where justified | Prompt 1 | Borrow immutable phase outputs; measure parser/engine changes; no gratuitous unsafe code |
| Deeper data structure optimization and cleaner/faster parsing guided by Carbon | Prompt 1 | Sparse snapshots, precomputed membership and bounded parser experiments with primary sources |
| Remove CLI assumptions so GUI/other clients can build reports | Prompt 2 | Client-neutral context, borrowed source provider, typed reports and renderers |
| Stronger traits and fewer intermediate allocations using borrowing | Prompt 2 | Source/rendering interfaces and reusable static plan/checkpoint |
| Forecasting should be a property of the model, rather than a separate string-based approach | Prompt 3 | Borrowed typed contract occurrences, dated terms/waiver/escalation, forecast consuming those APIs |
| Work systematically through cleaner boundaries, control flow and testability | Prompts 4 and 5 (same request repeated) | Multiple bounded lanes, source fixtures, incremental review and verification |

`v2/REMAINING.md`, explicitly incorporated by prompt 1, contains wider continuing
work: R4 report/CLI completion; M4a/M4b v4 declarations and journal; E4b/E4c engine
semantics, units and norms; SY2 book binding; Y4 systems/examples; final v4
integration, mistakes, goldens and benchmarks. The file remains the authoritative
inventory for those requests. Existing SY merge denials and stash deletion are
not bypassed by this rework. No claim of complete v4 migration is made until its
own definition of done is verified.

## Implementation choices inferred from the requests

The prompts specify goals, not exact patches. Reusable report Context, a paired
engine view/checkpoint, typed source lookup and JSON rendering, borrowed contract
occurrences, sparse snapshots and precomputed membership are new design choices
that serve those goals. They are not descriptions of the lost implementation.
The first bounded checkpoint retained v3 behavior. The subsequent cutover is new
native implementation and fixture migration; passing counts from the earlier
checkpoint are not evidence that the native execution or corpus is complete.

## Verification and final status

The coordinator reviews every integrated change and records new commit hashes,
test results, golden changes, measurements and remaining migration gaps in
`RESULTS.md`. Tests of this tree establish its behavior only; they cannot prove
what was present on the erased remote host.
