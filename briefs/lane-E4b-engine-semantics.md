You are **lane E4b (engine v4: semantics)** of the Axiom v4 rework. First read `v2/briefs/common.md`. It is part of this brief.

**Read first:**
- `v2/LANGUAGE.md` (v4, normative), in full, and especially:
  - §0 and §2 (ends, purposes, line items, pass-through legs, pairing);
  - §3 (statements, and changes that hold from their day);
  - §5 (contracts, and terms that change);
  - §6 (laws: `on flow`, `consume`, `carry`, tallies, `total(#P, window)`);
  - §7 (claims and settlement), §8 (parcels and assets), §9 (derived events), §11 (diagnostics).
- `v2/DESIGN.md`, in full.
- The worked example `v2/examples/v4-sketch/`: its README says what `check` and `why condo` print, including the assertion diagnostic you will build.
- The public types in `crates/model/src/{book,journal,law}.rs` and `crates/engine/src/lib.rs`: lanes I4 and C5 put them there. The annotated sources are `v2/briefs/types-v4.rs` and `types-v4b.rs`.
- `v2/briefs/limabean.md`: the benchmark our diagnostics beat.
- The engine as lane E4a left it: `Plan`/`Ledger`, `LawFacts`, `Verdict`, `Suspect`.

**You own `v2/crates/engine/**`.** The model will build real v4 books later: the v3 model still builds today's books and leaves the new fields empty.
- Build and test every v4 semantic through hand-made books in `src/fixture.rs` and `src/tests.rs`.
- Keep every v3 behaviour and golden intact: v3 books must keep working until the v4 model lands.
- Change a public type only if the design cannot be met otherwise, and say so.

## What the engine must do

1. **Flows carry an owner and a purpose.**
   - Tallies key by `flow.owner`, with the household rule unchanged: a member governed as one counts into the household.
   - `Var::Purpose` and `Var::Description` evaluate.
   - `is` accepts a purpose (the tree test), and a purpose with its object (`purpose is repair of self`).
   - `total(#PURPOSE, window)` reads that purpose's total for the subject.
2. **Purpose laws** (`Trigger::Flow`).
   - For every posted flow with a purpose, fire `Rules.purposes[purpose]`, whose ancestors are already included. The subject is the flow's owner, as a `Subject::Entity`.
   - For a flow whose purpose is `of` an asset, also fire `Rules.about[asset's place]`.
   - `total(window)` under a purpose is that purpose's total for the subject in the window, by recognition. It is signed so that the purpose's own direction is positive: money out for spending and capital, money in for income, so a refund subtracts.
   - Watch only the purposes some law reads, as `Totals` watches places.
3. **Budgets** (`Book.budgets`, LANGUAGE §4).
   - The cap path reads the budget's `limits: Timeline<Limit>` for the window being judged. It is the limit in force on the window's first day; say so in a doc comment, and let `why` show every change.
   - `Limit::Share` is that share of the other purpose's total in the same window.
   - `carries` judges the running total since the budget's first window against the sum of its limits through this one.
   - Headroom is recorded as for every cap. Extend `LawFacts`' `Cap` shortcut to budgets, so a budget that holds costs no evaluation.
4. **Properties by day** (LANGUAGE §3). A field read (`.in-service`, `land`, any declared property) takes `prop(props, name, day)` for the day being judged.
5. **Assets.**
   - An asset's place holds its one unit (`Asset.unit`).
   - The arrival of the unit makes part 0, from a flow `#purchase of ASSET` or an opening or `basis` statement. Its cost is the base value paid, including the costs of exchange and the derived sales tax; its day is the flow's.
   - A flow whose purpose has root `Capital`, and is `of` an asset, adds a part. Its cost is the flow's base value; its day, the flow's.
   - `consume EXPR`, under a law of an asset or asset kind, lowers the current part's basis and the parcel's, and records an `Adjustment::Consumed`. It never goes below zero, and what it could not consume is a diagnostic.
   - Timed laws of an asset kind run once per part, and fields resolve on that part:
     - `.cost` is the part's cost;
     - `.basis` is what remains of it;
     - `.in-service` is the asset's property for part 0, and the part's day for later parts;
     - declared properties such as `land` apply to part 0 only.
   - `#sale of ASSET` relieves the unit with the sum of the parts' basis. The gain is the proceeds less the sale's costs (`-` items whose purpose is a cost) less that basis. It sets `AssetState.disposed`. `Derivation::Disposal` does the same for nothing.
   - `Run.assets` lists every asset's parts.
6. **`carry EXPR to UNIT within SPAN`** (a wash sale).
   - It holds a disallowed loss from the triggering sale, and adds it to the basis of the nearest acquisition of `UNIT` by the same owner within the span, before or after the sale.
   - A later acquisition receives it when it arrives; an earlier one receives it now, if its parcel still exists.
   - The receiving parcel's holding period then starts at the sold parcel's acquisition day.
   - Record an `Adjustment::Carried`. A loss nothing receives within the span stays allowed.
7. **Parties, tabs and the market.**
   - `Class::Outside` places are parties. Value reaching one has left the owners, so realization and `on spend` behave as for v3's expense places; value from one is new.
   - Claims are tabs (`claim` places).
   - The market is the entity `roots.market`: a flow between an asset-class place and the market is a revaluation.
   - Remove every remaining `Class::Income`, `Expense` and `Equity` assumption.
8. **Claims** (LANGUAGE §7). A flow from a party with open claims settles:
   - first those its codes name, in order;
   - else the one whose open amount is exactly the flow's;
   - else the oldest first.
   
   What remains is an ordinary flow. `PaidFor` and `WriteOff` flows arrive derived from the model: post them. A write-off in accrual books reverses what was recognized.
9. **Promises** (LANGUAGE §5, `Run.promises`).
   - For each contract, `Contract::due_days(within)` gives the expected occurrences, segment by segment of its terms; waived segments expect nothing.
   - Match them with the journal's occurrences (`Txn.contract`): each due day is kept by the nearest unmatched occurrence within half a cadence.
   - A due day on or before `today`, kept by nothing, is missing: `warning[late]`, naming the contract, the party, the amount as the terms stood that day, and the days late. When the promise is the party's (income: rent, pay), it is a claim on the party while missing.
   - Occurrences of `estimate` terms are never compared by amount.
   - A kept-late occurrence is recorded with its lateness, and no diagnostic.
   - Past `days.last()` nothing is expected.
   - `Run.promises` lists them all. Forks (forecasts) carry the state.
10. **Derived flows arrive from the model**: interest, principal, escrow, match, shares, sales tax, exchange cost, pass-through halves, paid-for, write-off, disposal, and line items (which are written flows).
    - Post them like any flow, and never derive them twice.
    - A pass-through pair (party → owner → party) counts as wages to the owner and as tax paid by the owner, with no net holding left behind.
    - A loan's schedule comes from the model (interest per occurrence, given every earlier one, and each rate change). A flow to the contract (`checking -> mortgage 1_000 USD`) pays principal alone.

## Revision: the spec at 5f15e35 and types-v4c

Read LANGUAGE.md at 5f15e35 or later in full: the spec was revised after this brief was first written, and it wins where they differ. Read `theory.md` for the ideas behind promises and norms. On top of the list above:

11. **Promises are one monitor** (LANGUAGE §7; CSL's residuation).
    - A contract's due days and a claim's due day are the same thing to the engine: an obligation with a deadline, someone to blame, and what settles it. `grace` replaces the half-cadence rule; its default is half a cadence.
    - `due SPAN else ITEM` adds its item when the deadline passes (`Derivation::Otherwise`): a late fee is a claim that grows.
    - `Run.promises` and the claim tabs must not be two bookkeeping systems. Find the one structure that is both.
    - Deposits, and missed occurrences, are open obligations of the same kind.
12. **`also` is evaluated by the engine** (the model compiles and indexes it by `AlsoOn`).
    - For each posted flow, find the `also` lines of its contract, its party, its party's kind chain and its purpose chain.
    - Evaluate each `when` and amount on the law evaluator, with the flow in context (`amount`, `gross`, `from`, `to`, `date`, totals and tallies), and post the implied item or flow right after it (`Derivation::Also`).
    - A written line of the same transaction with the same ends and purpose replaces the derived one.
    - Escrow and an employer's match arrive this way now.
13. **Measures** (`Book.measures`) are events.
    - Purpose laws fire on them.
    - `total` counts them in their unit.
    - `on in` and `on out` never fire for them: nothing moves.
14. **`against`** (`Detail.against`). A refund takes the purpose and object of the flow it refunds; recognition not yet used stops; an asset part's cost falls. A reimbursement is visible to laws as `flow.against`.
15. **Owner sets.** A business's tallies reach its owners in its `owned_by` shares.

## Constraints

- **Performance must not regress** from E4a's numbers: `check` on `the 1m bench project (`bench/gen.py`)`, and callgrind Ir on `the 100k bench project (`bench/gen.py`)`. v3 books have no purposes, assets, contracts or budgets, and must pay nothing for them.
- **Size:** at most +900 lines for all of this. The diagnostics are lane E4c's. Fold assets into the existing lot store rather than adding a second store: a part is basis carried beside the asset's single parcel.
- `cargo test --workspace --release` is green, and goldens and mistakes are unchanged for v3 books.

**Report:** what you built, the fixtures that prove each semantic, what the model must provide that it does not yet (for the model lane), and the numbers.

Work in your worktree, from the commit the orchestrator gives you, which has lane E4a's structure and the v4c types.
