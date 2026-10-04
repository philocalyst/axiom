# Lanes L2 and L3: positions under their agent, an optional counterparty, purposes without a direction root

You are lane U (the Opus lane) in the same worktree (`/home/user/axiom/.claude/worktrees/lane-unify`, branch
`claude/great-wozniak-pnqn7x-v5-unify`; merge the v5 head first). Read [`lane-U-unify.md`](lane-U-unify.md) (the rules and
the bar still hold), [`../PROPOSAL.md`](../PROPOSAL.md) **§6.2, §6.3, §6.4 and §6.5** (the language as the user signed it off),
`K3b-map.md` (addresses: the declarative association of an account with its agent and owner is yours to build on, not to
undo), `L1-map.md` (the junction is done: `fmt --upgrade`, the `v4-syntax` hint, and why the upgrade needs the book),
`K5d-map.md`, `K5e-map.md`, `K3f-map.md` (a loan is a contract plus a debt tab plus an account today), and STATUS's **"Lane D,
the books, in numbers"** (six gaps between what LANGUAGE says and what the engine builds, found by porting `08` to `10`;
several are the same fault as §6.2 to §6.4: say, for each, whether your design makes it fall out or leaves it).

The user's words on this: *"the jordan-401k is proof that the typing is still a little weak; it should be easy and declarative
to set up associations for accounts."* and *"stronger formulations rooted in bleeding-edge theory"*; and on the whole pass:
*less is more, to the extreme*. **These two lanes are the one place the tree can get smaller by changing the language rather
than the code**: every concept the language drops is a pass, a table and a diagnostic family that go. So the measure of the
design is how many model, engine and report concepts it deletes, and it must be as good for a family, a freelancer, a
landlord, an expat and a small business as for the examples: the examples under `examples/` and
`examples/explore-v5/` are the corpus, `05-family`, `07-landlord`, `08-expat` and `explore-v5/02-family` the hard cases.

## What the language says after (PROPOSAL §6, restated; the map may correct it where the code or the corpus disagrees)

- **L2 positions belong to the agent that holds them** (§6.2): `entity chase : bank` with `checking : deposit`, `card :
  credit-card` and so on nested under it, `fidelity/brokerage` for a short name that is ambiguous; a debt is a promise
  (`rocket : lender` holds `mortgage : loan ... for condo` with its schedule lines and `also`, so the debt is written once,
  not as an account plus a contract). What goes: the `account` keyword, `at PARTY`, `entity chase : org`, one debt
  written twice, `kind receivable` and `kind payable` (a claim is a position with any party). Ownership stays a property
  of the position (`owner jordan`): *who holds it* and *whose money it is* are different questions, and K3b's slots already
  answer the second; keep one mechanism for both, do not add a second.
- **L3 the counterparty is optional and purposes drop `income` and `spending` as roots** (§6.3, §6.4): a flow with a purpose
  or a description that says why needs no party (`06 card -> 94.21 USD #dining`), and the counterparty defaults to the
  position's agent (`31 savings <- 170.70 USD #interest` is from chase); the engine already derives direction
  (`lib.rs purpose_direction`), so `#interest` paid to a lender is spending and received from a bank is income,
  `#rent` to a landlord is spending and from a tenant income, with no `pays` line; the party still decides refund versus income.

## How you work: a design stop, then builds

**Phase A (read-only, then STOP): `docs/v5/lanes/L2L3-map.md`**, committed and pushed, then your final message. It contains:

1. what each of the four changes deletes and adds **by file and function**, counted strictly (what no longer exists), against
   the code as it is after C3: the `account` declare passes, the implied-party walks (`declare/parties.rs`, `mentions.rs`),
   `Kind` receivable/payable, the direction roots and every reader of them, `spelled.rs` and `addresses.rs` (what survives as
   the resolution of a nested path), the loan's three records (`contracts` + account + debt tab, `loan_opening.rs`,
   K5d/K5e), the syntax productions;
2. the **language as the examples would read after it**: `05-family`'s institutions and one month, `07-landlord`'s mortgage,
   `08-expat`'s two countries, a small business (a sole proprietor with a business account, a card and an invoice) written
   out in full, so I can judge the surface before it exists; the ten places where the design is weakest, said plainly;
3. the **types** (the entity/position tree: arena and ids, what an `Address` is now, how a debt position holds its contract,
   how direction is derived, what the optional counterparty resolves to), and which of today's types and passes they replace;
4. the **proof plan**: where the Book changes and where it must not. L1 was syntax-only and proved it by parse equivalence;
   these change the language, so the proof is (a) an upgrade porter (a one-off program in `docs/v5/measure/`, not a permanent
   `fmt --upgrade` path unless the map shows that is smaller; the grammar then accepts only the new forms, with one hint per
   removed form naming the new one) that ports every example, test fixture and doc code block mechanically (hand-edit only what
   it refuses, and list it), (b) the check that the ported book's **outputs** (`check`, `balance`, `flow`, `tax`, `claims`,
   `forecast` at the dates `diff/` covers) are byte-identical to the old book's wherever the semantics did not change and
   differ only in the ways the map lists, with the independent verifiers (`examples/verify/verify*.py`) green;
5. the **order** of builds (L3's optional counterparty and the purpose roots are the smaller and independent pair; L2's
   nesting and the debt-as-promise merge are the large one: say whether the debt merge is a third lane), each a green series,
   and a **line estimate for the whole**, with your ratio of delivered to planned applied (C1 to C3 delivered 28%);
6. the **risks to features**: nothing the user has today may be lost; where the new language cannot say what the old one could,
   say so before building.

**Phase B (after my review): build**, in the order the map says, small green commits, byte-identical proof as above,
goldens and mistakes regraded **once per language change, in one commit, with every changed message listed and its reason**,
a mistake book per new diagnostic, the examples ported by the porter, LANGUAGE.md, the cheat sheet and `DESIGN.md`'s
examples rewritten in the new spelling. Net lines in `model`, `engine`, `report` and `syntax` must **go down**; if a part
adds more than it deletes it needs a reason that is not the count, and the map says so.

## Rules

Common bar and lane U's rules. Do not touch `crates/engine` beyond what direction derivation and the debt position force
(sibling lane U-E is finishing C4 there; merge its work when it lands: I will tell you). Do not touch `report` beyond names
(sibling lane U-C6 owns it for now). No legacy reader that costs more than a hint per removed form. Never push to a branch but
yours; commit trailer exactly as in your brief; small commits; a container restart loses uncommitted work.
