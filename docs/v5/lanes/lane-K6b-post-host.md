# Lane K6b: a law of a kind, a purpose or an account derives a flow from a posted one

Read [`common.md`](common.md) first. Then [`K6-map.md`](K6-map.md) **all of it** (§0.1 and §0.6: what `also` is, and why a
derived flow cannot be an allocation after the fact; §5 "a posted flow": this lane is that host), and K6's report section in
`STATUS.md` (what K6 built: `Effect::Derive`, the one `Rules` index keyed by `Watch`, the occurrence host, the sugars through
`Derive`). Then `crates/engine/src/post.rs` (where laws fire after the value has moved), `fire.rs`, `promising.rs`
(`post_made`, how a promised occurrence's flows are posted and recorded), `state.rs` (`Recorded`), and K4b-map §10 (the group
and `solve`). Your worktree is `/home/user/axiom/.claude/worktrees/lane-k6b`, on branch
`claude/great-wozniak-pnqn7x-v5-k6b`. **This lane starts after K6 has merged.**

**Your crates:** `engine` (`post.rs`, `fire.rs`, `state.rs`, `ledger.rs` where a flow is made), `model` (`laws/` only where
the inert-`also` warning K6 added is lifted, and the `RuntimeTxn` for a derived flow), `report` (the views that list flows).

## What is wrong

After K6 a contract's `also` derives flows with its occurrence. A kind's, purpose's, entity's or account's `also` still does
nothing (K6 made it a warning): **a card's cash back, a processor's fee on every sale, an employer's match declared on the
plan, sales tax collected on a party kind** are all "when a flow of this kind happens, another one does", and LANGUAGE §10
promises them. They need a host that is not an occurrence: a law that fires on a flow that has **already posted**, and derives
another flow of its own, or a `+`/`-` item (the same ends or the reversed) beside it.

What makes it hard, from K6's map §5 (verify each against the code, do not take them from here):

- the derived flow needs a `RuntimeTxn` (a record of its own, so `why`, `register`, the views and the oracles can see it);
- a **guard against a derived flow firing the law that made it** (or any cycle of laws): cycles must be detected, not
  looped, and reported once with the chain;
- a derived flow is **not an allocation**: it cannot reduce what the flow already put in its owner's tally, so `share` and
  carved `sales-tax` stay where they are (the occurrence host); only a flow of its own and a `+`/`-` item are derivable here;
- where it sits in the order laws fire (`on out` at the source, relief, `on in` at the target, the purpose's laws, `on spend`,
  `always`): a derived flow posts after the laws of the flow that made it, and fires its own laws (bounded);
- the forecast: a forecast is the fold (K5c), so a promised occurrence's flows that match a kind's law derive too, with no
  second path;
- pending (`!`) flows, written-ahead flows, and returned flows: a returned flow reverses what it derived.

## What to build

1. **The host**, in the one function that posts, behind the one dispatch K6 built (`Rules::at(Watch)`): after a flow posts, the
   laws watching it fire as today, and a `Derive` effect adds the derived flows to a queue the fold drains in order. A
   derived flow carries its **cause** (`Cause::Derived(rule, flow)`: a typed edge, K7's provenance walk reads it).
2. **The record**: a derived flow is a real flow of the book's run (`Recorded`), listed by `register`, `flow` and `why`, with
   its origin shown ("derived by the law of kind `credit-card` from line 12").
3. **The guard**: a depth bound that is also a cycle detector (a law that derives into a flow it watches fires once per chain
   and says so: a diagnostic naming the two laws and the flow, with the chain as labels).
4. **Lift the K6 warning** for the kinds that now work, one test per owner kind (entity, account, kind, purpose, asset); delete
   what K6 wrote to say they were inert.
5. **A returned flow** reverses what it derived (the derived flows are returned with it, by cause), so a refund of a card
   purchase reverses its cash back. The K3c `Record::settled` map is the model of what must be remembered per flow.

## The proof

- **An oracle** (`docs/v5/measure/derived.py`, in the style of `derives.py` that K6 wrote): each owner kind's `also` beside
  the hand-written flow it stands for, equal on `check`, `flow`, `balance`, `tax`, `register`; plus cycles, depth, returns,
  pending and written-ahead; mutation-test it (every mutant killed by the oracle or a named unit test).
- Goldens and mistakes: byte-identical except what the examples' kind- or purpose-level `also` lines change (list them with a
  book that shows each). `fuzz.py ... diff`, K4b's `splits.py`, K6's `derives.py`: unchanged.
- Performance: `axiom check` on `bench/` 100k and 1m, three runs, fastest, load average: a book with no kind-level `also` pays
  one empty-slice check per posted flow at most (say what it costs in instructions; callgrind).

## Rules of this lane

- Common bar: functions under 40 lines, no bool parameters, no parameter bundles, no unsafe, no `Arc/Mutex/Rc/RefCell`.
  The derived queue is a plain `Vec` the fold owns and drains (not a recursion: the stack depth must not depend on the book).
- No test deleted or weakened. The K6 test that said "a kind's `also` does not fire" is rewritten to say what is true now
  (the commit says so).

## Step 0: the map

`docs/v5/lanes/K6b-map.md`, committed first: today's `post` order with line numbers; where the queue drains; `RuntimeTxn`
for a derived flow and what each view needs of it; the cycle rule and its diagnostic, drawn; what a return reverses; the
interaction with `promising.rs` and with pending flows; what Layer 3's `on start`/`on end` would need of the host
(K6-map §10), so that the next brief can be written.

## Not in this lane

- Carved items (`share`, `sales-tax` as an allocation of a posted flow): they stay in the occurrence host (K6-map §5).
- `on start`, `on end`, `part`, `joins`: K6's Layer 3; stop at the map.
