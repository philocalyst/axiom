# Lane L1: the junction says what happened, one line grammar, and `fmt --upgrade` ports the books

Read [`common.md`](common.md) first. Then [`../PROPOSAL.md`](../PROPOSAL.md) §6.1, §6.5 and §6.6 (the table of what each
junction means and the before/after of a paycheck), LANGUAGE.md §2 to §3 (the line grammar and the split flow as they are),
`crates/syntax/src/{flow,journal,statement,contract,lines,style}.rs`, and the maps of the lanes before you (`K4a-map.md` for
the `Group`/`Leg`/`Item` vocabulary the AST lowers into). Your worktree is `/home/user/axiom/.claude/worktrees/lane-l1`, on
branch `claude/great-wozniak-pnqn7x-v5-l1`. **This lane starts when the kernels have merged** (K3b, K4c, K5c at least: it
rewrites every example and so conflicts with any lane that edits one).

**Your crates:** `syntax` (the grammar, the AST's flow node, the formatter), `cli` (`fmt --upgrade`), and the **examples,
tests, docs** (text only). **The model, engine and report do not change in this lane**: that is the proof that the
junction is syntax. The semantic parts of §6 (positions under their agent, an optional counterparty, purposes without a
direction root) are lane L2 and L3, written when this has landed.

## What is wrong

A line's verb column is meant to say what happened (LANGUAGE §2), but for 3,009 flow lines in the examples it always says
`->`, so a reader cannot tell a purchase from a paycheck from an exchange without reading both ends:

```text
2025-03-14 acme -> joint-checking 12_000.00 USD #wages          // the party is the subject: income reads like spending
2025-01-05 checking 1_499.99 USD -> fidelity 5.2237 VTI @ 287.15 USD   // an exchange: an amount on each side
20 lumen 9_200 USD ->                                            // a one-ended arrow: where does it go?
```

and the parser pays for it: `transaction()` and `statement()` are two productions for one shape, `contract.rs::at_schedule`
clones the lexer for four lookaheads to tell a schedule from a flow, a leg and an item differ only by whether an end is
named, and the formatter aligns three sets of columns.

## What to build

**The rule: the subject of a money line is the book's own side** (an account, an owner, a position), and the junction is the
action:

| line | action |
|---|---|
| `checking -> trader-joes 84.20 USD` | **give**: checking pays |
| `checking <- acme 5_750 USD #wages` | **take**: checking receives |
| `fidelity <- 7 VTI @ 285.70 USD` | **exchange, buy**: fidelity buys 7 VTI, and the money leaves fidelity |
| `fidelity -> 1.62 VTI @ 297.00 USD` | **exchange, sell**: the proceeds stay at fidelity |
| indented `-> irs 692 USD`, `<- acme 40 USD` | **a leg of the same event**, the arrow explicit |
| indented `+ 4 USD #tip`, `- 60 USD`, `32 USD #gifts` | **items**, unchanged |

1. **One production.** `DATE SUBJECT VERB ...` with a verb table; `transaction()` and `statement()` merge, `flow.rs` and
   `journal.rs` fold into `statement.rs`, and a contract line leads with a keyword or an arrow so `at_schedule`'s lookaheads
   go. **Measure what this deletes first** and put the number in the map.
2. **The AST says what was written and lowers to what the model already reads.** `<-` is `->` with the ends swapped and a
   `Junction` field the formatter keeps; `@` is the price on the exchange's one amount. A flow's AST node normalizes, so the
   model is **byte-identical**: if you find the model needs to know the junction, stop and tell me why before adding it.
3. **Legs lead with their arrow**; an item never does; a leg between two parties still passes through the owner
   (LANGUAGE §3), now because the syntax says so.
4. **The formatter** aligns one set of columns and prints the verb the junction names; it works from AST locations and
   keeps every comment and blank line (K0a's formatter standard), idempotent.
5. **`axiom fmt --upgrade`** rewrites a v4 book into v5 mechanically from the AST: flip party-subject lines to `<-`, put
   arrows on legs, turn `A X -> B Y @ P` into the `@` form where the amounts say it is an exchange, nest nothing (that is
   L2). It never changes a name, an amount or a comment, and it **refuses** (a diagnostic naming the line) where it cannot be
   sure which end is the book's own side (both ends owned, or neither): those lines are listed, not guessed.
6. **The old forms are accepted for one release with a `v4-syntax` warning that names the upgrade command**, as one small
   module (`syntax/legacy.rs`) that lowers into the same AST. The last commit of the lane deletes nothing: removing the
   legacy module is a later one-commit decision for me. (A clean break is smaller; this is the courtesy, and the module
   is the line count of the courtesy: report it.)
7. **Every example is upgraded** by running `fmt --upgrade` over `examples/`, `tests/` fixtures and the docs' code blocks
   (the `systems/` standard library has no flow lines: check). A second run changes nothing.

## The proof

- **Parse equivalence, the whole proof of "syntax only"**: for every example and every fuzz book, the v4 text and its
  upgraded text produce the **same lowered `Book`** (compare the model's debug dump or a hash of it; build the comparison
  into a test, not a one-off) and the same `check`, `balance`, `flow`, `tax`, `claims` and `forecast` output, byte for byte.
- **Goldens and mistakes**: the *output* is byte-identical. The mistakes corpus is the one place where the **text** changes
  with the syntax (spans and messages quote lines): regrade it once, and list every message that changed with the reason
  (a quote of a `<-` line, a new diagnostic for a malformed junction). A new mistake book per new diagnostic.
- **A generator for lines** in `docs/v5/measure/` (seeded, in the style of `splits.py`): random legal flows of every shape
  (give, take, exchange buy and sell, one-sided split with legs and items, a contract's templates), rendered in v4 and in
  v5, compared as above; mutation-test it (swap the sides, drop a leg's arrow, flip buy/sell: each must fail).
- **Malformed junctions get gorgeous diagnostics**: `<-` between two parties, an exchange with no price, a leg with no
  arrow where one is required, `->` with the subject a party that no owner holds. Each says what it saw, the three words
  that would fix it, and points at the junction. Mistake books for each.
- Parser throughput on a 1m-line book: no slower (`sh bench/run.sh 1m`; three runs, fastest, load average).

## Rules of this lane

- Common bar: functions under 40 lines, no bool parameters, no parameter bundles, no `unsafe`; the verb table is data
  (`const` table keyed by the token), not a chain of `if`.
- No test deleted or weakened: a test that spells a v4 line stays as a legacy test **and** gains a v5 twin.
- LANGUAGE.md, the cheat sheet and `docs/v5/DESIGN.md`'s examples are rewritten in the new spelling, the one place prose
  changes; the semantics paragraphs do not.
- Do not touch `model`, `engine`, `report`, `sync` source (their *tests* that spell a line may change to the new spelling
  only through the upgrade).

## Step 0: the map

`docs/v5/lanes/L1-map.md`, committed first: today's productions for a line and what each costs in lines (`transaction`,
`statement`, `at_schedule`'s lookaheads, `flow_head`, `body`); the verb table with every case and the example line that
shows each (count them in the examples: gives, takes, exchanges, legs, items, one-ended arrows); how `fmt --upgrade`
decides a line's subject and the cases it refuses (count them on the examples: if more than a handful, the rule needs
the model's owner set, and the map says so before the lane builds); the legacy module's size; what the old forms
would cost to keep versus drop.

## Measure

Lines for `syntax` before and after, the legacy module separately; the histogram; the examples' line counts before and
after (the upgrade should shorten some: say which); the parser's timing; the mistakes that changed.

## Not in this lane

- L2: a position belongs to its agent (`entity chase : bank` with `checking : deposit` nested; `fidelity/brokerage` paths),
  a debt as a promise (`rocket` holds `mortgage` with its contract): K3b's address resolution is the foundation, and the
  `account` keyword goes then. L3: an optional counterparty (defaults to the position's agent), and purposes without
  `income`/`spending` roots (the engine already derives the direction). Both change the model and wait for K3b and K6.
