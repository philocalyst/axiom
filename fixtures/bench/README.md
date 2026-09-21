# Axiom benchmark corpus

`axiom-bench` is the source of truth for the reproducible XVII corpus.  It
generates the eleven named workloads from `confirmed-direction.md` using only
the Rust standard library and the checked-in Axiom parser/model APIs; no input
files, network access, random seed, wall clock, or floating-point arithmetic
are involved in workload generation.  The optional peak-RSS observation uses
the target's native `getrusage` resource API.

```text
cargo run --quiet --bin axiom-bench -- --quick
cargo run --quiet --bin axiom-bench -- --scale 4 --workload large-proof-explanation
cargo run --quiet --bin axiom-bench -- --self-test
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
The scope is called out in the JSON `note`; unsupported platform or parallel
engine measurements remain `null` rather than being inferred.

The corpus uses the V0 source vocabulary where it exists.  Invoice/payment,
ownership, corporate-action, and recursive-rule shapes are represented by
labelled evidence rows and comments because those domains are not yet
accepted by the V0 parser.  Their unsupported metrics are emitted as `null`
with an explicit reason. Non-null timings, proof sizes, cache data, and RSS for
these rows measure only the accepted V0 evidence projection; they are not
measurements of the richer domain semantics represented by the shape.

The performance numbers are measurements from the current process, not
assertions.  The performance goals copied from section XVII are emitted in a
separate `targets` object.
