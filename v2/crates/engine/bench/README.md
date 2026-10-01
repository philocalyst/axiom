# Kind-widened total lookup microbenchmark

This small standard-library-only benchmark isolates one part of
`total(in|out, window, KIND)`: checking each place's kind and owner versus
iterating a precomputed place list for the requested kind. The data represents
500,000 places, with 31,264 under the queried kind and descendants, and runs
300 totals per sample. It checks that both paths produce the same sum.

From `v2/`, compile and run it with:

```sh
rustc --edition=2021 -O crates/engine/bench/kind_total_lookup.rs -o /tmp/kind_total_lookup
/tmp/kind_total_lookup
```

The executable starts its timers only after setup, so Rust compilation is not
included. It reports index construction time and the candidate-index vector's
capacity in bytes. The timed comparison is a synthetic lookup-only benchmark;
it does not measure `Plan` construction, law evaluation, or whole-engine
performance. Its `HashMap` and `usize` membership vector approximate the
production sparse index but do not reproduce its exact allocator or `FxHasher`
costs. One run in the shared October 1, 2026 executor reported a 377 µs median
index build, about 256 KiB of candidate storage, and a 20.78× median paired
lookup speedup (266.06 ms scan, 11.66 ms indexed, 300 repeated reads). Earlier
unpaired runs varied from 5.49× to 30.12× under executor load, so treat these
figures as evidence for this lookup shape rather than a whole-engine claim.
