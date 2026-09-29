# Measured layout results

The one-million-fact fixture uses 1,453 MiB in the owning map baseline and
64.33 MiB in safe typed columns: **22.6× less requested live heap**. A simple
dense layout retaining BigRational already reduces it to 164.10 MiB. Eliminating
maps, duplicated text/field names and temporary ID strings delivers the main
memory/allocation improvement. No unchecked tags or pointer tricks are needed.

These are synthetic storage/query measurements, **not one million production
propositions checked or certificates verified**. There are no production edits
in this crate.

## Reproduce and interpret

```sh
cargo test --offline --manifest-path v2/lab/Cargo.toml
cargo clippy --offline --manifest-path v2/lab/Cargo.toml --all-targets -- -D warnings
cargo run --release --offline --manifest-path v2/lab/Cargo.toml -- 10000,100000,1000000 3
```

Recorded on 2026-09-26, Apple M3 Pro MacBook Pro (Mac15,6), 11 cores (5
performance/6 efficiency), 36 GB RAM, Darwin 24.6.0 arm64. Rust 1.98.1
(`48a229cea`, LLVM 22.1.8), `aarch64-apple-darwin`; Cargo release with thin LTO,
one codegen unit, default target CPU. Locked offline dependencies:
num-bigint 0.4.8, num-rational 0.4.2, num-traits 0.2.19, num-integer 0.1.47.

Construction time is one counter-disabled build; memory/allocation statistics
come from a separate construction. Query time is minimum of three warmed trials
with counters disabled, and allocation statistics come from a separate counted
trial. Other agents/builds were active on this host: scheduling, cache state and
allocator reuse affect timings. No CPU pinning, statistical confidence intervals,
hardware performance counters or RSS measurements were collected. Memory and
allocation counts are stronger evidence than small timing differences.

The allocator reports **requested** heap bytes: vector capacity, map nodes,
strings and numeric storage; it excludes allocator metadata, fragmentation,
stack, executable pages and OS residency. Allocation events include realloc;
requested bytes sum full requested sizes, whereas live/peak account for frees
and realloc replacement. Dictionaries and all ID bytes are included. No
post-hoc memory-size estimate is substituted for these counters.

## Full comparison

Same semantic filter → explicit-reference join → three exact unit sums. Point
reads use 10,000 requests for the 10k fixture and 100,000 for the other fixtures.
The baseline has a retained string ID index; compact references are already
resolved row handles. This evaluates the benefit of lowering references, not
the cost of resolving arbitrary input IDs.

Generated purchases and their sales sit near each other in row order, so this
join favors locality. The fixture is fixed-width, mostly populated and has a
small repeated text vocabulary. Cross-ledger references, long unique memos,
nested records/lists, many package schemas and sparse fields can change the
best representation. The 150-bit values deliberately stress exact fallback;
they are not a claim about a typical financial magnitude distribution.

| Facts | Layout | Build ms | Build allocations | Live MiB | Filter/join/sum ms | Point reads ms |
| ---: | --- | ---: | ---: | ---: | ---: | ---: |
| 10k | BTreeMap values | 14.998 | 184,408 | 14.53 | 2.821 | 11.712 |
| 10k | Dense / big | 2.704 | 245 | 1.65 | 1.270 | 0.626 |
| 10k | Dense / compact | 2.308 | 132 | 1.19 | 0.356 | 1.559 |
| 10k | SoA / compact | 1.438 | 148 | 0.65 | 0.387 | 0.480 |
| 100k | BTreeMap values | 146.416 | 1,843,027 | 145.31 | 63.535 | 196.937 |
| 100k | Dense / big | 54.304 | 1,364 | 16.42 | 33.478 | 35.788 |
| 100k | Dense / compact | 19.257 | 198 | 11.86 | 26.095 | 48.224 |
| 100k | SoA / compact | 14.356 | 214 | 6.44 | 27.222 | 48.485 |
| 1M | BTreeMap values | 2,989.133 | 18,429,191 | 1,453.10 | 702.512 | 260.353 |
| 1M | Dense / big | 555.641 | 12,530 | 164.10 | 322.976 | 37.514 |
| 1M | Dense / compact | 132.532 | 858 | 118.57 | 331.088 | 40.566 |
| 1M | SoA / compact | 128.281 | 874 | 64.33 | 333.091 | 57.642 |

At 1M the baseline processes 1.42M input facts/s for the aggregate query; dense
big processes 3.10M/s and compact layouts about 3.00M/s. Point reads are 0.384M
reads/s in the baseline, 2.666M in dense big, 2.465M in dense compact and 1.735M
in SoA. **SoA is the smallest representation; it does not win every access
pattern.** Dense rows give better locality when random reads need several fields.

Peak construction requested bytes at 1M: baseline 1,523,694,273; dense big
172,076,481; dense compact 124,334,289; SoA 67,459,265. The complete allocation,
total-requested-byte and timing rows are in [raw.csv](raw.csv).

## Exact arithmetic and ownership probes

| Probe | Facts | Time ms | Allocation events | Requested bytes | Extra peak bytes |
| --- | ---: | ---: | ---: | ---: | ---: |
| Owning rows/filter/map/field projection | 10k | 66.246 | 777,499 | 71,760,564 | 15,136,053 |
| Borrowed fused projection, same maps/numbers | 10k | 15.985 | 26,191 | 549,912 | 72 |
| BigRational cents aggregate | 100k | 57.506 | 0 | 0 | 0 |
| Checked i128 rational cents aggregate | 100k | 15.155 | 0 | 0 | 0 |

The borrowing probe is 4.14× faster and has 29.7× fewer allocation events while
keeping the original map representation. It models allocation patterns rather
than running the complete current production interpreter. The tiny borrowed
peak is possible because only the exact running total is newly allocated.

The offline num-bigint 0.4.8 implementation keeps small magnitudes inline, so
**both cents probes have zero heap allocations**. The measured 3.79× speedup is
arithmetic/representation overhead, not removal of nonexistent allocations.

The mixed fixture also contains i128 and 150-bit values. As soon as a selected
150-bit term enters a group, its running total promotes to BigRational; compact
storage alone cannot eliminate subsequent large-result arithmetic. At 1M,
baseline/dense-big aggregation performs 466,206 allocation events and requests
10,846,296 bytes; compact layouts perform 425,058 and request 10,187,928. The
much larger compact benefit at 10k (9 allocations versus 4,024) does not persist
after the large terms participate. This is a real workload boundary.

## Correctness evidence

Five tests pass: every field and external identity reconstructs identically;
all layouts match the complete exact aggregate and deterministic point-read
checksum; compact values/fingerprints match BigRational over 20k generated
values; borrowed/owned projections match; checked i128 overflow and the minimum
integer normalization edge promote rather than wrap. Clippy passes with
warnings denied. Each measured result is checked, not merely sampled.

| Facts | Selected | Ordered query checksum | Point-read checksum |
| ---: | ---: | --- | --- |
| 10k | 1,580 | `7131d880117caf06` | `7e47eeffaffe815b` |
| 100k | 15,845 | `49a952fcbe83015a` | `48873e556f631864` |
| 1M | 158,645 | `c55dba18e7f87269` | `649b13fdaf6e45af` |

The three exact 1M group totals, asserted equal in every layout, are:

```text
76459697823533565056693952020977134967212493219/25
713623846352979940529143840063376777956109763333/350
1855422000517747845375773446169178061761694441977/700
```

## Compact representations and simpler production steps

Measured arm64 sizes: mirrored Value 88 B, Row header 72 B, BigRational 64 B,
generic Cell 12 B, seven-slot DenseRow 92 B, Exact64 16 B, exceptional numeric
enum 64 B, global schema/local location 8 B, RowId 4 B and Option<RowId> 4 B.
An owned `Integer(i64) / Rational64(i64, NonZeroU64) / boxed i128 / boxed
BigRational` enum is also included as a size-only proposal; it is not timed.
Its measured size is 24 B. This gives a simpler owned number option at an extra
eight inline bytes and one allocation for each wide/large fallback.

1. Remove repeated work before changing canonical data. Append claims and sort
   once; compile rule read sets once; index occurrences/schemas; share immutable
   relation snapshots; represent relation provenance with bound shared witnesses.
   Rebuilding every world relation per subject and embedding a full input list
   in every derived claim grows quadratically regardless of enum size.
2. Smallest evaluator change: traverse a Name path by reference and clone only
   the final result; borrow row slices/handles for `rows`, filter, map and choice;
   use stacked binding slots instead of cloning environments. Fuse bounded
   filter/map/sum in a lowered plan when error, hole, budget and provenance
   semantics permit it. The borrowing probe measures the potential independently
   of physical storage. Fault/short-circuit behavior still needs production tests.
3. Compile a generic `SchemaLayout` mapping field names to numeric slots and
   `Type` to physical column kinds, plus lowered relation/field/binding handles.
   Freeze one relation snapshot per ordered rule; keep stable row indices through
   each snapshot. This is package-schema specialization, not built-in purchase
   or payment variants. Begin with a borrowed dense execution view over existing
   canonical rows to remove temporary clones without changing public Row/Value,
   source syntax, certificate payload or canonical codec.
4. Owning dense cells plus pooled text already capture most retained-memory
   reduction; use typed columns for large scans once measurements justify their
   complexity. Preserve authored ID bytes in one arena and lower explicit refs
   to handles after checking identity/type. Do not infer identity from equal
   values. The fixture's fixed dictionaries and already resolved handles omit
   arbitrary-source interning/resolution and sparse/heterogeneous schemas.
5. Exact numeric storage can use the tested 16-byte inline value plus exception
   arena. Keep external Number syntax, normalization and serde unchanged. Every
   operation must check intermediate overflow, preserve arbitrary rational
   denominators, dimensions, negative values and result-size limits. This lab
   implements addition only; it does not replace production Number's arithmetic.

Keeping `World::evaluation() -> &Evaluation` and a public owning certificate
payload means their original BTreeMap rows still occupy memory. Adding a dense
execution view removes transient allocation but **does not achieve the table's
retained-memory numbers while both owning representations are held**. Reaching
those numbers in production requires a compact authoritative store with a
canonical serialization/materialization boundary, or an API/storage revision;
that larger change is not implemented or assumed here.

Typed columns are safely tagless because the schema fixes each column's type;
units, text and refs use checked indices; optional handles use Rust's standard
nonzero niche. Unsafe pointer tagging, unaligned packed references and custom
union discriminants add invariants without evidence of a necessary gain. The
only unsafe in this experiment is allocator forwarding.
