# Ledger layout lab

Independent experimental crate. It does not modify or benchmark production
checking, certificates, durable storage, provenance, or parsing. See
[RESULTS.md](RESULTS.md) for the measured host and limitations.
The subsequent [production interpreter measurement](PRODUCTION.md) reports an
actual evaluator improvement separately from these independent layout probes.

```sh
cargo test --offline --manifest-path v2/lab/Cargo.toml
cargo clippy --offline --manifest-path v2/lab/Cargo.toml --all-targets -- -D warnings
cargo run --release --offline --manifest-path v2/lab/Cargo.toml -- 10000,100000,1000000 3
```

Arguments are comma-separated fact counts and the number of timing trials.
Construction runs once with counters disabled for timing and once with counters
enabled for allocation/memory statistics. Queries have one allocation-counting
run and warmed timing trials with counters disabled; the minimum is reported.
The timing wrapper uses `black_box` and checks every result outside the timing.
The global allocator still has an inexpensive disabled-counter branch.

The CSV-like stdout contains ABI sizes, construction/query statistics, exact
rational aggregate results, order-sensitive checksums, and two focused probes.
`raw.csv` saves the full-size recorded measurement rows. No network dependencies
are needed when the existing offline Cargo cache is available.

## Representation

- `btree-values`: owning `Vec<Row>`, string IDs/schema names, `BTreeMap<String,
  Value>` fields, a retained ID-to-row BTreeMap, exact BigRational quantities.
- `dense-big`: schema-interned slots in contiguous dense rows; row/number/symbol
  handles; one BigRational arena; shared text dictionary and ID byte arena.
- `dense-compact`: same slots with 16-byte inline normalized rationals;
  exceptional i128 and arbitrary-precision values live in a separate arena.
- `soa-compact`: two generated schema tables with typed columns, 16-byte exact
  amounts, interned text/unit handles, bit-packed booleans, direct row references.

The fixture is deterministic: every fourth fact is a purchase and others are
sales referring explicitly to that purchase. All seven possible fields and all
external ID bytes are retained. Dates cover twelve real 28-day month prefixes;
three units, 64 memos, 32 accounts and 16 counterparties are repeated. Quantities
are ordinary exact cents, a wide amount every 257 facts, and a 150-bit numerator
every 4,093 facts. The fixture supplies pre-resolved handles and pre-interned
symbol IDs. An arbitrary source loader would additionally resolve and intern
identities and text; that loading cost is outside this experiment.

`query` filters active sales after July 1, joins their explicit purchase, filters
the target's active flag/account, and computes exact sums grouped by three units.
`lookup` reads dates/text/identity and explicit target dates at deterministic
pseudorandom handles (10k, 100k, and 100k reads respectively). Every layout must
match the baseline's count, both checksums and all exact aggregate values.

`owned-cloning` approximates owning rows/filter/map/field-projection behavior;
`borrowed-fused` uses the same baseline maps and BigRational arithmetic but
borrows field values and sums in one pass. These are focused evaluator patterns,
not complete production interpreters: no parsing, budget, faults or provenance.
The cents probe isolates checked i128 rational accumulation versus BigRational.

## Safety

Only the forwarding counting allocator uses unsafe. Pointer operations and
Layouts go directly to `System`; counters neither allocate nor touch payloads.
Scopes are single threaded; preexisting allocations are never freed in a scope.
The construction's live-byte count initializes query scopes. Fixture/result
destruction happens with counters disabled.

`NonZeroU32` gives optional four-byte handles without manual enum tags.
`Exact64` uses an explicit denominator-zero sentinel and a checked one-based
exception index. Ordinary values have positive reduced denominators; overflow
promotes to arbitrary precision. All i128 intermediate additions/multiplications
are checked, and the `i128::MIN` normalization edge is rejected before calling
Ratio normalization. No pointer tagging, unions, packed references or unchecked
access is used. Schema columns remove per-value tags by their Rust types.
