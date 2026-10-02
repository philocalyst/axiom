# Theory scan: stronger formulations for Axiom v5

Lane R5. For each item: what it is (two sentences), the primary source, the **concrete change** to Axiom (syntax, or the kernel
types of PROPOSAL §5), what it **removes**, what it **costs**, and a **verdict** (adopt, adapt, reject).

**How I verified.** Marks on every citation:

- **[V]**: read in the primary text, or in the primary repository that I cloned (`valueflows/valueflows`, `nemo-ufes/gufo`,
  `actusfrf/actus-dictionary`, `formancehq/numscript`, `tigerbeetle/tigerbeetle`, `ledger/ledger-semantics`).
- **[S]**: confirmed by a search result giving title, authors, venue and year; I did not read the paper.
- **[M]**: from memory, unchecked. Treat as a pointer, not a citation. Almost all web hosts were blocked by the egress proxy, so
  few papers could be read.

**What the repo already says.** `briefs/theory.md` covers ValueFlows, ACTUS (PAM, ANN, NAM, LAM, UMP clauses), Composing
Contracts and CSL, defeasible deontic logic, units, Fowler's temporal patterns, XBRL OIM, ISO 20022 camt.053, Ellerman's network
view and differential dataflow with month-end snapshots. PROPOSAL §5 cites Catala (K6), the XBRL OIM (K7), FRP (K2), REA (K3) and
ACTUS ANN (K5). Each item below says what is **new** against those, and does not re-propose them.

**Three things the user's MCP server and GUI need**, and the items that serve each:

| need | what the program must do | items |
|---|---|---|
| **read live** | recompute only what a keystroke changed | T4 DBSP/Salsa, T9 days as sets |
| **write back** | turn a GUI edit of a derived number into a text patch that keeps the comments | T11 lenses |
| **converge** | merge a book edited on a phone and a laptop | T12 CRDTs/local-first |

## Verdicts at a glance

| # | item | verdict | the change in one line |
|---|---|---|---|
| T1 | UFO relators and roles | **adopt** | typed slots, relators, role kinds, phases as `when`: ASSOCIATIONS.md |
| T2 | linear / affine types | **adapt** | conservation is structural in `Leg { from, to }`; check split sums and template ends statically; `Parcel` is move-only |
| T3 | session types, choreographies | **adapt** (small) | `Term::Choose` and `project(term, agent)`; no session-type checker |
| T4 | DBSP, differential dataflow, Salsa | **adapt** | K7 as Z-sets (linear views are incremental for free); month checkpoints; no runtime |
| T5 | Datalog with lattices | **adapt** (semantics only) | K6 is stratified Datalog with a time column and four built-in lattices; no engine |
| T6 | Pacioli group | **adopt** | set-off is the group's normal form; `ledger-semantics` as a test oracle |
| T7 | Catala, L4, LegalRuleML, Blawx | **adapt** (lightly) | v4 §8 already has `unless` and equal-rank conflicts; add labelled, cited exceptions and "why not" |
| T8 | ACTUS, FpML, Marlowe, Findel | **adapt** | `Choose` in K5; `kind option` with ACTUS term names; reject FpML and Findel |
| T9 | SQL:2011 bitemporal, Allen | **adapt** | `DaySet` in K2; git is the system-time axis; no bitemporal store |
| T10 | XBRL GL / OIM, ISO 20022 | **adapt** | `export gl` as a pivot; roles of ISO 20022 map onto slots; no `pain.001` |
| T11 | lenses, Boomerang | **adapt** | lossless syntax tree, edits as span patches, round-trip laws as property tests |
| T12 | CRDTs, local-first | **adapt** | stable flow ids and a three-way merge driver; the fold is already convergent given the set |
| T13 | newer work 2023-2026 | table | VF 1.0, gUFO 2026, timed MPST, Choral, ledger-semantics, Eg-walker, LoRe, XTDB v2, Numscript, TigerBeetle |

---

## T1. UFO relators and roles (Guizzardi)

**What it is.** A relation with its own attributes, span and consequences is a *relator* that mediates two or more participants;
"employer" and "tenant" are roles a thing plays for a time, a phase is a partition of a kind by a condition. gUFO is the OWL form,
with `gufo:Relator` carrying `minQualifiedCardinality 2` on `gufo:mediates`.

**Source.** Guizzardi, *Ontological Foundations for Structural Conceptual Models*, Twente 2005 [M]; Guizzardi, Wagner, Almeida,
Guizzardi, *Applied Ontology* 10(3-4), 2015, DOI 10.3233/AO-150157 [S]; `nemo-ufes/gufo`, `gufo.ttl:1222` [V]; Almeida, Guizzardi,
Prince Sales, Fonseca, "gUFO: A Gentle Foundational Ontology for Semantic Web Knowledge Graphs", arXiv 2603.20948, March 2026 [S].

**Change.** See `ASSOCIATIONS.md`: slots with range and multiplicity (`has beneficiary person`), nesting and path filling by forced
placement, kinds as relators (`employment`, `lease`, `management`, membership), role kinds extending `agent`, phases as `when`.
K1 `Kind { slots, parts, verbs }`, K2 `Timeline<Fill>` per (thing, slot), K3 `Position.slots`.

**Removes.** `account … at`, `owner`/`employer`/`member` lines, names that encode relations, `children` as a number, the per-contract
plan legs, the hand-opened escrow, two weight syntaxes. Estimate in ASSOCIATIONS §7.1: 25 written relations become 3 on `05-family`.

**Costs.** Institutions need kinds (`bank`, `broker`); a bare address is stable only until a sibling opens (day-aware resolution
contains it); about 540 lines in K1/K2/K3/K6 (estimate).

**Verdict. Adopt**, the relator, role and phase parts. **Reject** the rest of UFO (sortals, modes, qualities, the OntoUML stereotype set).

---

## T2. Linear and affine types for resources

**What it is.** In linear logic (Girard) a hypothesis is used exactly once; affine allows dropping it. Move's resource types, Nomos
and Linear Haskell bring this to programs that hold money, so that value cannot be copied or lost by the type system.

**Sources.** Girard, "Linear logic", *Theoretical Computer Science* 50(1), 1987 [M]. Blackshear, Dill, Qadeer, Barrett, Mitchell,
Padon, Zohar, "Resources: A Safe Language Abstraction for Money", arXiv 2004.05106, 2020: "resource safety, a conservation property"
for linear resource types [S]. Das, Balzer, Hoffmann, Pfenning, Santurkar, "Resource-Aware Session Types for Digital Contracts"
(Nomos), *IEEE CSF* 2021, pp. 111-126 (linear types "prevent the duplication or deletion of assets") [S]. Bernardy, Boespflug, Newton,
Peyton Jones, Spiwack, "Linear Haskell", *POPL* 2018, DOI 10.1145/3158093 [S].

**Can conservation be checked statically on legs and templates?** Mostly it does not need to be, and the part that does is linear
arithmetic.

1. **A leg is conserving by its shape.** PROPOSAL K3/K4 give `Leg { from: Id<Position>, to: Id<Position>, qty }`: value leaves one
   position and arrives in another. In the Pacioli group (T6) a leg is `[q // 0]` at `to` and `[0 // q]` at `from`, which sums to zero.
   Conservation is a *typing consequence* of `Leg` having two required ends. A leg with no party written (PROPOSAL 6.4,
   `06 card -> 94.21 USD #dining`) still has two ends in the store: the other is the book's implicit outside position (v4's
   `Role::Outside`), so the optional counterparty is optional in the *syntax* and not in the type.
2. **A split is where it can fail**, and that is a static check. Every leg quantity of a template is an affine form in the header
   amount `x`: `a·x + b`, with `a` a share and `b` a constant (`Rest` is the remainder). A split conserves iff, per commodity,
   `Σ a = 1` and `Σ b = 0` over the split's legs, or one leg is `Rest`. For `min(x, cap)`-style legs the check splits into the
   piecewise cases and a `Rest` leg closes each. This is the same linear algebra as `solve` in K4 (it is checking instead of
   solving), so it is about 60 lines.
3. **Parcels are used once, dynamically.** A parcel is split, merged and relieved; relief must consume it. Rust's affine types give
   "cannot be duplicated" at compile time; "cannot be dropped silently" needs a guard.

**Change.**
```rust
// K4: static, at elaboration. `Affine` is the leg's quantity as a·x + b over the header amount.
pub fn check_split(legs: &[LegIr]) -> Result<(), Unbalanced>;     // per commodity: Σa == 1 and Σb == 0, or a `Rest` leg
pub struct LegTemplate { pub from: End, pub to: End, pub qty: Affine }   // `End` has no `None`: a template cannot name one side

// K3: parcels are move-only. No Clone, no Copy; every operation takes `self`.
#[must_use = "a parcel that is dropped is value that vanished"]
pub struct Parcel { qty: Qty, basis: Basis, acquired: Day, … }
impl Parcel { pub fn split(self, q: Qty) -> (Parcel, Parcel); pub fn merge(self, o: Parcel) -> Parcel; pub fn relieve(self, …) -> Relieved; }
impl Drop for Parcel { fn drop(&mut self) { debug_assert!(self.consumed, "parcel dropped: {} {}", …) } }
```
Add `#![warn(clippy::wildcard_enum_match_arm)]` over `Fact`: PROPOSAL F1 lists `Fact::ClaimChange(_) => {}` (`ledger.rs:1767`), a
discarded value of exactly the kind linearity forbids. An exhaustive `match` with no wildcard arm is the cheap form of that rule
[the lint's name is from memory, M].

**Removes.** A class of silent bugs (the `{}` arms), the runtime "unbalanced split" diagnostic path in the three split resolvers K4
absorbs (replaced by one static check). **Costs.** The `Drop` guard is debug-only; a move-only `Parcel` makes `lots.rs` take
ownership where it borrows today (a rewrite of its API surface, not of its algorithm). **Verdict. Adapt:** the static split check,
move-only parcels, the lint. **Reject** a linear or quantitative type system in the *user* language: no user writes a program that
could duplicate money, because there is no program, only statements.

**Not claimed.** Conservation across time (a `Every` schedule over many days) needs an invariant, not a template check; the fold
tests it, and `ledger-semantics` (T6) is the oracle.

---

## T3. Multiparty session types and choreographies, with blame

**What it is.** A multiparty session type is a *global* protocol among several roles that is *projected* to a local type per role;
a choreography language writes the joint behaviour once and compiles a program per participant. Monitors built from the local types
assign blame to the role whose message is not permitted.

**Sources.** Honda, Yoshida, Carbone, "Multiparty asynchronous session types", *POPL* 2008, JACM 2016 [M]. Jia, Gommerstadt,
Pfenning, "Monitors and blame assignment for higher-order session types", *POPL* 2016, DOI 10.1145/2837614.2837662 [S]. Giallorenzo,
Montesi, Peressotti, "Choral: Object-oriented choreographic programming", *ACM TOPLAS* 46(1), 2024, DOI 10.1145/3632398 [S]. Shen,
Kashiwa, Kuper, "HasChor: Functional choreographic programming for all", *ICFP* 2023 (PACMPL), DOI 10.1145/3607849 [S]. Hou,
Lagaillardie, Yoshida, "Fearless asynchronous communications with timed multiparty session protocols", *ECOOP* 2024, with the Rust
toolchain MultiCrusty^T, deadlines and affine handling of timeouts [S]. Montesi, *Introduction to Choreographies*, Cambridge UP, 2023 [M].

**What is already there.** K5 `Term::Due { grace, blame, body, otherwise }` is the CSL obligation with a responsible party and a
deadline: a global protocol with blame, for two parties. v4 has no way to say a third party's move.

**Change.** Two small things and a vocabulary.
```rust
// K5: the missing constructor. A choice made by one party, open until a day, with what happens if they do not choose.
Choose { by: Id<Entity>, until: Day, options: Range<(Sym, TermId)>, default: TermId },      // Marlowe `When [Case (Choice …) c] t c0`; CfC `or`

// K5/K7: the part of a joint term one party is bound by, and the part it may expect: for `claims` and for blame.
pub fn project(term: &Term, who: Id<Entity>) -> Local;       // obligations (blame == who) and expectations
```
`project` is what `ledgers/shared.ax` needs when one household term (the flat's bills) is read from each roommate's side, and what
a payment-processor protocol (charge, refund, dispute with a deadline for evidence, chargeback) needs when a book wants the merchant's
obligations only. "Blame" is already `Due.blame`; the new fact is that a *third* party can be the one who owes the next move.

**Removes.** Nothing today. It is the *vocabulary* that K5's `residual` and `claims` views already have, named. **Costs.** `Choose`
is non-deterministic, so the forecast needs a rule: it takes `default` until an event (an exercise, an assignment, a notice) says
otherwise. **Verdict. Adapt**, small: `Choose` (see also T8) and `project`. **Reject** session-type checking, deadlock freedom
and typed channels: Axiom's agents do not run code; the book *observes* statements, so the problems session types solve (races,
protocol conformance of running programs) do not occur. The timed MPST paper's deadlines are what `Due.grace` is.

---

## T4. DBSP, differential dataflow and Salsa: an incremental fold

**What it is.** DBSP represents data as **Z-sets** (multisets with integer weights, an abelian group) and a stream as a sequence of
changes; the **integral** `I` turns changes into state and the **derivative** `D` turns state into changes. For a **linear**
operator `Q`, the incremental version is `Q` itself (`D∘Q∘I = Q`); for the others there is a general rewrite. Differential dataflow
is the partially ordered version; Salsa and Adapton are demand-driven memoization over a graph of queries.

**Sources.** Budiu, Chajed, McSherry, Ryzhyk, Tannen, "DBSP: Automatic incremental view maintenance for rich query languages", *PVLDB*
16(7), 2023 (VLDB best paper), extended in *The VLDB Journal* 34(4), 2025 [S]. McSherry, Murray, Isaacs, Isard, "Differential
dataflow", *CIDR* 2013 [S]. Hammer, Phang, Hicks, Foster, "Adapton: composable, demand-driven incremental computation", *PLDI* 2014
[S]. `salsa-rs/salsa` ("on-demand, incrementalized computation, inspired by adapton, glimmer and rustc's query system") [S]. Mokhov,
Mitchell, Peyton Jones, "Build systems à la carte", *ICFP* 2018 [M, cited in `briefs/theory.md`].

**What is already there.** `briefs/theory.md` §7 item 4 chose *month-end snapshots with early cutoff*, no Datalog, the fold stays
sequential. That is right and I keep it. What DBSP adds is the **reason it works and where it stops**.

**Which Axiom views are linear.**

| view | operator | linear in the stream of flows? |
|---|---|---|
| balance by position and commodity | sum | yes |
| tally, flow by purpose, pivot (rows by a dimension, columns by period) | group-by-sum | yes |
| `claims` open amount per party | sum | yes |
| a view with a filter or a join on slot values (`where beneficiary is riley`) | select, join | yes (joins are bilinear) |
| relief of a lot (FIFO, HIFO, prorata) | picks parcels from state | **no**: depends on the state |
| `peak`, `low`, `days_where` | running max/min, count | **no** (max), yes (count) |
| `require tally <= limit` | comparison | no, but it is a diagnostic over a linear view |
| the forecast | the same fold past `today` | no |

**Change.**
```rust
// K7: facts are Z-sets. A day's flows are a delta; a view over a linear query patches itself with the same query over the delta.
pub struct ZSet<K> { w: Map<K, Decimal> }            // an abelian group: add, neg, zero
pub struct Delta { day: Day, by_position: ZSet<(Id<Position>, Id<Commodity>)>, by_purpose: ZSet<(Id<Purpose>, Id<Position>)>, … }

// K3/K4: a checkpoint is the arena marks plus a persistent copy of the parcel store: cheap, because the arenas are append-only.
pub struct Checkpoint { month: Month, marks: Marks, parcels: PersistentParcels, residuals: Residuals }
// Edit dated d: restart from the last checkpoint before d, re-run, and stop at the first later checkpoint whose state hash is unchanged.
```
K4's `Staged { marks, committed }` already truncates the arenas to `Marks`; a checkpoint is that plus persistent parcel maps (`im`/
`rpds`), so the rollback machinery PROPOSAL already specifies is the checkpoint machinery. For the **MCP server and GUI** an edit
is `apply(file, range, text) -> (Diagnostics, Vec<ViewDelta>)`: parse and resolve only the changed month (a query keyed by month,
Salsa-style), restart the fold at the checkpoint, and send each open view the delta of its linear part and a recomputation of its
non-linear part from the checkpoint.

**Removes.** PROPOSAL F4's "5 views re-run the ledger" (`holdings_at` re-plan, `projection.rs` second replay): they become a
query on a checkpoint. **Costs.** State must be snapshot-able (no hidden `RefCell`: already a PROPOSAL rule); checkpoints cost
memory, about one per month; DBSP's own runtime (Feldera, a Rust library) would be a large dependency to use two of its ideas.
Salsa requires owned `Clone`/`Update` outputs and uses `Arc` internally, which conflicts with PROPOSAL's "no `Arc`" rule in the
Plan; it could live in the language-server layer only, where the rule can be relaxed. **Verdict. Adapt:** the Z-set algebra for K7,
month checkpoints with early cutoff, month-granular memoization hand-rolled (about 150 lines). **Reject** a DBSP or differential
dataflow runtime. **Unsure:** whether early cutoff on a state hash is cheap enough with parcel stores; I did not measure.

---

## T5. Datalog with lattices (Flix, Datafun, Ascent)

**What it is.** Flix extends Datalog with lattices and monotone functions so recursion over a user-defined partial order reaches a
fixed point; Datafun tracks monotonicity in a functional language's types; Ascent embeds Datalog with lattices in Rust through
macros.

**Sources.** Madsen, Yee, Lhoták, "From Datalog to Flix: a declarative language for fixed points on lattices", *PLDI* 2016, DOI
10.1145/2908080.2908096 [S]. Arntzenius, Krishnaswami, "Datafun: a functional Datalog", *ICFP* 2016, DOI 10.1145/2951913.2951948,
and "Seminaïve evaluation for a higher-order functional language", *POPL* 2020 [S]. Sahebolamri, Gilray, Micinski, "Seamless
deductive inference via macros" (Ascent), *CC* 2022, DOI 10.1145/3497776.3517779 [S]. Green, Karvounarakis, Tannen, "Provenance
semirings", *PODS* 2007, DOI 10.1145/1265530.1265535 [S]. Alvaro et al., "Dedalus: Datalog in time and space", 2010 [M].

**Where Axiom's rules sit.** Typed slots (ASSOCIATIONS) are exactly the extensional relations of a Datalog program: `employee(e, p)`,
`employer(e, o)`, `unit(l, u)`, `beneficiary(a, p)`, each with a validity interval. The laws that read them are rules:
`eligible(m, d) :- employment(e), start(e, s), waits(plan, w), d >= s + w`; a vacancy is `vacant(u, d) :- rentable(u), not lease(_, u, d)`
(stratified negation); `dependent(p, d) :- dependents(h, p), age(p, d) < 17`. The rule IR of K6 with `Derive(LegTemplate)` is
already that, written as `on … when … derive …`.

**Aggregates are not lattice joins.** A tally is a sum that takes negative contributions (a refund): an abelian-group aggregate,
which is not monotone and not a lattice. Lattices appear in four places only: `peak` and `low` (max/min), and `any`/`all` (the
bool lattice). The right frame for tallies is the Z-set of T4, and for thresholds `require tally <= limit` it is a stratum
boundary: compute the tally to a fixed point, then check.

**Change.** None to syntax. K6's documentation states the semantics as **stratified Datalog with a time column and group
aggregates, plus four built-in lattices**; that is what makes "a law cannot loop" and "the order of laws does not matter" true
statements. Provenance semirings give `why` its shape: the provenance of a view cell is the polynomial (sum over alternative
derivations, product over joint use) of event ids, and for a linear view it is the Z-set of the events with their weights, which K7's
`Origin` edges already hold.

**Removes.** The nine dispatch tables PROPOSAL K6 absorbs are already `Groups<(Trigger, Key), Rule>`; this adds a termination
argument and no code. **Costs.** None, as documentation. An embedded engine (Ascent) would add a second evaluation path next to the
fold, which is PROPOSAL F2 ("every concept is lowered by its own path") again. **Verdict. Adapt**, as a semantics. **Reject** Flix,
Datafun and Ascent as dependencies.

---

## T6. Ellerman's Pacioli group, and accounting as a group of differences

**What it is.** Double-entry bookkeeping uses the *group of differences* of the non-negative reals: pairs `[d // c]` (T-accounts) with
`[a // b] = [c // d]` iff `a + d = b + c`, so `[x // x]` is zero and `[d // c]` has inverse `[c // d]`. Ellerman calls it the Pacioli
group; Grothendieck's group is its modern generalisation.

**Sources.** Ellerman, "The mathematics of double entry bookkeeping", *Mathematics Magazine* 58(4), 226-233, 1985 [S]. Ellerman, "On
double-entry bookkeeping: the mathematical treatment", *Accounting Education* 23(5), 483-501, 2014, arXiv 1407.1898 [S]. And the
newest formalisation, which I read: `ledger/ledger-semantics` (Lean 4, J. Wiegley, September 2026) [V], whose README states that a
journal is a term of the *free symmetric strict monoidal category over finite multisets of accounts*, that valuation is a functor into
commutative groups, that `netFlow` is an additive homomorphism, that value is conserved "within each commodity or names the one
posting that absorbs the remainder", and the negative result that **no compositional pricing algebra with the expected laws exists**
(`zero_absorption_degenerates`), so "pricing must live outside the compositional core, as a lookup performed at reporting time".

**What is already there.** The network view (`briefs/theory.md` §7 item 3, PROPOSAL K4): unknowns form a forest. That is Ellerman's
second paper. The first, the group, is not used.

**Change.**
1. **Set-off is the group's normal form.** Two claims in opposite directions between the same two parties are one element
   `[x // y]`; its normal form is `[x − y // 0]`. `ledgers/shared.ax`'s `net` is not an operation: it is *displaying a pair's
   claims in normal form* (K7, a view), and settling is *adding the inverse*, a payment. K3 needs no new store: a pair view sums the
   claim parcels both ways. This closes `sh-c01` without a keyword.
2. **Multilateral netting** (`sh-u02`) is the sum homomorphism to per-person net balances. Choosing the fewest payments that zero
   everyone is a separate problem (subset-sum-hard in general, M), but greedy matching gives at most n − 1 payments, which is plenty
   for a household. A view `settle flat` can print it; it must not *post* it, because settling through a third person is a
   novation that both of them must accept.
3. **Pricing stays outside the core.** `value(x, USD at POLICY)` is a view; the `@` price on an exchange *names* the exchange. That
   is already v4 and PROPOSAL's rule, and the Lean negative result is a proof that it must stay.
4. **A test oracle.** `ledger-semantics` ships `lake env lean --run Ledger/Driver.lean FILE.dat`, which prints each nonzero balance,
   and `Ledger/Gen.lean`, a seeded generator of property-test journals "that never authors expected results". An `axiom export
   ledger` for the plain-transaction subset (no promises, no lots) lets CI check that the fold's balances equal the oracle's on
   generated books.

**Removes.** Any thought of a `net`/`set-off` keyword; a hand-checked arithmetic path in the fold's tests. **Costs.** A Lean
toolchain in an optional CI job (the repo's own tests do not need it); the export is lossy (parcels with lot prices, promises);
the oracle checks balances, not tallies or laws. **Verdict. Adopt**, as semantics (1-3) and as an oracle (4).

---

## T7. Catala, L4, LegalRuleML, Blawx: laws as default logic

**What it is.** Catala writes statute as *default rules with exceptions*: "if justification j holds the value is c, unless an
exception applies", compiled to a lambda calculus whose default terms raise a conflict when two exceptions of the same priority apply.
L4 (SMU Centre for Computational Law) adds deontic modals with deadlines; LegalRuleML is the OASIS interchange for defeasible deontic
rules with legal metadata; Blawx is a block-based front end over s(CASP) with justification trees.

**Sources.** Merigoux, Chataing, Protzenko, "Catala: a programming language for the law", *ICFP* 2021 (PACMPL 5), DOI
10.1145/3473582 [S]. OASIS, *LegalRuleML Core Specification 1.0*, OASIS Standard, 30 August 2021 [S]. SMU Centre for Computational
Law, L4, `github.com/smucclaw` [S]. Morris, "Building Blawx", CEUR-WS Vol-3437, 2023 [S].

**What is already there.** More than I first thought. LANGUAGE §8 has Catala's structure: "A law states what normally holds; the book says
when it does not": `unless COND` is "an exception the law itself knows (the statute's own list)"; `!` and `waived` are the exceptions the
book states; the more specific law wins by a fixed order; and "two laws of equal rank that disagree are an error naming both", which is
Catala's conflict. PROPOSAL K6 adds posterior and superior ranks. I do not re-propose any of that.

**Change (two, both small).**
1. **A labelled, cited exception.** `unless` takes a name and a source: `unless separated-from-service-at-55 "§72(t)(2)(A)(v)"`. The
   diagnostic and `why` can then say *which* exception applied, as Catala's named exception scopes do. Today `us/401k.ax` tells the
   user to mark a flow `!` "with the reason" for exceptions that the law's own `unless` list could carry.
2. **"Why not" explanations** (s(CASP) justification trees): when a law's `when` guard is false, `why-not LAW FLOW` prints each conjunct
   with its value. K7's `why` explains what *did* happen; this explains what did not. The guard is a conjunction of comparisons, so it is
   one evaluation, not a proof search.

**Removes.** Per-flow `!` waivers for statutory exceptions. **Costs.** About 80 lines (a label on `unless`, one evaluation mode). **Verdict.
Adapt**, lightly: v4 had already adopted the structure. **Reject** LegalRuleML as a format (XML, verbose); from it take only that a law *cites its source*, which
`us/*.ax` already do in doc comments.

---

## T8. ACTUS, FpML, Marlowe, Findel: promises and options

**What it is.** ACTUS is a dictionary of 31 contract types with a fixed vocabulary of terms and event types; FpML is the OTC
derivatives XML; Marlowe is a Haskell-embedded DSL with `When`, `Choice` and timeouts, executed on Cardano; Findel is a 2017
declarative DSL for derivatives on Ethereum. Composing Contracts is the combinator algebra (`or`, `anytime`, `truncate`) behind them.

**Sources.** `actusfrf/actus-dictionary` v1.4 (2023-12-08) [V]: 31 contract types (`ANN PAM LAM NAM UMP CLM OPTNS SWAPS SWPPV STK COM
CSH FXOUT FUTUR CAPFL …`), terms including `OPTP` (call/put), `OPS1`/`OPS2` (strikes), `OPXT` (exercise type), `OPXED` (last exercise
day), `DS` (cash/physical delivery), `STP` (settlement period), and events `XD` (exercise), `STD` (settlement), `MD`, `TD`. Lamela
Seijas, Thompson, "Marlowe: financial contracts on blockchain", *ISoLA* 2018, LNCS 11247 [S]. Biryukov, Khovratovich, Tikhomirov,
"Findel: secure derivative contracts for Ethereum", *FC 2017 Workshops*, LNCS 10323 [S]. Peyton Jones, Eber, Seward, "Composing
contracts: an adventure in financial engineering", *ICFP* 2000, pp. 280-292 [S]. FpML: ISDA, `fpml.org` [M].

**What is already there.** `briefs/theory.md` §2 proposes ACTUS-named clauses for loans (PAM, ANN, NAM, LAM, UMP) and PROPOSAL K5 has
`Annuity`. **What is missing, and the ledgers need it:** a *conditional* (`Choose`, `iv-c02`), an interest-only note with a bullet
principal (PAM, `sb-c05`), a revolving line (UMP, `sb-c04`), and a claim in kind with consideration up front (`sb-c17`, `iv-c01`).

**Change.**
```text
kind option : contract                      // ACTUS OPTNS, with ACTUS's own term names where a user would write them
  has underlying commodity
  has type       one of call | put                    // OPTP
  has strike     amount                               // OPS1
  has exercise   one of european | american          // OPXT
  has expires    date                                 // OPXED
  has settle     one of cash | physical = physical    // DS
  has multiplier number = 100
```
and in K5, with the table row `writer owes market 1 CONTRACT received 235.00 USD` desugaring to
`All[Pay(received in), Choose { by: holder, until: expires, options: [exercise ↦ Pay(deliver, for strike), …], default: Done }]`.
The three endings of `ledgers/investor.ax` are branches: **expiry** is reaching `until` with `default`; **assignment** is the holder's
`exercise` option; **closing purchase** is `Pay` of the series from the writer. K3: a claim parcel's basis is the consideration
received; its relief realizes it (ASSOCIATIONS-level A8). K5's `At(end, …)` appears in the PROPOSAL's table (the deposit row) and not
in its `Term` enum: **an inconsistency to fix**.

**Removes.** The three hand-typed option endings, the per-lot `covers` bookkeeping by hand, the deferred-revenue special case (a prepaid
subscription is the same claim in kind). **Costs.** `Choose` makes the forecast depend on a policy (T3); the `kind option` terms are
a fixed vocabulary, copied from the dictionary and not invented. **Verdict. Adapt:** ACTUS term names for kinds, `Choose` for K5.
**Reject** FpML (XML, no payment semantics) and Findel (I found no active repository; **unverified**; Marlowe covers the same ground).

---

## T9. Bitemporal SQL:2011 and Allen's interval algebra

**What it is.** SQL:2011 adds **application-time period tables** (`PERIOD FOR`, valid time) and **system-versioned tables** (transaction
time); a bitemporal table has both. Allen's algebra names the 13 relations between two intervals (`before`, `meets`, `overlaps`,
`starts`, `during`, `finishes`, `equals` and the inverses).

**Sources.** Kulkarni, Michels, "Temporal features in SQL:2011", *SIGMOD Record* 41(3), 34-43, 2012 [S]. Snodgrass, *Developing
Time-Oriented Database Applications in SQL*, Morgan Kaufmann, 1999 [S]. Allen, "Maintaining knowledge about temporal intervals",
*CACM* 26(11), 832-843, 1983 [S]. Gadia, "A homogeneous relational model and query languages for temporal databases", *ACM TODS* 13(4),
1988, on *temporal elements* (finite unions of intervals) [M]. XTDB v2 (JUXT) as a live bitemporal SQL database, MPL-2.0 [S].

**What is already there.** `briefs/theory.md` §6 and Proposal 7: valid time is the flow's day, `filed` is the *only* transaction time, no
general bitemporal store. I agree, and add why git makes that sound.

**Change.**
1. **Git is the system-time axis.** Axiom is pure and deterministic, so `git checkout REV && axiom report` *is* the query "as we knew it at
   REV": valid time is the flow day, system time is the commit. No store is needed; what is needed is `axiom diff REV1 REV2 --view tally`
   (the amendment diagnostic of Proposal 7, for any view).
2. **`DaySet` in K2**: a temporal element, a sorted run of disjoint day spans with `∪ ∩ −`, `len()`, and Allen predicates on a `Span`.
```rust
pub struct DaySet(Run<Span>);
impl<T> Timeline<T> { pub fn where_(&self, holds: impl Fn(&T) -> bool) -> DaySet; }        // `days_where` returns only a count today
impl DaySet { pub fn len(&self) -> u32;  pub fn max_in_window(&self, months: u32) -> (Day, u32);  pub fn earliest_reaching(&self, n: u32, within: u32) -> Option<Day>; }
```
`lena.in is foreign` is a `DaySet`; `days(…)` is `len()`; a flow recognized per day over `for 2026-02` intersected with it gives the
income *earned* abroad (`ex-c04`); the physical presence test is `max_in_window(12) >= 330`, and its earliest day is
`earliest_reaching(330, 12)` over a presence forecast (`ex-c01`): a two-pointer pass, O(n).
3. **Allen's relations as the vocabulary of `for` and `covers`**, only four: `during` (a payment inside its recognition period),
   `overlaps` (a prepaid year against a tax year), `meets` (a lease ending the day a lease begins), `before`. Nine of the thirteen have no
   use in a ledger.

**Removes.** The bespoke day-counting in FEIE-style laws; the `days_where` count-only API. **Costs.** About 150 lines in K2; a general
bitemporal store would cost an order of magnitude more and nothing in the seven ledgers needs one. **Verdict. Adapt:** `DaySet`, git as
system time, four Allen predicates. **Reject** SQL:2011 tables and XTDB (a database is the wrong shape for a plain-text book).

---

## T10. XBRL GL and OIM, ISO 20022

**What it is.** The XBRL Open Information Model defines a report as facts, each with core aspects (concept, entity, period, unit,
language) and dimensions, independent of syntax (xBRL-XML, xBRL-JSON, xBRL-CSV). XBRL GL is the taxonomy for a general ledger
(`entryHeader`, `entryDetail`, `account`, `amount`, `debitCreditCode`, `documentNumber`, `identifierReference`). ISO 20022 is the
financial messaging standard whose statements (`camt.053`) and payment instructions (`pain.001`) name parties by role (`Dbtr`,
`Cdtr`, `UltmtCdtr`, `DbtrAgt`).

**Sources.** XBRL International, *Open Information Model 1.0*, Recommendation (candidate 2021-02, recommendation 2021-10) [S]; XBRL GL
Taxonomy Framework [M]. ISO 20022 account statement guides: camt.053 carries `EndToEndId`, `RmtInf/Strd/CdtrRefInf` and
`RfrdDocInf/Nb`; the transfer of `EndToEndId` is guaranteed through SEPA [S, from bank implementation guides].

**What is already there.** The OIM is K7's fact shape; `camt.053` is Proposal 8 (codes, `via`, pending, `BkTxCd`). Both stand.

**Change.**
1. **ISO 20022's role names are the sync vocabulary for slots.** A message's `Dbtr`/`Cdtr`/`UltmtCdtr` fill a kind's `payer`, `payee`
   and `via` slots, and `DbtrAgt`/`CdtrAgt` its custodians. One typed mapping per format replaces regexes on memos
   (`known-as` stays for CSV).
2. **`export gl`**: the journal as debit/credit rows `(date, document, account, amount, side, party)`, an OIM-conforming xBRL-CSV with the GL
   taxonomy, for the accountant. It is a K7 pivot: rows by leg. It answers a different need from `sb-c26` (the forms W-2, 941, 1099,
   K-1, which are tax documents, not a ledger).
3. **No `pain.001`.** Writing payment instructions is outbound: Axiom "never opens a network connection" (LANGUAGE §14), and a
   generated file that moves money is a different risk class.

**Removes.** Memo heuristics where a bank sends structure (already planned). **Costs.** One export view, about 200 lines. **Verdict.
Adapt:** (1) and (2). **Reject** (3).

---

## T11. Lenses and Boomerang: writing back into the text

**What it is.** A lens is a pair `get: S → V`, `put: V × S → S` satisfying round-trip laws (`get(put(v, s)) = v`; `put(get(s), s) = s`);
Boomerang makes them for strings and adds *resourceful* matching, so that `put` aligns the edited view with the original source by
a key and keeps what the view did not show (comments, spacing). Cambria uses edit lenses to evolve schemas of CRDT documents.

**Sources.** Foster, Greenwald, Moore, Pierce, Schmitt, "Combinators for bidirectional tree transformations", *POPL* 2005, DOI 10.1145/1040305.1040325 [S]; journal version, "…: a linguistic
approach to the view-update problem", *TOPLAS* 2007 [S]. Bohannon, Foster, Pierce, Pilkiewicz,
Schmitt, "Boomerang: resourceful lenses for string data", *POPL* 2008 [S]. Litt et al., "Cambria: schema
evolution in distributed systems with edit lenses", *PaPoC* 2021, DOI 10.1145/3447865.3457963 [S].

**What the GUI needs.** The user edits a number in a view (a K-1 row, a balance, a payment's amount); the book is a set of text files
with comments. The edit has to land as a *patch on the line that made the number*, not a rewrite of the file.

**Change.**
- **A lossless syntax tree** (every token keeps its trivia: comments, spacing), the way `rowan` underlies rust-analyzer [M], with every K7
  fact carrying the `Span` of the line that made it (`Origin` already half-exists: PROPOSAL K7).
- **An edit is a span patch** `(file, range, new text)`, produced by a small set of *lens-shaped* operations on the syntax tree:
  `set_amount(leg)`, `set_date(event)`, `add_flow(after: Span, text)`, `delete(span)`, `rename(code)`. Each is a `put` with an
  alignment key: the flow's `^code` if present, else its (day, position) pair, which is Boomerang's "resourceful" matching.
- **Round-trip laws as property tests** over the corpus of `examples/`: `parse(print(parse(s))) == parse(s)`; applying any single
  operation then reading back the changed fact returns the value written; comments are byte-identical outside the patched span.

**Removes.** The risk that "edit in GUI" means "reformat the user's file". **Costs.** A lossless tree is a change to the parser's
output type (trivia on tokens), about the size of K1's builder; the operations list is closed and small. **Verdict. Adapt:** the laws and
the alignment key; **reject** Boomerang as a runtime (the operations are five, not a language).

---

## T12. CRDTs and local-first software: one book, several devices

**What it is.** A CRDT is a replicated structure whose merge is commutative, associative and idempotent, so replicas that have seen the
same updates agree. Local-first software keeps the data on the device and merges when connected. CALM says a program is
coordination-free exactly when it is monotone.

**Sources.** Kleppmann, Wiggins, van Hardenberg, McGranaghan, "Local-first software: you own your data, in spite of the cloud",
*Onward!* 2019 [S]. Shapiro, Preguiça, Baquero, Zawirski, "Conflict-free replicated data types", *SSS* 2011 [M]. Gentle, Kleppmann,
"Collaborative text editing with Eg-walker: better, faster, smaller", *EuroSys* 2025, arXiv 2409.14252 [S]. Hellerstein, Alvaro, "Keeping
CALM: when distributed consistency is easy", *CACM* 63(9), 72-81, 2020 [S]. Haas, Mogk, Yanakieva, Bieniusa, Mezini, "LoRe: a
programming model for verifiably safe local-first software", *ACM TOPLAS* 46(1), 2024 [S].

**The observation.** A book is a *set of dated events*; the fold is a deterministic function of that set sorted by `(day, id)`. A set under
union is a CRDT (an add-wins set), and a function of the merged set is the same on every replica: **the fold is already convergent
given the set.** What CALM adds is where coordination *would* be needed: constraints (`require` laws, a balance floor) are
non-monotone, but in Axiom they are diagnostics over a view, so they never block a merge and cost nothing.

The practical failure is not the semantics but git: two devices append lines in the same place and git reports a text conflict.

**Change.**
- **Stable flow identity.** An event has an id: its `^code` if the author wrote one, else a content hash of (day, legs). `axiom fix` can
  write the id as a trailing `^` token if the user wants identity to survive edits; the default is the hash.
- **A merge driver**, `axiom merge BASE OURS THEIRS`, registered in `.gitattributes` (`*.ax merge=axiom`): the three-way *union by id*,
  re-sorted by day within each month file, with a conflict only when both sides edit the same id differently. It needs no CRDT library
  and no runtime state.
- Eg-walker's lesson is that replaying from the last common version beats carrying per-character state: for a ledger the common version
  is a commit and the replay is the fold, which T4's checkpoints make cheap.

**Removes.** Spurious text conflicts on append-heavy files. **Costs.** A merge driver is about 300 lines and must be installed per clone
(a `git config` step); hashing identity makes a line edit look like a delete and an add unless the author supplied a code. **Verdict.
Adapt:** ids and the merge driver. **Reject** a CRDT library and an operation log: git is the log.

---

## T13. Newer work, 2023-2026, that changes a choice

| date | what | source | what it changes | verdict |
|---|---|---|---|---|
| Dec 2025 | **ValueFlows 0.16 removes `AgentRelationship`**, 1.0.0 follows in Feb 2026; the schema files are titled `-DEPRECATED`; `provider`/`receiver` stay `maxCardinality 1` | `valueflows/valueflows` `CHANGELOG.md`, `all_vf.TTL` [V] | a standard dropped the untyped (subject, label, object) triple: ASSOCIATIONS 2.3 | informs T1 |
| Mar 2026 | **gUFO: A Gentle Foundational Ontology for Semantic Web Knowledge Graphs** (Almeida, Guizzardi, Prince Sales, Fonseca) | arXiv 2603.20948 [S] | the reference for relator and mediation in OWL form | informs T1 |
| 2026 (last commit 09-22) | **`ledger-semantics`**: Lean 4 semantics of double-entry journals, with an executable oracle and property-test generator | `ledger/ledger-semantics` [V] | the Pacioli invariant as a theorem; "pricing outside the core"; a test oracle | **adopt** (T6) |
| Dec 2023 | **ACTUS dictionary 1.4**: 31 contract types, `OPTP`/`OPS1`/`OPXT`/`OPXED`/`DS`, events `XD`/`STD` | `actusfrf/actus-dictionary` [V] | the option terms and event names (T8) | adapt |
| 2024 | **Timed multiparty session types** (ECOOP 2024; Rust, MultiCrusty^T) with deadlines and affine timeouts | Hou, Lagaillardie, Yoshida [S] | deadlines are `Due.grace`; no change to K5 beyond `Choose` | informs T3 |
| 2023-24 | **HasChor** (ICFP 2023), **Choral** (TOPLAS 2024): choreographic programming in a functional and an OO host | [S] | a household's terms written once and projected per person (T3, `shared.ax`) | adapt (small) |
| 2023, 2025 | **DBSP** (VLDB 2023, VLDB Journal 2025) | [S] | Z-sets and linearity for K7 (T4) | adapt |
| 2025 | **Eg-walker** (EuroSys 2025) | [S] | replay from the common version, not per-element state (T12) | informs T12 |
| 2024 | **LoRe** (TOPLAS 2024): local-first with verified invariants | [S] | invariants that need coordination are the non-monotone ones; Axiom's are diagnostics (T12) | informs T12 |
| current repo | **Numscript** (Formance): a DSL for money movements with `remaining`, `kept`, `max … from`, allotment fractions, zero postings trimmed | `formancehq/numscript` `Numscript.g4`, `differences-with-machine.md` [V] | an independent design reached the same split grammar as K4's `...` and `Rest`, and chose to *trim zero postings* (K4 should too) | validates K4 |
| current docs | **TigerBeetle**: debit/credit transfers with `pending`, `post_pending_transfer`, `void_pending_transfer`, `balancing_debit`, `balancing_credit`, `linked` | `tigerbeetle/docs/reference/transfer.md` [V] | pending flows `(…)` are two-phase transfers; `balancing_*` is `...`; `linked` chains are atomic groups: no change, a cross-check | validates K4/K5 |
| 2023 | **Daml** (Digital Asset): templates with `signatory`, `observer`, `controller` | Digital Asset authors, "Daml: a smart contract language for securely automating real-world multi-party business workflows", arXiv 2303.03749, 2023 [S] | typed parties on a contract template, checked at creation: the same as slots on a relator | informs T1 |
| 2020 | **Cambria**: schema evolution with edit lenses on CRDT documents | Litt et al. [S] | how `kind` changes between versions of a book migrate (T11) | informs T11 |
| 2024-26 | **XTDB v2**: bitemporal SQL (valid time and system time, SQL:2011) | `xtdb/xtdb` [S] | confirms the two axes; a database is the wrong shape (T9) | reject as a dependency |

---

## What I could not verify, and where I am least sure

1. **Most cited papers were not read.** Marks [S] mean a search result gave authors, venue and year. The papers' *content* is as
   I remember it, apart from the six repositories marked [V].
2. **Subset-sum hardness of minimum-payment netting** (T6) is from memory. The greedy bound of n − 1 payments is easy to see.
3. **`clippy::wildcard_enum_match_arm`** (T2): the lint's name is from memory.
4. **The ISO 20022 role names** (`Dbtr`, `Cdtr`, `UltmtCdtr`, `DbtrAgt`, `CdtrAgt`) and the camt.053 remittance fields are from bank
   implementation guides returned by search, and from `briefs/theory.md`; I did not read the standard.
5. **Findel** has no maintained repository that I found: "I found none" is not evidence that none exists.
6. **Line-count estimates** (T1, T2, T4, T9, T10, T11, T12) are estimates by analogy with PROPOSAL's budgets, not measurements.
7. **Early cutoff on a parcel-store hash** (T4) is plausible and unmeasured.
