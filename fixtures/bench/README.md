# Axiom benchmark corpus

`axiom-bench` is the source of truth for the reproducible XVII corpus.  It
generates the eleven named workloads from `confirmed-direction.md` using only
the Rust standard library and the checked-in Axiom parser/model APIs; no input
files, network access, random seed, wall clock, or floating-point arithmetic
are involved in workload generation.

```text
cargo run --quiet --bin axiom-bench -- --quick
cargo run --quiet --bin axiom-bench -- --scale 4 --workload large-proof-explanation
cargo run --quiet --bin axiom-bench -- --self-test
```

Each stdout line is one stable JSON object.  Human-readable tables go to
stderr, so stdout can be piped to a JSON-lines consumer.  `--quick` reduces
the row count and sample count while retaining every workload.  `--scale N`
multiplies the deterministic row counts after the quick/full choice.

The corpus uses the V0 source vocabulary where it exists.  Invoice/payment,
ownership, corporate-action, and recursive-rule shapes are represented by
labelled evidence rows and comments because those domains are not yet
accepted by the V0 parser.  Their unsupported metrics are emitted as `null`
with an explicit reason; the harness never treats a placeholder as a semantic
result.

The performance numbers are measurements from the current process, not
assertions.  The performance goals copied from section XVII are emitted in a
separate `targets` object.
