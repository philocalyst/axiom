# L1 map: what a journal line costs today, what the junction table is, and what an upgrade can know

Written before the first code change of lane L1, from the code at `25566a5`, and checked against what the code does (the
last section says how). Paths are in `crates/`; line numbers are those of `25566a5`. The vocabulary of the tree is K4a's
(`Group`, `Leg`, `Item`) and K3b's (an account may be written as an address, `jordan/bluefin/401k`).

The brief says one production, a verb table, `<-` and `@`, arrows on legs, and a `fmt --upgrade` that works "mechanically
from the AST". **The AST alone cannot say which end of a line is the book's own** (section 5), and the model cannot say
which junction a line was written with (and must not). That splits the lane along a line the brief draws only
implicitly: the syntax crate knows the *spelling*, the CLI (which owns a `Book`) knows the *sides*. Section 0 lists what
else the code contradicts.

## 0. What the brief says and what the code does

Eleven things in the brief, the proposal or the research do not match the code. Each decides something below.

1. **There are 3,343 flow headers in the examples, not 3,009**, and 489 of them (14.6%) flip to `<-`, not 314. The
   proposal's 2,387 and 314 were counted on the nine numbered examples before K3b, K6 and the `explore-v5` books landed.
   Counts in this map are of the tree at `25566a5`: the 11 files of `examples/` (`01` to `11`, 2,170 headers) and
   `explore-v5/` with `v4-sketch/` (1,173), from the AST and the book each project builds.
2. **The model reads a flow as `from`, `to`, two optional amounts and a tail; nothing else.** `lower_flows`
   (`model/lower/record.rs:326`) matches `(from.end, to.end)`; `make_flow` (`lower/flow.rs:244`) reads `from.amount` and
   `to.amount`. A `Junction` field the model does not read changes nothing it builds, so **the lowered book is the same by
   construction for every line whose normal form (section 3) is the v4 line's**, which is every line but the three in
   section 3.3. No STOP is needed for the lowering. One is needed for two diagnostics (section 9).
3. **`fidelity -> 1.62 VTI @ 297.00 USD` already lowers today**, as `fidelity 1.62 VTI -> fidelity @ 297.00 USD`: the model
   accepts a flow whose two ends are one place (`record.rs:243` writes `take(1 + usize::from(flow.from != flow.to))`), and
   `apply_price` (`lower/flow.rs:319`) fills the side that is not written. Checked on the baseline binary: `fidelity -> fidelity
   3 VTI @ 290 USD` and `fidelity 2 VTI -> fidelity @ 295 USD` post 870.00 USD out and 3 VTI in, 2 VTI out and 590.00 USD in. So
   the buy and the sell of the proposal's table are **desugaring**: one `End` written once is the end on both sides.
4. **The exchange "written with only a source" (`fidelity 20 VTI -> 5_940 USD`) is accepted by the parser and rejected by the
   model.** `check_shape` returns `Ok` for it (`flow.rs:389`), and `lower_flows` answers `flow-shape: a flow needs a named
   end and at least one leg` (`record.rs:353`). LANGUAGE §3 lists it as a sale; seven lines of the examples (`11-sam`,
   `explore-v5/04-nomad`) are errors today. The v5 sell (`fidelity -> 20 VTI @ 297 USD`) is the working spelling of
   what it meant, so upgrading them **fixes** them: the one place the lowered book is not the same, because the v4 line
   has none (section 3.3).
5. **A party-headed split has no subject in its text, and the model never reads one.** `acme -> 8_000 USD` with legs
   (`02-household` line 13) lowers to flows from `acme` to each leg; "the owner of a transaction" is `from_place.owner`
   or `to_place.owner` of each flow (`lower/flow.rs:395`). The proposal's `me <- acme 8_000 USD` names an owner the v4
   line does not. All 82 party-headed splits in the examples have exactly one owner among the accounts of their legs
   (`me` 80, `family` 2), and so has the one split into a party, so the owner is *derivable*, from the book (section 5).
6. **The amount-on-both-sides and the price are not the same thing the model checks.** With `@` and both amounts the model
   demands `expected == actual` after rounding half-to-even at the unit's scale (`apply_price`, `prices::rescale`). Of the 88
   lines in the examples that state two amounts, 2 are one unit twice, 50 carry a price, and **36 carry none** (29 with both
   ends named, 7 with the source alone): 15 whose quotient is an exact decimal and 21 whose is not
   (`etrade 13.2000 NWND -> irs 1_788.73 USD` is 135.5098... a share). A price that rounds to the stated amount gives the
   same book, and a price nobody wrote is a claim about a trade. Section 6 says what the upgrade does with them.
7. **A cross-account sale cannot be spelled by the proposal's table.** `fidelity 60 FAST -> checking 2_748.60 USD` sells out
   of one place and puts the proceeds into another (12 lines: 9 in `06-investor`, 3 in `02-household`; and 10 "sold to
   cover" lines whose proceeds go to a tax party). `S -> O 60 FAST @ P` keeps v4's meaning only with the amount at `O` (a *purchase*:
   `checking -> fidelity 7 VTI @ P` is in LANGUAGE §3 today), and the sale needs the amount at the source. The research
   (`research/REPORT.md` A10) drafted an `into` clause for it and the proposal did not adopt one. This lane keeps the one thing
   v4 already says: **an amount left of the arrow is the subject's** (`fidelity 60 FAST -> checking @ 45.81 USD`). It is
   valid v5, with no warning, and no `into` or `from` is built (section 3.2, decision D3).
8. **Nothing in the old syntax is "retired" by being unreadable.** Every old form (a bare leg, two amounts, `S X ->`) is
   what the one grammar already parses, plus a check that says it is old. An upgrade-only recogniser would need the same
   parse to build the AST it upgrades from, so it cannot be smaller than the courtesy; section 7 counts both.
9. **`Book` is not `Debug`, and the lane may not make it one.** `Txn`, `Assert`, `Event`, `Measure`, `Prices`, `Contract`,
   `Asset`, `Commodity` and `Filed` lack the derive, so the identity proof cannot dump the whole book. `Flow`,
   `Program`, `WrittenOccurrence`, `EndEvent`, `ClaimChange`, `Reading`, `Split`, `Detail`, `Select` have it, and
   `docs/v5/measure/internals/main.rs` already prints the flows, gains and holdings a fold makes. The proof is those, plus
   the JSON of **every** query over the book, byte for byte (section 8). Say if a `#[derive(Debug)]` on nine model types is
   welcome: it is a test-support change with no behaviour.
10. **`sync` and `session` write v4 lines.** `sync/write.rs` writes `acme -> checking 3_200 USD` (30 lines of format
    strings) and `session/transaction.rs` writes a typed flow as a `->` line. Both are valid v5 (a give whose subject is a
    party), neither is a legacy form, and the lane does not touch them. Said in the report, since a party-subject `->` is
    not something the parser can warn about (section 5).
11. **The examples are not formatter-clean today.** `axiom fmt --check` says every journal file of every example would
    change (973 lines of `05-family` alone). So `fmt --upgrade` rewrites the junction **and lays the file out**, and the
    diff of an example is its whole journal, once.

## 1. What a dated line costs today

A line that starts with a date is read by two productions that both begin by reading the first end, which is why one
pass tells them apart (`journal.rs:41`). What each costs, in lines of source (doc comments included):

| production | where | lines | what it does |
|---|---|---:|---|
| `journal_entry` | `journal.rs:15` | 28 | dispatch on the first token: `^code`, `#purpose`, a unit not followed by an arrow, else a transaction |
| `transaction` | `journal.rs:44` | 31 | `[..DATE] SIDE`, a priced left amount, then: arrow makes a flow, anything else a statement |
| `spread` | `journal.rs:85` | 13 | `DATE..DATE` |
| `statement` + `said` | `statement.rs:30,54` | 22 + 15 | a statement's header, tail and lines |
| `verb` + `word_verb` + `occurrence` | `statement.rs:70,98,117` | 26 + 19 + 18 | `=` or a word, else an occurrence |
| `flow_head` + `flow_legs` + `check_shape` | `flow.rs:20,31,382` | 14 + 5 + 19 | `-> SIDE TAIL`; the legs; one named side or both |
| `body` + `at_item` | `flow.rs:39,54` | 15 + 6 | legs and items: a leg names an end, an item starts with a sign, a number, a percent or a code |
| `side`, `arrow`, `expected_arrow`, `end`, `quantity` | `flow.rs:74-173` | 25 + 15 + 14 + 20 + 21 | the parts |
| `leg`, `line_item`, `item_body`, `item_with_end` | `flow.rs:176-234` | 19 + 11 + 12 + 15 | the lines under a header |
| `at_schedule` | `contract.rs:149` | 33 | a contract's line is a schedule: **four `lexer.clone()`s**, one per first token |
| `at_slot_line`, `cadence_follows_name`, `at_cadence` | `contract.rs:136,187,181` | 10 + 7 + 4 | two more clones |

So a flow is `transaction → flow_head → body → check_shape`, a statement is `transaction → statement → said → verb`, and
`statement` is entered from four places (`journal_entry` three times, `transaction` once; an opening's `owes` line calls
`said`, the half under it).
The table of what a statement's verb takes is two matches (`takes_lines`, `takes`) and one array (`VERBS`, used only for
the suggestion `closest`). **The verb is a match on a word, not a table** (`word_verb`, 12 arms): the brief wants data.

The formatter reads the same lines again, by tokens: `flow_header` (49 lines) and `statement_header` (50) build the same six
cells (`date, subject, verb, object, amount, tail`) from two different ASTs; `leg` (22) and `item` (21) build the others;
`columns` (54) and `items` (38) lay them out. Three of them are over the 40-line limit today (`flow_header` 49,
`statement_header` 50, `columns` 54).

Six `lexer.clone()`s total, all in `contract.rs`. They are not on the hot path (a contract's body is a few lines), and
`peek_second` (a cached second token, no clone) is what `journal.rs` and `flow.rs` use for the same job. What `at_schedule`
does is separate a schedule from an item, because both start with an amount: `45 USD monthly on 8 from visa` against
`32 USD #gifts`. **A keyword would remove the question; reading the amount and then looking at the next token does too,
with no new word in 36 contracts' bodies** (D4).

## 2. The verb table, and every case with the lines that show it

The word after the subject says what a dated line is. The table, as data (a `const` array keyed by the token, as `KEYWORDS`
is in `structure.rs`), has three kinds of entry:

| key | what it says | v4 or new | lines in the examples |
|---|---|---|---:|
| `->` | **give**: the subject pays (`checking -> trader-joes 84.20 USD`) | v4's arrow, unchanged | 2,649 |
| `<-` | **take**: the subject receives (`checking <- acme 5_750 USD`) | new | 0 today; 489 lines flip |
| `->`, no object, an amount, `@` | **sell**: the amount leaves the subject, the proceeds stay (`fidelity -> 1.62 VTI @ 297.00 USD`) | new | 0 (the 7 source-only exchanges become one) |
| `<-`, no object, an amount, `@` | **buy**: the money leaves the subject (`fidelity <- 7 VTI @ 285.70 USD`) | new | 0 |
| `->`/`<-`, no object, an amount, legs | **split**: the legs are the other side | v4's one-sided arrow | 29 |
| `<-`/`->`, an object, legs | **split through the subject**: an owner passes a party's money on (`me <- acme 12_000 USD`) | new | 83 |
| `=`, `owes`, `now`, `worked`, `used`, `waived`, `ends`, `settled`, `void`, `returned`, `split`, `basis`, `filed` | the statements | unchanged | 1,225 |
| none | an occurrence (`01 flat`, `08 phone 47.30 USD`) | unchanged | 621 |

What the 3,343 headers are, in the verbs they say (the first row is the lines whose text **does not change**):

| the line | v5 verb | lines | the example |
|---|---|---:|---|
| subject is an account, owner or position | give, as written | 2,649 | `visa -> trader-joes 84.20 USD` |
| subject is a party, object is the book's | take: flip | 460 | `acme -> checking 3_200 USD` |
| the same, the amount left of the arrow | take: flip, move the amount | 17 | `brightwave 3_200 USD -> business-checking #design` |
| the same, the subject does not resolve | take: flip | 12 | `interest -> us-savings 144.55 USD` (`08-expat`, a v3 book) |
| split from a party, arrow-less legs | split through an owner | 82 | `15 acme -> 8_000 USD` with five legs |
| split into a party | split through an owner | 1 | `14 -> bayside-honda 45_046.25 USD #purchase of car2` |
| split from an own account | split: arrows on the legs | 20 | `visa -> 85.05 USD #shared-2025-03` with three legs |
| `S X ->` and legs (a dangling arrow) | split: the amount after the arrow | 9 | `girokonto 900.00 EUR ->` |
| both amounts and a price | give/take with `@`: one amount | 50 | `checking 1_499.99 USD -> fidelity 5.5851 VTI @ 268.57 USD` |
| both amounts, no price, exact quotient | `@` price written | 10 | `fidelity 60 FAST -> checking 2_748.60 USD` (45.81) |
| both amounts, no price, inexact quotient | **refused** | 19 | `etrade 13.2000 NWND -> irs 1_788.73 USD` |
| both amounts, one unit | one amount | 2 | `coinbase 6 ETH -> wallet 6 ETH` |
| one amount and a price (v5 already) | as written | 5 | `checking -> brokerage 7 VTI @ 285.70 USD` |
| the source alone, both amounts, exact | sell or buy (**fixed**) | 5 | `fidelity-brokerage[2026-01-20] 1.620 VTI -> 481.14 USD` |
| the source alone, both amounts, inexact | **refused** | 2 | `wise-bal 3_000.00 EUR -> 3_247.01 USD` |

Of the first row, 9 say an amount that is `all`, `?` or `(pending)` (`fidelity all VXUS -> schwab`, `checking -> cash ? USD`)
and are never moved. The rows add to 3,343.

Under the headers: **legs** 386 (359 under journal splits, 27 in `explore-v5`), 93 under occurrences (`15 job`), 119 in
contract templates, plus the lines of 16 openings (a balance, not a flow: no arrow, ever); **items** 198 (33 in the
numbered examples). A leg is an indented line that names an end; an item names none.

## 3. The AST, and what each line lowers to

### 3.1 What is added

```rust
pub enum Junction { Out, In }      // `->`, `<-`: which way the arrow was written
Flow { from, to, tail, body, junction: Junction, through: Option<End> }
Leg  { doc, end, amount, tail, loc, arrow: Option<Junction> }     // None: an opening's line, or an old bare leg
```

`from` and `to` keep their meaning: what the model reads. **`<-` is `->` with the ends swapped**: the parser builds
`Flow { from: O, to: S }` from `S <- O`, so everything downstream of the parser is what it was. `through` is the one
thing with no home today: the owner that a split passes a party's money through (`me` in `me <- acme 8_000 USD`).
Nothing reads it in this lane (L2 will); the formatter and the upgrade do.

The action a line names (give, take, buy, sell, split) is **derived**, by one function over a `Flow`, and is not stored:
it is `(junction, whether an end is named opposite the subject, whether legs follow)`, and a field that repeated it
could disagree with the ends.

### 3.2 The normal form of each v5 line

The amount is where v4 puts it, for every line v4 could write; and the two new lines put it where the verb says.

| v5 text | `from` | `to` | `through` | v4 text with the same AST |
|---|---|---|---|---|
| `S -> O A tail` | `S` | `O`, `A` | | the same |
| `S A -> O tail` (the subject's amount) | `S`, `A` | `O` | | the same |
| `S <- O A tail` | `O` | `S`, `A` | | `O -> S A tail` |
| `S -> A @ P` (sell, no legs) | `S`, `A` | `S` (no selector) | | `S A -> S @ P` |
| `S <- A @ P` (buy, no legs) | `S` | `S`, `A` | | `S -> S A @ P` |
| `S -> A` + `->` legs | `S` | (`A`) | | `S -> A` + bare legs |
| `S <- A` + `<-` legs | | `S`, `A` | | `-> S A` + bare legs |
| `S <- O A` + `->` legs | `O` | (`A`) | `S` | `O -> A` + bare legs |
| `S -> O A` + `<-` legs | | `O`, `A` | `S` | `-> O A` + bare legs |

Two things the table settles. **A leg's arrow is the split's side**: `->` when the header's named end is the source (the
split is outward), `<-` when it is the destination, and a mismatch is an error naming the arrow to write. And **an
`S -> A` or `S <- A` with no legs and no `@` is an exchange without a price**, an error with the three words that fix it
(`@ PRICE`), decided after the lines under it are read, as `check_shape` already decides the other shapes.

### 3.3 Where the lowered book differs, and the only three places

1. A both-amounts line becomes one amount and a price: the model computes the dropped side, and the same flow follows
   when `round(A * P) == dropped`, which the upgrade checks with the model's own rounding before it writes.
2. The seven source-only exchanges are errors today and flows after.
3. A left amount that moves right, in a flipped take (17 lines), changes `from.amount` into `to.amount`. For a literal the
   model reads one amount as both sides (`stated_amounts`, `(Some(a), None) => (a.amount, a.amount, ...)` and the mirror),
   so the flow is the same; a `?`, an `all` or a pending amount is **never moved** (`resolve_quantity` takes the side as an
   argument, and `Infer` carries it). The upgrade leaves those lines alone.

The oracle (section 8) is what says this is true and not argued.

## 4. Legs and items

A line under a header is **an arrow-led leg, an item, or an old bare leg**. The first token says which:

```text
->  <-            a leg: `-> irs 692 USD #federal-tax`, `-> checking ...`, `<- acme 40 USD`
+  -  number  %  fraction  ^code  (    an item: `+ 4 USD #tip`, `32 USD #gifts`
a name, a unit, ?                       an old bare leg: read as one, and said to be old (section 7)
```

so `body` is one `match` on the token, with no lookahead; the existing `at_item` is its second arm. An opening's lines
(`END AMOUNT`) and a return's tally lines are neither: they are not flows and keep their plain form (`Legs::Plain`).
**An arrow in a contract's template or an occurrence's override is `->` only**: the model derives their direction from the
schedule's `from` or `into` (K4a-map section 3), so an arrow there is declarative, and a `<-` would say nothing it could keep.

## 5. How `fmt --upgrade` decides a line's subject, and what it refuses

### 5.1 Can it be purely syntactic?

**No.** Of the 19 projects, a rule written on the AST alone (an account declared with `account`, an `owner` argument, `me`, a
loan contract) agrees with the book on every end of every line of 15, and is wrong on 4, which hold 1,341 of the 3,343 headers:

| book | why the AST cannot say |
|---|---|
| `08-expat`, `09-shared`, `10-budgeter` (489 headers) | a name resolves by suffix: `account assets/bank/visa` is written `visa` in the journal |
| `explore-v5/06-family-addresses` (852) | an account is an address (`jordan/401k`, K3b), written by any subsequence of it, resolved by the line's day |

The rules that make a name mean a place (`Names<Place>` suffixes, `Addresses` posting-list intersection, scoped entities, an
issuer's place for a commodity, a loan contract's debt tab) are the model's. Writing them again in `syntax` is the
second source of truth the lanes keep removing, and the next lane's nesting (L2) would change them under it.

### 5.2 How the CLI gets them, and the syntax crate does not depend on model

`axiom fmt --upgrade` is a CLI command and the CLI already holds a `Session` (it builds the book). The upgrade is
`cli/src/upgrade.rs`: it reads the syntax trees (`Source.file`), asks the **book** through public lookups
(`Book::place`, `Book::entity`, `Book::contract`, `places[..].class` and `.owner`, `roots.me`), and builds
**edits** (a `Loc` and its replacement text: the vocabulary a diagnostic's `fix` already has). Applying them and laying
the file out is `axiom_syntax::format`, unchanged in its signature. The syntax crate's part is the junction table, the
AST, the formatter, and the diagnostic for each form that is wrong by its spelling alone.

A name's side is one of three: **own** (an account, an owner's holding, a loan's debt tab), **party** (an entity outside
the owners, `?`, a commodity's issuer, a contract that is not a loan) or **unresolved**.

### 5.3 The decision

For `S [A] -> O [B] tail`, v4's header:

| the ends | the rewrite |
|---|---|
| `S` is own | the line stays a give. Only its amounts and price are touched (section 6) |
| `S` is party or unresolved, `O` is own | `O <- S B tail`, the amount moved if it was left and is a literal |
| `S` is party, no object, legs follow | `OWNER <- S A tail`, legs get `->`; `OWNER` is the one owner of the legs' own accounts, else `me` (`roots.me`) |
| no subject, `O` named, legs follow | `OWNER -> O A tail`, legs get `<-` |
| neither end is own | **refused**: nothing in the line says whose book it is |
| the legs' own accounts have two owners | **refused**: which owner passes the money on? |
| an end is a role of a `kind` (`employer -> irs` in a kind's `also`) | **left alone and listed**: its side depends on whose book reads it |
| an end is unresolved and so is the other | **refused** |

**Counted on the examples: 0 refusals for a side, 0 for an owner, 3 `also` lines of kinds listed** (`also employer -> irs ...`
in `explore-v5/07-relators/kinds.ax`; the survey classified every end of every header and `also` flow). Zero is not a
property of the rule; it is that `Party -> Party` and two-owner splits do not occur in the 19 projects (checked: the survey
prints each class pair). The refusals the examples *do* produce are the 21 inexact prices of section 6 (19 with both ends named, 2 with the
source alone), each named in the output of `fmt --upgrade` with the price that would make it work.

## 6. The exchange, and what the upgrade will not invent

A line with both amounts has a redundant side. The price's unit says which: **the amount in the price's unit is dropped**
(it is `quantity * price`), the other kept where it was written (left of the arrow, or after the object).

| the line | the rewrite | the check before writing |
|---|---|---|
| both amounts and `@ P`, P's unit is on one side | drop that side's amount | `round(kept * P) == dropped`, half-to-even at the dropped unit's scale |
| P's unit is on neither side | refused: the model refuses it too (`price-unit`) | |
| both amounts, no price, `dropped / kept` an exact decimal | write `@ dropped/kept` and drop the amount (15 lines) | the same |
| both amounts, no price, not exact | **refused** (21 lines), naming the line and the price that rounds right (`@ 135.51 USD` gives 1,788.73 USD) | |
| one unit twice (`6 ETH -> 6 ETH`) | one amount | the two are equal |

The upgrade does not write a price nobody wrote. A line it refuses stays as v4 wrote it, which the parser still reads
(and says is old). **The 21 inexact lines of the examples are rewritten by hand with the price the diagnostic names**, in
the commit that upgrades the examples, and each is listed in the report: a human decision with the line in front of them
(the broker's confirmation says the price), not a tool's. The book is the same either way (the oracle compares it).

## 7. The legacy forms

Three forms are old by their spelling alone, and the parser can say so without knowing any name:

| form | what v5 writes | detected where |
|---|---|---|
| a leg with no arrow, under a header v4 could write (`->` header, an occurrence, a template) | `-> irs 692 USD` | `body`: the first token is a name |
| an amount on each side of the arrow | one amount and `@` | `flow_head`: both sides have one |
| an amount left of the arrow with nothing after it, and legs | `S -> A` | `check_shape`: the one-ended arrow with an amount before it |

**They are not a second grammar.** Each is what `body`, `side` and `check_shape` parse for v5, and each is recognised by a
test on the result: so the legacy module is the three tests, the warning, and its aggregation, not a recogniser. The
warning is **one per file** (as `tab-indent` is, `lines.rs:61`): `v4-syntax: 17 lines are written the v4 way`, with a label
at the first of each form and the command that fixes them, because a book with 359 bare legs would otherwise print 359 warnings.
A **bare leg under a header v4 could not write** (a `<-` header, a through-split) is not legacy: it is an error with the
arrow to insert, since there is nothing old to be tolerant of.

Estimated cost, to be measured: `legacy.rs` about 110 lines (three predicates, the aggregate, the diagnostic with its
fixes), plus 12 in `parse_in`/`lines` to carry the count across pieces. **Dropping the forms later is a one-commit
decision**: the three tests turn from a warning into an error. **Keeping them costs the module; dropping them without it
costs more**, because `fmt --upgrade`, which parses v4 text to produce its AST, would need a recogniser of its own that
is the same parse plus a place to put the form it found. An accepting parser with a warning is the smaller of the two.

## 8. The proof

"Syntax only" is the claim that the v4 text and its upgrade lower to the same book. The book cannot be dumped (item 9), so
the proof has three parts, each a test and not a one-off.

1. **`cli/src/upgrade` tests**: for every project directory under `examples/`, `tests/mistakes/` and
   `docs/v5/measure/diff/`, build the session from the v4 text and from the upgraded text, and compare the JSON of **every**
   query (`balance` plain, `--value`, `--monthly`; `register` of every place; `flow` by month and party; `available`,
   `budget`, `limits`, `claims`, `contracts`, `tax` and `gains` of every year, `lots`, `forecast`) and the diagnostics
   (code, severity, message; the `v4-syntax` warning is the one allowed to be gone).
2. **The same, through `internals/main.rs`**, which prints the debug text of every posted flow, gain and holding: the part
   of the book that has `Debug`.
3. **`docs/v5/measure/junction.py`**, seeded, in the style of `splits.py`: random lines of every shape (give, take, buy,
   sell, an exchange with each amount, a split with legs and items, a contract's templates) written once as data and
   **rendered in v4 and in v5**; the two renders run through the one binary and compared; the upgrade of the v4 render is
   compared with the v5 render after both are formatted. It mutation-tests itself: swap the sides, drop a leg's arrow,
   flip buy and sell, each as a mutant of the v5 render that must change the output.

`fmt --upgrade` also **checks its own result** before it writes: it builds the book from the upgraded sources and writes
only when the diagnostics agree and every query it can render matches. A file whose upgrade does not match is reported and
left as it was.

## 9. What the lane does not do, and the one STOP

- **A semantic check of the subject (`<-` between two parties, `->` whose subject is a party no owner holds) cannot be made
  by the parser** (it knows no sides) **and would need the model to read `Flow.junction`.** That is the one place the brief
  says to stop. The lane does not add it. What it builds instead: the upgrade *refuses* both shapes in a v4 book, naming the
  line (section 5.3); `check` does not say anything about a v5 line like `acme <- irs 40 USD`, which lowers exactly as
  `irs -> acme 40 USD`. A model check is about fifteen lines in `lower_txn` (read `junction`, resolve both ends, compare
  classes) and waits for a word.
- **`into` and `from` for an exchange between two of the owner's positions** (research A10) are not built (D3).
- **`sync` and `session` still write party-subject `->` lines.**
- **The positions under their agent, the optional counterparty and purposes without a direction root** are L2 and L3.

## 10. Decisions this map makes, for review

| | decision | why |
|---|---|---|
| D1 | the upgrade is in `cli`, over the book's public lookups; the syntax crate knows spellings only | section 5.1: 4 books of 18 name a place by a rule only the model has |
| D2 | `<-` is lexed only when a blank or the end of the line follows | `x <-5` in a law is `x < -5`; no corpus has `<-` outside the research ledgers |
| D3 | the subject's amount may stand left of the arrow (`S A -> O @ P`), and no `into`/`from` is built | v4 already says it; the cross-account sale has no other spelling without a new clause (research A10) |
| D4 | a contract's schedule is told from an item by reading its amount, not by a keyword | no new word in every contract; at_schedule's four clones go |
| D5 | the legacy warning is one per file | 359 bare legs in the examples |
| D6 | an inexact price is refused, with the price that rounds right, and the 21 lines of the examples are rewritten by hand | a tool should not write a number nobody wrote |
| D7 | `fmt --upgrade` lays the file out | the examples are not formatter-clean; a rewritten line has a new width |
| D8 | the legacy lines are left as written by plain `fmt` | one set of columns; `fmt` is not a silent upgrade |

## 11. How this map was checked

- **Counts:** a scratch program (not committed; a copy of it would need a `Cargo.toml` with path dependencies on `core`,
  `syntax`, `model`, `session` and `systems`) parsed each project with `axiom_syntax::parse`, built it with `Session::open`,
  and classified each header end through `Book::contract`, `Book::place` and `Book::entity` (a place's class, an entity's own
  place). It ran over the 11 files of `examples/` and over `explore-v5/` and `v4-sketch/`. It also compared a rule written
  on the AST alone against the book (section 5.1), and printed the owners of the own legs of every party-headed split.
- **Prices:** the survey printed every both-amount line without a price, and a Python pass divided each with `Decimal` and
  tested the quotient to ten places (a quotient with more digits is not a price anyone wrote). Two further both-amount lines
  of `08-expat` carry v3's `/ party` and are errors the parser reports before the model sees them: they are not counted.
- **Behaviour of the baseline:** a binary built from `25566a5` (`scratchpad/l1/baseline-axiom`) ran `fidelity -> fidelity 3 VTI @
  290 USD`, `fidelity 2 VTI -> fidelity @ 295 USD` and `fidelity 5 VTI -> 1_000 USD` (item 3 and 4), and `axiom fmt --check`
  over each example (item 11), and `check` over each (the baseline's errors, so that a change in them is seen).
- **Sizes:** `python3 briefs/loc.py .` before: syntax 5,632, cli 2,338, total 55,246. `docs/v5/measure/hist.py crates`: 3,546
  functions, 7 over 80 lines. `bench/run.sh 1m` on the baseline: `check` 4.337 s, `check-nolaws` 3.079 s (three runs, the
  fastest; the machine's load average was 1 to 7).

## 12. What was built, and where it departs from this map

The commits, in order: the map; one production for a dated line and the verb table (3512058); the junction in the AST and the
formatter's arrow column (7035ded); `fmt --upgrade` (bb3b2cd, c213635, dd2a138); every example, fixture and generator in the
new spelling (6d03832); the v4 warning and the diagnostics of a malformed junction (cb1c1b3, 817ce9b); the mistakes corpus
regraded in one commit (b445296); the proof test (40f36d9); the exchange inside one end as a purchase or a sale, and the
seeded generator with its mutants (6888cc5); `LANGUAGE.md` (a2acb5c); the measuring scripts that discount the v4 warning
(77238b8, a202f22); the counting bench (49ae099); and the course of a flow in one word (e1e01dc).

**Where it departs from sections 5 to 10.**

- **D1 changed: the upgrade is in `syntax`, behind a `Registry`.** The rewrites need the AST's edits and the formatter, and
  both are in the syntax crate; what only the book knows is five lookups (`standing`, `owner`, `keeper`, `base`, `scale`),
  so the syntax crate has the trait and the cli's `BookRegistry` implements it over `Book`. The syntax crate still does not
  depend on the model. The cli's part is `fmt.rs` (+115 lines of code) and the option (+22).
- **A fourth legacy form.** A `->` with no subject (`-> checking 100 USD`) is v4's way to say what `checking <-` says, so
  it is the fourth form the warning counts, beside the bare leg, the two amounts and the amount before an arrow that has
  only legs after it.
- **D8 stands, and the warning is the rest of it.** Plain `fmt` leaves a v4 line as written; the parser notes the line, and
  `axiom check` says once for the file, with a label at the first of each form, that it is written the v4 way and which
  command rewrites it.
- **The refusals are two, not three.** `upgrade-owner` (no owner holds the subject, so the arrow cannot be told) and
  `upgrade-price` (an amount that no price a person would write relates to the other; the message has the shortest price that
  agrees and the whole v5 line). `upgrade-sides` is gone: a flow that neither end owns is left as it is written, as is an
  exchange that names one end (the v4 book rejects it, and v5 gives it a meaning, so rewriting it would change the book's
  errors). `upgrade-changes-the-book` is for a whole file: it is not written when the book it makes says anything else.
- **A file is written unless it would change the book, and a refused line is left and reported.** The upgrade writes every
  line it can place, leaves the others as they were, names each, and exits with failure; a file is refused whole only when the
  book it would make differs from the book it was (five counts, the diagnostics by severity, code and message less the v4
  warning, and the JSON of the balance sheet, the flows and the claims).
- **An exchange inside one end is a purchase or a sale.** `S -> S A @ P` is `S <- A @ P`, and `S A -> S @ P` is
  `S -> A @ P`: the price is the only thing a line says twice, and v5 says it once.
- **Steps 4 and 5 of the map's order were swapped.** The fixtures were upgraded first, and all green; the warning came after,
  so that no commit had a test that failed only because a fixture was in the old spelling.
- **Tests were converted in place, with a legacy twin for a v4-spelled one.** A test whose subject is a v4 spelling keeps
  its text and runs it and its v5 spelling in a loop (`for text in [V4, V5]`), so that no assertion was weakened and none
  deleted. The helpers that parse a fixture accept the one `v4-syntax` warning.
- **A flow's course is one word.** The first AST kept `junction` and `through` as two fields, and a `Txn` grew from 128 to 160
  bytes (1,000,094 flows: 31 MB). They are a `Course`, with the owner in a table of its own, and a `Txn` is 136 bytes.
- **The model reads one thing of a junction.** The map's STOP was answered yes, and `junction-subject` (the last commit, which
  reverts alone) is that one read: for a line written `<-`, as a purchase or a sale, or as a split through an owner, the
  subject must be an end an owner holds, and a party is not. It is 39 lines of code in the model (`party_subject` and its
  call; the brief said about 30, and the rest is the message) and 27 in the syntax crate (`Flow::owner`, which says which end
  a spelling makes the subject, and `File::arrow_after`, which finds the arrow to point at), with four mistake books (123 to
  126). A `->` that starts at a party is read as it always was, so no book written in v4 changes meaning. It found one
  fixture the upgrade's heuristics had got wrong: `ann -> stockroom 5 BOX` in the engine's claim tests, between two parties,
  had been written `stockroom <- ann 5 BOX`; it is as it was.

**What was measured.** See the lane's report for the numbers (they are not repeated here): the proof test
(`crates/cli/tests/upgrade.rs`), the 120 seeded books and their mutants (`docs/v5/measure/junction.py`), `fuzz.py ... diff`,
`splits.py`, the K0a harness and the K7b session scripts; and the parser's instruction count on the counting bench
(3,194 M before, 3,461 M after on v4 text) and on `check` of a 100,000-flow book (1,872.8 M before, 1,889.8 M after).
