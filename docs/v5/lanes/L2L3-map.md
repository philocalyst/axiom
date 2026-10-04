# L2/L3 map: positions under their agent, an optional counterparty, purposes without a direction root

Written before any code of lanes L2 and L3, from the v5 head `d52032c` (C3 merged; C4 and C6 still open in their lanes),
and checked against what the code does: the last section says how. Paths are in `crates/`; line numbers are those of
`d52032c`. Line counts are `quality.py`'s (code lines: no blanks, comments or tests), of the functions named, read one by
one. The vocabulary is K3b's (an *address*, a *filler*, the *index*), K3f's (a *tab*, a *claim place*), K5d's (a loan's
*schedule*) and L1's (the *junction*). The baseline binary is `scratchpad/laneu/axiom-base` (built from `f1baf39`, which
`d52032c` changes only in docs).

**Status at the end of the pass (2026-10-04):** this is Phase A, the design, and it is complete. Nothing of L2/L3 is built.
A later session builds from it after the user has answered section 10's decisions. Everything below was read or measured on
the code at `d52032c`; the proofs of section 6 are a plan, not results.

## 0. The short answer

The four changes are worth making, and the language reads better after them. But **they delete little code by
themselves**. Nesting a position under its agent is a change to the grammar (the model already stores the agent: `at` fills
`Role::Account { institution }`). A debt written once is a different link, not fewer types. Purposes without `income` and
`spending` as roots save a few lines, and they have to settle a question the proposal thought was already settled.

The one large deletion comes from L3. Once a flow needs no party, **every party can be declared**. Then the walk that
invents parties from mentions (`declare/mentions.rs`), the implied-party pass (`declare/parties.rs`), K3b's address gate
and its list of references all go: about **−340 lines** of `model`, all of them whole functions and one whole module. A
mistyped party becomes an error with a suggestion, where today it silently becomes a new party.

| step | what | model | syntax | engine | report | net |
|---|---|---:|---:|---:|---:|---:|
| 1 | L3a: the counterparty is optional | +5 | +6 | | | **+11** |
| 2 | every party is declared (follows from 1) | −328 | | | | **−328** |
| 3 | L2a: positions nested under their agent | +4 | +48 | | | **+52** |
| 4 | L2b: a debt is a promise; a claim is a position with any party | +10 | | −6 | | **+4** |
| 5 | L3b: purposes without a direction root (after C4 and C6) | −3 | | −9 | −12 | **−24** |
| | **all** | **−312** | **+54** | **−15** | **−12** | **−285** |

Step 2 is whole items, read and counted: it holds at about 100%. Steps 1, 3 and 4 are additions estimated by writing them
out, and steps 3 and 4 add more than they delete; section 4 gives the reason for each, which is not the count. The C1 to C3
ratio (28%) is a ratio of unifications. It does not apply to whole-module deletions, but it does apply to step 5. Central
estimate: **about −270 net**. The books shrink much more than the code:

- 94 `at`s, and every institution written twice;
- 160 `: spending`/`: income` parents;
- 69 flows of `05-family` alone that name an invented party;
- one of every loan's two or three records.

Two decisions are bigger than the steps and are yours (section 4.6):

- **`fmt --upgrade`** writes `account … at`, which the new grammar refuses. It must either learn to nest (+~80) or be
  retired, with the v4 hint (−~450 in `syntax` and `cli`).
- **The paystub written through its owner** (lane D's gap 5) does not fall out of L2/L3 without a lowering change (+~10).

## 1. What the brief and the proposal say, and what the code does

Twelve things do not match. Items 1 to 6 decide the design.

1. **§6.3's premise is half right.** The engine derives the direction in which a flow crosses the owners' boundary
   (`lib.rs:134 purpose_direction`: source owned → `Out`, target owned → `In`; the root only decides a flow between two owners,
   where `Capital` counts `Out`). But **whether money in is income or a refund is decided by the purpose's root**, not the
   party. Three readers decide it:
   - `eval.rs:1627 purpose_net`: `Income` → in − out, anything else → out − in, read by `purpose_total`, `budget_total` and a
     share limit;
   - `fire.rs:309`: the budget cap's shortcut, the same match;
   - `report/flow.rs:570`: the flow view's sign.

   The party only *supplies* a purpose when none is written (`lower/infer.rs:83 said_at`: money from a grocer is
   `#groceries`). The corpus has refunds that depend on the root: `card <- amazon 189.99 USD #household` and `card <- target
   62.40 USD #kids` (`05-family` and its copy), and `joint-checking <- state-farm 7_940.00 USD #repair of car2`
   (`explore-v5/02-family`). Neither `amazon` nor `target` has a purpose of its own (`: store`). So "the party still decides
   refund versus income, as today" is not what happens today, and §4.5 has to choose.
2. **§6.4's two examples contradict each other under §6.2.** "The counterparty defaults to the position's agent" (`31 savings
   <- 170.70 USD #interest` is from chase). But after §6.2 *every* position has an agent, so the same rule makes `06 card ->
   94.21 USD #dining` a payment to chase. That is §6.4's own first example, which says "no `restaurant` party". §4.1 takes
   one rule and says why.
3. **`checking -> jo 600 USD due 04-01` makes no claim** (lane D's gap 1, shown on the baseline). `check` passes with net worth
   4,600.00 and `claims` says "Nothing is owed either way. A flow with `due` into a receivable place makes one". The engine
   makes a claim only of value that reaches a claim place (`said.rs:199 makes_claim`), and a party's outside place is none.
   This **falls out** of "a claim is a position with any party": a `due` flow to a party goes into the position the owner has
   with it, which is the tab (§4.4).
4. **A loan is written as two or three records, and the third is not always visible.**
   - `07-landlord`: a contract `home-loan with lender` with `loan …`, plus the tab `world.tab(lender, me, debt)` that
     `loan_endpoint` (`lower/contracts.rs:153`) makes and `contract_endpoints` lets the contract's name stand for.
   - `05-family`: the same, *and* `account mortgage : mortgage at lender`. 2025 is written into the account; the contract
     (`mortgage-payment`, from 2026) has its own empty tab. Its golden balance shows the account's 406,692.02 and no tab.

   Merging them (§4.4) needs one new rule: **a loan whose position already has a balance on its `on` day restates that
   balance**, and does not lend a second principal.
5. **Nesting is a grammar change, not a model change.** K3b §7.1 predicted it: in an `entity` block, a nested position is
   `Decl { what: Account, at: Some(container) }`. Everything the model does with an account (`declare_accounts`, the place
   tree, `Role::Account { institution }`, the address index) already reads exactly that. So L2a deletes the resolution of the
   `at` word and its diagnostic, about −12 lines, and the parser grows.
6. **Implied parties exist so that a journal may name a party nobody declares**, and they cost a walk and a pass.
   - `declare/mentions.rs` (155 code lines) walks every item of every source, before any entity exists, to find each name
     written where a party can stand.
   - `declare/parties.rs::implied_parties` (42) makes the parties nothing else claims, after spelling out every path, kind,
     purpose, commodity, system and asset (`add_path_spellings`, `spellings`).
   - `strict_path_suffixes` (17) keeps a written suffix from making two parties.
   - K3b added `Addressed` (31) and the book's gate (`Addresses::used`). Both exist only so that a mention that is an address
     attempt does not become a party, and so that a book with no address keeps its implied parties.
   - The same walk feeds `settle_addresses` its list of references (the memo, `reference.rs:95`).

   With an optional counterparty, the reason to name an undeclared party goes. Sync never writes one: it writes a declared
   name or `?` with the memo, `sync/world.rs:567`. The corpus has about 25 implied parties in seven books: `ssa` and
   `edd` where `us` is used without declaring them; real payees in `11-sam`, `explore-v5/02-family` and `04-nomad`
   (`amazon`, `taqueria-cancun`, `two-men-and-truck`, …); `04-freelancer`'s `meal-vendor`. Beside them, institutions that
   `at` names and nothing declares are `unknown-institution` errors today: `explore-v5/01-agency`, `02-family`,
   `03-triplex`, `04-nomad`, `05-budgeter` and `v4-sketch`.
7. **`kind receivable` and `kind payable` duplicate the built-in `claim` and `debt-claim`** (std.ax:30-33: an `asset`/`debt`
   kind that says `claim`, which is what the built-ins are). Their declared uses are few:
   - `04-freelancer`'s `account receivable : receivable`, whose only reader is `code inv-* on receivable`;
   - `07-landlord`'s `bills : payable at summit-roofing` and `deposits : payable`, which are only asserted. Their 16
     assertions say a bill or a deposit is held where nothing puts one, and three of them are among the golden's five errors
     today (lane D: `deposit` is not built);
   - four `diff/cases2` probes.

   `09-shared` already keeps its claims in tabs (`ben owes me`). A code rule's `on` scope is resolved and never enforced
   (`sync_lower.rs:392`; no reader of `CodeRule.on` outside it), so `on receivable` becomes `on claim` with no other effect.
8. **`bank` is a place kind** (std.ax:13, `kind bank : deposit`, with the overdraft law). §6.2's `entity chase : bank` needs
   an entity kind of that name. std has `lender` (org) but no `broker` or `card-issuer`.
9. **`fmt --upgrade` (L1) writes `account NAME : KIND at PARTY`**, which the grammar after L2 refuses. The upgrade must learn
   to nest and to declare parties, or go (§4.6).
10. **Where `account` is written.** 130 account declarations in the example books, 94 of them with `at`. 571 lines in Rust
    test sources (78 in `engine/src/source_tests.rs`, 57 in `model/tests/native_records.rs`, 49 in
    `report/src/source_tests.rs`, 39 in `model/tests/addresses.rs`, …). And the code blocks of LANGUAGE.md and DESIGN.md.
    The porter has to port all of it (§6).
11. **`kind loan`'s `rate` and `maturity` slots** (std.ax:24-26, read by its `payoff` law) say what a loan promise's `at RATE
    over SPAN` says. `08-expat`'s `student-loan : loan` has the slots and no schedule. A debt that is a promise has both.
12. **The income root is read by name in two places.** `us/ira.ax:40` (`when not purpose is #income`: an arrival that is not
    income is a contribution) and LANGUAGE §6's `budget fun 10% of #income monthly` (no book writes it; `diff/cases2` writes
    `20% of #pay`).

## 2. The language after

### 2.1 The grammar

```text
entity NAME [: KIND] [#PURPOSE]
  PROPERTY ARG*                             // as today: born, filing, lives, member, owner, of, known-as, purpose …
  law … | also … | has …                    // as today
  PATH : KIND                               // a position this entity holds for the book's owners
    PROPERTY ARG*                           //   owner, employer, beneficiary, holds, known-as, opened … ; law, also
    loan AMOUNT on DATE at RATE over SPAN [for ASSET]   // its promise, when it is one: the contract's lines
    AMOUNT CADENCE [on DAY] from|into HOLDING
    from DATE | until DATE | also … | -> LEG …
  PATH                                      // a position written as an address (K3b): `alex/401k`, kind by its last word

FLOW := SUBJECT -> [OBJECT] AMOUNT [@ PRICE] TAIL     // the object may go when the tail says why: a purpose or a description
      | SUBJECT <- [OBJECT] AMOUNT TAIL
```

- **What goes.** The `account` keyword and `at PARTY`, each with a hint naming the new form (`removed-form`). The purposes
  `income` and `spending` as parents. `kind receivable`, `kind payable`, `interest-income`. A loan's separate contract
  (it is the position's lines). Undeclared parties.
- **A nested line is a position when it has `: KIND`, or when its name is a path of two words or more.** A property line
  never has a `:` (LANGUAGE §1: `:` only means "is a kind of", in declarations), and a property's name is one word. A
  bare one-word position (§6.2's `brokerage`) is not accepted: it cannot be told from a property with no argument.
- **A position's owner** is its `owner` line, else its fillers (K3b), else its agent when the agent is one of the owners
  (`entity me`, a member, a business: a position held by its owner, like cash), else `me`.
- **A position with no institution** nests under its owner: `entity me` / `wallet : cash`.

### 2.2 `05-family`: the institutions and March

What the porter writes. Names are kept, so every report row reads as before. The `at` lines become nesting, the mortgage
written twice becomes one position, and the seven invented parties go, by hand, in the one book that shows them:

```text
entity chase : bank
  joint-checking : deposit
    owner family
  joint-savings : deposit
    owner family
  card : credit-card
    owner family

entity fidelity : broker
  alex-401k : 401k
    employer acme
  jordan-401k : 401k
    owner jordan
    employer bluefin
  hsa : hsa
    coverage family
  riley-529 : 529-plan
    owner family
    beneficiary riley

entity acme : employer
  dcfsa : dependent-care-fsa
    owner family

/// The mortgage is one position: 2025 is written into it, and its remaining term, which starts with the
/// independently stated balance on 2025-12-31, is its promise.
entity lender : lender
  escrow : escrow
    owner family
  mortgage : mortgage
    owner family
    loan 406_692.02 USD on 2025-12-31 at 5.875% over 27y6m for house
    monthly on 1 from joint-checking
    from 2026-01-01
    also -> escrow 705 USD #contribution

entity honda-finance : lender
  car-loan : loan
    owner family
    loan 16_976.91 USD on 2025-12-31 at 4.9% over 30m for crv
    monthly on 5 from joint-checking
    from 2026-01-01
```

The same, written the way K3b's addresses let it be (`explore-v5/06-family-addresses`), with the relations in the names
instead of beside them. That is the idiom; the porter does not write it:

```text
entity chase : bank
  family/checking : deposit
  family/savings  : deposit
  family/card     : credit-card
entity fidelity : broker
  me/acme/401k
  jordan/bluefin/401k
  me/hsa
    coverage family
  family/riley/529 : 529-plan
```

March, from the 87 lines of today. The three changes are the purpose-less parties, the interest, and nothing else; the pay
lines stay as they are (the paystub as a promise is §6.5's, not L2/L3's):

```text
2025-03

01 joint-checking -> mortgage     469.85 USD ^mortgage-2025-03   // principal
01 joint-checking -> lender     2_014.61 USD #interest of house  // interest
01 joint-checking -> escrow       690.00 USD                     // taxes and insurance
02 card -> netflix    17.99 USD
03 card -> costco    158.77 USD
06 card -> 94.21 USD #dining                                     // was `card -> restaurant 94.21 USD #dining`
13 card -> 94.80 USD #dining
…
31 joint-savings <- 170.70 USD #interest                         // was `<- interest-source 170.70 USD #interest-income`
31 joint-checking = 23_072.46 USD
```

`#interest` is one purpose: paid `of house` to the lender, it counts out (`us`'s `home-interest`); received by savings, it
counts in (`count-interest`, rewritten `on in when purpose is #interest`). The flow view shows it in both directions, where
today it shows −49.73 under *spending* (golden `household-flow`).

### 2.3 `07-landlord`'s mortgage

```text
entity bank : bank
  checking     : deposit
  rental-bank  : deposit
  deposit-bank : deposit

entity lender : lender
  home-loan : mortgage
    loan 279_000 USD on 2024-12-18 at 6.75% over 30y for house
    monthly on 1 from rental-bank
    from 2025-02-01
```

- `home-loan` is a place of kind `mortgage`. Today it is a tab of the root `debt` that the contract's name stands for. The
  kept payments, the month-end values (`2025-02-28 home-loan = 278_759.79 USD`) and `register home-loan` read it by its name.
- The contract keeps `home-loan`'s name and its party is the agent, `lender`.
- `bills : payable at summit-roofing` and `deposits : payable` go. The roof bill is already `me owes summit-roofing`, a tab.
  Their 16 assertions go with them (three are among the golden's five errors today), and the diff lists them.

### 2.4 `08-expat`: two countries

The porter can only nest `08`'s accounts under `me`, because none of them says `at`:

```text
entity me : person
  …
  us-checking : deposit
  girokonto   : deposit
    holds EUR
  …
```

With its institutions named (an edit by hand), the two countries read as the two sets of agents they are:

```text
entity me : person
  born   1987-09-14
  filing single
  lives  us/ca/san-francisco until 2025-06-30
  lives  us/abroad from 2025-07-01
  lives  de from 2025-07-01

entity chase : bank                       // the US
  us-checking : deposit
  us-savings  : deposit
  visa        : credit-card
entity fidelity : broker
  us-401k : 401k
    employer acme
entity mohela : lender
  student-loan : loan
    rate     4.53%
    maturity 2032-06-30

entity sparkasse : bank                   // Germany
  girokonto : deposit
    holds EUR
  tagesgeld : deposit
    holds EUR
entity hausverwaltung : landlord
  /// The rent deposit is the landlord's to give back: a position held by the landlord.
  kaution : deposit
    holds EUR
    liquidity 700d
entity wise : bank
  gbp : deposit
    holds GBP
```

July:

```text
2025-07

01 girokonto   -> hausverwaltung 1_450 EUR
03 us-savings  -> tagesgeld 20_334.11 EUR @ 1.180283 USD
03 girokonto   -> 47.09 EUR #groceries                // was `-> ?`
05 us-checking -> 420 USD
  -> student-loan 309.44 USD                          // principal
  -> mohela       110.56 USD #interest                // interest: one purpose, out
31 us-savings  <- 63.61 USD #interest                 // was `<- ? … #interest-income`
31 de-clearing <- employer-gmbh 5_400.00 EUR
…
```

`kaution` under `hausverwaltung` is the clearest case of what L2 says: a rent deposit is a position the landlord holds for
you. Today it is an account with nobody beside it.

`de-clearing` stays: it is lane D's workaround for gap 5 (§5), and L2/L3 do not remove it.

### 2.5 A sole proprietor

```text
base USD
use std
use us

entity me : person
  filing single
  lives  us

/// The studio is Alex's, so its profit is on Alex's return.
entity studio : org
  owner me

entity first-bank : bank
  operating : deposit
    owner studio
  personal  : deposit
entity amex : card-issuer
  business-card : credit-card
    owner studio

entity halcyon : client
  of studio
entity figma : software

purpose design
  law receipts
    on in
    count amount as gross-receipts
purpose meals
  law half-deductible
    on out
    count amount * 50% as business-expenses

2026-01
05 business-card -> figma 15 USD                           // #software, by figma's kind
27 halcyon owes studio due 30d ^inv-12
  3_000 USD #design "brand refresh"
    800 USD #design "icon set"
28 business-card -> 9.80 USD #meals "coffee with halcyon"  // no party

2026-02
20 operating -> business-card 24.80 USD                    // the statement, paid
26 operating <- halcyon 3_800 USD ^inv-12                  // settles the invoice, a claim in the studio's tab with halcyon
28 operating -> personal 2_000 USD #owner-draw
```

The invoice is today's: a claim is a tab. The difference is in the line above it. `operating -> halcyon 500 USD due 30d`
(an advance) would now be a claim too (§1.3), where today it is spending.

### 2.6 The ten places where this design is weakest

1. **No counterparty means no party, not "the agent".** §6.4's "from chase" is not taken (§4.1). A user who wants interest
   attributed to chase writes `savings <- chase 170.70 USD #interest`, as today.
2. **There is no refund.** Money in for `#groceries` is money in, whoever sent it. A budget still nets it: what went out,
   less what came back. The flow view shows groceries in both directions instead of one net row. Today a root decides
   (§4.5).
3. **A share's base is the size of its purpose's net**, |out − in|. In a window where refunds exceed spending, a share of a
   spending purpose reads the excess as a positive base. No book or probe has such a window.
4. **Every party is declared.** A quick journal can no longer name a payee nobody declared. It writes the payee as a
   description, or declares it in one line. The 25-odd implied parties of the corpus, and the institutions `at` names
   that nothing declares, get declarations from the porter.
5. **A position has one written place.** It is declared where its agent is, so an institution's positions are in one file,
   and a system's entity (`irs`) can hold no declared position. Only claims (tabs) can be held there.
6. **A bare one-word position is not accepted** (`brokerage` under `fidelity` must be `brokerage : brokerage`, or an
   address).
7. **When two agents hold positions of one name, both are renamed in every report**, to `chase/checking` and
   `ally/checking`. The path is the shortest unique address, decided at declaration; a single `checking` stays `checking`.
8. **A loan stated on a position that already has a history needs the restating rule** (§1.4). It is the one new piece of
   lowering logic in L2b.
9. **The memo of address references goes with the walk that listed them.** K3b measured the memo as taking a 100k book
   written in addresses from +12.6% to +1.75%. No bench book is written that way, and none of today's books asks the index,
   but a book in K3b's idiom will be slower until it is measured (§6).
10. **`bank` changes sort.** It becomes an entity kind, and its place-kind overdraft law moves to `deposit`. `05-family`'s
    eleven `: deposit` accounts gain it; the proof says whether any of them goes below zero.

## 3. The types

- **Entities: unchanged.** `Tree<Entity>` in pre-order, the scoped index, and an owner's `Holding` place or a party's
  `Outside` place. Every entity is now declared (`Home::Builtin` is left to `me`, `?`, `opening`, `market`), so
  `Parties.implied`, the implied drafts and their locations go.
- **A position is a `Place` with `Role::Account { institution: Some(agent) }`.** It has the same arena, id, `Names<Place>`
  entries and `class`/`kind`/`owner`/`shares` as an account has today. The institution is now always written (it is the
  container), so K3b's custodian word is in every position's address, as K3b §11.5 foresaw. A rename to `Role::Position
  { agent }` is a name change in about twelve readers (engine, report, sync), worth doing once C4 and C6 have merged, and not
  counted here.
- **A tab stays a tab**: `Role::Tab(party)`, made on demand, keyed `(party, owner, kind)`.
  - It is the position the owner has with a party that nothing declares. Merging `Role::Tab` and `Role::Account` was weighed
    and is not proposed: the readers that ask which end is the party (`infer.rs:89`, `record.rs:1301`, `said.rs:127`,
    `eval.rs:1001`) treat a tab as its party and a declared position as the owner's own, and would have to ask a field
    where they now match a variant.
  - Loans no longer ask for tabs. So the kind in the key only tells a bill (`debt-claim`) from a receivable (`claim`), and
    the `debt` tab of K3f §0.3 goes.
- **An address is K3b's `Addresses`**: an entity's posting list, the name's list, and each position's address. It loses
  `used` (the gate) and `once` (the memo). Every book's positions are in the index, and `agent/name` resolves in every
  book.
- **A debt position holds its contract by name.** The contract is the same `Contract`, lowered from the position's promise
  lines. `party` is the agent, `owner` is the position's owner, and `loan.debt` is the position's place, not a tab. One
  `Sym` names both, so `lookup.contracts` finds the promise (an occurrence `01 mortgage`) and `Names<Place>` finds the debt
  (`checking -> mortgage 10_000 USD`, a prepayment). `special_end`'s loan arm becomes "a contract that is a position stands
  for its position", and `contract_endpoints` goes.
- **Direction.**
  - Each flow's direction is the one it crosses the owners' boundary in. `purpose_direction` is unchanged, and so is the
    totals store, which keeps `(in, out)` per owner and purpose.
  - `PurposeRoot` becomes `{ Ordinary, Capital, Transfer }`. The roots are `capital` and `transfer`, and a purpose with no
    parent is ordinary: today it hangs under `transfer` (`purposes.rs:40`, `ORPHAN`).
  - Each consumer says what net it reads: a budget reads what went out less what came back; a share's base reads the size of
    its purpose's net; the flow view reads each direction.
- **The optional counterparty resolves to `?`'s place** (`Role::Outside(None)`). A flow with no object is exactly today's
  `-> ? AMOUNT`, with no new place, role or entity.

**Replaced:**

- the `at` word's resolution and `unknown-institution` (`holdings.rs:59-70`);
- `World.contract_endpoints` and `loan_endpoint`;
- `World.references`, `Mentions`, `implied_parties`, `Addressed`, `spellings`, `of_two_words`, `strict_path_suffixes`,
  `add_path_spellings`;
- `Addresses.used`/`once`/`settle`/`settled`/`is_used`/`is_always_open`, `settle_addresses`;
- `PurposeRoot::{Income, Spending}` and `PurposeRoots.{income, spending}`;
- std's `receivable`, `payable` and `interest-income`.

## 4. Each change, by file and function

Counts are code lines of the function or item as it is at `d52032c`. "Goes" is a line that no longer exists. "Adds" is
written out in the style around it, then counted.

### 4.1 L3a: the counterparty is optional (+11)

**The rule.** A flow line with one end, an amount, no price and no legs, whose tail has a purpose or a description, is a
flow to (or, with `<-`, from) no party in particular: `?`. With neither it stays `exchange-no-price`, whose fix now reads
"say what it is for (`#groceries`), or name the party".

**Why not the agent.** Under §6.2 every position has an agent. "Defaults to the agent" would make every party-less card
purchase a payment to the card's issuer, and would quietly give those flows a party's meaning: chase's purpose for
inference (`said_at`), chase's open bills settled by them (`settle.rs:151 paid_to_party`), chase's rows in `flow` by party.
A refinement was weighed and not proposed: the agent only when its kind says the purpose (`kind bank pays interest`). That
adds a rule a reader has to learn, about 15 lines, for a word the user can write.

| where | goes | adds |
|---|---:|---:|
| `syntax/flow.rs:499 settle_exchange`: accept one end and an amount when the tail says why | | +6 |
| `model/lower/record.rs:300 lower_flows`: an absent end is `?`'s place, instead of `flow-shape` | | +5 |

The Book of a book that writes `-> ?` today is the same Book. The porter drops `?` from every flow line that has a purpose
or a description (in `08-expat` alone, about 90).

### 4.2 Every party is declared (−328)

| where | goes | adds |
|---|---:|---:|
| `model/declare/mentions.rs`, the module: `Mentions`, `Role`, `slot_names`, the walk (non-test, with its header and uses) | −160 | |
| `declare/parties.rs`: `implied_parties` 42, `Addressed` and its `impl` 31, `spellings` 7, `of_two_words` 5; `find` and `declare` without the implied drafts and their locations | −102 | |
| `declare/parties.rs::find`: a contract named for its party (`contract netflix`) declares that party, as a declaration and not a mention | | +8 |
| `declare.rs`: `add_path_spellings` 8, `strict_path_suffixes` 17, the `references` field and its plumbing | −29 | |
| `reference.rs`: `settle_addresses` 20, the three gates on `is_used`, the memo's branch in `address_end` | −32 | |
| `addresses.rs`: `used`, `once`, `is_used`, `settle`, `settled`, `is_always_open`, the gate's line in `of` | −16 | |
| `lib.rs`: the call to `settle_addresses` | −1 | |
| `resolve.rs`: an unknown party is today's `unknown-name` (with its suggestion), plus the fix "declare it, or write it as a description" | | +4 |
| **net** | **−340** | **+12** |

What it changes:

- **The Book.** Implied entities become declared ones. Their kinds stay the root `entity`; their `home` and `loc` change.
- **Unknown names.** A name nothing declares is an error, where it was a party. In a book that writes an address, a
  mention of two words that no table knows is `unknown-address` (K3b's message) instead of `unknown-name`.
- **Addresses everywhere.** `agent/name` and K3b's references resolve in every book; nothing in the corpus changes because
  of it (K3b §10's census still holds).

### 4.3 L2a: positions nested under their agent (+52)

| where | goes | adds |
|---|---:|---:|
| `syntax/decl.rs::decl_line` for an `entity`: a line `PATH : KIND`, or a path of two words, is a position. Its body takes property, `law`, `also`, `known-as` and `has` lines and, through `contract_line`, the promise lines. It emits `Decl { what: Account, at: Some(container) }`, and a `Contract { party: container }` when it has promise lines. A position under an entity with several names is an error | | +45 |
| `syntax`: the `account` keyword and `at` are hints (`removed-form`); `decl.rs:42` (`at`) goes; `CHART_ROOTS` moves into the hint | −5 | +8 |
| `model/declare/holdings.rs::declare_accounts`: the institution is the container, already an entity; the `at` word's resolution and `unknown-institution` go | −12 | +2 |
| `holdings.rs`: two agents' positions of one written path get the agent before the path (the shortest unique address); one agent's two is still `duplicate-account` | | +10 |
| the owner of a position under one of the owners is that owner | | +4 |
| **net** | **−17** | **+69** |

This step adds more than it deletes. The reason is the language itself: an institution is said once, by holding the
position, instead of in every account's `at` and again in its `entity` line. That is what "easy and declarative to set up
associations" asks for. The model was already shaped for it (K3b), so the cost is the parser's.

### 4.4 L2b: a debt is a promise, and a claim is a position with any party (+4)

| where | goes | adds |
|---|---:|---:|
| `lower/contracts.rs`: `loan_endpoint` 9 and its loop in `contracts` 4 | −13 | |
| `declare.rs`: the `contract_endpoints` field and its initialization; `resolve.rs:250` its arm in `special_end` | −8 | |
| `lower/contracts.rs::contract_loan`: the debt is the position's place, not `world.tab(…, debt)` | −1 | +3 |
| `special_end`: a contract that is a position stands for its place (today's loan arm, for every position-contract) | | +2 |
| `lower/contracts.rs`: a loan whose position has a balance on its `on` day restates it. It lends nothing, and `loan-balance` says it when the principal differs (§1.4) | | +15 |
| `lower/tail.rs` or `record.rs`: a flow to a party with `due`, or `for` a party it leaves the owners for, goes into the tab with that party (§1.3) | | +12 |
| `engine/settle.rs:162 paid_into_debt`: no declared claim place is left to pay into (after C4) | −6 | |
| std.ax: `receivable`, `payable` (not counted: `.ax`) | | |
| **net** | **−28** (of which engine −6) | **+32** |

What it changes:

- **The Book.** Every loan's debt is a declared place of the loan's kind (`mortgage`, `loan`), where it was a tab of the
  root `debt`. The position therefore gains its kind's laws: `kind loan`'s `payoff` reads `self.maturity`, which a promise
  does not fill (§1.11). The proof checks that it says nothing.
- **`05-family`'s two records of its mortgage become one**, which changes its `balance` (the tab it never showed is gone)
  and its `check` (one record of the loan's occurrences).
- **The new claims.** Every `due` flow to a party now makes a claim. No example writes one (they use `owes`); `diff/cases`
  and the mistakes may.

**Is the debt merge a third lane?** No. Its model half is the lines above, after step 3. Its engine half (`loan_balance.rs`'s
`Role::Tab` test, `settle.rs`) waits for C4 to merge and is a few lines. Lanes D, K5d and K5e built the loan's machinery on
`Loan.debt` as a place, and nothing in it asks whether that place is a tab, except `loan_balance.rs:28`.

### 4.5 L3b: purposes without a direction root (−24)

**The choice §1.1 forces.** Three designs were weighed:

| | rule | refunds | lines | corpus changes |
|---|---|---|---:|---|
| (a) **direction is the flow's; no refund** | income and spending are the two directions money crosses the boundary in. A budget reads out − in; a share reads the size of its base's net; the flow view shows each direction | net in budgets, as today; shown gross in `flow` | −24 | the flow view's layout (every book); `interest` received out of *spending*; nothing else in the corpus (§1.12) |
| (b) direction, and the party decides | (a), and money in is a refund when it comes from a party paid that purpose before (or `against`) | a refund needs a party who was paid; a fold-time set of (owner, party, purpose) | +5 | `amazon`/`target` stay refunds; `state-farm`'s repair becomes income |
| (c) optional roots | `: income` and `: spending` stay, and a purpose without one counts in both directions | as today | +15 | `interest` only |

**This map proposes (a).** It is the only one of the three that deletes the concept rather than moving it, and it is what
"income and spending are directions" means. Under it a refund is money that came back for a purpose, and only a budget
nets it, because a budget asks what a purpose cost. (b) is what the proposal's sentence describes, at about thirty more
lines than (a). (c) keeps the two roots the proposal wants gone.

| where | goes | adds |
|---|---:|---:|
| `model/purposes.rs`: `INCOME`, `SPENDING`, two of `ROOTS`/`KINDS`; `book.rs` the root table's two rows and `PurposeRoot`'s two variants; `PurposeRoots.{income, spending}` | −9 | |
| `model`: a purpose written `: income` or `: spending` says the hint "income and spending are directions now: drop the parent" | | +6 |
| `engine/eval.rs:1627 purpose_net`, `fire.rs:309` (after C4): out − in, and the size of the base for a share | −14 | +5 |
| `engine/statement.rs:350`: an exchange's cost is an ordinary purpose (was `Spending`) | | |
| `report/flow.rs` (after C6): the four root sections become the two directions and capital and transfer; `why/purpose.rs` `root_name`, `root_fact` | −16 | +4 |
| std and us (`.ax`, not counted): `interest-income` merged into `interest`; `count-interest` `on in`; `us/ira.ax:40` `when from is asset` (a contribution comes from the owners' money); the 30-odd std/us purposes lose `: spending`/`: income` | | |
| **net** | **−39** | **+15** |

### 4.6 Two decisions beyond the steps

**`fmt --upgrade` (L1).**

| | lines |
|---|---:|
| `syntax/upgrade.rs` (316 code lines), `legacy.rs` (78), and `cli/src/fmt.rs`'s upgrade path; the v4-syntax hint stays as a hint | **−~450** if retired |
| teach it to nest accounts under their `at` party, and to declare implied parties (it already needs the book, L1 §5) | **+~80** if kept |

The porter (§6) ports every book in the corpus either way. A v4 book outside it would be upgraded by the last binary that
reads v4, then ported. This map recommends retiring the upgrade, and says so here because it is a feature: §13's "reading
v3 and v4 books".

**The paystub through its owner (lane D's gap 5).**

- `me <- acme 8_000 USD #wages` with legs `-> irs 880 USD #federal-tax` and `-> checking ...` lowers to flows from acme to
  each leg (`Course::Through`). Wages therefore count only the legs that keep `#wages`, which is why 08 and 10 write the gross
  and the withholding as separate outflows.
- Posting the gross into the owner's holding and the legs out of it makes the written split count the gross. It costs about
  +10 lines in `lower/record.rs`, and changes the register of `me` and the tax of every book that writes a split through
  its owner.
- It is the junction's (L1's) promise more than L2/L3's. It is listed so that it is decided, not lost.

## 5. Lane D's six gaps

| gap | after L2/L3 |
|---|---|
| (1) `-> party due` makes no claim | **falls out** (§4.4): a `due` or `for PARTY` flow to a party goes into the position with that party |
| (2) a value cannot be asserted of a tab | left: a tab has no name of its own. `jo` names the party's outside place, and "what jo owes me" and "what I owe jo" are two tabs |
| (3) a write-off is whole only | left: not a question of the language's structure |
| (4) `#loan` is not a built-in purpose | left: one line of std (`purpose loan : transfer`) if wanted; L3b does not need it |
| (5) a paystub through its owner counts as wages only what lands in an account | **not without the decision of §4.6** |
| (6) a contract paid `from` a fund projects without capping | left: Decision 9's |

## 6. The proof

1. **The porter**, `docs/v5/measure/l2l3/port.py`: a one-off over source text, which asks the old binary's Book dump what
   the text cannot say (which parties were implied, each account's owner). In order:
   - nest each `account … at X` under `entity X`, declaring `X` if it was implied or is declared nowhere (the
     `unknown-institution` books of §1.6), and each account with no `at` under its owner;
   - merge a loan contract with the account of its debt where both exist (`05-family`), or make the contract a position
     under its party;
   - declare the implied parties;
   - drop `?` where a tail says why, and the `: income`/`: spending` parents;
   - rename `#interest-income`;
   - rewrite `on receivable` as `on claim`;
   - drop the `receivable`/`payable` places of `04` and `07` and the assertions on them.

   It ports the examples, `explore-v5`, `tests/v4-syntax`, the mistakes, `diff/cases*`, the Rust test strings (§1.10) and the
   code blocks of LANGUAGE.md and DESIGN.md. It writes a **location map** (old file:line → new file:line) for each book,
   because nesting moves declarations and a diagnostic's location must still be compared exactly. Hand edits are listed:
   `05-family`'s seven invented parties, `08-expat`'s institutions (both shown in §2), and whatever the porter refuses.
2. **The outputs.**
   - For every book, the old binary on the old book against the new binary on the ported book: `check`, `balance`, `flow`,
     `tax`, `claims` and `forecast`, at the dates `diff/run.sh` uses, with locations mapped.
   - A classifier accepts only the differences the steps list:
     - the flow view's layout (step 5);
     - claims from `due` flows (step 4);
     - `05`'s merged mortgage and `07`'s dropped places and assertions (step 4);
     - the hand edits;
     - the hint and refusal changes in the mistakes.
   - Any other difference fails the step.
   - `examples/verify/verify*.py` stay green (verify11's `straight-line` failure is the head's, lane D).
3. **The Book.** The dump of the ported book against the old one, by step, with locations normalized. It must be identical
   except as listed:
   - entities' homes (step 2);
   - loans' debt places (step 4);
   - purposes' roots (step 5).
4. **The mistakes**, regraded once per language change, in one commit each, with every changed message and its reason. There
   is a mistake book per new diagnostic:
   - the `removed-form` hints (`account`, `at`, `: income`);
   - an undeclared party with its fix;
   - a position under a several-name entity;
   - a loan whose stated principal differs from its position's balance.
5. **Speed.** `bench/` at 100k and 1m after step 2, against the baseline, and K3b's `addresses.py bench` (the address-spelled
   book), for the memo that goes (§2.6 item 9). If the address book is slower than K3b's +1.75%, the memo comes back, fed
   by a walk of references alone (about 80 lines), and step 2 nets −250.

## 7. The order

| step | depends on | lanes it waits for | lines |
|---|---|---|---:|
| 1. L3a, the counterparty optional | nothing | none | +11 |
| 2. every party declared | 1 (so that the porter can drop invented parties first) | none | −328 |
| 3. L2a, positions nested | 2 (an agent is a declared entity) | none | +52 |
| 4. L2b, debt as promise, claims as positions | 3 | C4 for its engine half | +4 |
| 5. L3b, purposes by direction | 1 | C4 (engine) and C6 (report) | −24 |
| the `fmt --upgrade` decision | 3 | none | −450 or +80 |
| the paystub decision | none | none | +10 |

Steps 1 to 3 are model and syntax only and can start now. Step 4's model half follows step 3. Its engine half and step 5
wait for C4 and C6 to merge. Each step is a green series: the porter's run for that step, its proof, the regrade, and the
examples, LANGUAGE.md, the cheat sheet and DESIGN.md in the new spelling.

**Estimate for the whole: −285 planned.**

- Step 2 is whole items, read: about 100%.
- Steps 1, 3 and 4 are additions: their error is about a fifth, in both directions.
- Step 5 is a unification and takes the observed 28%, which makes it about −7.

Central estimate: **about −270**, or **about −720 with `fmt --upgrade` retired**. `model` goes down in every case. `syntax`
goes down only if the upgrade is retired: steps 1 and 3 add about +54 there.

By crate, planned, and with lane U's realized ratio of 28% applied to the whole plan (the most pessimistic reading: it treats
the whole-module deletion of step 2 like a unification, which the record says it is not):

| crate | deleted | added | **net planned** | net at 28% | central |
|---|---:|---:|---:|---:|---:|
| model | −383 (−340 step 2, −12 step 3, −22 step 4, −9 step 5) | +71 | **−312** | −87 | −310 |
| syntax | −5 | +59 | **+54** | +54 (additions are not discounted) | +54 |
| engine (after C4) | −20 | +5 | **−15** | −4 | −9 |
| report (after C6) | −16 | +4 | **−12** | −3 | −3 |
| **all** | **−424** | **+139** | **−285** | **−40** | **−268** |
| with `fmt --upgrade` retired | −~450 more | | **−735** | −166 | −718 |

The honest reading: L2/L3 are worth their lines for what the books lose (section 0) and for one deletion that is certain,
step 2. They are not a lever on the 27,000 question: at most about 0.5% of the tree, about 1.3% with the upgrade retired.

## 8. Risks to features

Nothing the user has today is lost, except these, each a consequence named above:

- **An undeclared party.** Replaced by a declaration or a description, and a typo is now an error with a suggestion
  (step 2).
- **"From chase" when a flow names no party.** It is no party; write `chase` (§4.1).
- **A refund as negative spending in the flow view.** It is money that came in for the purpose. Budgets still net it
  (§4.5).
- **A share of "all income" (`10% of #income`).** It needs a purpose that groups the income purposes. No book writes one.
- **`receivable`/`payable` places.** Their uses are a code scope and assertions that fail today (§1.7).
- **Declaring a position anywhere but under its agent.** One place per position (§2.6, item 5).
- **v4 books.** Only if `fmt --upgrade` is retired (§4.6): they go through the last binary that reads them.

What the new language says that the old could not: a deposit, a card, a loan and a claim are the same thing, a position with
someone; a debt is written once; a rent deposit is the landlord's to give back; `fidelity/brokerage` and `jordan/401k`
resolve in every book.

## 9. How this map was checked

- **On the baseline binary:**
  - `checking -> jo 600 USD due 04-01` makes no claim (§1.3);
  - `card -> 94.21 USD #dining` is `exchange-no-price` with the `?` fix (§4.1);
  - `me <- acme 8_000 USD #wages` with an `-> irs` leg counts 8,000 of wages when the leg has no purpose of its own
    (§4.6), which is lane D's gap only when the legs carry theirs.
- **Read:**
  - the declare passes (`declare.rs`, `declare/{parties,mentions,holdings,places}.rs`);
  - `spelled.rs`, `addresses.rs`, `reference.rs`, `resolve.rs`'s `end_on`;
  - `lower/contracts.rs`'s loan, `lower/loan_opening.rs`, `lower/infer.rs`;
  - every reader of `PurposeRoot` (`model`, `engine`, `report`) and of `Role::Account`/`Role::Tab`;
  - `engine/settle.rs`;
  - `syntax/{decl,contract,flow}.rs`;
  - std.ax, us.ax, us/ira.ax;
  - K3b, K3f, K5d, K5e and L1's maps, and STATUS's lane D section.
- **Counted:**
  - every line count of §4, function by function, with `quality.py`'s rule;
  - a census of `examples/` and `explore-v5/` by text: accounts, `at`, entities, purpose-only entities, flows, legs,
    purposes by root, and names written as ends that nothing declares (the implied parties of §1.6; the build takes them
    from the old Book instead);
  - account declarations in Rust test sources by `grep` (571).

## 10. Open decisions for the user

Each is a question with this map's recommendation. Nothing is built until they are answered.

1. **Every party declared?** (§4.2) Undeclared payees stop being parties: a description or a one-line declaration replaces
   them, and a mistyped party becomes an error. *Recommended: yes.* It is the only large deletion (−328), and it makes
   addresses work in every book.
2. **What does a flow with no counterparty go to?** (§4.1) *Recommended: no party (`?`).* The alternative is §6.4's "the
   position's agent", which makes card purchases payments to the card's issuer, or the agent only where its kind says the
   purpose (+15 lines, one more rule).
3. **Income and spending: directions or roots?** (§4.5) (a) directions, no refund concept, budgets net; (b) directions, and
   a refund is money back from whom it was paid to; (c) optional roots. *Recommended: (a).* (b) is what §6.3 says in words.
4. **The flow view after (a):** two directions, each purpose gross in each, or one signed net row per purpose? It is
   `report`'s, after C6. *Recommended: two directions, as the store keeps them.*
5. **`fmt --upgrade`: keep and teach it nesting (+~80), or retire it with the v4 hint (−~450)?** (§4.6) *Recommended:
   retire*, porting the corpus with the one-off porter; a v4 book outside the corpus goes through the last binary that
   reads v4.
6. **The paystub through its owner** (lane D's gap 5, §4.6): post the gross into the owner and the legs out of it (+~10)?
   *Recommended: yes, but as the junction's change (L1's promise), in its own commit with its own regrade.*
7. **`bank` as an entity kind** (§1.8): the place kind's overdraft law moves to `deposit`, which every deposit account then
   has. *Recommended: yes*, with std gaining `broker` and `card-issuer` entity kinds.
8. **A loan stated on a position that already has a history** (§1.4): restate, and say `loan-balance` when the stated
   principal differs. *Recommended: yes*, the only new lowering rule of L2b.
9. **Same-named positions under two agents**: name both `agent/name` in every report (the shortest unique address), or
   keep the rule that a position's name is unique in the book? *Recommended: the shortest unique address.*
10. **`kind loan`'s `rate` and `maturity` slots beside a loan promise's terms** (§1.11): keep both, or fill the slots from
    the terms when there are terms? *Recommended: keep both for now* and let the `payoff` law read the promise's end when
    it has one (a few lines, not counted above).
11. **Lane D's gaps 2, 3, 4 and 6** stay open (§5); none is decided by this design.
