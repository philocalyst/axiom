You are **lane M4b (model: the journal)** of the Axiom v4 rework. First read `v2/briefs/common.md`. It is part of this brief.

Lane M4a rebuilt the model's declarations, names, kinds, places, purposes, properties, laws, budgets and sources on the v4 syntax. Its handoff notes are in `v2/briefs/notes-m4a.md`. You elaborate the journal on what it left.

**Read first, in full:**
- `v2/LANGUAGE.md` (v4, normative) and `v2/DESIGN.md`;
- `v2/examples/v4-sketch/`, all of it, with its README;
- the v4 AST in `crates/syntax/src/ast.rs`: statements, items, flows and tails;
- the public types in `crates/model/src/{book,journal,law}.rs` (`v2/briefs/types-v4.rs`, `types-v4b.rs`);
- `v2/briefs/audit-core-syntax-model.md` finding 11 (elaboration allocations), which is yours;
- `notes-m4a.md`.

**You own `v2/crates/model/src/flows/**`**, and whatever the model needs for the journal, statements, contracts, claims and assets, with their tests. Keep to M4a's interfaces. Where one falls short, fix it in place and say so.

## Elaborate the journal

1. **Flows.** Ends resolve across the namespaces.
   - A contract with a loan is an end: its debt.
   - An asset written as an end is an error whose fix is `#purchase of ASSET`, or `#sale of ASSET` in the other direction.
   - `flow.owner` is the account end's owner, else the transaction's.
   - A party-to-party leg splits into two flows through the owner (`Derivation::PassThrough`).
   - `via PARTY` records the intermediary. Purpose inference reads the real party.
   - `for`, by what it names:
     - a period is recognition;
     - an owner or envelope on money arriving holds it for them;
     - a party on money leaving is `PaidFor`: a flow into the party's tab (it owes the owner) and from the tab to the payee.
   - `due` makes a claim.
2. **Line items** (LANGUAGE §2).
   - Carved items split the header's flow: each item is its own flow, with its own purpose, description and codes, and the remainder keeps the header's.
   - `+` items are additional flows between the same ends.
   - `-` items reduce what moves. An item with its own purpose is a counter-flow of that purpose, which is how costs withheld from proceeds work. An item without one simply shrinks the header.
   - `N% of AMOUNT` is computed with banker's rounding, and all items round so the total is exact.
3. **Purposes.** Call M4a's `infer` for each flow, and record `Purposed` with its provenance. Two sources that disagree are an error naming both. Check `of` against the purpose's `of KIND`, and intern descriptions.
4. **Derivations.**
   - **Shares** (`business N% for OWNER`, on a contract, party, purpose or asset): split each flow into the owners' parts (`Derivation::Share`), rounded to sum exactly.
   - **Sales tax** (a party kind's `sales-tax`): split out of a payment. On a capital purchase it stays part of the cost, recorded as derived but joined to the part.
   - **Exchange cost:** where a price for the day exists and the exchange gave more than it got. Build prices before elaboration, as today.
5. **Codes table** (finding 11). Flow codes live in one book-wide table, and each flow holds a `Run<Sym>` into it.
   - `Placed` keeps the AST's `Many<Select>`, which is `Copy`.
   - Legs are collected straight into `Option<Vec<_>>`.
   - Nothing per flow is cloned twice.
6. **Contracts** (LANGUAGE §5).
   - Compile the declaration into `Contract { days, terms: Timeline<Terms>, .. }`: the schedule, the template (legs and items, as in a split), `about`, `covers`, shares, `deposit`, `loan`, `escrow`, `match`, `buy`.
   - Then paint the journal's statements onto it, in journal order:
     - new terms (amount with cadence), where what is omitted carries over;
     - `waived`, as an empty template for the one occurrence due on or next after the day, or for every one in `until`'s span;
     - property statements (`business`, `at` for a loan's rate, `covers`, `escrow`);
     - `until` (a new end);
     - `ends`.
   - Each painted `Terms` carries its `Change`. A code on a change names it: `^code until DATE` repaints that change's value over the new span, and `^code waived` ends it.
   - **An occurrence** (`DATE NAME [AMOUNT]`, with legs and items) elaborates to the terms in force that day, re-dated, with overrides:
     - A loan occurrence derives interest and principal from the schedule, given every earlier occurrence, every rate change (the payment re-figured over the rest of the term) and every flow to the contract (principal alone). These are `Derivation::Interest` and `Principal`.
     - Escrow and match flows are derived.
     - `buy` records the quantity bought.
     - `covers` sets the recognition.
     - An occurrence of `estimate` terms must state its amount: it is an error without one, and the fix shows the form.
     - An occurrence of an unknown contract is an error with the nearest name.
   - A loan that began before the book's first day opens its debt at the schedule's balance on that day.
   - A deposit is a claim the party holds (a debt tab), with the money held `for` the party.
7. **Claims** (LANGUAGE §7).
   - `PARTY owes OWNER [AMOUNT]` and `OWNER owes PARTY [AMOUNT]`, with items, are flows into the tabs, in mode `Actual`; their other end is outside.
   - An opening's `X owes Y` line.
   - `^code due DATE` moves a claim's due day.
   - `^code waived` writes off what remains (`Derivation::WriteOff`; accrual books reverse what was recognized).
8. **Assets.**
   - `#purchase of ASSET` acquires it: the money leaves to the seller, and the asset's unit arrives, costing the flow and its costs of exchange.
   - A capital purpose `of` an asset adds a part: the flow goes to the party, and the engine adds the part.
   - `#sale of ASSET`: the unit leaves as the money arrives. `-` items of cost purposes are the sale's costs.
   - `ASSET basis AMOUNT [since DATE]`, in an opening or a statement, brings the unit unbought.
   - `DATE ASSET ends` gives `Derivation::Disposal`.
9. **The other statements.**
   - Assertions, including a contract's loan balance, `via`, and `!`.
   - `settled`, `void` and `returned`.
   - Prices and splits.
   - Property statements: M4a's `set`, with `until` restoring the value before.
   - Budget statements, painted on `Budget.limits`.
   - `owes`.
   - `ends` on an account: it closes.
   - A statement whose subject cannot take its predicate is an error that says what the subject is and which predicates it takes. For example, `waived` on an account.
10. **Documents** (LANGUAGE §10). None in the model: the report finds them by name.

## Verify

- **The sketch builds** with std and the `us` stub M4a wrote. Write a test that builds it and checks:
  - every purpose the README and the `▸` hints name, with its provenance;
  - every derived flow: January interest 1,527.88, principal 365.02, the phone share 27.00, sales tax 138.09, exchange cost 0.39;
  - March's rent at 2,918.60, with its 18.60 `#utilities` item;
  - the gym at 120.00 in March, under the promotion's terms extended to 06-30;
  - figma waived in March;
  - the itemized invoice settled on 02-26;
  - every claim.
- **Tests:** small `.ax` tests for each feature above. Also the journal part of the refusal table, carried over from v3 and extended with v4's refusals:
  - two purpose sources that disagree;
  - a purpose missing its object;
  - a short date with no context;
  - an occurrence of an unknown contract;
  - an estimate occurrence with no amount;
  - `waived` on an account;
  - an asset as a flow end;
  - `/ PARTY`.
- **Performance:** `sync` (load and build) at 1M flows. The generator emits v3 syntax, so convert its output or write a v4 variant, and keep the converter in `bench/`. Report the numbers before and after, and peak RSS. It must not regress by more than 10% from today's `sync` numbers in `bench/REPORT.md`.
- **Size:** the whole model at or below 7,000 lines.

## Where you work

Work in `/home/user/axiom/.claude/worktrees/lane-v4`, on branch `v4`. Start every shell command with `cd /home/user/axiom/.claude/worktrees/lane-v4`. Commit in steps, each with its tests, so that an interruption loses little.

## Revision: the spec at 5f15e35 and types-v4c

This brief predates the spec's third pass. Read LANGUAGE.md at 5f15e35 or later in full, with `theory.md` and `v2/examples/explore-v5/FINDINGS.md` for why. Where this brief and the spec differ, the spec wins. On your side, add:

- **Statements by verb** (§2, §5). Elaborate each kind:
  - `=` values: balances, prices, and readings (`^code = QTY`, into `Book.readings`);
  - `worked`/`used`: `Measure`s, with purpose, owner and party inferred like a flow's;
  - `now` changes:
    - terms, which may carry legs (`empty` drops one);
    - properties;
    - `#purpose now budget …`;
    - `^code now due …`;
    - `^code now until …`;
    - a claim amended by items (a credit note);
  - events: `waived` (with a purpose and items on claims), `ends`, `settled`/`void`/`returned`, `split`, `basis`;
  - `SYSTEM filed YEAR`, into `Book.filed`.
- **References** (§4).
  - `^code` and `NAME`, with selectors (`[#purpose]`, `[END]`, `[^code]`, `[UNIT]`, `[DATE]`), resolve to amounts at build time, in dependency order: a reference to a later fact, or a cycle, is an error.
  - `N% of`, `N/M of`, `REF @ PRICE` and `X up to Y`.
  - Record `Detail.reckoned`, so `why` can show the arithmetic.
- **Contracts:**
  - inputs bound by an occurrence's `NAME = AMOUNT` lines; an item that reads an unbound input is left out;
  - templates with expressions;
  - `for last month|quarter|year`, `covers the month|quarter|year | SPAN`, `prorated`;
  - `rising`/`indexed` escalations, applied to each occurrence's amount on its anniversaries. Provide `Contract::amount_on(day)` for the forecast.
  - `due SPAN else ITEM` and `grace`;
  - `deposit AMOUNT into H`: a tenant's deposit is held for the tenant, and yours to a landlord is a claim on it;
  - loans with `resets` (the index param read at each reset) and `prepay shortens|recasts`, in the schedule you derive;
  - occurrences may carry a tail.
  
  Escrow and match are `also` lines now. The engine evaluates `also`: do not derive them here.
- **Pro-rata refunds:** a `covers` promise that `ends` early makes the unused part a claim on the party (`Derivation::Refund`).
- **Flows.**
  - `against ^code`, into `Detail.against`, resolved to the transaction.
  - The `for` table of §3: period, envelope or party arriving, owner release, party leaving (`PaidFor`), another owner.
  - A flow between two owners without a transfer purpose is an error that asks which.
- **Shares.**
  - Percents, fractions, and measures resolved against the thing's `area`.
  - Shares for parties become claims.
  - `owned_by` owner sets split what a business earns and bears.
  - A flow `of` a building is divided among its `part of` assets by area.
- **Items.** A trade-in (`- X #sale of ASSET` in a purchase) disposes of the asset at X, and derived sales tax excludes it. An item in another unit is its own exchange.
- **Recognition of claims:** in accrual books an occurrence is recognized on its due day, and the payment settles it.

**Size:** as M4a's: the whole model at or below 7,500 lines.
