# Research lane R5: what the language still cannot say, and stronger formulations

You are a research lane for Axiom v5. Axiom is a typed, plain-text ledger written in Rust. **Do not modify any
source code.** Write only under `docs/v5/research/` in this worktree (`/home/user/axiom/.claude/worktrees/v5`).

## Read first

1. `docs/v5/PROPOSAL.md`: the v5 plan. Seven kernels, plus the language changes in §6. Pay attention to these:
   - the junction verbs (`->` gives, `<-` takes, `@` exchanges, legs lead with arrows);
   - positions belong to agents, so there is no `account` keyword;
   - a debt is a promise;
   - direction decides income versus spending;
   - the counterparty is optional.
2. `LANGUAGE.md` and `DESIGN.md`: v4, the normative reference v5 changes.
3. `briefs/theory.md`: the theory v4 rests on, covering REA, ValueFlows, ACTUS, CSL/POETS, defeasible deontic logic,
   units of measure, FRP behaviours, XBRL and Ellerman.
4. `examples/explore-v5/FINDINGS.md`: five ledgers written against v4 and what they could not say.
5. `examples/05-family/accounts.ax` and `contracts.ax`, `examples/11-sam/`, `examples/v4-sketch/`,
   `crates/systems/src/std.ax` and `crates/systems/src/us/*.ax`.

## The problem the user named

In `examples/05-family/accounts.ax`:

```text
account jordan-401k : 401k at fidelity
  owner jordan
  employer bluefin
account hsa : hsa at fidelity
  owner me
  coverage family
account riley-529 : 529-plan at fidelity
  owner family
  beneficiary riley
```

The user's words: "the jordan-401k is proof that the typing is still a little weak, should be easy and declarative to
setup associations for accounts if need be." What goes wrong today:
- names encode relations (`jordan-401k`);
- `employer`, `coverage` and `beneficiary` are free property lines whose types nothing declares;
- nothing infers that Jordan's 401(k) is sponsored by Jordan's employer;
- nothing checks that a 529's beneficiary is a person.

Design a declarative, typed way to state associations. It should be easy when the book is simple and precise when it
is not.

## What to produce

All files go in `docs/v5/research/`.

### 1. `ledgers/*.ax`: realistic books in the proposed v5 syntax

Write one file per profile. Each should cover 3 to 6 months, use real-looking numbers, and follow the sketch's
conventions (`// ▸` marks what Axiom derives). Mark each line where v5 cannot say something, or says it badly:
- `✗` cannot say;
- `✎` says it badly;
- `?` unclear;
- `⇄` what sync would need.

The profiles:

- **`small-business.ax`**: a two-owner LLC taxed as an S-corp that sells physical goods online and does some service
  work. Include:
  - invoicing, receivables and late fees;
  - bills and payables;
  - payroll for three employees, with withholding, both FICA halves, FUTA/SUTA and a 401(k) match;
  - an officer's salary for an owner;
  - sales tax collected in two states and remitted;
  - **inventory**: purchases, cost of goods sold, returns and shrinkage;
  - a Stripe payout with fees, refunds and a chargeback;
  - annual subscriptions sold (deferred revenue) and an annual insurance prepayment;
  - accrued wages at month end;
  - equipment depreciation;
  - a credit line;
  - owner distributions and K-1 allocation 60/40;
  - quarterly estimated taxes, 1099 contractors, one customer paying in EUR.

  This is the most important profile: the language was designed for households, and a small business stresses it
  hardest.
- **`household.ax`**: two earners and a child. Cover two employers with plans, both 401(k)s with matches, HSA,
  dependent-care FSA, 529, a mortgage with escrow, a credit card, and a job change mid-year. **Show the association
  design here.**
- **`freelancer.ax`**: hours become invoices; mileage; a home-office share; quarterly estimates; a retainer client.
- **`landlord.ax`**: a triplex with the owner in one unit, tenants with deposits, a property manager at 8%, repairs
  versus improvements, depreciation per part, and a vacancy.
- **`investor.ax`**: lots, a wash sale, dividends with foreign tax withheld, a split, crypto with staking and gas,
  and a covered-call option (say what options need).
- **`expat.ax`**: income in EUR and GBP, FX, FBAR peaks, FEIE day counting, and a move between countries.
- **`shared.ax`**: roommates, shared bills by shares, claims, and settlement through Venmo.

### 2. `ASSOCIATIONS.md`: the typed association design

Theory to read and cite:
- **UFO / OntoUML** (Guizzardi): relators, roles, role mixins, phases. An employment, a 401(k) plan membership and
  a lease are each a *relator* that mediates its participants.
- **REA**: typification, participation, custody and responsibility relationships.
- **ValueFlows** `AgentRelationship` and `AgentRelationshipRole`.
- **Description logics**: role restrictions and qualified number restrictions.
- **Record typing**: row polymorphism and typeclass-like interfaces on kinds.

Then propose concrete syntax and semantics:
- How a kind declares typed role slots (holder, sponsor, custodian, beneficiary, coverage, tenant, unit, employer)
  and their cardinality.
- How a declaration fills them: by role word, positionally, or by inference when unique.
- How positions are addressed structurally, for example `jordan's 401k`, `401k of jordan` or `jordan/401k`. Choose one
  and justify it.
- How relators (`jordan works at bluefin`, a lease) imply positions, laws and `also` lines: plan eligibility, the
  match, a tenant's deposit.
- The errors: ambiguity, a wrong kind, a missing required role.

Show before and after for every association in `examples/05-family/accounts.ax`, plus a small business and a
landlord.

### 3. `THEORY.md`: a scan of stronger formulations

For each item, say what it is in two sentences and cite the primary source (author, year, venue or URL). Then give:
- the **concrete change** to Axiom, in syntax or in the kernel types of PROPOSAL §5;
- **what it removes or simplifies**;
- **what it costs**;
- **a verdict**: adopt, adapt or reject.

Cover at least:
- UFO relators and roles (see `ASSOCIATIONS.md`).
- **Linear and affine types for resources.** Money cannot be duplicated or discarded. Can conservation be checked
  statically on legs and templates?
- **Multiparty session types and choreographies** for promises: protocols between agents, with blame.
- **DBSP** (Budiu et al., VLDB 2023), differential dataflow, and Salsa-style demand-driven queries for an
  incremental fold and views. The user wants an MCP server and a GUI on top, so live edits matter.
- **Datalog with lattices** (Flix, Datafun, Ascent) for inference and rules.
- **Ellerman's Pacioli group**, and accounting as a group of differences.
- **Catala** (default logic), L4, LegalRuleML and Blawx for laws.
- **ACTUS** contract types, FpML, Marlowe and Findel for promises, including options.
- Bitemporal data (SQL:2011), and Allen's interval algebra for `for PERIOD` and `covers`.
- XBRL GL and OIM for facts; ISO 20022 for sync.
- Bidirectional transformations and lenses (Boomerang) for sync writing back into files.
- CRDTs and local-first software for a book edited on several devices.
- Anything newer (2023–2026) that you find that fits better. Search the web.

### 4. `REPORT.md`: the synthesis, which is what the orchestrator reads first

- **The twelve strongest changes, ranked.** Rank by how many profiles need each and by how much it simplifies.
  Each gets before and after syntax, the theory it rests on, and what it removes from the language and the code.
- **Per profile: what still cannot be said after those changes.**
- **A table** that maps every item of `examples/explore-v5/FINDINGS.md` (01a1 … 05d3) to: closed by v4, closed by
  the PROPOSAL, closed by your changes, or still open.
- **Small-business specifics.** Does accrual need anything beyond claims plus `books accrual`? What are inventory and
  COGS in REA terms: a resource with parcels, its cost flowing to `#cost-of-goods` on sale? How do equity and
  retained earnings show without equity accounts? How does a K-1 work?
- **What must change in the kernels of PROPOSAL §5** to support all this, with type sketches.

## Rules

- Be concrete: real syntax, real numbers, real citations. Do not pad.
- Prefer formulations that **remove** concepts. A new keyword must remove two.
- The orchestrator will review this brutally. Mark anything you are unsure of as such.
- Report back with a 25-line summary when done: the top changes, the association design in five lines, and the
  biggest open problems.
