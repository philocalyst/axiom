# Axiom v2 diagnostics: the mistakes corpus and audit

Lane G2 (diagnostics). Built and run against the tree at the commit this file ships
with (`cargo build --release`, `axiom 0.3.0`), colourless, `--today 2026-06-30`.

**What is here.**

| item | where |
|---|---|
| 99 mistakes, each one realistic error in a single-file or small directory project | `NN-name.ax` and `NN-name/` |
| the captured output of `axiom check` on each (stdout and stderr, then the exit status) | `NN-name.out` |
| 24 robustness probes (must not panic or hang) | `robust/rNN-*.ax`, `robust/rNN-*.out` |
| 16 "constraint surfacing" probes | `constraints.sh`, `constraints.out` |
| the runner that regenerates every `.out` | `run.sh` |
| this audit | `REPORT.md` |

Regenerate: `sh tests/mistakes/run.sh` (all), `sh tests/mistakes/run.sh 07 14`
(by prefix), `sh tests/mistakes/constraints.sh`. Nothing under `crates/` was touched.

**Grades.** Each case is graded on six questions: is the primary location exactly right;
is there cascade or noise; does it speak accounting (amounts, statements, checks) rather
than implementation; does it show the *other* source involved; is there a suggestion and
is it correct; would a first-time user know what to do.

- **A**: as good as rustc/Elm; the "ideal" below is the actual with cosmetic changes.
- **B**: right place and right idea; missing the fix as an edit, or a small noise or wording problem.
- **C**: correct fact, but unhelpful, or with a cascade, or in implementation words.
- **D**: misleading, several errors for one cause, points into a file the user cannot edit, or an unsafe fix.
- **F**: silent (no diagnostic for something that changes the books) or wrong.

**Every case gives:** the mistake; the actual output (a summary, with the `.out` file
as the source of truth); a verdict; and the **ideal diagnostic written out in full**,
in the house style (§14). In the ideal blocks, a line starting `NN +`/`NN -` is the
edit the tool would draw, as the renderer already does for its `did you mean` helps.

## 1. Grade distribution

| layer | cases | A | B | C | D | F |
|---|---|---|---|---|---|---|
| Lexing and parsing | 01–20 | 6 | 6 | 6 | 2 | 0 |
| Names | 21–31 | 5 | 4 | 1 | 0 | 1 |
| Transactions | 32–47 | 3 | 6 | 5 | 1 | 1 |
| Journal | 48–60 | 2 | 3 | 5 | 2 | 1 |
| Laws | 61–82 | 6 | 6 | 7 | 2 | 1 |
| Parsing, assertions, plans | 83–92 | 2 | 3 | 1 | 3 | 1 |
| Layout | 93–99 | 2 | 5 | 0 | 0 | 0 |
| **all** | 99 | **26** | **33** | **25** | **10** | **5** |

## 2. What the audit found, in six sentences

1. The **lexer and parser are excellent** at *isolated* mistakes (tabs, dates, `$`, `usd`, `0`, glued amounts): most get a precise span and a correct edit. The few weak ones are tokens the lexer splits differently from a human (`=>`, `→`, `01/15/2026`), and one suggested edit that **changes the amount** (`1.234,56` → `1.234_56`).
2. **Error recovery is the systemic failure**: a declaration that fails to parse is dropped, and each later use becomes an `unknown-place` (seven cases). Several model/engine errors also multiply (three errors for one missing `filing`, four for one missing table, two for one missing price).
3. **Five inputs are silently accepted and change the books**: an undeclared commodity, a typo inside a full account path, a settlement dated before its flow, a backwards date range (a payment vanishes), a missing `base`. These are the top defect, more than any wording problem.
4. **Assertion failures state the gap but never explain it**, and the first help is `!` (accept the gap). No transposition, reversal, sign or missed-transaction reasoning; an overdrawn balance cannot be asserted at all.
5. **Laws from the built-in systems are reported in the built-in files**, in implementation words ("cannot check `deferral-limit`: `born` is not set"), pointing the user at `us/401k.ax` instead of at their own `entity me`.
6. **Constraints are hard to see**: `check` hides priced violations (a 1,000 USD 401(k) penalty prints `✓`); no command shows a limit with the amount used and the room left; reports refuse to run while any error stands.

The cases follow the layers: §3 lexing and parsing, §4 names, §5 transactions, §6 journal,
§7 laws, §8 more parsing/assertions/plans, §9 layout, §10 robustness probes. §11 is constraint
surfacing, §12 the ten fixes, §13 systemic patterns, §14 the style guide, §15 the index.

## 3. Lexing and parsing (cases 01–20)

The lexer's messages are the best in the tool: they usually name the exact character
run, explain the rule, and carry a one-line fix. What goes wrong here is (a) tokens that
the lexer splits differently from how a human reads them (`=>`, `→`, `01/15/2026`),
(b) fixes that would change the amount, and (c) **error recovery**: when a
declaration fails to parse, the whole declaration is dropped, and every later use of
what it declared becomes an `unknown-place` error (cases 14, 15, 19, 72, 73, 81, 83).

### 01 tab-indent — grade B
**Mistake.** The legs of a paycheck are indented with a tab.
**Actual** (`01-tab-indent.out`): two `tab-indent` errors, one per line, each with the
note "indentation is spaces…" and the fixed line.
**Verdict.** Location exact; fix correct. But it is one error per *line*: a file that an
editor tab-indented has hundreds. (The tab is drawn four columns wide while the fix says
"two spaces per tab".)
**Ideal.**
```text
error[tab-indent]: 2 lines are indented with a tab
   ╭─[01-tab-indent.ax:14:1]
   │
13 │ 2026-01-15 acme -> 5_000 USD
14 │ ⇥ savings   500 USD
   │ ┬
   │ ╰── tab
15 │ ⇥ checking  ...
   │ ┬
   │ ╰── tab
   │
   = note: a tab is 2 columns in one editor and 8 in another, so the block only looks aligned
   = help: indent with two spaces
14 +   savings   500 USD
15 +   checking  ...
```
(One diagnostic per contiguous run of tab-indented lines; `⇥` marks the tab.)

### 02 bad-indent — grade B
**Mistake.** The second leg is indented deeper than the first.
**Actual** (`02-bad-indent.out`): `unexpected-indent`: "this line is indented further than
the lines above it", with the fix `  checking  ...`.
**Verdict.** Correct, with a fix. The label on the *previous* line ("this line takes no
indented block") reads as an accusation of the wrong line; "where the block uses 2" is
jargon.
**Ideal.**
```text
error[indent]: this leg is indented 4 spaces, but the legs above it use 2
   ╭─[02-bad-indent.ax:16:5]
   │
14 │ 2026-01-15 acme -> 5_000 USD
15 │   savings   500 USD
   │   ─┬
   │    ╰── the first leg sets the indent: 2 spaces
16 │     checking  ...
   │     ────┬───
   │         ╰── 4 spaces
   │
   = help: legs of one transaction line up
16 +   checking  ...
```

### 03 missing-arrow — grade B
**Mistake.** `2026-01-18 checking groceries 84.20 USD` (no `->`).
**Actual** (`03-missing-arrow.out`): `expected-arrow`: "expected `->`, found `groceries`",
underline on `groceries`; two helps (a generic sentence, then the edit).
**Verdict.** The underline is on the wrong token (the fault is the gap after
`checking`), and the first help is filler.
**Ideal.**
```text
error[expected-arrow]: a flow needs `->` between where the money leaves and where it goes
   ╭─[03-missing-arrow.ax:15:20]
   │
15 │ 2026-01-18 checking groceries 84.20 USD
   │                    ▲
   │                    ╰── `->` goes here
   │
   = help: insert the arrow
15 + 2026-01-18 checking -> groceries 84.20 USD
```

### 04 fat-arrow — grade C
**Mistake.** `checking => groceries` (`=>` for `->`).
**Actual** (`04-fat-arrow.out`): `expected-amount`: "expected an amount such as `50 USD`,
found `>`", underline on the `>` only.
**Verdict.** The lexer reads `=` then `>`. The reported token is half of what the user
typed, the expectation (an amount) is what follows an `=` assertion, and there is no
suggestion. The user wrote a perfectly clear intent.
**Ideal.**
```text
error[unknown-arrow]: `=>` is not the flow arrow; write `->`
   ╭─[04-fat-arrow.ax:15:21]
   │
15 │ 2026-01-18 checking => groceries 84.20 USD
   │                     ─┬
   │                      ╰── money moves with `->`
   │
   = help: replace it
15 + 2026-01-18 checking -> groceries 84.20 USD
```
(The same rule should catch `→`, `-->`, `>`, and `=` when a place follows; see 05.)

### 05 unicode-arrow — grade C
**Mistake.** A typographic arrow `→` pasted from a document.
**Actual** (`05-unicode-arrow.out`): `unexpected-character`: "unexpected character '→'", label
"not part of the language", no help.
**Verdict.** Location exact; no suggestion for the most likely intent. Also
`unexpected-character` says nothing about *what* the language does allow there.
**Ideal.**
```text
error[unknown-arrow]: `→` looks like an arrow, but flows are written with `->`
   ╭─[05-unicode-arrow.ax:15:21]
   │
15 │ 2026-01-18 checking → groceries 84.20 USD
   │                     ┬
   │                     ╰── U+2192 RIGHTWARDS ARROW
   │
   = help: use the ASCII arrow
15 + 2026-01-18 checking -> groceries 84.20 USD
```

### 06 amount-no-commodity — grade B
**Mistake.** `84.20` with no commodity.
**Actual** (`06-amount-no-commodity.out`): `expected-commodity`: "expected a commodity such as `USD`,
found the end of the line", caret after the number; help "an amount is a number and its
commodity".
**Verdict.** Correct and helpful; but it points at the empty space after the number and
never offers the completed line, although the project's base currency is known.
**Ideal.**
```text
error[expected-commodity]: `84.20` has no commodity
   ╭─[06-amount-no-commodity.ax:15:34]
   │
15 │ 2026-01-18 checking -> groceries 84.20
   │                                  ──┬──
   │                                    ╰── a commodity is missing after the number
   │
   = note: this ledger's base currency is USD
   = help: name the commodity
15 + 2026-01-18 checking -> groceries 84.20 USD
```

### 07 lowercase-commodity — grade A
**Mistake.** `84.20 usd`.
**Actual** (`07-lowercase-commodity.out`): `lowercase-commodity`: "commodities are written in capitals,
not `usd`", label "a commodity is uppercase", help with the corrected line.
**Verdict.** As good as rustc. Only nit: say *why* (lowercase names are places).
**Ideal.**
```text
error[lowercase-commodity]: commodities are written in capitals, not `usd`
   ╭─[07-lowercase-commodity.ax:15:40]
   │
15 │ 2026-01-18 checking -> groceries 84.20 usd
   │                                        ─┬─
   │                                         ╰── read as a place name, because it is lowercase
   │
   = help: write `USD`
15 + 2026-01-18 checking -> groceries 84.20 USD
```

### 08 zero-not-empty — grade A
**Mistake.** `2026-01-31 visa = 0`.
**Actual** (`08-zero-not-empty.out`): `bare-zero`: "a bare `0` has no commodity; write `empty`"
with note and fix.
**Verdict.** Ideal already; keep as is.
**Ideal.**
```text
error[bare-zero]: a bare `0` has no commodity; write `empty`
   ╭─[08-zero-not-empty.ax:14:19]
   │
14 │ 2026-01-31 visa = 0
   │                   ┬
   │                   ╰── zero of what?
   │
   = note: `empty` is the zero of every commodity, so it needs no unit
   = help: write `empty`
14 + 2026-01-31 visa = empty
```

### 09 invalid-date — grade A
**Mistake.** `2026-02-30`.
**Actual** (`09-invalid-date.out`): `bad-date`: "February 2026 has 28 days", label on `30`, help
`2026-02-28`.
**Verdict.** Excellent. (One accounting nit: if the intent was a month-end, `2026-02-28`
is right; if it was March 2, the suggestion is silently wrong. Offer both when the day is
1–3 over.)
**Ideal.**
```text
error[bad-date]: February 2026 has 28 days
   ╭─[09-invalid-date.ax:15:9]
   │
15 │ 2026-02-30 checking -> groceries 84.20 USD
   │         ─┬
   │          ╰── there is no day 30
   │
   = help: the last day of February 2026 is `2026-02-28`
15 + 2026-02-28 checking -> groceries 84.20 USD
```

### 10 date-slashes — grade D
**Mistake.** `01/15/2026`.
**Actual** (`10-date-slashes.out`): `unknown-keyword`: "unknown keyword `01/15/2026`", "a line
starts with a date or a keyword", then the list of 14 keywords.
**Verdict.** Misleading: the user typed a date and is told it is an unknown *keyword*.
The lexer already knows the three shapes users mistake for dates (`MM/DD/YYYY`,
`DD/MM/YYYY`, `YYYY/MM/DD`); the tool should say which it read and propose `2026-01-15`.
**Ideal.**
```text
error[bad-date]: `01/15/2026` is not a date; dates are written `YYYY-MM-DD`
   ╭─[10-date-slashes.ax:15:1]
   │
15 │ 01/15/2026 checking -> groceries 84.20 USD
   │ ─────┬────
   │      ╰── read as month/day/year: January 15, 2026
   │
   = note: `01/02/2026` would be ambiguous (Jan 2 or Feb 1), so Axiom never guesses;
           15 cannot be a month, so this one is clear
   = help: write the date year first
15 + 2026-01-15 checking -> groceries 84.20 USD
```

### 11 unclosed-string — grade B
**Mistake.** `! "cash tips` (no closing quote).
**Actual** (`11-unclosed-string.out`): `unterminated-string`: caret at the opening quote, "the string
starts here", help "a string ends with a closing `"` on the same line".
**Verdict.** Right place, right rule, but no edit; the closing quote can only go at the
end of the line, so the fix is exact.
**Ideal.**
```text
error[unterminated-string]: this string has no closing quote
   ╭─[11-unclosed-string.ax:15:46]
   │
15 │ 2026-01-18 checking -> groceries 84.20 USD ! "cash tips
   │                                              ┬───────┬
   │                                              │       ╰── the line ends here
   │                                              ╰── string starts here
   │
   = help: close it at the end of the line
15 + 2026-01-18 checking -> groceries 84.20 USD ! "cash tips"
```

### 12 comma-decimal — grade D
**Mistake.** `1.234,56 USD` (European: dot thousands, comma decimals).
**Actual** (`12-comma-decimal.out`): `thousands-comma`: "`1.234,…` is not a number: thousands are
separated with `_`", label `use _ here` on the comma, help "write `_` between digit
groups" and the edit **`1.234_56 USD`**.
**Verdict.** **The suggested edit changes the amount** from 1,234.56 to 1.23456 USD
(a decimal number `1.234_56`). A fix that silently changes money is the worst kind of
help. The message also blames a "thousands comma" although the comma is the decimal.
**Ideal.**
```text
error[european-number]: `1.234,56` looks like a European amount (dot for thousands, comma for decimals)
   ╭─[12-comma-decimal.ax:15:39]
   │
15 │ 2026-01-02 equity/opening -> checking 1.234,56 USD
   │                                       ───┬─────
   │                                          ╰── read as 1,234.56
   │
   = note: Axiom writes numbers with `.` for decimals and `_` between groups of digits
   = help: if you meant one thousand two hundred thirty-four and 56 cents
15 + 2026-01-02 equity/opening -> checking 1_234.56 USD
```

### 13 dollar-sign — grade A
**Mistake.** `$84.20`.
**Actual** (`13-dollar-sign.out`): `currency-symbol`: "amounts are written `84.20 USD`, not
`$84.20`", note about why, help with the edit.
**Verdict.** Ideal. (Add: `$` is USD only if the base currency is; in a CAD ledger the
edit should not assume.)
**Ideal.**
```text
error[currency-symbol]: amounts are written `84.20 USD`, not `$84.20`
   ╭─[13-dollar-sign.ax:15:34]
   │
15 │ 2026-01-18 checking -> groceries $84.20
   │                                  ───┬──
   │                                     ╰── a currency symbol
   │
   = note: the commodity comes after the number and is spelled out, so `USD` and `CAD` (both `$`)
           cannot be confused
   = help: write `84.20 USD` (this ledger's base currency)
15 + 2026-01-18 checking -> groceries 84.20 USD
```

### 14 trailing-operator — grade C
**Mistake.** A law ends in `+`.
**Actual** (`14-trailing-operator.out`): `expected-expression` at the end of the line, **plus** a second
error `unknown-place: there is no place food` on the first flow.
**Verdict.** The first error is right (caret after the `+`, but no "the `+` needs a
right-hand side" label). The second is a cascade: the whole `account expenses/food` block
failed to parse, so the account never existed. One root cause, two errors, and the
second's help ("write its full path under `expenses`") is wrong advice.
**Ideal.**
```text
error[expected-expression]: `+` needs something to add on its right
   ╭─[14-trailing-operator.ax:10:39]
   │
10 │     warn total(in, month) <= 650 USD +
   │                                      ┬
   │                                      ╰── the expression ends here, after `+`
   │
   = help: add a value, or remove the `+`
   = note: `expenses/food` and its law `monthly-cap` were kept, but this line was ignored; no
           other errors are caused by it
```
(Recovery rule: an item that fails inside its body is kept with the body line dropped.)

### 15 unbalanced-parens — grade C
**Mistake.** `warn (total(in, month) <= 650 USD "…"`: `(` never closed.
**Actual** (`15-unbalanced-parens.out`): `unclosed-delimiter` with two labels (the `(`, and the
string where `)` was expected), then the same `unknown-place` cascade as 14.
**Verdict.** The first diagnostic is very good (both ends labelled). The cascade is not.
**Ideal.**
```text
error[unclosed-delimiter]: this `(` is never closed
   ╭─[15-unbalanced-parens.ax:10:10]
   │
10 │     warn (total(in, month) <= 650 USD "food is over budget"
   │          ┬                            ──────────┬──────────
   │          │                                      ╰── `)` was expected before this text
   │          ╰── opened here
   │
   = help: close it after `650 USD`
10 +     warn (total(in, month) <= 650 USD) "food is over budget"
```
(The tool can produce the edit because the next token, a string, cannot continue an
expression.)

### 16 negative-amount — grade C
**Mistake.** A refund written `-12.00 USD`.
**Actual** (`16-negative-amount.out`): `negative-amount`: "amounts carry no sign", note
"the arrow gives the direction…", fix "remove the sign".
**Verdict.** The rule is right, but the fix (delete the `-`) turns a *refund* into a
second purchase: a silent doubling of the expense. The user's intent (money coming back)
needs the flow reversed.
**Ideal.**
```text
error[negative-amount]: amounts carry no sign
   ╭─[16-negative-amount.ax:16:34]
   │
16 │ 2026-01-19 checking -> groceries -12.00 USD
   │                                  ┬
   │                                  ╰── a refund goes the other way
   │
   = note: the arrow gives the direction: to record money coming back, swap the two places
   = help: write the refund as its own flow
16 + 2026-01-19 groceries -> checking 12.00 USD
```

### 17 thousands-comma — grade B
**Mistake.** `1,200 USD`.
**Actual** (`17-thousands-comma.out`): `thousands-comma`: "`1,…` is not a number: thousands are
separated with `_`", fix `1_200 USD`.
**Verdict.** Correct for US-style input. But `1,200` in a European ledger means 1.20;
the same message text and the same edit are shared with case 12, so the tool cannot
be right about both. It should say which reading it took.
**Ideal.**
```text
error[thousands-comma]: `1,200` is not a number; thousands are separated with `_`
   ╭─[17-thousands-comma.ax:15:40]
   │
15 │ 2026-01-02 equity/opening -> checking 1,200 USD
   │                                        ─┬───
   │                                         ╰── read as one thousand two hundred
   │
   = help: use `_`
15 + 2026-01-02 equity/opening -> checking 1_200 USD
   = note: if `,` was a decimal separator (1.20), write `1.20 USD`
```

### 18 glued-amount — grade A
**Mistake.** `84.20USD`.
**Actual** (`18-glued-amount.out`): `glued-amount` with the exact fix.
**Ideal.** Same as actual.
```text
error[glued-amount]: `84.20USD` needs a space between the number and its commodity
   ╭─[18-glued-amount.ax:15:34]
   │
15 │ 2026-01-18 checking -> groceries 84.20USD
   │                                  ────┬───
   │                                      ╰── number and commodity run together
   │
   = help: amounts are written `84.20 USD`
15 + 2026-01-18 checking -> groceries 84.20 USD
```

### 19 unknown-keyword — grade C
**Mistake.** `acount assets/checking : bank`.
**Actual** (`19-unknown-keyword.out`): `unknown-keyword` "did you mean `account`?" with the
fix, **then two more errors**: `unknown-place: there is no place checking`, one on each use.
**Verdict.** The first error is a model diagnostic. The two `unknown-place` errors are
cascades; their help ("for example `expenses/checking`") is wrong, since `checking` was
declared. One typo, three errors, and in a real file, one per use of `checking`.
**Ideal.**
```text
error[unknown-keyword]: `acount` is not a keyword; did you mean `account`?
  ╭─[19-unknown-keyword.ax:5:1]
  │
5 │ acount assets/checking : bank
  │ ───┬──
  │    ╰── a line starts with a date or a keyword
  │
  = help: the closest keyword is `account`
5 + account assets/checking : bank
  = note: read as the declaration of `assets/checking`, so its 2 uses (lines 9, 10) are not reported
```

### 20 date-unpadded — grade A
**Mistake.** `2026-1-5`.
**Actual** (`20-date-unpadded.out`): `bad-date` with the padded fix.
**Ideal.** Same as actual.
```text
error[bad-date]: `2026-1-5` is not a date
   ╭─[20-date-unpadded.ax:15:1]
   │
15 │ 2026-1-5 checking -> groceries 84.20 USD
   │ ────┬───
   │     ╰── dates are written `YYYY-MM-DD`, two digits each for month and day
   │
   = help: write `2026-01-05`
15 + 2026-01-05 checking -> groceries 84.20 USD
```


## 4. Names and declarations (cases 21–31)

Name errors are handled well *when the name is not a commodity or a full path*: a
did-you-mean with an edit, a candidate list for ambiguity, a pointer to both
declarations for duplicates. Two silent failures here (23 and, in §5, 45) are the worst
defects in the whole corpus, because the typo does not error at all.

### 21 typo-account — grade B
**Mistake.** `dinning` for `dining`.
**Actual** (`21-typo-account.out`): `unknown-place`, "did you mean `dining`?" with the edit,
**and a second help** "to open a new account, write its full path … for example
`expenses/dinning`".
**Verdict.** The suggestion is right. The second help is noise when a near miss exists
and, worse, its example (`expenses/dinning`) is invented from the first root, not from
the nearest place (`expenses/food/dining`). Only one candidate is offered; there are
two other places with `food` in them.
**Ideal.**
```text
error[unknown-place]: there is no place `dinning`
   ╭─[21-typo-account.ax:15:24]
   │
15 │ 2026-01-24 checking -> dinning 62.35 USD
   │                        ───┬───
   │                           ╰── not declared
   │
   = help: did you mean `dining` (expenses/food/dining)?
15 + 2026-01-24 checking -> dining 62.35 USD
   = note: other close places: `groceries` (expenses/food/groceries)
```
(The "open a new account" help appears only when nothing is within edit distance.)

### 22 typo-entity — grade A
**Mistake.** `/ trader-joe` for `trader-joes`.
**Actual** (`22-typo-entity.out`): `unknown-entity`, note "a payee must be a declared entity",
did-you-mean with the edit.
**Verdict.** Ideal.
**Ideal.**
```text
error[unknown-entity]: there is no entity `trader-joe`
   ╭─[22-typo-entity.ax:15:46]
   │
15 │ 2026-01-08 checking -> groceries 84.20 USD / trader-joe
   │                                              ─────┬────
   │                                                   ╰── not a known entity
   │
   = note: a payee must be a declared entity, so that it is typed like everything else
   = help: did you mean `trader-joes`?
15 + 2026-01-08 checking -> groceries 84.20 USD / trader-joes
```

### 23 typo-commodity — grade F
**Mistake.** `84.20 UDS` for `USD`.
**Actual** (`23-typo-commodity.out`): **three errors and no mention of the typo**:
`insufficient-holding: assets/checking does not hold 84.2 UDS` ("holds 0.0 UDS"),
`no-price: no price for UDS in USD on 2026-01-08`, and
`no-price: cannot check overdraft: no price for UDS…`.
**Verdict.** An undeclared commodity is silently created. The three errors are all
downstream and all wrong about the cause; amounts print as `84.2 UDS` and `0.0 UDS`
(no precision, because the commodity has none); the third error leaks an internal
(`cannot check overdraft`). A user would look for a missing price line.
**Ideal.**
```text
error[unknown-commodity]: there is no commodity `UDS`
   ╭─[23-typo-commodity.ax:15:40]
   │
15 │ 2026-01-08 checking -> groceries 84.20 UDS
   │                                        ─┬─
   │                                         ╰── not declared
   │
   = help: did you mean `USD`?
15 + 2026-01-08 checking -> groceries 84.20 USD
   = note: commodities are declared with `commodity UDS`; USD, EUR, GBP… come with `use std`
```
(No further errors: the flow is skipped.)

### 24 typo-kind — grade A
**Mistake.** `account assets/checking : bnk`.
**Actual** (`24-typo-kind.out`): `unknown-kind`, "did you mean `bank`?" with the edit.
**Ideal.**
```text
error[unknown-kind]: there is no kind `bnk`
  ╭─[24-typo-kind.ax:5:27]
  │
5 │ account assets/checking : bnk
  │                           ─┬─
  │                            ╰── not a known kind
  │
  = help: did you mean `bank`? (kinds for an asset: `bank`, `broker`, `cash`, `property`, `vehicle`)
5 + account assets/checking : bank
```

### 25 ambiguous-suffix — grade A
**Mistake.** Two `…/checking` accounts; the flow says `checking`.
**Actual** (`25-ambiguous-suffix.out`): `ambiguous-place` listing both declarations and two
edits.
**Verdict.** Ideal (labels both declarations, offers both shortest unambiguous edits).
Improvement: mention the current balances so the user can tell which is meant.
**Ideal.**
```text
error[ambiguous-place]: `checking` could be either of these accounts
   ╭─[25-ambiguous-suffix.ax:11:12]
   │
 5 │ account assets/bank/checking : bank
   │         ──────────┬─────────
   │                   ╰── `assets/bank/checking`: 1,000.00 USD on 2026-01-08
 6 │ account assets/old-bank/checking : bank
   │         ────────────┬───────────
   │                     ╰── `assets/old-bank/checking`: 0.00 USD
   ⋮
11 │ 2026-01-08 checking -> food 84.20 USD
   │            ────┬───
   │                ╰── which one?
   │
   = help: write `bank/checking` for the first
11 + 2026-01-08 bank/checking -> food 84.20 USD
   = help: write `old-bank/checking` for the second
11 + 2026-01-08 old-bank/checking -> food 84.20 USD
```

### 26 duplicate-account — grade B
**Mistake.** `account assets/checking` declared twice, with different kinds (`bank`, `cash`).
**Actual** (`26-duplicate-account.out`): `duplicate-declaration`, both lines labelled, "remove one
of the two declarations".
**Verdict.** Correct; but the two declarations *conflict* (bank vs cash, which brings
the overdraft law), and the message treats them as identical.
**Ideal.**
```text
error[duplicate-declaration]: account `assets/checking` is declared twice, and the kinds differ
  ╭─[26-duplicate-account.ax:7:9]
  │
5 │ account assets/checking : bank
  │         ───────┬───────
  │                ╰── first declared here, kind `bank`
6 │ account assets/savings : bank
7 │ account assets/checking : cash
  │         ───────┬───────
  │                ╰── again here, kind `cash`
  │
  = note: `bank` warns when the balance goes below zero; `cash` does not
  = help: keep the declaration you mean and delete the other
```

### 27 entity-no-via — grade B
**Mistake.** `checking -> landlord`, but `entity landlord : org` has no `via`.
**Actual** (`27-entity-no-via.out`): `entity-without-via`: "`landlord` has no place to stand for",
labels on the declaration and the use, help "add `via` … for example `via
expenses/landlord`".
**Verdict.** Good. The headline is awkward, and the suggested place (`expenses/landlord`)
does not exist: the tool knows `expenses/rent` exists and was opened for a rent payee.
**Ideal.**
```text
error[entity-without-via]: `landlord` is not tied to a place, so a flow cannot end there
   ╭─[27-entity-no-via.ax:12:24]
   │
 9 │ entity landlord : org
   │        ────┬───
   │            ╰── declared without `via`
   ⋮
12 │ 2026-01-01 checking -> landlord 1_800 USD
   │                        ────┬───
   │                            ╰── an entity written where a place is expected stands for its `via` place
   │
   = help: name the place its payments belong to
10 +   via expenses/rent
```

### 28 unknown-system — grade A
**Mistake.** `use us/401l`.
**Actual** (`28-unknown-system.out`): `unknown-system`, did-you-mean with the edit.
**Ideal.**
```text
error[unknown-system]: there is no system `us/401l`
  ╭─[28-unknown-system.ax:4:5]
  │
4 │ use us/401l
  │     ───┬───
  │        ╰── no system has this path
  │
  = help: did you mean `us/401k`? (systems under `us/`: `401k`, `529`, `hsa`, `ira`, `ca`, `ny`)
4 + use us/401k
```

### 29 property-typo — grade A
**Mistake.** `employr acme` on a 401(k).
**Actual** (`29-property-typo.out`): `unknown-property`, lists the account's properties, "did you
mean `employer`?" with the edit.
**Ideal.** Same as actual; list only the properties that apply to *this kind* first.
```text
error[unknown-property]: `employr` is not a property of an account of kind `401k`
   ╭─[29-property-typo.ax:12:3]
   │
11 │ account assets/retirement : 401k
12 │   employr acme
   │   ───┬───
   │      ╰── no such property
   │
   = note: `401k` accounts have `employer`; every account has `owner`, `holds`, `select`, `opened`,
           `closed`, `budget`, `liquidity`
   = help: did you mean `employer`?
12 +   employer acme
```

### 30 payee-not-entity — grade C
**Mistake.** `/ groceries`: the payee after the slash is a *place*, not an entity.
**Actual** (`30-payee-not-entity.out`): `unknown-entity: there is no entity groceries`; note "a
payee must be a declared entity".
**Verdict.** True but unhelpful: `groceries` exists, as an account, and the flow already
pays it. The user probably wanted `trader-joes`, an entity that stands for that account.
**Ideal.**
```text
error[unknown-entity]: `groceries` is a place, not an entity, so it cannot be a payee
   ╭─[30-payee-not-entity.ax:15:46]
   │
15 │ 2026-01-08 checking -> groceries 84.20 USD / groceries
   │                        ────┬────             ────┬────
   │                            │                     ╰── a payee is an entity (a person or a company)
   │                            ╰── this is already the place, `expenses/food/groceries`
   │
   = help: drop the payee, or use the entity that stands for this place
15 + 2026-01-08 checking -> groceries 84.20 USD / trader-joes
```

### 31 kind-in-wrong-slot — grade B
**Mistake.** `account assets/brokerage : fund` (`fund` is a commodity kind).
**Actual** (`31-kind-in-wrong-slot.out`): `kind-sort`: "kind `fund` is a commodity kind, so it
cannot describe the asset account", labels the use and the kind's declaration in `std.ax`.
**Verdict.** Correct and shows the other source. Missing: what the user probably meant
(`broker`), and the built-in file is not marked as such.
**Ideal.**
```text
error[kind-sort]: `fund` is a kind of commodity; an account needs a kind of asset
   ╭─[31-kind-in-wrong-slot.ax:8:28]
   │
 8 │ account assets/brokerage : fund
   │                            ──┬─
   │                              ╰── commodity kind
   │
   ├─[std.ax:99:6] (built in)
   │
99 │ kind fund : security
   │      ──┬─
   │        ╰── `fund` describes a commodity such as VTI (a kind of `commodity`)
   │
   = help: an account that holds funds is a `broker`
 8 + account assets/brokerage : broker
```

## 5. Transactions (cases 32–47)

The pairing rules produce the tool's most accounting-fluent diagnostics (`price-disagrees`,
`ambiguous-lots`). The weak points are the rules that *refer to another declaration*
(`closed`, `opened`, `holds`, `code`): the message quotes the rule but does not show it.
And two more silent or misleading cases (43, 45).

### 32 two-remainders — grade A
**Mistake.** Two `...` legs.
**Actual** (`32-two-remainders.out`): `two-remainders`, both legs labelled.
**Ideal.** Same as actual, plus the corrective edit.
```text
error[two-remainders]: only one leg can take the remainder
   ╭─[32-two-remainders.ax:15:3]
   │
13 │ 2026-01-15 acme -> 5_000 USD
14 │   savings   ...
   │   ──────┬──────
   │         ╰── this leg already takes whatever remains
15 │   checking  ...
   │   ──────┬──────
   │         ╰── a second `...`
   │
   = help: give one of the legs an amount, for example 800.00 USD of the 5,000.00 USD
15 +   checking  4_200 USD
```

### 33 many-to-many — grade B
**Mistake.** Both header sides named, and legs as well.
**Actual** (`33-many-to-many.out`): `many-to-many`, labels on the header arrow and the first
leg, "write two transactions".
**Verdict.** Correct; the fix is prose. The tool knows the legs and can write both
transactions.
**Ideal.**
```text
error[many-to-many]: a transaction names one side and lists the other; this one names both
   ╭─[33-many-to-many.ax:15:3]
   │
14 │ 2026-01-16 checking -> savings 1_000 USD
   │            ────────────────┬───────────
   │                            ╰── both places are already named here
15 │   savings   600 USD
   │   ────────┬────────
   │           ╰── legs belong under a header with one side missing
16 │   checking  400 USD
   │
   = help: split it into two transactions
14 + 2026-01-16 checking -> savings 600 USD
15 + 2026-01-16 checking -> savings 400 USD
```

### 34 split-short — grade B
**Mistake.** Legs sum to 4,800.00 USD under a 5,000.00 USD header, no `...` leg.
**Actual** (`34-split-short.out`): `split-short`: "the legs add up to 4,800.00 USD, but the total
is 5,000.00 USD", label on the header amount, help "add a `...` leg to take the remaining
200.00 USD".
**Verdict.** Good arithmetic, good accounting language, correct help. The underline is
on the header only; the legs are what to fix.
**Ideal.**
```text
error[split-short]: the legs add up to 4,800.00 USD, but the paycheck is 5,000.00 USD
   ╭─[34-split-short.ax:14:12]
   │
14 │ 2026-01-15 acme -> 5_000 USD
   │            ──┬─    ────┬───
   │              │         ╰── 5,000.00 USD
   │              ╰── acme pays
15 │   savings   800 USD
16 │   checking  4_000 USD
   │   ──────────┬────────
   │             ╰── legs: 800.00 + 4,000.00 = 4,800.00 USD; 200.00 USD is unassigned
   │
   = help: give the 200.00 USD a home: add a leg
16 +   expenses/taxes  200 USD
   = help: or let one leg take the remainder
16 +   checking  ...
```

### 35 split-over — grade C
**Mistake.** Legs sum to 5,500.00 USD under a 5,000.00 USD header.
**Actual** (`35-split-over.out`): `split-over`: "the legs use 5,500.00 USD, but the total is
5,000.00 USD", note "nothing is left for the remainder, or the legs already exceed the
total", help "lower a leg, or raise the total".
**Verdict.** The number is right. The note is hedged ("or…") because the message is
written for two situations at once; the tool knows there is no `...` leg here. The help
does not give the amount to lower by (500.00 USD).
**Ideal.**
```text
error[split-over]: the legs add up to 5,500.00 USD, which is 500.00 USD more than the paycheck
   ╭─[35-split-over.ax:13:12]
   │
13 │ 2026-01-15 acme -> 5_000 USD
   │            ──┬─    ────┬───
   │              │         ╰── 5,000.00 USD
   │              ╰── acme pays
14 │   savings   3_000 USD
15 │   checking  2_500 USD
   │   ──────────┬─────────
   │             ╰── legs: 3,000.00 + 2,500.00 = 5,500.00 USD
   │
   = help: lower a leg by 500.00 USD, or raise the paycheck to 5_500 USD
13 + 2026-01-15 acme -> 5_500 USD
```

### 36 price-disagrees — grade A
**Mistake.** `2_000 USD -> 7 VTI @ 285.70 USD` (7 × 285.70 = 1,999.90 USD).
**Actual** (`36-price-disagrees.out`): `price-disagrees` with both computations, and a `help`
that writes the fee leg out.
**Verdict.** The model diagnostic: an arithmetic derivation, the implied price, and the
exact leg to write. Nothing to add except the phrase "0.10 USD" in the headline.
**Ideal.**
```text
error[price-disagrees]: 7 VTI at 285.70 USD costs 1,999.90 USD, but 2,000.00 USD leaves checking
   ╭─[36-price-disagrees.ax:13:52]
   │
13 │ 2026-01-22 checking 2_000 USD -> brokerage 7 VTI @ 285.70 USD
   │                               ─┬                   ─────┬────
   │                                │                        ╰── this price
   │                                ╰── 2,000.00 USD for 7 VTI
   │
   = note: 2,000.00 USD ÷ 7 VTI = 285.71428… USD per VTI, which is not the price written
   = help: if the 0.10 USD difference is a fee or a rounding, write it as a leg:
             checking -> 2_000.00 USD
               brokerage 7 VTI @ 285.70 USD
               expenses/fees 0.10 USD
```

### 37 transfer-unequal — grade B
**Mistake.** `checking 500 USD -> savings 450 USD`.
**Actual** (`37-transfer-unequal.out`): `amounts-differ`: "500.00 USD leaves, but 450.00 USD
arrives", note "a transfer keeps its amount", help "write it as its own leg, or state the
amount once".
**Verdict.** Accounting-correct. The help is prose; the exact split is known.
**Ideal.**
```text
error[amounts-differ]: 500.00 USD leaves checking but 450.00 USD arrives in savings
   ╭─[37-transfer-unequal.ax:14:29]
   │
14 │ 2026-01-20 checking 500 USD -> savings 450 USD
   │                     ───┬───            ───┬───
   │                        │                  ╰── arrives
   │                        ╰── leaves
   │
   = note: a transfer between two of your own places keeps its amount; 50.00 USD is unaccounted for
   = help: if it was a fee, write it as a leg
14 + 2026-01-20 checking -> 500 USD
15 +   savings        450 USD
16 +   expenses/fees   50 USD
   = help: if the amount is the same, state it once
14 + 2026-01-20 checking -> savings 500 USD
```

### 38 closed-account — grade C
**Mistake.** A flow dated 2026-03-20 into `old-savings`, which `closed 2026-01-31`.
**Actual** (`38-closed-account.out`): `place-closed`: "`assets/old-savings` closed on
2026-01-31", label "this flow is dated 2026-03-20", help "use another account, or change
`opened` or `closed`".
**Verdict.** Correct facts; the declaration that says `closed` is not shown, the
duration is not stated, and the balance the account closed with is the natural
follow-up question (was it emptied?).
**Ideal.**
```text
error[place-closed]: `assets/old-savings` was closed on 2026-01-31; nothing can move in on 2026-03-20
   ╭─[38-closed-account.ax:12:24]
   │
 6 │ account assets/old-savings : bank
 7 │   opened 2024-01-01
 8 │   closed 2026-01-31
   │          ────┬─────
   │              ╰── closed here
   ⋮
12 │ 2026-03-20 checking -> old-savings 500 USD
   │                        ─────┬─────
   │                             ╰── 48 days after it closed
   │
   = note: it closed holding 0.00 USD
   = help: pay into another account, or, if it reopened, change `closed`
 8 -   closed 2026-01-31
```

### 39 holds-violation — grade C
**Mistake.** 500 USD sent to a wallet that `holds BTC`.
**Actual** (`39-holds-violation.out`): `not-held`: "`assets/wallet` does not hold USD", note "it
holds `BTC`", help "change what the account holds, or use another account".
**Verdict.** True, but the `holds` line is not shown, and the common cause (the user
meant an exchange: buying BTC with USD) is not considered.
**Ideal.**
```text
error[not-held]: `assets/wallet` only holds BTC, and this flow puts 500.00 USD into it
   ╭─[39-holds-violation.ax:14:24]
   │
 9 │ account assets/wallet
10 │   holds BTC
   │         ─┬─
   │          ╰── only BTC may be held here
   ⋮
14 │ 2026-02-10 checking -> wallet 500 USD
   │                        ───┬──
   │                           ╰── USD arrives here
   │
   = help: to buy BTC with these dollars, say how many arrive
14 + 2026-02-10 checking 500 USD -> wallet 0.01 BTC
   = help: to keep dollars there, allow them
10 +   holds BTC, USD
```

### 40 code-forbidden — grade B
**Mistake.** `#check-1041` on a flow from a credit card; `code check-*` allows it only on flows touching a `bank`.
**Actual** (`40-code-forbidden.out`): `code-placement`: "`#check-1041` may not mark this flow",
labels the rule (`code check-*`) and the code, note "it may only mark flows touching `bank`".
**Verdict.** Shows the rule: good. Says `bank` but the reader sees `visa`, so the fix is
one step away: "this flow touches `visa` (credit-card) and `plumber` (expenses/home)".
**Ideal.**
```text
error[code-placement]: `#check-1041` belongs on flows touching a bank account; this flow touches none
   ╭─[40-code-forbidden.ax:18:38]
   │
14 │ code check-*
   │ ──────┬─────
   │       ╰── `check-*` may only mark flows touching a `bank`
   ⋮
18 │ 2026-02-01 visa -> plumber (350 USD) #check-1041
   │            ──┬─                      ─────┬─────
   │              │                            ╰── this code
   │              ╰── `liabilities/visa` is a credit-card, and `plumber` stands for `expenses/food`
   │
   = help: a check is paid from a bank account
18 + 2026-02-01 checking -> plumber (350 USD) #check-1041
```

### 41 before-opened — grade C
**Mistake.** A flow into `savings` dated 2026-02-14; the account `opened 2026-03-01`.
**Actual** (`41-before-opened.out`): `place-not-open`: "`assets/savings` opens on 2026-03-01", label
"this flow is dated 2026-02-14".
**Verdict.** Same as 38: correct facts, missing the declaration, and "opens on" reads as
a statement rather than an error.
**Ideal.**
```text
error[place-not-open]: `assets/savings` opened on 2026-03-01; this flow is 15 days earlier
   ╭─[41-before-opened.ax:11:24]
   │
 6 │ account assets/savings : bank
 7 │   opened 2026-03-01
   │          ────┬─────
   │              ╰── opens here
   ⋮
11 │ 2026-02-14 checking -> savings 500 USD
   │                        ───┬───
   │                           ╰── before it opened
   │
   = help: move the flow to 2026-03-01 or later, or change `opened`
 7 -   opened 2026-03-01
 7 +   opened 2026-02-14
```

### 42 self-flow — grade A
**Mistake.** `checking -> checking`.
**Actual** (`42-self-flow.out`): `self-flow`, both ends labelled.
**Ideal.** Same as actual.
```text
error[self-flow]: `assets/checking` cannot pay itself
  ╭─[42-self-flow.ax:9:12]
  │
9 │ 2026-02-14 checking -> checking 500 USD
  │            ────┬───    ────┬───
  │                │           ╰── and the target
  │                ╰── the source
  │
  = help: a flow moves value between two places, or between two commodities of one
```

### 43 exchange-no-price — grade D
**Mistake.** `checking -> brokerage 7 VTI` with no USD amount and no `@` price.
**Actual** (`43-exchange-no-price.out`): two errors: `insufficient-holding: assets/checking does
not hold 7 VTI` and `no-price: cannot check overdraft: no price for VTI in USD…`.
**Verdict.** The tool reads the line as "move 7 VTI out of checking", then complains
that checking has none (misdiagnosis), then complains about a price for the overdraft
law (cascade). What the user did is much simpler: bought shares and forgot to say what
they cost.
**Ideal.**
```text
error[exchange-without-cost]: this moves 7 VTI out of checking, which holds only USD
   ╭─[43-exchange-no-price.ax:13:1]
   │
13 │ 2026-01-22 checking -> brokerage 7 VTI
   │                        ─────────┬─────
   │                                 ╰── the amount is in VTI; the source holds USD
   │
   = note: to buy shares say what they cost, in USD or as a price
   = help: state the cost
13 + 2026-01-22 checking 2_000 USD -> brokerage 7 VTI
   = help: or give the price
13 + 2026-01-22 checking -> brokerage 7 VTI @ 285.70 USD
```

### 44 overdraft — grade C
**Mistake.** Rent paid from `checking` (800.00 USD) while `savings` holds 9,000.00 USD.
**Actual** (`44-overdraft.out`): `warning[law]: balance below zero`, the flow, the std law
line with value "-1,000.00 USD", and the law's doc text as a note.
**Verdict.** The plumbing is right and the *note* is good ("look for a pending payment or
a missing deposit"). The headline is the law's message string; it does not say which
account, by how much, or that another account could cover it.
**Ideal.**
```text
warning[overdraft]: checking would go to -1,000.00 USD after this payment
   ╭─[44-overdraft.ax:15:1]
   │
15 │ 2026-01-01 checking -> landlord 1_800 USD
   │ ───────────┬─────────────────────────────
   │            ╰── 800.00 USD − 1,800.00 USD = −1,000.00 USD in `assets/checking`
   │
   ├─[std.ax:26:10] (built in: law `overdraft` of kind `bank`)
   │
26 │     warn balance >= empty "balance below zero"
   │          ───┬───
   │             ╰── −1,000.00 USD
   │
   = note: the bank lends you the difference, usually for a fee; look for a pending payment or a
           missing deposit before assuming it is real
   = note: `assets/savings` holds 9,000.00 USD
   = help: move 1,000.00 USD in first
15 + 2026-01-01 savings -> checking 1_000 USD
```

### 45 typo-full-path — grade F
**Mistake.** `expenses/grocries` (a full path under a root) for `expenses/groceries`.
**Actual** (`45-typo-full-path.out`): `✓ 3 flows · 5 places … net worth 818.70 USD`. **No diagnostic.**
**Verdict.** The language rule "a full path under a root opens a place" turns every
typo in a full path into a new account. The summary line shows one more place than the
user declared, and nothing else. This is the single most dangerous silent failure: the
books balance, the budget for `groceries` never sees the spending, the report is wrong.
**Ideal.**
```text
warning[new-place]: this opens a new account `expenses/grocries`
   ╭─[45-typo-full-path.ax:12:24]
   │
 7 │ account expenses/groceries
   │         ─────────┬────────
   │                  ╰── declared here
   ⋮
12 │ 2026-01-15 checking -> expenses/grocries 97.10 USD
   │                        ────────┬────────
   │                                ╰── not declared; one letter from `expenses/groceries`
   │
   = help: if it is a typo, correct it
12 + 2026-01-15 checking -> expenses/groceries 97.10 USD
   = help: if it is a new account, declare it (this silences the warning)
 8 + account expenses/grocries
```
(Rule: opening by full path warns whenever a declared place is within edit distance 2; the
summary line should list "1 place opened by use").

### 46 unknown-place-in-leg — grade B
**Mistake.** `taxes/federl` in a leg.
**Actual** (`46-unknown-place-in-leg.out`): `unknown-place`, did-you-mean `taxes/federal` + the
generic "open a new account" help.
**Verdict.** Same as 21: right suggestion, plus a noise help, plus an invented example.
**Ideal.**
```text
error[unknown-place]: there is no place `taxes/federl`
   ╭─[46-unknown-place-in-leg.ax:15:3]
   │
14 │ 2026-01-15 acme -> 5_000 USD
15 │   taxes/federl  900 USD
   │   ──────┬─────
   │         ╰── not declared
   │
   = help: did you mean `taxes/federal`?
15 +   taxes/federal  900 USD
```

### 47 unknown-place-in-assertion — grade B
**Mistake.** `2026-01-31 chekcing = 915.80 USD`.
**Actual** (`47-unknown-place-in-assertion.out`): `unknown-place`, did-you-mean `checking` +
the generic help.
**Ideal.**
```text
error[unknown-place]: there is no place `chekcing`
   ╭─[47-unknown-place-in-assertion.ax:11:12]
   │
11 │ 2026-01-31 chekcing = 915.80 USD
   │            ────┬───
   │                ╰── not declared
   │
   = help: did you mean `checking`?
11 + 2026-01-31 checking = 915.80 USD
```


## 6. The journal: assertions, events, lots, inference (cases 48–60)

A failed balance assertion is the moment a bookkeeper most needs a *reasoning* tool.
Today the diagnostic states the gap and lists every flow since the last assertion. It
does not attempt the four explanations that account for nearly every real gap: a
**transposition** (the gap is divisible by 9), a **reversed flow** (the gap is twice a
flow), a **missed transaction** (the gap is a plausible amount), and a **sign** error.
It also offers `!` (accept the gap) as its only help.

### 48 assert-transposed — grade C
**Mistake.** The statement says 3,015.80 USD; the assertion transposes two digits
(`3_051.80`).
**Actual** (`48-assert-transposed.out`): `assertion`: "assets/checking holds 3,015.80 USD, not
3,051.80 USD", every flow since the opening listed with its signed amount, label "36.00 USD
missing: the ledger holds less than this" (the wording is backwards: the *assertion* is 36.00
higher than the ledger), help "if the gap is a genuine externality … accept it explicitly"
with `!` appended.
**Verdict.** Numbers right; direction of the label wrong; the suggested edit hides a
typo behind an accepted gap: the *worst* first help.
**Ideal.**
```text
error[assertion]: checking holds 3,015.80 USD on 2026-01-31, not the 3,051.80 USD you wrote
   ╭─[48-assert-transposed.ax:21:1]
   │
21 │ 2026-01-31 checking = 3_051.80 USD
   │                       ─────┬─────
   │                            ╰── 36.00 USD more than the ledger holds
   │
   = note: 36.00 is divisible by 9, which is what transposing two digits does: 3,015.80 ↔ 3,051.80
   = note: 1,000.00 opening + 3,900.00 acme − 1,800.00 landlord − 84.20 food = 3,015.80 USD
           (4 flows since the start; `axiom register checking` lists them)
   = help: if the statement says 3,015.80 USD, fix the digits
21 + 2026-01-31 checking = 3_015.80 USD
   = help: if 36.00 USD really left the account, record what it was
21 + 2026-01-30 checking -> ? 36 USD
```

### 49 assert-missed-txn — grade C
**Mistake.** A 45.00 USD payment was never entered; the bank shows 2,970.80 USD, the
ledger 3,015.80 USD.
**Actual** (`49-assert-missed-txn.out`): `assertion`: "holds 3,015.80 USD, not 2,970.80 USD",
flows listed, label "45.00 USD too much: the ledger holds more than this", help `!`.
**Verdict.** Right numbers, no reasoning; the `!` is offered first and is a one-character
way to bury a missing transaction.
**Ideal.**
```text
error[assertion]: checking holds 3,015.80 USD on 2026-01-31, but the bank says 2,970.80 USD
   ╭─[49-assert-missed-txn.ax:22:1]
   │
22 │ 2026-01-31 checking = 2_970.80 USD
   │                       ─────┬─────
   │                            ╰── 45.00 USD less than the ledger holds
   │
   = note: 1,000.00 opening + 3,900.00 acme − 1,800.00 landlord − 84.20 food = 3,015.80 USD
   = note: 45.00 USD is not a multiple of 9 (not a transposition) and no single flow is 22.50 USD
           (not a reversal); most likely a payment is missing
   = help: record the missing payment
22 + 2026-01-30 checking -> ? 45 USD
   = help: or accept the gap: it is booked as unexplained spending and shown in every report
22 + 2026-01-31 checking = 2_970.80 USD !
```

### 50 assert-card-sign — grade C
**Mistake.** The card statement shows 452.00 USD owed; the assertion is written `-452.00 USD`.
**Actual** (`50-assert-card-sign.out`): `negative-amount`: "amounts carry no sign", note "the arrow gives
the direction…", help "remove the sign" with the edit `visa = 452.00 USD`.
**Verdict.** The fix is right, but the note is about `->` and the user wrote `=`; the
one thing worth saying, that an assertion is written in the place's *display* sign,
is missing.
**Ideal.**
```text
error[negative-amount]: an assertion is written the way the statement shows it: owed amounts have no sign
   ╭─[50-assert-card-sign.ax:14:19]
   │
14 │ 2026-01-31 visa = -452.00 USD
   │                   ┬
   │                   ╰── remove the `-`
   │
   = note: `visa` is a liability: its balance is what you owe, so 452.00 USD owed is `452.00 USD`
   = note: with the sign removed this assertion holds: 300.00 + 152.00 = 452.00 USD owed
   = help: write what the statement shows
14 + 2026-01-31 visa = 452.00 USD
```

### 51 settle-unknown-code — grade A
**Mistake.** `#check-1014 settled` for `#check-1041`.
**Actual** (`51-settle-unknown-code.out`): `unknown-code`, "did you mean `#check-1041`?" with the edit.
**Ideal.** Same as actual; add the transposition hint (both are codes 1041/1014).
```text
error[unknown-code]: no transaction is marked `#check-1014`
   ╭─[51-settle-unknown-code.ax:14:12]
   │
14 │ 2026-02-06 #check-1014 settled
   │            ─────┬─────
   │                 ╰── nothing carries this code
   │
   = note: an event names the transaction it changes by its code
   = help: did you mean `#check-1041` (2026-02-01, checking → plumber, 350.00 USD, pending)?
14 + 2026-02-06 #check-1041 settled
```

### 52 void-twice — grade B
**Mistake.** `#check-1041 void` on 2026-02-10 and again on 2026-02-12.
**Actual** (`52-void-twice.out`): `repeated-event`: "`#check-1041` already has an event", labels the flow
and the second void ("this second one is ignored for the flow below"), help about
bounced payments.
**Verdict.** Points at the right line; doesn't show the *first* event, and the label
says "the flow below" although the flow is above.
**Ideal.**
```text
error[repeated-event]: `#check-1041` was already voided on 2026-02-10
   ╭─[52-void-twice.ax:15:1]
   │
13 │ 2026-02-01 checking -> plumber (350 USD) #check-1041
   │ ──────────────────────────┬─────────────────────────
   │                           ╰── written, pending
14 │ 2026-02-10 #check-1041 void
   │ ──────────────┬─────────────
   │               ╰── voided: it never happened
15 │ 2026-02-12 #check-1041 void
   │ ──────────────┬─────────────
   │               ╰── voided again
   │
   = help: delete the second line
   = note: to reverse a payment that did clear, write a new flow the other way
```

### 53 sell-more-than-held — grade B
**Mistake.** Selling 10 VTI with 7 held.
**Actual** (`53-sell-more-than-held.out`): `insufficient-holding`: "assets/brokerage does not hold 10 VTI",
"3 VTI more than assets/brokerage has", note "holds 7 VTI", help "record the purchase
before this flow…; the missing amount is left as a negative balance".
**Verdict.** Accurate and accounting-flavoured. Missing: the holding's history (the
one purchase) and the likely fix (correct the quantity).
**Ideal.**
```text
error[insufficient-holding]: selling 10 VTI, but brokerage holds 7 VTI
   ╭─[53-sell-more-than-held.ax:14:1]
   │
13 │ 2026-01-22 checking 2_000 USD -> brokerage 7 VTI
   │ ──────────────────────────────┬─────────────────
   │                               ╰── the only purchase: 7 VTI, basis 2,000.00 USD
14 │ 2026-03-02 brokerage 10 VTI -> checking 3_050 USD
   │ ──────────────────────────┬───────────────────────
   │                           ╰── 3 VTI more than are held
   │
   = note: the 3 VTI shortfall is carried as a negative holding, so later lines still add up
   = help: if you sold all 7, correct the quantity
14 + 2026-03-02 brokerage 7 VTI -> checking 3_050 USD
   = help: if there was another purchase, record it before 2026-03-02
```

### 54 ambiguous-lot-sale — grade A
**Mistake.** A sale from an account with two different lots and no lot policy.
**Actual** (`54-ambiguous-lot-sale.out`): `ambiguous-lots` labelling both purchases with their basis,
"from it alone the gain is …", a note that FIFO was used so later lines stay consistent,
and a help naming the selector, the policy and the account property.
**Verdict.** A model diagnostic: nothing is guessed, and the tax consequence of each
choice is visible. One improvement: show the *whole-sale* gain under each policy, since
the sale here spans both lots.
**Ideal.**
```text
error[ambiguous-lots]: selling 8 VTI: brokerage holds 2 lots that differ and no policy says which goes first
   ╭─[54-ambiguous-lot-sale.ax:15:1]
   │
13 │ 2026-01-22 checking 2_000 USD -> brokerage 7 VTI
   │ ────────────────────────┬───────────────────────
   │                         ╰── lot A: 7 VTI, basis 2,000.00 USD (285.71 per share)
14 │ 2026-02-18 checking 1_500 USD -> brokerage 5 VTI
   │ ────────────────────────┬───────────────────────
   │                         ╰── lot B: 5 VTI, basis 1,500.00 USD (300.00 per share)
15 │ 2026-03-02 brokerage 8 VTI -> checking 2_400 USD
   │ ────────────────────────┬───────────────────────
   │                         ╰── proceeds 2,400.00 USD (300.00 per share)
   │
   = note: the gain this sale realizes depends on the order:
             fifo   7 from A + 1 from B:   100.00 USD
             lifo   5 from B + 3 from A:    42.86 USD
             hifo   5 from B + 3 from A:    42.86 USD
   = note: the sale is booked FIFO for now so the lines after it stay consistent
   = help: name the lots, or a policy, on the sale
15 + 2026-03-02 brokerage[hifo] 8 VTI -> checking 2_400 USD
   = help: or give the account a policy for every sale
 5 +   select hifo
```

### 55 infer-no-assertion — grade B
**Mistake.** `checking -> cash ? USD` with no assertion after it.
**Actual** (`55-infer-no-assertion.out`): `cannot-infer`: "cannot infer the amount of this
flow", **two helps**: assert `cash`, or assert `checking`.
**Verdict.** Correct diagnosis and pointer. Two equal-weight helps make the user choose
the wrong one half the time: a statement exists for the bank account, not for the wallet.
**Ideal.**
```text
error[cannot-infer]: the amount of this withdrawal is unknown, and no later balance can solve it
   ╭─[55-infer-no-assertion.ax:10:1]
   │
10 │ 2026-02-08 checking -> cash ? USD
   │                             ┬
   │                             ╰── `?` is solved from a balance you assert afterwards
   │
   = help: assert what the bank statement shows for `checking` after this date, and the amount is worked out
10 + 2026-02-28 checking = 900 USD          ← the statement balance
   = note: `assets/cash` has no statement to assert against; asserting it would work too
```

### 56 infer-two-unknowns — grade D
**Mistake.** Two `?` amounts between the same two assertions on `checking`.
**Actual** (`56-infer-two-unknowns.out`): **three errors**: two `cannot-infer` (one per flow, each
labelled with the other) and a third `assertion: holds 1,000.00 USD, not 800.00 USD` whose
flows are printed as **`-0.00 USD`**.
**Verdict.** Three errors for one problem; the third is a cascade (the unsolved amounts
are booked as zero, then the assertion fails), and `-0.00 USD` is an implementation
artifact leaking into a message about money.
**Ideal.**
```text
error[cannot-infer]: two unknown amounts between the same two assertions on `checking`
   ╭─[56-infer-two-unknowns.ax:12:1]
   │
11 │ 2026-01-31 checking = 1_000 USD
   │            ───────┬──────────────
   │                   ╰── 1,000.00 USD here
12 │ 2026-02-08 checking -> cash ? USD
   │ ───────────────┬─────────────────
   │                ╰── unknown (1)
13 │ 2026-02-12 checking -> food ? USD
   │ ───────────────┬─────────────────
   │                ╰── unknown (2)
14 │ 2026-02-28 checking = 800 USD
   │            ──────┬──────────────
   │                  ╰── 800.00 USD here: (1) + (2) = 200.00 USD
   │
   = note: one balance gives one equation; two unknowns need one more fact
   = help: write one of the amounts; the other is worked out
12 + 2026-02-08 checking -> cash 80 USD
   = note: the assertion on line 14 is not checked until this is resolved (it is not reported as failing)
```

### 57 settle-before-written — grade F
**Mistake.** `2026-02-06 #check-1041 settled`, for a check written on 2026-02-10.
**Actual** (`57-settle-before-written.out`): `✓ 2 flows … net worth 1,650.00 USD`. **No diagnostic.**
**Verdict.** A check cannot clear before it is written. Silence here means the tool
quietly *rewrites* the user's date: `axiom register checking` shows
`#check-1041 · settled 2026-02-10`, and the 2026-02-06 that was typed appears nowhere.
A typo in either date (02-16 typed as 02-06) is therefore never noticed.
**Ideal.**
```text
error[event-before-flow]: `#check-1041` is settled on 2026-02-06, four days before it was written
   ╭─[57-settle-before-written.ax:14:1]
   │
13 │ 2026-02-10 checking -> plumber (350 USD) #check-1041
   │ ─────────────────────────┬───────────────────────────
   │                          ╰── written on 2026-02-10
14 │ 2026-02-06 #check-1041 settled
   │ ──────────────┬─────────────
   │               ╰── settled on 2026-02-06
   │
   = help: an event happens on or after the flow it changes: settle it on 2026-02-10 or later
14 + 2026-02-12 #check-1041 settled
```

### 58 assert-wrong-commodity — grade C
**Mistake.** `checking = 915.80 EUR` on an account that only ever held USD.
**Actual** (`58-assert-wrong-commodity.out`): `assertion: assets/checking holds 0.00 EUR, not 915.80
EUR`, "915.80 EUR missing", `!` help.
**Verdict.** Technically true; the real fact is that the account never held EUR, and
it holds exactly 915.80 *USD*.
**Ideal.**
```text
error[assertion]: `checking` has never held EUR; it holds 915.80 USD
   ╭─[58-assert-wrong-commodity.ax:11:1]
   │
11 │ 2026-01-31 checking = 915.80 EUR
   │                              ─┬─
   │                               ╰── this ledger's base currency is USD
   │
   = help: assert in USD (915.80 USD is exactly what the ledger holds)
11 + 2026-01-31 checking = 915.80 USD
```

### 59 reversed-card-payment — grade C
**Mistake.** The card payment was written backwards (`visa -> checking 200.00 USD`); the statement
shows 252.00 USD owed.
**Actual** (`59-reversed-card-payment.out`): `assertion: liabilities/visa holds 652.00 USD, not
252.00 USD`, flows listed as `+300.00 USD to expenses/food`, … `+200.00 USD to assets/checking`,
"400.00 USD too much", `!` help.
**Verdict.** The gap (400.00 USD) is exactly twice the payment (200.00 USD) and the
tool does not notice. It is the tell-tale of a reversed flow.
**Ideal.**
```text
error[assertion]: visa owes 652.00 USD on 2026-01-31, but the statement says 252.00 USD
   ╭─[59-reversed-card-payment.ax:15:1]
   │
12 │ 2026-01-08 visa -> food 300.00 USD
   │                         ─── +300.00 owed
13 │ 2026-01-17 visa -> food 152.00 USD
   │                         ─── +152.00 owed
14 │ 2026-01-25 visa -> checking 200.00 USD
   │ ─────────────────────┬───────────────
   │                      ╰── +200.00 owed: this payment charges the card instead of paying it
15 │ 2026-01-31 visa = 252.00 USD
   │ ──────────────┬──────────────
   │               ╰── 400.00 USD less than the ledger says
   │
   = note: the gap is exactly twice this flow (2 × 200.00 USD): it is probably written backwards
   = help: pay the card from checking
14 + 2026-01-25 checking -> visa 200.00 USD
```

### 60 redeclare-std-commodity — grade D
**Mistake.** `commodity USD : currency` in the project file; `std` already declares it.
**Actual** (`60-redeclare-std-commodity.out`): `duplicate-declaration: commodity USD is declared
twice`, **primary location `std.ax:160:11`, labelled "declared again here"**, with the
project's line labelled "first declared here".
**Verdict.** The roles are swapped: the user's declaration is the duplicate, but the
report points into a file the user cannot edit and calls it the offender. The help,
"remove one of the two", is unactionable for the built-in one.
**Ideal.**
```text
error[duplicate-declaration]: `USD` is already declared by the standard library
   ╭─[60-redeclare-std-commodity.ax:6:11]
   │
 6 │ commodity USD : currency
   │           ─┬─
   │            ╰── declared again here
   │
   ├─[std.ax:160:11] (built in)
   │
160 │ commodity USD : currency
   │           ─┬─
   │            ╰── first declared here, with `precision 2` and `name "United States dollar"`
   │
   = help: delete your declaration: USD already has 2 decimals
   = help: to change a built-in property, override the whole system by copying `std.ax` to `systems/std.ax`
```


## 7. Laws: type checking, parameters and the standard systems (cases 61–82)

Type errors are precise and cheap (`type-mismatch` shows both operand types); they lack
only the *fix* (`650` → `650 USD`). The costly problems are the ones where a law from a
built-in system is what fails: the message quotes the law's own text and points into a
file the user cannot edit. A first-time user needs the same information expressed in
*their* file: which of their declarations the law reads, and what to add.

### 61 type-amount-plus-date — grade A
**Mistake.** `total(in, month) + date <= 650 USD`.
**Actual** (`61-type-amount-plus-date.out`): `type-mismatch: cannot add a date to an amount`, labels
"this is an amount" / "this is a date", note "`+` adds two amounts, two numbers or two
spans, or a span to a date".
**Verdict.** Ideal for a type error. (Nit: the note could name the likely intent.)
**Ideal.**
```text
error[type-mismatch]: cannot add a date to an amount
   ╭─[61-type-amount-plus-date.ax:10:29]
   │
10 │     warn total(in, month) + date <= 650 USD "food is over budget"
   │          ────────┬───────   ──┬─
   │                  │            ╰── a date
   │                  ╰── an amount, in USD
   │
   = note: `+` adds two amounts, two numbers or two spans, or a span to a date
```

### 62 type-amount-lt-number — grade B
**Mistake.** `total(in, month) <= 650` (the commodity is missing inside a law).
**Actual** (`62-type-amount-lt-number.out`): `type-mismatch: cannot compare an amount with a number`,
labels on both operands, a note.
**Verdict.** Right; but the intent is unmistakable and the fix is one word.
**Ideal.**
```text
error[type-mismatch]: cannot compare an amount with a number
   ╭─[62-type-amount-lt-number.ax:10:30]
   │
10 │     warn total(in, month) <= 650 "food is over budget"
   │          ────────┬───────    ─┬─
   │                  │            ╰── a number: no commodity
   │                  ╰── an amount, in USD
   │
   = help: give the number a commodity
10 +     warn total(in, month) <= 650 USD "food is over budget"
```

### 63 is-against-non-kind — grade B
**Mistake.** `when amount is wages` (meant `from is wages`).
**Actual** (`63-is-against-non-kind.out`): `type-mismatch: cannot test an amount against a kind`.
**Verdict.** Correct; missing the obvious repair, since the trigger provides `from`.
**Ideal.**
```text
error[type-mismatch]: `is` cannot test an amount against a kind
   ╭─[63-is-against-non-kind.ax:10:20]
   │
10 │     when amount is wages
   │          ───┬──    ──┬──
   │             │        ╰── a kind of income place
   │             ╰── an amount: a quantity of money, not a place
   │
   = note: `is` asks what a place, entity or commodity is; to ask where the money came from, test `from`
   = help: did you mean `from`?
10 +     when from is wages
```

### 64 unknown-function — grade B
**Mistake.** `sum(in, month)`.
**Actual** (`64-unknown-function.out`): `unknown-function: there is no function sum`, note listing
the eight functions.
**Verdict.** The list is good; there is no nearest-name (`sum` is far from `total`), so
the tool could match on meaning: `sum`/`add`/`count` → `total`.
**Ideal.**
```text
error[unknown-function]: there is no function `sum`
   ╭─[64-unknown-function.ax:10:10]
   │
10 │     warn sum(in, month) <= 650 USD "food is over budget"
   │          ─┬─
   │           ╰── not a function
   │
   = note: functions: `total`, `tally`, `min`, `max`, `abs`, `progressive`, `value`, `date`
   = help: to add up what came in this month, use `total(in, month)`
10 +     warn total(in, month) <= 650 USD "food is over budget"
```

### 65 wrong-arity — grade C
**Mistake.** `total(month)`.
**Actual** (`65-wrong-arity.out`): `call-arity: `total` takes 2 or 3 arguments, but 1 were given`,
label "wrong number of arguments", no help.
**Verdict.** Grammar ("1 were given"), and the signature is not shown, so the user has
to look it up.
**Ideal.**
```text
error[call-arity]: `total` needs a direction and a window; only `month` was given
   ╭─[65-wrong-arity.ax:10:10]
   │
10 │     warn total(month) <= 650 USD "food is over budget"
   │          ──────┬─────
   │                ╰── total(DIRECTION, WINDOW): `in` or `out`, then `month`, `year` or `ever`
   │
   = help: `in` counts what arrives
10 +     warn total(in, month) <= 650 USD "food is over budget"
```

### 66 unknown-field — grade A
**Mistake.** `owner.aeg`.
**Actual** (`66-unknown-field.out`): `unknown-field`, lists the fields, did-you-mean `age` with the edit.
**Ideal.** As actual, listing only the fields of an *entity of kind person* first.
```text
error[unknown-field]: an entity has no field `aeg`
   ╭─[66-unknown-field.ax:17:19]
   │
17 │     require owner.aeg >= 18y "owner is a minor"
   │             ──┬── ─┬─
   │               │    ╰── no such field
   │               ╰── `me`, a person
   │
   = note: a person has `age`, `born`, `filing`, `kind`, `owner`
   = help: did you mean `age`?
17 +     require owner.age >= 18y "owner is a minor"
```

### 67 no-param-row — grade D
**Mistake.** A 2023 paycheck; the standard 401(k), income-tax and deduction figures
start in 2024.
**Actual** (`67-no-param-row.out`): **four errors**: `cannot check deferral-limit: limit has no
row for 2023-01-13` (pointing into `us/401k.ax`), then `standard-deduction has no row for
2023-12-31`, and two `ordinary has no row …` (all in `us.ax`), each with the help "add a row
to `param …` that starts on or before that day".
**Verdict.** One root cause (the built-in tables start in 2024), four errors, three
files. The help asks the user to edit a built-in file. A first-time user has no idea
how, or that they can override systems at all.
**Ideal.**
```text
error[no-param-row]: the built-in tax and 401(k) figures start in 2024; this ledger has flows in 2023
   ╭─[67-no-param-row.ax:20:3]
   │
19 │ 2023-01-13 acme -> 6_000 USD
   │ ───────────────────────────
20 │   retirement  800 USD
   │   ─────────┬─────────
   │            ╰── a 401(k) deferral in 2023: its limit is needed
   │
   ├─[us/401k.ax:82:7] (built in)
   │
   │ param limit
   │       ──┬──
   │         ╰── first row is 2024 (23,000.00 USD)
   │
   = note: 3 more figures start in 2024 (`standard-deduction`, `ordinary`, `capital-gains`), so no 2023
           check or 2023 tax can run; 4 checks were skipped, none reported separately
   = help: start the ledger in 2024, or add 2023 rows: copy `us.ax` and `us/401k.ax` to `systems/`
           and write the rows there (`axiom` uses your copy)
```

### 68 gain-under-on-in — grade B
**Mistake.** A law that fires `on in` reads `gain`.
**Actual** (`68-gain-under-on-in.out`): `law-variable: gain is not available in this law`, label "this
law's trigger does not provide `gain`", help "`gain` is provided by `on gain` laws".
**Verdict.** Diagnosis and hint are right. It does not point at the trigger line the
user would change, and does not show the edit.
**Ideal.**
```text
error[law-variable]: `gain` does not exist when money arrives; this law fires `on in`
   ╭─[68-gain-under-on-in.ax:12:10]
   │
10 │   law gains-note
11 │     on in
   │        ─┬
   │         ╰── `on in` provides: amount, from, to, payee, date, year, month, self, owner
12 │     warn gain <= 1_000 USD "large gain"
   │          ──┬─
   │            ╰── `gain` is realized when parcels are sold
   │
   = help: fire when a sale realizes a gain
11 +     on gain
```

### 69 401k-two-employers — grade B
**Mistake.** A job change: 15,000.00 USD deferred at Acme and 12,000.00 USD at NewCo in
2026 (limit 24,500.00 USD per *person*).
**Actual** (`69-401k-two-employers.out`): `error[law]: 401(k) deferrals over the yearly limit`, the
flow, the std law with a power-assert tree (`27,000.00 USD`, `false`, `24,500.00 USD`, `2026`,
`24,500.00 USD`, `empty`), a 5-line note (doc comment), help "at most 9,500.00 USD more
can count".
**Verdict.** One of the best diagnostics in the tool (the help number is right). What is
missing: *which earlier flow* brought the tally to 15,000.00 USD; `empty` printed where a
reader expects `0.00 USD`; the law's own doc comment is a generic paragraph including
instructions ("Ask payroll…") that belong in a `help:`.
**Ideal.**
```text
error[deferral-limit]: 27,000.00 USD deferred in 2026 against a limit of 24,500.00 USD: over by 2,500.00 USD
   ╭─[69-401k-two-employers.ax:32:3]
   │
26 │   retirement  15_000 USD
   │   ─────────┬────────────
   │            ╰── 2026-05-29: 15,000.00 USD deferred (acme → retirement)
   ⋮
32 │   retirement-newco  12_000 USD
   │   ────────────────┬─────────────
   │                   ╰── 2026-06-26: 12,000.00 USD more (newco → retirement-newco): 27,000.00 USD in all
   │
   ├─[us/401k.ax:33:13] (built in: law `deferral-limit`, IRC §402(g))
   │
33 │     require tally(elective-deferrals) <= limit[year] + extra
   │                    27,000.00 USD          24,500.00 USD + 0.00 USD
   │                                           (2026)          (no catch-up: me is 37)
   │
   = note: the cap is per person and year, across every plan (IRC §402(g)); employer contributions do not count
   = help: lower this deferral to at most 9,500.00 USD; any excess must be returned by the plan
           before April 15, 2027 or it is taxed twice
32 +   retirement-newco  9_500 USD
```

### 70 grant-wrong-purpose — grade B
**Mistake.** Scholarship money (purpose: education) pays the rent.
**Actual** (`70-grant-wrong-purpose.out`): `error[law]: grant money spent outside its purpose`,
the flow ("1,200.00 USD of restricted money leaving assets/checking"), the std law line
with `expenses/rent`, `false`, `scholarship`, `education`, and a 3-line note.
**Verdict.** Complete and accountant-friendly. The power-assert row (`to is self.purpose`)
prints four values whose meaning the user must reconstruct.
**Ideal.**
```text
error[grant-purpose]: 1,200.00 USD of the scholarship pays rent, and it may only pay for education
   ╭─[70-grant-wrong-purpose.ax:18:1]
   │
16 │ entity scholarship : grant
   │        ────┬──────
   │            ╰── purpose: education; until 2026-12-31
17 │ 2026-04-01 scholarship -> checking 1_200 USD
   │ ───────────────────────┬─────────────────────
   │                        ╰── 1,200.00 USD, tied to `scholarship`, lands in checking
18 │ 2026-04-02 checking -> landlord 1_200 USD
   │ ────────────────────┬───────────────────
   │                     ╰── this payment goes to `expenses/rent` (not education); it spends the tied money
   │
   = note: money from a grant stays tied to it after it lands in your account
   = help: pay the rent from another account, or, if the funder agreed, change `purpose` on `scholarship`
```

### 71 early-withdrawal — grade F
**Mistake.** A 10,000.00 USD withdrawal from a 401(k) at age 37.
**Actual** (`71-early-withdrawal.out`): `✓ 2 flows · 4 places · 4 laws enforced · net worth 40,000.00 USD`.
No diagnostic, no notice. (The cost, a 1,000.00 USD penalty and 10,000.00 USD of income, is
only visible in `axiom tax 2026`; see §10.)
**Verdict.** "Priced, not failed" is the right semantics, but `check` is where a user
looks, and a quiet check reads as approval. The README of the household example says
`check` reports these; the golden output does not.
**Ideal.**
```text
note[priced]: taking 10,000.00 USD out of the 401(k) at 37 costs 1,000.00 USD in penalty and counts as income
   ╭─[71-early-withdrawal.ax:20:1]
   │
20 │ 2026-02-02 retirement -> checking 10_000 USD
   │ ────────────────────────┬────────────────────
   │                         ╰── 10,000.00 USD of pre-tax money leaves a tax-deferred account
   │
   ├─[us/401k.ax:64:5] (built in: law `early-withdrawal`, IRC §72(t))
   │
64 │     require owner.age >= 59y6m else owe 10% * gain to irs as early-withdrawal-penalty
   │                    ────┬────         ─────┬────
   │                        │                   ╰── 10% of 10,000.00 USD = 1,000.00 USD
   │                        ╰── me is 37y9m21d (born 1988-04-12); 59y6m is 2047-10-12
   │
   = note: owed to irs by 2026-02-02 as `early-withdrawal-penalty`; the withdrawal also counts as a
           10,000.00 USD distribution (ordinary income for 2026): see `axiom tax 2026`
   = help: if an exception applies (disability, separation from service at 55+, …), keep the
           withdrawal and mark it with the reason
20 + 2026-02-02 retirement -> checking 10_000 USD ! "IRC §72(t)(2)(A)(iii) disability"
```

### 72 law-no-trigger — grade C
**Mistake.** A `law` with no trigger line.
**Actual** (`72-law-no-trigger.out`): `missing-trigger: this law has no trigger`, label "when does it
apply?", help listing triggers; **then a second error** `unknown-place: there is no place
food` on the flow (cascade).
**Ideal.**
```text
error[missing-trigger]: this law does not say when it applies
   ╭─[72-law-no-trigger.ax:8:3]
   │
 7 │ account expenses/food
 8 │   law monthly-cap
   │   ───────┬───────
   │          ╰── the first line of a law is its trigger
   │
   = help: a budget fires when money comes in
 8 +     on in
   = note: triggers: `on in`, `on out`, `on gain`, `on spend`, `each month`, `each year`, `by DATE`, `always`
   = note: `expenses/food` is still declared; its uses are not reported
```

### 73 unknown-trigger — grade C
**Mistake.** `on deposit`.
**Actual** (`73-unknown-trigger.out`): `unknown-trigger: unknown trigger deposit`, note "the triggers are
`in`, `out`, `gain` and `spend`", **then the same `unknown-place` cascade**.
**Verdict.** The list omits `each month|year`, `by` and `always` (which is `each`, `by`
and `always` also accepted here?) and has no suggestion (`deposit` means `in`).
**Ideal.**
```text
error[unknown-trigger]: `on deposit` is not a trigger
  ╭─[73-unknown-trigger.ax:9:8]
  │
9 │     on deposit
  │        ───┬───
  │           ╰── not a trigger
  │
  = note: `on in`, `on out`, `on gain`, `on spend`; or `each month`, `each year`, `by DATE`, `always`
  = help: money arriving is `on in`
9 +     on in
```

### 74 unknown-param — grade C
**Mistake.** `cap[year]` with no `param cap`.
**Actual** (`74-unknown-param.out`): `unknown-param: there is no param cap`, label "not a known param",
no help.
**Verdict.** The reader is left to guess whether it is a typo or a missing declaration.
**Ideal.**
```text
error[unknown-param]: there is no param `cap`
   ╭─[74-unknown-param.ax:10:30]
   │
10 │     warn total(in, month) <= cap[year] "food is over budget"
   │                              ─┬─
   │                               ╰── no `param cap` here or in a system this ledger uses
   │
   = note: no params are in scope; `use us/401k` brings `limit`, `catch-up`, `super-catch-up`
   = help: declare it
 8 + param cap
 9 +   2026 650 USD
```

### 75 unknown-tally — grade A
**Mistake.** `tally(wagess)`.
**Actual** (`75-unknown-tally.out`): `unknown-tally: no law counts wagess`, note "`tally(NAME)` reads what
`count … as NAME` lines add up", did-you-mean `wages` with the edit.
**Ideal.** As actual.
```text
error[unknown-tally]: no law counts `wagess`
   ╭─[75-unknown-tally.ax:20:17]
   │
20 │   require tally(wagess) <= 500_000 USD "wages over 500_000 USD"
   │                 ───┬──
   │                    ╰── nothing is tallied under this name
   │
   = note: `tally(NAME)` reads what `count … as NAME` lines add up
   = help: did you mean `wages`?
20 +   require tally(wages) <= 500_000 USD "wages over 500_000 USD"
```

### 76 kind-typo-in-law — grade A
**Mistake.** `when from is wage`.
**Actual** (`76-kind-typo-in-law.out`): `unknown-name: wage means nothing in this law`, explanation of what
a name in a law can be, did-you-mean `wages` with the edit.
**Ideal.** As actual.
```text
error[unknown-name]: `wage` means nothing in this law
   ╭─[76-kind-typo-in-law.ax:18:16]
   │
18 │   when from is wage
   │                ──┬─
   │                  ╰── not a variable, param, kind, place or entity here
   │
   = help: did you mean `wages` (a kind of income place)?
18 +   when from is wages
```

### 77 budget-overspend — grade C
**Mistake.** Groceries and dining go over the 650.00 USD monthly food budget.
**Actual** (`77-budget-overspend.out`): two warnings, each `law budget does not hold for
expenses/food`, the flow, the `budget 650 USD monthly` line labelled with the running
total (`690.00 USD`, then `750.00 USD`), help "at most 170.00 USD more can go in this
month" / "nothing more can go in this month: it is already 40.00 USD over".
**Verdict.** The plumbing is right. The *speech* is not: "law `budget` does not hold" is
the implementation. And **every flow after the crossing repeats the warning**: in a
month with 300 purchases this is 250 near-identical warnings (measured in
`bench/REPORT.md`: 40,320 warnings for 100k flows when budgets are tight).
**Ideal.**
```text
warning[over-budget]: expenses/food is 40.00 USD over its 650.00 USD monthly budget
   ╭─[77-budget-overspend.ax:15:1]
   │
 6 │ account expenses/food
 7 │   budget 650 USD monthly
   │          ───────┬──────
   │                 ╰── 650.00 USD a month
   ⋮
15 │ 2026-01-19 checking -> groceries 210.00 USD
   │ ─────────────────────┬─────────────────────
   │                      ╰── this took January from 480.00 to 690.00 USD
   │
   = note: January so far: groceries 510.00 USD + dining 180.00 USD
   = note: 1 more flow in January is over budget too (2026-01-26 dining 60.00 USD → 750.00 USD)
   = help: 170.00 USD of this flow fitted; the rest is over
```
(Report once per breached window, at the first crossing, with the count and the final
overrun of the window.)

### 78 missing-filing-status — grade D
**Mistake.** `entity me : person` lives in `us`, but has no `filing`; 2025 has ended.
**Actual** (`78-missing-filing-status.out`): **three errors**, all `unset-property: cannot check
federal-income-tax: filing is not set`, all located inside `us.ax` (lines 136, 137, 138),
each with the help "give it a value where the thing is declared: `filing …`".
**Verdict.** One missing line of the user's file, three errors, none of them pointing
at the user's file. The user has never seen `us.ax`.
**Ideal.**
```text
error[unset-property]: `filing` is not set on `me`, so the 2025 federal income tax cannot be worked out
   ╭─[78-missing-filing-status.ax:6:8]
   │
 6 │ entity me : person
   │        ──┬─
   │          ╰── `me` lives in `us`, whose tax laws need a filing status
   │
   ├─[us.ax:128:44] (built in: law `federal-income-tax`)
   │
128 │   let deduction = standard-deduction[year, owner.filing]
   │                                            ────┬──────
   │                                                ╰── read for `me` on 2025-12-31 (3 places in the law read it)
   │
   = help: add the filing status: `single` or `joint`
 8 +   filing single
```

### 79 missing-born — grade C
**Mistake.** `born` missing; the 401(k) deferral law needs the owner's age.
**Actual** (`79-missing-born.out`): `unset-property: cannot check deferral-limit: born is not set`,
the flow, the std line with the underline on `owner.age`'s `.age`, help "give it a value
where the thing is declared: `born …`". In a probe whose paychecks span a year end, a second error for the same missing `born`
(from `required-minimum-distribution`, located in `us/401k.ax:73`) appeared.
**Verdict.** The right diagnosis; the primary label is the paycheck, the *fix* is in the
`entity` block, and the second error is the same root cause.
**Ideal.**
```text
error[unset-property]: `born` is not set on `me`; the 401(k) limit depends on age
   ╭─[79-missing-born.ax:6:8]
   │
 6 │ entity me : person
   │        ──┬─
   │          ╰── no `born`
   │
   ├─[us/401k.ax:32:20] (built in: law `deferral-limit`)
   │
32 │     let extra = if owner.age >= 60y and owner.age < 64y then …
   │                    ────┬────
   │                        ╰── `owner.age` counts from `born`; asked because of the 2026-01-15 deferral (line 18)
   │
   = note: `required-minimum-distribution` needs it too
   = help: add your date of birth
 7 +   born 1988-04-12
```

### 80 unknown-name-in-law — grade A
**Mistake.** `balanse` for `balance`.
**Actual** (`80-unknown-name-in-law.out`): `unknown-name`, explanation, did-you-mean `balance` with the edit.
**Ideal.** As actual.
```text
error[unknown-name]: `balanse` means nothing in this law
   ╭─[80-unknown-name-in-law.ax:10:13]
   │
10 │     require balanse >= empty "food cannot be negative"
   │             ───┬───
   │                ╰── not a variable, param, kind, place or entity here
   │
   = note: a law that fires `always` can read: balance, date, self, owner, year, month
   = help: did you mean `balance`?
10 +     require balance >= empty "food cannot be negative"
```

### 81 incomplete-if — grade C
**Mistake.** `let cap = if date >= 2026-01-01 then 650 USD` (no `else`).
**Actual** (`81-incomplete-if.out`): `incomplete-conditional: expected else, found the end of the
line`, note about the form, **then the `unknown-place` cascade**.
**Ideal.**
```text
error[incomplete-conditional]: this `if` has no `else`
   ╭─[81-incomplete-if.ax:10:49]
   │
10 │     let cap = if date >= 2026-01-01 then 650 USD
   │                                                 ┬
   │                                                 ╰── `else …` is expected here
   │
   = note: a conditional is written `if CONDITION then A else B`
   = help: say what applies before 2026-01-01
10 +     let cap = if date >= 2026-01-01 then 650 USD else empty
```

### 82 relaxed-keyword — grade A (control)
**Situation.** `relaxed` in the file demotes the overdraft to a warning. Not a mistake.
**Actual** (`82-relaxed-keyword.out`): the warning plus the note "shown as a warning because the
book is `relaxed`".
**Verdict.** Ideal. (The summary line should say "1 warning, 1 relaxed".)
```text
warning[overdraft]: checking would go to -1,000.00 USD after this payment (shown as a warning because the ledger is `relaxed`)
```


## 8. More parsing, assertions, prices and plans (cases 83–92)

### 83 extra-close-paren — grade C
**Mistake.** `warn (total(in, month)) <= 650 USD) "…"`: one `)` too many.
**Actual** (`83-extra-close-paren.out`): `expected-end-of-line: expected the end of the line, found )`
(caret on the stray paren), **then the `unknown-place` cascade**.
**Verdict.** The stray paren is found, but the message describes the parser's state, not
the mistake; the cascade follows.
**Ideal.**
```text
error[unbalanced-delimiter]: this `)` closes nothing
   ╭─[83-extra-close-paren.ax:10:39]
   │
10 │     warn (total(in, month)) <= 650 USD) "food is over budget"
   │          ┬                            ┬
   │          │                            ╰── nothing is open here
   │          ╰── the only `(` on this line is closed at column 27
   │
   = help: remove it
10 +     warn (total(in, month)) <= 650 USD "food is over budget"
```

### 84 assert-sign-overdrawn — grade D
**Mistake.** The account is 50.00 USD overdrawn at the bank; the assertion is `= 50 USD`.
**Actual** (`84-assert-sign-overdrawn.out`): an overdraft warning, then `assertion: assets/checking
holds -50.00 USD, not 50.00 USD` ("100.00 USD missing"), help `!`. In
`robust/r15-assert-negative.out` the user tries the obvious repair, `= -50 USD`, and gets
`negative-amount: amounts carry no sign … remove the sign`: **removing the sign returns to the
failing assertion. There is no way to assert an overdrawn balance.**
**Verdict.** A dead end. The user who is overdrawn (exactly the user who most needs
the assertion to reconcile) can only write `!`.
**Ideal.**
```text
error[assertion]: checking is overdrawn by 50.00 USD, not 50.00 USD in credit
   ╭─[84-assert-sign-overdrawn.ax:12:1]
   │
10 │ 2025-12-31 equity/opening -> checking 100 USD
11 │ 2026-01-08 checking -> food 150 USD
   │ ────────────────────┬───────────────
   │                     ╰── 100.00 − 150.00 = −50.00 USD
12 │ 2026-01-31 checking = 50 USD
   │                       ─┬
   │                        ╰── the exact opposite of what the ledger holds
   │
   = note: an asset below zero is an overdraft; assert it with a minus sign
   = help: if the bank shows 50.00 USD overdrawn
12 + 2026-01-31 checking = -50 USD
```
(Assertions must accept a signed amount; today the lexer forbids it.)

### 85 no-base-currency — grade D
**Mistake.** No `base` declaration; a EUR opening balance with no price.
**Actual** (`85-no-base-currency.out`): `no-price: no price for EUR in USD on 2025-12-31`, then
`no-price: cannot check overdraft: no price for EUR in USD…` for the same flow.
**Verdict.** The base currency silently defaults to USD (nothing says so), and one
missing price produces two errors, the second leaking "cannot check overdraft": an
`overdraft` law compares against `empty`, which needs no price at all.
**Ideal.**
```text
error[no-price]: no price for EUR in USD on 2025-12-31, needed to value the 500.00 EUR opening balance
   ╭─[85-no-base-currency.ax:10:1]
   │
10 │ 2025-12-31 equity/opening -> euro-account 500 EUR
   │                                           ───┬───
   │                                              ╰── 500.00 EUR arrives from equity: its basis is set in USD
   │
   = note: no `base` is declared, so USD is assumed; write `base USD` (or `base EUR`) at the top of axiom.ax
   = help: price it
 9 + 2025-12-31 EUR 1.09 USD
```

### 86 price-missing-unit — grade B
**Mistake.** A price line without its currency: `2026-01-31 VTI 285.70`.
**Actual** (`86-price-missing-unit.out`): `expected-commodity: expected a commodity such as USD, found
the end of the line`, help "an amount is a number and its commodity, like `285.70 USD`".
**Verdict.** Right; the completion is known.
**Ideal.**
```text
error[expected-commodity]: a price needs the currency it is priced in
   ╭─[86-price-missing-unit.ax:14:16]
   │
14 │ 2026-01-31 VTI 285.70
   │                ──┬───
   │                  ╰── 1 VTI = 285.70 …?
   │
   = help: a price line is `DATE COMMODITY PRICE CURRENCY`
14 + 2026-01-31 VTI 285.70 USD
```

### 87 unknown-cadence — grade B
**Mistake.** `every fortnight …`.
**Actual** (`87-unknown-cadence.out`): `unknown-cadence: unknown cadence fortnight`, note "the cadences are
day, week, month, quarter and year".
**Verdict.** The note omits `SPAN` (`2w`) which LANGUAGE.md allows, and the tool could
translate common English (`fortnight`, `biweekly`, `semimonthly`).
**Ideal.**
```text
error[unknown-cadence]: `fortnight` is not a cadence
   ╭─[87-unknown-cadence.ax:14:7]
   │
14 │ every fortnight checking -> landlord 900 USD
   │       ────┬───
   │           ╰── not a cadence
   │
   = note: `day`, `week`, `month`, `quarter`, `year`, or a span such as `2w` or `3m`
   = help: a fortnight is two weeks
14 + every 2w checking -> landlord 900 USD
```

### 88 lot-selector-nothing — grade D
**Mistake.** `brokerage[2026-01-23]`: no lot was acquired on that date.
**Actual** (`88-lot-selector-nothing.out`): `insufficient-holding: assets/brokerage does not hold 3
VTI`, notes "holds 12 VTI" and "the selectors match 0 VTI of it", generic help.
**Verdict.** Says the account "does not hold 3 VTI" although it holds 12; the answer
is buried in a second note. The user needs the *list of lots* and the nearest date.
**Ideal.**
```text
error[selector-matches-nothing]: no lot of VTI was acquired on 2026-01-23
   ╭─[88-lot-selector-nothing.ax:15:20]
   │
13 │ 2026-01-22 checking 2_000 USD -> brokerage 7 VTI
   │ ────────────────────────┬───────────────────────
   │                         ╰── lot 2026-01-22: 7 VTI, basis 2,000.00 USD
14 │ 2026-02-18 checking 1_500 USD -> brokerage 5 VTI
   │ ────────────────────────┬───────────────────────
   │                         ╰── lot 2026-02-18: 5 VTI, basis 1,500.00 USD
15 │ 2026-03-02 brokerage[2026-01-23] 3 VTI -> checking 900 USD
   │                      ─────┬────
   │                           ╰── matches none of the 2 lots
   │
   = help: did you mean the lot bought a day earlier?
15 + 2026-03-02 brokerage[2026-01-22] 3 VTI -> checking 900 USD
```

### 89 unknown-policy — grade A
**Mistake.** `brokerage[fifoo]`.
**Actual** (`89-unknown-policy.out`): `unknown-policy` with the edit `[fifo]`.
**Ideal.** As actual, plus the list.
```text
error[unknown-policy]: unknown lot policy `fifoo`
   ╭─[89-unknown-policy.ax:15:22]
   │
15 │ 2026-03-02 brokerage[fifoo] 3 VTI -> checking 900 USD
   │                      ──┬──
   │                        ╰── not a lot policy
   │
   = note: policies: `fifo`, `lifo`, `hifo`, `prorata`; or a date, a month, a `#code`
   = help: did you mean `fifo`?
15 + 2026-03-02 brokerage[fifo] 3 VTI -> checking 900 USD
```

### 90 unattached-doc — grade A (by design)
**Situation.** `/// The everyday account.`, a blank line, then `account assets/checking`.
**Actual** (`90-unattached-doc.out`): `✓`; no diagnostic.
**Verdict.** LANGUAGE.md: "Blank lines separate nothing and mean nothing", so the doc
attaches to the account; not a mistake. (The parser does have an `unattached-doc`
warning for `///` at end of file or before a comment; it just did not fire here.)

### 91 backwards-range — grade F
**Mistake.** `2026-12-31..2026-01-01 checking -> insurance 600 USD`.
**Actual** (`91-backwards-range.out`): `✓ 2 flows`. `axiom register checking` shows only the opening
balance; the 600.00 USD payment has **no effect anywhere** (checking still holds
1,000.00 USD).
**Verdict.** A payment vanishes silently: the worst class of error for a ledger.
**Ideal.**
```text
error[range-backwards]: this range ends on 2026-01-01, 364 days before it starts on 2026-12-31
   ╭─[91-backwards-range.ax:10:1]
   │
10 │ 2026-12-31..2026-01-01 checking -> insurance 600 USD
   │ ──────────┬─────────────
   │           ╰── a range runs from the earlier date to the later
   │
   = help: swap the dates
10 + 2026-01-01..2026-12-31 checking -> insurance 600 USD
```

### 92 waiver-hides-overdraft — grade B (control)
**Situation.** `!` waives an overdraft warning on one flow, with a reason.
**Actual** (`92-waiver-hides-overdraft.out`): the warning is still printed, with "waived here" on
the `!`, and `= note: waived: landlord accepts a late deposit`.
**Verdict.** The design is right: "the waiver is reported, never hidden". The summary
line does not count it: `1 warning`, with no "waived".
**Ideal.**
```text
warning[overdraft]: checking would go to -1,000.00 USD after this payment (waived)
   ╭─[92-waiver-hides-overdraft.ax:14:1]
   │
14 │ 2026-01-01 checking -> landlord 1_800 USD ! "landlord accepts a late deposit"
   │ ─────────────────────┬─────────────────── ┬─────────────────────────────────
   │                      │                    ╰── waived: landlord accepts a late deposit
   │                      ╰── 800.00 USD − 1,800.00 USD = −1,000.00 USD
   │
1 warning (1 waived)
```

## 9. Project layout (cases 93–99)

Layout errors are already clear and actionable (`move it to journal/2026/02.ax`). The
gaps: a file with a hundred misdated lines gets a hundred errors, and none of them
offers the move as an edit.

### 93 feb-in-jan — grade B
**Mistake.** A 2026-02-10 flow in `journal/2026/01.ax`.
**Actual** (`93-feb-in-jan.out`): `layout: this transaction is dated 2026-02-10, which
journal/2026/01.ax does not hold`, label "outside January 2026", help "move it to
`journal/2026/02.ax`, or set `layout free`".
**Verdict.** Good. Grouping is missing.
**Ideal.**
```text
error[layout]: `journal/2026/01.ax` holds January 2026, and this flow is dated 2026-02-10
  ╭─[journal/2026/01.ax:3:1]
  │
3 │ 2026-02-10 checking -> food 61.00 USD
  │ ─────┬────
  │      ╰── February; belongs in `journal/2026/02.ax`
  │
  = note: the file's folder and name fix its month; `layout free` in axiom.ax turns that off
  = help: move the line
  ├─[journal/2026/02.ax:1:1]
1 + 2026-02-10 checking -> food 61.00 USD
```
(If several lines are misfiled, one diagnostic per file with all the lines.)

### 94 flow-in-prices — grade A
**Mistake.** A flow in `prices/2026.ax`.
**Actual** (`94-flow-in-prices.out`): `layout: files under prices/ may contain only prices`, label "this is
not a price", help "move it out of `prices/`, or set `layout free`".
**Ideal.** As actual.
```text
error[layout]: files under `prices/` may contain only prices
  ╭─[prices/2026.ax:2:1]
  │
2 │ 2026-01-22 checking -> food 12.00 USD
  │ ──────────────────┬──────────────────
  │                   ╰── a flow, not a price
  │
  = help: move it to `journal/2026/01.ax`, or set `layout free`
```

### 95 non-system-in-systems — grade A
**Mistake.** `systems/mine.ax` does not start with `system`.
**Actual** (`95-non-system-in-systems.out`): `layout: files under systems/ must be systems`, help
"start it with `system PATH`…".
**Ideal.**
```text
error[layout]: files under `systems/` must be systems
  ╭─[systems/mine.ax:1:1]
  │
1 │ kind piggy-bank : asset
  │ ───────────┬───────────
  │            ╰── this file does not begin with `system`
  │
  = help: start it with a system header
1 + system mine
```

### 96 year-file-mismatch — grade B
**Mistake.** A 2026 flow in `journal/2025.ax`.
**Actual** (`96-year-file-mismatch.out`): as 93, with "outside 2025" and a move suggestion.
**Ideal.**
```text
error[layout]: `journal/2025.ax` holds 2025, and this flow is dated 2026-01-08
  ╭─[journal/2025.ax:2:1]
  │
2 │ 2026-01-08 checking -> food 84.20 USD
  │ ─────┬────
  │      ╰── 2026; belongs in `journal/2026.ax` (or `journal/2026/01.ax`)
  │
  = help: move the line, or set `layout free`
```

### 97 no-root — grade B
**Mistake.** `axiom check -C dir` with no `axiom.ax` above it.
**Actual** (`97-no-root.out`): `error: no axiom.ax in <path> or any folder above it` + help; **exit
status 2**; no code in the header.
**Verdict.** Clear; add the fix (create the root file) and give it a code like every
other error.
**Ideal.**
```text
error[no-project]: `97-no-root` is not inside an Axiom project (no `axiom.ax` here or above it)
  = note: looked in /…/tests/mistakes/97-no-root and every folder above it
  = help: create a project here
  + axiom.ax:
  + base USD
  = help: or check a single file: `axiom check FILE.ax`
```

### 98 duplicate-across-files — grade B
**Mistake.** `account assets/checking` declared in `accounts.ax` and again in `journal/2026/01.ax`.
**Actual** (`98-duplicate-across-files.out`): `duplicate-declaration` with both locations
("declared again here" / "first declared here"), help "remove one of the two".
**Verdict.** Correct. Say which file is the natural home (declarations go in
`accounts.ax`).
**Ideal.**
```text
error[duplicate-declaration]: account `assets/checking` is already declared in `accounts.ax`
  ╭─[journal/2026/01.ax:2:9]
  │
2 │ account assets/checking : bank
  │         ───────┬───────
  │                ╰── declared again here
  │
  ├─[accounts.ax:1:9]
  │
1 │ account assets/checking : bank
  │         ───────┬───────
  │                ╰── first declared here (same kind: `bank`)
  │
  = help: delete the line in the journal
```

### 99 month-file-mismatch — grade B
**Mistake.** A 2026-01-31 flow in `journal/2026-02.ax`.
**Actual** (`99-month-file-mismatch.out`): as 93.
**Ideal.**
```text
error[layout]: `journal/2026-02.ax` holds February 2026, and this flow is dated 2026-01-31
  ╭─[journal/2026-02.ax:1:1]
  │
1 │ 2026-01-31 checking -> food 30.00 USD
  │ ─────┬────
  │      ╰── January; belongs in `journal/2026-01.ax`
  │
  = help: move the line, or set `layout free`
```

## 10. Robustness probes (`robust/`)

24 inputs (22 that are not plausible mistakes but must never crash or hang, and two probes of
error multiplicity). **No panic, no hang, no crash in any of them** (each finishes in about 5 ms).

| probe | input | result |
|---|---|---|
| r01 | empty file | `✓ 0 flows`, exit 0 |
| r02 | CRLF line endings | accepted |
| r03 | UTF-8 BOM | accepted |
| r04 | amount `99999999999999999999 USD` | `bad-number`: "more digits than an amount can hold" |
| r05 | `0.000000001 USD` | `amount-precision`: "USD counts 2 decimal places", help `precision 9` |
| r06 | dates `0000-01-08` and `9999-12-31` | accepted |
| r07 | 5,000 nested parentheses | `expression-too-deep` ("nest at most 100 levels") **but the snippet prints the whole 10,000-character line** (no line truncation) |
| r08 | `account assets/café` | `unexpected-character 'é'` on each use: non-ASCII names are unsupported, and nothing says names are ASCII-only |
| r09 | NUL byte inside an amount | `unexpected-character ' '`: the NUL is drawn as a space, and the help ("an amount is a number and its commodity") is wrong |
| r10 | a 2 MB comment line | accepted |
| r11 | `amount / 0 > 1` in a law | `type-mismatch` (amount vs number), never reaches a divide |
| r12 | `kind a : b` / `kind b : a` | `kind-cycle` with the chain, then a second `kind-sort` error (cascade) |
| r13 | two amounts of 9e16 USD | `amount-range` on each: "beyond 100,000,000,000,000,000 quanta" |
| r14 | `food empty`, `food 0 USD` | `empty-amount` and `zero-flow` |
| r15 | `checking = -50 USD` (overdrawn) | `negative-amount` "remove the sign" (case 84's dead end) |
| r16 | a spread over `0001-01-01..9999-12-31` | accepted; `forecast` says "from 24305 months of history" |
| r17 | 300 lines of ever-deeper indentation | `expected-amount`, `unexpected-indent`; no blow-up |
| r18 | a `code` glob with 40 `*`s | accepted (linear matcher) |
| r19 | `every day … until 9999-12` | `check` ignores plans; `forecast` fine (bounded to one year) |
| r20 | comments only | `✓ 0 flows` |
| r21 | 1 KB of binary | `cannot read … stream did not contain valid UTF-8`, exit 2 |
| r22 | invalid UTF-8 in a comment | same |
| r23 | the same typo (`grocries`) used 3 times | **3 identical `unknown-place` errors**, each with two `help:`s: one root cause, N errors |
| r24 | `born` missing; paychecks in 2025 and 2026 | 2 errors (`deferral-limit`, then `required-minimum-distribution` in `us/401k.ax:73`): deduplicated per law, not per root cause |

Findings from the probes: **unbounded snippet width** (r07), **non-ASCII rejected without
explanation** (r08), **a control character drawn as a space with an unrelated help** (r09),
and the non-UTF-8 message does not say *where* (byte offset) or suggest re-saving as UTF-8.


## 11. Constraint surfacing

The design promise: "constraints at all levels", and a user can ask *what governs this,
how close am I, and why did that fire*. The probes are in `constraints.sh`; the captured
output is `constraints.out`. Probe numbers below are its headings.

**What works.**
- `available` (probe 16) is the best answer in the tool: for each illiquid holding it
  runs a hypothetical withdrawal through the real laws and prints the penalty and the tax.
  "Liquidity is derived from law" is visible there. (It also prints every internal tally,
  `counts 43,700.00 USD as agi`, as noise; and it is the one command that scales badly:
  `bench/REPORT.md`.)
- `why <account>` (probe 1) lists the laws that *govern* an account with their source
  line and the first sentence of their doc comment.
- `why <#code>` and `why <file:line>` (probe 14) connect a warning to its flow.
- Diagnostics for a failed limit (case 69) say "at most 9,500.00 USD more can count".

**What is missing, in order of pain.**

1. **"What limits apply to me this year, and how close am I?" has no answer.**
   `why retirement` (probe 1) lists 13 laws, none with a number. The 401(k) cap of
   24,500.00 USD, the 2,400.00 USD counted so far and the 22,100.00 USD of room exist in
   the engine (they are exactly the operands of `tally(elective-deferrals) <=
   limit[year] + extra`) but appear nowhere until the cap is *broken*. `budget` (probe 2)
   shows only laws written as `warn total(…) <= X`; the 529 gift exclusion appears with
   "Spent 900.00 USD" and an *empty* limit column, because its limit is a param lookup.
   The tax page (probe 5) shows the tally (`elective-deferrals 2,400.00 USD`) with no
   limit beside it.
2. **Priced violations are invisible in `check`** (probes 6, 7, 8). An early 401(k)
   withdrawal owes 1,000.00 USD and adds 10,000.00 USD to income; `check` prints `✓`.
   The cost appears in `tax` only. The household README states that `check` reports the
   priced 529 withdrawal; the golden output (`tests/golden/household-check.txt`) does not.
3. **13 laws are listed where 3 constrain.** The "Governed by" table mixes limits
   (`deferral-limit`), prices (`early-withdrawal`), pure tallies (`count-wages`,
   `count-interest`, `credit-ca-withholding`), a city property-transfer tax on a
   retirement account, and a jurisdiction chain ("everyone living under us/ca/san-francisco").
   No grouping, no relevance filter, and a 200-column-wide table (probe 1).
4. **Reports refuse to run while any error stands** (probes 10, 11). With the 401(k)
   over its limit, `why retirement`, `budget` and `available` print the violation again
   and stop: exactly when the user wants to investigate. `--relaxed` makes them work
   but the refusal never mentions it.
5. **Entities cannot be asked about** (probes 12, 13). `why scholarship` prints
   "Why income/grants" (the `via` place) and none of the grant's laws (`purpose`,
   `deadline`). `why checking` lists only `overdraft`, although the tied scholarship
   money in it is bound by two laws (its `on spend` purpose and its `by` deadline).
   Constraints that follow *money* rather than *places* are invisible from the place.
6. **A law's name is not a handle** (probe 9). `why early-withdrawal` fails with
   `` `early-withdrawal` could be `early-withdrawal`, `early-withdrawal` `` (two systems
   define it) and prints neither system.
7. **The doc comment mixes description and remedy.** `why deferral-limit` (probe 3)
   shows "This payment took the year's deferrals over the cap. Ask payroll to lower the
   deferral…" for a law that has not been broken, hard-wrapped with a `·` bullet on each
   physical line, with one over-long line.
8. **`why FILE:LINE` says what fired, not what was evaluated** (probe 14). "law `budget`
   does not hold for expenses/food"; the 681.00 USD versus 650.00 USD appears only in
   `check`.
9. **Nothing forward-looking.** "I can still put 22,100.00 USD into the 401(k) in 2026;
   the 529 gift exclusion has 18,100.00 USD of room; my checking may not go below 0"
   is the most useful sentence a constraint system could say, and every operand is
   already computed.

**Proposed surface.**

```text
$ axiom why retirement
assets/retirement   401(k) of me, employer acme         43,700.00 USD

Limits in 2026 (resets 2027-01-01)
  Law               Counted            Limit           Room left        Used
  deferral-limit    2,400.00 USD   24,500.00 USD    22,100.00 USD       10%
                    limit 24,500.00 USD (no catch-up: me is 37)  · us/401k.ax:33 · IRC §402(g)

What a withdrawal costs today
  early-withdrawal  10% of the gain to irs while me is under 59y6m (37y9m today; 2047-10-12)
  count-distributions  the gain counts as income (`axiom available` prices a full draw: 8,555.54 USD)

Also counted here: wages, agi, pretax   (7 more laws only tally; `--all` lists them)
```

```text
$ axiom limits            # every cap and budget, one table
  Who    Limit                    Window   Counted         Cap             Room left       Used
  me     401(k) deferrals         2026     2,400.00 USD    24,500.00 USD   22,100.00 USD   10%
  me     529 gifts (college)      2026       900.00 USD    19,000.00 USD   18,100.00 USD    5%
  me     expenses/food budget     2026-03    681.00 USD       650.00 USD      -31.00 USD  105%  ⚠
  me     checking overdraft       always     10,332.70 USD   0.00 USD floor  10,332.70 USD
```

And `check` should print, after the diagnostics, one line per priced consequence and one
line for limits nearing their cap (`401(k) deferrals: 10% of the 2026 limit`).


## 12. The ten most impactful diagnostic improvements

Ranked by (damage of the current behaviour) × (how many cases it touches).

1. **Accept nothing silently; five inputs today lose or misroute money without a word.**
   An undeclared commodity is created (`UDS`, case 23, F); a typo inside a *full path*
   opens a new account (`expenses/grocries`, 45, F); a settlement dated before the flow
   is rewritten to the flow's date (57, F); a backwards range `2026-12-31..2026-01-01`
   makes a payment vanish (91, F); a missing `base` silently means USD (85, D). Each
   needs an error or, for "opens a place", a warning that names the nearest declared
   place. *This is the top item because the failure is invisible in a system whose
   whole purpose is to be right.*
2. **One root cause, one diagnostic.** A declaration that fails to parse is dropped,
   so every use of it is an `unknown-place` (14, 15, 19, 72, 73, 81, 83: seven cases,
   and one error per *use* in a real file). Pairing failures cascade (43, 56), one
   missing property fires three errors (78), one missing table four (67), one missing
   price two (85). Recovery rule: keep the declaration with the bad line dropped; give
   every error a *root key* and print the first, with "N more errors caused by this
   are hidden".
3. **`check` must print priced violations and their cost.** A 10,000.00 USD early
   401(k) withdrawal, a non-qualified 529 withdrawal, a grant deadline: all
   "priced, not failed" and all silent (71, F; the household README promises them).
   Print them as `note[priced]` with the amount owed, and count them in the summary.
4. **A failed assertion should reason like a bookkeeper.** Try, in order: a *reversed
   flow* (gap = 2 × a flow, case 59), a *transposition* (gap divisible by 9, 48), a
   *sign error* (gap = 2 × balance, 84), a *wrong commodity* (58), a *missed
   transaction* (49). Offer "correct the number" before `!`, and let assertions be
   written negative (84: at present an overdrawn balance cannot be asserted at all).
5. **Point into the user's file and say what to add.** `unset-property` and
   `no-param-row` are reported inside `us.ax`/`us/401k.ax` (67, 78, 79): the user must
   go read a file they did not write. Report at the `entity`/`account` that lacks the
   property, with the edit (`+ filing single`), and mark built-in sources `(built in)`.
   Never make a built-in the primary of a duplicate (60).
6. **State the accounting fact in the headline.** "27,000.00 USD deferred in 2026
   against a limit of 24,500.00 USD" instead of "401(k) deferrals over the yearly
   limit"; "checking would go to −1,000.00 USD" instead of "balance below zero"; "expenses/food is 40.00 USD
   over its 650.00 USD monthly budget" instead of "law `budget` does not hold". Show
   the *earlier* flows that built the tally (69). Move the power-assert tree behind
   `--explain`, split each law's doc into *what it is* and *what to do*, and print
   `0.00 USD` for `empty`.
7. **Every edit must be safe, and mechanical fixes must be edits.** Case 12 proposes
   `1.234_56 USD` for `1.234,56 USD`: it changes the amount. Case 16 turns a refund
   into a second purchase. Meanwhile 33–37, 62, 65, 86, 87, 91 have fully determined
   fixes that are only prose. Add the lexer's easy rescues: `=>`, `→` (04, 05),
   `01/15/2026` (10), a missing commodity (06).
8. **Deduplicate floods.** A breached budget re-warns on every later flow of the
   month (77; 40,320 warnings for 100k flows in `bench/REPORT.md`); a typo used 300
   times is 300 errors with two `help:`s each (21, 46, `robust/r23`); one
   tab-indented file is one error per line (01). Report once per window/typo/file and
   list the locations.
9. **When a rule from a declaration is applied, show the declaration.** `closed
   2026-01-31`, `opened 2026-03-01`, `holds BTC`, `via`, the `code` rule, the list of
   lots for a selector that matched none (38, 39, 41, 88). rustc's core lesson: the
   error is the *conflict* between two places in the source, so draw both.
10. **Make constraints first-class in the CLI.** `why <account>` grouped into limits
    (with used/room), prices, and tallies; `axiom limits`; `why` on entities and on
    an unambiguous law path; reports that run in the presence of errors (§11).

## 13. Systemic patterns

- **Every name error lists the three nearest names**, ranked by edit distance *and* by
  class (a place is not offered for a commodity slot), each with the edit. Today: one
  candidate, at most `len/3` edits (minimum 1), so short names only match on a single
  edit, and no class awareness. Show the generic "open a new account" help *only* when nothing is close.
- **Never report more than one error per root cause.** Give each diagnostic a `root`
  (the source location that, if fixed, removes it) and suppress dependents. Applies to
  parsing (poisoned declarations), pairing (`?`), model (missing prices, params,
  properties) and reports (`check` and every report print the same set).
- **Resolution must not create.** A rule that lets a full path open a place, a
  commodity that need not be declared, a base that defaults, an event that is
  clamped: all four silent successes are "resolve by inventing". Invent only when
  there is no near miss, and say so once ("this opens `expenses/foo`").
- **A rule error draws both ends.** Location of the use + location of the rule
  (opened/closed/holds/code/via/budget/param), with the value that decided it
  (`closed 2026-01-31`, `59y6m`).
- **Built-in sources are never the primary label and are always marked `(built in)`.**
  If the *user's* declaration caused the conflict, the primary is theirs (60, 67, 78, 79).
- **Money is always printed as money.** `1,234.56 USD`, the commodity's precision,
  thousands separators, and the derivation when it helps (`800.00 − 1,800.00 =
  −1,000.00 USD`). Never `84.2 UDS`, `0.0 UDS`, `-0.00 USD`, `empty` or `2026` (the
  year inside a power-assert row) as a *value of money*.
- **Speak about the ledger, not about the engine.** Banned in messages: "law … does
  not hold", "cannot check", "unset property", "elaborate", "resolve", "no such
  property", "found the end of the line". Preferred: what the user did, in their nouns
  (paycheck, check, statement, lot, budget, limit).
- **Help is an edit, or it is not help.** If the fix is mechanical, show the line
  (the renderer already draws `NN + …` rows); if there are alternatives, rank them
  and offer at most three; if the fix is not a source edit, say who must act
  ("ask payroll", "ask the bank").
- **A note states a fact; a help states an action.** Notes carry derived values and
  laws' doc sentences (one paragraph, wrapped once); helps are imperative.
- **Labels carry values and derivations, not restatements.** "not a known place" under an
  unknown place restates the header; "800.00 − 1,800.00 = −1,000.00 USD" does not.
- **Truncate wide snippets.** A 10,000-character line is printed whole (r07): clip to
  the terminal width around the span with `…`.
- **Stable codes with an explanation.** Codes are already stable (`unknown-place`,
  `price-disagrees`); add `axiom explain CODE`, and codes for the code-less errors
  (`no axiom.ax…`), and rename `law` (a generic bucket) to the law's own name
  (`deferral-limit`, `overdraft`, `budget`).
- **Sort by cause, not only by position.** Errors first, in source order, then priced
  notes, then warnings; the summary line counts each (`1 error, 3 warnings, 1 priced, 1 waived`).

## 14. Proposed diagnostic style guide

### 14.1 The shape of a message

```text
SEVERITY[code]: HEADLINE
   ╭─[FILE:LINE:COL]                      ← the primary label, in the user's file
   │
NN │ source line
   │ ───┬───
   │    ╰── PRIMARY LABEL
   │
   ├─[OTHER-FILE:LINE:COL] (built in)     ← the other end of the conflict, if any
   │
NN │ the declaration or law line
   │        ──┬──
   │          ╰── SECONDARY LABEL (a value)
   │
   = note: FACT
   = note: FACT
   = help: ACTION
NN + the corrected line
```

- **Severity.** `error` stops the ledger from being trustworthy (a fact is wrong or
  cannot be read). `warning` is a soft law or a budget. `note[priced]` is a
  consequence the ledger accepts and prices (a penalty, an income line). `note` alone
  is information (a pad, a waiver).
- **Headline.** One sentence about the *ledger*, ≤ 90 characters, no trailing period,
  lower-case first word except names, subject first: `checking would go to −1,000.00
  USD after this payment`. Backticks around anything written in source. Amounts with
  commodity and separators. No engine words (§13). It must stand alone in a log line.
- **Primary label.** Under the exact tokens at fault, *not* the whole line unless the
  line is the fault; say what is wrong with them in ≤ 8 words, or give the value they
  computed. One primary label per diagnostic.
- **Secondary labels.** The other end of the conflict (the declaration, the earlier
  flow, the rule), each with the *fact it contributes*: `closed 2026-01-31`, `first
  declared here, kind bank`, `+3,900.00 USD from acme`. At most three; more are folded
  into a note.
- **Notes.** Facts the reader could not read off the snippet: derived amounts, why the
  rule exists (the law's one-paragraph doc), what else it affects. Never advice.
- **Help.** One or more actions, imperative mood, each preferably followed by the
  edited source line(s) in diff form. Rank alternatives; ≤ 3. If the fix changes an
  amount, say so in the help text. A help that is not an edit says *who* acts.
- **Summary.** After the last diagnostic: `✗ 2 errors, 1 warning, 1 priced, 1 waived`.

### 14.2 When to use note and when to use help

| the text says… | it is a | example |
|---|---|---|
| what the rule is, or why it exists | note | `a payee must be a declared entity` |
| a derived number | note | `36.00 USD is divisible by 9: a transposition?` |
| an alternative reading of the input | note | `if the comma is a decimal separator, write 1.20 USD` |
| what to type | help + edit | `write `84.20 USD`` |
| what someone else must do | help | `ask payroll to lower the deferral` |
| what will happen next | note | `the sale is booked FIFO so later lines stay consistent` |

### 14.3 Amounts

- Always `1,234.56 USD` in prose; `1_234.56 USD` in source edits (the language's own
  separator).
- Negative balances take a true minus, `−1,000.00 USD`; a liability is shown by what
  is *owed*, `452.00 USD owed`, never signed (matches assertions).
- A derivation is an equation on one line: `800.00 − 1,800.00 = −1,000.00 USD`.
- A gap has a direction and a subject: `36.00 USD more than the ledger holds`, not
  `36.00 USD missing`.
- Percentages against a limit: `27,000.00 USD of 24,500.00 USD (110%)`.
- Zero is `0.00 USD`, never `empty` or `-0.00`.

### 14.4 Showing a law and its values

A failed law is read in three layers, most human first.

1. **The fact** (headline): what was counted, against what, over by how much:
   `27,000.00 USD deferred in 2026 against a limit of 24,500.00 USD`.
2. **The contributors** (labels): the flows behind the count, each with its amount and
   date; the *flow that crossed the line* is primary.
3. **The rule** (secondary label, built-in): the law's own line with the operands *named
   in words*, not the raw expression tree:
   ```text
   ├─[us/401k.ax:33:13] (built in: law `deferral-limit`, IRC §402(g))
   33 │     require tally(elective-deferrals) <= limit[year] + extra
   │                    27,000.00 USD          24,500.00 USD + 0.00 USD
   ```
   The full power-assert tree (every subexpression) is shown by `--explain` and by
   `axiom why FILE:LINE`; the default view shows only the operands of the failing
   comparison.

The law's doc comment is *split*: the first paragraph (what the law is) is the `note`;
a paragraph starting "To fix:" (what to do) is the `help`. Both are wrapped once, at
the terminal width, without per-line bullets.

### 14.5 Where each kind of message lives

| situation | severity | code shape | show |
|---|---|---|---|
| cannot read the text | error | `expected-…`, `bad-…`, `unknown-…` | the token, the rule, the edit |
| names an undeclared thing | error | `unknown-<noun>` | up to 3 nearest names |
| conflicts with a declaration | error | `duplicate-…`, `place-closed`, `not-held`… | both sites |
| balance assertion fails | error | `assertion` | flows since the last passing assertion; the four hypotheses |
| a limit is broken | error | the law's name (`deferral-limit`) | contributors + rule + room |
| a soft limit/budget is passed | warning | `over-budget`, `overdraft` | once per window |
| a penalty is priced | note[priced] | the law's name | what is owed, to whom, by when |
| something was accepted | note | `pad`, `waiver` | who accepted, and the reason |
| something was assumed | warning | `assumed-…`, `new-place` | the assumption and how to state it |

## 15. Case index

| # | case | grade | file |
|---|---|---|---|
| 01 | tab-indent | B | `01-tab-indent.ax` |
| 02 | bad-indent | B | `02-bad-indent.ax` |
| 03 | missing-arrow | B | `03-missing-arrow.ax` |
| 04 | fat-arrow | C | `04-fat-arrow.ax` |
| 05 | unicode-arrow | C | `05-unicode-arrow.ax` |
| 06 | amount-no-commodity | B | `06-amount-no-commodity.ax` |
| 07 | lowercase-commodity | A | `07-lowercase-commodity.ax` |
| 08 | zero-not-empty | A | `08-zero-not-empty.ax` |
| 09 | invalid-date | A | `09-invalid-date.ax` |
| 10 | date-slashes | D | `10-date-slashes.ax` |
| 11 | unclosed-string | B | `11-unclosed-string.ax` |
| 12 | comma-decimal | D | `12-comma-decimal.ax` |
| 13 | dollar-sign | A | `13-dollar-sign.ax` |
| 14 | trailing-operator | C | `14-trailing-operator.ax` |
| 15 | unbalanced-parens | C | `15-unbalanced-parens.ax` |
| 16 | negative-amount | C | `16-negative-amount.ax` |
| 17 | thousands-comma | B | `17-thousands-comma.ax` |
| 18 | glued-amount | A | `18-glued-amount.ax` |
| 19 | unknown-keyword | C | `19-unknown-keyword.ax` |
| 20 | date-unpadded | A | `20-date-unpadded.ax` |
| 21 | typo-account | B | `21-typo-account.ax` |
| 22 | typo-entity | A | `22-typo-entity.ax` |
| 23 | typo-commodity | F | `23-typo-commodity.ax` |
| 24 | typo-kind | A | `24-typo-kind.ax` |
| 25 | ambiguous-suffix | A | `25-ambiguous-suffix.ax` |
| 26 | duplicate-account | B | `26-duplicate-account.ax` |
| 27 | entity-no-via | B | `27-entity-no-via.ax` |
| 28 | unknown-system | A | `28-unknown-system.ax` |
| 29 | property-typo | A | `29-property-typo.ax` |
| 30 | payee-not-entity | C | `30-payee-not-entity.ax` |
| 31 | kind-in-wrong-slot | B | `31-kind-in-wrong-slot.ax` |
| 32 | two-remainders | A | `32-two-remainders.ax` |
| 33 | many-to-many | B | `33-many-to-many.ax` |
| 34 | split-short | B | `34-split-short.ax` |
| 35 | split-over | C | `35-split-over.ax` |
| 36 | price-disagrees | A | `36-price-disagrees.ax` |
| 37 | transfer-unequal | B | `37-transfer-unequal.ax` |
| 38 | closed-account | C | `38-closed-account.ax` |
| 39 | holds-violation | C | `39-holds-violation.ax` |
| 40 | code-forbidden | B | `40-code-forbidden.ax` |
| 41 | before-opened | C | `41-before-opened.ax` |
| 42 | self-flow | A | `42-self-flow.ax` |
| 43 | exchange-no-price | D | `43-exchange-no-price.ax` |
| 44 | overdraft | C | `44-overdraft.ax` |
| 45 | typo-full-path | F | `45-typo-full-path.ax` |
| 46 | unknown-place-in-leg | B | `46-unknown-place-in-leg.ax` |
| 47 | unknown-place-in-assertion | B | `47-unknown-place-in-assertion.ax` |
| 48 | assert-transposed | C | `48-assert-transposed.ax` |
| 49 | assert-missed-txn | C | `49-assert-missed-txn.ax` |
| 50 | assert-card-sign | C | `50-assert-card-sign.ax` |
| 51 | settle-unknown-code | A | `51-settle-unknown-code.ax` |
| 52 | void-twice | B | `52-void-twice.ax` |
| 53 | sell-more-than-held | B | `53-sell-more-than-held.ax` |
| 54 | ambiguous-lot-sale | A | `54-ambiguous-lot-sale.ax` |
| 55 | infer-no-assertion | B | `55-infer-no-assertion.ax` |
| 56 | infer-two-unknowns | D | `56-infer-two-unknowns.ax` |
| 57 | settle-before-written | F | `57-settle-before-written.ax` |
| 58 | assert-wrong-commodity | C | `58-assert-wrong-commodity.ax` |
| 59 | reversed-card-payment | C | `59-reversed-card-payment.ax` |
| 60 | redeclare-std-commodity | D | `60-redeclare-std-commodity.ax` |
| 61 | type-amount-plus-date | A | `61-type-amount-plus-date.ax` |
| 62 | type-amount-lt-number | B | `62-type-amount-lt-number.ax` |
| 63 | is-against-non-kind | B | `63-is-against-non-kind.ax` |
| 64 | unknown-function | B | `64-unknown-function.ax` |
| 65 | wrong-arity | C | `65-wrong-arity.ax` |
| 66 | unknown-field | A | `66-unknown-field.ax` |
| 67 | no-param-row | D | `67-no-param-row.ax` |
| 68 | gain-under-on-in | B | `68-gain-under-on-in.ax` |
| 69 | 401k-two-employers | B | `69-401k-two-employers.ax` |
| 70 | grant-wrong-purpose | B | `70-grant-wrong-purpose.ax` |
| 71 | early-withdrawal | F | `71-early-withdrawal.ax` |
| 72 | law-no-trigger | C | `72-law-no-trigger.ax` |
| 73 | unknown-trigger | C | `73-unknown-trigger.ax` |
| 74 | unknown-param | C | `74-unknown-param.ax` |
| 75 | unknown-tally | A | `75-unknown-tally.ax` |
| 76 | kind-typo-in-law | A | `76-kind-typo-in-law.ax` |
| 77 | budget-overspend | C | `77-budget-overspend.ax` |
| 78 | missing-filing-status | D | `78-missing-filing-status.ax` |
| 79 | missing-born | C | `79-missing-born.ax` |
| 80 | unknown-name-in-law | A | `80-unknown-name-in-law.ax` |
| 81 | incomplete-if | C | `81-incomplete-if.ax` |
| 82 | relaxed-keyword | A | `82-relaxed-keyword.ax` |
| 83 | extra-close-paren | C | `83-extra-close-paren.ax` |
| 84 | assert-sign-overdrawn | D | `84-assert-sign-overdrawn.ax` |
| 85 | no-base-currency | D | `85-no-base-currency.ax` |
| 86 | price-missing-unit | B | `86-price-missing-unit.ax` |
| 87 | unknown-cadence | B | `87-unknown-cadence.ax` |
| 88 | lot-selector-nothing | D | `88-lot-selector-nothing.ax` |
| 89 | unknown-policy | A | `89-unknown-policy.ax` |
| 90 | unattached-doc | A | `90-unattached-doc.ax` |
| 91 | backwards-range | F | `91-backwards-range.ax` |
| 92 | waiver-hides-overdraft | B | `92-waiver-hides-overdraft.ax` |
| 93 | feb-in-jan | B | `93-feb-in-jan` |
| 94 | flow-in-prices | A | `94-flow-in-prices` |
| 95 | non-system-in-systems | A | `95-non-system-in-systems` |
| 96 | year-file-mismatch | B | `96-year-file-mismatch` |
| 97 | no-root | B | `97-no-root` |
| 98 | duplicate-across-files | B | `98-duplicate-across-files` |
| 99 | month-file-mismatch | B | `99-month-file-mismatch` |
