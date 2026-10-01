# Parser counting benchmark

This standalone Rust program measures parser allocations and wall time on two
deterministic generated inputs. Its relative path dependencies point at the
`axiom-core` and `axiom-syntax` crates in this checkout. It has no benchmark
framework or dependencies beyond those crates.

From the repository root, run:

```sh
cargo run --manifest-path v2/crates/syntax/bench/Cargo.toml --release --locked --offline
```

The committed `Cargo.lock` pins the harness dependencies. `--offline` uses
dependencies already present in the Cargo cache. The release profile is
optimized, and the program prints median parse nanoseconds and mean allocation
calls and requested bytes for each fixture.

Each fixture repeats this exact 47-byte record:

```text
2026-01-15 checking -> 5 USD
  groceries 5 USD
```

The small input repeats it 700 times (32,900 bytes and 700 parsed items); the
large input repeats it 30,000 times (1,410,000 bytes and 30,000 parsed items).
The generator reserves `n * 54` bytes before appending the records.

For each fixture, the program parses twice to warm the parser, then records 30
parses. The timer covers only `axiom_syntax::parse`; the global allocator
counters are reset immediately before each timed parse. Allocation calls
include `alloc`, `alloc_zeroed`, and `realloc`; requested-byte totals add each
call's requested size (for `realloc`, the new size). Deallocations, live heap
size, and fixture construction are not counted. The reported allocation
figures are integer means over 30 parses; parse time is the middle sorted
sample (`samples[15]`).

The parser may spawn workers. The allocator counts allocations across those
threads, so worker setup and scheduling can move the measured totals and times
slightly between runs. Run one copy at a time on an otherwise idle host when
comparing changes. This is an observational harness, not a statistical
benchmark suite.
