# The Axiom language

This is the normative reference. [DESIGN.md](DESIGN.md) says why it is shaped
this way. This is version 4: the language after the move from a chart of accounts
to agents, resources, events and promises. `examples/v4-sketch/` is its first
worked example, and every example in `examples/` is an acceptance test.

## 0. The model in one page

Axiom records economic **events** (flows) that move **resources** (money and
things) between **agents** (owners and parties), and the **promises** (contracts)
that say which events are to come.

- **Owners** are you, and anyone whose money this book keeps: a spouse, a
  household, a business you own. `me` always exists.
- **Parties** are everyone else: employers, shops, friends, tax authorities,
  lenders, funds, the market. What dealing with a party means comes from the
  party: paying a grocer is groceries, money from an employer is wages.
- **Accounts** are positions with institutions: a deposit, a card, a brokerage, a
  401(k). An institution holds the money; an owner owns it. Money in no account is
  with its owner (cash in hand).
- **Assets** are identified things: a condo, a car, a laptop. Each has a history of
  parts (its purchase, each improvement), and its basis is derived from that
  history, never written. An asset never moves like money: flows name it as what
  they are about (`#improvement of condo`).
- **Commodities** are what money and holdings are counted in: `USD`, `EUR`, `VTI`.
  Money is fungible and held as parcels that remember their cost, acquisition day,
  and whom they are held for.
- **Purposes** say what a flow is for, as a tree (`groceries` is `food` is
  `spending`). A flow's purpose is written (`#groceries`) or inferred from its
  contract, its party, its money or its accounts.
- **Contracts** state recurring and scheduled promises once: pay, rent, a loan, a
  lease, a subscription, a standing order. The journal records each time one is
  kept; `check` notices when one is not; the forecast runs them forward.
- **Claims** are what a party owes an owner or an owner owes a party: a loan to a
  friend, an invoice, a lease deposit, a late rent. They are the time between an
  event and its counterpart, and they live on the party.
- **Laws** attach to kinds, purposes, individual things, and systems (`us`,
  `us/ca`), and say what must hold, what is owed, and what is counted.
- **Terms change.** Every declaration holds from the book's first day, and the
  journal may change it from a day on, for a while or for good: a new rent, a free
  month, a promotion and its extension, a looser budget for December, a move to
  another state.
- **Derived events** are what the book implies without anyone writing it: a loan
  payment's interest, a bill's business share, the sales tax inside a price, the
  cost of an exchange rate, depreciation, a wash sale. They are computed, explained
  by `why`, and shown by an editor as hints.
- **Sync** brings in statements, invoices, prices and parameters, recognizes who
  and what each record is, reconciles it with what is already written, and adds
  only what is new (§13).

The journal has two kinds of line. A **flow** (`->`) moves value. A **statement**
says one thing about one thing on a day: a balance, a contract kept or changed, a
claim, a price, a property. There are no income, expense or equity accounts.

## 1. Lexical structure

Source is UTF-8, read line by line.

- **Items** start in column 0. Indented lines (spaces only; a tab in indentation is
  an error) belong to the nearest less-indented line above them.
- **Comments**: `//` to end of line, at line start or after whitespace. **Doc
  comments**: consecutive `///` lines document the item, leg, law or contract that
  follows. A law's doc has two parts: the first paragraph says what the law is, a
  paragraph starting `To fix:` says what to do when it fails.
- **Headings**: a line holding only a year (`2026`) or a month (`2026-02`) gives
  the dates below it their year, or year and month (§10).
- Blank lines mean nothing.

| token       | shape                                                   | examples |
|-------------|---------------------------------------------------------|----------|
| date        | `YYYY-MM-DD`; or `MM-DD` or `DD` where the context gives the rest (§10) | `2026-01-15`, `01-15`, `15` |
| month       | `YYYY-MM`                                               | `2026-03` |
| number      | digits, `_` separators, optional `.frac`                | `84.20`, `24_500` |
| percent     | number then `%`                                         | `12%`, `5.875%` |
| span        | `(digits [ymwd])+`; years may have a fraction           | `30d`, `59y6m`, `27.5y` |
| name        | `[a-z0-9][a-z0-9_-]*`, `/`-separated, may contain `*`   | `checking`, `trader-joes`, `joint/checking`, `401k` |
| commodity   | `[A-Z][A-Z0-9_.]*`                                      | `USD`, `BRK.B` |
| purpose     | `#` then a name                                         | `#groceries`, `#repair` |
| code        | `^` then `[a-z0-9][a-z0-9_:./-]*`, may contain `*`      | `^inv-2026-01`, `^check-1041` |
| string      | `"…"` with `\" \\ \n \t`                                | `"food for the routine"` |
| punct       | `-> .. ... = == != < <= > >= + - * / @ ( ) [ ] , : ! ? . \|` | |

A token starting with a digit is a date, month, number, percent or span if it
matches that shape exactly, and a name otherwise (`401k`). At the start of an item,
a lone number of one or two digits is a day (§10). Numbers carry no sign; `-` is an
operator, except before an amount in an assertion or an opening line, and at the
start of a line item (§2). Case separates names (lowercase) from commodities
(uppercase).

`:` means only "is a kind of", in declarations: `entity lumen : employer`,
`purpose groceries : food`. It never appears on a flow. `/` appears only inside
names and as division in expressions.

Keywords are recognized by position and not reserved. `empty` is the zero of every
commodity; a bare `0` where an amount belongs is an error whose fix is `empty`.

## 2. Flows

```text
DATE FLOW
FLOW   := SOURCE -> TARGET [@ PRICE] TAIL (INDENT (LEG | ITEM))*
SOURCE := [END [SELECTOR]] [AMOUNT | all [UNIT]]
TARGET := [END] [AMOUNT]
END    := ACCOUNT | OWNER | PARTY | UNIT | CONTRACT | ?
LEG    := END [SELECTOR] LEGAMOUNT [@ PRICE] TAIL
ITEM   := [+ | -] ITEMAMOUNT TAIL
TAIL   := [via PARTY] [PURPOSE] [STRING] CODE* [for WHOM|PERIOD] [due WHEN] [basis AMOUNT] [! [STRING]]
PURPOSE:= #NAME [of THING]
AMOUNT := NUMBER UNIT | (NUMBER UNIT) | ? UNIT | empty
LEGAMOUNT  := AMOUNT | NUMBER% | ... | = NUMBER UNIT | all [UNIT]
ITEMAMOUNT := AMOUNT | NUMBER% [of AMOUNT]
```

The tail's clauses may come in any order.

**Ends.** Each end of a flow is one of:

- an **account**: money leaves or joins a position with an institution;
- an **owner**: money or a thing held directly (`checking -> me 100 USD` is cash in
  hand; `me -> taqueria 18.50 USD`);
- a **party**: money leaves the book's owners to it, or comes to them from it;
- a **commodity in party position**: its issuer, as in `VTI -> fidelity 198.12 USD`
  (a fund pays);
- a **loan contract**: its debt, as in `checking -> mortgage 1_000 USD`, which pays
  principal alone (§5);
- `?`: an unknown party, for money whose other end nobody knows;
- nothing: the other side is the legs (a one-sided split), or, for an exchange
  written with only a source, the same account (`fidelity 20 VTI -> 5_940 USD`: the
  proceeds stay at fidelity).

An asset is never an end. A flow says which asset it concerns through its
purpose's object (§8): `#purchase of laptop`, `#improvement of condo`, `#sale of
condo`, `#rent of condo`.

Accounts, owners, parties, assets, contracts and purposes are separate namespaces;
a name that could mean two of them is an error at the later declaration, except
that a contract may share its party's name (§5).

```text
06 visa -> trader-joes 84.20 USD                      a payment, #groceries by its party
09 visa -> amazon 62.40 USD #household "hooks"        purpose and description written
05 checking -> me 100 USD                             cash in hand
20 checking 2_000 USD -> fidelity 7 VTI               an exchange
20 checking -> fidelity 7 VTI @ 285.70 USD            price given: 1,999.90 USD out
05 fidelity[2026-01-20] 1.62 VTI -> 481.14 USD        a sale; the proceeds stay at fidelity
24 visa -> best-buy 1_739.13 USD #purchase of laptop  the laptop arrives (§8)
02 checking -> bay-plumbing 1_480 USD #improvement of condo
12 checking -> jo 600 USD due 04-01                   lent: jo owes it (§7)
24 visa -> delta 420 USD for lumen                    paid for lumen: lumen owes it (§7)
20 checking -> etsy-seller 20 USD via paypal          paid through an intermediary
26 halcyon -> checking 3_800 USD ^inv-2026-01         settles what ^inv-2026-01 marks
16 checking -> savings 400 USD for emergency          held for the `emergency` envelope
01 checking -> insurer 1_140 USD #insurance for 2026  paid now, recognized over 2026
02 checking -> plumber (350 USD) ^check-1041          pending until settled (§3)
14 checking -> ? 40 USD                               to someone unknown
14 checking -> me ? USD                               an amount inferred from assertions
```

**Split flows.** When the header names only one end, the indented legs are the
other side; their total is the header amount, or the sum of the legs, and at most
one leg is `...` (the remainder). A leg may be a percentage of the header amount
(`retirement 6%`). A leg `= AMOUNT` makes its account's balance equal that amount
after the flow. Many-to-many is an error.

```text
15 job                      // a contract occurrence (§5): lumen, 4,600 USD gross
  retirement   276.00 USD   // an account of an owner
  irs          498.00 USD   // a party: see below
  checking     ...
```

**A leg between two parties passes through the transaction's owner.** When the
header's end and a leg's end are both parties, the value is the owner's on the
way: `lumen -> irs 498 USD` in Sam's paystub is Sam's wages, paid on to the IRS.
The owner of a transaction is the owner of its accounts, or `me`.

**Line items.** An indented line that names no end is an item of the flow above it,
between the same two ends, with its own purpose, description and codes:

- `AMOUNT TAIL` is carved out of the header's amount: what the items do not claim
  keeps the header's purpose;
- `+ AMOUNT TAIL` comes on top of it: a fee, a tip, the building's water on a rent;
- `- AMOUNT TAIL` is taken off it: a discount, a credit, or a cost withheld from
  proceeds (a broker's commission, a seller's closing costs);
- an item's amount may be `N%` (of the header's) or `N% of AMOUNT`, which keeps the
  arithmetic visible; an item with no purpose has the header's.

```text
14 visa -> target 120.00 USD #household
  32.10 USD #groceries
  12.00 USD #gifts "for jo's birthday"                // 75.90 stays #household
01 flat
  + 12% of 155.00 USD #utilities "the building's water, January"
15 title-co -> checking 627_000 USD #sale of condo
  - 36_600 USD #selling-costs "commission and escrow fees"   // 590,400 arrives
```

When the header gives no amount, it is the sum of its items.

**Pairing.** The same commodity on both sides is a transfer, and stated amounts
must agree. Different commodities are an exchange at `out / in`. An `@` price
given with both amounts must agree, or the difference is written as a leg or an
item: Axiom never books a difference silently. In an exchange, legs in the
header's commodity take their stated amounts; legs and items whose purpose is a
cost (`#fees`, `#closing-costs`, `#selling-costs`) are costs of the exchange: a
sale's gain is less by them, a purchase's basis more.

**Purposes.** A flow's purpose, first match wins:

1. written on the item, the leg, or the header (for every leg that says none);
2. its contract's (§5);
3. its party's: the party's own `#purpose`, else its kind's
   (`entity corner-store #groceries`; `kind grocer … purpose groceries`);
4. its commodity's kind in party position (`kind fund … pays dividend`);
5. its accounts' kinds (a `401k`'s `takes pre-tax-deferral from wages`).

Two sources that disagree are an error naming both. A flow none of them classifies
is *unclassified*; with a description it is *unclassified, described*. `check
--strict` asks for a purpose on each. `#NAME of THING` gives a purpose its object
where the purpose takes one (`#improvement of condo`); a purpose that requires an
object reports its absence. A flow in the opposite direction to its purpose is a
refund: money from a grocer is groceries, negative.

**Descriptions.** A string in the tail says why in words. It has no meaning to the
book: it is shown by `register`, grouped by `flow` among unclassified flows, and
searched by `why "text"`. A purpose is a description that earned a name: `check`
offers to declare one for a description that recurs.

**Codes.** `^code` marks a flow, leg or item so other lines can refer to it: a
payment carrying the code of an open claim settles that claim (§7); a selector
`[^code]` picks the parcels it marked; a statement on `^code` changes what it
marks (§3); `why ^code` lists everything it marks, with its documents (§10).

**Dates.** A flow has the day money moves and the period it belongs to. `for
PERIOD` (a year, a month, a date, or `DATE..DATE`) recognizes it evenly per day
over that period; `for 2025` on an estimated tax payment made in January makes it
2025's. `DATE..DATE FLOW` is the same with the payment on the first day.
Tallies, window totals, budgets, `flow` and `tax` read the recognition; balances
and relief read the day.

**Other tail clauses.**
- `via PARTY` names an intermediary the money passed through (`checking ->
  etsy-seller 20 USD via paypal`): the party at the end is who it was for, and
  purpose inference reads that party; sync recognizes both on a statement.
- `for OWNER-OR-ENVELOPE` on money arriving holds it for that entity (an envelope,
  a tenant's deposit); `for` the owner itself releases it (§8).
- `for PARTY` on money leaving the owners pays on that party's behalf: the party
  owes it (§7), and it is not the owner's spending.
- `due WHEN` (a date, or a span after the day) makes the flow a claim (§7).
- `basis AMOUNT` gives arriving parcels a total basis other than their cost (a gift
  of shares that keeps the giver's basis).
- `!` waives every law violation this flow raises, priced ones included. The waiver
  is reported; a `!` that waives nothing is a warning.

## 3. Statements

```text
DATE SUBJECT PREDICATE [until DATE] [STRING] CODE*
```

A statement says one thing about one thing on a day. It moves no value itself:
what follows from it (an occurrence's flows, a claim, a gap) is derived from it.

```text
31 checking = 8_828.87 USD                        a balance at the end of the day
31 visa = 2_333.99 USD                            owed, as the statement shows it
31 retirement = 58_420.18 USD via market          the gap is growth from the market
30 me = 45.15 USD !                               the gap is accepted as unexplained
31 mortgage = 310_978.17 USD                      a loan contract's balance (§5)
01 flat                                           a contract kept (§5)
08 phone 47.30 USD                                …for a different amount this time
07-01 flat 3_050 USD monthly                      a contract's terms from this day
03-01 gym 120 USD monthly until 05-31 ^promo      …for a while
05-28 ^promo until 08-31 "extended"               what a code marks, changed
12-01 flat waived "December free, from greystar"  an occurrence released
10 netflix ends                                   a contract, account or asset ends
27 halcyon owes studio 3_800 USD due 30d ^inv-12  a claim (§7)
04-20 ^inv-12 due 05-15                           …given more time
06 ^check-1041 settled                            pending → real on this day
20 ^check-1044 void                               pending → never happened
04 ^deposit-77 returned                           real → reversed on this day
02 VTI 280.14 USD                                 1 VTI = 280.14 USD that day
22 FAST split 2 for 1                             every FAST parcel doubles; basis stays
06-15 me lives us/ny                              a property, from this day
12-01 budget food 1_200 USD monthly until 12-31   a budget, from this day
05-01 car basis 12_000 USD since 2019-03-01       an asset arrives unbought (a gift)
```

**Changes hold from their day.** A statement that sets terms, a property or a
budget holds from its day on. With `until`, what held the day before it resumes the
day after. A later statement overrides an earlier one for the days it covers, and
the declaration is simply the first. Laws, promises and reports read every value as
it stood on the day they are judging; `why NAME` shows a thing's changes in order.
A code on a change names it, so a later statement can extend it, cut it short
(`until`), or release it (`waived`). A shortened `until` or `due` date is the first
such day on or after the statement's.

**Assertions.** An assertion states a balance the way a statement shows it: what is
owed on a card or a loan is positive. A failed assertion is an error that shows
the window since the last passing assertion and the likeliest cause (§11); the gap
is carried, so a later assertion failing by the same amount is not reported again.
`!` accepts the gap as unexplained; `via PARTY` makes it a flow with that party, and
`via market` is a revaluation (§8). An assertion that depends on an unsolved `?` is
not checked, and says so once.

A **split** multiplies every parcel of a commodity by `N / M`, keeping basis and
acquisition day. A settlement event is dated on or after what it settles. `ends`
on an account closes it (flows after are errors); on an asset, it leaves the owners
with nothing received (scrapped, lost); on a contract, nothing more is expected.

**Openings.** A book begins with what its owners hold:

```text
opening 01
  checking    6_062.55 USD
  checking    2_350.00 USD for dana
  fidelity    210 VTI basis 48_300 USD since 2021-06-01
  visa        1_240.18 USD                      owed
  condo       basis 402_000 USD since 2024-02-20
  jo owes me  600 USD due 04-01                 a claim already open
```

Each line creates holdings with an optional total `basis` and acquisition day
(`since`, default the opening's day). Openings are states, not flows: no law sees
them, and they may be older than any param. A loan contract's balance comes from
its terms and needs no opening line.

## 4. Declarations

```text
base USD
use us/ca/san-francisco
relaxed                      // law violations become warnings

entity NAME[, NAME…] [: KIND] [#PURPOSE]   account NAME : KIND [at PARTY]
asset NAME : KIND                         commodity UNIT [: KIND]
purpose NAME [: PARENT]                   kind NAME [: PARENT]
contract NAME [with PARTY]                budget PURPOSE LIMIT monthly|yearly [carries]
  PROPERTY ARG*   (indented lines)
  law NAME        (a nested block)
```

- `base` is required once the book uses more than one currency.
- An **account** is always of a kind whose root is `asset` (what is yours at an
  institution) or `debt` (what you owe on one). `at PARTY` names the institution;
  interest from it (`chase -> checking 3.12 USD`) and its fees are inferred by its
  kind. Names are flat; a `/` groups them for reports.
- An **entity** is an owner or a party. The owners are `me`, entities `member` of
  a household `me` belongs to, entities with an `owner` among the owners (a
  business), and any entity named as the `owner` of an account or asset. Every
  other entity is a party. A party may carry its own purpose (`entity corner-store
  #groceries`), with or without a kind. A party that is never declared can still
  be written: it is untyped, and its flows need a purpose or a description.
- An **asset** is an identified thing, of a kind rooted at `thing`.
- A **purpose** is a node of the purpose tree. The roots are `income`, `spending`
  and `capital`; std ships the rest. A purpose may require an object: `of KIND`.
- A **budget** is a `warn` on the purpose's total for each month or year (§6).
  `LIMIT` is an amount, or a share of another purpose's total in the same window
  (`budget fun 10% of #income monthly`). A budget that `carries` is judged on its
  total since it began against the sum of its limits through this window: what one
  month leaves unspent the next may spend, and what one month overspends the next
  must make up.

Built-in properties:

| on | property | meaning |
|----|----------|---------|
| account | `owner ENTITY` | default `me` |
| | `holds UNIT, … \| any` | commodities it may hold |
| | `select fifo\|lifo\|hifo\|prorata` | relief policy |
| | `opened DATE` | flows before are errors (`ends` closes it) |
| | `liquidity SPAN` | time to turn into cash |
| account, entity | `known-as STRING, …` | how it appears on statements: globs, any case (§13) |
| entity | `lives SYSTEM [from DATE] [until DATE]` | a residence; may overlap |
| | `member ENTITY` | belongs to that household |
| | `owner ENTITY` | a business: owned by that owner |
| | `of OWNER` | a client of that owner: what it pays is that owner's |
| asset | `owner ENTITY` | default `me` |
| | `at ACCOUNT` | held by an institution, not the owner |
| commodity | `precision N`, `name STRING`, `liquidity SPAN`, `grows PERCENT yearly` | |
| kind | `restricted` | money from, or held for, entities of this kind stays tied to them |
| | `deferred` | accounts of this kind realize nothing inside |
| | `basis zero\|cost` | what basis arriving value takes (§8) |
| | `purpose NAME` | on a party kind: what flows with its parties are for |
| | `pays NAME` | on a commodity kind: what its issuer pays is for |
| | `takes NAME from NAME` | on an account kind: what arrives from flows of the second purpose is the first |
| | `select POLICY`, `liquidity SPAN` | defaults |
| | `has NAME TYPE` | declares a property of this kind's things |
| party kind | `sales-tax PERCENT` | the tax inside every price paid to its parties (derived, §9) |
| purpose | `of KIND` | requires an object of that kind |
| contract, party, purpose, asset | `business PERCENT for OWNER` | an allocation of each flow (§9) |

A kind's other property lines are defaults for its things. `TYPE` is one of `date
amount number percent span text name entity place kind unit bool purpose asset`.
An unknown property is an error with a suggestion. Every property may change on a
day (§3): `07-01 flat business 20% for studio`, `06-15 me lives us/ny`.

```text
code GLOB [GLOB…]          // `code inv-*`: codes matching it
  on KIND | NAME …         // may only mark flows touching these

param NAME
  KEY+ VALUE               // a year (latest ≤), a date, or names
  2026 single 0 USD 10% | 12_400 USD 12%

sync NAME                  // §13
  PROPERTY ARG*
```

## 5. Contracts

```text
contract NAME [with PARTY]
  [about] [AMOUNT | buy UNIT for AMOUNT] CADENCE [on DAY] (from | into) HOLDING
  [PURPOSE] [STRING]
  [from DATE] [until DATE]
  [covers PERIOD-SPAN]                         // `covers the year`, `covers 6m`
  [business PERCENT for OWNER]
  [deposit AMOUNT]
  [loan AMOUNT on DATE at PERCENT over SPAN [for ASSET]]
  [escrow AMOUNT into HOLDING]
  [match PERCENT of ACCOUNT up to PERCENT]
  (LEG | ITEM)*                                // a template, as in a split flow
// CADENCE: daily | weekly | monthly | quarterly | yearly | twice monthly | every SPAN
// DAY: 15 | last | 15, last | 04-15 | monday … sunday
```

A contract is a promise of flows with one party: amounts, a schedule, a purpose,
and what else each flow means. A contract without `with` is with the entity of its
own name (`contract netflix`). `about` says the amount varies (a utility bill):
each occurrence states its own, and the forecast uses this one.

- **The journal records it kept.** `DATE NAME` is one occurrence: the contract's
  flow on that day, written in full by the contract as its terms stand that day. An
  amount after the name replaces the contract's this once (`08 phone 47.30 USD`),
  or, for `buy`, is what was bought (`20 vti-monthly 1.620 VTI`). Indented legs
  replace the template's legs of the same end, `...` absorbs the difference, and
  line items add to it, carve it up or take from it (§2).
- **`check` notices it not kept.** An occurrence that is due and not in the journal
  by `today` is a claim for as long as it is missing: a rent the party owes, a bill
  the owner owes. It is reported as late, with the day it was due; the occurrence,
  when written, settles it. Each due day is kept by the nearest occurrence within
  half a cadence.
- **The forecast runs it forward**, occurrence by occurrence, through the laws.
- **What it says applies to every occurrence:** its purpose, `covers` (each
  payment is recognized over that span from its day), `business` (an allocation,
  §9), `deposit` (a claim the party holds, and money held for it, from `from` to
  `until`).
- **A loan** (`loan AMOUNT on DATE at RATE over SPAN`) is a debt of the owner to
  the party. Its schedule gives each payment's interest (`#interest`, `of` the
  asset when `for` names one) and principal, and its balance on any day, which an
  assertion on the contract's name checks. `escrow` adds to each payment a flow
  into that holding. A flow to the contract pays principal alone: the payment stays
  and the loan ends sooner.
- **A match** (`match 50% of retirement up to 6%`) derives, for each occurrence, a
  flow from the party into that account, as a share of what the template puts
  there, up to a share of the gross.

**Terms change** (§3). What a contract says may change on a day, for good or until
another:

```text
07-01 flat 3_050 USD monthly "renewed at 3,050"         a new rent from July
03-01 gym 120 USD monthly until 05-31 ^spring-promo   a promotion
05-28 ^spring-promo until 08-31 "extended"            …extended through August
12-01 flat waived "December free: greystar's gift"    one occurrence released
04-01 gym waived until 06-30 "frozen while abroad"    every occurrence in the span
07-01 flat business 20% for studio                    a property of the contract
2029-03-01 mortgage at 6.25%                          a loan's new rate
2027-06-30 lease until 2028-06-30 "renewed a year"    a new end
```

- A statement with an amount and a cadence restates the terms; what it leaves out
  (the day, the holding, the purpose, the legs) carries over. With an amount and no
  cadence, it is an occurrence that differed this once.
- `waived` releases the occurrence due that day, or next due after it, or with
  `until`, every occurrence due in the span: each is kept at nothing, so nothing is
  late and nothing is owed.
- A loan's new rate refigures the payment over the rest of the term, from the
  balance on that day.
- `check` suggests new terms when three occurrences in a row differ from the
  contract by the same amount.

Contracts replace v3's plans: `every` and named plans are gone.

## 6. Laws

```text
/// What the law is.
///
/// To fix: what to do when it fails.
law NAME
  TRIGGER
  when EXPR
  let NAME = EXPR
  require EXPR [else EFFECT] [STRING]
  warn EXPR [STRING]
  owe EXPR to ENTITY [by EXPR] [as NAME]
  count EXPR as NAME
  consume EXPR                     // on an asset: lowers its basis (depreciation)
  carry EXPR to UNIT within SPAN   // a disallowed loss joins a nearby purchase's basis
```

| trigger | fires | context |
|---------|-------|---------|
| `on in` | value arrives at the governed thing | `amount from to party purpose date self owner` |
| `on out` | value leaves it | same |
| `on gain` | parcels leaving it realize a gain | `gain proceeds basis held amount from to date self owner` |
| `on spend` | money held for a restricted entity leaves its owner | `amount from to party date self` |
| `on flow` | a flow of the governed purpose; under an asset, a flow whose purpose is `of` it | `amount from to party purpose date self owner` |
| `each month`, `each year` | a period of the governed thing ends | `date year month self owner` |
| `each year closing MM-DD` | the year closes on that day of the next | same |
| `by EXPR` | the journal reaches that date | `date self owner` |
| `always` | after any change to the governed thing | `balance date self owner` |

`on in from X` is `on in` with `when from is X` first. Every context has `year`,
`month` (of the recognition's start), `date`, `flow`, `purpose` and `description`
where a flow triggered it.

**What a law governs**:

- In an account kind, asset kind or party kind: every thing of that kind.
- In an account, asset or entity: that thing.
- In a purpose: every flow of that purpose and those beneath it. A law in a
  purpose needs no trigger: it is `on flow`. `total(month|year|ever)` there is
  the purpose's own total.
- Top-level in a system: the owners who live there (a household as one) and
  everything they own and do. Top-level in a project: the whole book.

`self` is the governed thing, or for a purpose law the flow's owner; `owner` is its
owner. **Tallies** belong to owners: `count` adds to a line of the owner's year
(the household's, for a member governed as one), and `tally(x)` reads it;
`tally(x, year - 1)` reads another year. Properties and params are read as they
stood on the day being judged (§3).

**Order**: a law that reads a tally runs after every law that counts into it on the
same occasion; a cycle is an error. Otherwise declaration order.

**Effects**: `require` fails with an error unless `else` prices it; `warn` warns; a
violation is reported once per subject and window, at the flow that crossed the
line. `owe` creates an obligation to an entity. `consume` lowers the governed
asset's basis by an amount (depreciation, depletion). `carry` holds a disallowed
loss and adds it to the basis of the nearest acquisition of that commodity within
the span, before or after (a wash sale). Every `require` and `warn` comparing two
amounts records its headroom.

### Expressions

```text
or   and   not
== != < <= > >= is
+ -   * /   unary -
postfix: .field   [key, …]   (args)
atoms:   24_500 USD  10%  2026-04-15  59y6m  "text"  empty  name  UNIT  #purpose  ^code
         ( EXPR )  if EXPR then EXPR else EXPR
```

Types: `amount number bool date span text place entity kind unit purpose asset
schedule`. Numbers are exact rationals; `amount * number` rounds half to even.

- `x is K` tests against a kind, an entity or place (the same or a descendant), a
  purpose (`purpose is food`, `purpose is repair of self`), a code, or a glob.
- Fields: `.balance .owner .kind` on accounts; `.owner .kind .age` on entities;
  `.cost .basis .in-service .parts` on assets (and their declared properties);
  `.unit` on amounts; `.year .month` on dates; `.of` on a purpose.
- Functions: `total(in|out, window)` on things; `total(window)` under a purpose, and
  `total(#PURPOSE, window)` anywhere, for the subject; `tally(name [, year])`; `min
  max abs`; `progressive(schedule, x)`; `straight-line(cost, life, from, period [,
  mid-month])` (this period's share); `value(x, UNIT)`; `date(y, m, d)`;
  `remaining`.

## 7. Claims

A claim is value one side owes the other, between an event and its counterpart:

```text
12 checking -> jo 600 USD due 04-01                lent: jo owes me 600
12 jo -> checking 200 USD                          settles 200 of it
24 visa -> delta 420 USD for lumen                 paid for lumen: lumen owes me 420
27 halcyon owes studio due 30d ^inv-2026-01        an invoice, itemized
  3_000 USD #design "brand refresh"
    800 USD #design "icon set"
26 halcyon -> checking 3_800 USD ^inv-2026-01      settles exactly that claim
05 me owes pge 142.50 USD due 02-20 #utilities     a bill received
20 checking -> pge 142.50 USD                      settles it
04-20 ^inv-2026-01 due 05-15                       more time
09-30 ^inv-2026-02 waived "written off"            forgiven
```

- A flow to a party with `due`, or for a party with `for`, is a claim the party
  owes the flow's owner. `PARTY owes OWNER AMOUNT` and `OWNER owes PARTY AMOUNT`
  record one without moving money; items break it down.
- A later flow from the party to an owner (or from an owner to the party) settles
  its open claims: those its codes name, in order; else the one whose open amount is
  exactly the flow's; else the oldest first. What remains of the flow is an
  ordinary flow.
- A contract's missed occurrence is a claim while it is missing (§5), and its
  deposit is a claim the party holds.
- A claim's purpose is its recognition: an invoice is income when invoiced in
  accrual books, when settled in cash books (the owner's `books cash|accrual`,
  default cash). A claim `waived` is forgiven: in accrual books, what was
  recognized is reversed.
- `claims` lists what is open, with age and due day; `check` warns on what is past
  due; `available` counts claims as coming in, never as money to spend.

## 8. Parcels, lots, assets and gains

Money is held as parcels `(quantity, basis, acquired, transaction, tie)`. Parcels
that agree on all of these merge; base-currency money at its face value, tied to
nothing, is plain and always one parcel.

- **Arrival** from a party or `?` creates a parcel. Its basis is its cost: face
  value in the base currency, or `P × quantity` for an `@ P` price, or what the
  exchange gave. An account kind that says `basis zero` gives none (pre-tax
  deferrals). `basis AMOUNT` overrides both. It is tied to the paying party when
  that party's kind is `restricted`, and to the `for` entity when the flow names
  one.
- **Transfers** between an owner's holdings move parcels unchanged.
- **Relief** chooses which parcels leave: ties first, then the policy (the
  selector's, the account's, its kind's, then the commodity kind's; `currency` is
  FIFO). Selectors: `[2024]`, `[2026-01]`, `[2026-01-20]`, `[^code]`, `[fifo]`.
  Parcels that differ with no policy are ambiguous: an error listing each candidate
  and its gain, while quantities move FIFO.
- **Realization** happens when parcels change commodity, leave the owners for a
  party, or leave a `deferred` account for one that is not. `gain = proceeds −
  basis`, and `on gain` fires per parcel.
- **Revaluation.** Flows with `market` (usually `via market` on an assertion)
  change what an account holds without realizing: growth arrives with no basis, a
  loss shrinks parcels and keeps their basis.
- **Splits** scale quantities and keep basis.

**Assets** are identified things, and the flows about them say so by their
purpose's object:

- `#purchase of ASSET` acquires it: the flow's cost (with its costs of exchange)
  is its first *part*. An asset may also arrive unbought, in an opening or a
  statement with its `basis` (a gift, an inheritance).
- A purpose whose root is `capital` and that takes an object adds a part:
  `#improvement of condo`. A part has a cost, a day, and a basis that laws may
  `consume`.
- A flow `#repair of` an asset is spent: it adds no part. Rent received, insurance
  and interest `of` an asset are income or spending about it.
- `#sale of ASSET` on money arriving disposes of it: every part is relieved, and
  the gain is the proceeds, less the sale's costs, less the parts' basis. `DATE
  ASSET ends` disposes of it for nothing.

The asset's `cost` is the sum of its parts' costs, its `basis` what remains of
them; laws of the asset's kind run for each part (so each improvement depreciates
on its own schedule). `why ASSET` shows the parts, what consumed them, and every
flow about the asset by purpose.

Restricted money is money held for someone: tied when it arrives from a restricted
party or `for` one (an envelope, a tenant's deposit). It is not available to spend;
its entity's `on spend` laws judge it when it leaves the owners; a flow `for` the
owner releases it.

## 9. Derived events

Derived events are computed during the run, never written into the journal. Each
names the line and the declaration it comes from, `why FILE:LINE` shows it, and an
editor shows it as a hint on that line.

| derived | from | what it is |
|---------|------|------------|
| a loan payment's interest and principal | the contract's `loan` | `#interest` (of its asset) and the debt's decrease |
| escrow | the contract's `escrow` | a flow into that holding |
| a match | the contract's `match` | a flow from the party |
| a share | `business N% for OWNER` on a contract, party, purpose or asset | the same purpose, N% of each flow, borne by that owner |
| recognition | `covers`, `for PERIOD` | how a flow's amount spreads over days |
| sales tax | a party kind's `sales-tax` | the tax inside a price paid to it (`#sales-tax`, a share of the flow; part of the cost of a purchase) |
| exchange cost | an exchange with a market price that day | what was given less what was got, valued that day (`#exchange-cost`) |
| depreciation | `consume` in a law | a part's basis consumed |
| a wash sale | `carry` in a law | a loss moved into another parcel's basis |
| a late occurrence | a contract | a claim, until the occurrence is written |
| a claim paid for | `for PARTY` on a payment | the party owes it; the payment is its, passed through the owner |

A share is a flow of its own: the studio's 27.00 of a 45.00 phone bill is a flow
`#phone` owned by `studio`, and Sam's own phone is 18.00. Budgets, `flow` and the
tax see both; the bill as paid is one flow in `register`.

## 10. Dates, files and documents

`axiom.ax` marks the project root; every `.ax` under it is loaded, and `systems/`
may add or override the embedded systems. How files are arranged is a convention,
never a law:

- A full date is right wherever it is written.
- A short date takes the rest from its context: the nearest heading above it in
  the file (`2026`, `2026-02`); else the file's path, where a folder or file named
  `YYYY` gives the year, and `MM` beneath it (a folder, or `MM.ax`), or a file
  `YYYY-MM.ax`, gives the month. In `journal/2026/01.ax`, `15 job` is enough.
- Nothing checks that a file's dates agree with its name or its place. A short
  date with no context to complete it is an error whose fix is the full date.
- Sync writes each new line into the file its day belongs to by this convention
  (§13), creating it if needed.
- A file under `documents/` named after a code (`documents/inv-2026-01.pdf`)
  belongs to what the code marks; one named `YYYY-MM-DD PARTY` to that day's flows
  with that party. `why` lists them, and `register` marks the flows that have one.

## 11. Diagnostics

Every diagnostic follows `tests/mistakes/REPORT.md` §14, and these:

1. **The headline states the fact in the book's words**: the names, amounts and
   days involved ("checking holds 3,015.80 USD, not 3,051.80 USD"), never a parser's
   or an index's.
2. **Labels point at causes.** The primary label is in the reader's file; related
   labels point at what made it so: the declaration of the rule, the contract that
   derived a flow, each flow a tally counted, the terms in force that day. Built-in
   sources are marked and never primary.
3. **The fix is an edit** wherever one is mechanical, and each is a code action for
   an editor: the full date, the near name, `^` for a code written as a purpose,
   the amendment that would relax a budget this month, the `!` that accepts.
4. **One root cause is reported once**; what it breaks downstream is summarized,
   not repeated.
5. **Derived flows are explained where they come from**: a problem in a derived
   flow points at the line that caused it and the declaration that derived it.
6. **A failed assertion shows its window**: a table of every flow since the last
   passing assertion (day, other end, amount, running balance, purpose or
   description), and names the likeliest cause among: a transposed digit; a flow
   entered twice, backwards or with the wrong sign; a flow of that size missing; a
   contract occurrence due and not written; a claim settled off the book; a flow the
   book derives, written again by hand; a pending flow that settled; a flow the
   bank dated on the other side of the assertion.
7. **Everything inferred can be asked about**: `why FILE:LINE` says what a line
   means and where each part of that meaning came from.

`check --json` writes each diagnostic as one JSON object per line: its code,
severity, headline, labels (file, line, column, text), notes, and fixes as edits.

## 12. Command line

```text
axiom check     [--strict]                     diagnostics, late promises, a summary
axiom balance   [GLOB…] [--at DATE] [--value] [--monthly]   holdings, assets, claims
axiom register  ACCOUNT|OWNER|PARTY|ASSET|CONTRACT [--from DATE] [--to DATE]
axiom flow      [--by month|year] [--by purpose|party] [--from DATE] [--to DATE]
axiom available [--at DATE]
axiom budget    [MONTH|YEAR]
axiom limits    [YEAR]
axiom claims    [--at DATE]
axiom contracts                                every promise: terms, next due, kept, late
axiom tax       [YEAR]
axiom gains     [YEAR]
axiom lots      [ACCOUNT] [--at DATE]
axiom forecast  [--until DATE] [--paths N]
axiom why       TARGET                         a name, #purpose, ^code, law, tax line, "text" or FILE:LINE
axiom sync      [NAME…] [--dry]                bring in what is new (§13)
```

Global options: `--for ENTITY`, `--relaxed`, `--today DATE`, `--color
auto|always|never`, and `--json`, which writes any view as JSON for the scripts
that feed and read the book.

## 13. Sync

Sync is how a book stays current without being typed: bank and card statements,
invoices issued and bills received, prices, and the parameters laws read. Axiom
never touches the network; each source is a command whose output Axiom reads,
recognizes, reconciles and writes.

```text
/// Chase's export of the checking account.
sync checking
  run   chase-export checking --since {since}
  csv   date "Posting Date" "MM/DD/YYYY", amount "Amount", memo "Description", balance "Balance"

sync visa
  run   chase-export visa --since {since}
  csv   date 1, amount 4 flipped, memo 3

sync invoices
  run   invoicing-export --since {since}       // prints Axiom: `owes` statements

sync prices
  run   quotes {units} --since {since}
  into  prices/{year}.ax

sync cpi
  run   bls-series CUUR0000SA0 --since {since}
  into  param cpi
```

**Feeds.** A sync named after an account reads records of that account: `csv`
maps columns (by header or by number) to `date` (with its format), `amount` (money
into the account is positive; `flipped` for exports that show charges positive, or
`debit` and `credit` columns), `memo`, and optionally `balance` and `pending`. Each
record then goes through:

1. **Recognition.** The other end is the entity or account whose `known-as` matches
   the memo (the longest match wins; two that tie are an error naming both); a
   memo matching one party inside another's (`PAYPAL *ETSY SELLER`) is the inner
   party `via` the outer. Codes are found by the declared `code` globs, in any case
   (`INV-2026-01` is `^inv-2026-01`).
2. **Reconciliation.** A record matches a flow already on that account with the
   same amount, within three days: matched records are already written. A flow
   written by hand from a receipt is not written twice, and a transfer between two
   synced accounts is written once.
3. **Promises.** A record with a contract's party, within half a cadence of an
   occurrence due and unwritten, is written as that occurrence (`01 flat`, or `08
   phone 47.30 USD` when it differs).
4. **Claims.** A record from a party with open claims carries the codes it named;
   settlement follows §7.
5. **Writing.** Each new line goes, in day order, into the file its day belongs to
   (§10), with its date as short as that file allows. A record no one is known as
   goes to `?`, with its memo as its description. The last `balance` becomes an
   assertion. A `pending` record is written in parentheses, and settles when its
   posted record arrives.

**Files.** A sync with `into PATH` reads Axiom text (`{year}` in the path splits
it by year); `into param NAME` reads rows of that param. What is already there
(the same day and subject, the same row key) is kept as written, and the rest is
added in order. A sync with neither reads Axiom statements into the journal, as a
feed does.

**Rules.**
- Nothing already written is ever changed: sync only adds facts.
- A command runs from the project root, with `{since}` (the day after the last
  record the book has from this sync, or its first day), `{today}` and `{units}`
  (the commodities held). Its errors are shown, and a command that fails writes
  nothing.
- A file is written only if the book still parses, and `axiom sync --dry` prints
  what would be written, as a diff.
- `check` groups the memos nothing recognized and offers, for each, the `known-as`
  line that would.
