# Parser notes and measurements

## Carbon research

- Carbon's [parse design](https://docs.carbon-lang.dev/toolchain/docs/parse.html)
  separates local syntax from name lookup and other non-local checks, and
  discusses keeping a useful structure for invalid input. Its flat postorder
  tree is also designed to emit and walk cheaply. Axiom's v4 parser follows the
  same useful boundaries with borrowed source text, per-piece typed tables and
  a postorder expression arena; the documentation was a reason to retain those
  properties rather than copy Carbon's C++ tree API.
- Chandler Carruth's [C++Now talk on modernizing Carbon's compiler](https://www.youtube.com/watch?v=ZI198eFghJk),
  listed on the [official C++Now session page](https://schedule.cppnow.org/session/2023/modernizing-compiler-design-for-carbons-toolchain/),
  covers data-oriented compiler design, performance on modern hardware, and
  how grammar and implementation choices interact. I used that as guidance to
  keep the parse pipeline's context and storage stages visible and measured.

## v4 storage and indices

`File<'s>` borrows its source. Names, codes, literals, raw text and docs are
source slices; nodes that vary in count live in typed vectors owned by each
piece. A `Ref<T>` packs an 8-bit piece number and a 24-bit local table index.
Expressions are stored in postorder, so children precede their parent. The
formatter reads that borrowed tree and rewrites only parsed journal lines;
`File::format()` uses the exact source borrowed by the tree.

An indivisible piece larger than 2^24 source bytes is rejected before a typed
reference is made. Each stored node requires at least one source byte, so the
piece byte count bounds every node table's local index. The outer file limit
remains 2 GiB, subject to this per-piece index bound.

## Ordered piece collection experiment

After promoting the recovered v4 parser, `parse_in` collected each parallel
parse result in a `Vec<Parsed>`, then copied its pieces, diagnostics and tab
counts into final outputs. The candidate uses the existing
`par::map_each_ordered` callback to fold each result in source order as it
arrives. It retains the `Piece` vector required by `File::new` and the core
primitive's reorder slots, while removing the separate full `Vec<Parsed>`.
Diagnostics are sorted by source offset as before, and pieces are collected in
order. The parser's AST and public parse results do not change.

The committed counting harness generates the same exact 47-byte record
(`2026-01-15 checking -> 5 USD\n  groceries 5 USD\n`) 700 or 30,000 times. It
does two warmups and measures 30 parses per fixture in release mode. Two
baseline/fold pairs were run against the v4 API after the index guard landed:

| Fixture | Source bytes / items | Baseline → fold allocation calls | Baseline → fold requested bytes | Baseline / fold parse medians (ns) |
| --- | ---: | ---: | ---: | ---: |
| Small | 32,900 / 700 | 18 → 17 | 305,552 → 301,936 | 103,250 / 105,500; 129,334 / 102,625 |
| Large | 1,410,000 / 30,000 | 586 → 581 | 14,532,976 → 14,420,948; 14,533,065 → 14,422,164 | 2,350,541 / 3,770,625; 1,966,208 / 1,943,209 |

Allocation calls and requested bytes are means over 30 parses. The large-file
fold saves 110,901–112,028 requested bytes per parse; the small-file fold saves
one call and 3,616 bytes. Timing has no supported speedup: the baseline and
fold observations overlap, and one 3.77 ms fold result was not reproduced by
the immediate 1.94 ms fold rerun. Treat the result as an allocation reduction,
not a throughput claim. Root's ordinary-release workload check remains the
promotion gate.

The timer runs while a counting global allocator updates atomics for every
allocation. Counts include `alloc`, `alloc_zeroed` and `realloc` requests,
including worker setup; requested bytes are not live heap size, and
deallocations are not counted. Fixture creation is outside the timed parse.
This instrumentation affects execution timing, so these values are not
ordinary-release timings. Parallel worker scheduling also varied across runs.

## Validation

`CARGO_INCREMENTAL=0 cargo test -p axiom-syntax --locked --offline` passes the
focused syntax suite, including malformed Unicode spans, concurrent files with
distinct `FileId`s, oversized indivisible pieces, and serial-versus-piecewise
AST/diagnostic equality across damaged input. The standalone harness runs with
`cargo run --manifest-path v2/crates/syntax/bench/Cargo.toml --release --locked --offline`.
