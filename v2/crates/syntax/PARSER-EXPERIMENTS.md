# Parser experiment notes

## Carbon research

- Carbon's [parse design](https://docs.carbon-lang.dev/toolchain/docs/parse.html)
  keeps name lookup and non-local validity checks out of parsing. It favors
  retaining a structurally useful result for invalid input, and describes a
  flat postorder tree that is cheap to emit and walk. Axiom already borrows its
  source and stores expressions in a postorder vector, so this was evidence to
  preserve that representation rather than copy Carbon's C++ parse-tree API.
- Chandler Carruth's [C++Now talk on modernizing Carbon's compiler](https://www.youtube.com/watch?v=ZI198eFghJk),
  listed by the [official C++Now session page](https://schedule.cppnow.org/session/2023/modernizing-compiler-design-for-carbons-toolchain/),
  focuses on data-oriented compiler design, performance on modern hardware,
  and the interaction between grammar and implementation. Together with the
  parse docs' guidance to keep only local checks in parsing, it supports making
  parse context local and visible. The `TailContext` parameter makes the
  opening-only `since` rule explicit instead of mutating parser-wide mode.

## Ordered piece collection

`parse_in` previously made a `Vec<Range>` in `cut`, copied those ranges into a
second indexed vector, then buffered all `(Piece, diagnostics, tabs)` results
before walking them. `cut` now produces indexed ranges directly, and
`map_each_ordered` aggregates diagnostics and tab counts in source order
without materializing the extra full vector of `(Piece, diagnostics, tabs)`
results; its internal reorder slots still hold results until their predecessors
are ready. The caller collects the `Piece` vector that `File::new` needs. The
parser also passes a `TailContext` to each tail parse, replacing a mutable
parser-wide `opening` flag with an explicit grammar input. Both changes
preserve the existing parse tree, diagnostics, and piece order.

The benchmark fixture is deterministic: repeat
`2026-01-15 checking -> 5 USD\n  groceries 5 USD\n` 700 or 30,000 times. A
release harness warmed each input twice, then parsed it 30 times while a
counting global allocator tracked allocation calls and requested bytes. The
allocation figures are means per parse. Parallel worker activity introduced a
small run-to-run variation in the larger fixture's byte totals, so before and
after values are shown as observed ranges. Wall-time medians varied
substantially across runs for both versions; no statistically supported
speedup was observed.

| Fixture | Source bytes | Items | Alloc calls, before → after | Requested bytes, before → after |
| --- | ---: | ---: | ---: | ---: |
| 700 items | 32,900 | 700 | 17 → 15 | 341,400 → 338,824 |
| 30,000 items | 1,410,000 | 30,000 | 544 → 538–539 | 16,095,397–16,095,595 → 16,015,838–16,018,574 |

The small allocation reduction is repeatable in the before/after structure;
the observed allocator totals also include per-piece table growth and runtime
thread setup. The wall-time result is null, not a speedup. The change is
retained for the lower allocation count and simpler collection path.

## Validation

`cargo test -p axiom-syntax --release --locked --offline` passes.
The focused suite compares single-piece and multi-piece ASTs and diagnostics,
including invalid syntax with Unicode source text. A new test parses two files
concurrently, checks piece-parsed output against serial output, verifies error
ordering and file-local locations, and ensures diagnostic spans remain on
UTF-8 boundaries.
