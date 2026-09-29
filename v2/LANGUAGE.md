# The Axiom language

This is the normative reference. [DESIGN.md](DESIGN.md) says why it is shaped
this way. Everything here is implemented, or is being implemented by the
current rework; the examples in `examples/` are its acceptance tests.

## 1. Lexical structure

Source is UTF-8, read line by line.

- **Items** start in column 0. Indented lines (spaces only; a tab in indentation is
  an error) belong to the nearest less-indented line above them. Nesting is by
  indentation, as in Python, so blocks are contiguous deeper lines.
- **Comments**: `//` to end of line, at line start or after whitespace.
  **Doc comments**: consecutive `///` lines document the item, leg, or law that
  follows them. They are shown by `why`. A law's doc comment has two parts: the
  first paragraph says what the law is, and a paragraph that starts `To fix:`
  says what to do when it fails. Diagnostics print the first as a note and the
  second as help.
- Blank lines separate nothing and mean nothing.

Tokens:

| token     | shape                                   | examples                        |
|-----------|-----------------------------------------|---------------------------------|
| date      | `YYYY-MM-DD` (validated)                | `2026-01-15`                    |
| month     | `YYYY-MM`                               | `2026-03` (selectors, bounds, periods) |
| number    | digits, `_` separators, optional `.frac`| `84.20`, `24_500`, `3`          |
| percent   | number immediately followed by `%`      | `10%`, `3.5%`                   |
| span      | `(digits [ymwd])+`                      | `59y6m`, `2w`, `60d`, `1y`      |
| name      | `[a-z0-9][a-z0-9_-]*`, `/`-separated, may contain `*` (glob) | `checking`, `assets/bank/checking`, `trader-joes`, `401k`, `expenses/food/*` |
| commodity | `[A-Z][A-Z0-9_.]*`                      | `USD`, `VTI`, `BRK.B`           |
| code      | `#[a-z0-9][a-z0-9_:./-]*`, may contain `*` | `#house`, `#check-1041`      |
| string    | `"…"` with `\" \\ \n \t`                | `"cash tips"`                   |
| punct     | `-> .. ... = == != < <= > >= + - * / @ ( ) [ ] , : ! ? . \|` | |

A token starting with a digit is a date, month, number, percent, or span if it
matches that shape exactly, and a name otherwise (`401k`). A four-digit number
where a period is expected is a year. Numbers carry no sign; `-` is an operator,
except before the amount of a balance assertion (`= -50 USD`). `a-b` is one name.
`a - b` is a subtraction. Case separates places and entities (lowercase) from
commodities (uppercase).

Keywords are not reserved: `for`, `due`, `basis`, `since`, `via`, `as`, `all`,
`split`, `empty` and the rest are recognized by position, so an account may be
called `basis`.

`empty` is the zero of every commodity. A bare `0` where an amount belongs is
an error, and the fix is to write `empty`. (`precision 0` is a number, not an
amount, and is fine.)

## 2. Transactions

```text
DATE [..DATE] FLOW
DATE PLAN [AMOUNT] (INDENT LEG)*              an occurrence of a named plan (§6)

FLOW   := SOURCE -> TARGET [@ PRICE] TAIL (INDENT LEG)*
SOURCE := [END [SELECTOR]] [AMOUNT | all [UNIT]]
TARGET := [END] [AMOUNT]
END    := PLACE | PLACE.basis
LEG    := END [SELECTOR] LEGAMOUNT [@ PRICE] TAIL
TAIL   := [/ PAYEE] CODE* [for WHAT] [due WHEN] [basis AMOUNT] [! [STRING]]
AMOUNT := NUMBER COMMODITY | (NUMBER COMMODITY) | ? COMMODITY | empty
LEGAMOUNT := AMOUNT | ... | = NUMBER COMMODITY | all [UNIT]
PLACE  := name | ?
SELECTOR := [ SEL (, SEL)* ]
SEL    := DATE | MONTH | YEAR | DATE..DATE | CODE | fifo | lifo | hifo | prorata
WHAT   := CODE | PERIOD | ENTITY
PERIOD := YEAR | MONTH | DATE | DATE..DATE
WHEN   := DATE | SPAN
```

The shapes, and what they mean:

```text
2026-01-18 checking -> food 84.20 USD                one flow
2026-01-18 visa -> food 84.20 USD / trader-joes      payee: a declared entity
2026-01-22 checking 2_000 USD -> brokerage 7 VTI     exchange: 2000 USD out, 7 VTI in
2026-01-22 checking -> brokerage 7 VTI @ 285.70 USD  exchange, price given, 1999.90 USD out
2026-09-02 brokerage[fifo] 10 VTI -> checking 3_050 USD
2026-09-02 brokerage[#house] all -> checking 52_000 USD
2026-05-01 old-broker all VXUS -> new-broker         every VXUS parcel, basis and dates kept
2026-02-01 checking -> plumber (350 USD) #check-1041 pending: parenthesised amount
2026-03-02 checking -> ? 40 USD                      destination unknown
2026-03-02 checking -> cash ? USD                    amount unknown, inferred
2026-01-01..2026-12-31 checking -> insurance 1_200 USD   paid now, recognized over the year
2026-01-15 checking -> taxes/federal 3_000 USD for 2025  paid now, belongs to 2025
2026-03-01 design -> acme 4_800 USD #inv-12 due 30d  an invoice: acme owes it within 30 days
2026-04-02 acme -> checking 4_800 USD for #inv-12    …and its payment
2026-09-06 checking -> savings 100 USD for car-fund  earmarked: tied to the `car-fund` envelope
2026-05-16 grandma -> college 3_000 USD basis 3_000 USD   a gift that arrives with its basis
2026-09-15 checking -> house.basis 14_200 USD / roofer    an improvement: raises the house's basis
```

**One side split.** When the header names only one place, the indented legs are
the other side. With no header amount on that side, its total is the sum of the
legs. The header may state both amounts when it names one place, so that an
exchange is written the way a closing statement reads:

```text
2026-01-15 acme -> 5_200 USD         legs are targets
  retirement     800 USD
  taxes/federal  910 USD
  checking       ...                  the remainder (at most one leg)

2026-02-01 -> landlord 1_800 USD     legs are sources
  checking       1_000 USD
  savings        ...

2026-12-29 house 1 HOME -> 431_500 USD   a sale: 1 HOME out, 431,500 USD in, allocated
  closing-costs  25_000 USD
  mortgage       276_282.05 USD
  checking       ...
```

A leg `= 5_000 USD` is a *target balance*: its amount is whatever makes that
place's balance equal to the target after the flow. If both header sides name a
place, the transaction has no legs. Many-to-many splits are an error, and the fix
is to write two transactions.

**Pairing rules.**

- *Same commodity* on both sides is a transfer. If both sides state an amount,
  they must be equal.
- *Different commodities* are an exchange. The price is `out / in`. If `@ P` is
  given together with both amounts and they disagree, the difference must be
  written as a leg. Axiom never books a difference silently. An `@` price may
  carry more decimals than its commodity's precision.
- In a split, legs in the header commodity take their stated amounts. A single
  leg in another commodity receives the remainder as its cost (a purchase with a
  fee leg). Two such legs need `@` prices.
- A leg into an expense place during an exchange is a cost of that exchange
  (e.g. a trading fee, a seller's costs): a sale realizes that much less, and
  what a purchase buys costs that much more (its basis takes it, unless the flow
  states a `basis`). It is still recorded as an expense. With several exchanges in
  one statement the costs are shared out by what each exchanged.

**Places and entities.** A place may be written as any unique suffix of its path,
or by its alias (§5). An *entity* in place position resolves to its `via` place
and becomes the payee. An entity that has the name wins over a place whose path
only ends with it (`lantern -> 1_440 USD` names the employer, not
`assets/owed/lantern`), and when the place is not the entity's own `via` the name
is reported once as ambiguous, offering the place its longer suffix; a full path
or an alias still names the place. An entity as the *source* of a payment out of
an asset place also says whose money it is: `car-fund -> car-repair 150 USD`
spends the parcels tied to `car-fund`, then untied money, and never another
entity's. `?` as a place is the built-in `equity/unknown`.
Undeclared places are an error, unless written as a full path under one of the
roots `assets`, `liabilities`, `income`, `expenses`, or `equity`, which opens
them; a full path that is a near miss of a declared place is an error with the
suggestion instead.

**The basis end.** `PLACE.basis` names the basis of what a place holds rather
than its quantity. A flow into it pays money and raises the basis of the place's
parcels (a capital improvement, a wash-sale adjustment); a flow out of it lowers
their basis and recognizes the amount at the target (depreciation). Quantities do
not change, so a house that `holds HOME` still takes an improvement, and the
money is never counted as held by the place. Selectors narrow which parcels:
`house[#roof].basis`. The basis is spread over the chosen parcels in proportion
to their quantity.

**Dates.** A transaction has the day money moves and the period it belongs to.
`DATE..DATE` pays on the first day and recognizes the flow evenly per day over
the range. `for PERIOD` says the same with the payment day kept: `for 2025`
recognizes the flow over 2025, `for 2026-03` over March, `for DATE..DATE` over
the range. Everything that reasons about a period (tallies, window totals,
budgets, `flow`, `tax`) reads the recognition; balances and relief read the
payment day. Folder layout (§10) constrains the payment day.

**Tail.** Header tails apply to every leg; a leg's own tail adds to it and
overrides it.

- `/ payee` names a declared entity. A leg's own payee, or the entity written as
  its place, is that leg's counterparty; a leg with neither takes the header's.
- `#codes` mark the transaction (a leg's codes mark that leg). They select lots
  later and link flows.
- `for` says what the flow is on account of:
  - `for #code` settles what `#code` marked: at the source it takes the parcels
    that flows marked `#code` left there (as the selector `[#code]` does), and it
    links the two. Paying an invoice, returning a deposit and reimbursing a bill
    are `for` the flow that created them.
  - `for PERIOD` sets the recognition period (see *Dates*).
  - `for ENTITY` ties the arriving parcels to that entity: they are held for it,
    they are not available to spend, and its `on spend` laws govern them.
    `for` the owner itself (`for me`) unties them. An envelope or a sinking fund
    is an entity of a restricted kind (§8).
- `due WHEN` makes the flow a *claim* due on a date, or a span after the payment
  day (`due 30d`). A leg's own `due` overrides the header's, so each leg of a split
  is a claim on its own debtor, due on its own day. A claim that is still open
  after its due day is reported by `check` and listed by `claims`.
- `basis AMOUNT` is the total basis the arriving parcels take, overriding the
  kind's arrival rule (§8): a gift into a 529 plan that is not a contribution
  of pre-tax money, a nondeductible IRA contribution.
- `!` waives every law violation this transaction raises, priced ones included;
  the waiver is reported, never hidden. A `!` that waives nothing is a warning.

## 3. Balance assertions, events, prices and splits

```text
2026-01-31 checking = 7_921.30 USD                balance assertion (end of day)
2026-01-31 checking = 7_921.30 USD !              …and accept any gap as unexplained
2026-03-31 retirement = 24_600 USD via market     …and post any gap to a place
2026-06-30 checking = -42.17 USD                  an overdrawn account
2026-12-31 visa = empty
2026-02-06 #check-1041 settled                    pending → actual on this day
2026-02-20 #check-1044 void                       pending → never happened
2026-03-04 #deposit-77 returned                   actual → reversed on this day
2026-01-02 VTI 280.14 USD                         price: 1 VTI = 280.14 USD on that day
2026-05-22 FAST split 2 for 1                     every FAST parcel doubles; basis and dates stay
```

An assertion is written the way a statement shows the balance: in the place's
display sign, so `visa = 1_234.56 USD` means 1,234.56 is owed. A failed
assertion is an error that shows the difference, the flows since the last
passing assertion, and the likeliest explanation (a transposed digit, a flow
entered backwards, a sign, a missing flow of that size). After a failure the gap
is carried: a later assertion that fails by the same amount is not reported
again.

- With `!`, the gap becomes an explicit flow from `equity/unknown`.
- With `via PLACE`, the gap becomes a flow from or to that place. Into a place
  of kind `market` it is a *revaluation* (§8): a statement that says what a
  401(k) is worth now, without inventing a withdrawal.

When exactly one flow with a `?` amount touches the place between two
assertions, its amount is inferred from them. An assertion that depends on a `?`
that could not be solved is not checked, and says so once.

A **split** changes how many units of a commodity exist: every parcel of `UNIT`,
in every place, is multiplied by `N / M` (`split 1 for 10` is a reverse split),
keeping its basis and acquisition day. Prices before the split stay in the old
units.

A settlement event must be dated on or after the flows it settles.

### Opening balances

```text
opening 2024-12-31
  checking      10_000 USD
  college       24_600 USD   basis 19_850 USD
  house         1 HOME       basis 540_000 USD   since 2023-06-15
  brokerage     40 VTI       basis 7_200 USD     since 2019-03-04
  brokerage     25 VTI       basis 6_100 USD     since 2021-11-20
  mortgage      412_428.22 USD
```

Each line creates holdings from `equity/opening`, in the place's display sign,
with an optional total `basis` and acquisition day (`since`; default: the
opening day). Several lines on one place make several lots. Openings are not
flows: no law sees them, they start no period, and they may be older than any
param. A book may have several `opening` blocks (one per account as it joins the
book).

## 4. Declarations

```text
base USD
use us/401k
relaxed                         // law violations become warnings
layout free                     // disable folder rules

account PATH [as ALIAS] [: KIND]    entity NAME[, NAME…] [: KIND]
commodity SYMBOL [: KIND]           kind NAME [: PARENT]
  PROPERTY ARG*                       (indented lines)
  law NAME                            (nested block)
```

`base` is required once the journal uses more than one currency. With one, that
currency is the base.

`account expenses/business as biz`: `biz` names the place wherever a place is
written, and an alias always wins over a suffix. When a new declaration makes a
suffix ambiguous, the error is reported once, at the declaration, with the lines
it affects counted.

`entity aldi, kroger, trader-joes : grocer` declares several entities of one kind
at once; with `kind grocer : org` carrying `via expenses/food/groceries`, that is
a whole payee table.

Property arguments are a space- or comma-separated list of primary expressions.
Properties are typed. The built-in ones are:

| on        | property                          | meaning                                          |
|-----------|-----------------------------------|--------------------------------------------------|
| account   | `owner ENTITY`                    | default `me`; a household may own a place        |
|           | `holds UNIT, …` / `holds any`     | commodities this place may hold                  |
|           | `select fifo\|lifo\|hifo\|prorata` | lot relief policy                               |
|           | `opened DATE` / `closed DATE`     | flows outside are errors                         |
|           | `budget AMOUNT monthly\|yearly`   | sugar for a `warn` law on inflows (§7)           |
|           | `liquidity SPAN`                  | time to turn into cash                           |
| entity    | `via PLACE`                       | the place used when the entity is a flow end     |
|           | `lives SYSTEM [from DATE] [until DATE]` | a residence; residences may overlap        |
|           | `member ENTITY`                   | this person belongs to that household            |
| commodity | `precision N`                     | decimal places (default: most seen in source)    |
|           | `name STRING`                     |                                                  |
|           | `liquidity SPAN`                  |                                                  |
|           | `grows PERCENT yearly`            | valuation model for forecasts                    |
| kind      | `restricted`                      | money from, or earmarked for, entities of this kind stays tied to them |
|           | `deferred`                        | places of this kind do not realize gains inside  |
|           | `basis zero\|cost`                | what basis arriving value takes (§8)             |
|           | `claim`                           | places of this kind hold what others owe (§8)    |
|           | `select POLICY`, `liquidity SPAN` | defaults for things of this kind                 |
|           | `has NAME TYPE`                   | declares a property for things of this kind      |

A kind's other property lines, `via` included, are defaults for its instances.
`TYPE` is one of `date amount number percent span text name entity place kind
unit bool`. An unknown property is an error with a suggestion.

Root kinds: `asset liability income expense equity` (places, one per class),
`commodity`, `entity`, and `market : income` (the counterpart of revaluations).
Built-in places: `equity/unknown` (written `?`), `equity/opening`, and
`income/market`. The `me` entity (kind `person`) always exists and owns every
place by default. A project sets its properties by declaring it: `entity me :
person` with `born`, `filing` and `lives` lines.

```text
code GLOB [GLOB…]         // e.g. `code trip-*`
  on PLACE-GLOB | KIND …  // codes matching GLOB may only mark flows touching these

param NAME
  KEY+ VALUE              // KEY: a year (step lookup: latest ≤), a date, or a name
  2026 single 0 USD 10% | 12_400 USD 12% | 50_400 USD 22%    // a schedule value

sync FILE
  run COMMAND…            // raw text to end of line, run by `axiom sync`
```

A header code applies to every leg; a code rule is satisfied when any leg of the
transaction touches one of its places.

## 5. Households and residence

A household is an entity of kind `household` (in `std`); people join it with
`member`:

```text
entity household : household
  filing joint
  lives  us/ca
entity alex : person
  born   1988-04-12
  member household
entity jordan : person
  born   1990-09-30
  member household
account assets/bank/joint : bank
  owner household
account assets/retirement/jordan-401k : 401k
  owner jordan
account income/jordan-salary : wages
  owner jordan
```

A household is governed as one: the top-level laws of the systems it lives in
govern it and every place it or its members own, with the household as `self`,
so a joint return reads one `tally(agi)` that both paychecks counted into.
A member keeps what is personal: the laws of the kinds of the places they own
(a 401(k)'s deferral limit, an early-withdrawal penalty) run with that person as
the owner, and count into that person's tallies. What they count is a line of the
household's year too: a limit reads only the person's own line, and the joint
return reads the household's, which both members counted into.

Residences may overlap and have ends: a citizen abroad `lives us` and
`lives de from 2025-07-01`; a move is `lives us/ca until 2025-06-30`. A path
includes its ancestors. A law of a system governs on the days its residence
covers, and an `each` law runs for every period its residence touched, so a
part-year resident is taxed for the part.

## 6. Plans

```text
every CADENCE [on DAY] [from DATE] [until DATE|MONTH] FLOW
plan NAME every CADENCE [on DAY] [from DATE] [until DATE|MONTH] FLOW
  // CADENCE: day | week | month | quarter | year | SPAN (2w, 3m)
  // DAY: 15 | 04-15 | monday … sunday

every month on 1 checking -> landlord 2_400 USD until 2027-06

/// Every other Friday.
plan paycheck every 2w from 2026-01-02 acme -> 5_200 USD
  retirement     800 USD
  taxes/federal  910 USD
  checking       ...
```

A plan exists only in forecasts until the journal says it happened. A named plan
is also a template: `2026-01-16 paycheck` in the journal is one occurrence,
written in full by the plan. An amount after the name replaces the header amount,
and indented legs replace the plan's legs of the same place:

```text
2026-01-16 paycheck
2026-01-30 paycheck
  taxes/federal  950 USD            // this stub withheld more; `...` absorbs it
2026-03-13 paycheck 5_900 USD       // a raise
```

The forecast knows which occurrences happened, projects the rest from the last
one, and never learns a recurrence from history that a plan already describes.
Day-of-month past the month's end clamps to the last day.

## 7. Laws

```text
/// Doc comment: what the law is.
///
/// To fix: what to do when it fails.
law NAME
  TRIGGER
  when EXPR                        filter: stop silently if false
  let NAME = EXPR
  require EXPR [else EFFECT] [STRING]
  warn EXPR [STRING]
  owe EXPR to ENTITY [by EXPR] [as NAME]
  count EXPR as NAME
```

Steps run top to bottom. Triggers:

| trigger       | fires                                                        | context                              |
|---------------|--------------------------------------------------------------|--------------------------------------|
| `on in`       | value arrives in a governed place (or its subtree)           | `amount from to payee date self owner` |
| `on out`      | value leaves a governed place                                | same                                 |
| `on gain`     | parcels leaving a governed place realize a gain              | `gain proceeds basis held amount from to date self owner` |
| `on spend`    | money tied to a restricted entity leaves its owner's places  | `amount from to payee date self`     |
| `each month`, `each year` | a period of the governed thing ends              | `date year month self owner`         |
| `each year closing MM-DD` | …or closes, on that day of the next year         | same                                 |
| `by EXPR`     | the journal reaches that date                                | `date self owner`                    |
| `always`      | after any change to a governed place                         | `balance date self owner`            |

`on in from X`, `on out to X` and `on gain from X` are the trigger with
`when from is X` (or `to`) as its first step. `X` may be a list:
`on in from wages | bonus`.

Every context also has `year` and `month` (of the recognition period's start,
§2), `date` (the day it happens), and `flow` (the triggering flow, for
`flow is #code`) where a flow triggered the law.

An `each year closing 04-15` law runs for 2025 on 2026-04-15, and what the
journal recognizes `for 2025` up to that day counts: a fourth estimated tax
payment made in January is part of the year it pays for.

Within a day, everything that happens comes before what closes the day: a payment
dated on a closing day counts, wherever the journal writes it, and so does a
planned or hypothetical flow of that day (`available` on the last day of a year
prices a withdrawal with the year-end tax it makes). The flows of one day run in
the order the journal writes them, so a law that reads what has been counted so
far sees only what came before its flow: the half month of depreciation on the
day of a sale is written before the sale.

*Governed*:

- A law in a place kind governs every place of that kind.
- A law in an account governs that account and its subtree.
- A law in an entity kind governs entities of that kind (`on spend`, `by`).
- A top-level law in a jurisdiction governs the entities living there (a
  household as one, §5) and every place they own. A top-level law in a project
  file governs every place in the book.

`self` is the governed thing: the place for place and kind laws, the entity for
entity-kind laws, and the resident (a person or household) for top-level laws.
`owner` is the governed thing's owner (an entity is its own owner).

**Order.** A law that reads `tally(x)` runs after every law that counts into `x`
on the same occasion, whichever file either is written in; a cycle is an error
naming both laws. Otherwise laws run in declaration order: systems before the
project, files in path order. So a project adds an itemized deduction by
counting into the tally the system reads, without copying the system.

Effects:

- `require` fails with an error (a warning when relaxed or waived) unless an
  `else` effect prices the violation. A priced violation is reported by `check`
  as `priced`, with what is owed.
- `warn` is a warning. Budgets are warnings.
- A violated law is reported once per subject and window (the month or year of
  the total it reads, or until it holds again for `always`), at the flow that
  crossed the line. A window that value was recognized into ahead of time (a
  premium paid in December `for` the next year, a cost spread over months) is
  read as it opens: its headroom counts that value, and a limit it breaks alone
  is reported once, on the window's first day.
- `owe` creates an obligation from `self`'s owner to an entity, due by a date
  (default: the flow's date). It is named for reports.
- `count` adds to a named tally, keyed by owner and year: one namespace per
  person- (or household-) year, so a tally is a line on that year. A per-person
  limit is a count followed by a `require` on the tally.
- `tally(NAME)` reads that line for the owner in the current year, and
  `tally(NAME, YEAR)` the line of another (a year, or a date in it):
  `tally(loss-carried, year - 1)` is what the year before handed on. A reading of
  another year is settled, so it has no headroom; the law still runs after every
  law that counts into the tally.

Every `require` and `warn` that compares two amounts records its *headroom*:
what was counted and what it is compared against, per subject and window. That
is what `axiom limits`, `budget` and `why` show before anything breaks.

`budget 650 USD monthly` on an account is the law
`on in` + `warn total(in, month) <= 650 USD "over budget"`, named `budget`.

### Expressions

Operators, from loosest to tightest binding:

```text
or        and        not
== != < <= > >= is
+ -       * /        unary -
postfix: .field   [key, …] (param lookup)   (args) (call)
atoms:   24_500 USD  10%  2026-04-15  59y6m  "text"  empty  name  UNIT  #code
         ( EXPR )  if EXPR then EXPR else EXPR
```

Types: `amount number bool date span text place entity kind unit schedule`.
Numbers are exact rationals. `amount * number` rounds half-to-even to the
commodity's precision. `amount / amount` (same commodity) is a number.
Comparing amounts in different commodities values the left side in the right
side's commodity on the flow's date, or fails with a missing-price error.
`empty` is the zero of any amount.

- `x is K` tests a place, entity or commodity against a kind (inherited),
  against a place or entity (the same or a descendant), or against a glob.
  `flow is #code` tests codes.
- Fields: `.balance .basis .owner .kind` on places, `.owner .kind .age` on
  entities (`age` is a span, from `born` to the context date), `.unit` on
  amounts, `.year .month` on dates, plus any declared property.
- Functions:
  - `total(in|out, month|year|ever)`: governed-subtree flow total recognized in
    this window, including the current flow.
  - `tally(name[, year])`, `min(a, b)`, `max(a, b)`, `abs(a)`.
  - `progressive(schedule, x)`: tax on `x` under marginal brackets.
  - `value(x, UNIT)`, `date(y, m, d)`.
  - `remaining`: money still tied to `self`, a restricted entity.
- Params: `limit[year]`, `ordinary[year, owner.filing]`. A bare param name is
  the row in force on the law's day.

## 8. Parcels, lots, and gains

Every asset place holds parcels `(quantity, basis, acquired, transaction, tie)`.
Parcels with equal attributes merge. Base-currency money whose basis equals its
face value and that is tied to nothing is *plain*, and plain money is one
parcel. In a place of a `claim` kind, parcels are also told apart by the
transaction that made them, so each invoice or loan stays its own claim.

- **Arrival** from an income, equity, liability or `?` place creates a parcel.
  Its basis is set by the target place's kind: `basis cost` (the default) is the
  face value in the base currency, or `P × quantity` with an `@ P` price;
  `basis zero` is nothing (pre-tax 401(k) deferrals, deducted IRA
  contributions). A `basis` tail overrides both. It is tied to the source
  entity if that entity's kind is `restricted`, and to the `for` entity if the
  flow names one.
- **Transfer** between asset places moves parcels unchanged: basis, acquired date,
  and ties all travel with them, unless the flow says `for` (which re-ties).
- **Relief** is choosing which parcels leave. Ties go first, then the lot
  policy. Selectors (`[2024]`, `[#house]`, `[2026-01-22]`) and `for #code`
  restrict the candidates. The policy comes from the selector, then the place
  (its own, or its kind's), then the commodity's kind chain. `std` says
  `select fifo` once, on `currency`, so a currency is FIFO unless something
  nearer says otherwise. If parcels differ and no policy applies, the flow is
  *ambiguous*. The error lists every candidate with the gain each would realize,
  and quantities still move FIFO so everything downstream stays consistent.
- **Realization** happens when parcels change commodity, leave the owner's asset
  places, or leave a `deferred` place for a non-deferred one. Then
  `gain = proceeds − basis`, and `on gain` laws fire, one per parcel relieved.
  Parcels created by the exchange take `basis = basis(given) + gain realized`,
  so inside a deferred place basis carries over.
- **Revaluation.** A flow between an asset place and a `market` place changes
  what the asset place holds without realizing anything: growth arrives with no
  basis (an unrealized gain), and a loss shrinks the parcels' quantities and
  keeps their basis (an unrealized loss).
- **Basis flows** (`PLACE.basis`) change basis and nothing else (§2).
- **Splits** scale quantities and keep basis (§3).
- Liabilities, income, expenses, and equity hold plain balances.

Restricted money is money held for someone. It is tied when it arrives from a
restricted entity (a grant) or when a flow earmarks it `for` one (an envelope, a
sinking fund, a tenant's deposit). It is not available to spend. When it leaves
its owner's places, that entity's `on spend` laws judge the flow. A flow `for`
the owner releases it.

## 9. Claims

A claim is value someone owes: an invoice, a loan to a friend, a deposit paid,
a reimbursement due, an IOU. It is a parcel in a place of a `claim` kind
(`receivable` in `std`), created by the flow that made it and settled by flows
`for` it:

```text
account assets/owed/clients : receivable
entity acme : org
  via assets/owed/clients

2026-03-01 design -> acme 4_800 USD #inv-12 due 30d
2026-04-02 acme -> checking 3_000 USD for #inv-12       partial
2026-04-20 acme -> checking 1_800 USD for #inv-12       settled
```

A claim's age is its parcel's acquisition day, its counterparty the payee (or
the entity written in place position), and its due day the `due` of the flow
that made it: each leg of a split is a claim of its own. `axiom claims` lists
what is open, `check` warns on what is past due, `available` counts claims as
coming in (never as money to spend), and `forecast` expects them on their due
days. What you owe others is the same thing seen from the other side: a place of
kind `payable` (a liability) whose flows carry `due`.

## 10. Projects and layout

`axiom.ax` marks the project root. Every `.ax` file under it is loaded. The
standard systems are embedded, and `systems/` in the project may add or override
them. Folder names are constraints unless `layout free` is set:

- a path segment `YYYY` or a filename `YYYY.ax` restricts dated items in that
  file to that year;
- a following `MM` segment or `YYYY-MM.ax` filename restricts them to that month;
- files under `prices/` may contain only prices;
- files under `systems/` must be systems.

## 11. Diagnostics

Every diagnostic follows the style guide in `tests/mistakes/REPORT.md` §14:

- The headline states the accounting fact ("checking would go to −1,000.00 USD").
- The primary label sits in the user's file. Built-in sources are marked
  `(built in)` and are never the primary.
- A help is an edit where the fix is mechanical.
- One root cause is reported once: later errors it causes are counted, not
  printed.
- Nothing the user wrote is silently reinterpreted. That covers an unknown
  commodity near a known one, a mistyped full path, a backwards range, and an
  event dated before its flow.

## 12. Command line

```text
axiom check     [PATH]                            diagnostics, priced violations, a summary
axiom balance   [GLOB…] [--at DATE] [--value] [--monthly]
axiom register  PLACE [--from DATE] [--to DATE]
axiom flow      [--by month|year] [--from DATE] [--to DATE]   income and spending
axiom available [--at DATE]                       what you can spend, and what it costs to get more
axiom budget    [MONTH|YEAR]
axiom limits    [YEAR]                            every cap and budget: counted, limit, room left
axiom claims    [--at DATE]                       what is owed to you and by you, and how old
axiom tax       [YEAR]
axiom gains     [YEAR]                            each disposal: acquired, sold, proceeds, basis, gain
axiom lots      [PLACE] [--at DATE]
axiom forecast  [--until DATE] [--paths N]
axiom why       TARGET                            a place, entity, system, #code, law, tax line, or file:line
axiom sync      [FILE…]
```

Global options: `--for ENTITY` (whose money: default everything; a household
includes its members), `--relaxed`, `--today DATE`, `--color auto|always|never`.
A report runs even when the book has errors; it says so in its header.
