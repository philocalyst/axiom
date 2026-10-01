You are **lane E4c (engine v4: units, norms and diagnostics)** of the Axiom v4 rework. First read `v2/briefs/common.md`. It is part of this brief.

**Read first:**
- `v2/LANGUAGE.md` in full, especially §6, §8 (norms, units, functions), §11 (filed returns) and §12 (diagnostics);
- `v2/DESIGN.md`;
- `theory.md` (defeasible norms, units of measure, the network view of double entry) and `limabean.md` (the benchmark our diagnostics beat), in `v2/briefs/`;
- `v2/examples/v4-sketch/README.md`: its `check` sketches are the target;
- the engine as lanes E4a and E4b left it.

**You own `v2/crates/engine/**`.** Build on hand-made books in `fixture.rs` and `tests.rs`, and keep v3 books' behaviour.

## Units and conversions

- Values carry their dimension at run time where the model left `Dim::Any`.
- `value(x, U [at POLICY])` converts at the owner's system's `rates`: `spot` is the day's price, `param NAME` is a param's row (such as the IRS's yearly average). The rate it used is recorded, so `why` can show it.
- Tallies count in their owner's currency. A gain is figured in the owner's currency at its system's rates.
- A conversion with no rate for the day is a `Missing` fault, reported once per missing thing, as today.

## Norms

- **`unless`:** the law does not apply.
- **`require A else B else C`:** if A fails, B's effect is owed (usually an `owe` with a deadline, or a `count`). If B is an obligation not met by its deadline, C applies. A repaired violation is reported as repaired: a note, not an error.
- **Ranks.** The model already chose, per subject, the most specific of same-named laws. The engine only has to respect `Law.rank` where rules meet at run time.
- **Functions:** `days(COND, window)`, `peak(x, window)`, `low(x, window)`, `open(^code)`.
- **Filed returns** (`Book.filed`). At the return's closing, compare each filed line with the tally for that owner, system and year. A difference is a `warning[amended]` that lists each changed line (filed, now, and the difference) and the flows that changed it since the filing day.

## Diagnostics: show the might of the architecture


LANGUAGE §12 is the spec. `limabean.md` shows the benchmark: a running-balance table, related labels, and nothing more. Beat it by using what only this engine knows.

- **A failed assertion shows its window.** Give a table of every flow since the last passing assertion: day, other end (the party, or the account), amount, running balance, and purpose or description. Mark the suspect row. Then name the likeliest cause. `Suspect` (from E4a) grows these variants:
  - **a contract occurrence due in the window and not written**, whose amount explains the gap. Put a related label on the contract's schedule line, "due 02-01, and not written", and give the fix `01 flat` in the right file.
  - **a claim the party settled off the book**: an open claim of that amount.
  - **a derived flow written again by hand**: a written flow equal to one the book derives (escrow, match, interest) on the same day.
  - **a pending flow that settled** without a `settled` statement.
  - **a flow dated just past the assertion that the bank dated inside it**, within three days.
  - the existing ones: transposition, doubled, backwards, wrong sign, and missing of that size.

  Rank by how exactly each explains the gap. The sketch README's `check` sketch is the target output.
- **Derived flows point where they come from.** A violation raised by a derived flow (an escrow that overdraws, a share that breaks a budget) labels the line that caused it and, as a related label, the declaration that derived it (the contract's `escrow` line, the party's `business` line).
- **Late promises** give the fix in both directions:
  - "record it: `05 lease`";
  - "if it was released: `03-01 lease waived`".
  
  Put a related label on the terms in force that day, which may be an amending statement.
- **Budgets** over the limit label the top contributing flows (at most three, then "and N more"). They offer both fixes as edits:
  - the amendment that would relax this window (`12-01 budget food 950 USD monthly until 12-31`);
  - the `!` that accepts it.
- **Suggest new terms.** Three occurrences in a row that differ from the contract by the same amount give `note[terms]`: "phone has been 47.30 three times; if the terms changed, `08 phone 47.30 USD monthly`".


- **Unsolvable unknowns name their cycle** (LANGUAGE §12.7). Merge every end that has no value into one node. `?` amounts are solvable exactly when they form a forest; otherwise name the cycle: the accounts and the flows.
- **Unit errors** found at run time, where `Dim::Any` met a fixed unit without a conversion, name both units, the law, and the fix.

## Constraints

- **Performance:** no regression from E4b's numbers. v3 books pay nothing.
- **Size:** at most +700 lines.
- The mistakes corpus: the assertion window table may change v3 assertion outputs. Regenerate them, and show before and after for two cases in your report.

Work in your worktree, from the commit the orchestrator gives you.
