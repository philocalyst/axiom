# Theory for Axiom: research report

I read `v2/DESIGN.md`, `v2/LANGUAGE.md`, `examples/v4-sketch`, `examples/04-freelancer` and `crates/systems/src/us/401k.ax`, and changed nothing. Where the proxy blocked a website, I cloned the primary source instead: the ValueFlows ontology TTL, the ACTUS dictionary and tech spec, the HIPERFIT Coq contracts, and POETS/CSL.

Three things in the current book motivate several proposals below:
- **Miles are faked as money.** `odometer -> mileage 44 MI` moves a fake commodity between fake accounts, and the IRS rate is stored as a *price* (`2025-01-01 MI 0.70 USD`).
- **The office share hides its basis.** It is written `business 12% for studio // 120 of its 1,000 sq ft`, with the real basis in a comment.
- **Some law cannot be written.** The deferral-limit doc says an excess "must also be returned … before April 15 … or it is taxed twice". The early-withdrawal fix says "if an exception applies … mark it `!`". Neither can be expressed as law today.

## 1. ValueFlows

Sources: https://www.valueflo.ws/concepts/actions/ and `all_vf.TTL` (https://github.com/valueflows/valueflows, `mkdocs/docs/assets/`).

**Core idea.** Every flow has an `action`, whose data row says how it changes a resource. Flows come in four kinds:
- **EconomicEvent**: what happened;
- **Commitment**: what was agreed, for the future;
- **Intent**: what is offered or wanted, with no agreement yet;
- **Claim**: started by the receiver and `triggeredBy` an event, for example "logged work triggers a claim for income".

A resource has two quantities. `accountingQuantity` counts rights and `onhandQuantity` counts custody. An event carries a `resourceQuantity` (it moves resources) and/or an `effortQuantity`, which "does not affect economic resources" (hours, cycles). Its time is `hasPointInTime`, or `hasBeginning` and `hasEnd`, plus `due`.

Duality is explicit. An Agreement `stipulates` primary commitments and `stipulatesReciprocal` the counterpart ones, and events are `realizationOf` or `reciprocalRealizationOf` it. Corrections are new events that `corrects` an earlier one. Measures are OM2 `Measure = hasNumericalValue + hasUnit`, and VF deliberately treats currencies as resource specifications, not units.

**The 19 actions, as the TTL defines them (rights/custody effect), mapped onto Axiom:**

| action | rights / custody | quantity | Axiom |
|---|---|---|---|
| transfer | −+ / −+ | resource | a flow owner↔party |
| transfer-all-rights | −+ / · | resource | title passes, thing stays (rare) |
| transfer-custody | · / −+ | resource | money held `for dana`; asset `at ACCOUNT` |
| move | −+ / −+ (same agent) | resource | `checking -> savings`: parcels unchanged |
| produce / consume | + / + ; − / − | resource | a part added; `consume` (depreciation) |
| combine / separate | contained-in set/cleared | resource | parts joining an asset; sale relieves them |
| accept → modify | custody out/in, same resource | resource | `#repair of condo`: the same thing before and after |
| use | none | resource **and** effort | business use of car or home: **missing** |
| work | none | effort | hours worked: **missing** |
| cite | none | resource | n/a |
| deliver-service | none (output and input at once) | resource | subscriptions (`covers`) |
| pickup / dropoff | custody, same resource | resource | n/a |
| raise / lower | + / + ; − / − | resource | `opening`, `!`-accepted gaps; `via market` is "the real action when known" |
| copy | + to receiver | resource | n/a |

**Mapping.** Axiom already covers the resource side. What it lacks is **effortQuantity**: a dated measure that moves nothing, which other amounts are computed from (see Proposal 4). REA duality in Axiom: an exchange flow is a realized dual pair on one line. A claim is a reciprocal that has not been realized yet, and a `work` event (the give) creates a claim for the take (VF's own example).

## 2. ACTUS

Sources: https://www.actusfrf.org/, https://github.com/actusfrf/actus-dictionary and https://github.com/actusfrf/actus-techspecs.

**Core idea.** A contract is a set of **terms** that generate a **schedule of typed events**. At each event, a payoff function `POF(terms, state, observed)` gives the cash flow, and a state transition `STF` updates the state:
- `NT`: notional;
- `IPNR`: rate;
- `IPAC`: accrued interest;
- `PRNXT`: next payment;
- `SCNT` and `SCIP`: scaling multipliers;
- `PRF`: performance, one of performing, delayed, delinquent or default;
- `MD`: maturity.

The outside world enters only through observers: `O^rf(marketObject, t)` for an index, and `O^ev(CID, PP, t)` for unscheduled prepayments. Everything else is declarative.

**Contract types for households:**
- **PAM**: bullet repayment.
- **ANN**: level payment; a rate change *recomputes the payment*.
- **NAM**: the payment stays fixed and *maturity shifts*.
- **LAM**: fixed principal.
- **LAX**: stepped schedules.
- **UMP**: principal in and out at any time, with interest capitalized. It covers an overdraft, a card or a HELOC, with `CLA`, the credit-line amount.
- **CLM**: callable.
- **CSH, STK, COM**: holdings.

**Event types:**
- **IED**: initial exchange.
- **IP**: interest payment.
- **IPCI**: interest capitalized.
- **PR**: principal redemption.
- **PD**: drawing on a credit line.
- **PP**: prepayment. Its term `PPEF` is N (none), A (lower the payments) or M (shorten the maturity).
- **PY**: penalty.
- **FP**: fee.
- **RR**: rate reset from a market index: `Δr = clamp(O(RRMO)·RRMLT + RRSP − IPNR, RRPF, RRPC)`, then the rate is clamped by the life floor and cap `RRLF`/`RRLC`.
- **RRF**: reset to a known rate, `RRNXT`.
- **SC**: indexation, where `SCEF` scales interest and/or principal.
- **MD, TD, CE**: maturity, termination, credit event.

**The minimal household term set, in Axiom words:**
- `IED`, `NT`, `IPNR`, `IPDC`, `MD`: `loan 320_000 USD on 2024-02-20 at 5.875% over 30y [days actual/365]`.
- `PRCL`, `IPCL`: the cadence.
- The RR group (`RRCL`, `RRANX`, `RRMO`, `RRSP`, `RRMLT`, `RRPC`/`RRPF`, `RRLC`/`RRLF`): `resets yearly from 2029-03 to sofr + 2.75% cap 2% life 5%`.
- `PPEF`, `PYTP`/`PYRT`: `prepay shortens|recasts [penalty 2%]`.
- `IPCED`: `capitalizes until 2027-06` (student-loan deferment).
- `CLA`: `credit line 10_000 USD`.
- `SCMO`, `SCCL`, `SCEF`: `indexed to cpi yearly`.
- `GRP`, `DQP`: `grace 15d`, `delinquent 60d`.

**Mapping.** Axiom already contains three pieces of ACTUS without naming them:
- "a flow to the contract pays principal alone … the loan ends sooner" is **PP with PPEF=M**;
- `2029-03-01 mortgage at 6.25%` is **RRF**;
- "a new rate refigures the payment" is **ANN** (NAM would keep the payment).

The ad hoc rule "kept … within half a cadence" should become ACTUS's grace and delinquency periods.

## 3. Composing contracts, and CSL

Sources:
- Peyton Jones, Eber and Seward (2000): https://www.microsoft.com/en-us/research/publication/composing-contracts-an-adventure-in-financial-engineering/
- Bahr, Berthold and Elsman (2015), in Coq: https://github.com/HIPERFIT/contracts (`Coq/Syntax.v`)
- CSL: Andersen et al. 2006 (https://link.springer.com/article/10.1007/s10009-006-0010-1); Hvitved 2012 (https://di.ku.dk/english/research/phd/phd-theses/2011/hvitved12phd.pdf); POETS (https://github.com/legalese/poets)

**Core idea.** Peyton Jones, Eber and Seward (PJE) define `zero, one k, give, and, or, cond o, scale o, when o, anytime o, until o` over observables (`konst`, `lift`, `date`). A contract is a value, and valuation is compositional.

Bahr and colleagues cut the core to seven constructors, `Zero | Let e c | Transfer p q a | Scale e c | Translate d c | Both c c | If e d c1 c2` ("if e within d then c1 else c2"), with observables `Obs(label, offset)` and a bounded accumulator `Acc`. With that core they prove causality and horizon properties.

CSL adds what commercial contracts need: **responsibility and deadlines**.
- `<p> T(x) where pred due within d remaining r then c` is an obligation, and a breach **blames p**.
- `when T(x) … due within d then c1 else c2` is a permission, whose `else` fires at the deadline.
- `and`, `or`, `if`, `fulfilment`, and recursive parameterised clauses complete the language.

POETS's own sale shows the style:

```text
clause payment(lines, me, deadline)<customer> =
  if null lines then fulfilment
  else <customer> Payment(sender s, receiver r, money m)
         where s == customer && r == me && checkAmount m lines
         due within deadline remaining newDeadline
       then payment(remainingOrderLines m lines, me, newDeadline)<customer>
```

Monitoring is **residuation**: each event rewrites the contract into what is still owed. Axiom's claim, "the time between an event and its counterpart", is exactly the residual of a dual pair.

**A core for Axiom (nine variants):**

```rust
enum Promise {
    Done,                                        // fulfilment / zero
    Pay(LegId),                                  // from, to, amount: Expr, purpose (one+give+scale)
    All(Box<[Promise]>),                         // and / Both
    On(DateExpr, Box<Promise>),                  // when / Translate
    Due { within: Span, blame: Agent, what: Box<Promise>, otherwise: Box<Promise> }, // CSL obligation + reparation
    If(Expr, Box<Promise>, Box<Promise>),        // cond on terms, indices, balances
    Choose { by: Agent, until: DateExpr, take: Box<Promise>, leave: Box<Promise> }, // or/anytime: prepay, renew, cancel
    Every(Schedule, Box<Promise>),               // the only recursion: a calendar
    Let(Name, Expr, Box<Promise>),               // fix an observable that day (rate reset, CPI)
}
```

**How the existing constructs compile:**
- **Invoice.** `halcyon owes studio 3_800 USD due 30d` becomes `Due{30d, halcyon, Pay, Done}`.
- **Rent with a late fee.** `Every(monthly on 1, Due{5d, me, Pay(rent), Pay(rent + 5%)})`.
- **Deposit.** `All[Pay(dana→checking for dana), On(end, Due{30d, me, Pay(→dana), Done})]`.
- **Loan.** `All[Pay(rocket→me, NT), Every(monthly, Let(r, rate, Due{grace, me, Pay(annuity(balance, r, left)), …}))]`.
- **Match, escrow, share.** Each is one more `Pay` in the body.
- **Escalations, promotions and waivers change no structure.** `Pay` amounts read term timelines (§6), and `waived` fulfils the `Due`s it covers.

**What the engine does with it.** `check` is residuals past their deadline, and blame says who owes. The claims view is the open `Due`s. The forecast unfolds the promise.

## 4. Defeasible deontic logic

Sources:
- Nute (1994): https://en.wikipedia.org/wiki/Defeasible_logic
- Maher, "Propositional defeasible logic has linear complexity" (2001): https://dl.acm.org/doi/10.1017/S1471068401001168
- Governatori and Rotolo, FCL (2005): https://www.worldscientific.com/doi/abs/10.1142/S0218843005001092
- Catala (ICFP 2021): https://arxiv.org/abs/2103.03198

**Core idea.** Nute's defeasible logic has:
- strict rules `→`;
- defeasible rules `⇒`;
- **defeaters** `⇝`, which only block a conclusion;
- a **superiority** relation `>`.

Propositional inference is linear-time (Maher).

Governatori's Formal Contract Logic adds `⊗`. A rule `r: a ⇒ O(A ⊗ B ⊗ C)` means A is obligatory; if A is violated, B becomes obligatory; if B is violated, C does. A violation whose reparation is met is *compensated*, not non-compliant. Obligations come in three kinds:
- **achievement**: by a deadline, and *preemptive* if it may be met early;
- **maintenance**: holds throughout;
- **punctual**.

Catala makes the same idea practical for tax statutes: a base definition plus labelled `exception`s. If two exceptions apply at once, that is a conflict error.

**Mapping onto Axiom's laws:**
- `require A else E` is `O(A ⊗ E)`. Generalize it to chains. The 402(g) excess is a real three-link chain: `require tally(elective-deferrals) <= cap else owe corrective-distribution by 04-15 else count excess as wages`.
- `warn A` is `O(A ⊗ ⊤)`: it is compensated by being reported.
- `on in/out` laws are punctual obligations, `always` laws are maintenance, and `by DATE` and `due` are achievement. "Kept by the nearest occurrence" is preemptive.
- The defeaters are `!` (one item), `waived` (occurrences in a span), and a proposed `unless COND` (one law). `--relaxed` demotes strict rules to defeasible ones. §72(t)'s exceptions then become `unless owner.separated-at >= 55y or …` in `us/401k`, instead of a hand-written `!` per withdrawal.
- **Superiority follows the three legal canons, built in:**
  - *lex posterior*: a later statement beats an earlier one for the days it covers. Axiom already does this, and `until` makes it an override rather than an overwrite, which is the defeasible behaviour.
  - *lex specialis*: a same-named law on a thing beats the one on its kind, which beats the one on the parent kind.
  - *lex superior*: project, then child system, then parent system.
- Anything else is an explicit `law X overrides Y`. Two applicable rules of equal rank that disagree are an error, as in Catala.

## 5. Units of measure

Sources: Kennedy, "Types for units-of-measure: theory and practice" (CEFP 2009), http://typesatwork.imm.dtu.dk/material/TaW_Paper_TypesAtWork_Kennedy.pdf, and F# `[<Measure>]`.

**Core idea.** Units form a free abelian group: a unit is a product of base units with integer exponents. `+` and comparison need equal units, `*` and `/` add and subtract exponents, and a conversion is itself a value of type `u1/u2`. Inference needs abelian-group unification. Without unit polymorphism, however, *checking* is plain bottom-up synthesis.

**How it would work in Axiom:**
- **Representation.** `Unit = SmallVec<(BaseId, i8)>`, normalized. The base units are commodities (`USD`, `VTI`), measures (`MI`, `HR`, `KWH`, `SQFT`) and calendar units (`d`, `mo`, `yr`); `%` is dimensionless.
- **Typing.** `@ 285.70 USD` on VTI has type `USD/VTI`, and `3_050 USD monthly` has type `USD/mo`.
- **Conversion.** `value(x, U [at POLICY])` is the only conversion, and the policy names a dated timeline: the spot price, the IRS yearly average, or a param.
- **Every leaf has a unit, so nothing is inferred:**
  - literals carry theirs;
  - properties declare theirs (`has rate USD/MI`), and so do params;
  - `tally(x)` is fixed by its first `count`;
  - `amount` in a law is the subject's commodity, known from `holds`. This is the only "variable" per law.
- **Errors look like this:** "adds 1,200 EUR to `elective-deferrals`, a tally in USD; convert with `value(amount, USD)`".

## 6. Temporal patterns

Sources:
- Fowler, temporal patterns: https://martinfowler.com/eaaDev/timeNarrative.html
- Fowler, bitemporal history: https://martinfowler.com/articles/bitemporal-history.html
- Snodgrass (1999): https://www2.cs.arizona.edu/~rts/tdbbook.pdf
- Elliott and Hudak (1997): http://conal.net/papers/icfp97/

**Core idea.** Fowler describes three patterns:
- **Temporal Property**: an accessor that takes a date;
- **Effectivity**: a value with an explicit validity range;
- **Temporal Object**: versions of a continuing thing.

Bitemporal data separates valid (actual) time from transaction (record) time. Valid history may be corrected, but record history is only ever extended.

In FRP, *behaviors* are values over time and *events* are dated occurrences. `stepper x ev` turns events into a piecewise-constant behavior, `switcher`/`untilB` swaps behaviors, and `integral` accumulates.

**Mapping onto Axiom:**
- **Events** are flows and measures, dated by a point (`DATE`) or an interval (`for PERIOD`), as in VF.
- **Behaviors** are step functions: terms, properties, prices, params, budgets, residence and limits. Statements are the `stepper` events, and the declaration is the initial value ("the declaration is simply the first").
- **`until`** is Effectivity on a change, plus a switch back to what held before.
- **Balances** are the integral of flows. Recognition (`for 2025`, `covers`) is a constant rate over an interval.
- **Corrections** are new facts, so record time only extends. Axiom needs record time only for what was published outside the book: a filed return (Proposal 7).

## 7. Others worth adopting (four picks)

1. **ISO 20022 camt.053.** Sources: https://validatefin.com/en/blog/camt053-bank-statement and https://developer.gs.com/docs/services/transaction-banking/camt-053-us-sample. Every field of an entry maps to a concept Axiom already has:
   - `BookgDt` vs `ValDt`: the booking day and the value day;
   - `Sts` BOOK or PDNG: pending;
   - `BkTxCd` Domain/Family/SubFamily (e.g. PMNT/ICDT/STDO, a standing order): a contract occurrence or purpose hint;
   - `EndToEndId`, `RmtInf/Strd/CdtrRefInf/Ref` (an ISO 11649 RF reference) and `RfrdDocInf/Nb` (an invoice number): **codes**;
   - `Cdtr` vs `UltmtCdtr`: exactly **`via`**.

   Structure beats memo globs wherever a bank sends it.
2. **XBRL Open Information Model** (https://www.xbrl.org/Specification/oim/REC-2021-10-13/oim-REC-2021-10-13.html). A fact has the core dimensions concept, entity, period (instant or duration, fixed by the concept) and unit, plus taxonomy dimensions. Axiom's tallies are exactly such facts, `(name, owner, year, unit, [dims])`. Statements are instants and flows are durations. Adopt it as the `--json` shape of tallies. Units (`USD` vs `USD/mo`) enforce the stock/flow split for free.
3. **The network view of double entry.** Sources: Ellerman, https://arxiv.org/abs/1407.1898; Arya, Fellingham, Glover, Schroeder and Strang, "Inferring transactions from financial statements" (CAR 2000), https://onlinelibrary.wiley.com/doi/abs/10.1506/L0LW-NX5L-4WUR-9JKL. Balances are the incidence matrix times the flows. This gives `?` a crisp rule: the unknowns between passing assertions are determined **iff the `?` flows form a forest** in the account graph. A cycle is underdetermined, and the error can name it.
4. **Incremental recomputation.** Sources: McSherry et al., CIDR 2013, https://www.cidrdb.org/cidr2013/Papers/CIDR13_Paper111.pdf; Mokhov, Mitchell and Peyton Jones, "Build systems à la carte", https://www.microsoft.com/en-us/research/wp-content/uploads/2018/03/build-systems.pdf. Differential dataflow's partially ordered (time × revision) versions are bitemporal data, but the cheap part is **early cutoff**:
   - snapshot the fold at month ends;
   - an edit dated d restarts from the snapshot before d;
   - the fold stops at the first later snapshot whose state hash is unchanged and past which no input changed.

   This needs no Datalog, and the fold stays sequential.

**Not picked:**
- **Grigg's triple entry** (https://iang.org/papers/triple_entry.html): its insight, one event seen from two books, is already sync's reconciliation. Ijiri's momentum triple entry matters more (Proposal 3).
- **Ledger's automated and periodic transactions**: already subsumed by derived events and contracts.
- **hledger CSV rules**: regex-to-account mapping per file. `known-as` identity is better, and camt structure is better still.

## Proposed unifications for Axiom

In priority order.

**1. Contracts and claims become one thing: a promise.**
- **Unifies:** contract, claim, invoice, IOU, deposit, reimbursement, late occurrence, standing order.
- **New construct:** the surface syntax stays (`contract`, `owes`, `due`), plus reparation: `due 5d else + 5% #late-fee`, and `grace 15d`. Everything compiles to the nine-variant `Promise`. A claim is an open `Due` in a residual; it has a `blame` agent and is settled by what fulfils it (VF `fulfills`).
- **Removes or simplifies:** the separate claim store, the special cases for late occurrences and deposits, and the "half a cadence" heuristic. `check`, `claims` and `forecast` all become one monitor.
- **Rests on:** REA duality, CSL, PJE, Bahr et al., and VF's Commitment and Claim.

**2. One `Timeline<T>` for everything that changes on a day.**
- **Unifies:** properties, contract terms, budgets, prices, params, residence, promotions and waivers.
- **New construct:** a declaration is the initial value, and each statement is a step. `until` is a layered override: the step beneath resumes afterwards. Param rows become statements dated January 1, keyed by extra keys such as filing status. `lives us/ny from … until …` becomes `DATE me lives us/ny`.
- **Removes:** `from`/`until` on individual properties, param's special year lookup, and budget-change special handling. Laws simply sample the timeline on the judged day.
- **Rests on:** FRP `stepper`/`switcher`, Fowler's Temporal Property and Effectivity, and *lex posterior*.

**3. Typed units, with rates as momenta.**
- **Unifies:** prices, FX, statutory rates, percentages and contract amounts.
- **New construct:** unit expressions such as `USD/MI`, `EUR/USD` and `USD/mo`. `2_900 USD monthly` has type `USD/mo`, which is a rate: Ijiri's *momentum* and XBRL's duration. A balance is `USD`, an instant. `value(x, U at POLICY)` is the only conversion.
- **Removes:** the price-file hack (`MI 0.70 USD` becomes `param mileage-rate 2025 0.70 USD/MI`) and silent currency mixing. The forecast is momenta integrated. A statement that changes a rate is an Ijiri *impulse*, and `check`'s "three occurrences differ" is impulse detection.
- **Rests on:** Kennedy, Ijiri and XBRL `periodType`.

**4. Norms are defeasible, with fixed canons.**
- **Unifies:** `!`, `waived`, `--relaxed`, `until`, `else`, and system override.
- **New construct:**
  - `require A else B else C` (a ⊗-chain; the 402(g) excess);
  - `unless COND` (a defeater; §72(t) exceptions written once in `us/401k`);
  - priority: explicit `overrides`, then specialis (thing > kind > parent kind), then superior (project > child system > parent system), then posterior for dated statements;
  - two applicable rules of equal rank that disagree are an error.
- **Removes:** file-shadowing in `systems/`, and per-flow `!` for exceptions the statute already lists.
- **Rests on:** Nute, Maher (linear time), Governatori's FCL and Catala.

**5. Measures are statements with a quantity and a purpose.**
- **Unifies:** mileage, hours worked, kWh, floor area, and allocation bases.
- **New construct:** `DATE THING QUANTITY [#PURPOSE] [for OWNER] [^code]`, where the quantity is in a `measure` unit. For example:
  - `21 car 44 MI #business-travel for studio`
  - `12 me 6.5 HR #consulting for halcyon ^inv-12`

  Nothing moves. Purpose laws read these lines as they read flows, so `count amount * mileage-rate[year] as car-expense` checks as `MI·USD/MI = USD`. `business 120 SQFT for studio` computes its share against `flat area 1_000 SQFT`. A contract priced `150 USD/HR monthly` turns a month's hours into a claim.
- **Removes:** the fake `odometer`/`mileage` accounts, and percentages justified in comments.
- **Rests on:** VF `use`/`work` and effortQuantity, OM `Measure`, and REA duality (work → claim).

**6. Loan and escalation terms speak ACTUS.**
- **Unifies:** fixed loans, ARMs, prepayments, card and HELOC interest, student-loan capitalization, and CPI-indexed rents.
- **New construct:** the clauses `resets yearly to sofr + 2.75% cap 2% life 5%`, `prepay shortens|recasts [penalty X]`, `capitalizes until DATE`, `credit line AMOUNT`, `indexed to cpi yearly` (on *any* contract) and `delinquent 60d`. They compile to ACTUS events (RR, RRF, PP, IPCI, PD, SC) with ACTUS's state transitions and payoffs. Indices are params that sync brings in.
- **Removes:** hand-written yearly escalations, and the one-off "a flow to the contract pays principal alone" rule, which becomes `prepay shortens`, the default.
- **Rests on:** ACTUS PAM, ANN, NAM, LAM and UMP.

**7. `filed` is the only transaction time.**
- **Unifies:** corrections, amended returns and reconciled periods.
- **New construct:** `2026-04-15 tax 2025 filed`, with indented tally assertions giving what the return said. These are authored facts. A later edit that changes them produces an *amendment* diagnostic listing each changed line, rather than silent drift.
- **Removes:** any need for a general bitemporal store. It makes "nothing written is silently reinterpreted" hold for taxes.
- **Rests on:** Snodgrass, Fowler's bitemporal history, and VF `corrects`.

**8. Structured sync.**
- **Unifies:** recognition of codes, `via`, pending and standing orders.
- **New construct:** `sync checking` with `camt053 FILE-GLOB` takes:
  - codes from `EndToEndId`, `CdtrRefInf` and `RfrdDocInf`;
  - `via` from `Cdtr` vs `UltmtCdtr`;
  - pending from `Sts`;
  - an occurrence or purpose hint from `BkTxCd`.

  `known-as` remains for CSV feeds that give only a memo.
- **Removes:** code-glob and memo heuristics wherever a bank sends structure.
- **Rests on:** ISO 20022, and the triple-entry insight that one event is seen from two books.
