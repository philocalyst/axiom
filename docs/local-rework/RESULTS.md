# Verified local Axiom rework

> This report describes the earlier bounded rework. The subsequent native
> cutover stopped on 2026-10-02 at product commit `1be566d`, with 734 tests passed,
> four failed and eight ignored. Its full example checks and formatting still
> fail. Read [the current handoff](STOPPING-POINT-2026-10-02.md) for present status;
> the historical green results below do not certify the promoted native workspace.

Completed on October 1, 2026 in the independent checkout
`/Users/mileswirht/Documents/Codex/2026-10-01/task/axiom-local`, branch
`rework/recovered-client-boundaries`. The verified source commit is `03368b3`;
the following documentation commit records these results. The baseline is
`12d5d1889fe9bc699a20c32a2056aa571d142d4b`.

This is new implementation guided by surviving messages. It does not restore
the erased H16002 patches: zero recent source or diff payloads survived. Six
implementation lanes were reviewed and integrated incrementally. The exact
cached user text, source SHA-256 and JSON pointers are in
[recovered-prompts.json](recovered-prompts.json); [SCOPE.md](SCOPE.md) separates
explicit requests from new implementation choices.

## Every surviving prompt

Prompt numbers are cached array positions, not recovered timestamps.

| Prompt | Request and provenance | Implemented local work | Known gap or limit |
| --- | --- | --- | --- |
| 0 | Clone the Axiom repository | Independent clone from the preserved exact Git objects; new branch and isolated worker checkouts | Clone uses recovered objects; it does not contain the missing recent Windows edits |
| 1 | Continue from the named main and REMAINING; cleaner typed formulations, fewer allocations, data structures/parsing, Carbon guidance | Coherent named baseline; borrowed per-kind defaults; sparse snapshots; kind-total membership buckets; local parser context; ordered piece aggregation; reproducible allocation/lookup experiments and Carbon primary-source notes | Broad v4 migration in REMAINING remains unfinished. Existing immutable `core::par` concurrency is retained; no new production unsafe, ref-counted or locking architecture is introduced. `Kind.props` still copies cumulative slices; parser timing showed no supported speedup |
| 2 | Abstract CLI assumptions for GUI/other clients, stronger traits and borrowing | Borrowed `SourceProvider`, `ReportRenderer`, typed report JSON and diagnostic NDJSON; reusable coherent `Context`; CLI uses the same API; shared Plan/Known/Sides/checkpoint; exact historical effect prefix | Current views are covered. Future facts/sentences/XBRL and v4-only views still need model/engine producers; this does not build a GUI |
| 3 | Make forecasting a property of the model | Borrowed typed contract occurrences; dated active/waived terms; checked anniversary escalation, recognition and proration; typed flow derivation and coverage; report projects those flows through ledger laws; incomplete contracts are visibly reported | Native contract fixtures construct the model directly. Source v4 contract parsing and loan-payment derivation remain unavailable; current v3 plan/history forecasting stays as a compatibility fallback. Deadlines, shares, also, purchases, deposits and matching return explicit unsupported-feature errors |
| 4 | Work systematically through cleaner boundaries, control flow and testability | Paired Context state; checkpoint phases and resumed synthetic IDs; shared projection kernel; real source regressions for dates/owner scope/closings/assertions; incremental reviewed commits and aggregate verification | This is a bounded new rework, not exhaustive reconstruction of unavailable history or completion of every v4 lane |
| 5 | Repeat prompt 4 and be more clever | Compute immutable names/signs/membership once; resume state instead of refolding; sparse per-place/unit snapshots; stream schedules; exact pre-close record boundary avoids inference and Effect copies | Optimizations are measured below with tradeoffs. No universal speed or memory improvement is claimed |

The incorporation of `v2/REMAINING.md` is honored as an inventory, not marked
complete. M4a/M4b, E4b/E4c, SY2, Y4 and final v4 integration remain. The divergent
report/sync/v4 branches were not blindly merged. Prior sync/stash/UI denials
were not bypassed; no push, publication or backend change occurred.

## Reviewed behavior

`Context` constructs its Plan, Run and checkpoint together and retains the
caller’s relaxed setting. Forward views resume the exact pre-closing boundary;
past views replay the same Plan. The compatibility free report API continues
to use its supplied Run. Forecasting shares the projection kernel and the
Context’s plan/checkpoint.

Review caught two subtle forecast issues and the implementation was corrected.
An empty waiver selects the nearest active terms before matching movement
identity, instead of matching any template in the contract’s history. Historical
obligations come from the exact recorded prefix captured before the view fork,
then combine with resumed effects. Dates and `Cause::Time` cannot identify that
prefix because assertion pads also use `Cause::Time`. A real source fixture
proves an old flow fee, today’s assertion fee and today’s year-end closing are
each shown once. Changed due days do not duplicate a matching fallback rhythm.

See [REPORT-CLIENT-API.md](REPORT-CLIENT-API.md) for source positions, typed JSON,
channels and error statuses; [CONTRACT-FORECASTS.md](CONTRACT-FORECASTS.md) for
forecast assumptions and unsupported features; [PROPERTY-DEFAULTS.md](PROPERTY-DEFAULTS.md)
for the remaining inherited storage tradeoff; and
[PARSER-EXPERIMENTS.md](../../v2/crates/syntax/PARSER-EXPERIMENTS.md) for Carbon
references and allocation measurements.

## Aggregate verification

The final coordinator checks ran serially against the integrated source:

- `cargo check --workspace --tests --offline --locked`: passed without warnings.
- `cargo test --workspace --release --offline --locked`: **432 passed, zero failed,
  two ignored**, compared with baseline 399 passed and two ignored. The ignored
  cases are the existing million-flow benchmark and syntax AST documentation.
- Locked offline release CLI build: passed.
- `sh tests/golden.sh`: all 60 golden files byte-identical.
- `sh tests/mistakes/run.sh`: all 125 output files byte-identical.
- Independent `verify04.py` through `verify10.py`: all seven passed.
- Strict external JSON decoder: 22 report/check/error/scope cases passed,
  rejecting duplicate keys and checking typed cells, byte ranges and channels.
- Paired benchmark stdout hashes: identical for all eight command/scale pairs.
- `git diff --check` against the baseline: passed.

`cargo fmt --all --check` remains failed. The unchanged baseline already
produced 30,673 lines of formatting differences against default rustfmt; the
integrated tree produces 32,550. Surrounding compact style was retained instead
of reformatting the repository. This is a remaining formatting limitation,
not a green formatting check.

The final non-test line count is **23,872**, versus baseline 22,327 and the
existing 24,000 workspace cap. This does not establish the full v4 size target.
No production dependency was added; the standalone parser benchmark has its
own locked path-dependency manifest. Local commits are unsigned because the
configured signing store is unavailable in this environment; it was not
bypassed and no attribution trailers were fabricated.

Verification commands, exact outputs, generated fixtures and machine-readable
results are preserved under
`/Users/mileswirht/Documents/Codex/2026-10-01/task/verification/`:
`final-checks.json`, `final-tests.log`, `final-corpus.diff` (empty),
`final-json-checks.json`, `final-benchmarks.json`, `final-format.log`,
`final_verify.py`, `verify_cli_json.py` and `compare_cli.py`.

## Before/after measurements

The deterministic fixtures have 100,030 and 1,000,094 flows. The baseline release
binary was preserved before changes. Each command has one paired warmup and
three alternating paired samples; values below are medians. RSS comes from
Darwin `wait4` in bytes and is displayed in MiB. The old Linux `time -v` harness
was not used for Mac RSS. These are observational samples on a shared host, not
statistical evidence for a general speedup.

Forecast uses `--paths 200`; balance uses `--monthly`. The fixture days are
2031-12-31 for 100k and 2035-12-31 for 1M, with color disabled for both versions.

| Flows | Command | Wall s, before → after | Peak RSS MiB, before → after |
| --- | --- | ---: | ---: |
| 100k | check | 0.037742 → 0.037030 | 102.6 → 102.1 |
| 100k | available | 0.046657 → 0.035970 | 102.5 → 103.2 |
| 100k | forecast | 0.069425 → 0.055078 | 107.2 → 107.6 |
| 100k | balance | 0.045589 → 0.045378 | 108.4 → 104.3 |
| 1m | check | 0.291653 → 0.279379 | 814.3 → 834.6 |
| 1m | available | 0.404248 → 0.289622 | 836.2 → 863.0 |
| 1m | forecast | 0.641768 → 0.541033 | 883.1 → 877.2 |
| 1m | balance | 0.329342 → 0.327919 | 874.2 → 845.7 |

Available and forecast wall times were lower in these samples. Memory was
mixed: the shared context retains state, and the 1M available run used about
3.2% more peak RSS while monthly balance used about 3.3% less. The 1M check
median was 0.279 seconds on this Mac; cross-host historical timings are not
directly comparable.

The parser allocator experiment reduced calls from 17 to 15 on the small
fixture and from 544 to 538–539 on the larger fixture; no statistically
supported parser time improvement was observed. The kind-total lookup harness
showed a synthetic lookup-only median improvement of 20.78×; it does not
measure Plan construction or whole-engine speed. The harnesses and their
assumptions are committed in the syntax and engine `bench/` directories.

## Preservation and commit sequence

The original Downloads checkout remains clean at `80877d6da3ba7a8bcad3297b3e260fd4f4f96ab0`.
The recovery checkout remains clean at the selected baseline. The cached
prompt source still hashes to
`070f35ba350cee41df9754fa4a59ed670cfbc2b00c308f7977fda0659b5963c9`.
Recovery bundles, evidence and alternate refs remain unchanged. Only disposable
build outputs produced by this reconstruction were removed during disk recovery.

The following new commits are in integration order. Worker dependency copies
already present in main were not re-applied. The documentation commit that
contains this table follows the verified source commit.

| Commit | Change |
| --- | --- |
| `091c038` | Document recovered request scope for new local rework |
| `7d8c08d` | model: borrow inherited property defaults |
| `a7f674e` | engine: preserve checkpoint day boundaries |
| `4c6aedd` | docs: record property default allocation tradeoff |
| `808b61c` | Add client-neutral report rendering and JSON output |
| `290673e` | syntax: streamline piece parsing and make tail context explicit |
| `abebbfa` | syntax: retain counting allocator benchmark harness |
| `41e0108` | report: add reusable view context |
| `9586b72` | Document report client and JSON contracts |
| `be6bf24` | Add typed contract occurrence forecasts |
| `cf6c7a5` | engine: index kind-widened total places |
| `adafebf` | Use shared report context in CLI views |
| `73d4281` | report: reuse prepared place signs |
| `35a7060` | report: store historical snapshots sparsely |
| `08583e2` | report: resume forecast from view checkpoint |
| `bfef899` | Model contract coverage and forecast limitations |
| `81a1a1a` | Use nearest active terms for empty waivers |
| `d74a2f1` | Forecast contract flows and suppress covered fallback |
| `9be4d9c` | report: resume forecast from exact historical prefix |
| `81c850c` | Clarify typed contract forecast limits |
| `7d07452` | docs: explain contract forecast boundaries |
| `03368b3` | report: preserve exact pre-close effect prefix |
