# The Axiom language

This is the normative reference. [README.md](README.md) is the tour.

## 1. Lexical structure

Source is UTF-8, read line by line.

- **Items** start in column 0. Indented lines (spaces only; a tab in indentation is
  an error) belong to the nearest less-indented line above them. Nesting is by
  indentation, as in Python, so blocks are contiguous deeper lines.
- **Comments**: `//` to end of line, at line start or after whitespace.
  **Doc comments**: consecutive `///` lines document the item, leg, or law that
  follows them. They are shown by `why`, and a law's doc comment explains its
  diagnostics.
- Blank lines separate nothing and mean nothing.

Tokens:

| token     | shape                                   | examples                        |
|-----------|-----------------------------------------|---------------------------------|
| date      | `YYYY-MM-DD` (validated)                | `2026-01-15`                    |
| month     | `YYYY-MM`                               | `2026-03` (selectors, bounds)   |
| number    | digits, `_` separators, optional `.frac`| `84.20`, `24_500`, `3`          |
| percent   | number immediately followed by `%`      | `10%`, `3.5%`                   |
| span      | `(digits [ymwd])+`                      | `59y6m`, `2w`, `60d`, `1y`      |
| name      | `[a-z0-9][a-z0-9_-]*`, `/`-separated, may contain `*` (glob) | `checking`, `assets/bank/checking`, `trader-joes`, `401k`, `expenses/food/*` |
| commodity | `[A-Z][A-Z0-9_.]*`                      | `USD`, `VTI`, `BRK.B`           |
| code      | `#[a-z0-9][a-z0-9_:./-]*`, may contain `*` | `#house`, `#check-1041`      |
| string    | `"…"` with `\" \\ \n \t`                | `"cash tips"`                   |
| punct     | `-> .. ... = == != < <= > >= + - * / @ ( ) [ ] , : ! ? . \|` | |

A token starting with a digit is a date, month, number, percent, or span if it
matches that shape exactly, and a name otherwise (`401k`). Numbers carry no
sign; `-` is always an operator. `a-b` is one name. `a - b` is a subtraction.
Case separates places and entities (lowercase) from commodities (uppercase).

`empty` is the zero of every commodity. A bare `0` with no commodity is an
error, and the fix is to write `empty`.

## 2. Transactions

```text
DATE [..DATE] FLOW
FLOW   := SOURCE -> TARGET [@ PRICE] [/ PAYEE] CODE* [! [STRING]]
          (INDENT LEG)*
SOURCE := [PLACE [SELECTOR]] [AMOUNT]
TARGET := [PLACE] [AMOUNT]
LEG    := PLACE [SELECTOR] LEGAMOUNT [@ PRICE] CODE* [! [STRING]]
AMOUNT := NUMBER COMMODITY | (NUMBER COMMODITY) | ? COMMODITY | empty | all
LEGAMOUNT := AMOUNT | ... | = NUMBER COMMODITY
PLACE  := name | ?
SELECTOR := [ SEL (, SEL)* ]
SEL    := DATE | MONTH | YEAR | DATE..DATE | CODE | fifo | lifo | hifo | prorata
```

The shapes, and what they mean:

```text
2026-01-18 checking -> food 84.20 USD                one flow
2026-01-18 visa -> food 84.20 USD / trader-joes      payee: a declared entity
2026-01-22 checking 2_000 USD -> brokerage 7 VTI     exchange: 2000 USD out, 7 VTI in
2026-01-22 checking -> brokerage 7 VTI @ 285.70 USD  exchange, price given, 1999.90 USD out
2026-09-02 brokerage[fifo] 10 VTI -> checking 3_050 USD
2026-09-02 brokerage[#house] all -> checking 52_000 USD
2026-02-01 checking -> plumber (350 USD) #check-1041 pending: parenthesised amount
2026-03-02 checking -> ? 40 USD                      destination unknown
2026-03-02 checking -> cash ? USD                    amount unknown, inferred
2026-01-01..2026-12-31 checking -> insurance 1_200 USD   spread over the range
```

**One side split.** When the header names only one place, the indented legs are
the other side. The header amount is the total. With no header amount, the total
is the sum of the legs.

```text
2026-01-15 acme -> 5_200 USD         legs are targets
  retirement     800 USD
  taxes/federal  910 USD
  checking       ...                  the remainder (at most one leg)

2026-02-01 -> landlord 1_800 USD     legs are sources
  checking       1_000 USD
  savings        ...
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
  written as a leg. Axiom never books a difference silently.
- In a split, legs in the header commodity take their stated amounts. A single
  leg in another commodity receives the remainder as its cost (a purchase with a
  fee leg). Two such legs need `@` prices.
- A leg into an expense place during an exchange is a cost of that exchange
  (e.g. a trading fee). It is still recorded as an expense.

**Places and entities.** A place may be written as any unique suffix of its path.
An *entity* in place position resolves to its `via` place and becomes the payee.
`?` as a place is the built-in `unknown` place. Undeclared places are an error,
unless written as a full path under one of the roots `assets`, `liabilities`,
`income`, `expenses`, or `equity`, which opens them. The root is the class.

**Dates.** `DATE..DATE` spreads the arrival over the range: the source pays on the
first day, and the target is recognized linearly per day. Folder layout (§8) may
constrain dates.

**Tail.** `/ payee` must name a declared entity. `#codes` mark the transaction
(and a leg's codes mark that leg's flows). They link events and select lots later.
`!` waives every law violation raised by this transaction; the waiver is reported,
never hidden.

## 3. Other journal items

```text
2026-01-31 checking = 7_921.30 USD       balance assertion (end of day)
2026-01-31 checking = 7_921.30 USD !     …and accept any gap as unexplained
2026-12-31 visa = empty
2026-02-06 #check-1041 settled           pending → actual on this day
2026-02-20 #check-1044 void              pending → never happened
2026-03-04 #deposit-77 returned          actual → reversed on this day
2026-01-02 VTI 280.14 USD                price: 1 VTI = 280.14 USD on that day
```

A failed assertion is an error that shows the difference and the flows since the
last passing assertion. With `!`, the gap becomes an explicit flow from `unknown`.
When exactly one flow with a `?` amount touches the place between two
assertions, its amount is inferred from them.

## 4. Declarations

```text
base USD
use us/401k
relaxed                         // law violations become warnings
layout free                     // disable folder rules

account PATH [: KIND]           entity PATH [: KIND]
commodity SYMBOL [: KIND]       kind NAME [: PARENT]
  PROPERTY ARG*                   (indented lines)
  law NAME                        (nested block)
```

Property arguments are a space- or comma-separated list of primary expressions.
Properties are typed. The built-in ones are:

| on        | property                          | meaning                                          |
|-----------|-----------------------------------|--------------------------------------------------|
| account   | `owner ENTITY`                    | default `me`                                     |
|           | `holds UNIT, …` / `holds any`     | commodities this place may hold                  |
|           | `select fifo\|lifo\|hifo\|prorata` | lot relief policy                               |
|           | `opened DATE` / `closed DATE`     | flows outside are errors                         |
|           | `budget AMOUNT monthly\|yearly`   | sugar for a `warn` law on inflows                |
|           | `liquidity SPAN`                  | time to turn into cash                           |
| entity    | `via PLACE`                       | the place used when the entity is a flow end     |
|           | `lives SYSTEM [from DATE]`        | jurisdiction (repeatable, dated)                 |
| commodity | `precision N`                     | decimal places (default: most seen in source)    |
|           | `name STRING`                     |                                                  |
|           | `liquidity SPAN`                  |                                                  |
|           | `grows PERCENT yearly`            | valuation model for forecasts                    |
| kind      | `restricted`                      | money from entities of this kind stays tied to them |
|           | `deferred`                        | places of this kind do not realize gains inside  |
|           | `select POLICY`, `liquidity SPAN` | defaults for things of this kind                 |
|           | `has NAME TYPE`                   | declares a property for things of this kind      |

A kind's other property lines are defaults for its instances. `TYPE` is one of
`date amount number percent span text name entity place kind unit bool`. An
unknown property is an error with a suggestion.

Root kinds: `asset liability income expense equity` (places, one per class),
`commodity`, `entity`. The `me` entity (kind `person`) always exists and owns
every place by default.

```text
code GLOB                 // e.g. `code trip-*`
  on PLACE-GLOB | KIND    // codes matching GLOB may only mark flows touching these

param NAME
  KEY+ VALUE              // KEY: a year (step lookup: latest ≤), a date, or a name
  2026 single 0 USD 10% | 12_400 USD 12% | 50_400 USD 22%    // a schedule value

every CADENCE [on DAY] [from DATE] [until DATE|MONTH] FLOW
  // CADENCE: day | week | month | quarter | year | SPAN (2w, 3m)
  // DAY: 15 | 04-15 | monday … sunday
every month on 1 checking -> landlord 2_400 USD until 2027-06

sync FILE
  run COMMAND…            // raw text to end of line, run by `axiom sync`
```

Plans (`every`) exist only in forecasts. Day-of-month past the month's end clamps
to the last day.

## 5. Systems

A file whose first item is `system PATH` is a system. It may `use` others, and
declare kinds, entities, params, codes and laws. A path inherits its ancestors:
`us/ca/san-francisco` includes `us/ca` and `us`. `use X` brings a system's kinds
into scope. `lives X` on an entity puts it under X's top-level laws.

Name resolution for kinds: `401k` resolves if exactly one used system declares
it, and `us/401k` always resolves.

## 6. Laws

```text
/// Doc comment: the explanation shown when this law fails.
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
| `by EXPR`     | the journal reaches that date                                | `date self owner`                    |
| `always`      | after any change to a governed place                         | `balance date self owner`            |

*Governed*:

- A law in a place kind governs every place of that kind.
- A law in an account governs that account and its subtree.
- A law in an entity kind governs entities of that kind (`on spend`, `by`).
- A top-level law in a jurisdiction governs the entities living there and every
  place they own.

In `each` and `by` laws, `self` is the governed thing. In top-level laws it is
the resident entity.

Effects:

- `require` fails with an error (a warning when relaxed or waived) unless an
  `else` effect prices the violation.
- `warn` is a warning. Budgets are warnings.
- `owe` creates an obligation from `self`'s owner to an entity, due by a date
  (default: the flow's date). It is named for reports.
- `count` adds to a named tally, keyed by owner, year, and system.
- `tally(NAME)` reads a tally in the current year, looking in the law's own
  system and then its ancestors.

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
- Fields: `.balance .owner .kind .age` (a span, from `born` to the context date),
  plus any declared property.
- Functions:
  - `total(in|out, month|year|ever)`: governed-subtree flow total in this
    window, including the current flow.
  - `tally(name)`, `min(a, b)`, `max(a, b)`, `abs(a)`.
  - `progressive(schedule, x)`: tax on `x` under marginal brackets.
  - `value(x, UNIT)`, `date(y, m, d)`.
  - `remaining`: money still tied to `self`, a restricted entity.
- Params: `limit[year]`, `ordinary[year, owner.filing]`.

## 7. Parcels, lots, and gains

Every asset place holds parcels `(quantity, basis, acquired, transaction, tied-to)`.
Parcels with equal attributes merge. Base-currency money whose basis equals its
face value is plain, and plain money is one parcel.

- **Arrival** from an income, equity, or `?` place creates a parcel. Its basis is
  its face value in the base currency, or zero if the target place's kind is
  `deferred`. An `@` price makes the basis the price times the quantity. It is
  tied to the source entity if that entity's kind is `restricted`.
- **Transfer** between asset places moves parcels unchanged: basis, acquired date,
  and ties all travel with them.
- **Relief** is choosing which parcels leave. Ties go first, then the lot
  policy. Selectors (`[2024]`, `[#house]`, `[2026-01-22]`) restrict the
  candidates. The policy comes from the selector, then the place, then the kind
  chain. If parcels differ and no policy applies, the flow is *ambiguous*. The
  error lists every candidate with the gain each would realize, and quantities
  still move FIFO so everything downstream stays consistent.
- **Realization** happens when parcels change commodity, leave the owner's asset
  places, or leave a `deferred` place for a non-deferred one. Then
  `gain = proceeds − basis`, and `on gain` laws fire, one per parcel relieved.
  Parcels created by the exchange take `basis = basis(given) + gain realized`,
  so inside a deferred place basis carries over.
- Liabilities, income, expenses, and equity hold plain balances.

## 8. Projects and layout

`axiom.ax` marks the project root. Every `.ax` file under it is loaded. The
standard systems are embedded, and `systems/` in the project may add or override
them. Folder names are constraints unless `layout free` is set:

- a path segment `YYYY` or a filename `YYYY.ax` restricts dated items in that
  file to that year;
- a following `MM` segment or `YYYY-MM.ax` filename restricts them to that month;
- files under `prices/` may contain only prices;
- files under `systems/` must be systems.

## 9. Command line

```text
axiom check     [PATH]                            diagnostics and a summary
axiom balance   [GLOB…] [--at DATE] [--value] [--monthly]
axiom register  PLACE [--from DATE] [--to DATE]
axiom flow      [--by month|year] [--from DATE] [--to DATE]   income and spending
axiom available [--at DATE]                       what you can spend, and what it costs to get more
axiom budget    [MONTH]
axiom tax       [YEAR] [--entity E]
axiom lots      [PLACE]
axiom forecast  [--until DATE] [--paths N]
axiom why       TARGET                            a place, #code, law, tax line, or file:line
axiom sync      [FILE…]
```

Global options: `--relaxed`, `--today DATE`, `--color auto|always|never`.
