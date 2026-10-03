# Lane K3d: a claim is recognized when the books say, a split payment settles by what the party pays, a debt is a parcel

Read [`common.md`](common.md) first. Then LANGUAGE.md §6 (`books`), §7 (claims: normative) and §9, then
[`K3c-map.md`](K3c-map.md) **all of it**, especially §0.3, §0.5, §6, §10 and §11 (this brief is its §11 and the three
things §10 says were not done), `K4b-map.md` §10 and §11 (a split statement is solved when its first flow lands; a
split's legs are flows from the source). Your worktree is `/home/user/axiom/.claude/worktrees/lane-k3d`, on branch
`claude/great-wozniak-pnqn7x-v5-k3d`.

**Your crates:** `engine` (`settle.rs`, `claims.rs`, `post.rs`'s purpose gating, `totals.rs`/`eval.rs` where a purpose is
counted), `model` (`traits`/`props` for `books`, `said.rs`, `book.rs`'s `Class` and `holds_parcels`), `report`
(`claims.rs`, `flow.rs`, `budget.rs`, `tax.rs`, the forecast's copy of the rule). Lane K5c (the forecast) and K3b/K6
(model) may run beside you: keep your forecast change to the one rule and rebase onto theirs.

## What is wrong

K3c made a claim relievable: write-off, settlement by a party's flow, `exact`. Three things it left, each shown on a book
in `docs/v5/measure/diff/cases2/`:

1. **Recognition.** LANGUAGE §7: "A claim's purpose is its recognition: an invoice is income when invoiced in accrual
   books, when settled in cash books (the owner's `books cash|accrual`, default cash)." Today `books` is parsed and read
   by **nothing**; a claim made with a purpose counts when made (accrual), and its payment, a flow with the same purpose,
   counts again: `claim-recognition.ax` has income **600** where either reading says **300**. Under cash the claim's
   making counts nothing and the settled part of the payment counts under the claim's purpose; under accrual the claim
   counts when made and the settled part counts nothing; a write-off in accrual books reverses what was recognized.
2. **A payment written as a split.** `fernhill -> 3_100 USD` with legs `business-checking 3_009.80` and
   `stripe 90.20 #business-fees` is a client paying a 3,100 invoice net of the processor's fee. K3c settles **by flow**,
   and the fee leg is a flow from the client to the processor, not to the owner, so it settles nothing: after K3c on K4b,
   `04-freelancer` lists `fernhill still owes 130.80 USD, 396 days past its due day` for an invoice that was paid in full.
   (`tests/golden/04-freelancer-claims.txt`: seven invoices show the fee as their remainder.) A payment settles by what the
   **party pays in all** in the transaction to the owner or on the owner's behalf: the header of the split, not the
   legs.
3. **A debt is a plain balance.** `Class::Debt.holds_parcels()` is false; `me owes pge 142.50 USD ^b1` is in `balance` and
   `claims` says "Nothing is owed either way"; a bill that is paid is not settled. It needs a parcel on the Debt-class
   tab (`holds_parcels`, `credit`, `balance`, `owed_by_you` and the `payable` gate of `claims.rs` change together).

## What to build

Phase order, each its own commits and each behind acceptance tests written first (`#[ignore]`d with the reason until
the code lands, as K3c did):

**A. Settlement by what the party pays in all** (small, do first). In `settle.rs`, a payment is the **flow, or for a
split statement the group**: the amount the party's place is debited in the transaction, to any place of the owner or
paid on the owner's behalf. Say in the map how the fold knows a leg's flow is part of one payment (`Made`'s group, K4b's
`Heading::Source`), and what a leg to a third party that is **not** on the owner's behalf is (nothing: a party paying
someone else settles nothing of ours). Acceptance: the 04-freelancer fee-leg invoices settle fully; a returned split
payment reopens them; a payment to a third party settles nothing. The oracle (`docs/v5/measure/claims.py`, K3c's) gains
split payments; mutation-test them.

**B. Recognition.** Read `books` once into the owner's traits (default `cash`, as §7 says). Then:
- cash: a claim made with a purpose recognizes **nothing** when made; the settled part of a payment recognizes the
  claim's purpose, on the payment's day; the unsettled part of a payment is an ordinary flow with its own;
- accrual: as today at making; the settled part of the payment recognizes nothing (an internal transfer);
- a write-off: accrual reverses what was recognized (a posting with the claim's purpose, the forgiven amount, and no
  movement, on the write-off day); cash has nothing to reverse;
- the purpose laws (`on flow`) follow the same gate, or a law fires on a flow that counts nothing;
- `flow`, `budget`, `tax` and the forecast read a flow's counted amount (a `Posted.settled` or the like), not recompute
  it; **one rule, in one place**, read by all four. Count the places that recompute a purpose's amount first.
- **When accrual counts is two statements in the repo**: LANGUAGE §7 says "when invoiced" (when the claim is made) and
  the doc of `Books::Accrual` and §6 say "when it is due". Build the one in §7 (it is normative and the simpler),
  as a single `enum AccrualAt { Made, Due }` read in one function so the other is one line, and **say in your report**
  that the two documents disagree and which you took. This is a decision for the user; do not hide it.
- **The default flips to cash**: which goldens move is unmeasured. Measure it first (a scratch build with the
  default flipped, `sh tests/golden.sh`), put the list **in the map before the code**, and keep the flip in its own
  commit with its goldens in another, so I can take or drop it. The examples that write a claim with a purpose are
  04-freelancer, 07-landlord, 08-expat, 09-shared, 11-sam; their income and tax will move, and the report says by how
  much and why.

**C. Debts as parcels** (only if A and B land inside budget; otherwise stop at the map with the design). A debt of the
owner's is a parcel on a `Debt`-class tab, so a payment to the party settles it by the same `exact`/`code`/`oldest`
order; `owed_by_you` and the `payable` gate go; `claims` lists a debt by the place saying `claim`. The failing probe is
`claim-debt-tab.ax` (`available` falls from 857.50 to 665.00 when the gate is dropped, counting the paid bill twice:
the acceptance is that it does not).

## Rules of this lane

- Goldens, mistakes, tests: byte-identical except what the phases list, each with its reason and a probe book. Phase B's
  default flip is the one big change; it goes behind its own measurement.
- No test deleted or weakened. No `unsafe`. Common bar: no bool parameters (use `AccrualAt`-style enums), functions under
  40 lines, no parameter bundles; `Request` is built by hand in three places today with mostly default fields (K3c's own
  note): give it a constructor per use (`Request::settling`, `::forgiving`, `::selling`) if that is what reading three
  copies asks for.
- A baseline binary from your starting commit; `fuzz.py ... diff`; K3c's claims oracle; K4b's splits oracle.

## Step 0: the map

`docs/v5/lanes/K3d-map.md`, committed before any code: what a posting's purpose counts toward and where it is counted
(every reader: `eval.rs`, `totals.rs`, `report/flow.rs`, `budget.rs`, `tax.rs`, the forecast: file, function, what each
recomputes); the measured list of goldens the cash default moves; how a split's legs are recognised as one payment; the
claim-debt design (what `Class::holds_parcels` gates).

## Measure

Lines per crate; the histogram; the list of changed outputs. The recognition rule should **delete** the readers' private
recomputation: report net lines per phase.

## Not in this lane

- Assets: no change (K3c-map §4). Prorata: the user's decision. Loans, deposits: K5d.
