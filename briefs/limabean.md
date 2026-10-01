## 1. What limabean is

- A new Beancount implementation by GitHub user **tesujimath**. Repo: https://github.com/tesujimath/limabean. It is dual-licensed MIT/Apache.
- The user interface is only a Clojure REPL: there is no BQL and no Python. Rust (the `limabean-pod` JSON-RPC server) does the parsing and booking.
- It is built from three pieces:
  - the parser crate **beancount-parser-lima** (chumsky + logos + **ariadne 0.6**),
  - the booking crate **limabean-booking** (https://github.com/tesujimath/limabean-booking),
  - error formatting in Clojure (`clj/src/limabean/adapter/error.clj`).
- **Status:** v0.6.1 was released 2026-05-25 on crates.io and Clojars; main was last touched 2026-05-29. It is young (0.2.0 was 2026-02-05) and one person hand-crafts it.
- **Docs:** they say almost nothing about errors. Everything below comes from the source, the golden test fixtures, and runs I did myself.

**Method:** I cloned all three repos and built `limabean-pod` at commit 59386bc. I called its `book` method and then `parser.format-errors`, which is the same path the Clojure CLI uses. The CLI only adds a `Booking failed` line in front. I ran this on crafted ledgers. My runs reproduce the repo's golden `test-cases/error/*.golden/error.ansi` byte-for-byte. Outputs below have ANSI colour codes stripped.

## 2. Verbatim diagnostics

**Balance failure** (golden `balance-failure.golden/error.ansi`):
```
Booking failed

Error: invalid balance
    ╭─[ ../test-cases/error/balance-failure.beancount:29:1 ]
    │
 29 │ 2020-02-28 balance Assets:Bank:Current                                   835.00 NZD
    │ ─────────────────────────────────────────┬─────────────────────────────────────────  
    │                                          ╰─────────────────────────────────────────── accumulated 935.00, error -100.00 NZD
────╯
2020-01-01    1000.00  1000.00  Initial income
2020-02-01     -50.00   950.00  Food          
2020-02-02     -15.00   935.00  Drinks        
```

**Balance on a parent account** (my run). A subaccount column appears when more than one account contributes:
```
 18 │ 2020-02-01 balance Assets:Bank   1000.00 NZD
    │                       ╰─────────────────────── accumulated 1001.23, error -1.23 NZD
────╯
2020-01-05  :Current  1000.00  1000.00  Pay     
2020-01-06  :Current  -400.00   600.00  Transfer
2020-01-06  :Savings   400.00  1000.00  Transfer
2020-01-10  :Savings     1.23  1001.23  Interest
```

**Unbalanced transaction** (golden):
```
Error: invalid transaction
   ╭─[ ../test-cases/error/booking-unbalanced-txn.beancount:7:1 ]
 7 │ ╭─▶ 2023-04-10 * "WIKIMEDIA" "WIKIMEDIA ;"
   ┆ ┆   
 9 │ ├─▶   Expenses:Donations                                                      15.00 NZD
   │ ╰───────────────────────────────────────────────────────────────────────────────────────── unbalanced transaction with residual -5.00 NZD
```
With two currencies it reads `unbalanced transaction with residual 3.00 NZD, 0.01 USD`.

**Currency constraint** (my run). A second, "related" label points at the `open` directive:
```
Error: invalid posting
   ╭─[ 04-currency-constraint.beancount:5:3 ]
 4 │ ╭─▶ 2020-02-01 * "Lunch in London"
 5 │ │     Assets:Bank          -15.00 GBP
   │ │     ───────────────┬───────────────  
   │ │                    ╰───────────────── currency incompatible with account
 6 │ ├─▶   Expenses:Food
   │ ╰───────────────────── in this transaction
   │
   ├─[ 04-currency-constraint.beancount:5:3 ]
 1 │ 2020-01-01 open Assets:Bank NZD
   │                ╰───────────────── open
```

**Other booking and validation messages** (all from my runs):
- A posting to an account that was never opened: `account not open`, with an "in this transaction" context label. The message does not name the account and suggests nothing.
- A posting after the account was closed: `account was closed`, with a related label on the `close` directive.
- A duplicate open: `account already opened`, with a related label on the first `open`.
- Reopening a closed account: `account was closed`, with a related label on the `close`.
- A balance in the wrong currency: `invalid balance` / `currency incompatible with account`, with a related label on the `open`.
- Pad errors come in three kinds:
  - `unused, no balance directive`
  - `unused, no balance adjustment required`
  - `unused, second pad encountered`, with a related label `pad` on the second pad.

**Booking and lot errors.** These are labelled on the whole transaction, never on the posting:
```
Error: invalid transaction
    ╭─[ 09-ambiguous-strict.beancount:14:1 ]
 14 │ ╭─▶ 2020-04-01 * "Sell some"
    ┆ ┆   
 17 │ ├─▶   Income:Gains
    │ ╰──────────────────── ambiguous matches on posting 0
```
The other lot and interpolation reasons I saw:
- `not enough lots to reduce on posting 0`
- `no position matches on posting 0`
- `inferred negative cost per-unit on posting 0`
- `ambiguous auto-post on posting 2` (from two postings with no amount)
- `cannot infer anything on posting 0` (from a missing currency)
- `can't have auto-post with multiple currencies USD,GBP,NZD`

The full list of reasons is in `limabean-booking/src/errors.rs`, for example `"too many missing numbers for interpolation"`, `"can't determine currency for balancing transaction"`, `"multiple currencies in cost spec matches against inventory"` and `"unsupported booking method {booking} for {account}"`.

**Parse errors** (my run). These are chumsky "Rich" errors:
```
Error: found 'D' expected '@', '@@', '{', '\n', or end of input in posting at 88..119 in transaction at 67..119
   ╭─[ 30-parse-bad-amount.beancount:5:34 ]
 4 │ ╭─▶ 2020-02-01 * "Typo"
 5 │ ├─▶   Assets:Bank          -15.00 NZ D
   │ │    ───────────────┬─────────────── ┬   
   │ │                   ╰──────────────────── in this posting
   │ ╰──────────────────────────────────────── in this transaction
   │                                      ╰─── found 'D' expected '@', '@@', '{', '\n', or end of input
```
Other parse and option messages I saw:
- `found 'ERROR month out of range' expected transaction, directive, or end of input`
- `found 'A' expected something else in directive at 0..15` (from `Assets:bank`)
- `unknown option in directive at 0..99`
- `Expected one of STRICT, STRICT_WITH_SIZE, NONE, AVERAGE, FIFO, LIFO, HIFO`
- `duplicate tag #a`, `duplicate key m1`
- `invalid poptag` / `missing corresponding pushtag`
- `invalid option` / `duplicate value`, with a related label `option value`
- `duplicate include` / `context #duplicate-include`, with a related label `context #included` pointing at the first include

**The only "hint" anywhere** (parser warning):
```
Warning: string too long
 22 │ ├─▶ except for this fourth line"
    │ ╰────────────────────────────────── exceeds long_string_maxlines(3) - hint: would require option "long_string_maxlines" "4"
    ├─[ ... ]
  1 │ option "long_string_maxlines" "3"
    │                                ╰─── max allowed
```

**Plugin errors** (golden). An error on a directive that a plugin created is rendered against a pseudo-source:
```
Error: limabean.test.plugins.fail
   ╭─[ Synthetic directive from limabean.test.plugins.duplicate-txns:3:1 ]
 3 │ 2023-04-10 * "WIKIMEDIA" "WIKIMEDIA ;"  Assets:Bank:Current -10.00 NZD
   │                    ╰──────────────────── Plugin configured to fail on any transaction
```

## 3. Techniques

- **Renderer.** ariadne, called from `beancount-parser-lima/src/sources.rs` `write_report`. Each report has:
  - a `message` headline, which is `"invalid {element_type}"`, or `"questionable {element_type}"` for warnings;
  - one red primary label holding the `reason`;
  - yellow **context** labels reading `"in this {element}"`, which put a posting inside its transaction;
  - yellow **related** labels named after the element type (`open`, `close`, `pad`, `option value`, `max allowed`).
- **Not used:** ariadne `with_note` / `with_help` are never called. There are no did-you-mean suggestions; I grepped all three repos for "did you mean", levenshtein and strsim.
- **Annotation.** This is free text appended after the ariadne box, not an ariadne note. It is used only for balance failures (`accumulator.rs::balance_report`). It is a `tabulator` table of every posting since the account's previous balance assertion (the "balance window"). Columns: date, the subaccount suffix (only when more than one account contributes), units, running total, and payee or narration. There are no column headers and no currency.
- **Localising balance failures.** After a failed assertion the account is reset to the asserted amount (source comment: "reset accumulated balance to what was asserted, to localise errors"). Beancount does not do this.
- **Errors by index, not span.** Booking emits `IndexedReport {directive, posting}` plus `related` indexes. Those are resolved to spans afterwards, which lets errors land on directives that plugins rewrote or created ("synthetic spans"). Plugins attach errors with `dct-error!`, and the plugin's namespace becomes the headline.
- **All errors in a phase.** Every error in a phase is collected and reported. Structured errors are also available as EDN/JSON:
  `{:message "invalid balance", :reason "accumulated 935.00, error -100.00 NZD", :span [0 746 831], :annotation "..."}`

## 4. Compared with Beancount

**Where limabean is better:**
- Errors are anchored in the source with underlines. Beancount prints `file:line: message` and then a reformatted copy of the entry.
- Related spans point at the cause: the `open` behind a currency violation, the `close`, the first `open`, the duplicate option, the first include and its context.
- The running-balance table since the last assertion. Beancount only says `Balance failed for '{}': expected {} != accumulated {} ({} {})`.
- Resetting the account after a failed balance, so one bad entry does not fail every later assertion.
- Three distinct pad messages, where Beancount has only `"Unused Pad entry"`.
- Errors render even on directives that plugins generated.

**Where it is weaker:**
1. **Lot errors carry less information than Beancount's.** Beancount says `'Ambiguous matches for "{}": {}'` and `'Not enough lots to reduce "{}": {}'` and lists the candidate lots. limabean says `ambiguous matches on posting 0`, uses a 0-based index, labels the whole transaction and shows no candidate lots or inventory. The source admits it: `// TODO attach posting error to actual posting`.
2. **Messages do not name the account or give the amounts.**
   - Beancount: `Invalid reference to unknown account '{}'`. limabean: `account not open`.
   - The balance message gives a signed `error -100.00` with no "too much/too little", does not restate the asserted amount, and does not show the tolerance.
   - Formatting is inconsistent (`accumulated 0` vs `0.00`).
3. **Some reasons are wrong or misleading.**
   - Two postings without amounts give "ambiguous auto-post", not "too many missing numbers".
   - A missing currency gives "cannot infer anything".
   - Closing an account twice gives `account not open` with no related label. From reading the source, the `"account was already closed"` branch looks unreachable.
4. **The parser leaks internals.**
   - Headlines contain byte offsets (`in posting at 88..119 in transaction at 67..119`).
   - Lexer errors show up as tokens (`found 'ERROR month out of range'`).
   - Some messages say nothing useful (`found 'A' expected something else`).
   - For an unknown option, the "in this directive" context span runs on to line 3.
   - With no context, the headline and the label repeat the same text.
5. **Warnings are dropped.** `server.rs` has `warnings: _ // TODO warnings`, and the Clojure side never calls `format-warnings`: my ledger with an AVERAGE booking method printed no warning. Two further bugs I read in the source but did not trigger: one copy of that warning is a literal `"booking method {} unsupported, falling back to default"` that never gets formatted, and it would be pushed twice.
6. **ariadne quirk.** The header of a related section repeats the primary's `line:col` (for example `├─[ …:5:3 ]` above line 1).
7. **Cascades are not explained.**
   - A transaction that fails to book is silently left out of a later balance table.
   - After a reset, the "accumulated" figure is synthetic and the table can be empty.
   - If the assertion itself was the wrong line, the reset can cause a spurious follow-on error (+20.00 in my 3-assertion test).
8. **Small bug in `error.clj`.** From the source only: `(:span-p :dct)` should be `(:span-p dct)`, so the "source" related label for plugin errors is never attached.

**Local files:** nothing in the project was changed. The clones, crafted ledgers and driver script (`run.py`) are in `v2/briefs/lb/`, and the crafted ledgers are under `cases/`.

Sources: [limabean](https://github.com/tesujimath/limabean), [beancount-parser-lima](https://github.com/tesujimath/beancount-parser-lima), [limabean-booking](https://github.com/tesujimath/limabean-booking), [lib.rs](https://lib.rs/crates/limabean), [Beancount balance.py](https://raw.githubusercontent.com/beancount/beancount/master/beancount/ops/balance.py) (plus `booking_method.py` and `validation.py` in the same directory).