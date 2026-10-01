# Per-kind property defaults

Commit `595edbddaf706080725d42b5fcf2b32a44bf29dd` changes the model's default
storage from expanded inherited lists to assignments held only by the kind that
wrote them. When applying defaults, the model walks the kind lineage from the
oldest ancestor to the nearest kind, then applies the instance's own lines. A
single reusable path buffer serves each target's instances.

## Structural allocation comparison

For a chain of depth `D` with one default on every kind, the old representation
stored `1 + 2 + … + D = D(D+1)/2` assignment slots across its per-kind vectors.
The new representation stores `D` slots. At depth 128, that is 8,256 slots
versus 128. Building the old expanded lists cloned `D(D−1)/2` inherited
assignments (8,128 at depth 128); building the new buckets clones none. For an
instance, the old `settings` path also cloned its inherited assignments into a
temporary vector. The new path borrows those assignments while applying them.

These are counts derived from the two data paths, not wall-clock measurements.
Owned values still need copying into their destination where required, and
`Kind::inherit` still clones the cumulative `Kind.props` slice because the
current `Book` stores that cumulative slice on each kind.

## Reproduction

From `v2/`, run:

```sh
cargo test --offline --locked -p axiom-model --release deep_kind_defaults_keep_nearest_value_and_its_source_location
```

The fixture builds a five-kind chain. The root, an intermediate kind and a
nearer kind set the same `nickname` property; an instance without its own value
must receive `near`, while another instance's `instance` value takes precedence.
The test also checks each resulting property's `Loc` against the exact source
span of the winning line. The focused test and the worker's workspace release
tests passed. The coordinator's final aggregate totals, including doctests and
ignored tests, are recorded in [RESULTS.md](RESULTS.md).

`python3 v2/briefs/loc.py v2` counted 7,584 model and 22,327 workspace
non-test lines before the change, and 7,621 model and 22,364 workspace lines
after it. The documentation file is outside the counted `v2/` tree.
