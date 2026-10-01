You are **lane SY (sync)** of the Axiom v4 rework. First read `v2/briefs/common.md`. It is part of this brief.

**Your job:** make `axiom sync` the way a book stays current without typing. LANGUAGE §13 is the spec, and DESIGN §11 says why. The user said "a system operated at this scale for many businesses will be mostly automated, so it needs to be much cleaner to link invoices and other payment information, and support declaring behavior for the sync subcommand … to query/parse data and write updates back to the journal".

**Read first:**
- `v2/LANGUAGE.md` in full, especially:
  - §2 (flows, line items, `via`);
  - §3 (statements);
  - §5 (contract occurrences);
  - §7 (claims and settlement);
  - §10 (the date convention that decides where a line goes);
  - §13;
- `examples/v4-sketch/sync.ax`, `parties.ax` and `journal/2026/*.ax`: what sync writes should look like the lines there;
- today's `crates/cli/src/sync.rs`.

**You own:**
- a new crate `v2/crates/sync`;
- `v2/crates/cli/src/sync.rs`, and the CLI wiring for `axiom sync [NAME…] [--dry]`.

The report and CLI lane owns the rest of `cli`; if you need a change there, ask through your report.

## Build it in two layers

**1. Pure algorithms, over the crate's own small types.** These are independent of `Book`, so they are testable now.
- **CSV.**
  - Quoted fields, doubled quotes, CRLF, a header row, and columns by header name or 1-based index.
  - Date formats from a pattern (`YYYY-MM-DD`, `MM/DD/YYYY`, `DD.MM.YYYY`, `M/D/YY`…).
  - Amounts with thousands separators, parentheses for negatives, a leading currency sign, and `debit`/`credit` column pairs.
  - A malformed row is a diagnostic naming the file, the row and the column, and never a panic.
- **`Record`:** day, signed quantity, memo, optional balance and pending flag.
- **Recognition.**
  - `known-as` globs are matched case-insensitively, and the longest literal match wins. A tie is an error naming both.
  - An inner/outer match gives `via` (`PAYPAL *ETSY SELLER` is etsy-seller `via` paypal).
  - Code extraction uses the declared code globs, and the extracted code is lowercased.
  - Precompile the patterns once: at a million records this is the hot path. Use memchr-driven matching, not per-record allocation.
- **Reconciliation.** Match records to flows already on the account: same amount, within three days, nearest day first, each flow matched at most once. It must be deterministic and a multiset: two identical coffees on one day are two.
- **Promises.** A record with a contract's party, within half a cadence of an unwritten due occurrence, becomes that occurrence. The line is `DD NAME`, or `DD NAME AMOUNT` when it differs.
- **Writing.** Render lines in the house style, with dates as short as the destination file allows (§10: path, then headings). Insert them in day order into the file's text, after the last item of the same day, without reformatting anything already there. Create the file if needed.
  - The last `balance` becomes an assertion.
  - `pending` records are written in parentheses.
  - An unrecognized record goes to `?` with its memo as its description.
- **File and param sinks.** Merge the command's Axiom output into the file (or param rows): an item already present, with the same day and subject (or the same row key), is kept as written.
- **Dry run:** a unified diff of every file that would change.
- **Idempotence:** a second sync of the same input writes nothing. Test it.

**2. The binding to `Book` and `Run`.** Lane C5 is adding the public types (`Source`, `Sink`, `Csv`, `Money`, `Column`, `known_as`, `Contract::due_days`) on main. When the orchestrator tells you, merge the given commit and write the adapter:
- recognition tables from `Book.entities` and `Book.places`;
- written flows per account from `Book`;
- due occurrences from contracts;
- open claims by party from the `Run`.

Until then, define the adapter's inputs as small traits or structs in your crate, so layer 1 does not wait.

**Commands.**
- Run each source's command with `std::process::Command` from the project root.
- Substitute `{since}` (the day after the last record the book has from this source, or the book's first day), `{today}` and `{units}`.
- Run sources in parallel with `core::par`. A failing command writes nothing and shows its stderr.
- The core never opens a network connection itself.

**Also:** `check`'s list of unrecognized memos, grouped, each with the `known-as` line that would recognize it. Build the grouping here as a function the CLI calls; the orchestrator will wire it into `check` with the report lane.

## Verify

- Unit tests per piece.
- An end-to-end test with a fixture project:
  - a CSV export for checking and one for a card;
  - the transfer between them appearing in both;
  - a hand-typed flow the feed also has;
  - a rent that keeps a contract;
  - an invoice payment whose memo names `INV-2026-01`;
  - an unknown memo.

  Sync twice and assert the second writes nothing. Then assert the journal reads as the sketch's does.
- **Performance:** reconcile a 100k-record feed against a 1M-flow book in well under a second. Report the numbers.
- **Size:** aim for about 900 lines of non-test code for the crate.

Work in your worktree (from the main branch at 26a396c). Commit in steps.
