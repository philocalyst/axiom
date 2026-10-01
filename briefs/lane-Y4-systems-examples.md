You are **lane Y4-{PART} (systems and examples)** of the Axiom v4 rework. First read `v2/briefs/common.md`. It is part of this brief: context, reading list, quality and working rules, report format.

**Read first:**
- `v2/LANGUAGE.md` and `v2/DESIGN.md` (v4);
- `v2/examples/v4-sketch/`: the target surface, with its README;
- `crates/systems/src/std.ax`, which the model lane ported to v4.

Your branch has the whole v4 toolchain (syntax, model, engine, report), so everything in the spec runs. **You write Axiom, not Rust.** When the toolchain gets something wrong, write a minimal repro into your report under "bugs found". Never work around a bug silently: a workaround carries a `// WORKAROUND:` naming the repro.

**The rule for every file you touch: accounts only where money sits.**
- No `income/`, `expenses/` or `equity/` accounts.
- Parties are the other end of flows, typed so their purpose follows.
- Recurring things are contracts.
- Assets have histories.
- `#purpose` is written only where inference cannot know it.
- `"descriptions"` carry what v3 kept in comments on a flow.
- Codes are `^code`.
- Dates are as short as their file's place allows.

## PART A: the systems, examples 01–05 and the sketch

1. **`us` and its subsystems** (`401k`, `ira`, `hsa`, `529`, `ca`, `ny`, `nyc`, `san-francisco`) in v4:
   - purposes and their laws (wages, tax-paid, mortgage-interest, property-tax, charity, premium, pre-tax-deferral…);
   - party kinds (employer, tax-authority, charity);
   - account kinds with `takes … from …` (a 401(k) takes pre-tax deferrals from wages);
   - `irs`, `ftb`, `ssa` and `edd` as entities;
   - `us/rental` (new): `rental-home` with its depreciation law (`straight-line` and `consume`, per part), and rental income and expense counting by `of`;
   - a wash-sale law with `carry` (`us` or `us/securities`).

   Keep every slot, closing law and figure v3 had: the return's numbers must not change for the same facts. Every law keeps its note and `To fix:`.
2. **Examples 01, 02 and 03** stay small and pedagogical, rewritten in v4. Their numbers must not change (goldens are regenerated, and every diff must be surface only), unless a v4 semantic legitimately differs; say why.
3. **Examples 04 and 05** are rewritten in v4, with their `README.md` and `outputs/` and their verify scripts (`examples/verify/verify04.py`, `verify05.py`, with `axparse.py` taught v4 syntax). Every hand-checked number must still agree. Make each example use v4 where it is natural:
   - **04 freelancer:** clients typed `of` the business; invoices as `owes` with `^codes`; the phone and the home office as shares on contracts; estimated tax `for` a year; the laptop as an asset.
   - **05 family:** the household; contracts for pay, the mortgage (a loan with escrow) and childcare; the house as an asset; the 529 and HSA by their kinds' `takes`.
4. **The sketch becomes example `11-sam`**, runnable: the same story, completed with a `systems/` stub only if `us` lacks something. Verify its README numbers independently (`verify11.py`), and correct the sketch's illustrative figures to the truth.

## PART B: examples 06–10, and the mistakes corpus

1. **Examples 06–10** are rewritten in v4 in the same way, with READMEs, outputs and verify scripts:
   - **06 investor:** a fund pays dividends as a party; wash sales are derived by the law, not written; ESPP; the split.
   - **07 landlord:** the house as an asset whose improvements are parts; the lease as a contract with its deposit; the mortgage as a loan; the sale as one statement.
   - **08 expat:** currencies, with the exchange cost derived; residences.
   - **09 shared:** claims on friends; shared bills as `owes`.
   - **10 budgeter:** budgets on purposes; envelopes; subscriptions as contracts with `ends`.
2. **The mistakes corpus** (`v2/tests/mistakes/`):
   - Convert each case to v4 syntax where the mistake still exists.
   - Replace cases whose mistake v4 makes impossible (an income account typo, for example) with the v4 mistakes a user now makes: a purpose typo; two purpose sources disagreeing; a shortened date in a file with no place; `#code` for `^code`; a party-to-party leg outside a transaction's owner; an occurrence of an unknown contract; a missed promise; `.basis`.
   - Regenerate the outputs, and regrade them against `REPORT.md` §14 in a short new section, "v4 cases".

## Verification (both parts)

- `cargo test --workspace --release` passes, and `sh tests/golden.sh` has been run with every diff explained in your report.
- Every `verifyNN.py` agrees with its README.
- Every example's `check` is clean apart from what it teaches.
- Your report lists, per example, what v4 simplified (lines of journal and declarations, before and after), and which FINDINGS items are now closed.
