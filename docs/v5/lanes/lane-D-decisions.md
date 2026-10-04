# Lane D: the user's decisions, built

Read [`common.md`](common.md) first, then the **Decisions** table of [`../STATUS.md`](../STATUS.md) and, for each item below,
the section it names. Your worktree is `/home/user/axiom/.claude/worktrees/lane-d`, on branch
`claude/great-wozniak-pnqn7x-v5-d`. **This lane starts after K6b has merged** (the fee leg and the claim touch `post.rs`
and `claims.rs`), and **its example edits wait for L1** (which rewrites every example): do the code items first and the
book items last, from a branch that has merged L1.

The user decided every open question by taking the most intuitive and the most capable reading. This lane builds them. Each
item is **its own commit**, green, so one that proves wrong is one `git revert`. Behaviour changes are the point here, so each
commit lists the outputs it changed (golden, mistake, example) and why that output is now right. Anything beyond what an
item says is not in this lane.

## The items

1. **Prorata basis, reading R1 (K3c-map §5).** `post.rs` keeps the rule that what arrives in a `basis zero` place from a place
   that is not `deferred` has no basis unless the flow states one. The failing test
   `a_prorata_place_realizes_only_the_lots_share_and_deferrals_merge_into_one_lot` gets a **stated basis on its second flow** (a
   fixture edit: `basis 6_300 USD`, say what the test then asserts and that the HSA test is unchanged). A nondeductible
   contribution in the language is `basis AMOUNT` on the flow: check LANGUAGE §9 says so, and add the sentence if it does not.
   No golden should move.
2. **The fee leg of an exchange split is a cost of the exchange (K4b, STATUS Decisions 4).** `girokonto 900 EUR ->` with a leg
   `fx-fees 4.77 EUR` and a leg `us-checking 1_027.63 USD`: the fee adds to the basis of what is bought (and comes off the
   proceeds of what is sold). The case `08-expat`'s README describes (basis 9,500.00 USD, not 9,444.90). The solve of K4b makes
   a split; the rule is where the fee leg's purpose and the exchange's two ends meet: find it in the code, put it where one
   reader decides it, and write a test with the README's numbers. The promise's `= AMOUNT` leg is **as built**: change nothing.
3. **One `missed-occurrence` per run (Decisions 7).** `engine/monitor.rs` warns for every missed due day. A **run of
   consecutive missed due days of one contract** (no kept occurrence between them) is **one** warning: how many, since which
   due day, the last, and the edit that ends it (`until DATE` after the last kept day, spelled as the edit the diagnostic
   machinery already offers). The record per promise (`Promise.kept == None`) stays one per due day, because `why`, `overdue`
   and the claim read them: only the **diagnostic** is grouped. A contract with one missed day warns as it does today.
   Mistake books for a single miss, a run, and two runs with a kept day between.
4. **A missed occurrence's claim carries the contract's purpose (Decisions 11).** `claims.rs::claim_missed`: the claim made when
   a party-owed `due` is missed takes the contract's own purpose, so accrual counts it when it is found and cash when it is
   paid. One line and one test, as K5c's report said. Say which goldens move (probably none).
5. **Mortgage interest (Decisions 15, option c).** `systems/us.ax` (`crates/systems/src/us.ax`, line 103 `purpose
   mortgage-interest : spending` and the law around line 270) reads `#mortgage-interest` and no loan writes it. Make the
   itemized deduction read **`#interest of ASSET` for a home-kind asset**, the asset's shares deciding the personal part, so a
   loan `for` a home needs no special purpose and one interest posting never counts twice (rental share and personal share
   add up to the posting). `05-family` books its house interest as `#mortgage-interest` and the car's as `#interest`: both
   become `#interest`, and the purpose `mortgage-interest` goes if nothing else reads it. Read the K5d map and `examples/07-landlord`
   first: the tax numbers of 07 and 05 move; list them old to new, and check the rental and itemized figures add to the interest.
6. **Words, docs and the grammar nobody reads (Decisions 8, 17).**
   - `Books::Accrual`'s doc and LANGUAGE §6 say a claim counts **when it is made** (§7's "invoiced"); `AccrualAt::Due` stays the
     one-line alternative and a test (it is untested today: write it).
   - `claim`, `debt-claim` and `principal` are reserved words; LANGUAGE lists them in one place.
   - `?` beside `...` in a split stays `cannot-infer`; LANGUAGE says so.
   - The employer `match`: STATUS says the grammar accepts it and `Match` is never set. **Check the code first**; if it is gone,
     say so and do nothing; if the grammar still accepts it, remove it and make its diagnostic name the `also` line that does
     what it meant (a mistake book).
7. **The year-end test (Decisions 10).** `report/src/source_tests.rs:1129`: the book says `each year closing 12-31`; LANGUAGE §8
   closes the 2026 year on 2027-12-31, so the `year-end-tax` due 2027-01-15 cannot exist. Change the book's input to `each year`
   (the spelling the language defines). The sibling test K5c added (line 1185) stays **only if it asserts something the original
   does not**; if the two are now the same, merge them into one and say so. No assertion is weakened.
8. **The books (after L1).** `examples/07-landlord`: `grace 30d` on the manager-fee contract (the 2025-12-29 payment keeps its
   occurrence again; the 12-31 assertion on `rental-bank` agrees). `examples/03-violations`: the stale "expect: purpose conflict"
   comment on line 60 says what happens (the written purpose outranks the grocer's, and the flow posts). `examples/11-sam`:
   declare the purposes `match` and `escrow` it uses (or use the ones the system declares: whichever the book's own README says).
   **`examples/08-expat`, `09-shared`, `10-budgeter` are v3 books**: port them to v5 (chart accounts to addresses, `via
   income/salary` to what the address resolves, `/ party` the other way round, and `fmt --upgrade`'s output) until `check` has no
   error that is the port's. Their READMEs name numbers; list where the port changes one and whether the README or the book is
   right (an independent verifier is in the README of each where it exists). Regenerate the goldens only for what these edits move.

9. **Two bug fixes K12b listed (K3b-map section 7 and the K12b brief).** (a) `owner` has no range: an `owner` slot accepts
   `acme/529`, placing `acme` as the owner of an account without a `wrong-kind` error. Give the slot the range it means (an
   entity that can own: the kind says), so the line is refused with the usual `wrong-kind` and the edit. (b) `unknown-address`
   suggests the closest *name*, not the closest *address* the leading words point at (K3b-map section 4's design): the
   suggestion must be something that can be pasted in place of what was written. A mistake book each, the old behaviour in the
   map. Lane U rewrites `resolve.rs`, `fill.rs` and the address passes after you, so keep both fixes small and tested.

10. **Two things lane U's instruments found.** (a) `currency USD` cannot be written: the built-in property reads a *name*, but a
    currency is lexed as a *unit* (`docs/v5/measure/diff/cases3/` has the book that shows it, on the `unit-currency` or
    nearest name; read `declared.py` for the case). Fix the one reader so the line a book needs is accepted, with a test and
    the old failure in the map. (b) Two engine tests, `computed_kind_totals_*` and `kind_totals_use_descendants_*`, fail **in
    debug builds only**: they rebuild `book.kinds` after the holders are numbered and trip the numbering check in
    `holders.rs`. The fixture is inconsistent, not the product: fix the test input so they pass in both profiles. (Run the
    debug build with `CARGO_INCREMENTAL=0` and delete `target/debug` afterwards: the disk is shared.)

## The proof

- The workspace tests: all green, **including the two that were the known failures** (items 1 and 7). `cargo fmt --check`.
- Goldens (`sh tests/golden.sh`) and mistakes (`sh tests/mistakes/run.sh`): byte-identical except the list, each entry with the
  item it belongs to and the reason it is more right. A new mistake book for each new diagnostic (item 3, item 6's `match`).
- A differential run (`docs/v5/measure/diff/`, `fuzz.py ... diff`, `splits.py`) against the baseline built at your starting
  commit: every difference is in the area of an item. List the counts.
- `check` on `bench/` 100k and 1m: not slower (three runs, fastest, with the load average).

## Rules

Common bar. Functions under 40 lines, no bool parameters, no `unsafe`, no new dependency. **Net lines should not grow**: each item
is small; if one needs more than 80 lines, say why in the report. No test deleted or weakened. Commit trailer exactly
`Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>` and `Claude-Session: https://claude.ai/code/session_01DZMgABaaMrSoCXHzmY1D6u`;
no model name anywhere else. Push only `claude/great-wozniak-pnqn7x-v5-d` (`git push -u origin ...`); no pull request.

## Step 0: the map

`docs/v5/lanes/D-map.md`, committed first: for each item the file and function where it lives today (found by reading, not by
this brief), the lines it touches, what it changes in output (run it and look), and the order you will do them in.
