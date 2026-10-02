# v5 status

Where the rewrite stands, and what is waiting on a decision. Read [`DESIGN.md`](DESIGN.md) for the design and
[`lanes/common.md`](lanes/common.md) for the standard every lane is held to. The integration branch is
`claude/great-wozniak-pnqn7x-v5`; each lane works on its own `-v5-*` branch and is merged here after review.

## Lanes

| lane | what | state |
|---|---|---|
| **C** core primitives | `tagless`, `dayset`, `sparse`, `postings`, `placement`, `trail` | **merged** (`8704e75`) |
| **K0a** model groundwork | `Staged`, one `problem` catalog, `Word::of`, one collect pass, dead code, short functions | **merged** (`5f99b98`) |
| **K0b** outer groundwork | owners, JSON writers, sync dates, sync apply, `RuntimeRange`, short functions | **merged** (`daf104e`) |
| **C3** `postings` SIMD | the `fearless_simd` block-compare kernel, kept only if it is 1.3× | **merged**: kept, 1.8-2.7× |
| **C2** `core::facts` | the store of timelines: `Key<V>`, painting `Builder`, frozen CSR `Facts`, `days_where` as an integral, sets as `Many` | **merged** (after K0a) |
| **K12** kinds, slots, facts | typed slots, `Taxonomy`, numbering the holders, moving every reader to `core::facts` | **merged** (`e5a3554`) |
| **K4a** one split vocabulary | `Quantity`, `Part`, `Expr`, `Group<H,F,I>`, one `Program` replace the two parallel Template*/Journal* families | running; has merged K12 |
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

1. **`fearless_simd` source access.** The registry source stayed unreadable to the lanes (a permission refusal) and docs.rs is
   blocked by the network policy; nothing was worked around. The API was recovered from our own scratch prototypes, and
   lane C3 built the `postings` kernel from them: no `unsafe`, 1.8-2.7× the scalar merge. If you would like lanes to be
   able to read the crate, allow `~/.cargo/registry/src/*/fearless_simd-*`.
2. **The budget ceiling.** The design lands at about 27,000 lines, with a floor of about 24,500 and levers to about
   20,000 (PROPOSAL §7). Say if you want the levers pulled.
3. **Prorata basis semantics** (K3): whether a prorata sale carries basis per unit or by exact share. The lane keeps the
   current behaviour until you say.

## K0a, in numbers

| | |
|---|---|
| model lines | 17,034 → 16,604 (−430). The brief's −2,000 target was missed: the catalog and the single collect pass saved about 390, and the splits that followed added signatures and context structs. The gain is in function length: functions over 80 lines in `model` went from 38 to 4, and the four left are K3 and K4's |
| goldens | unchanged. Mistakes 26, 60 and 98 changed in wording only (`duplicate-*` codes unified, listed in the lane's commit 5271942) |
| panics | K0a turned some parser-guaranteed diagnostics into `assert!`/`unreachable!`. 2,700 mutated corpus and example files through the old and new binaries: none panics in either |
| budget on a rerun | a lane that changes the parser (K12) may violate those invariants. The mutation fuzzers are in `docs/v5/measure/` territory: rerun them after K12 |

## What K0b found, routed

- `check` on `examples/02-household` takes 24.6 s, from `Ledger::sample_temporal_through` sampling daily. K12 deletes it.
- `totals::History::read` is 6.5% of a 100k `check`, mostly an edge-block scan over about 62 facts. A prefix sum per day
  removes it (about 5%). A `fearless_simd` kernel on structure-of-arrays columns gave only −1.6%, below the bar, so it
  was not kept. Belongs to K7's position steppers.
- A smaller or interned `Diagnostic` in `core` would remove the boxed-error aliases the groundwork needed.
- The CLI and report render cells twice. K7.

## K12, in numbers

| | |
|---|---|
| what it is | kinds declare typed, counted, weighted slots (`has NAME RANGE [MULT] [by WEIGHT]`) in one `Schema`; a property line fills a slot and is checked once; kinds and purposes share one `Taxonomy` builder; every property is a fact in `core::facts`; the fold reads place, entity and commodity traits from dense arrays resolved at plan build |
| deleted | `Assign`, `Prop` rows, kind defaults, about twenty cached fields. `Kind` 192 to 64 bytes, `Place` 152 to 88, `Entity` 176 to 88, `Commodity` 96 to 36 |
| lines | **+517 net against a target of −2,000.** The sampling machinery stays for balance conditions until K7 (only residence is integrated), `props.rs` is still 1,182 lines, and the new files carry their tests |
| tests | 882 passed, 3 failed (the residence test passes now), 16 ignored |
| goldens, mistakes | byte-identical. Fuzzed 1,000 mutated example projects against the pre-K12 binary and diffed output: 13 differ, all in the intended diagnostics below |

**Diagnostics K12 changed on purpose:** `property-value` and `duplicate-property-value` on a built-in property are now
`too-many`; a required slot left empty is `missing-role` (`rental-home` has no `land` or `in-service`); a word outside a
closed set is `wrong-word`; the taxonomy's help text reads "write `: income`, …"; `has` is no longer listed among a
thing's properties. `days(self.lives is X, window)` counts the whole window as the book states it, future steps
included; the sampled count projected today's state forward.

**What K12 left, for a K12b cleanup:**
- `props.rs` does three jobs (the grammar of the built-ins, the staging of `has` values, the system settings), and
  built-in properties are not parsed by the generic fill path: that would delete the `Args` readers (about 250 lines).
- Two reader styles side by side (typed keys, and a dynamic `Value`), and `Book.sites` keyed by a bare tuple.
- Weights are validated but not stored (`owners dana 60%, theo 40%` keeps the members): K3 and K6 need them.
- The facts are frozen twice, because `end` statements say `closed` after lowering.

## Lane C3, in numbers

| | |
|---|---|
| kernel | broadcast block compare, `u32x8`, a 2 KB packing table, about 80 lines, no `unsafe` |
| speed | 0.95-1.05 ns per id against 2.0-2.8 scalar, at 10⁴ to 10⁶ ids, 1% and 50% shared (median of five runs, worst single run 1.39×) |
| the bar | 1.3×: cleared |
| gap | the AVX-512 path was measured on the old host (3-4×) but not with the final code: this host reports AVX2 |
| side effect | galloping now starts at skew 32 instead of 8, because the block merge beats it below that |

## Lane C2, in numbers

| | |
|---|---|
| lines | +507 non-test in `core`; `facts.rs` is 757 lines before its tests, 439 of them code |
| size | 26.5 bytes a step, 135 MB for 5.1M statements over a million holders; nested vectors: 61 bytes a step, 4.0M allocations |
| build | 275-550 ms on 4 cores for 5.1M statements; 1.4-1.7 s for the nested layout |
| `days_where` | 9-97x faster than sampling every day, and exact |
| a read | warm sweep 15.5-17 ns (nested 22-25); **cold random read 55-70 ns (nested 47-52): the layout loses 15-25% cold**, so per-event reads must be resolved at plan build |
| tests | 31 unit, 3 doc, a 2,550-book property test against a per-day model, ~20 hand-made mutants all caught |

## Operational note

Sonnet's session limit was hit once (about 08:40 to 10:40 UTC): lanes C2 and C3 died mid-work and were resumed from their
transcripts. Their worktrees kept their uncommitted changes. Keep at most three lanes running.

## Known gaps, not hidden

- `cargo clippy --workspace -- -D warnings` already fails on the baseline: eight errors in `core` files nobody in v5 has
  touched (`day.rs`, `calendar.rs`, `num.rs`, `tree.rs`, `unit.rs`). Nothing new was added by any lane.
- `trail`: a mark kept across an undo to an earlier mark, once the trail has grown past it again, cannot be told from a
  good one. The module says so. A list of checkpoints must drop the marks it undoes past.
