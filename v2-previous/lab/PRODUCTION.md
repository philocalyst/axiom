# Production interpreter borrowing measurement

Unlike the synthetic layout lab, this probe calls the actual private evaluator
in `v2/src/expr.rs` with production `Expr`, `Value`, `Number`, `Fault` and `Budget`.
The same evaluator supports a test-only owning reference mode which disables
borrowed sequences/operands and reproduces owning relation/binding copies. It
is a comparison inside one engine, not a second interpreter or two separately
built historical revisions. Production builds contain only the borrowed mode.

```sh
cargo test --release --offline --manifest-path v2/Cargo.toml \
  --lib expr::tests::production_borrowing_benchmark \
  -- --exact --ignored --nocapture --test-threads=1
```

Recorded on 2026-09-26 on the same M3 Pro / Rust 1.98.1 host as RESULTS.md,
using v2's default Cargo release profile. Allocation counts come from a separate
scoped counted execution; times are the minimum of three counter-disabled
trials. The source fixture and environment are built before measurement; peak
bytes therefore describe newly allocated transient query memory, not world
storage or process RSS. Other active builds can affect timing.

The composed query is:

```text
(sum (map
  (filter (rows fact) x (and (eq x.account s.account) x.active))
  x x.amount))
```

Facts have six ordinary fields (explicit identity, account, exact USD amount,
active flag, repeated memo, date). There are 64 accounts; the query selects
account07. Every thirteenth fact is inactive. Exact amounts are `i/100 USD`.
Both modes must return the identical exact quantity and remaining work budget.

| Rows | Mode | Time ms | Allocation events | Requested bytes | Extra peak bytes |
| ---: | --- | ---: | ---: | ---: | ---: |
| 10k | Owning reference | 23.779 | 273,142 | 28,284,753 | 14,342,765 |
| 10k | Borrowed production | 3.323 | 1,246 | 56,010 | 25,011 |
| 100k | Owning reference | 264.690 | 2,731,638 | 282,835,356 | 143,492,765 |
| 100k | Borrowed production | 33.937 | 12,868 | 480,189 | 200,937 |

At 100k this is 7.80× faster, 212.3× fewer allocation events and 714.1× less
transient peak heap. All outputs are checked outside the timed section:

| Rows | Exact result | Remaining from 10,000,000 work units |
| ---: | --- | ---: |
| 10k | `724471/100 USD` | 9,919,404 |
| 100k | `72140853/100 USD` | 9,194,104 |

Production borrowing keeps relation slices and filtered row references until an
owned result is required. Mapping owns projected values only; lexical iteration
bindings borrow their parent and current item; direct comparison operands and
record projections borrow existing values. Eager stage order is retained, so
later mapper/filter faults still precede downstream aggregate/first results.
Fallback list-producing expressions use the same ordinary evaluator.

Differential tests compare results, exact Fault variants/messages, ordering and
remaining budget at **every budget up to termination** over 20 deterministic
seeded fixtures and 31 expression forms. Cases include holes and nested holes,
incompatible units, empty/unknown relations, malformed binders, compound
predicates, nested shadowed bindings, quantifier short circuits, sorting, indexing
and owning fallback sources. Public replay tests also cover composed outputs,
authored reference identities/order, late holes and mapping fault precedence.

This is production **expression execution**, not complete ledger checking,
proof construction, source parsing, durable storage or million-row verification.
Correlated filters still scan their input relation. No index was introduced, and
the unchanged logical budget still charges all rows. Public source, claim,
certificate and evaluator interfaces are unchanged; there is no production
unsafe. The test-only allocator reuses the standalone lab's forwarding counter.
