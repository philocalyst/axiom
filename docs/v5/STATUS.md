# v5 status

Where the rewrite stands, and what is waiting on a decision. Read [`DESIGN.md`](DESIGN.md) for the design and
[`lanes/common.md`](lanes/common.md) for the standard every lane is held to. The integration branch is
`claude/great-wozniak-pnqn7x-v5`; each lane works on its own `-v5-*` branch and is merged here after review.

## Lanes

| lane | what | state |
|---|---|---|
| **C** core primitives | `tagless`, `dayset`, `sparse`, `postings`, `placement`, `trail` | **merged** (`8704e75`) |
| **K0a** model groundwork | `Staged`, one `problem` catalog, `Word::of`, one collect pass, dead code, one lowering of a literal | running |
| **K0b** outer groundwork | owners, JSON writers, sync dates, sync apply, `RuntimeRange` | running |
| **K12** kinds, slots, facts | typed slots, `Taxonomy`, `Facts` with typed keys, integrated conditions | brief written; starts when K0a merges |
| K3 positions and addresses | | after K12 |
| K4 events | | after K3 |
| K5 promises | | after K4 |
| K6 norms (relators) | | after K12 and K3 |
| K7 facts out, `Session` | | after K5 |
| L language | | last |

Test baseline before any lane: 734 passed, 4 failed, 8 ignored. Lane C on top: 777 passed, the same 4 failed, 13
ignored (the new ones are benchmarks). The four failures are the ones `v2/REMAINING.md` names.

## Lane C, in numbers

| | |
|---|---|
| lines added | +683 non-test in `core` (2,508 total) |
| `unsafe` | one macro, nine expansions, argued sound for every bit pattern |
| tagless | a count of one kind of value over the tag column alone is 6.5× faster than over an enum |
| sparse table | a range query is 4.2 ns at any width, against 64 µs for a scan of 65,536 |
| `postings` merge | 2.1–2.7× faster than the classic merge, galloping wins from a skew of 8 |
| `Trailed<_, ()>` | writes exactly like a `Vec` |

## Waiting on you

1. **The `fearless_simd` kernel for `postings` is not built.** The measured headroom is real: a hand-written SSE or
   AVX2 block-compare is 1.7–2.4× faster than the scalar merge on lists of 10⁴–10⁶. The lane was refused permission
   to read the crate's source in the cargo registry, and docs.rs is blocked by the network policy, so nobody could
   learn the `fearless_simd` API. Nothing was worked around. Either allow reading
   `~/.cargo/registry/src/*/fearless_simd-*` (or allow `docs.rs`), or tell me to ship the `std::arch` version as a
   stopgap. `postings` works and is fast without it.
2. **The budget ceiling.** The design lands at about 27,000 lines, with a floor of about 24,500 and levers to about
   20,000 (PROPOSAL §7). Say if you want the levers pulled.
3. **Prorata basis semantics** (K3): whether a prorata sale carries basis per unit or by exact share. The lane keeps the
   current behaviour until you say.

## Known gaps, not hidden

- `cargo clippy --workspace -- -D warnings` already fails on the baseline: eight errors in `core` files nobody in v5 has
  touched (`day.rs`, `calendar.rs`, `num.rs`, `tree.rs`, `unit.rs`). Nothing new was added by any lane.
- `trail`: a mark kept across an undo to an earlier mark, once the trail has grown past it again, cannot be told from a
  good one. The module says so. A list of checkpoints must drop the marks it undoes past.
