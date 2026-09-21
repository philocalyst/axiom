Axiom: a master plan for a proof-producing economic constraint system
The decisive direction

Axiom should not be a ledger language with a smarter balancing engine attached. It should be an economic reasoning system whose outputs include ledgers.

Its fundamental pipeline should be:

immutable evidence
        ↓
typed observations
        ↓
candidate economic facts
        ↓
explicit resolution by constraints, policies, and decisions
        ↓
accepted economic world
        ↓
book-specific recognition
        ↓
journals, statements, lots, taxes, budgets, forecasts, and reports

The journal remains important: it is a compact human input format, an inspection format, and an interoperability target. But it is not the ontology. The ontology is a world of entities, positions, rights, obligations, transfers, contracts, evidence, time, and policies.

The implementation should likewise not encode finance as fake Rust traits. Build a finance-native logical IR and solver. Use formality-core as an executable semantic laboratory; selectively borrow canonicalization, tabling, cycle handling, and proof ideas from Rust’s new trait solver; use Salsa for incremental computation; use rollback unification, relational fixed points, exact arithmetic, and specialized optimization engines behind clean theory interfaces.

A compact description would be:

Axiom is a local-first, content-addressed, bitemporal, proof-producing language for economic reality. A book is an interpretation of that reality, not the reality itself.

I. Non-negotiable design laws

These should be written down before implementation and treated almost like a constitution.

Evidence is immutable. Corrections supersede, qualify, or contradict earlier evidence; they never rewrite history.
Nothing semantic is silently invented. An unknown account, lot, date, counterparty, rate, or amount remains a typed hole until uniquely inferred or explicitly decided.
There is no magical balancing account. A balancing hole may be requested explicitly, but the solver must expose what it inferred and why.
Rules derive; they do not mutate. A rule may derive a candidate event, posting, valuation, state, or recognition. It may not secretly append an accepted transaction.
Absence is not falsity. A proposition may be considered absent only inside an explicit completeness claim over a defined source, scope, and interval.
Ambiguity is a useful result. Multiple admissible lots, accounts, prices, classifications, or matches should remain visible rather than being resolved by arbitrary precedence.
Conflict is not database corruption. Conflicting bank records, policies, documents, and observations coexist in the evidence layer. Reports may require a consistent accepted slice.
Identity and equality are separate. Two identical-looking payments may be two occurrences; two differently formatted records may refer to the same occurrence.
Quantity, cost, quote, value, basis, and proceeds are distinct concepts. Do not overload one punctuation mark or field to mean several of them.
Units are mandatory except for polymorphic zero. 0 may inhabit any quantity type; every nonzero number carries an explicit unit.
Time is multidimensional. Occurrence, legal effectiveness, authorization, settlement, observation, recording, and recognition may all be different.
Account categories are book-specific interpretations. “Asset,” “liability,” “income,” “expense,” and “equity” should not be the root ontology.
Actual and hypothetical worlds never mingle implicitly. Budgets, forecasts, proposals, and counterfactuals live in named scenarios.
All derived results are explainable. A user should be able to ask “why?”, “why not?”, “what is missing?”, “what conflicts?”, and “what changes if I choose this?”
The same economic event can feed multiple books. Cash, accrual, tax, regulatory, management, and personal views are pure interpretations over shared facts.
The semantic core stays small. Product-specific accounting behavior belongs in versioned policy packages, not ever-growing special cases in the kernel.
II. How the notebook requirements resolve into one architecture
Notebook concern	Unified design decision
The journal should remain concise	Keep a journal-like surface language, but compile it into richer propositions and events.
Transactions involve debits and credits	Double-entry is a recognition invariant and projection, not the universal primitive.
Accounts describe where money is	Split accounts into real venues/contracts, economic positions, and named views.
Accounts may contain several commodities	Positions are instrument-agnostic; one venue may hold arbitrarily many instruments.
Debts are not money	Model obligations and claims independently from currencies and positions.
Payer and payee matter	Parties and roles are explicit on transfers and obligations.
Bounced checks and pending payments	Settlement instruments have event histories: issued, authorized, presented, settled, returned, disputed, reversed.
Restricted money is not freely spendable	Encumbrances and rights are first-class; “available balance” is derived.
Liquidity is mobility	Liquidity becomes a multiobjective path query over possible transfers and conversions.
Prices, costs, and valuation are confused by existing syntax	Give each concept its own record and syntax. Eliminate overloaded @ semantics.
Lots and “longest held” need identities	Lots are stable objects with acquisition, basis, rights, adjustments, and provenance.
Tags are too weak	Semantic tags become typed, namespaced annotations with placement rules.
Cleared/uncleared is too special	State is a generic relation derived from state-transition events.
Rules matching transactions can become dangerous	Matching rules generate candidates or derived views; they never alter accepted evidence.
Transactions may span dates	A transaction is an event graph with independently timed legs, not a row with one date.
Quotes must be time indexed	Quotes carry effective time, observed time, source, venue, side, confidence, and validity.
Virtual postings are useful	Preserve them as derived projections or scenario facts, visibly distinct from actual events.
Budgeting and recurring activity overlap	Both use scenario event generators plus constraints over intervals.
Forecasting should resemble an effect system	External assumptions and observations are explicit capabilities and inputs; semantic evaluation remains pure.
Only zero should omit a unit	Implement a polymorphic zero literal and require typed units everywhere else.
Unknown information should not “balance itself”	Use _ and ?name as first-class typed existential holes.
Rules can become recursively strange	Permit positive, well-founded recursion; stratify negation and aggregation; reject uncontrolled recursion.
Comments should remain approachable	Keep comments ordinary; typed annotations carry machine semantics separately.

The key reconciliation is that the language may look journal-like without the engine thinking in journals.

III. The semantic universe
1. Objects have three forms of identity

Every important object should carry three identities:

OccurrenceId    “this particular thing happened or was recorded”
ContentHash     “these normalized contents are structurally identical”
ExternalId      “the source system called it this”

This prevents several common failures:

Two $20 purchases on the same day do not collapse merely because their normalized fields match.
A bank CSV row and a receipt can be linked as evidence for the same event.
A corrected statement can preserve its external identifier while superseding earlier contents.
Renaming an account does not change the identity of the underlying institution contract.
Copying a recurring transaction produces a new occurrence but may share a structural template.

Relations such as these must be explicit:

same_as
possibly_same_as
derived_from
corrects
supersedes
splits
merges
settles
satisfies
reverses
reclassifies

Automatic matching may propose any of these, but it may not collapse identities by itself.

2. Do not represent modality with one overloaded enum

The system needs several orthogonal axes.

struct Statement<P> {
    proposition: P,

    // Is the proposition asserted or explicitly denied?
    polarity: Polarity,

    // Which world does it concern?
    world: World,

    // Is this descriptive or normative?
    force: Force,

    // How far has it moved through the evidence pipeline?
    phase: Phase,

    // When does it apply?
    temporal: TemporalScope,

    // Why should anyone believe it?
    provenance: Provenance,

    // Who or what was entitled to assert it?
    authority: Authority,
}

Suggested dimensions:

Polarity
  positive
  negative

World
  actual
  scenario <id>

Force
  descriptive
  required
  permitted
  prohibited
  preferred

Phase
  observed
  resolved
  accepted
  recognized <book>

Authority
  source observation
  user assertion
  policy derivation
  institutional record
  signed decision

This avoids conflating claims such as:

The payment was observed.
The payment actually occurred.
The payment was required.
The payment was permitted.
The payment is recognized in the tax book.
The payment is expected in a forecast.

Those are not variants of the same state.

3. Use phase-indexed representations

Rather than passing one loosely typed Transaction everywhere, use distinct representations:

RawEvidence
    ↓ parse
Observed<T>
    ↓ identify and reconcile
Candidate<T>
    ↓ solve and decide
Accepted<T>
    ↓ apply book policy
Recognized<Book, T>
    ↓ project
ReportValue

A function that requires an accepted event should not accidentally consume an unverified bank-row candidate. In Rust, this can be enforced with wrappers or separate types:

struct Observed<T>(T);
struct Candidate<T>(T);
struct Accepted<T>(T);
struct Recognized<B, T>(T, PhantomData<B>);

The real implementation will probably use IDs and database queries rather than deeply nested values, but the phase distinction should remain statically visible.

4. Time needs named roles

Bitemporality is necessary but insufficient. Financial events routinely carry more than two meaningful times.

Time role	Meaning
occurred	When the economic activity happened
effective	When a contract or right became legally effective
authorized	When a settlement action was authorized
captured	When a card or similar instrument was captured
settled	When custody or a claim actually changed
due	When performance was required
observed	When a source recorded or exposed the information
recorded	When Axiom received it
recognized	Which reporting period receives the consequence
valid	Interval during which a state or rule applies
superseded	When a later assertion displaced it for some purpose

Time values should support:

exact instant
local date with named time zone
date without time
month or accounting period
closed/open interval
uncertain interval
before/after constraint
recurrence
business calendar

A check may therefore have:

issued     2026-09-01
presented  2026-09-04
settled    2026-09-05
returned   2026-09-08
recognized 2026-Q3

There is no reason to force those facts into one transaction date.

5. Open-world reasoning and explicit completeness

The default is:

not proven ≠ false

A negative inference is allowed only when one of these applies:

There is explicit evidence for the negative proposition.
The relevant relation is declared complete over the relevant scope.
A policy defines a closed domain and records that assumption in the proof.

For example:

complete bank_activity(checking)
  during 2026-09
  from statement/september

may justify:

There was no other bank-reported transaction in September.

It does not justify:

No cash transaction occurred anywhere in September.

Completeness claims need their own provenance, scope, validity interval, source set, and revocation history.

6. Contradiction must be survivable

The observed layer should use a paraconsistent interpretation. For one proposition, positive and negative support yield four meaningful states:

Positive proof	Negative proof	Result
No	No	Unknown
Yes	No	Proven
No	Yes	Refuted
Yes	Yes	Conflict

A conflict must not permit arbitrary conclusions. It should produce a focused diagnostic:

conflict: closing balance(checking, 2026-09-30)

  4,812.20 USD
    from bank-statement-v1, page 3

  4,782.20 USD
    from bank-statement-v2, page 3
    supersedes bank-statement-v1 according to source metadata

resolution candidate:
  prefer v2 for recognized statements
  retain both observations

The accepted world can select one interpretation without destroying the rejected evidence.

7. Decisions are first-class data

A decision should be more than an edited field:

struct Decision {
    subject: GoalId,
    selected: AnswerId,
    rejected: Vec<AnswerId>,
    scope: DecisionScope,
    authority: AuthorityId,
    rationale: Option<Text>,
    made_at: Instant,
    effective_during: TimeSet,
    supersedes: Option<DecisionId>,
}

Decision scope matters:

this disposal only
all disposals in this account
all 2026 disposals
this tax book
this jurisdiction
this scenario
until superseded

Named policy selection is just a machine-produced decision with a proof. A default must never be an invisible branch in solver code.

IV. A better economic ontology
1. Entities and roles

An entity may be a person, organization, trust, household, institution, government, fund, estate, or software-controlled actor. Roles are separate, time-scoped relations:

legal_owner
beneficial_owner
custodian
controller
authorized_user
issuer
debtor
creditor
payer
payee
employer
employee
agent
principal
trustee
beneficiary
tax_owner

Avoid one universal owner field. A brokerage may be the custodian, an individual the beneficial owner, a trust the legal owner, and an adviser the authorized controller.

Roles may be:

fractional,
conditional,
shared,
delegated,
effective only for an interval,
disputed,
different across books or jurisdictions.
2. Instruments and commodities

Currency should be one instrument family, not a privileged root object.

Instrument
  Currency
  Equity
  DebtSecurity
  Derivative
  PhysicalGood
  ServiceUnit
  Claim
  LoyaltyPoint
  EnergyUnit
  CarbonCredit
  Basket
  UniqueAsset

An instrument declaration can specify:

unit
issuer
fungibility
divisibility
canonical quantum
transferability
expiration
constituents
settlement mechanism
rights represented

A venue may impose an operational quantum stricter than the instrument:

instrument USD canonical-quantum 0.0001 USD
account checking operational-quantum 0.01 USD

That handles institutions that store or settle an instrument at different precision.

A basket is not a price hack. It is an instrument or bundle with constituents and effective intervals.

3. Positions, rather than balances, are primitive

A position is a relation among:

entity or beneficiary
instrument
quantity
venue or custodian
rights
encumbrances
lot or provenance
valid interval

Conceptually:

struct Position {
    beneficiary: EntityId,
    custodian: Option<EntityId>,
    venue: Option<VenueId>,
    instrument: InstrumentId,
    quantity: ExactQuantity,
    rights: RightsBundle,
    encumbrances: Set<EncumbranceId>,
    lot: Option<LotId>,
    validity: TimeSet,
}

A bank “balance” is then a view over positions. So are:

nominal balance
settled balance
available balance
withdrawable balance
collateral value
tax basis
market value
spendable cash

This cleanly represents money that exists but is:

on hold,
pledged,
held for someone else,
earmarked,
restricted by contract,
subject to a pending transfer,
technically owned but not controlled,
controlled but not beneficially owned.
4. Accounts are named lenses and contract boundaries

The account should no longer be a universal bucket.

Material account

A material account corresponds to a real contractual or institutional boundary:

bank deposit contract
brokerage account
credit card agreement
escrow arrangement
loan agreement
wallet
cash container
inventory location

It identifies a venue, parties, permitted operations, instrument restrictions, and settlement behavior.

Virtual account

A virtual account is a named query or classification over positions and events:

spendable cash
emergency fund
business travel
tax withheld
household groceries
long-term investments
restricted grant funds

A virtual account does not claim that the external institution maintains that partition.

Book account

A book account is a recognition category within a particular accounting interpretation:

Assets:Bank
Liabilities:Card
Income:Wages
Expenses:Food
Equity:Opening

These categories are output types of a recognizer, not primary economic objects.

An ergonomic account declaration may combine all three, but the compiler should desugar it into separate concepts.

account checking : material {
  contract bank/deposit/1234
  legal-holder me
  beneficiary me
  controller me
  accepts USD
}

view rent-reserve =
  positions at checking
  where encumbered-for == lease/october

book-account personal-cash.Assets.Checking =
  recognize positions at checking

This resolves the notebook’s central question: an account can be a real place, a contractual relationship, a filtered view, or a reporting category—but those meanings must not be silently conflated.

5. Events replace monolithic transactions

The fundamental occurrence is an Event. A transaction is a named coherence boundary grouping one or more events.

Core event families should include:

Transfer
Exchange
Issue
Retire
Acquire
Dispose
Accrue
Settle
AttemptSettlement
FailSettlement
Reverse
Refund
Charge
Allocate
Encumber
ReleaseEncumbrance
Reclassify
Adjust
Measure
Observe
CorporateAction

They should remain extensible through typed records rather than an enormous closed enum.

A transfer explicitly names direction:

move 250.00 USD
  from acme/payroll
  to   me@checking

A transfer of an already existing fungible instrument normally conserves quantity. Issuance and retirement are separate event types so that creation and destruction are never hidden behind synthetic accounts.

An exchange couples consideration:

exchange {
  give    100.00 USD from me@checking to broker
  receive   4.25 ABC from broker to me@brokerage
}

A transaction spanning dates becomes an event graph:

order placed
    ↓
authorization
    ↓
capture
    ↓
settlement
    ↓
possible dispute
    ↓
possible reversal

The journal projection may choose to recognize these as one posting group or several, depending on the book.

6. Obligations and contracts are first-class

Debt is a claim or obligation, not a negative quantity of money.

struct Obligation {
    debtor: EntityId,
    creditor: EntityId,
    performance: Performance,
    due: TimeSet,
    conditions: Vec<Condition>,
    priority: Option<Priority>,
    collateral: Vec<PositionId>,
    contract: Option<ContractId>,
    jurisdiction: Option<JurisdictionId>,
}

Performance may be:

transfer a quantity
deliver a unique asset
provide a service
refrain from an action
meet a threshold
perform one of several alternatives

Payment and obligation satisfaction are many-to-many:

one payment may satisfy several invoices;
several payments may satisfy one obligation;
a payment may be partial;
an overpayment may create a reverse obligation;
an obligation may be forgiven, netted, novated, disputed, or discharged nonfinancially.

The solver should answer:

What remains due?
Which event satisfied this amount?
Was payment timely?
Who bears the obligation after assignment?
What collateral secures it?
Which book recognizes the receivable?
7. Settlement instruments are state machines built from events

“Cleared” should not be a special Boolean.

For a check:

issued
delivered
presented
accepted
settled
returned
stopped
expired
reissued

For a card:

authorized
held
partially captured
captured
authorization expired
settled
disputed
charged back
represented
resolved

For ACH or bank transfers:

instructed
submitted
pending
posted
settled
rejected
returned
recalled
reversed

The current state is a derived result of the event history and instrument-specific policy. This handles unusual transitions without adding another set of booleans to the core schema.

A bounced check does not erase the earlier attempted payment. It contributes:

an issued settlement instrument;
a presentation;
a provisional credit if the bank exposed one;
a failed or reversed settlement;
possibly a fee;
a still-outstanding underlying obligation.
8. Separate quote, consideration, cost, basis, value, and proceeds

These should be different types:

Concept	Meaning
Quote	Observed exchange relation between instruments
Consideration	What was given or promised in an exchange
AcquisitionCost	Economic cost attached to acquisition
Basis	Amount recognized under a particular policy
Valuation	Result of applying a valuation policy at a time
Proceeds	Consideration attributed to a disposal
GainLoss	Difference under a named recognition policy

A quote should look conceptually like:

quote eur-usd/2026-09-20T16:00Z {
  base       EUR
  counter    USD
  kind       mid
  ratio      1 EUR = 1.1724 USD
  effective  2026-09-20T16:00Z
  observed   2026-09-20T16:01Z
  venue      market/source-a
  source     feed-row/9812
  validity   30 minutes
}

Bid, ask, close, net-asset value, appraised value, model value, and tax value should not be reduced to one global price relation.

A valuation query is:

value(position, target-unit, time, policy)

The answer may be:

unique
ambiguous between sources
conditional on a route
unavailable
stale
in conflict
outside policy tolerance

Triangulated conversion must preserve its path:

ABC → EUR → USD

so that the resulting proof exposes the quotes, spreads, and times used.

9. Lots are real objects

A lot should carry:

instrument
quantity acquired
remaining quantity
acquisition event
acquisition time
consideration
fees
recognized basis by book
holding-period facts
adjustments
rights or restrictions
provenance

Disposal should refer to a lot, lot set, or a typed hole:

dispose 10 ABC
  from brokerage
  lot ?selected
  proceeds 500.00 USD

The solver may report:

eligible lots:
  lot/buy-1   basis 200.00 USD
  lot/buy-2   basis 300.00 USD

selected lot:
  unresolved

recognized gain:
  blocked by LotSelection

conditional results:
  lot/buy-1 → 300.00 USD gain
  lot/buy-2 → 200.00 USD gain

A named FIFO package can resolve it. A specific-identification decision can override that package within a defined scope. Jurisdiction-specific adjustments such as wash-sale handling belong in versioned policy packages, not in the general lot primitive.

10. Liquidity becomes a graph query

Liquidity should not be a static account property.

Construct a graph whose nodes are positions and instruments, and whose edges are admissible actions:

withdraw
transfer
sell
redeem
borrow
convert
settle
wait for maturity
release encumbrance

Each edge can carry:

time
fee
capacity
permission
market impact
tax consequence
counterparty risk
settlement risk
minimum quantity

A query can then ask:

How much USD can become spendable by tomorrow?
What is the cheapest route?
What is the fastest route?
What route minimizes tax?
What survives a failure of one institution?

The output should usually be a Pareto frontier, not one misleading “liquidity score.”

V. Recognition: one world, many books

A book is a pure interpretation:

recognize(BookPolicy, AcceptedWorld)
  -> RecognizedFacts + ProofGraph

Possible books include:

personal cash
personal accrual
tax
management
GAAP-like reporting
regulatory
household budgeting
estate
project cost
counterparty view

The same economic event may receive different:

dates,
classifications,
bases,
exchange rates,
accrual treatment,
ownership interpretation,
materiality treatment.

That should not require duplicating the source event.

A close should be a signed object containing:

source commit
book policy versions
period
completeness claims
accepted decisions
recognized root hash
exceptions
signatures

Closing does not freeze or mutate history. A reopening or restatement creates a new close that supersedes the previous one.

VI. The constraint language
1. Core logic

The trusted logical fragment should be expressive but deliberately bounded:

typed first-order terms
conjunction
disjunction
existential variables
universally quantified rules
explicit negation
equality and disequality
finite set relations
stratified aggregation
positive recursion
specialized arithmetic and temporal constraints

Do not initially permit:

unrestricted higher-order predicates,
arbitrary recursive user functions,
negation through recursion,
effects inside semantic rules,
unbounded generation of fresh economic objects,
procedural rule ordering.

Closures can exist in the surface language, but they should compile to a controlled query-plan IR rather than execute arbitrary Rust-like code inside the solver.

2. Types

The type system should combine:

Nominal identity

Used where legal or economic identity matters:

EntityId
InstrumentId
ContractId
EventId
LotId
AccountId
Structural row types

Used for extensibility:

{
  debtor: Entity,
  creditor: Entity,
  amount: Quantity<Currency>,
  due: TimeSet,
  ...extension
}
Set-theoretic composition
Currency | Security
MaterialAccount & ControlledBy<me>
Instrument & !Expired
Refinements
Quantity<USD> where value >= 0
Account where accepts(USD)
Quote where age <= 1 day
Lot where remaining >= disposal.quantity
Units of measure
100.00 USD
4.25 ABC
12 hour
3 kWh
Phase types
Observed<Event>
Accepted<Event>
Recognized<TaxBook, Disposal>
Gradual holes
?account : Account & Holds<USD>
?lot     : Lot<ABC>
?rate    : Ratio<EUR, USD>

The solver narrows holes by intersection. If one inhabitant remains, it can return a unique inferred answer. If several remain, the result is ambiguous.

3. Typed annotations replace semantic string tags

Comments remain free text:

// Bought during conference trip.

Semantic annotations are declared:

annotation project : Event -> ProjectId
annotation deductible : Recognized<TaxBook, Expense> -> Bool
annotation receipt : Event -> EvidenceId*

Then:

event hotel {
  project conference/2026
  receipt scan/hotel-41
}

Placement is type checked. A deductible annotation cannot be attached to raw evidence if it is defined only for recognized tax expenses.

Namespacing prevents accidental collisions:

personal/project
tax-us/deductibility
employer/cost-center
4. Rule classes must remain visibly distinct

Axiom should not have one generic “rule” that does everything.

Rule class	Purpose	May create accepted facts?
derive	Infer propositions from propositions	No
check	Validate an invariant or expectation	No
select	Choose among a candidate set under a named policy	Produces a decision proof
recognize	Interpret accepted facts in a book	Produces recognized facts
reconcile	Propose identity or satisfaction links	No
optimize	Generate plans satisfying constraints	Scenario only
import	Convert external bytes to observations	Observations only

This prevents a matching rule from becoming an invisible transaction generator.

5. Checks and severity

Truth and severity are different dimensions.

check invariant
check acceptance
check error
check warning
check advice

An invariant might make a candidate unacceptable. A warning may still be true but not blocking. A policy can configure which severities block a close.

Examples:

check invariant transfer_conserves_quantity
check acceptance recognized_journal_balances
check warning quote_age <= 2 days
check advice emergency_liquidity >= 3 months

The proof should say both whether the condition holds and what consequence the configured severity carries.

6. Recursion and aggregates

The safe rules are:

Positive recursion computes a least fixed point.
Negation is permitted only against a lower stratum.
Aggregation is permitted only over a finite, complete relation.
Temporal recursion needs a well-founded measure or explicit horizon.
Coinduction is off by default and must be explicitly justified.
Recurring plans are event generators, not recursive logical clauses.
Resource exhaustion returns an incomplete result, never false.

A recursive liquidity or dependency query may legitimately use a fixed point. A rule equivalent to “this obligation is valid because it depends on itself being valid” must not become a proof.

VII. Solver result semantics

Avoid one overloaded Result<T, Error>. A principled result has three independent axes:

enum Truth {
    Neither,    // no proof or refutation
    TrueOnly,
    FalseOnly,
    Both,       // conflict
}

enum Multiplicity<T> {
    None,
    Unique(T),
    Multiple(NonEmpty<T>),
}

enum Completion {
    Complete,
    OpenWorld,
    ResourceLimited,
}

A query result can then be:

struct Resolution<T> {
    truth: Truth,
    answers: Multiplicity<Conditional<T>>,
    completion: Completion,

    positive_proofs: Vec<ProofId>,
    negative_proofs: Vec<ProofId>,
    blockers: Vec<Requirement>,
    conflicts: Vec<Conflict>,
    repairs: Vec<Repair>,
}

This gives precise user-facing statuses:

Status	Meaning
Proven	Positive proof exists
Refuted	Explicit negative proof exists
Unknown	Neither proof exists under an open world
Conflict	Positive and negative proofs both exist
Ambiguous	Several compatible output substitutions exist
Conditional	Answer exists if residual obligations are met
Blocked	A required decision or input is missing
Incomplete	Search hit a declared resource boundary
Unsupported	The required theory or operation is unavailable

Incomplete must never be cached as a semantic refutation.

VIII. The hybrid solver architecture
1. One coordinator, several theories

No single algorithm should solve every kind of financial constraint.

                     typed canonical goal
                             │
                  logical search coordinator
                             │
         ┌───────────────────┼───────────────────┐
         │                   │                   │
   unification         relational closure    exact arithmetic
         │                   │                   │
    temporal             units/types          valuation
         │                   │                   │
   graph search          contract state      optimization
         └───────────────────┼───────────────────┘
                             │
              answers + proofs + blockers + repairs

The logical coordinator should:

enumerate candidates;
introduce existential variables;
unify terms;
table recursive goals;
delegate theory atoms;
combine proofs;
learn reusable conflicts or “nogoods” later if needed.

Theories should not mutate global semantic state.

2. A theory interface

A production interface could resemble:

pub trait Theory {
    type Atom;
    type State;
    type Propagation;
    type Conflict;
    type Model;
    type Certificate;

    fn normalize(&self, atom: &Self::Atom) -> CanonicalAtom;

    fn assume(
        &self,
        state: &mut Self::State,
        atom: CanonicalAtom,
        reason: ProofId,
    ) -> Result<Vec<Self::Propagation>, Self::Conflict>;

    fn check(
        &self,
        state: &Self::State,
    ) -> TheoryStatus<Self::Model, Self::Conflict>;

    fn explain_propagation(
        &self,
        propagation: &Self::Propagation,
    ) -> Self::Certificate;

    fn explain_conflict(
        &self,
        conflict: &Self::Conflict,
    ) -> Self::Certificate;

    fn verify(
        &self,
        certificate: &Self::Certificate,
    ) -> bool;
}

The initial implementation does not need a full CDCL(T) solver. Start with cooperative propagation and tabled search. Add learned conflict clauses only after benchmarks show combinatorial search is a real bottleneck.

3. Canonicalization

A query such as:

eligible_lot(sale/7, ?x)

must have the same cache identity as:

eligible_lot(sale/7, ?candidate)

Canonicalize free inference variables:

eligible_lot(sale/7, ?0)

A cache key should include:

canonical goal
accepted-world commit
rule-package set
book or scenario
completeness context
solver semantics version
resource profile, when observable

That last item matters because a recursion limit or timeout must not silently change a supposedly reusable answer.

4. Tabled search and cycles

The goal engine needs:

global result cache
active search stack
provisional cycle table
strongly connected component tracking
fixed-point reevaluation
answer subsumption
dependency recording

Cycle classes should include:

inductive
explicitly coinductive
arithmetic simultaneous system
temporal recurrence
illegal negative cycle
resource-limited

For example:

A depends on B
B depends on A

does not prove either proposition without a base case under ordinary inductive semantics.

A recursive aggregate such as organizational ownership may converge to a fixed point. A recurrence generating future rent payments should instead compile to a bounded generator with a requested horizon.

5. Proofs and certificates

Every derived result should form a proof DAG containing nodes such as:

evidence leaf
manual assertion
rule application
unification
relation lookup
fixed-point result
arithmetic certificate
temporal certificate
completeness assumption
policy selection
human decision
book recognition
external-solver certificate

Proofs should be:

hash-consed;
serializable;
independently checkable;
lazily materialized;
capable of sharing subproofs;
redactable under information-flow rules;
traversable both forward and backward.

Queries should include:

prove P
explain P
why-not P
alternatives P
minimal-evidence P
impact-of change
repair P

repair is bounded abductive search. It may suggest:

supply a missing quote
select one lot
declare the period complete
link two records
relax a scenario constraint

It must not silently perform those operations.

IX. How to use the current Rust ecosystem
1. formality-core: executable reference semantics

a-mir-formality is explicitly an early-stage experimental Rust Types Team project. Its formality-core layer is language-independent and provides variable binding, judgments, proof machinery, fixed-point computation, and a way to declare another language; judgment evaluation can retain proof and structured failure information. That makes it unusually well suited to the reference model, examples, and differential tests, but not yet something to assume will be the permanent high-volume runtime.

Use it to specify:

candidate_lot
selected_lot
valuation
basis
gain
balances
satisfies
recognized
available

Keep the reference implementation even after a faster engine exists. It becomes the semantic oracle against which production optimizations are checked.

2. Rust’s new trait solver: borrow the architecture, not the vocabulary

As of August 2026, Rust’s next-generation trait solver is enabled by default on nightly as it moves toward stabilization. It evaluates recursive goals through candidates and returns success, ambiguity, or error with associated constraints. Rust-analyzer now reuses rustc_next_trait_solver and rustc_type_ir through shared abstractions, proving that the implementation can support more than one compiler frontend. But those abstractions still represent Rust types, predicates, inference variables, associated items, regions, lang items, and impl lookup—not arbitrary financial predicates.

Therefore:

Do not implement a fake Rust Interner for finance.
Do not encode Account or Obligation as TraitRef.
Do not depend on rustc’s internal predicate vocabulary.

Copy the conceptual architecture:

canonical goals
candidate assembly
nested obligations
tabled search
ambiguity preservation
proof trees
cycle classification
cache soundness
3. rustc_type_ir::search_graph: algorithmic source material

The new solver’s SearchGraph handles caching, recursion depth, inductive and coinductive cycles, provisional answers, and fixed-point reevaluation. Rust’s own documentation calls out how subtle cache soundness becomes when cycle roots, participants, and available recursion depth interact. Its abstraction traits currently exist mainly so the component can be fuzzed separately, rather than as a polished domain-neutral solver API.

The correct plan is:

Study it closely.
Build a small finance-native search graph behind Axiom’s own stable API.
Differentially fuzz the two state-machine designs where possible.
Vendor or adapt implementation fragments only if that remains cleaner than a reimplementation.
Never expose rustc types through Axiom’s public API.
4. Chalk: specification literature, not a dependency

Chalk’s Prolog-like formulation, quantification model, answer enumeration, and ambiguity handling remain highly relevant. But Chalk now describes itself as being sunset in favor of rustc’s newer solver. Treat its book and code as a source of algorithms and terminology, not as the foundation of a new long-lived system.

5. Salsa: the incremental semantic database

Salsa is currently designed for incremental, on-demand computation. Its inputs, tracked entities, interned values, tracked functions, backdating, diagnostics accumulators, cycle support, and fine-grained dependency recording map well to parsing, name resolution, type checking, policy compilation, report compilation, and goal invalidation.

Use Salsa for:

source file → parsed module
module → typed HIR
policy set → compiled rules
evidence chunk → normalized observations
goal → candidate clauses
accepted commit → recognized book
report query → result

Do not automatically create one Salsa input per bank row at billion-row scale. Use chunked immutable relation inputs and a purpose-built delta engine underneath the logical relations. Salsa should coordinate semantic dependency boundaries, not replace the entire storage and analytics layer.

6. Rollback unification

Use ena or a similarly small internal implementation for:

union-find equivalence classes
inference variables
snapshots
speculative candidate exploration
rollback

The solver needs to try a candidate, accumulate substitutions and constraints, then restore the previous state cheaply when the candidate fails. Keep Axiom’s term and variable interfaces independent so this component can eventually be replaced.

7. Relational fixed points

Datafrog is a small embeddable Datalog-style engine with explicit repeated update rules; Ascent embeds Datalog-like inference in Rust and supports fixed-point relations, lattices, aggregation, and parallel variants. They are strong candidates for prototypes and benchmarks around reachability, ownership closure, matching, dependency analysis, and policy relations.

Likely progression:

prototype: Ascent for concise experiments
small embedded relations: Datafrog or custom
production at high scale: specialized indexed delta relations

Do not force every query into Datalog. Existential inference, alternatives, proof-directed top-down search, arithmetic, and open-world diagnostics need the wider solver.

8. Exact arithmetic

Axiom’s authoritative numeric representation should support:

arbitrary-size integers
exact finite decimals
arbitrary rational ratios
explicit intervals
declared rounding operations

rust_decimal is useful for many ordinary financial boundaries, but it has a fixed 96-bit mantissa and roughly 28 significant decimal digits; that is not a universal semantic representation for securities, crypto-like instruments, long chains of ratios, or exact symbolic calculations. num-rational exposes arbitrary-precision rationals.

A strong core representation is:

enum ExactNumber {
    Integer(BigInt),
    Decimal { coefficient: BigInt, scale: u32 },
    Rational { numerator: BigInt, denominator: BigInt },
}

Preserve a decimal’s declared scale when it is evidentially meaningful. Normalize rationals for equality, but preserve the source literal separately.

Never use f64 for accepted balances, basis, or proof checking.

9. SMT and optimization backends

Z3 and cvc5 Rust bindings can represent richer arithmetic and logical constraint problems, making them useful for prototyping, difficult reconciliation problems, and bounded verification. The core should nevertheless verify any concrete model or certificate it accepts rather than trusting an opaque solver invocation.

good_lp is a convenient linear-programming modeler with multiple solver backends, but it is specifically a linear optimization layer and its ordinary interface operates with floating-point coefficients and tolerances. It belongs in scenario planning and optimization, not authoritative bookkeeping arithmetic.

Use optimization for:

cash scheduling
debt repayment scenarios
tax-aware candidate planning
inventory allocation
liquidity routing
budget feasibility

Then verify the proposed plan against exact constraints.

10. Sandboxed adapters

Importers, data-source connectors, and optional policy extensions should eventually use a capability-limited component interface. Wasmtime’s Component Model support uses WIT-defined worlds to generate typed host and guest interfaces, which is a strong model for versioned, language-neutral adapters. Pin a stable release and expose narrow capabilities rather than handing plugins arbitrary filesystem and network access.

An importer should be able to:

read supplied bytes
request explicitly granted metadata
emit observations and diagnostics
attach provenance

It should not be able to:

modify accepted facts
read unrelated accounts
make undeclared network requests
choose tax policy
write arbitrary files
X. Concrete Rust workspace
axiom/
├── axiom-syntax          CST, parser, formatting, source spans
├── axiom-hir             names, modules, types, desugaring
├── axiom-ir              canonical terms, goals, clauses, statements
├── axiom-types           rows, refinements, units, phase typing
├── axiom-unify           inference variables, snapshots, rollback
├── axiom-logic           tabled search and candidate assembly
├── axiom-search-graph    caches, SCCs, cycles, fixed points
├── axiom-rel             indexed relational and delta evaluation
├── axiom-arith           exact numbers, equations, inequalities
├── axiom-time            intervals, recurrence, calendars
├── axiom-units           quantity and ratio theory
├── axiom-contracts       obligations and state-machine primitives
├── axiom-opt             scenario and optimization interface
├── axiom-proof           proof DAG, certificates, checker
├── axiom-db              Salsa queries and semantic dependency graph
├── axiom-store           content-addressed persistent storage
├── axiom-policy          packages, versions, coherence
├── axiom-recognize       book interpretations
├── axiom-query           query planner and analytics
├── axiom-adapter-sdk     importer/component interfaces
├── axiom-stdlib          core economic vocabulary
├── axiom-cli             command-line application
├── axiom-lsp             diagnostics, completion, proof navigation
├── axiom-format          canonical formatter
├── axiom-test-oracle     formality-core reference model
└── axiom-fixtures        edge-case and interoperability corpus

Keep dependencies one-directional. In particular:

axiom-ir must not depend on accounting policy
axiom-proof must not depend on UI
axiom-store must not decide semantics
axiom-adapter-sdk must not depend on accepted-world internals

The trusted computing base should eventually be limited to:

canonical decoder
type checker
proof checker
exact arithmetic verifier
policy/package identity verifier
signature verifier

The main solver can be aggressively optimized so long as its accepted conclusions remain checkable.

XI. A possible surface language

The syntax should be familiar enough for journal users but should name economic concepts directly.

Declarations
entity me   : Person
entity acme : Organization
entity bank : Institution

instrument USD : Currency {
  quantum 0.01 USD
}

instrument ABC : Equity {
  issuer acme
  quantum 0.000001 ABC
}
Material account and view
account checking : material {
  contract bank.deposit("ending-1234")
  legal-holder me
  beneficiary me
  controller me
  accepts USD
  operational-quantum 0.01 USD
}

view spendable-cash =
  positions
  where beneficiary == me
    and instrument <: Currency
    and not encumbered
Evidence and event
observe row bank-sept/184 {
  posted 2026-09-18
  description "ACME PAYROLL"
  amount 2_000.00 USD
  source file("checking-september.csv") row 184
}

event paycheck/september {
  occurred 2026-09-18
  settled  2026-09-18

  move 2_000.00 USD
    from acme
    to   me@checking

  evidence bank-sept/184
}
Obligation
obligation rent/october {
  debtor me
  creditor landlord
  performance transfer 1_200.00 USD
  due during 2026-10-01
  under lease/home
}
Settlement attempt
event rent-check/42 {
  issue check/42
    by me
    to landlord
    amount 1_200.00 USD
    drawn-on checking

  issued 2026-10-01
  satisfies rent/october
}

Later evidence:

event rent-check/42-return {
  returned check/42
  at 2026-10-06
  reason insufficient-funds
  evidence bank-notice/882
}

The obligation becomes outstanding again under the check policy; no earlier event is erased.

Quote
quote abc-usd/close-2026-09-20 {
  1 ABC = 52.14 USD
  kind close
  effective 2026-09-20T16:00-04:00
  observed  2026-09-20T16:04-04:00
  venue exchange/x
  source feed/391
}
Lots and holes
event buy/one {
  acquire 10 ABC into me@brokerage
  consideration 200.00 USD
  fees 1.00 USD
}

event buy/two {
  acquire 10 ABC into me@brokerage
  consideration 300.00 USD
  fees 1.00 USD
}

event sell {
  dispose 10 ABC
    from me@brokerage
    lot ?lot

  proceeds 500.00 USD
}

The named ?lot remains visible in diagnostics. _ can represent an anonymous hole when the user does not need to reference it later.

Decision
decide sell.lot = buy/one.lot {
  scope event sell
  reason "specific identification"
}
Named policy
use policy lots/fifo
  for book tax-us
  during 2026
Completeness
complete activity(checking)
  during 2026-09
  from statement/checking-september
Checks
check invariant transfer-conservation

check acceptance journal-balances
  for book personal-cash

check warning quote-age <= 1 day
  for report portfolio
Scenario, budget, and recurrence
scenario baseline-2027 {
  inherit accepted

  expect rent
    every month on day 1
    move 1_200.00 USD
      from me@checking
      to landlord

  constrain spendable-cash >= 5_000.00 USD

  assume salary-growth in 2% .. 4%
}

These expected events are enumerable and reportable but do not enter the accepted actual world.

XII. The journal remains a first-class projection

A user can still write:

2026-09-20 Groceries
  Expenses:Food       48.32 USD
  Assets:Checking    -48.32 USD

The compiler treats this as convenient syntax with incomplete economic detail. Depending on the declarations in scope, it can desugar into:

an exchange or consumption event
a transfer from the checking position
a recognition into the personal-cash book
an unresolved counterparty if omitted

When richer facts exist, the journal is generated:

report journal(personal-cash)
report journal(tax-us)

The two outputs may legitimately differ while pointing back to the same accepted event.

Round-trip rules:

Simple journal statements may remain in compact form.
Rich events that cannot be losslessly represented should remain event blocks.
Formatting must never discard source IDs, lot decisions, rights, or temporal distinctions.
Export to Beancount, Ledger, or another journal format is a projection and may emit warnings about lost semantics.
XIII. End-to-end workflows
1. Bank import and reconciliation
raw CSV bytes
  → adapter emits row observations
  → typed parser preserves exact strings and source positions
  → candidate matching finds existing events
  → reconciliation produces possible same-as links
  → unique high-confidence links may be policy-selected
  → ambiguous links enter the decision inbox
  → accepted events update affected book projections

Idempotence is based on source occurrence identity and content hash, not merely amount/date/description.

A repeated CSV import should produce no new accepted event. A genuinely duplicated bank transaction must remain representable.

2. Monthly close
declare source completeness
run unresolved-hole query
run conflict query
run acceptance checks
resolve or waive exceptions explicitly
compile books under pinned policy versions
sign close object

A waiver is itself a scoped decision with authority and rationale. It does not turn a failed check into a true proposition.

3. Investment disposal
observe trade confirmation
identify instrument and quantities
link cash settlement and fees
enumerate eligible lots
apply named lot-selection policy or decision
apply book-specific basis adjustments
derive recognized gain/loss
retain alternative results if another book selects differently

Changing one lot decision should invalidate only dependent basis, gain, tax, and report queries—not reparsing unrelated statements.

4. Bounced check
obligation remains due
check issuance records an attempted settlement
bank may expose provisional position
return event refutes successful final settlement
provisional position is reversed by a new event
fee is independently observed
proof explains why the obligation is still outstanding

No deletion and no special “uncleared transaction” mutation are required.

5. Forecast and budget
fork accepted world into scenario
instantiate bounded recurring plans
add uncertain assumptions
derive future obligations and liquidity paths
apply constraints
optionally optimize a plan
verify proposed plan exactly
compare scenario against actual world as events occur

A realized rent payment may link to its forecast counterpart without converting the forecast event into an actual event.

XIV. Edge-case architecture

No finite checklist can enumerate every future financial product or jurisdictional rule. The goal is to cover the structural classes so that new products decompose into existing primitives rather than requiring new core booleans.

A. Evidence and reconciliation
Edge case	Required behavior
Two identical source rows	Preserve separate occurrences unless linked by explicit source identity
Same event from receipt and bank	Derive candidate identity link; retain both evidence leaves
Statement corrected after import	New evidence supersedes old for a scope; old remains auditable
OCR uncertain between 8 and 3	Store alternative literals or an uncertainty set tied to image region
Missing date or commodity	Create typed hole and block dependent recognition
Conflicting balances	Return conflict with both proofs
Incomplete statement period	Do not infer absence
Deleted source file	Existing commit remains reproducible from retained content or records a deliberate unavailable-source tombstone
One source row represents several events	Permit split relation with quantity conservation
Several rows represent one event	Permit merge/reconciliation group
Fuzzy merchant matching	Produce ranked candidates, not accepted classification
Importer bug found later	Re-run under new adapter version; preserve both derivation histories
B. Settlement and payments
Edge case	Required behavior
Card authorization but no capture	Encumbrance may reduce availability; no final settlement
Partial capture	Release unused hold and settle captured amount
Authorization expires	State transition releases encumbrance
Pending ACH at period close	Book policy decides recognition; actual settlement remains pending
Returned ACH	Preserve attempted payment and return event
Bounced check	Underlying obligation remains unsatisfied
Chargeback	Model dispute and reverse settlement without deleting sale
Partial refund	Link to original exchange and preserve residual consideration
Tip adjusted after authorization	Separate authorization and final capture amounts
Payment satisfies several invoices	Explicit allocation relation
Several payments satisfy one invoice	Partial satisfaction over time
Overpayment	Create credit or reverse obligation according to policy
Fee withheld from transfer	Represent fee as separate consideration/charge leg
Cash withdrawal plus ATM fee	One grouped transaction containing independent events
Foreign card purchase	Separate merchant amount, card-network conversion, issuer conversion, and fees where evidence permits
Net settlement batch	Allocate batch result to underlying obligations with residual reconciliation
C. Ownership, custody, and accounts
Edge case	Required behavior
Joint account	Legal ownership, beneficial interests, and control represented separately
Authorized card user	User may cause events without being contractual debtor
Escrow	Custodian controls; beneficiaries retain conditional rights
Security deposit	Position plus repayment obligation and restrictions
Restricted grant funds	Same venue, separate encumbrance and allowable-use constraints
Trust or custodial account	Trustee, beneficiary, legal owner, and tax owner need not coincide
Borrowed securities	Custody and use without beneficial ownership
Short position	Obligation to return instrument, not merely a mysterious negative quantity
Pledged collateral	Position remains, but rights and liquidity change
Multi-currency account	Separate positions under one venue
Overdraft	Contractual credit facility creates obligation; negative cash is not assumed universally legal
Closed account	Venue validity ends; historical positions and evidence remain
Renamed or merged institution	External aliases change without rewriting account identity
Virtual envelope budgeting	Named view or encumbrance, not fictitious external bank account
D. Instruments, quotes, and investments
Edge case	Required behavior
No quote available	Return unknown valuation with missing requirement
Conflicting quotes	Preserve alternatives; named policy selects or report remains ambiguous
Stale quote	Policy violation or conditional result, not silent use
Bid versus ask	Valuation policy chooses appropriate side
Currency triangulation	Return route and all quote proofs
Different market calendars	Carry venue time and calendar
Fractional shares	Exact instrument quantity and venue quantum
Stock split	Corporate action transforms quantities and lots without fabricated gain
Reverse split with cash in lieu	Transform lot plus separate disposal/settlement
Merger or exchange	Map old rights to new instruments through corporate-action terms
Spin-off	Allocate basis under a named policy
Return of capital	Adjust basis and possibly recognize excess under book rules
Dividend reinvestment	Distribution and acquisition remain linked but distinct
Option exercise	Transform option rights into acquisition/disposal events
Option expiration	Retire right; book policy determines loss recognition
Assignment	Transfer contractual role and create resulting events
Wash-sale-like adjustment	Jurisdiction-specific package rewrites recognized basis, not source acquisition
Negative price	Exact signed quote allowed where instrument semantics permit
Nonfungible item	Unique identity and appraisal rather than fungible lot matching
Barter	Coupled transfers with no requirement that one leg be currency
Basket or fund	Constituents and quoted instrument value can coexist
Token redenomination	Corporate/instrument action with exact conversion ratio
Instrument expiration	Rights terminate according to contract, with explicit event or rule proof
E. Accounting and reporting
Edge case	Required behavior
Cash versus accrual	Two recognizers over one accepted event set
Prepaid expense	Position/right consumed over time
Deferred revenue	Obligation to perform plus cash position
Interest accrual	Time-dependent derived obligation with explicit convention
Amortization	Scheduled recognition over a declared basis and method
Depreciation	Book-specific measurement, not degradation of source evidence
Impairment	New measurement assertion with policy proof
Bad-debt write-off	Recognition decision; underlying claim history remains
Contra account	Reporting relationship, not a different economic ontology
Intercompany transaction	Shared event with entity-relative views
Consolidation elimination	Pure group-book recognition
FX remeasurement	Book-specific valuation at period boundary
Translation reserve	Recognized result under consolidation policy
Reversal versus correction	Reversal is a new economic/accounting event; correction supersedes an assertion
Period close and later discovery	New restated close rather than mutation
Materiality threshold	Named policy and proof, not hard-coded rounding
Multiple jurisdictions	Independently versioned recognition packages
Policy changes midyear	Effective intervals and explicit transition rules
Unknown tax treatment	Keep unresolved classification and expose alternatives
F. Temporal behavior
Edge case	Required behavior
Local midnight and UTC differ	Preserve named zone and local legal date
Daylight-saving transition	Use zone-aware instants and explicit ambiguity handling
Only month is known	Store coarse interval rather than invented day
Event occurs over a span	Represent interval or multiple linked events
Backdated entry	Separate occurrence time from recording time
Retroactive contract	Effective date may precede observation
Late settlement	Occurrence, recognition, and settlement remain independent
Recurring event on missing calendar day	Recurrence policy states skip, clamp, or move
Business-day adjustment	Named calendar package
Open-ended obligation	Unbounded due interval with conditions
Policy valid only for certain dates	Rule package has effective interval
Time-dependent source correction	Query supports “as known then” and “as currently known”
G. Logic and solver behavior
Edge case	Required behavior
Positive recursive rule	Compute least fixed point
Cycle with no base case	Do not invent proof
Negation through recursion	Reject at compile time or isolate in unsupported fragment
Aggregate over incomplete set	Return blocked/unknown
Multiple independent proofs	Retain shared proof DAG and optionally minimize explanation
Proof and refutation both present	Conflict, not explosion
Several candidate substitutions	Ambiguous result
Solver timeout	Resource-limited result, never refutation
Unsupported nonlinear relation	Residual obligation or explicit unsupported diagnostic
External solver produces model	Verify exact feasibility before acceptance
Conflicting policy packages	Coherence error or explicit candidate alternatives
Rule package update	Cache keys include package identity
Search-order dependence	Property test and reject as semantic bug
Infinite event generation	Require bounded horizon
User-defined rule creates objects recursively	Reject unless finite and well-founded
Stale cache after completeness change	Completeness context participates in dependency graph
H. Planning and uncertainty
Edge case	Required behavior
Expected event never occurs	Forecast remains separate; variance is reportable
Actual event differs from plan	Link them and derive variance
Amount is a range	Use interval constraints
Several possible dates	Use temporal alternatives or interval
Conditional income	Scenario rule with condition
Infeasible budget	Return unsatisfied core
Several feasible plans	Return alternatives or Pareto frontier
Optimization uses approximate solver	Exact verifier checks the proposed plan
Risk distribution	Keep probability model and provenance distinct from exact accepted facts
Scenario imports actual evidence	Explicit inheritance boundary
Scenario overrides an accepted fact	Scoped assumption, not mutation
Scenario is merged	Require explicit reconciliation of divergent assumptions
I. Security and collaboration
Edge case	Required behavior
Malicious importer	Sandboxed, capability-limited, observations only
Pathological rule package	Resource quotas and compile-time stratification checks
Package update changes results	Pin package hashes in every close
Revoked signer	Preserve historic signature and record revocation interval
Shared book with private evidence	Derived facts inherit information-flow labels
Redacted report	Proof projection must not leak hidden values through explanations
Concurrent conflicting decisions	Semantic merge conflict, not last-writer-wins
Lost encryption key	Explicit unavailable evidence state; no fabricated reconstruction
Compromised data source	Mark authority invalid over an interval and recompute dependents
Agent-generated proposal	Candidate only until accepted by declared authority
XV. Storage, versioning, and collaboration
1. Immutable object graph

Use a content-addressed store for normalized semantic objects:

evidence blobs
parsed observations
events
statements
rules
policy packages
decisions
proof nodes
commits
closes

Do not conflate content hash with occurrence identity.

A commit should contain:

parent commit(s)
root evidence set
accepted statement set
decision set
completeness claims
policy package hashes
schema version
author/signature metadata

This supports:

exact report reproduction;
branch-and-compare policy experiments;
collaborative merges;
historical “what was known then?” queries;
content-based deduplication;
rule-version coexistence.

There is no need to turn this into a blockchain. Signed Merkle-style commits and optional transparency logs are enough for ordinary local and organizational auditability.

2. Corrections and deletion

Corrections create:

new object
supersedes relation
scope of supersession
reason
authority

Privacy-mandated deletion may remove encrypted payload bytes while preserving an auditable tombstone and hashes where lawful. Semantic code should distinguish:

not supplied
unavailable
redacted
deleted
corrupt
unsupported

Those states are not equivalent.

3. Merge semantics

Evidence generally merges by set union. Names, decisions, policies, and completeness claims may conflict.

A semantic merge should report:

both branches selected different lots
both branches assigned different entity identity
one branch declared a relation complete and the other added evidence outside it
book package versions diverged

The merge does not need to resolve all conflicts before storing the combined branch. It must prevent a supposedly consistent close until required conflicts are resolved.

4. Storage tiers

A practical progression:

local alpha
  SQLite or embedded transactional metadata
  immutable blob directory or packfiles
  in-memory Salsa database
  generated indexes

larger installations
  append-only object packs
  persistent relation indexes
  columnar Arrow/Parquet projections
  distributed object storage
  deterministic worker execution

The source of truth remains the immutable semantic graph; columnar tables and search indexes are rebuildable projections.

XVI. User experience

The system should feel less like a theorem prover than Rust feels like first-order logic.

1. Decision inbox

Group unresolved items by cause:

missing identity
multiple account matches
multiple eligible lots
missing quote
conflicting evidence
policy conflict
incomplete period
failed acceptance check

Resolve one item and show all consequences before committing.

2. Proof-oriented diagnostics
error[ELOT001]: disposal has two admissible lots

  event sell
        ^^^^

candidate 1
  lot buy/one
  acquired 2026-01-04
  remaining 10 ABC
  recognized basis 201.00 USD

candidate 2
  lot buy/two
  acquired 2026-04-09
  remaining 10 ABC
  recognized basis 301.00 USD

recognized gain is blocked because no lot-selection
decision or policy applies.

help:
  decide sell.lot = buy/one.lot
  use policy lots/fifo for book tax-us
  inspect alternatives sell.lot

Diagnostics should link directly to evidence and proof nodes.

3. “Explain this number”

Every report cell should answer:

Which accepted facts contribute?
Which book rules transformed them?
Which quotes were used?
Which rounding occurred?
Which human decisions matter?
What source documents support them?
What would change the result?

A user should be able to expand from a balance to positions, from positions to events, from events to evidence, and from recognition to exact policy clauses.

4. Time travel

Two independent forms:

as-of economic time
as-known-at recording time

Examples:

What did the book report on September 30 using information available then?
What do we now believe the September 30 position was?
5. Scenario comparison

A scenario diff should operate semantically:

baseline vs new-job
  available cash
  future obligations
  liquidity paths
  tax-book effects
  breached constraints
  assumptions responsible

Do not present scenarios as fake transactions mixed into the real journal.

XVII. Performance plan

The targets below are engineering goals, not assumptions.

Workload	Target
Personal or small-business book, up to ~1 million propositions	Interactive warm queries and diagnostics, generally under 100 ms
Large portfolio or organizational book, tens to hundreds of millions of facts	Incremental queries in seconds, with partitioning
Institutional or research deployment, billions of facts	Distributed relation and analytics tier without changing semantics
Core techniques
Intern symbols and canonical terms.
Store relation columns densely rather than one heap object per fact.
Index clauses by predicate and discriminating arguments.
Key caches by canonical goal and semantic context.
Partition by entity, world, book, instrument, and time where sound.
Use delta propagation for changed evidence chunks.
Recompute only affected strongly connected components.
Hash-cons proof nodes.
Build full proof presentation lazily.
Maintain minimal explanation summaries separately from full derivations.
Separate transaction-oriented semantic queries from columnar analytics.
Parallelize deterministic relation strata and independent goals.
Record invalidation fan-out as a first-class benchmark.
Bound and report expensive abductive or optimization queries.
Keep a reproducible “slow reference mode” for debugging.
Benchmark corpus

Build benchmarks around:

ten-year personal bank history
high-frequency brokerage trades and lots
multi-currency business books
corporate-action-heavy portfolio
large invoice/payment graph
recursive ownership network
conflicting imports
one-row change near period close
policy-package upgrade
adversarial recursive rule set
large proof explanation

Track:

parse and normalization time
cold solve time
warm solve time
incremental update time
peak memory
relation sizes
cache hit rate
proof DAG size
invalidated query count
determinism across thread counts
XVIII. Validation and formal assurance
1. Reference versus production

Maintain two engines:

reference semantics
  formality-core
  simple and exhaustive
  small workloads

production semantics
  specialized Axiom solver
  indexed and incremental
  large workloads

Run every semantic fixture through both where supported.

2. Core properties

Property-based tests should assert:

source order does not change meaning
variable names do not change canonical answers
adding unrelated evidence does not change a result
monotone strata do not lose conclusions when evidence is added
no accepted result lacks a valid proof
proof checking reproduces the proposition
scenario facts cannot leak into actual-world reports
journal projection balances when the recognizer claims it does
transfers conserve quantity unless explicitly issuing or retiring
round-trip formatting preserves semantic identity
reimporting the same source is idempotent

For nonmonotonic operations, test that invalidation is explicit and limited to rules depending on completeness, selection, or negation.

3. Differential tests

For conventional cases, compare generated journal projections with established ledger systems. This does not make those systems the semantic oracle; it establishes compatibility for the common subset.

Also compare:

exact arithmetic engine against symbolic calculations
relational closure against a straightforward implementation
incremental result against clean full recomputation
optimized solver against reference model
parallel execution against single-threaded execution
4. Fuzzing

Fuzz:

parser and formatter
canonical serialization
unification snapshots and rollback
goal canonicalization
search-graph cycles
proof decoder and checker
policy-package compiler
temporal intervals
quantity conversions
content-addressed merge

Particularly dangerous cases are:

nested cycles
resource-bound-dependent answers
huge alternative sets
deep proof DAGs
negation plus completeness
zero and unit inference
near-overflow exact numbers
malicious component output
5. Mechanized specification

Long term, formalize the small trusted kernel in a proof assistant:

typing soundness for the core term language
soundness of proof checking
stratification guarantees
conservation theorem for transfer projections
semantic isolation of scenarios
correctness of scoped closed-world inference

The production Rust solver need not be extracted from the proof assistant. A small verified checker plus exhaustive differential testing gives a more practical trust boundary.

XIX. Phased execution plan

The following assumes a focused core team of roughly five to seven people. A smaller team can execute the early phases but should expect the full program to stretch considerably.

Phase	Rough duration	Deliverable and exit gate
0. Semantic constitution	4–6 weeks	Written ontology, logic fragment, time model, exact number model, proof format, and no-go rules. At least 75 edge cases represented without ad hoc core flags.
1. Executable reference model	8–12 weeks	formality-core model for entities, accounts, transfers, obligations, lots, quotes, valuation, and recognition. Proof and structured failure examples.
2. Vertical-slice demonstration	4–6 weeks	Two buys, ambiguous disposal, conflicting quotes, a policy decision, exact gain, journal projection, proof tree, and incremental change demo.
3. Finance-native solver kernel	4–6 months	Canonical IR, rollback unification, tabled search, cycle handling, exact arithmetic, proof DAG, independent checker. Differentially equivalent to the reference subset.
4. Language and incremental compiler	4–6 months	Parser, formatter, type system, Salsa database, modules, packages, source diagnostics, LSP foundation.
5. Immutable store and reconciliation	3–5 months	Content-addressed commits, import SDK, source provenance, identity candidates, decision inbox, bank workflow, time travel.
6. Personal and small-business alpha	4–6 months	Bank, card, cash, invoices, obligations, accrual/cash books, close workflow, budgets, forecasts, journal exports.
7. Investment and contract model	6–9 months	Lots, corporate actions, derivatives foundation, debt schedules, collateral, multi-currency valuation, policy packages.
8. Collaboration and plugin ecosystem	4–8 months, overlapping	Signed packages, Wasm components, branch/merge, access control, reproducible adapters, shared books.
9. Planning and optimization	4–6 months	Scenario constraints, liquidity graph, exact verification of optimized plans, Pareto results.
10. Scale and assurance	Continuous	Columnar analytics, distributed relations, proof compaction, adversarial fuzzing, mechanized checker specification, stable file and plugin formats.

A credible robust alpha is likely a 12–18 month effort for a strong multidisciplinary team. A broad, hardened system capable of replacing conventional ledgers in varied domains is closer to a multi-year program.

XX. The first vertical slice

Do not begin with every account type, a GUI, or an importer marketplace. Build one example that forces nearly the entire architecture to prove itself.

Inputs
Buy 10 ABC for 200 USD plus 1 USD fee.
Buy 10 ABC for 300 USD plus 1 USD fee.
Sell 10 ABC for 500 USD.
Do not specify a lot.

Provide two conflicting ABC/USD closing quotes.
Provide a brokerage statement quantity.
Provide a cash-settlement observation.
Required initial result
share quantity:
  proven

cash settlement:
  proven

eligible disposal lots:
  buy/one
  buy/two

selected lot:
  ambiguous

gain:
  conditional
    buy/one → 299.00 USD
    buy/two → 199.00 USD

recognized gain:
  blocked by LotSelection

market valuation:
  ambiguous because quote sources disagree

journal:
  partially derivable
  dependent entries visibly blocked
Then add a FIFO policy

The result must collapse with a proof:

selected lot = buy/one

because
  policy lots/fifo applies to tax-us
  buy/one acquisition precedes buy/two
  both lots are eligible
  buy/one has sufficient remaining quantity
Then add a conflicting specific-identification decision

The system must report:

policy selects buy/one
decision selects buy/two

status:
  conflict within tax-us recognition context

raw economic disposal remains accepted
tax gain remains blocked until conflict resolution
Then change one quote

Only valuation-dependent queries should recompute. Lot eligibility, cash settlement, and unrelated books should remain cached.

Vertical-slice acceptance criteria
Exact arithmetic only.
No hidden transaction generation.
Every answer has a proof.
Ambiguity survives until policy or decision.
Conflict does not corrupt unrelated results.
Source order does not affect answers.
Journal projection is derived.
Scenario and actual worlds remain isolated.
Reference and production engines agree.
Changing one input shows precise incremental invalidation.

This single example validates more of the architecture than months spent building syntax around conventional postings.

XXI. What must deliberately be deferred

Thinking big requires resisting the wrong forms of ambition.

Do not put these in the first trusted kernel:

arbitrary Python or Rust plugins
unrestricted user recursion
automatic external payments
machine-learning classifications treated as facts
global rule priorities
probabilistic facts mixed with exact facts
blockchain consensus
every jurisdiction’s tax law
general nonlinear symbolic mathematics
opaque AI agents that can commit accepted decisions

Machine learning can rank reconciliation candidates. An agent can draft a policy, propose classifications, or explain proofs. Its output enters as a candidate with provenance and confidence, never as invisible authority.

XXII. Long-term ceiling

Once the foundation is sound, Axiom can grow beyond bookkeeping without changing its core model.

Cross-party contract reasoning

Two organizations could share a selectively disclosed contract/event boundary:

invoice issued
goods accepted
payment instructed
payment settled
dispute opened

Each retains a private book while cryptographically agreeing on shared event identities.

Certified compliance

A report could include machine-checkable claims:

all recognized entries trace to accepted events
all accepted events trace to evidence or signed decisions
all conversions use an allowed valuation policy
all required completeness claims are present
Selective disclosure

A future research layer could prove limited assertions such as:

liquid assets exceed a threshold
a covenant is satisfied
a report balances under a pinned policy

without revealing every underlying event. The ordinary proof graph and content-addressed commitments are prerequisites for such work.

Economic digital twins

Households, companies, cooperatives, funds, and projects could model:

positions
obligations
contracts
settlement networks
future plans
risk constraints
liquidity
ownership
recognition policies

within one coherent system.

Public rule ecosystems

Accounting and tax experts could publish versioned, testable policy packages containing:

scope
jurisdiction
effective interval
formal rules
examples
counterexamples
migration behavior
source references
compatibility declarations

Books would pin exact package versions, and policy upgrades could be evaluated as semantic diffs before adoption.

Final architectural commitment

The system should be built around this separation:

Evidence says what was observed.
The ontology says what kinds of economic things can exist.
Logic says what follows.
Constraints say what must be coherent.
Policies say how alternatives are selected or recognized.
Decisions record where authority resolved ambiguity.
Books interpret accepted facts.
Reports project books.
Scenarios describe worlds that are not yet actual.
Proofs connect every layer.

Rust’s trait-solver infrastructure is valuable because it demonstrates how to handle recursive goals, inference variables, canonical queries, ambiguity, caching, and proof-oriented diagnostics at compiler scale. But the final Axiom kernel should be finance-native and substantially broader than trait solving.

The first implementation artifact should therefore be neither a journal parser nor an account hierarchy. It should be a small executable semantics, a corpus of difficult examples, and the lot–valuation–balancing vertical slice above. Once that works without hidden defaults, mutation, floats, or unexplained answers, the rest of the system has a foundation strong enough to carry the much larger vision.
