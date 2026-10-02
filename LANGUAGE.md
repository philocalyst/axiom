# The Axiom language

This is the normative reference. [DESIGN.md](DESIGN.md) says why it is shaped
this way and which theories it rests on. This is version 4: the language after
the move from a chart of accounts to agents, resources, events and promises.
`examples/v4-sketch/` is its first worked example, and every example in
`examples/` is an acceptance test.

## 0. The model in one page

Axiom records economic **events** that move or use **resources** between
**agents**, the **promises** that say which events are to come, and the
**norms** that say what must hold.

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
  history, never written. An asset never moves like money: flows are *about* it
  (`#improvement of condo`).
- **Units** are what quantities are counted in. Money and holdings are
  commodities (`USD`, `EUR`, `VTI`), held as parcels that remember their cost.
  Measures (`HR`, `MI`, `KWH`, `SQFT`) are never held: they count work and use.
  Units combine, so a price is `USD/VTI`, a mileage rate `USD/MI`, and a rent
  `USD` a month.
- **Purposes** say what an event is for, as a tree (`groceries` is `food` is
  `spending`) under four roots: `income`, `spending`, `capital`, and `transfer`
  for what only passes through (a gift received, tax withheld, a distribution). A
  purpose is written (`#groceries`) or inferred from the promise, the party, the
  money or the accounts.
- **Flows** move value between two ends. **Measures** record work done or a thing
  used (`me worked 6.5 HR for halcyon`, `car used 44 MI for studio`), which moves
  nothing but which amounts and laws are computed from.
- **Promises** are what an agent has committed to: a contract promises flows on a
  schedule (pay, rent, a loan, a subscription), and a claim promises one (an
  invoice, a loan to a friend, a deposit). A promise has a deadline and someone to
  blame when it is missed; whatever is due and unpaid is a claim.
- **Everything declared can change.** A declaration is the first value of a
  thing's terms and properties; the journal changes them from a day, for good or
  until another (`07-01 flat now 3_050 USD monthly`). Values that change on days
  (terms, properties, prices, params, budgets) are read as they stood on the day
  being judged.
- **Laws** attach to kinds, purposes, individual things and systems (`us`,
  `us/ca`), and say what must hold, what is owed if it does not, what is counted,
  and when they do not apply.
- **Derived events** are what the book implies without anyone writing it: a loan
  payment's interest, a bill's business share, the sales tax inside a price, the
  cost of an exchange rate, depreciation, a wash sale, a late fee, an employer's
  match, a card's cash back. Most are declared once with `also` on a promise,
  party, kind or purpose. They are computed, explained by `why`, and shown by an
  editor as hints.
- **Sync** reads statements, invoices, prices and parameters in the formats the
  book declares, recognizes who and what each record is by patterns the book
  declares, reconciles it with what is written, and adds only what is new.

There are no income, expense or equity accounts.

## 1. Lexical structure

Source is UTF-8, read line by line.

- **Items** start in column 0. Indented lines (spaces only; a tab in indentation is
  an error) belong to the nearest less-indented line above them.
- **Comments**: `//` to end of line, at line start or after whitespace. **Doc
  comments**: consecutive `///` lines document the item, leg, law or contract that
  follows. A law's doc has two parts: the first paragraph says what the law is, a
  paragraph starting `To fix:` says what to do when it fails.
- **Headings**: a line holding only a year (`2026`) or a month (`2026-02`) gives
  the dates below it their year, or year and month (§11).
- Blank lines mean nothing.

| token       | shape                                                   | examples |
|-------------|---------------------------------------------------------|----------|
| date        | `YYYY-MM-DD`; or `MM-DD` or `DD` where the context gives the rest (§11) | `2026-01-15`, `01-15`, `15` |
| month       | `YYYY-MM`                                               | `2026-03` |
| number      | digits, `_` separators, optional `.frac`                | `84.20`, `24_500` |
| percent     | number then `%`                                         | `12%`, `5.875%` |
| fraction    | number `/` number                                        | `1/3` |
| span        | `(digits [ymwd])+`; years may have a fraction           | `30d`, `59y6m`, `27.5y` |
| name        | `[a-z0-9][a-z0-9_-]*`, `/`-separated, may contain `*`   | `checking`, `trader-joes`, `joint/checking`, `401k` |
| unit        | `[A-Z][A-Z0-9_.]*`, and `U/U` for a rate               | `USD`, `BRK.B`, `USD/MI` |
| purpose     | `#` then a name                                         | `#groceries`, `#repair` |
| code        | `^` then `[a-z0-9][a-z0-9_:./-]*`, may contain `*`      | `^inv-2026-01`, `^check-1041` |
| string      | `"…"` with `\" \\ \n \t`                                | `"food for the routine"` |
| punct       | `-> .. ... = == != < <= > >= + - * / @ ( ) [ ] , : ! ? . \|` | |

A token starting with a digit is a date, month, number, percent, fraction or span
if it matches that shape exactly, and a name otherwise (`401k`). At the start of an
item, a lone number of one or two digits is a day (§11). Numbers carry no sign;
`-` is an operator, except before an amount in a value or an opening line, and at
the start of a line item. Case separates names (lowercase) from units (uppercase).

`:` means only "is a kind of", in declarations: `entity lumen : employer`. It never
appears on a journal line.

Keywords are recognized by position and not reserved. `empty` is the zero of every
unit; a bare `0` where an amount belongs is an error whose fix is `empty`.

## 2. Reading a journal

Every journal line is `DATE SUBJECT …`, and the word after the subject says what
kind of line it is. A reader skims that column.

```text
06 visa -> trader-joes 84.20 USD                    ->     a flow (§3)
01 flat                                             —      a promise kept (§7)
08 phone 47.30 USD                                  AMOUNT kept, differently this once
27 halcyon owes studio 3_800 USD due 30d ^inv-12    owes   a promise of one flow: a claim (§7)
31 checking = 8_828.87 USD                          =      a value: a balance (§5)
02 VTI = 280.14 USD                                 =      a value: a price
01 ^bldg-water = 155.00 USD                         =      a value: a named measure
12 me worked 6.5 HR for halcyon ^inv-12             worked a measure: work done (§5)
21 car used 44 MI #business-travel for studio       used   a measure: a thing used
07-01 flat now 3_050 USD monthly                    now    a change, from this day (§5)
12-01 flat waived "December free"                   waived an event (§5)
10 netflix ends                                     ends   an event
06 ^check-1041 settled                              settled, void, returned, split
2026-04-15 us filed 2025                            filed  a return, as filed (§11)
```

`axiom fmt` lays every file out this way (§13): subjects, verbs and amounts in
columns, and each flow's tail in one order.

## 3. Flows

```text
DATE FLOW
FLOW   := SOURCE -> TARGET [@ PRICE] TAIL (INDENT (LEG | ITEM))*
SOURCE := [END [SELECT]] [AMOUNT | all [UNIT]]
TARGET := [END] [AMOUNT]
END    := ACCOUNT | OWNER | PARTY | UNIT | PROMISE | ?
LEG    := END [SELECT] LEGAMOUNT [@ PRICE] TAIL
ITEM   := [+ | -] AMOUNT TAIL
TAIL   := [#PURPOSE [of THING]] [STRING] CODE* [for WHOM|PERIOD] [due WHEN] [against CODE] [via PARTY] [basis AMOUNT] [! [STRING]]
LEGAMOUNT := AMOUNT | ... | = AMOUNT | all [UNIT]
```

Amounts are the forms of §4. The tail's clauses may come in any order; `axiom fmt`
writes them in the order above.

**Ends.** Each end of a flow is one of:

- an **account**: money leaves or joins a position with an institution;
- an **owner**: money or a thing held directly (`checking -> me 100 USD` is cash in
  hand; `me -> taqueria 18.50 USD`);
- a **party**: money leaves the book's owners to it, or comes to them from it;
- a **commodity in party position**: its issuer, as in `VTI -> fidelity 198.12 USD`
  (a fund pays);
- a **promise**: what it is owed or owes (`checking -> mortgage 1_000 USD` pays the
  loan's principal; §7);
- `?`: an unknown party, for money whose other end nobody knows;
- nothing: the other side is the legs (a one-sided split), or, for an exchange
  written with only a source, the same account (`fidelity 20 VTI -> 5_940 USD`).

An asset is never an end: a flow says which asset it concerns through its
purpose's object (§9): `#purchase of laptop`, `#improvement of condo`, `#sale of
condo`. Accounts, owners, parties, assets, promises, purposes and units are
separate namespaces; a name that could mean two of them is an error at the later
declaration, except that a contract may share its party's name (§7).

```text
06 visa -> trader-joes 84.20 USD                      a payment, #groceries by its party
09 visa -> amazon 62.40 USD #household "hooks"        purpose and description written
20 checking 2_000 USD -> fidelity 7 VTI               an exchange
20 checking -> fidelity 7 VTI @ 285.70 USD            price given: 1,999.90 USD out
05 fidelity[2026-01-20] 1.62 VTI -> 481.14 USD        a sale; the proceeds stay at fidelity
24 visa -> best-buy 1_739.13 USD #purchase of laptop  the laptop arrives (§9)
12 checking -> jo 600 USD due 04-01                   lent: jo owes it (§7)
24 visa -> delta 420 USD for lumen                    paid for lumen: lumen owes it (§7)
20 checking -> etsy-seller 20 USD via paypal          paid through an intermediary
16 checking -> savings 400 USD for emergency          held for the `emergency` envelope
01 checking -> insurer 1_140 USD #insurance for 2026  paid now, recognized over 2026
02 checking -> plumber (350 USD) ^check-1041          pending until settled (§5)
14 checking -> ? 40 USD                               to someone unknown
14 checking -> me ? USD                               an amount inferred from values
```

**Split flows.** When the header names only one end, the indented legs are the
other side; their total is the header amount, or the sum of the legs, and at most
one leg is `...` (the remainder). A leg `= AMOUNT` makes its account's balance
equal that amount after the flow. Many-to-many is an error.

**A leg between two parties passes through the transaction's owner.** When the
header's end and a leg's end are both parties, the value is the owner's on the
way: `lumen -> irs 498 USD` in Sam's paystub is Sam's wages, paid on to the IRS.
The owner of a transaction is the owner of its accounts, or `me`.

**Line items.** An indented line that names no end is an item of the flow above it,
between the same two ends, with its own purpose, description and codes:

- `AMOUNT TAIL` is carved out of the header's amount: what the items do not claim
  keeps the header's purpose;
- `+ AMOUNT TAIL` comes on top of it: a fee, a tip, a share of a utility;
- `- AMOUNT TAIL` is taken off it: a discount, a credit, or a cost withheld from
  proceeds (a broker's commission, a seller's closing costs).

When the header gives no amount, it is the sum of its items. Under a split, items
sit between the header's end and the remainder leg. An item `- AMOUNT #sale of
ASSET` in a purchase is a trade-in: it disposes of the asset at that amount, as
credit toward the price, and a derived sales tax excludes it. An item in another
unit than the header's is an exchange of its own (gas paid in ETH on a swap).

```text
14 visa -> target 120.00 USD #household
  32.10 USD #groceries
  12.00 USD #gifts "for jo's birthday"                // 75.90 stays #household
15 title-co -> checking 627_000 USD #sale of condo
  - 6% #selling-costs "commission"                    // 37,620 withheld
```

**Pairing.** The same unit on both sides is a transfer, and stated amounts must
agree. Different units are an exchange at `out / in`. An `@` price given with both
amounts must agree, or the difference is written as a leg or an item: Axiom never
books a difference silently. In an exchange, legs and items whose purpose is a cost
(`#fees`, `#closing-costs`, `#selling-costs`) are costs of the exchange: a sale's
gain is less by them, a purchase's basis more.

**Purposes.** An event's purpose, first match wins:

1. written on the item, the leg, or the header (for every leg that says none);
2. its promise's (§7);
3. its party's: the party's own `#purpose`, else its kind's
   (`entity corner-store #groceries`; `kind grocer … purpose groceries`). A party
   kind's `purpose` is what money *to* its parties is for, and `pays` what money
   *from* them is for (an insurer: `purpose insurance`, `pays claim-payout`);
4. its commodity's kind in party position (`kind fund … pays dividend`);
5. its accounts' kinds (a `401k`'s `takes pre-tax-deferral from wages`).

Parent and child purposes are compatible classifications at different levels
of detail; the first match still selects the event's purpose. Two sources naming
unrelated purposes, or different explicit objects, are an error naming both.
An event none of them
classifies is *unclassified*; with a description it is *unclassified, described*.
`check --strict` asks for a purpose on each. `#NAME of THING` gives a purpose its
object where the purpose takes one; a purpose that requires an object reports its
absence; the object may be any declared thing (an asset, an employee, a unit). A
flow in the opposite direction to its purpose, with no `pays` to say otherwise, is a
refund: money from a grocer is groceries, negative. Between two owners, a flow
needs a purpose of the `transfer` root (`#contribution`, `#distribution`,
`#reimbursement`, `#loan`): a flow between owners with none asks which.

**Descriptions** say why in words. They have no meaning to the book: `register`
shows them, `flow` groups unclassified flows by them, `why "text"` searches them,
and `check` offers to declare a purpose for one that recurs.

**Codes.** `^code` names a flow, leg, item, measure, value, claim or change, so
other lines can refer to it (§4): a payment carrying the code of an open claim
settles it (§7); a statement on `^code` changes or ends what it names (§5); `why
^code` lists everything it names, with its documents (§11).

**Dates.** A flow has the day money moves and the period it belongs to. `for
PERIOD` (a year, a month, a date, or `DATE..DATE`) recognizes it evenly per day
over that period; `for 2025` on an estimated tax payment made in January makes it
2025's. `DATE..DATE FLOW` is the same with the payment on the first day. Tallies,
window totals, budgets, `flow` and `tax` read the recognition; balances and relief
read the day.

**Other tail clauses.**
- `for` names whose the money is, or when:

  | `for` … | on | means |
  |---|---|---|
  | a period (`2025`, `2026-03`, `DATE..DATE`, `last month`, `last quarter`, `last year`) | any flow | recognized over that period |
  | an envelope or a party (`emergency`, `dana`) | money that stays with the owners | held for it: tied, not available (§9) |
  | the flow's own owner | money that stays | releases what was held |
  | a party | money that leaves the owners | paid on its behalf: it owes it back (§7) |
  | another owner | money that leaves one owner | paid for that owner: a claim between them (§7) |
- `due WHEN` (a date, or a span after the day) makes the flow a claim (§7).
- `against ^code` says the flow is about an earlier one. In the opposite direction
  it refunds it, in proportion: its purpose and object are the earlier flow's,
  recognition not yet used stops, and an asset part's cost falls (a return, a
  chargeback, a VAT refund at the border). In the same direction from another
  source it reimburses it (an HSA paying back a bill paid out of pocket), and laws
  read what it reimburses.
- `via PARTY` names an intermediary the money passed through (`checking ->
  etsy-seller 20 USD via paypal`): the party at the end is who it was for.
- `basis AMOUNT` gives arriving parcels a total basis other than their cost (a gift
  of shares that keeps the giver's basis).
- `!` waives every law violation this flow raises, priced ones included. The waiver
  is reported; a `!` that waives nothing is a warning.

## 4. Amounts and references

```text
AMOUNT := NUMBER UNIT | (NUMBER UNIT) | ? UNIT | empty
        | PERCENT [of REF] | FRACTION of REF | REF [@ PRICE] | NUMBER UNIT @ PRICE
        | AMOUNT up to AMOUNT
REF    := ^CODE [SELECT]* | NAME [SELECT]* | INPUT | AMOUNT
SELECT := [DATE | MONTH | YEAR] | [#PURPOSE] | [END] | [^CODE] | [UNIT] | [POLICY]
```

An amount is written, or computed from what else the book says. `(350 USD)` is
pending; `?` is inferred (§5). A percent alone is of the header's amount. `X up
to Y` is the smaller of the two. In declarations (`also`, templates, laws) an
amount may be any expression of §8 over the flow (`amount`, `gross`), params,
properties and totals.

**References.** A reference names a fact, or a part of one, and stands for its
amount:

- `^code` is what the code names: a flow's amount, a claim's, a measure's quantity,
  a value;
- `NAME` is a promise's amount as its terms stand that day, or an input (§7);
- **selectors narrow** what a reference or an account means: `[#purpose]` the
  parts of that purpose, `[END]` the leg to that end, `[^code]` the parts that code
  names, `[UNIT]` the parts in that unit, `[DATE]` (or a month or year) what
  happened then. On an account in a flow's source, the same selectors choose which
  parcels leave (§9): `fidelity[2026-01-20]` is the lot bought that day.

```text
01 ^bldg-water = 155.00 USD "the building's water bill, Q1"
01 flat
  + 12% of ^bldg-water #utilities                        // 18.60, and why
05 me owes pge 142.50 USD #utilities ^pge-jan            // the whole bill, paid by me
05 jo owes me 1/3 of ^pge-jan                            // 47.50: her third
27 halcyon owes studio due 30d ^inv-12
  ^inv-12[HR] @ 150 USD/HR #design                       // the hours the code names, at the rate
  + 8.625% of ^inv-12[#design] #sales-tax                // the tax on those lines
03-15 halcyon owes studio 1.5% of ^inv-12 #late-fee      // on what it named
```

A reference to a later fact, or a cycle of references, is an error. Every computed
amount shows its arithmetic in `why` and as an editor's hint.

## 5. Statements

A statement says one thing about one subject on a day. It moves no value itself:
what follows from it is derived.

**Values** (`=`) state what a subject is worth or holds at the end of the day:

```text
31 checking = 8_828.87 USD                     a balance, as the statement shows it
31 visa = 2_333.99 USD                         owed on a card or loan is positive
31 retirement = 58_420.18 USD via market       the gap is growth from the market
30 me = 45.15 USD !                            the gap is accepted as unexplained
31 mortgage = 310_978.17 USD                   a loan's balance (§7)
02 VTI = 280.14 USD                            a price: one VTI that day
01 ^bldg-water = 155.00 USD                    a named measure, for references
31 ^odometer = 48_210 MI                       a reading
```

A failed balance is an error that shows the window since the last passing value
and its likeliest cause (§12); the gap is carried, so a later value failing by the
same amount is not reported again. `!` accepts the gap as unexplained; `via PARTY`
makes it a flow with that party, and `via market` is a revaluation (§9). A value
that depends on an unsolved `?` is not checked, and says so once.

**Measures** record work done or a thing used. Nothing moves; laws and amounts
read them as they read flows (`on flow` fires, `total` counts them in their unit):

```text
12 me worked 6.5 HR for halcyon ^inv-12              hours toward an invoice
21 car used 44 MI #business-travel for studio        the studio's use of the car
```

**Changes** (`now`) restate part of a thing's declaration from a day. Anything a
declaration says can change this way: a contract's terms, a property, a budget, a
claim's due day, a change's own span.

```text
07-01 flat now 3_050 USD monthly "renewed at 3,050"
03-01 gym now 120 USD monthly until 05-31 ^promo "spring promotion"
05-28 ^promo now until 08-31 "extended"
07-01 flat now share 20% for studio
2029-03-01 mortgage now at 6.25%
06-15 me now lives us/ny
12-01 #food now budget 1_200 USD monthly until 12-31 "the holidays"
04-20 ^inv-12 now due 05-15
12 ^inv-9 now "credit note CN-0002"                  // amends the claim, item by item
  - 900.00 USD #design
  - 93.15 USD #sales-tax-collected
08-16 job now 4_400 USD twice monthly                // new terms, with new legs
  ftb   empty                                        // a leg dropped
  dtf   190.40 USD                                   // a leg added
```

A change holds from its day. With `until`, what held the day before it resumes the
day after; a later change overrides an earlier one for the days it covers; the
declaration is simply the first. What it does not restate carries over. Laws,
promises and reports read every value as it stood on the day they judge; `why
NAME` lists a thing's changes. A code on a change names it, so a later statement
can extend it, cut it short, or end it. A shortened `until` or `due` date is the
first such day on or after the statement's.

**Events** happen to their subject:

```text
12-01 flat waived "December free: greystar's gift"   a promise's occurrence released
04-01 gym waived until 06-30 "frozen while abroad"   every occurrence in the span
09-30 ^inv-9 waived "written off"                    a claim forgiven
10 netflix ends                                      a promise, account or asset ends
06 ^check-1041 settled                               pending → real on this day
20 ^check-1044 void                                  pending → never happened
04 ^deposit-77 returned                              real → reversed on this day
22 FAST split 2 for 1                                every FAST parcel doubles; basis stays
```

`ends` on an account closes it (flows after are errors); on an asset, it leaves the
owners with nothing received; on a promise, nothing more is expected. `waived` on a
claim may carry a purpose (`#bad-debt`) and items, for what of it is recoverable. A settlement
event is dated on or after what it settles.

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
them, and they may be older than any param. A loan's balance comes from its terms
and needs no opening line. An asset may also arrive unbought later: `05-01 car
basis 12_000 USD since 2019-03-01` (a gift, an inheritance).

## 6. Declarations

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

- `base` is the book's reporting currency, required once it uses more than one.
  Each owner also has a **currency**: its own `currency` property, else its
  residence's system's, else `base`. Tallies and laws count in it (§8).
- An **account** is of a kind whose root is `asset` (what is yours at an
  institution) or `debt` (what you owe on one). `at PARTY` names the institution.
  Names are flat; a `/` groups them for reports.
- An **entity** is an owner or a party. The owners are `me`, entities `member` of
  a household `me` belongs to, entities with an `owner` among the owners (a
  business), and any entity named as the `owner` of an account or asset. Every
  other entity is a party. A party may carry its own purpose (`entity corner-store
  #groceries`), with or without a kind. A party that is never declared can still
  be written: it is untyped, and its flows need a purpose or a description.
- An **asset** is an identified thing, of a kind rooted at `thing`.
- A **commodity** of a kind rooted at `measure` (`HR`, `MI`, `KWH`, `SQFT`) is a
  measure: it counts work and use and is never held.
- A **purpose** is a node of the purpose tree. The roots are `income`, `spending`
  and `capital`; std ships the rest. A purpose may require an object: `of KIND`.
- A **budget** is sugar for the purpose's `budget` property, a `warn` on its total
  for each month or year (§8). `LIMIT` is an amount, or a share of another
  purpose's total in the same window (`budget fun 10% of #income monthly`). A budget
  that `carries` is judged on its total since it began against the sum of its
  limits through this window: an unspent month lends to the next, an overspent one
  borrows from it. A budget `funded from HOLDING into HOLDING` moves its limit each
  window into money held for it (an envelope, a sinking fund), and what the purpose
  spends is drawn from that money first.

Built-in properties:

| on | property | meaning |
|----|----------|---------|
| account, asset, business | `owner ENTITY [SHARE], …` | default `me`; `owner me 60%, theo 40%` gives each its share of what it earns and bears |
| | `holds UNIT, … \| any` | commodities it may hold; a measure never |
| | `select fifo\|lifo\|hifo\|prorata` | relief policy |
| | `opened DATE` | flows before are errors (`ends` closes it) |
| | `liquidity SPAN` | time to turn into cash |
| account, entity | `known-as PATTERN, …` | how it appears on statements (§14); a name is its own by default |
| entity | `lives SYSTEM, …` | residences; `now lives` moves them |
| | `citizen SYSTEM` | taxed by it wherever it lives |
| | `books cash\|accrual` | when claims are income or spending (default cash) |
| | `member ENTITY` | belongs to that household |
| | `owner ENTITY` | on a business: owned by that owner |
| | `of OWNER` | a client of that owner: what it pays is that owner's |
| | `currency UNIT` | what its tallies count in |
| asset | `part of ASSET` | a unit of a building, a room of a house: what is `of` the whole is shared among its parts |
| | `area`, `capacity` … | declared measures of it (`has area SQFT`), which shares divide by |
| commodity | `precision N`, `name STRING`, `liquidity SPAN`, `grows PERCENT yearly` | |
| kind | `restricted` | money from, or held for, entities of this kind stays tied to them |
| | `deferred` | accounts of this kind realize nothing inside |
| | `basis zero\|cost` | what basis arriving value takes (§9) |
| | `purpose NAME` | on a party kind: what flows with its parties are for |
| | `pays NAME` | on a commodity kind: what its issuer pays is for |
| | `takes NAME from NAME` | on an account kind: what arrives from flows of the second purpose is the first |
| | `select POLICY`, `liquidity SPAN` | defaults |
| | `has NAME TYPE` | declares a property of this kind's things |
| party kind | `sales-tax PERCENT` | the tax inside every price paid to its parties (derived, §10) |
| | `pays NAME` | what money from its parties is for |
| contract, party, kind, purpose | `also ITEM \| FLOW [when EXPR]` | an item or flow every matching flow implies (§10) |
| purpose | `of KIND` | requires an object of that kind |
| | `budget LIMIT monthly\|yearly [carries]` | see above |
| contract, party, purpose, asset | `share SHARE for ENTITY, …` | who bears each flow (§10): for an owner an allocation, for a party a claim on it. `SHARE` is a percent, a fraction, or a measure (`120 SQFT` of the thing's `area`) |

A kind's other property lines are defaults for its things. `TYPE` is one of `date
amount number percent span text name entity place kind unit bool purpose asset`,
and an amount type may name its unit (`has rate USD/MI`). An unknown property is an
error with a suggestion.

```text
code GLOB [GLOB…]          // `code inv-*`: codes matching it
  on KIND | NAME …         // may only name flows touching these
  known-as PATTERN         // how it appears in memos (§14)

param NAME [UNIT]
  KEY+ VALUE               // a year (latest ≤), a date, or names
  2026 single 0 USD 10% | 12_400 USD 12%

pattern NAME = PATTERN     // a named pattern (§14)
format NAME                // a record format (§14)
sync NAME                  // a source (§14)
```

A **system** (`us`, `us/ca`, a community system under `systems/`) is a file of
declarations. Its first lines may say `currency UNIT` (what its laws count in) and
`rates POLICY` (how it converts: `spot`, the day's price; or `param NAME`, such as
the IRS's yearly averages). It may declare the sources its data comes from.

## 7. Promises: contracts and claims

A promise is what an agent has committed to: flows to come, each with a deadline
and someone to blame when it is missed. A **contract** promises flows on a
schedule; a **claim** promises one. Whatever is due and unpaid is a claim, so a
late rent, an unpaid invoice and a friend's loan are the same thing to `check`,
`claims` and the forecast.

```text
contract NAME [with PARTY]
  [about] AMOUNT CADENCE [on DAY] (from | into) HOLDING     // the schedule
  [buy UNIT for AMOUNT CADENCE [on DAY] from HOLDING]       // a standing order
  [PURPOSE] [STRING]
  [from DATE] [until DATE]
  [due SPAN [else ITEM]] [grace SPAN]
  [for PERIOD] [covers the month|quarter|year | covers SPAN] [prorated]
  [rising PERCENT yearly | indexed to PARAM yearly]
  [share SHARE for ENTITY]
  [input NAME [UNIT]]
  [deposit AMOUNT [into HOLDING]]
  [loan AMOUNT on DATE at RATE over SPAN [for ASSET]]
    [resets EVERY from DATE to PARAM + PERCENT [cap PERCENT] [life PERCENT]]
    [prepay shortens | recasts]
  [also ITEM | FLOW]*                          // derived with each occurrence (§10)
  (LEG | ITEM)*                                // the template, as in a split flow
// CADENCE: daily | weekly | monthly | quarterly | yearly | twice monthly | every SPAN
// DAY: 15 | last | 15, last | 04-15 | monday … sunday
//      several days after `on` are each due: `yearly on 04-15, 06-15, 09-15, 01-15`
```

A contract without `with` is with the entity of its own name (`contract netflix`).
`about` says the amount varies (a utility): each occurrence states its own, and the
forecast uses this one. `for last month` recognizes each occurrence over the period
before its day (a sales tax return, a utility billed in arrears); an occurrence may
carry any tail of its own (`15 estimates 8_800 USD for 2025`). `covers the month`
is the calendar period that contains the due day, `covers 1y` twelve months from
it. `prorated` makes an occurrence that starts or ends inside its period that
share of it, by days, legs and all.

**Kept.** `DATE NAME` is one occurrence: the contract's flows on that day, as its
terms stand that day. An amount after the name replaces the contract's this once
(`08 phone 47.30 USD`), or, for `buy`, is what was bought (`20 vti-monthly 1.620
VTI`). Indented legs replace the template's legs of the same end, `...` absorbs the
difference, items add to it, carve it or take from it (§3), and `NAME = AMOUNT`
lines state its inputs:

```text
contract flat with greystar
  2_900 USD monthly on 1 from checking
  area 1_000 SQFT
  input water USD                              // stated by the occurrences that have it
  + 12% of water #utilities
  share 120 SQFT for studio                    // of its area: 12% of every flow is the studio's

01 flat                                        // 2,900.00: no water this month
04-01 flat
  water = 155.00 USD                           // 2,918.60: the water on top
```

An item that reads an input the occurrence does not state is left out.

**Missed.** Each due day is kept by the nearest occurrence within its `grace`
(default: half a cadence). A due day past its grace with no occurrence is missing:
a claim on whoever owes it (a rent the party owes, a bill the owner owes), reported
as late with the day it was due. `due SPAN else ITEM` gives the promise a deadline
after its due day, and what is added when it passes: `due 5d else + 5% #late-fee`.
The occurrence, when written, settles what is missing.

**Changes and events** (§5) apply to every occurrence from their day: new terms
(`now 3_050 USD monthly`), a property (`now share 20% for studio`), a new end
(`now until 2028-06-30`), `waived` for one occurrence or a span, `ends`. `rising 3%
yearly` and `indexed to cpi yearly` change the amount on each anniversary by
themselves.

**What a contract says applies to every occurrence:** its purpose, `covers` (each
payment is recognized over that span; ending early makes the unused part a claim on
the party, pro rata), `share` (§10), `also` (§10), `deposit` (paid at the start,
into the holding named, and owed back at the end: a tenant's deposit to you is held
for the tenant; yours to a landlord is a claim on it). In accrual books an
occurrence is income or spending on its due day, and the payment settles it.

**Loans** follow the ACTUS annuity. `loan AMOUNT on DATE at RATE over SPAN` is a
debt of the owner to the party. Its schedule gives each payment's interest
(`#interest`, `of` the asset when `for` names one) and principal, and its balance
on any day, which a value on the contract's name checks. A rate `now` changed, or
`resets` from an index param, refigures the payment over the rest of the term from
that day's balance (capped per reset and for life). A flow to the contract is a
prepayment: by default it `shortens` the loan and the payment stays; `recasts`
lowers the payment instead. Escrow and an employer's match are `also` lines:

```text
contract mortgage with rocket
  loan 320_000 USD on 2024-02-20 at 5.875% over 30y for condo
  monthly on 1 from checking
  also -> escrow 410 USD #escrow                          // with each payment
contract job with lumen
  4_600 USD twice monthly on 15, last into checking
  retirement 6%
  also lumen -> retirement 50% of [retirement] up to 3% of amount #match
```

**Claims** are promises of one flow:

```text
12 checking -> jo 600 USD due 04-01                lent: jo owes me 600
12 jo -> checking 200 USD                          settles 200 of it
24 visa -> delta 420 USD for lumen                 paid for lumen: lumen owes me 420
27 halcyon owes studio due 30d ^inv-12             an invoice, itemized
  3_000 USD #design "brand refresh"
    800 USD #design "icon set"
26 halcyon -> checking 3_800 USD ^inv-12           settles exactly that claim
05 me owes pge 142.50 USD due 02-20 #utilities     a bill received
```

- A flow to a party with `due`, or for a party with `for`, is a claim the party
  owes the flow's owner. `PARTY owes OWNER AMOUNT` and `OWNER owes PARTY AMOUNT`
  record one without moving money.
- A later flow between them settles open claims: those its codes name, in order;
  else the one whose open amount is exactly the flow's; else the oldest first. What
  remains is an ordinary flow.
- A claim's purpose is its recognition: an invoice is income when invoiced in
  accrual books, when settled in cash books (the owner's `books cash|accrual`,
  default cash). A claim `waived` is forgiven; in accrual books what was recognized
  is reversed.
- `claims` lists what is open, with age, due day and whom it blames; `check` warns
  on what is past due; `available` counts claims as coming in, never as money to
  spend.

## 8. Laws

```text
/// What the law is.
///
/// To fix: what to do when it fails.
law NAME [overrides NAME]
  TRIGGER
  when EXPR
  unless EXPR
  let NAME = EXPR
  require EXPR [else EFFECT]* [STRING]
  warn EXPR [STRING]
  owe EXPR to ENTITY [by EXPR] [as NAME]
  count EXPR as NAME
  consume EXPR                     // on an asset: lowers its basis (depreciation)
  carry EXPR to UNIT_EXPR within SPAN   // a disallowed loss joins a nearby purchase's basis
```

`UNIT_EXPR` is a full expression whose checked type is `unit`: either a written
commodity such as `VTI`, or a computed unit such as `amount.unit`.

| trigger | fires | context |
|---------|-------|---------|
| `on in` | value arrives at the governed thing | `amount from to party purpose date self owner` |
| `on out` | value leaves it | same |
| `on gain` | parcels leaving it realize a gain | `gain proceeds basis held amount from to date self owner` |
| `on spend` | money held for a restricted entity leaves its owner | `amount from to party date self` |
| `on flow` | a flow or measure of the governed purpose; under an asset, one whose purpose is `of` it | `amount from to party purpose date self owner` |
| `each month`, `each year` | a period of the governed thing ends | `date year month self owner` |
| `each year closing MM-DD` | the year closes on that day of the next | same |
| `by EXPR` | the journal reaches that date | `date self owner` |
| `always` | after any change to the governed thing | `balance date self owner` |

`on in from X` is `on in` with `when from is X` first. Every context has `year`,
`month` (of the recognition's start), `date`, `flow`, `purpose` and `description`
where an event triggered it.

**What a law governs**:

- In an account kind, asset kind or party kind: every thing of that kind.
- In an account, asset or entity: that thing.
- In a purpose: every flow and measure of that purpose and those beneath it. A law
  in a purpose needs no trigger: it is `on flow`. `total(month|year|ever)` there is
  the purpose's own total.
- In a contract: its occurrences.
- Top-level in a system: the owners who live there (a household as one) and
  everything they own and do. Top-level in a project: the whole book.

`self` is the governed thing, or for a purpose law the event's owner; `owner` is its
owner. **Tallies** belong to owners: `count` adds to a line of the owner's year
(the household's, for a member governed as one), and `tally(x)` reads it;
`tally(x, year - 1)` reads another year. Properties and params are read as they
stood on the day being judged.

**Norms are defeasible.** A law states what normally holds; the book says when it
does not:
- `require A else B else C` is an obligation with reparations: if A fails, B is
  owed instead; if B is not met by its deadline, C. A repaired violation is
  reported as repaired, not as a failure. `warn A` is `require A` whose failure
  costs nothing.
- `unless COND` is an exception the law itself knows (the statute's own list).
  `!` on a flow and `waived` on a span are exceptions the book states.
- When two laws say different things of the same subject, the more specific wins:
  a law on a thing over one on its kind, a kind over its parent, a project over a
  system, a child system over its parent, a later change over an earlier one.
  `overrides NAME` says so outright. Two laws of equal rank that disagree are an
  error naming both.

**Order**: a law that reads a tally runs after every law that counts into it on the
same occasion; a cycle is an error. Otherwise declaration order.

**Effects**: `require` fails with an error unless `else` repairs it; `warn` warns;
a violation is reported once per subject and window, at the flow that crossed the
line. `owe` creates a claim for an entity. `consume` lowers the governed asset's
basis. `carry` holds a disallowed loss and adds it to the basis of the nearest
acquisition of that commodity within the span, before or after (a wash sale).
Every `require` and `warn` comparing two amounts records its headroom.

### Expressions and units

```text
or   and   not
== != < <= > >= is
+ -   * /   unary -
postfix: .field   [key, …]   (args)
atoms:   24_500 USD  10%  1/3  2026-04-15  59y6m  "text"  empty  name  UNIT  #purpose  ^code
         ( EXPR )  if EXPR then EXPR else EXPR
```

Types: `amount number bool date span text place entity kind unit purpose asset
schedule`. Numbers are exact rationals; `amount * number` rounds half to even.

**Every amount has a unit, known before anything runs.** Literals carry theirs;
params and properties declare theirs; `amount` has the unit of what its subject
holds, or is "any commodity" where that is not one. `+`, `-` and comparisons need
the same unit; `*` and `/` combine units (`44 MI * 0.70 USD/MI` is `USD`). A tally
takes the unit of its owner's currency. The only conversion is `value(x, UNIT [at
POLICY])`, at the system's `rates` by default: a law that adds EUR to a USD tally
without it is an error naming both units and the fix.

- `x is K` tests against a kind, an entity or place (the same or a descendant), a
  purpose (`purpose is food`, `purpose is repair of self`), a code, or a glob.
- Fields: `.balance .owner .kind` on accounts; `.owner .kind .age` on entities;
  `.cost .basis .in-service .parts` on assets (and their declared properties);
  `.unit` on amounts; `.year .month` on dates; `.of` on a purpose.
- Functions: `total(in|out, window)` on things; `total(window)` under a purpose, and
  `total(#PURPOSE, window)` anywhere, for the subject; `tally(name [, year])`;
  `open(^code)`, a claim's open amount; `peak(x, window)` and `low(x, window)`, the
  highest and lowest value in a window; `days(COND, window)`, the days a condition
  held (`days(self.lives is foreign, …)`); `min max abs`; `progressive(schedule,
  x)`; `straight-line(cost, life, from, period [, mid-month])`; `value(x, UNIT [at
  POLICY])`; `date(y, m, d)`; `remaining`.

## 9. Parcels, lots, assets and gains

Money is held as parcels `(quantity, basis, acquired, transaction, tie)`. Parcels
that agree on all of these merge; base-currency money at its face value, tied to
nothing, is plain and always one parcel.

- **Arrival** from a party or `?` creates a parcel. Its basis is its cost: face
  value in the owner's currency, or `P × quantity` for an `@ P` price, or what the
  exchange gave. An account kind that says `basis zero` gives none (pre-tax
  deferrals). `basis AMOUNT` overrides both. It is tied to the paying party when
  that party's kind is `restricted`, and to the `for` entity when the flow names
  one.
- **Transfers** between an owner's holdings move parcels unchanged.
- **Relief** chooses which parcels leave: ties first, then the policy (the
  selector's, the account's, its kind's, then the commodity kind's; currencies are
  FIFO). Parcels that differ with no policy are ambiguous: an error listing each
  candidate and its gain, while quantities move FIFO.
- **Realization** happens when parcels change commodity, leave the owners for a
  party, or leave a `deferred` account for one that is not. `gain = proceeds −
  basis`, in the owner's currency at its system's rates, and `on gain` fires per
  parcel.
- **Revaluation.** Flows with the market (`via market` on a value) change what an
  account holds without realizing: growth arrives with no basis, a loss shrinks
  parcels and keeps their basis.
- **Splits** scale quantities and keep basis.

**Assets** are identified things, and the flows about them say so by their
purpose's object:

- `#purchase of ASSET` acquires it: the flow's cost (with its costs of exchange and
  its sales tax) is its first *part*.
- A purpose whose root is `capital` and that takes an object adds a part:
  `#improvement of condo`. A part has a cost, a day, and a basis laws may `consume`.
- `#repair of` an asset is spent: it adds no part. Rent, insurance and interest `of`
  an asset are income or spending about it; measures `of` it are its use.
- `#sale of ASSET` on money arriving disposes of it: every part is relieved, and the
  gain is the proceeds, less the sale's costs, less the parts' basis. `DATE ASSET
  ends` disposes of it for nothing.

The asset's `cost` is the sum of its parts' costs, its `basis` what remains of
them; laws of the asset's kind run for each part (each improvement depreciates on
its own schedule). `why ASSET` shows the parts, what consumed them, and every
event about the asset by purpose.

Restricted money is money held for someone: tied when it arrives from a restricted
party or `for` one (an envelope, a tenant's deposit). It is not available to spend;
its entity's `on spend` laws judge it when it leaves the owners; a flow `for` the
owner releases it.

## 10. Derived events

Derived events are computed during the run, never written into the journal. Each
names the line and the declaration it comes from, `why FILE:LINE` shows it, and an
editor shows it as a hint on that line.

| derived | from | what it is |
|---------|------|------------|
| a loan payment's interest and principal | the contract's `loan`, its resets and prepayments | `#interest` (of its asset) and the debt's decrease |
| an implied item or flow | `also` on a promise, party, kind or purpose | escrow, an employer's match, a card's cash back, sales tax collected, payroll taxes, a processor's fee |
| a share | `share SHARE for ENTITY`, `owner A 60%, B 40%`, `part of` | that share of each flow, of the same purpose, borne by an owner (an allocation) or owed by a party (a claim) |
| recognition | `covers`, `for PERIOD` | how a flow's amount spreads over days |
| a pro-rata refund | `covers` and `ends` | the unused part, owed by the party |
| an escalation | `rising`, `indexed to` | the amount from each anniversary |
| sales tax | a party kind's `sales-tax` | the tax inside a price paid to it (`#sales-tax`; part of a purchase's cost) |
| exchange cost | an exchange with a market price that day | what was given less what was got (`#exchange-cost`) |
| depreciation | `consume` in a law | a part's basis consumed |
| a wash sale | `carry` in a law | a loss moved into another parcel's basis |
| a missed occurrence | a contract | a claim, until the occurrence is written |
| a late fee | `due … else ITEM` | the item, when the deadline passes |
| a claim paid for | `for PARTY` on a payment | the party owes it |
| a repair | `require … else` | what the reparation owes |

A share is a flow of its own: the studio's 27.00 of a 45.00 phone bill is a flow
`#phone` owned by `studio`, and Sam's own phone is 18.00. Budgets, `flow` and the
tax see both; the bill as paid is one flow in `register`. A share for a party is
what it owes: `share 1/3 for ben, 1/3 for cleo` on a shared rent makes each
roommate's third a claim, and a flow `of` a building is divided among its parts by
area.

`also` is how a book says what always comes with something, once:

```text
kind card
  also issuer -> self 2% of amount #rebate                // cash back on every charge
kind processor
  also - 2.9% + 0.30 USD #fees                            // taken from every payout
purpose design
  also + 10.35% #sales-tax-collected for wa-dor when to.lives is us/wa
```

A written line that says the same thing replaces the derived one.

## 11. Dates, files, documents and returns

`axiom.ax` marks the project root; every `.ax` under it is loaded, and `systems/`
may add systems or override the embedded ones. How files are arranged is a
convention, never a law:

- A full date is right wherever it is written.
- A short date takes the rest from its context: the nearest heading above it in
  the file (`2026`, `2026-02`); else the file's path, where a folder or file named
  `YYYY` gives the year, and `MM` beneath it (a folder, or `MM.ax`), or a file
  `YYYY-MM.ax`, gives the month. In `journal/2026/01.ax`, `15 job` is enough.
- Nothing checks that a file's dates agree with its name. A short date with no
  context to complete it is an error whose fix is the full date.
- Sync writes each new line into the file its day belongs to by this convention.
- A file under `documents/` named after a code (`documents/inv-2026-01.pdf`)
  belongs to what the code names; one named `YYYY-MM-DD PARTY` to that day's flows
  with that party. `why` lists them, and `register` marks what has one.

**Returns.** `DATE SYSTEM filed YEAR` records a return as filed, with the tally
lines it reported:

```text
2026-04-15 us filed 2025
  wages           124_200.00 USD
  tax-withheld     11_952.00 USD
```

Once filed, a later edit that changes one of those lines is an *amendment*: `check`
lists each changed line, old and new, and the return to amend, instead of letting
the figures drift silently.

## 12. Diagnostics

Every diagnostic follows `tests/mistakes/REPORT.md` §14, and these:

1. **The headline states the fact in the book's words**: the names, amounts and
   days involved ("checking holds 3,015.80 USD, not 3,051.80 USD"), never a parser's
   or an index's.
2. **Labels point at causes.** The primary label is in the reader's file; related
   labels point at what made it so: the declaration of the rule, the promise that
   derived a flow, each flow a tally counted, the terms in force that day, the
   exception that did not apply. Built-in sources are marked and never primary.
3. **The fix is an edit** wherever one is mechanical, and each is a code action for
   an editor: the full date, the near name, `^` for a code written as a purpose,
   the conversion a unit mismatch needs, the change that would relax a budget this
   month, the `!` that accepts.
4. **One root cause is reported once**; what it breaks downstream is summarized.
5. **Derived events are explained where they come from**: a problem in a derived
   flow points at the line that caused it and the declaration that derived it.
6. **A failed balance shows its window**: a table of every flow since the last
   passing value (day, other end, amount, running balance, purpose or description),
   and names the likeliest cause among: a transposed digit; a flow entered twice,
   backwards or with the wrong sign; a flow of that size missing; a promise due and
   not written; a claim settled off the book; a derived flow written again by hand;
   a pending flow that settled; a flow the bank dated on the other side of the value.
7. **Unsolvable unknowns name their cycle**: `?` amounts are solvable when, with
   every end that has no value merged into one, they form no cycle; otherwise the
   error names the cycle.
8. **Everything inferred can be asked about**: `why FILE:LINE` says what a line means
   and where each part of that meaning came from.

`check --json` writes each diagnostic as one JSON object per line: its code,
severity, headline, labels (file, line, column, text), notes, and fixes as edits.

## 13. Command line

```text
axiom check     [--strict]                     diagnostics, late promises, a summary
axiom balance   [GLOB…] [--at DATE] [--value] [--monthly]   holdings, assets, claims
axiom register  ACCOUNT|OWNER|PARTY|ASSET|PROMISE [--from DATE] [--to DATE]
axiom flow      [--by month|year] [--by purpose|party] [--from DATE] [--to DATE]
axiom available [--at DATE]
axiom budget    [MONTH|YEAR]
axiom limits    [YEAR]
axiom claims    [--at DATE]                    what is owed either way, and whom it blames
axiom contracts                                every promise: terms, next due, kept, late
axiom tax       [YEAR]                         with what was filed, and what changed since
axiom gains     [YEAR]
axiom lots      [ACCOUNT] [--at DATE]
axiom forecast  [--until DATE] [--paths N]
axiom why       TARGET                         a name, #purpose, ^code, law, tax line, "text" or FILE:LINE
axiom sync      [NAME…] [--dry]                bring in what is new (§14)
axiom fmt       [FILE…] [--check]              lay files out in the house style
```

Global options: `--for ENTITY`, `--relaxed`, `--today DATE`, `--color
auto|always|never`, and `--json`, which writes any view as JSON: tallies and totals
as facts with an entity, a period (an instant or a duration), a unit and a value.

## 14. Sync

Sync is how a book stays current without being typed: bank and card statements,
invoices issued and bills received, prices, rates and the parameters laws read.
Axiom never opens a network connection; everything else is declared in the book.

```text
/// Chase's export of the checking account, dropped into imports/.
sync checking
  read    "imports/chase-checking-*.csv"
  format  csv
    date    "Posting Date" "MM/DD/YYYY"
    amount  "Amount"
    memo    "Description"
    balance "Balance"

sync visa
  read    "imports/chase-visa-*.qfx"
  format  ofx                                 // declared in std

sync prices
  run     quotes {units} --since {since}      // a script, where nothing else will do
  into    prices/{year}.ax
```

**Sources.** `read GLOB` reads files; `run COMMAND` runs a script and reads what it
prints, from the project root, with `{since}` (the day after the last record the
book has from this source), `{today}` and `{units}` (the commodities held). A
source named after an account is a **feed** of that account's records; `into PATH`
merges Axiom text into a file (`{year}` splits it by year); `into param NAME`
merges rows into a param; otherwise Axiom statements go into the journal (invoices,
bills). Systems may declare sources: `us` brings the IRS's yearly exchange rates
into the param its `rates` names.

On raw `run` and `into` lines, `//` starts a trailing comment only when a blank
comes before it. Thus `https://host/a//b` and `imports/a//b.ax` remain intact; a
space followed by `//` ends the raw text.

**Formats** are declarations. `csv` names columns (by header or number) for
`date` (with its layout), `amount` (money into the account is positive; `flipped`,
or `debit` and `credit`), `memo`, and optionally `balance`, `pending`, `code`,
`id` (rows sharing one are one flow, like the two sides of a conversion), `party`,
`gross` and `fee` (the fee is a `-` item `via` the source), `currency`, `category`
(mapped to purposes: `category "Groceries" is #groceries`), `object` (the purpose's
object), and `route` (which account a row belongs to, for an export of several
cards).
A tagged format names its records and fields:

```text
format ofx                                    // in std
  records STMTTRN
  date    DTPOSTED "YYYYMMDD"
  amount  TRNAMT
  memo    NAME, MEMO
  code    CHECKNUM
format camt053                                // ISO 20022 bank statements, in std
  records Ntry
  date    BookgDt/Dt
  amount  Amt, sign CdtDbtInd CRDT
  pending Sts PDNG
  memo    AddtlNtryInf, RmtInf/Ustrd
  code    NtryDtls/TxDtls/RmtInf/Strd/CdtrRefInf/Ref
  via     NtryDtls/TxDtls/RltdPties/UltmtCdtr/Nm
```

When a tagged format names more than one memo path, the reader joins the
non-empty values in declaration order with one space. Thus the CAMT memo above
keeps both `AddtlNtryInf` and `RmtInf/Ustrd`, just as a row format can join
multiple memo columns.

**Patterns** recognize memos. A pattern is a parsing expression, matched anywhere
in the memo, in any case:

```text
PATTERN := SEQ ( / SEQ )*          // ordered choice
SEQ     := ( [NAME:] ATOM [? | * | +] )+
ATOM    := STRING | digit | letter | space | alnum | any | rest | start | end
         | NAME | ( PATTERN )
```

```text
entity trader-joes : grocer
  known-as "TRADER JOE"
entity paypal : processor
  known-as "PAYPAL *" payee:rest              // the payee is recognized in turn: `via paypal`
code inv-*
  known-as "INV-" digit+ "-" digit+           // INV-2026-01 is ^inv-2026-01
pattern ach = "ACH " ("DEBIT" / "CREDIT") space+
```

A capture named `payee`, `code`, `amount`, `date` or `original` fills that part
of a record. `original` captures an amount together with its unit from the memo,
for example `original:rest` over `CHF 3,290.00`; sync may use that pair when it
reconciles a multi-currency flow. It does not add a second flow. Every entity
and account is known by its own name too (`ashgrove` matches "ASHGROVE",
`trader-joes` "TRADER JOES"), so only the ones a bank spells otherwise need a
pattern. The entity or account whose pattern matches the longest part of the
memo is the other end; two that tie are an error naming both.

A structured `code` is the canonical code as written in the book (an optional
leading `^` is accepted), with letter case ignored. A `code` rule's pattern
does not supply a prefix: it controls which memos can name the code, and the
code's `known-as` pattern recognizes its spelling. A structured code that names
an open claim selects that claim's party. Otherwise, a structured `party`, then
a structured `via`, then the recognized memo supply the other party, in that
order. `party` names who the transaction was with; `via` may name who the money
was for and is used as the other party when `party` is absent or unresolved. If
a structured field selects that party and the memo also identifies a different
party, the memo party is written as the intermediary (`via`). A memo tie does
not override an unambiguous structured party; an ambiguous structured value is
still an error.

**Each record** of a feed then goes through:

1. **Recognition** by the patterns, or by the record's own structured fields
   (a code for an open claim, `party`, or `via`), which win in that order.
2. **Reconciliation.** A record matches what the account already saw within three
   days, by amount: a whole flow, one leg of a split (a refinance's wire), the flows
   that share a code (a payroll batch), or a derived flow (a rebate). Matched
   records are already written. A flow typed from a receipt is not written twice,
   a transfer seen by two feeds is written once, and a document a source prints
   (an invoice, a payout with its fee) outranks the bank's line for the same money.
3. **Promises.** A record with a contract's party, within the grace of an
   occurrence due and unwritten, is written as that occurrence (`01 flat`, or `08
   phone 47.30 USD` when it differs).
4. **Claims.** A record from a party with open claims carries the codes it named;
   settlement follows §7.
5. **Writing.** Each new line goes, in day order, into the file its day belongs to
   (§11), in the house style of `axiom fmt`. A record no pattern recognizes goes to
   `?`, with its memo as its description. A `balance` belongs to its record; the
   latest posted, own-currency day with a balance is written as an assertion only
   when its reported balances imply one unambiguous day-end amount. Conflicting
   balances produce no assertion. Pending and foreign-currency rows do not
   determine that assertion. Tagged formats have no statement-level balance. A
   `pending` record is written in parentheses and settles when its posted record
   arrives.

**Rules.** Nothing already written is ever changed: sync only adds facts. What a
file or param already has (the same day and subject, the same row key) is kept as
written. A file is written only if the book still parses; `axiom sync --dry` prints
what would be written, as a diff. `check` groups the memos nothing recognized and
offers, for each, the `known-as` line that would.
