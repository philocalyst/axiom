# Lane C3: the `fearless_simd` kernel for `core::postings`

Read [`common.md`](common.md) first, then `crates/core/src/postings.rs` and its tests: lane C wrote the scalar version
and benchmarked it. Your worktree is `/home/user/axiom/.claude/worktrees/lane-c3`, on branch
`claude/great-wozniak-pnqn7x-v5-c3`.

**Your crate:** `core`, and only `postings.rs`, plus `fearless_simd = "1"` in `[workspace.dependencies]` of the root
`Cargo.toml` and in `crates/core/Cargo.toml`. Lane C2 is adding `facts.rs` to `core` at the same time: do not touch it,
or `lib.rs`.

## The one place the design allows `fearless_simd`

`intersect` merges two sorted `u32` lists of similar length with a branchless scalar loop: 2.4 to 3.3 ns per id of
both lists, on lists of 10⁴ to 10⁶. Lane C measured a hand-written `std::arch` block-compare kernel at **1.7 to 2.4×**
that on the same lists (1.4 to 1.65 ns per id), so the headroom is real and this lane builds it with `fearless_simd`
instead. Its prototype is in
`/tmp/claude-0/-home-user-axiom/d0ff8c60-72e7-58b1-a59f-e3f4cba55dfe/scratchpad/simd-exp/src/main.rs`: read it for the
algorithm, which compares a block of one list with every id of a block of the other, collects which ids matched as a
bit mask, and advances the list whose block ends first.

## How `fearless_simd` is used here

You cannot read the crate's source from the cargo registry, and `docs.rs` is blocked. Do not try to get around either.
The API is shown by working code that this project already has in its scratchpad (read these three files; they are our
own prototypes, and they compile against `fearless_simd = "1"`):
- `/tmp/claude-0/-home-user-axiom/d0ff8c60-72e7-58b1-a59f-e3f4cba55dfe/scratchpad/k0b/simd/src/main.rs`: `Level::new()`,
  `dispatch!(level, s => expr)` to run a generic kernel at the best level the machine has, `fn kernel<S: Simd>(simd: S, …)`,
  `S::i64s::LEN`, `splat`, `from_slice`, `store_slice`, and the operators on vectors;
- `…/scratchpad/simdbench/src/main.rs` and `…/scratchpad/s5/simd/syntax/src/lex.rs`: fixed-width vectors (`u8x32`,
  `u8x16`), `simd_eq(...)` giving a mask, and `.to_bitmask()`.

What you need follows from those: broadcast one id of one list with `splat`, compare it with a block of the other with
`simd_eq`, OR the masks of a few broadcasts, `to_bitmask`, and walk the set bits with `trailing_zeros`. That is the
"broadcast" kernel of Lemire, Boytsov and Kurz, "SIMD compression and the intersection of sorted integers", SPE 2016,
and it needs no shuffle, which this API shows no example of. Use a shuffle or rotate only if you can find it
the same way. If a type or method you want does not compile, find it by trying names the prototypes' neighbours suggest
and by the compiler's own errors, and stop at what the compiler tells you.

## What to build

1. The kernel, generic over `S: Simd`, in `postings.rs`, used by `merge` when both lists are long enough to fill a
   block. A scalar tail finishes the rest, and the scalar loop that is there now stays as that tail and as the
   fallback.
2. **Dispatch once.** `Level::new()` once for each call of `intersect`, not each block, or cache it if you can do that
   without a global; say which and what it costs, measured.
3. **Keep it only if it earns it.** On lists of 10⁴, 10⁵ and 10⁶ ids of similar length, with 1% and 50% shared, it must
   be at least **1.3×** faster than the scalar merge. The benchmark exists (`cargo test -p axiom-core --release
   postings::tests::bench -- --ignored --nocapture`); extend it to print both. If it does not reach 1.3×, delete the
   kernel and the dependency, leave `postings.rs` as it was, and report the numbers: that is a fine result.
4. **Tests.** The existing property tests against `BTreeSet` must run through the kernel as well as the scalar loop (a
   crate-private switch, or a second function, so that a test can ask for each). Add an equivalence test of the two on
   at least 200,000 random cases, with lengths from 0 to 200, lists that end exactly at a block boundary, ids near
   `u32::MAX` and `0`, and lists that share nothing, everything, and every other id.
5. `cargo fmt --check` clean; `cargo test --workspace --release --no-fail-fast` has the same failures as before and no new
   ones (`cargo build` will fetch `fearless_simd`: the proxy allows crates.io).

## Not in this lane

Nothing outside `postings.rs` and the two `Cargo.toml` lines (and `Cargo.lock`, which cargo updates).

## Report

The usual from `common.md`, plus the before and after table of ns per id for each size and overlap, the block width you
chose and why, and `unsafe` count (it should be zero: `fearless_simd` is the point).
