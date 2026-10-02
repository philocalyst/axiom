# Lane K3c: a claim is a parcel, settling it is relief; an asset's part may be one too

Read [`common.md`](common.md) first. Then [`../PROPOSAL.md`](../PROPOSAL.md) §3 F5 and §5 K3,
[`../DESIGN.md`](../DESIGN.md) §2.6 and §3.6, and LANGUAGE.md §7 (claims) and §9 (assets), which are normative. Then the
finished maps of the lanes before you (`K3a-map.md` section 10 says what a tab is and what K3c inherits; `K4a-map.md` and
`K4b-map.md` set the standard for yours). Your worktree is `/home/user/axiom/.claude/worktrees/lane-k3c`, on branch
`claude/great-wozniak-pnqn7x-v5-k3c`.

**Your crates:** `engine` (`ledger.rs`'s fact handling, `lots.rs`, `post.rs`, `assets.rs`, `assets_runtime.rs`, `fire.rs`,
`explain.rs`) and the readers in `report` (`claims.rs`) and `model` (`lower/statements.rs`'s claim change). Lane K5b will
later change how promises reach the fold: do not touch `ledger.rs`'s occurrence code.

## What is wrong

**Claims.** A claim is already a lot in a claim position: `finish()` reports `explain::overdue` from lots with `qty > 0`,
and the claims view reads those lots. That is the right model. What is missing around it:

- `ledger.rs` handles a claim write-off as `Fact::ClaimChange(_) => {}`: the `waived` statement is read, lowered into
  `Book::claim_changes`, ordered in the timeline, and then **discarded** by the fold.
- LANGUAGE §7 settles a flow against claims by code, then **the claim whose open amount is exactly the flow's**, then the
  oldest first. Relief by code exists (`Select::Code`); exact-amount settlement is not a relief policy, and where it is done,
  it is done beside the lots, not through them.
- `report/claims.rs::owed_by_you` and `Role::Tab` tests in `said.rs::is_claim` and `claim_target` ask what kind of place
  something is where the place could say it (K3a, section 10).

**Assets.** An identified thing (a house, a laptop) is a place with a parallel store, `Assets`/`AssetState`
(`assets.rs`, 871 lines, with RAII guards `ConsumptionGuard`, `CarryGuard`, `AssetBasisBatchGuard`; `assets_runtime.rs`, 304),
that keeps its **parts** (the purchase, each improvement), their basis, what each has been depreciated by, disposals with
their boundaries and pending wash-sale carries. `Parcel.part: Option<PartId>` already ties a lot to a part, so there are
two stores of one fact.

## What to build

### 1. Claims (the part that must land)

- **Write-off is relief.** `Fact::ClaimChange` reduces the target claim's parcels, as a settlement by a flow does, and is
  recorded as a waiver with its description and purpose, so `claims`, `check` and `explain::overdue` see it. The statement
  grammar and the lowering exist; the fold half is what is missing.
- **Settlement is one relief policy order** (code, exact amount, oldest), as `LANGUAGE §7` says: an `Exact` selector beside
  `Code`, `Fifo`, `Lifo`, `Hifo`, `Prorata` in `lots.rs`, and the existing exact-amount shortcut deleted where it sits
  outside the relief code.
- **Aging is the parcel's day, and blame is the class.** Nothing new to store: say in the map where aging is computed today.
- `report/claims.rs`, `said.rs::is_claim` and `claim_target` ask the place, not its role (the smallest `Role` change in
  `K3a-map.md` section 10: give a tab a `claim` kind).

Acceptance tests come first, written from LANGUAGE §7 and the behaviours above, `#[ignore]`d with the reason until the code
that makes them pass lands (the way K4b did): a write-off reduces what is owed and shows in `claims`; a flow settles the
claim whose open amount equals it before an older one; otherwise the oldest; a code beats both.

### 2. Assets (feasibility first)

The design (DESIGN §3.6) is that an asset is a position holding one parcel per part, so that:
- depreciation (`consume`) is a rebase of a part's parcel;
- a wash-sale carry is a rebase of the matched parcel;
- a sale relieves every part;
- and `AssetState`, its guards and about 1,000 lines go.

**Do not assume it holds.** A part carries things a lot does not (its kind, what it has consumed, a disposal boundary, a
pending carry). Step 0 must answer: for each field of `Part`, `Disposal`, `PendingCarry`, `AssetState`, is it (a) already a
field of a parcel, (b) derivable, or (c) real state a parcel would need to grow? If (c) is more than a few fields, or if the
unification would make the lot code (`lots.rs`, the best code in the engine: do not weaken it) branch on "is this a part",
**stop at the map**, report the evidence, and do only section 1. A well-argued "no, and here is the smaller cut" is a good
result. If it holds, do it behind the oracle below.

## Rules of this lane

- **No behaviour change outside claims write-off and exact settlement**, which change because they were unimplemented or
  inert (`Fact::ClaimChange(_) => {}`). List every golden or test output that changes in your report with the reason. Asset
  parts: no change at all (`tax`, `gains`, `lots`, `why` of the investor and landlord examples are the proof).
- No test deleted or weakened. The failing prorata test (`a_prorata_place_realizes_only_the_lots_share_…`) is yours to
  look at: it is about arriving basis per position kind. Say what the right rule is and whether this lane fixes it. If the
  right rule is a **decision for the user** (prorata basis semantics), do not decide it: describe the two readings and what
  each changes.

## Step 0: the map

`docs/v5/lanes/K3c-map.md`, committed before any code: where each of the following lives and is read: claim creation,
settlement (by code, exact, oldest), write-off, aging, `owed_by_you`; every field of the asset store; the lot fields; what
`Fact::ClaimChange` carries; the order of same-day facts (`timeline.rs`) and why `ClaimChange` is after movements. And the
feasibility answer for assets, field by field.

## Verification

An oracle for the engine's claims and lots as in K4a: a seeded generator of books with claims (invoices, loans, deposits,
partial settlements, equal-amount ties, write-offs) and an engine dump of every parcel and its history, compared against a
baseline binary built from your starting commit. Mutation-test it. Then `cargo test --workspace --release --no-fail-fast`,
the goldens at the end of each step, `fuzz.py ... diff` on mutated claims books.

## Measure

Lines per crate before and after; the function-length histogram; for assets, what was deleted. This lane may add lines for
claims (the write-off is new code) and delete for assets: report the two separately.

## Not in this lane

- Addresses and paths: K3b.
- The promise monitor, which creates a claim when a `Due` passes with nothing kept: K5b.
- Position histories and `peak`/`low` without sampling: K7.
