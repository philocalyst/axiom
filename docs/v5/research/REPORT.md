
# R5 report: what the language still cannot say, and the changes that close it

Lane R5 of the Axiom v5 rework. Read this first; the other files are its evidence.

| file | what it is |
|---|---|
| `REPORT.md` | this synthesis: twelve ranked changes, what is still unsayable per profile, the 105-row FINDINGS table, the small-business answers, the kernel changes |
| `ASSOCIATIONS.md` | the typed association design (the user's `jordan-401k` complaint), with before and after for every association in `examples/05-family` |
| `THEORY.md` | the theory scan: thirteen items, each with a verdict |
| `ledgers/*.ax` | seven books in the proposed v5 syntax: `small-business` (855 lines), `household` (461), `expat` (369), `freelancer` (300), `landlord` (271), `shared` (196), `investor` (162) |

**Status of everything here.** No toolchain reads these files. The ledgers' balances were computed by small scripts (FIFO parcels, payroll, amortization, double-entry checks) that are not part of the deliverable; the syntax was checked only by reading it against LANGUAGE.md and PROPOSAL section 6. Citations are marked [V] read, [S] confirmed by search, [M] memory in THEORY.md and ASSOCIATIONS.md. Any number that came from my memory of a tax rule (FEIE limit, FBAR thresholds, 2026 payroll brackets) is marked "verify" where it appears.

---

## 0. What I found, in eight lines

1. **The weakness the user saw is one missing type.** v4's slots are `has employer entity`, `has beneficiary entity`, `has coverage name` (`us/401k.ax:28`, `us/529.ax:30`, `us/hsa.ax:29`). `entity` accepts any entity; `name` any word. A typed, counted, closed-world slot (A1) plus placement of words by type (A2) plus relator kinds that carry what a relation implies (A3) answers it: 25 written relations in `05-family/accounts.ax` become 3, and the rest are placed and checked.
2. **The seven ledgers produced 95 marks.** 34 are cannot-say, 25 says-badly, 19 unclear, 17 what-sync-needs. The twelve changes close 56 of them; 21 stay open; 17 are sync; 1 is already closed by PROPOSAL K5.
3. **FINDINGS (105 items) is mostly closed already.** 77 by LANGUAGE.md v4, 3 by the PROPOSAL, 6 by my changes, 19 still open. The v4 spec is in better shape than FINDINGS suggests, because its twelve proposals were largely adopted. My own first reading overclaimed against v4 in two places and I corrected the ledgers (section 3).
4. **The most important profile, a small business, needs three of the twelve changes more than the rest:** A3 (employment, plan and lease as relator kinds), A9 (merchandise as `stock-in-trade`: sales are revenue and cost, never a gain) and A4 (owners as a weighted slot, so a K-1 is a view). Equity and retained earnings need no accounts: they are a view of the fold (section 4).
5. **PROPOSAL section 5/6 has four gaps.** K5's `Term` is missing `At` (its own table uses it) and `Choose` (options need it); section 6.3 drops the `income` root and with it `budget ... 30% of #income` (fl-b07); K3's `Position` has no slots.
6. **Two of my own assumptions were wrong and are corrected:** v4 already settles a claim "oldest first" when no code is given (section 7), and v4 already has Catala's structure (`unless`; equal-rank conflict is an error: section 8). Marks and verdicts were changed to match.
7. **What stays unsayable is mostly content or documents, not language:** two bases (book and tax), the forms themselves (W-2, 941, 1099, K-1, 2555, FBAR), order-level sales tax, `pt` and `us/feie` as systems, stablecoins and staking. Section 2.
8. **Biggest risks of the association design:** institutions need real kinds (`bank`, `broker`), a bare address is stable only until a sibling opens, and defaults are guesses about the past. ASSOCIATIONS section 10.

---

## 1. The twelve changes, ranked

Each change has a stable id (A1 to A12) that the ledgers' marks use (`sb-c17 [A8]`). The id is a label; the **rank** is the order below. **Method.** Rank is by how many of the seven profiles need the change (marks, plus uses visible in the declarations) and by how much it removes: a change that deletes a keyword or a family of hand-typed legs ranks above one that adds a capability. "Profiles" is my reading of the ledgers; "marks" is machine-counted from the seven index blocks.

| rank | id | change | profiles (of 7) | marks | what it removes |
|---|---|---|---|---|---|
| 1 | **A1** | typed slots: range, multiplicity, closed-world check | 7 | 5 | `entity` and `name` as slot types; free property lines |
| 2 | **A2** | filling and addressing: nesting, path, forced placement, defaults, day-aware subsequence addresses | 6 | 1 | `account ... at`, `owner` lines, names that encode relations |
| 3 | **A3** | relator kinds that carry legs, laws and lifecycle: employment, plan membership, lease, engagement, management | 6 | 7 | per-contract payroll and match legs; hand-typed final pay, forfeiture, deposit settlement |
| 4 | **A12** | roles and phases as facts derived on a day: kinds as a set, `children`, vacancy, presence and residence as `DaySet`s | 5 | 8 | `children` as a number; one-kind-per-thing; hand-counted day tests |
| 5 | **A4** | weighted many-slots: `owners`, `uses`, `tenants`, `by`; K-1 pass-through by share and day | 4 | 5 | two weight syntaxes; hand-split draws; K-1 arithmetic |
| 6 | **A6** | two calendars per flow: recognition against pay date; business-day rolls; `for the tax year`; pay period | 4 | 8 | per-occurrence `for`; hand-moved weekend dates |
| 7 | **A9** | commodity kinds decide purpose and cost: `stock-in-trade`, `sells`, landed cost, returns, shrinkage | 1, the most important | 5 | hand-typed cost of goods; capital-gain treatment of stock |
| 8 | **A8** | claim shapes: in kind with consideration, bullet principal, assignment, set-off | 3 | 5 | deferred-revenue and option-writing special cases |
| 9 | **A10** | an exchange between two of my own positions: `into`, conversions inside a position, `<-` with `due` | 3 | 5 | the one-subject-position limit of `@` |
| 10 | **A5** | carried-in state: opening tallies and pending carries | 2 | 2 | the back-filled prior year |
| 11 | **A7** | terms of a position: interest on a balance, a revolving line, a floating rate | 2 | 2 | a typed interest line every month |
| 12 | **A11** | options and conditionals: `Choose`, `kind option`, market-caused events | 1 | 3 | three hand-typed option endings |

A1 to A3 are one design (ASSOCIATIONS.md) and are the answer to the user's complaint. They rank first because every profile uses them, not because they are the biggest in lines.

### A1. Typed slots

**Before** (v4, `us/401k.ax`, `us/529.ax`, `us/hsa.ax`):
```text
has employer entity            has beneficiary entity            has coverage name
```
**After:**
```text
has sponsor     employer = employment[employee owner].employer        // a role, a default, a count (one)
has beneficiary person                                                // exactly one, and a person
has coverage    one of self-only | family = employment[employee owner].joins(hdhp).tier
```
**Theory.** UFO relators (gUFO: `Relator` subclass of `>=2 mediates.Endurant`, verified in `gufo.ttl`); description-logic qualified number restrictions (Hollunder and Baader 1991); the ValueFlows lesson (0.16 removed the untyped `AgentRelationship`: verified in its changelog). **Removes.** Language: `entity` and `name` as property types; the free `employer`/`coverage`/`beneficiary` lines. Code: `Ty::Entity` (`crates/model/src/props.rs`) and the unchecked path of `Assign`'s entity-valued setters. **Cost.** Custodians need kinds (`bank`, `broker`). **Marks.** sb-b07, hh-u07, fl-u02, iv-c03, sh-u01.

### A2. Filling and addressing

**Before** (`examples/05-family/accounts.ax:53-55`):
```text
account jordan-401k : 401k at fidelity
owner jordan
employer bluefin
```
**After:**
```text
entity fidelity : broker
jordan/bluefin/401k            // owner and sponsor are path words placed by type; the custodian is the nesting
family/529 riley               // `family` can only be the owner, so `riley` can only be the beneficiary
```
**Theory.** Bidirectional typing (Dunfield and Krishnaswami 2021), functional dependencies (Jones 2000), forced placement of words in typed slots. **Why a path** and not `jordan's 401k` or `401k of jordan`: ASSOCIATIONS 6.2 (the former needs a new lexeme, the latter collides with `of`; both reach only the owner; `/` is already in v4 for grouping). **Removes.** `account ... at`, `owner` lines, `member`, names that carry relations. **Cost.** A bare address is stable only until a sibling opens; resolution by the line's day contains it. **Marks.** hh-u11.

### A3. Relator kinds that carry what a relation implies

**Before** (`contracts.ax:37-50`, eleven legs and a match typed per contract):
```text
contract alex-pay with acme
5_750 USD twice monthly on 15, last into joint-checking #wages
alex-401k 575 USD #household-deferral      hsa 250 USD      blue-shield 212.50 USD
dcfsa 208.33 USD      irs 692 USD ...      joint-checking ...
also acme -> alex-401k 40% of ([alex-401k] up to 10% of amount) #contribution
```
**After:**
```text
contract alex-pay : employment with acme
employee alex
5_750.00 USD semi-monthly on 15, last into family/checking
joins 401k-plan  deferral 10%      joins hdhp      joins dcfsa  5_000.00 USD yearly
```
**Theory.** UFO relator and "association class". **Removes.** Language: the per-contract legs and match tiers; the participant's hand-opened account. Code: `lower/also.rs` (150 lines) and `Match`, which K6 absorbs. **Cost.** One new trigger (`on end`) for forfeiture and final pay; the membership sketch (ASSOCIATIONS 5.1) is the least settled part. **Marks.** hh-b06, hh-c03, hh-c04, ll-b03, fl-c01, sb-b29, sh-b01.

### A12. Roles and phases are derived on a day

**Before** (v4):
```text
entity oakcraft : vendor            // one kind per thing; a client later is a different thing
entity family : household           children 1            // a number that never ages
entity lena : person                lives us/tx           // residence only; presence is the same word
```
**After:**
```text
entity oakcraft : vendor, client                              // kinds are a set; the party-kind rule reads direction
entity family : household           dependents riley          // `children` is count(dependents where age < 17), on the day
15 lena now lives pt        15 lena now in pt      // presence is a second timeline that defaults to residence
warn any lease where unit is self "unit has no lease"          // a vacancy: an existential over the inverse of the slot
```
**Theory.** UFO roles are anti-rigid and phases are conditions on a thing; Gadia's temporal elements (finite unions of intervals) and Allen's relations for the day-sets (THEORY T9). **Removes.** `children` as a number; `member family` on each person; "a thing has one kind"; every bespoke day-count (the FEIE test, the 183-day rule, `days_where`'s count-only API). **Cost.** A `DaySet` in K2 (about 150 lines, estimate). **Marks.** sb-b01, sb-u08, hh-c01, ll-c02, sh-c02, ex-c01, ex-c07, ex-u01.

### A4. Weighted many-slots

**Before** (v4: two syntaxes, and a hand-split draw):
```text
entity loomfield : llc          owner me 60%, theo 40%
contract flat ...               share 120 SQFT for studio
8_000.00 USD monthly on 25 -> dana 4_800.00 USD, theo 3_200.00 USD #distribution      // typed by hand
```
**After** (`ledgers/small-business.ax`, `freelancer.ax`, `shared.ax`):
```text
entity lantern-row : s-corp     owners  dana 60%, theo 40%
contract apartment-rent : lease uses    noor 880 SQFT, noor-consulting 120 SQFT
8_000.00 USD monthly on 25 from operating -> owners by share #distribution
```
**Theory.** A weighted relation; K-1 allocation is the integral of a share timeline (K2) over days. **Removes.** `owner ... SHARE` and `share SHARE for ENTITY` as separate syntaxes; the hand-split draw; the hand-computed K-1. **Cost.** A weight word (`by`) and a rule that a period divides by days present. **Marks.** sb-c09, sb-c24, ll-u01, sh-b02, sh-u03.

### A6. Two calendars per flow

**Before** (v4: `for` recognizes by service period; a cash-basis return reads the day; each occurrence carries its own `for`):
```text
15 checking -> irs 3_200.00 USD #estimated-tax for 2025          // typed every January
05 dana-pay          // due 11-15 is a Saturday: typed on the 17th
```
**After:**
```text
contract est-me with irs      9_600 USD installments 04-15, 06-15, 09-15, 01-15 for the tax year
contract dana-pay : employment   2_750.00 USD semi-monthly in arrears paid 5, 20     // the 16th..last is paid on the 5th, rolled off weekends
```
A flow keeps two dates, **received** (the cash-basis tally reads it) and **earned** (`for`: where the work was done, which FEIE reads); each view says which it reads. **Theory.** Valid time against transaction time (SQL:2011; Snodgrass), Allen's `during`/`overlaps`. **Removes.** Per-occurrence `for`; hand-moved weekend dates; the accrued-wage workaround. **Cost.** K5 `Every` gets a pay-date schedule and a roll rule. **Marks.** sb-b27, sb-b28, sb-c06, sb-c20, ll-c05, fl-b06, ex-b02, ex-c04.

### A9. Commodity kinds decide purpose and cost

**Before** (v4): `pays` is for issuers only; a sale of merchandise realizes a gain, with the cost as a capital-gain basis.
**After** (`ledgers/small-business.ax`):
```text
kind merchandise : stock-in-trade
select fifo                    sells sales
kind stock-in-trade : commodity
law cost-of-goods  on gain  derive -> basis #cost-of-goods       // a leg out of the position the parcels left, to no party
15 warehouse -> customers into stripe/balance ^so-2025-11a         // priced SKU lines; derived #cost-of-goods 10,133.05 from the parcels that left
```
**Theory.** REA: a resource held as parcels, a stock-flow relation per event; the cost leaves with the consumption (section 4). **Removes.** Hand-typed cost of goods; capital-gain treatment of stock; the special case of "a fund pays, a store sells". **Cost.** A `derive` to no party (`Outside`), which K6's `Derive(LegTemplate)` must allow. **Marks.** sb-b03, sb-b19, sb-c10, sb-c13, sb-c15.

### A8. Claim shapes

**Before** (v4: a claim is money or nothing; a loan is the annuity): nothing for any of the cases below.
**After:**
```text
lantern-row owes market 144 CLUB_BOX monthly over 12m from 12-15 received 2_880.00 USD      // deferred revenue: a claim in kind
brokerage owes market 1 LMNT_C75_0417 received 235.00 USD ^call-1                           // a written option: the same substrate
theo/note : loan 15_000.00 USD on 2024-03-01 at 6% bullet 2027-03-01                        // ACTUS PAM: interest, principal at the end
marcus -> priya 888.80 USD via venmo net                                                    // set-off: the pair's claims in normal form
```
**Theory.** A claim parcel whose basis is the consideration received, relieved by delivery; ACTUS PAM; the Pacioli group of differences (a pair's opposite claims are one element, THEORY T6). **Removes.** Three special cases (deferred revenue, option writing, bullet notes) and the idea of a `net` keyword. **Cost.** `received` and `bullet` as clauses; K3 gets claims in kind. **Marks.** sb-c05, sb-c17, sb-u18, sh-c01, sh-c03.

### A10. An exchange between two of my own positions

**Before** (v4/PROPOSAL 6.1): `@` has one subject position; goods leaving a warehouse for a processor's balance, or a conversion with the fee in the source currency, must be written as two flows.
**After:**
```text
15 warehouse -> customers into stripe/balance ^so-2025-11a
22 wise/eur -> 2_385.60 EUR @ 1.1750 USD into operating ^fx-0122
   - 14.40 EUR #fees via wise
```
**Theory.** ValueFlows `transfer` and `move` with `resourceInventoriedAs` and `toResourceInventoriedAs` (verified in `all_vf.md`): the two ends of an event are two inventoried resources. **Removes.** The one-subject-position limit of `@`; the "a leg between two parties passes through the owner" workaround. **Cost.** A clause (`into`); K4's `Leg { from, to }` already holds both ends. **Marks.** sb-b10, sb-b14, sb-b22, iv-b12, ex-b03.

### A5. Carried-in state

**Before** (v4 section 5: "Openings are states, not flows: no law sees them"): a book that starts mid-year does not know the year's wages, deferrals or a wash sale's pending loss.
**After:**
```text
opening 2025-11-01
operating 84_512.19 USD
tally wages[2025] of dana 55_000.00 USD            // what laws read: FUTA/SUTA bases, the 401(k) limit, K-1 income to date
carry loss 72.83 USD to VXUS within 30d of 2025-12-22
```
**Theory.** Event sourcing with a snapshot: the opening is the fold's state at a day, including the state laws keep. **Removes.** The back-filled prior year. **Cost.** K3's `Opening` populates the tally store and a pending-carry list. **Marks.** sb-c02, iv-c11.

### A7. Terms of a position

**Before** (v4): each month's interest is a typed line.
**After:**
```text
family/savings : deposit        interest 3.95% on the daily balance, paid monthly
bluebonnet/line : credit-line 40_000 USD   rate prime + 2.5%   pays interest monthly on 5 for last month from operating
```
**Theory.** ACTUS UMP (undefined maturity profile), IPAC (interest payment capitalization) and RR (rate reset); the ACTUS dictionary 1.4 was read in `actus-dictionary` for the option terms; I did not verify the UMP/IPAC/RR event definitions beyond their acronyms. **Removes.** The monthly typed interest line (12 a year on every savings account and card). **Cost.** K5 `Accrue`. **Marks.** hh-c05, sb-c04.

### A11. Options and conditionals

**Before** (v4): no way to say that a counterparty chooses.
**After:**
```text
04 brokerage owes market 1 LMNT_C72_0619 received 310.00 USD ^call-2
   covers brokerage[2025-05-20] 100 LMNT
19 ^call-2 assigned          // the market chose; derived: delivers the covered lot at 72.00, the premium joins the amount realized
```
K5: `Choose { by: market, until: expires, options: [exercise -> Pay(100 LMNT for strike)], default: Done }`. **Theory.** Marlowe `When`/`Choice`, Composing Contracts' `or`, ACTUS OPTNS terms (`OPTP OPS1 OPXT OPXED DS`, verified). **Removes.** Three hand-typed endings. **Cost.** The forecast takes `default` until an event says otherwise. **Marks.** iv-b04, iv-c01, iv-c02.

---

## 2. What still cannot be said, per profile

After A1 to A12. `[open]` marks are in the ledgers' index blocks; items marked (content) are tax rules or systems, not language.

**Small business** (35 marks: 23 closed by A-changes, 5 sync, 1 K5, 6 open):
- two bases at once: books accrual, tax cash, with section 179 expensing against five-year depreciation (sb-c25, also fl-c05);
- the forms as documents: W-2, W-3, 940, 941, 1099-NEC, K-1 (sb-c26): Axiom reports lines, nothing writes a form;
- a sales-tax rate by the order's ship-to address; only the order document knows it (sb-c11), and a half-month document is one flow on one day, so balances between are wrong (sb-b12);
- whether the sales tax on a lost chargeback is recoverable (sb-u16); section 988(e) does not exempt a business (sb-u23) (content);
- the late fee that repeats every 30 days is closed by K5 (`Every` inside `otherwise`) and not by the v4 spec (sb-b21).

**Household** (10 marks: 7 tagged with the change that closes them, 3 sync; two keep a residual question): the HSA coverage default reads only the owner's own HDHP, and family coverage through the spouse's plan is the same fact from another employment (hh-u07); a default for `sponsor` is a guess about the past (hh-u11); three sync needs (paystubs, escrow analysis, custodian statements).

**Freelancer** (7 marks, 3 open): a cost billed back at cost is income and spending at once (fl-b04); a budget "of income" lost its root (fl-b07, the PROPOSAL regression); two bases again (fl-c05).

**Landlord** (7 marks, 1 open): repair against improvement under the de minimis safe harbor is an election made once a year, and nothing says a choice was made (ll-u04); two sync needs (the manager's owner statement, the building's area on the utility bill).

**Investor** (11 marks, 3 open): a stablecoin is a `crypto` that behaves as cash (iv-b05); staking is income on receipt, a move, or a disposal (iv-u06, content); section 1092(c) qualified covered calls pause the holding period (iv-u08, content); two sync needs (assignment notices, staking reports).

**Expat** (16 marks, 7 open): FBAR maximum per account in its own units at the Treasury's year-end rate, where `peak` gives the owner's currency at the day's price (ex-b05); a due day that depends on where the person lives that day (ex-c05); the forms (ex-c09); `pt` and the US-Portugal totalization agreement do not exist as systems (ex-u02); section 988(e) for a person who holds business receipts (ex-u03); which exchange rate a return uses, chosen once (ex-u04); whether Wise's multi-currency account is one FBAR account (ex-u05).

**Shared flat** (9 marks, 1 open): multilateral netting needs the household as the common counterparty (sh-u02). Closed: set-off (A8), assignment of a deposit share (A8), the deposit's creditor as a role (A12), the weight set for a one-off flow (A4).

---

## 3. All 105 FINDINGS items

FINDINGS = `examples/explore-v5/FINDINGS.md`. **Classification rules.** *closed by v4*: LANGUAGE.md has it (section cited); this is a **spec** claim: the implementation is another matter (PROPOSAL section 0 says the claim monitor does not exist). *closed by the PROPOSAL*: section 5 kernels or section 6 language changes. *closed by my changes*: one of A1 to A12 (a residue that only an A-change closes is named, whatever the primary status). *still open*: nothing closes it. Where a finding is half closed I classified by its point and named the residue.

Counts: **77 closed by v4, 3 by the PROPOSAL, 6 by my changes, 19 still open.**

| id | what | status | evidence and residue |
|---|---|---|---|
| 01a1 | a 60/40 owner; reaching the owners' returns | closed by v4 | `owner A 60%, B 40%` (section 6 table); tallies belong to owners (section 8). By day, separately stated items and stock basis: A4 (sb-c24). |
| 01a2 | flows between owners; draws | closed by v4 | `transfer` root with `#distribution`, `#contribution`; `for` another owner (sections 2, 3). `owners` as one end of a leg: A4 (sb-c09). |
| 01a3 | withheld and collected tax; nothing ties the cash | closed by v4 | `transfer` root; `for wa-dor` ties the money (section 3 table, section 9). |
| 01a4 | reserve: 30% of what arrived stays | still open | v4 had `budget ... funded from ... into ...` with `% of #income`. PROPOSAL 6.3 drops the `income` root, so the form loses its base (fl-b07). |
| 01a5 | a retainer is an invoice, not a receipt | closed by my changes | v4 makes a claim only when a due day is missed (section 7); `invoiced` was proposed in FINDINGS and never added. A3 `engagement` (fl-c01). |
| 01a6 | sales tax by date and place, typed on each contract | closed by v4 | `also + 10.35% ... when to.lives is us/wa` (section 10); rates are params. |
| 01a7 | payroll: 12 lines a month; FUTA base is a running total | closed by v4 | `also` with expressions and `total(#wages of employee, year)` (sections 8, 10). A3 makes it one `employment`; wage bases at the start of a book: A5 (sb-c02). |
| 01a8 | `for last month`; installment for the year before | closed by v4 | `for last month`, tails on occurrences (section 7). The January installment is still typed per occurrence: A6 `for the tax year` (fl-b06, sb-b27, ex-b02). |
| 01a9 | amending a claim: credit note, late fee, write-off | closed by v4 | `^code now` with items, `waived` with purpose and items, `due ... else` (sections 5, 7). |
| 01a10 | a cost billed for a client is claimed twice | closed by v4 | `against ^code` (section 3). Unverified for this exact double claim. |
| 01a11 | an opening claim has no items | still open | `opening` takes `jo owes me 600 USD due 04-01` with no purpose or items (section 5). One sentence of spec; the ledgers write the purpose on the opening claim. |
| 01b1 | a cost billed back at cost is income and spending | still open | `transfer` hides it from income and leaves the cost in spending; the return wants both (fl-b04). |
| 01b2 | `merchant` has no purpose; written on 21 of 33 charges | closed by the PROPOSAL | 6.4: purpose or description replaces the invented party; sync `category` (section 14). |
| 01b3 | list declarations share every property | still open | Cosmetic: `entity a, b, c : kind` shares all of them; 30 clients still need per-client place and exemption lines. |
| 01b5 | `quarterly on 04-30` fixes the day | closed by v4 | `on last`, `15, last` (section 7). |
| 01b6 | `yearly on` four dates reads as one date | closed by v4 | "several days after `on` are each due" (section 7). |
| 01b7 | one purpose per party kind (insurer in and out) | closed by v4 | `pays NAME` on a party kind (sections 2, 6). |
| 01c1 | the `books` property missing from the property table | closed by v4 | In the section 6 table (`books cash` or `accrual`). |
| 01c3 | `#wages of maya`: `of` takes an entity | closed by v4 | "the object may be any declared thing" (section 2). |
| 01c4 | money from a processor: a refund of fees? | closed by v4 | `pays` (section 6). |
| 01c5 | `covers the month` vs `covers the year` | closed by v4 | Section 7 defines `covers the month` and `covers 1y`. |
| 01c6 | settle by the header or by what arrives | closed by v4 | Section 7: the claim whose open amount is exactly the flow's. The ledgers write the header as gross with the fee as a `-` item; the sentence still does not say so. |
| 01d1 | the name as the default `known-as` | closed by v4 | Section 14: every entity and account is known by its own name. |
| 01d2 | one export, three cardholders | closed by v4 | `route` (section 14). |
| 01d3 | Stripe's payout as one document | closed by v4 | "a document a source prints ... outranks the bank's line" (section 14). |
| 01d4 | the invoicing export must print amendments | closed by v4 | `^code now` statements are Axiom text a script can print (sections 5, 14). |
| 01d5 | a payroll run against two ACH lines | closed by v4 | Reconciliation by shared code, and `id` (section 14). Two lines with no shared id stay a heuristic. |
| 01d6 | retainer paid 18 days after the 1st | closed by v4 | `grace SPAN` (section 7). |
| 02a1 | dependent-care FSA and its rule | closed by v4 | A kind with a law (`05-family` has `dependent-care-fsa`); `us/fsa` is content. Typed `dependents` and `sponsor`: A1 (hh ledger). |
| 02a2 | a household whose members join in June | closed by my changes | A12: `members` is a dated slot, so tallies count per person until the day and for the household after. The year-end sum rule is a law of `us` (partly). |
| 02a3 | withholding from salary and W-4 | closed by v4 | `also` with `withholding(...)` (section 10). A3 puts it on the `employment` kind. |
| 02a4 | proration of first paycheck, rent, daycare | closed by v4 | `prorated` (section 7). |
| 02a5 | interest follows the use of a loan's proceeds | still open | FINDINGS: "not proposed"; a tracing rule in a law. |
| 02a6 | replace an account across contracts | still open | Each contract still names its position, by path in v5; one statement per contract. `moves` was never added. |
| 02a7 | a terms statement with legs | closed by v4 | `now ... ftb empty` (section 5). |
| 02a8 | owner to owner: joint account funding | closed by v4 | `transfer` root (section 2). |
| 02a9 | trade-in; tax on price less trade-in | closed by v4 | `- X #sale of ASSET` item in a purchase (section 3). |
| 02a10 | wedding gifts | closed by v4 | `transfer` root: "a gift received" (section 0). |
| 02a11 | HSA reimbursing an out-of-pocket expense | closed by v4 | `against`, same direction (section 3). |
| 02a12 | vesting: unvested match forfeited on leaving | closed by my changes | A3: the plan's `vests` schedule and an `on end` norm (hh-c03). |
| 02a13 | negative items in an `owes` statement | closed by v4 | `-` items and `^code now` with items (sections 3, 5). |
| 02a14 | a plan that pays a claim only up to its balance | closed by v4 | `against` plus a law in `us/fsa` (content); the household ledger shows it. |
| 02b2 | insurer: premium out, payout in | closed by v4 | `pays` (section 6). |
| 02b3 | one Target in two places, selling food | closed by v4 | `also ... when` by place and purpose (section 10). |
| 02b4 | `yearly on 04-10, 12-10`; each covers a half-year | closed by v4 | Date lists and `covers SPAN` (section 7). |
| 02b5 | a split with only a source: a dangling arrow | closed by the PROPOSAL | 6.1 retires the one-ended arrow (the PROPOSAL names 02b5). |
| 02c1 | `lives` may overlap, a later statement overrides | closed by v4 | `lives SYSTEM, ...`; `now lives` moves them (section 6 table). |
| 02c2 | `ssa`, `edd`: authorities or purposes | closed by v4 | std `tax-authority`; the rest is content. |
| 02c3 | `deposit`: direction, and paid before `from` | closed by v4 | Section 7 `deposit` covers both directions. |
| 02c4 | an occurrence with a tail | closed by v4 | Section 7: an occurrence may carry any tail. |
| 02c5 | `owe` obligations vs claims | closed by v4 | Section 8: `owe` creates a claim. |
| 02c6 | a year-closing law of a system an owner left | still open | Section 8 `each year closing` is silent about owners who left; A12 derives residence (ex-c07) but not the part-year division. |
| 02d1 | a refund memo has no year | still open | A `law`-`owe`d claim carries its year (section 8); whether a refund settles a negative `owe` is unspecified. |
| 02d2 | paystub PDFs: print the contract or its legs | closed by v4 | `01 flat` / `08 phone 47.30 USD` (sections 7, 14). |
| 02d3 | a refinance wire is one leg of a flow | closed by v4 | Section 14: "one leg of a split (a refinance's wire)". |
| 02d4 | a memo picks an earlier flow, not a party | still open | `against` has no recognizer in sync; a heuristic. |
| 02d5 | FSA portal claim numbers | closed by v4 | A structured `code` (section 14). |
| 03a1 | a building of units, shares by area | closed by v4 | `part of`, `area`, a flow of the whole divided by area (sections 6, 10). An improvement of the whole becoming a part of each unit: A4 (ll-u01). |
| 03a3 | a late fee is a term of the lease | closed by v4 | `due 5d else + 5% #late-fee` (section 7). A fee that repeats: K5 `Every` (sb-b21). |
| 03a4 | utilities re-billed by share | closed by v4 | `input` and `+ 12% of water` (section 7). |
| 03a5 | a vacancy is a contract's absence | closed by my changes | A12: an existential over the inverse of the `unit` slot (ll-c02). |
| 03a6 | deposit deductions | closed by v4 | Items and purposes on a returned deposit (sections 5, 7). The lease kind deriving the settlement: A3 (ll-b03). |
| 03a7 | prorating the first month | closed by v4 | `prorated` (section 7). |
| 03b1 | header and items with the same purpose | closed by v4 | Items carve the header (section 3). |
| 03b2 | `yearly on 01-31, 07-31` | closed by v4 | Section 7 date lists. |
| 03c1 | two `business` lines on one asset | closed by v4 | `share SHARE for ENTITY, ...` (section 6). |
| 03c2 | how a rental's tallies reach the owner | closed by v4 | Section 8: tallies belong to owners. |
| 03c3 | `covers 1y` for a policy from 06-01 | closed by v4 | Section 7. |
| 03c4 | a deposit held where | closed by v4 | `deposit AMOUNT [into HOLDING]` (section 7). |
| 03d1 | the manager's CSV has a unit column | closed by v4 | `object` (section 14). |
| 03d2 | a utility PDF the script cannot read for areas | still open | `run` receives units, not their areas (sync, ll-s08). |
| 03d3 | tenant ledgers as `owes` statements with items | closed by v4 | Items on claims (section 7). |
| 04a1 | FEIE, foreign tax credit, FBAR, Portugal | still open | Content, not language: `us/feie` and `pt` do not exist (ex-u02). |
| 04a2 | the physical presence test, forecast | closed by my changes | A12: a `DaySet` window search (THEORY T9) over a planned presence (ex-c01). v4 `days(COND, window)` counts one window. |
| 04a3 | FBAR: the highest balance of the year | closed by the PROPOSAL | K2's sparse table: "covers FBAR peaks with no sampling"; v4 has `peak(x, window)`. The per-account, year-end-rate form is open (ex-b05). |
| 04a4 | four installments, the last for the year before | closed by v4 | Date lists (section 7); the January one is A6, as 01a8. |
| 04a5 | Stripe's schedule is conditional | closed by v4 | `also ... when` (section 10). |
| 04a6 | a sprint half earned in Austin, half in Lisbon | closed by my changes | A6 with a `DaySet`: income by the days it was earned (ex-c04). |
| 04a7 | a chargeback and its reversal | closed by v4 | `against` (section 3). |
| 04a8 | a VAT refund at the border | closed by v4 | `against`: "a VAT refund at the border" (section 3). |
| 04b1 | a citizen's obligation written `lives us` | closed by v4 | `citizen SYSTEM` (section 6). |
| 04b2 | a stablecoin is a crypto that is money | still open | iv-b05. |
| 04b3 | VAT as `sales-tax`: nothing says it is recoverable | still open | Nothing types recoverability (sb-u16). |
| 04c1 | `lives us` and `lives pt` overlap | closed by v4 | As 02c1. |
| 04c2 | `purpose fees` on a processor, both ways | closed by v4 | `pays` (section 6). |
| 04c3 | the basis of an arrival with no cost | still open | Section 9 gives cost-based basis only; a reward's basis at the day's price is unstated (iv-u06). |
| 04c4 | gas in ETH on a swap | closed by v4 | Section 3: an item in another unit is an exchange of its own. |
| 04c5 | a tenant's deposit two ways | closed by v4 | Section 7 `deposit`. |
| 04d1 | a card memo carries the store's price and currency | closed by v4 | `original` capture (section 14). |
| 04d2 | Wise: currency, id, fee | closed by v4 | `currency`, `id`, `fee` (section 14). |
| 04d3 | PayPal: gross, fee, net | closed by v4 | `gross`, `fee` (section 14). |
| 04d4 | staking rows and the price of the day | still open | iv-s10: a format to write; no spec gap beyond the price join. |
| 05a1 | an account-kind rule that derives flows (cash back) | closed by v4 | `kind card: also issuer -> self 2% of amount #rebate` (section 10). |
| 05a2 | miles realize nothing | still open | No property says a redemption is not a disposal. |
| 05a3 | "every dollar assigned": a law over budget limits | still open | A budget is a `warn`; `funded` budgets give envelopes, not the zero-based check. |
| 05a4 | a gift card bought for 90, worth 100 | still open | `worth` was proposed in FINDINGS and not added. |
| 05a5 | pro rata refund of a prepaid year | closed by v4 | `against` stops recognition (section 3). |
| 05b1 | a sinking fund written twice | closed by v4 | `funded from ... into ...` (section 6). |
| 05b2 | moving 40 between budgets for a month | still open | `budget move` was not added; two statements must sum to zero. |
| 05b3 | a declaration overridden from day one | closed by v4 | "the declaration is simply the first" (section 5). |
| 05c1 | `kind envelope` assumed | closed by v4 | std.ax `kind envelope`. |
| 05c2 | reconciling a record with a derived flow | closed by v4 | Section 14: "or a derived flow (a rebate)". |
| 05d1 | the card's `Category` column | closed by v4 | `category` (section 14). |
| 05d2 | Venmo `From`/`To`, `Funding Source`, `Note` | closed by v4 | `party`, `route`, `memo` (section 14). |
| 05d3 | a miles earn matched to its charge | closed by v4 | Derived flows are reconciled (section 14). |

**Corrections to my own first pass**, made in the ledgers after checking LANGUAGE.md: (1) v4 section 7 already settles a claim "the one whose open amount is exactly the flow's, else the oldest first" (the shared-flat mark for Venmo payments was dropped and the Venmo mark reduced to sync); (2) section 8 already has Catala's `unless` and "two laws of equal rank that disagree are an error", so THEORY T7 now proposes only labelled exceptions and "why not"; (3) the freelancer's retainer mark `fl-c01` first claimed that `Due` cannot blame the client; v4 can (direction decides who owes) and the real gap is that a claim does not exist until a due day is missed.

---

## 4. Small-business specifics

The profile is `ledgers/small-business.ax`: Lantern Row Games LLC (Austin), S-corp 60/40, 2025-11 to 2026-03: board games and puzzles sold online in WA and TX, a mystery-box subscription club, consulting, payroll for three employees and an officer, sales tax in two states, a Stripe processor, a credit line, a EUR customer, a label printer, a K-1. Net assets 84,000.73 at opening, 90,482.03 at 2026-03-31; income Nov 16,432.68, Dec 25,577.46, Jan -2,238.56, Feb 3,013.33, Mar 2,696.39; distributions 39,000.00. An internal double-entry check verified that net assets moved by exactly income less distributions.

### 4.1 Does accrual need anything beyond claims plus `books accrual`?

Yes, four things, and **none of them is an account**:

1. **A recognition period on the occurrence** (A6). Accrued wages at 12-31 (9,361.40) exist because the pay occurrence "covers" the period it pays for (the 16th to the last, paid on the 5th). Without it the expense lands in the month paid, and the v4 contract has no way to say two calendars (sb-c06, sb-c20).
2. **A claim in kind with consideration** (A8). A 12-month club subscription paid up front is not revenue: it is a promise of 12 boxes. Unearned revenue is `lantern-row owes market 144 CLUB_BOX monthly over 12m from 12-15 received 2_880.00 USD`; revenue is 20.00 USD per box delivered. At 03-31, 698 boxes owed to 80 members are 13,960.00 USD, derived, never posted (sb-c17).
3. **A view that lists the remainder of a recognition.** Prepaid insurance (annual premium 4,860.00 paid 11-01, 405.00 a month, 2,835.00 left at 03-31) is a `for PERIOD` recognition map, which is not a position; `balance` had no row for it (sb-u18, unclear). The closing block derives it as a *claim*: the prepaid remainder is a claim the company holds on its insurer for the unused days (an `ends` makes it a pro-rata refund, v4 section 7). That is the REA reading: a prepayment is a commitment partly kept.
4. **Accrual of interest as a term of the position** (A7): the line's interest for last month.

Everything else in an accrual book is already a claim: receivables (`^inv-1136`), payables (`pi-1140`), sales tax collected and owed (tied for the state), payroll liabilities (941, FUTA, SUTA, accrued wages), the credit card and the line. The accounting equation needs no accrual accounts, because "a claim is the time between an event and its counterpart" (FINDINGS section 4, an REA duality statement).

### 4.2 Inventory and cost of goods in REA terms

- **Resource and type.** `commodity ORCHARD : merchandise` is a *resource type*; the goods in the warehouse are **parcels** `(quantity, basis, acquired, transaction, tie)` held in the position `warehouse`, which is custody by the company itself (REA custody). A purchase from a distributor is an exchange event whose dual is the payable (`lantern-row owes brightwood-dist ... ^pi-1097`): REA's duality, stock-flow *in*.
- **Cost of goods is the consumption of parcels at the sale.** A sale document `warehouse -> customers into stripe/balance` relieves parcels FIFO (`select fifo`; `prorata` would be average cost); the kind's law `on gain derive -> basis #cost-of-goods` writes **a leg out of the position the parcels left, to no party**, so their basis leaves the owners as spending, while the proceeds keep the sale's own purpose (`sales`). For 11-15: revenue and cost, never a gain; the ledger shows `#cost-of-goods 10,133.05` split by SKU. This is A9.
- **Landed cost.** A later freight bill `#landed-cost of ^pi-1140` *joins the parcels of the purchase it belongs to*, pro rata to cost (3.745%: ORCHARD +413.44, LANTERN +319.07, ...): REA's "costs are value assigned to the lot", a `rebase` of parcels (PROPOSAL K3 already lists `rebase`).
- **Returns** go back to stock at the cost they left with (30.20 for TILEWRIGHT, not today's), a damaged one is not stock (`#shrinkage`): provenance of the parcel, with `against` for the refund of the sale (sb-c15).
- **Shrinkage** is a count `31 warehouse = ...` whose gap is a loss of stock, not unexplained and not the market's: `=` needs `via` a purpose (sb-b19).
- **Valuation.** FIFO is the default; lower of cost or market would be an `each month` law the ledger does not exercise. **Unsure:** whether `rebase` on a joining freight bill is O(parcels) per bill; no measurement.

### 4.3 Equity and retained earnings, without equity accounts

**Equity is a view of the fold**, the Haig-Simons identity that PROPOSAL K7 names: net assets (holdings minus claims on us) split by the purpose roots of what built them. The ledger's closing block, derived and never typed:

```text
net assets                           90_482.03
contributed (#contribution)        20_000.00     dana 12_000.00 - theo 8_000.00
brought forward on 11-01           64_000.73     income less distributions of every earlier month
income since 11-01                 45_481.30     Nov 16_432.68 - Dec 25_577.46 - Jan -2_238.56 - Feb 3_013.33 - Mar 2_696.39
distributions since 11-01         -39_000.00     #distribution, 60/40 each time
by owner (the weights, by day)       dana 54_289.22 - theo 36_192.81
```

The figure cannot disagree with the balance sheet because it is the fold's own difference. **Retained earnings** is "brought forward": cumulative income less cumulative distributions of closed years, a query and not a closing entry. **S-corp stock basis** is a per-shareholder view: contributions, plus share of income, less distributions, never below zero. **Not solved:** the accumulated adjustments account and other shareholder-level accounts a CPA tracks (content).

### 4.4 How a K-1 works

1. The company is `kind s-corp` with `owners dana 60%, theo 40%` (A4): a weighted slot, so a sale of 10% on 09-01 splits the K-1 at that day.
2. `passes-through` on the kind: every tally the company counts is *also* counted for each owner, in the share held on **each day** (K2: the integral of the share timeline). The tally is `count ... as ordinary-income` and the K-1 box reads it.
3. **Separately stated items** are their own tallies: charitable contributions 600.00 (to dana 360.00, theo 240.00); section 179 deduction 6,800.00 (a tax-only basis: sb-c25, open).
4. **Distributions** are `#distribution` flows `-> owners by share`; a law over the per-owner tallies checks that they are pro rata (single class of stock; sb-c09); they reduce stock basis.
5. The 2025 K-1 for Nov and Dec only: ordinary income 42,610.14 (net 42,010.14 + 600.00 charity), dana 25,566.08, theo 17,044.06; Q1 2026: 3,471.16, dana 2,082.70, theo 1,388.46. **The K-1 as a document** (the form) is `sb-c26`, open.

---

## 5. What must change in PROPOSAL section 5

Counts are estimates by analogy with PROPOSAL's own budgets, not measurements.

**K1 Taxonomy.** `Kind` gains slots, parts, verbs; an entity has a **set** of kinds (A12); a `Range` replaces `Ty::Entity`.
```rust
pub struct Kind { pub parents: SmallVec<[Id<Kind>; 1]>, pub slots: Vec<Slot>, pub parts: Vec<Part>, pub verbs: Vec<Verb>, pub props: Props }
pub struct Slot { pub name: Sym, pub range: Range, pub mult: Mult, pub prep: Option<Prep>, pub weight: Option<Weight>, pub default: Option<Expr> }
pub enum Range { Kinds(SmallVec<[Id<Kind>; 2]>), Names(Box<[Sym]>), Value(Ty) }       // `Ty::Entity` deleted
pub enum Mult  { One, Optional, Some, Many, Between(u8, u8) }
```
**K2 Behaviours.** A slot's fill is a `Timeline`; day-sets are values; a schedule has a pay date.
```rust
pub enum Fill { None, One(Ref), Many(Vec<(Ref, Weight)>) }       pub type SlotTimeline = Timeline<Fill>;
pub struct DaySet(Run<Span>);                                    // union, intersection, difference, len(), max_in_window(m), earliest_reaching(n, within)
impl<T> Timeline<T> { pub fn where_(&self, holds: impl Fn(&T) -> bool) -> DaySet; }       // `days_where` returns only a count today
pub struct Schedule { on: Cadence, paid: Option<Cadence>, roll: Roll }                     // A6: pay period and pay date
```
**K3 Positions and parcels.** `Position` gains the slots; `Parcel` is move-only; a claim may be in kind.
```rust
pub struct Position { pub owner: Id<Entity>, pub with: Id<Entity>, pub name: Option<Sym>, pub kind: Id<Kind>, pub class: Class, pub slots: SlotMap }
pub struct Parcel { qty: Qty, basis: Basis, acquired: Day, consideration: Option<Amount> /* A8: received up front */, .. }   // no Clone; every method takes self
pub struct Opening { holdings: Run<Holding>, tallies: Run<TallyLine>, carries: Run<PendingCarry> }       // A5
```
**K4 Events.** A static split check; the optional counterparty is an implicit `Outside` end; zero postings are trimmed (Numscript does it, THEORY T13).
```rust
pub fn check_split(legs: &[LegIr]) -> Result<(), Unbalanced>;     // per commodity: sum of shares == 1 and sum of constants == 0, or a `Rest` leg
```
**K5 Promises.** The `Term` enum is missing two constructors, and gains two more.
```rust
pub enum Term {
  Done, Pay(Id<LegTemplate>), All(Range<TermId>), Every { schedule: Schedule, body: TermId },
  Due { grace: Span, blame: Id<Entity>, body: TermId, otherwise: TermId },
  If { cond: NodeId, then: TermId, otherwise: TermId }, Let { name: Sym, value: NodeId, body: TermId }, Annuity(Id<Loan>),
  At { day: DayExpr, body: TermId },                                                                 // used by PROPOSAL's own deposit row, absent from its enum
  Choose { by: Id<Entity>, until: Day, options: Range<(Sym, TermId)>, default: TermId },             // options (A11)
  Accrue { rate: IndexExpr, on: Balance, pay: Schedule },                                            // interest on a balance, a revolving line (A7)
}
// a bullet loan is All[Pay(principal in), Every(monthly, Pay(interest)), At(maturity, Pay(principal))]; a late fee that repeats is `Every` inside `otherwise`.
pub fn project(term: &Term, who: Id<Entity>) -> Local;       // THEORY T3
```
**K6 Norms.** Two triggers and a labelled exception; `derive` may end at `Outside`.
```rust
pub enum Trigger { Flow, In, Out, Gain, Start(Id<Kind>), End(Id<Kind>) }     // A3: a relator's life
```
`unless NAME "source"` (THEORY T7); a law over per-owner tallies for the pro-rata check (sb-c09).
**K7 Facts.** Z-sets and checkpoints (THEORY T4); new views: `equity` (section 4.3), `k1`, `claims` in normal form with `net`, `settle`, `export gl`.

**Four defects in the PROPOSAL text to fix:**
1. K5's `Term` lacks `At` (its own table writes `At(end, ...)` for the deposit) and `Choose`.
2. Section 6.3 drops the `income` root; v4's `budget ... 30% of #income` has no base, and a funded reserve "of what arrived" has no word (fl-b07).
3. K3's `Position` has no slots: ASSOCIATIONS 9.1.
4. Section 6.1 defines the junction for money lines; a purchase on credit (`<-` with `due`) and an exchange between my own positions are not covered (sb-b14, A10).

**Order of work** (ASSOCIATIONS 9.2): L0 typed slots; L1 nesting, path, forced placement; L2 defaults, parts, memberships, weights; L3 triggers and verbs. A1 and A2 answer the complaint and are one change to K1/K3 plus a resolver.

---

## 6. The biggest open problems, and what I am unsure of

1. **Two bases.** A small business keeps books on accrual and a return on cash with a different depreciation rule (sb-c25, fl-c05). A second set of tallies read by `tax` against `books` is the likely answer, but I did not design it.
2. **Documents out.** Axiom reports lines; nothing writes W-2, 941, 940, 1099-NEC, K-1, Form 2555 or FinCEN 114 (sb-c26, ex-c09).
3. **Order-level sales tax and half-month documents** (sb-c11, sb-b12): sync outranks the bank line, but one flow on one day makes balances between wrong.
4. **Content, not language:** `pt`, `us/feie`, `us/fsa`, `us/s-corp`, `us/payroll` tables, section 988, section 1092(c), QBI. The language can say them; nobody has written them.
5. **A bare address is stable only until a sibling opens**; resolution by the line's day contains it, `axiom fix` can rewrite it. **Defaults are guesses about the past.**
6. **The membership sketch** (ASSOCIATIONS 5.1) was never run. Whether a membership is a stored thing or a projection is undecided.
7. **Stablecoins and staking** (iv-b05, iv-u06): tax treatment unsettled; the language can carry either answer.
8. **The PROPOSAL regresses budgets** by dropping the `income` root (section 5).
9. **Everything numeric was computed outside Axiom**, by scripts that are not in the repository, and the syntax was never parsed.
10. **My citations** are mostly [S] or [M]; the six repositories I cloned are [V]. THEORY.md lists what I could not check.

