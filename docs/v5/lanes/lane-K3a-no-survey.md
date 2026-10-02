# Lane K3a: positions that need no prediction

Read [`common.md`](common.md) first. Then [`../PROPOSAL.md`](../PROPOSAL.md) §3 F3 and §5 K3, and
[`../DESIGN.md`](../DESIGN.md) §2.4 and §3.3. Your worktree is `/home/user/axiom/.claude/worktrees/lane-k3a`, on branch
`claude/great-wozniak-pnqn7x-v5-k3a`.

**Your crates:** `model` (`lower.rs`, `declare.rs`, `declare/`, `book.rs`'s `Role`, `lower/`), and the readers of `Role`
in `engine` (`eval.rs`), `report` (`register.rs`), `sync`. Lane K4b works at the same time on the engine's split
resolution (`ledger.rs`) and `model/lower/flow.rs`'s `lower_items`: stay out of those.

## What is wrong

A claim, a promise and a `for` clause each need a **tab**: a place for what a party owes an owner. Tabs have no source
path; their identity is `(party, owner, class)`. But the place tree is frozen in pre-order, once, **before** lowering,
so every tab has to be known first. Two walkers over the whole journal predict which tabs lowering will want:

- `lower::survey` (`lower.rs`, 838 lines in all): scans every transaction, statement, claim, contract and tail into
  `Mention`s (`Claim`, `Promise`, `For`, `Due`, `Ends`);
- `lower::visit_endpoints`: a second walk, over a 14-variant `EndpointContext`, that collects every name an endpoint
  mentions;
- `declare/holdings.rs::find_tabs` turns the mentions into `TabDraft`s, and `declare.rs::contract_endpoints` does the same
  for loans.

If the prediction misses one, lowering fails with `World::tab() -> unregistered-tab`. That is about 500 lines whose only
job is to guess what a later phase will ask for, and a diagnostic that says "the model's own two phases disagree". The
engine and the reports never see it; it is purely a phase-ordering artefact.

## What to build

Positions are created **when something asks for them**, after the tree is frozen. There is then nothing to predict, and
the survey, the two walkers, `find_tabs`, `contract_endpoints`, `Mention`, `TabDraft`, `World::tab`'s registry and the
`unregistered-tab` diagnostic all go.

The design question is how a tab lives beside a frozen pre-order tree. Decide it from evidence, in your map (below),
not from this paragraph, but know the candidates:

1. **A trailing root.** `Tree<Place>` is pre-order, so a subtree is a range of ids. If the tab root is the *last* root
   of the tree, the tabs are the tail of the id space, and appending a leaf under it extends the root's range without
   moving any other id. This keeps `Id<Place>` one dense space for the engine and the reports, which index dense arrays by
   it. It needs one operation on `core::Tree`: append a child to the last root.
2. **A second arena of tabs** with their own id type. That splits every `Id<Place>` consumer in the engine and report, and
   is almost certainly worse.
3. **Not in the tree at all**, with the tree's queries (`is under`, a subtree range) never asked of a tab. Check whether
   any code asks them of a tab.

Whatever you choose, find out first what is **sized by the number of places** during lowering (a `Groups<Place, _>`,
a `Vec` by place id, `rules.rs`'s indexes) and so would break if the count grows while lowering runs. Say in the map what
you move after lowering, and why that is safe.

## Rules of this lane

- **No behaviour change.** Goldens and mistakes byte-identical (they exercise tabs in `balance`, `claims` and `register`),
  the same three known test failures, no test deleted or weakened.
- **No new `Id` order visible to a user.** The order in which places are listed in a report is today the tree's pre-order
  with tabs in the order the survey found them. If a lazy creation order changes that, make the report order explicit
  (sort by what a reader sees) rather than leaving it to id order, and say so.
- Where the survey's prediction was **wrong** (found a tab nobody uses, or merged two that lowering keeps apart), this
  lane may change a golden: list each change, with the reason, in your report and in the commit.

## Step 0: the map, before any code

Write `docs/v5/lanes/K3a-map.md` and commit it first, as K4a did (`docs/v5/lanes/K4a-map.md` is the standard):
1. every place a `Role::Tab` is created, looked up or read (file, function): model, engine, report, sync;
2. what each `Mention` becomes, and which tab it asks for: a table of mention to `(party, owner, class)`;
3. what is sized by the place count while lowering runs;
4. whether anything asks a tab for its subtree, its path, or its parent;
5. the order tabs are created in today, and where that order is visible in output.

## Step 1: lazy creation

Implement the chosen design. A lookup that finds no tab makes it. The key stays `(party, owner, class)`.

## Step 2: delete the prediction

Delete `survey`, `visit_endpoints`, `EndpointContext`, `Mention`, `JournalSurvey`, `find_tabs`, `TabDraft`,
`contract_endpoints` (or what remains of it if the engine reads it), the `Said.survey` field, `unregistered-tab`. The
declarations' diagnostics that came from walking endpoints (`unknown-entity` for a name only a journal mentions) keep
firing, from lowering, in the same words: check this with the mistakes corpus and the K0a differential harness.

## Step 3: what `Role` is for

With tabs lazy, look at the rest of `Role` (`Account`, `Holding`, `Outside`, `Issuer`, `Tab`, `Asset`). The design (§2.4)
wants a position to be `(owner, with, kind)`. Do not build that here, but say in the report whether `Role` as it stands
blocks it, and what the smallest change would be.

## Verification

At each commit that touches model or engine: `cargo fmt --all`; `cargo test --workspace --release --no-fail-fast`; the
mistakes corpus; `python3 docs/v5/measure/fuzz.py OLD NEW examples SEED 1000 diff` with `OLD` a binary built from your
starting commit (a difference is a failure unless it is a change you listed); K0a's `docs/v5/measure/diff/` harness; and
K4a's `splits.py` oracle. `sh tests/golden.sh` at the end of each step (it is slow). Then `git diff tests/` is empty
except for what you listed.

## Measure

Lines before and after per crate; the function-length histogram. The target is about **−600 lines**, almost all in
`model/lower.rs` and `model/declare/`.

## Not in this lane

- Paths that fill slots, addresses as definite descriptions (`jordan/bluefin/401k`), the `Addresses` index: K3b.
- Claims and assets as parcels, settlement as relief, deleting `AssetState`: K3c.
- Relators and projection: K6.
