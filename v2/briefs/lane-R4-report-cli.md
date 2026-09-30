You are **lane R4 (report and cli)** of the Axiom v4 rework. First read `v2/briefs/common.md`. It is part of this brief.

**Read first:**
- `v2/LANGUAGE.md` (v4, normative) in full, especially §0, §3, §5, §7, §8, §9, §10 (documents), §11 (`check --json`) and §12.
- `v2/DESIGN.md`.
- `v2/examples/v4-sketch/README.md`. Its `check`, `why condo` and `flow` sketches are the shape of your output.
- The public types in `crates/model/src/{book,journal,law}.rs` and `crates/engine/src/lib.rs`, with annotated sources in `v2/briefs/types-v4.rs` and `types-v4b.rs`.
- `v2/briefs/audit-engine-report-cli.md` findings 4, 5, 7, 8 and 10: part of your work list.

**You own `v2/crates/report/**` and `v2/crates/cli/**`, except `cli/src/sync.rs`.** The sync lane owns that file; ask it for changes through your report.

The v3 model still builds today's books and leaves the v4 fields empty. So build every v4 view against hand-made books and runs in `report/src/tests.rs`: the household fixture, extended with purposes, contracts (terms that change, waived occurrences), assets, shares, budgets and promises. Keep v3 books reporting as they do, with goldens unchanged, until the v4 model lands. Where a view needs `v3_side`, keep the bridge for v3 books, use purposes when flows have them, and mark the bridge `// v3 bridge:`.

## Work in two phases

**Phase 1 (now, from main at c7b352a):** the rework below, views as data, JSON, `check --json`, and `axiom fmt`.

**Phase 2 (when the orchestrator gives you lane C5's `types-v4c` commit):** the v4 views. They show promises, measures, units, `also`-derived flows, shares, filed returns and changes, built on fixtures.

Read LANGUAGE.md at main 5f15e35 or later in full: the spec was revised after this brief was first written. Where this brief and the spec differ, the spec wins.

**Views are data.** Today views render text as they compute. Split every view into two parts:
- **the data:** sections, rows, and typed cells (amount with unit, day, name, text, percent), plus facts;
- **the renderers:** text and JSON, in the CLI.

No view formats a string. JSON facts have the XBRL shape: concept, entity, period (instant or duration), unit and value. A GUI will be a third renderer, so the data must say everything the text shows.

**`axiom fmt [FILE…] [--check]`:** wire the formatter that lane S5 is writing in the syntax crate, `syntax::format(src, &File) -> String`. Until it lands, stub the command, and wire it when the orchestrator says.

## Rework first (the audit)

1. **One `Priced` accumulator** for the seven copies of "count the amounts that have no price", with one wording of its note (finding 10).
2. **`Section::total`**, which pads to the table's own columns, replacing the magic column counts (finding 10).
3. **`Sides`**: the display sign and side resolved once per place, not by parsing paths (finding 5). This is the one v3 seam.
4. **Well-known names resolved once**: `Lens { currency, .. }`, `maturity`, and no `"budget"` string compare (finding 8). The model's `Book.budgets` says which laws are budgets.
5. **`Snapshots`** indexed by the `(place, unit)` pairs that occur, not by a dense grid (finding 4). The `check` summary must not scan a grid.
6. **Views read the fold once.** Lane E4a is splitting the engine into an immutable `Plan` and a `Ledger` that borrows it, with a checkpoint at `today`.
   - Until it lands, leave `available`, `claims` and `forecast/projection` as they are.
   - When the orchestrator gives you the commit, merge it, and move those views onto forks of the checkpoint (finding 1), so nothing re-solves the journal.

## The views (phase 2)

These are in addition to what the spec now asks:
- **measures:** hours and miles, by purpose and owner, in `flow` and `register`;
- **promises:** contracts and claims in one `contracts`/`claims` monitor, with blame and deadlines;
- **`also`-derived flows:** shown once with the flow that implies them;
- **units:** rates such as `USD/MI` and `USD/HR` shown as written;
- **`tax`:** shows a filed return beside what the book says now, with amendments marked.

- **`flow`**: income, spending and capital by the purpose tree (the default), or by party (`--by party`).
  - Unclassified flows are grouped by their description, then "unclassified".
  - A shared flow shows each owner's share under the owner scope (`--for studio`), and the whole under everyone.
  - Capital flows show what they joined.
  - Line items show under their own purposes.
- **`balance`**: for each owner (or `--for`), everything it holds, then its net worth:
  - accounts, grouped by institution;
  - its own holdings;
  - assets, at price when one exists, else at basis, with the basis beside;
  - claims it holds on parties;
  - its debts: debt accounts, and loan contracts' debts.
- **`register`**: accounts, owners, parties, assets and contracts.
  - A party's register is every flow with it, with claims opened and settled.
  - An asset's register is its history.
  - A contract's register is its occurrences and its changes of terms, in order.
  - Pass-through halves and shares show once, as the bill was paid, with the share noted.
  - A flow with a document (LANGUAGE §10) is marked.
- **`contracts`** (a new command): every contract with:
  - its party;
  - its current terms (amount, cadence and holding);
  - the next due day, how many were kept, and what is late now (with days);
  - for loans, the balance.
  
  Terms changed by a statement show the change, and until when ("120.00 until 06-30, then 240.00: spring promotion").
- **`why`**:
  - **`why CONTRACT`**: its terms over time, each change with its statement, its occurrences, what each derived, and what it still promises.
  - **`why ASSET`**: parts, what laws consumed and when, and the flows about it by purpose, as the sketch shows.
  - **`why #purpose`**: its laws, its budget (limits over time, `carries`, and headroom), this year's total, and its largest parties.
  - **`why ^code`**: everything the code marks, what changed it, and its documents.
  - **`why "text"`**: flows with that description.
  - **`why FILE:LINE`**: everything the line produced. For each flow, give its inferred purpose and the `Provenance` it came from, and every derived flow with its `Derivation` and the declaration it came from. For a statement, give what it changed and for which days. This is the command form of the editor's hints, so it must read as well as they do.
  - **`why NAME`** for any declared thing lists its changes (props by day).
- **`budget` and `limits`**: budgets on purposes, as the limit stood each window, with `carries` shown as the running balance. A purpose law's headroom has the owner as its subject.
- **`claims`**: tabs as today, plus the promises that are late now (from `Run.promises`), aged. Itemized claims show their items.
- **`forecast`**:
  - Contracts replace plans: each contract's occurrences after today come from `due_days` and the terms in force on each day (a promotion that ends in June forecasts 240 from July). Loans derive their split.
  - Keep plans working for v3 books until the model drops them.
  - Habits (recurrences learned from history) remain only for flows no contract covers. The forecast lists them as "looks like a contract: declare it?".
  - Missing promises that are already late are expected at once.
- **`available`**: money in hand is the currencies held in owners' holdings and in `asset`-class accounts that hold money freely, less what is held for others, pending or falling due. Claims and late promises are coming in.
- **`tax`, `gains` and `lots`**: unchanged in shape. Tallies are by owner.

## Machines read the book too (LANGUAGE §12)

- **`--json`** on every view writes the same content as JSON: one document, stable keys in snake_case, amounts as strings with their unit, days in ISO. Build it as a second renderer over the same report structures, never as a second computation. Write the JSON by hand; there are no dependencies.
- **`check --json`** writes each diagnostic as one JSON object per line: code, severity, headline, labels (file, line, column, text, and whether primary), notes, helps, and fixes as edits (file, line range, replacement).

**CLI:** `contracts` is a new command; `register` and `why` accept the new targets; the help lists them; everything else stays.

## Constraints

- **Size:** at most +700 lines net, including the data/renderer split, JSON, `fmt`, and the new command. The rework items should pay back part of it. Delete what v4 makes redundant as soon as v3 books no longer need it.
- `cargo test --workspace --release` is green, goldens are unchanged, and new behaviour is tested on fixtures.

**Report:** what each view now shows, the fixture tests that prove it, what you need from the model and engine that they do not yet provide, and the line counts.

Work in your worktree, from main at c7b352a.
