# Axiom benchmark corpus

`axiom-bench` is the source of truth for the reproducible XVII corpus.  It
generates the named workloads from `confirmed-direction.md` plus the
package-authored generic-form and settlement-proof workloads using only
the Rust standard library and the checked-in Axiom parser/model APIs; no input
files, network access, random seed, wall clock, or floating-point arithmetic
are involved in workload generation.  The optional peak-RSS observation uses
the target's native `getrusage` resource API.

```text
cargo run --quiet --bin axiom-bench -- --quick
cargo run --quiet --bin axiom-bench -- --scale 4 --workload large-proof-explanation
cargo run --release --quiet --bin axiom-bench -- --quick --scale 100 --workload generic-form-elaboration
cargo run --release --quiet --bin axiom-bench -- --quick --workload settlement-state-proof
cargo run --quiet --bin axiom-bench -- --self-test
# discovers and verifies the release persistence boundary (ignored by default)
cargo test --release --test benchmark_gate10 settlement_state_proof_scale_boundary_discovers_maximum -- --ignored
```

Each stdout line is one stable JSON object.  Human-readable tables go to
stderr, so stdout can be piped to a JSON-lines consumer.  `--quick` reduces
the row count and sample count while retaining every workload.  `--scale N`
multiplies the deterministic row counts after the quick/full choice.

In addition to same-process cache replay, every workload records
`independent_clean_recompute_equal` and `independent_clean_solve_ns`.  These
come from a fresh `Workspace` and fresh incremental database, so they do not
turn cache replay into a determinism claim.  `peak_memory_bytes`, when
available, is the process-lifetime peak resident set size obtained from the
target's `getrusage` API (KiB converted to bytes on Linux; bytes on macOS).
The scope is called out in the JSON `note`. Independent-worker fields compare
serial and concurrent clean workspaces; they are not shared-engine parallelism.
Legacy shared-engine fields and unsupported platform measurements remain
`null` rather than being inferred.
The legacy `semantic_relation_*` fields also remain `null`; the measured graph
sizes are named `dependency_graph_nodes` and `dependency_graph_edges` because
they describe incremental dependency indexes, not domain relation cardinality.

The corpus uses the V0 source vocabulary where it exists. Invoice/payment is
an executable obligation/settlement/satisfaction workload. Currency,
corporate-action, ownership, package-upgrade, and recursive-rule workloads
also run separately labelled public domain-API probes; their source projection
and semantic probe timings are not conflated. The generic-form workload runs
through persisted package compilation, source-commit artifact pinning, exact
schema-bound document elaboration, canonical value verification, and a one-row
correction. `--scale 1`, `10`, and `100` select its 1k, 10k, and 100k profiles.
Its revision measurement is warm re-elaboration, not an incremental-cache
claim. Unsupported metrics remain `null` rather than being inferred.

The `settlement-state-proof` workload runs the complete package-authored
SettlementStateV1 path: persisted package compilation, artifact pinning,
document elaboration, ordered settlement projection, independent proof
checking, typed proof-child commit persistence, and `ObjectStore::verify`. It
also checks that a source revision creates a new proof authority while a
rejected negative amount leaves the store unchanged. Quick mode uses one form
per scale unit. The public proof boundary has a 4,096-row architectural cap,
but the canonical proof-byte limit is reached first for this corpus; therefore
`--quick --scale 4096` is expected to be rejected. The ignored release gate
discovers the largest successful scale through the public Workspace
persistence path (currently 3,309 rows) and checks the exact canonical-byte
rejection at the next scale.
Its `settlement_setup_ns` includes source loading, package compilation and
persistence, and artifact pinning; `settlement_document_projection_ns`
includes the Workspace document elaboration performed by the projection call.
`settlement_persistence_boundary_ns` measures typed proof and child-commit
persistence. `peak_memory_bytes` remains the isolated workload child-process
peak RSS when `getrusage` is available.

The performance numbers are measurements from the current process, not
assertions.  The performance goals copied from section XVII are emitted in a
separate `targets` object.
