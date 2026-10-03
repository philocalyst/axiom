# Lane K3b: an address is a definite description, and a declaration's words fill its slots

Read [`common.md`](common.md) first. Then [`../DESIGN.md`](../DESIGN.md) §2.4 (addresses), §3.3 (`Addresses`: posting
lists), §3.4 (forced placement), [`../research/ASSOCIATIONS.md`](../research/ASSOCIATIONS.md) (the user's complaint, and
the ladder of precision that the design answers it with), and the finished maps of the lanes before you: `K3a-map.md`
(positions are made when something asks), `K3c-map.md`. Your worktree is `/home/user/axiom/.claude/worktrees/lane-k3b`,
on branch `claude/great-wozniak-pnqn7x-v5-k3b`.

**Your crates:** `syntax` (the declaration and reference grammar, only as far as the map says), `model` (`names.rs`,
`resolve.rs`, `scope.rs`, `declare/`, `holders.rs`, `lower/`), and `core` (`placement.rs`, `postings.rs`: built, used by
nothing yet).

## What is wrong

The user's words, about `examples/05-family/accounts.ax`: *"the jordan-401k is proof that the typing is still a little
weak, should be easy and declarative to setup associations for accounts."*

```text
account alex-401k : 401k at fidelity
  employer acme
account jordan-401k : 401k at fidelity
  owner jordan
  employer bluefin
account riley-529 : 529-plan at fidelity
  owner family
  beneficiary riley
```

The name `jordan-401k` encodes a relation (jordan owns it); the `owner jordan` line says it again, and nothing checks the
two agree. A reference in a journal says `jordan-401k`, a string that has to be remembered. K12 gave kinds typed, counted
slots (`has owner person`, `has sponsor agent`), and `core::placement` (forced placement) was built and tested alone; **no
declaration uses it**, and no reference can name a thing by what it is related to.

## What to build

1. **A thing's canonical address** is the path of its slot fillers, in the kind's slot order, then its name:
   `jordan/bluefin/401k`. **`Addresses`** is the index: for each filler, a posting list of the things it fills a slot of
   (`core::postings`, galloping intersection, the SIMD block kernel where lists are long); resolving a reference is
   intersecting the lists of its words, then filtering to what is open on the line's day.
2. **A reference is any subsequence of an address that denotes exactly one thing open on the line's day** (Russell's ι):
   `jordan/401k` is *the* 401(k) with jordan among its fillers. None: `unknown-address`, offering the nearest name.
   Several: `ambiguous-address`, listing each candidate's **shortest unique address**; that list is also the quick fix.
3. **A declaration's words fill slots by forced placement** (`core::placement`): `account jordan/bluefin/401k : 401k`
   places `jordan` in `owner` and `bluefin` in `employer`/`sponsor` because each word's kind fits one slot. A word that
   could land in several is `ambiguous-placement`, naming the slots and the role word that settles it
   (`jordan/401k employer bluefin`). Nesting fills the slot marked `as with`. A kind's name stands for the name
   (`alex/401k`); a different name needs the kind (`family/checking : deposit`). Entities and assets keep flat names.
4. **The ladder holds** (R5): `alex/401k` → `jordan/bluefin/401k` → `jordan/401k sponsor bluefin` → every slot written as
   today. The old spelling (`account jordan-401k : 401k`, `owner jordan`) keeps working: this is **additive**, and `fmt
   --upgrade` (lane L) is what rewrites old to new.

**The acceptance target** is `examples/05-family/accounts.ax`: write a **copy** of it (`examples/explore-v5/` or a test
fixture; do not edit the example the goldens read) in the new spelling, with every journal reference the shortest stable
one, and show it **checks to the same balances, tallies and claims** as the original. That is the proof the typing got
stronger, and the demo the user will read.

## What I need from your map before you write code

The grammar is the risk. `a/b/c` is already a hierarchical path in today's names, so the map must say precisely what
`jordan/bluefin/401k` means to the parser and the resolver **today**, and what in `syntax` would have to change, if
anything. If the model can do it with no grammar change (words of a path are looked up as fillers first and as tree
segments second), say so and stop at that. If the grammar must change, **stop at the map** and describe the smallest
change with an example of each ambiguity it introduces: I decide before any grammar edit.

## Rules of this lane

- **No behaviour change for a book that does not use the new spelling.** Goldens, mistakes, tests, byte-identical; the
  three known failures. New diagnostics (`unknown-address`, `ambiguous-address`, `ambiguous-placement`) are for the new
  spelling only, and are written to the standard of the mistakes corpus (`tests/mistakes`: a label, a note, a help that
  fixes it; add a case for each, with the expected output).
- Nothing a **name's meaning depends on** is order of declaration: resolving a reference never depends on what was
  declared after it unless it is open on the line's day (the line's day decides, as a `from`/`until` of a relator does).
- A baseline binary from the starting commit; K0a's diff harness (`docs/v5/measure/diff/`) and `fuzz.py ... diff` for the
  old spelling; for the new, a generator (`docs/v5/measure/addresses.py`, in the style of `splits.py`) of books with
  random slot fillers: every reference at every rung of the ladder resolves to the same thing as the full spelling, or
  is ambiguous exactly when two things share the subsequence. Mutation-test it.
- Common bar: functions under 40 lines, no bool parameters, no parameter bundles; the index is borrowed and built once.

## Step 0: the map

`docs/v5/lanes/K3b-map.md`, committed before any code: every place a name is resolved (file, function) and from what
(tree path, entity name, party, holding, `with`, `at`, `owner`); what `holders.rs` and `scope.rs` do; how today's
declaration parser reads the words after the name; which slot of which built-in kind each of today's relation lines
(`owner`, `at`, `employer`, `coverage`, `beneficiary`) is; the cost of a lookup today and of the intersection; what the
facts store knows about slots (K12) that the index would reuse rather than copy.

## Measure

Lines per crate before and after; the histogram; the lookup cost on `bench/` at 100k and 1m (`axiom check` time must not
move). This lane adds code; say what it will let `L` delete (the `owner` lines of every example).

## Not in this lane

- Relators and projection (an employment's legs written once, a membership): K6.
- Rewriting the examples to the new spelling: L. The positions-under-agents syntax (`jordan/bluefin/401k` as the thing
  the paycheck pays into by default): L.
