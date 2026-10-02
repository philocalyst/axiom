# Typed associations for Axiom v5

Lane R5. The user's complaint: *"the jordan-401k is proof that the typing is still a little weak, should be easy and
declarative to setup associations for accounts if need be."* This document designs the answer: easy when the book is
simple, precise when it is not. Every claim about the repository was read in the files named; every claim about a source
is marked **verified** (read in the primary text or its repository), **search** (confirmed by a search result, not read) or
**memory** (not checked; the orchestrator should not trust it without a look).

The ledgers in `ledgers/` are written in the syntax defined here: `household.ax` is the showcase, and `small-business.ax`,
`landlord.ax`, `freelancer.ax`, `expat.ax` and `shared.ax` use it for engagements, leases, plans and tenancies.

---

## 1. What is wrong today, exactly

v4 already has typed properties. `kind NAME … has PROP TYPE` is in LANGUAGE.md §6, and the three that matter are in the
shipped systems:

```text
us/401k.ax:28    has employer entity          // any entity at all: a grocer, a person, riley
us/529.ax:30     has beneficiary entity       // same
us/hsa.ax:29     has coverage name            // any word: `family`, `famly`, `banana`
std.ax:117       kind employer : org          // a person cannot be an employer
std.ax:136       kind tenant : org            // or a tenant
```

The `Ty` the checker has for a slot is `Ty::Entity`, with no kind and no count (`crates/model/src/props.rs`, `Has { name, ty }`).
That is the whole of the weakness, and it shows up as four failures in `examples/05-family/accounts.ax`:

| # | failure | where |
|---|---|---|
| 1 | **A name encodes a relation.** `jordan-401k`, `alex-401k`, `riley-529`, `joint-checking`. The owner is written twice: in the name and on the `owner` line, and nothing says they agree. | `accounts.ax:51-63` |
| 2 | **A slot accepts anything.** `beneficiary acme` passes: `acme` is an `employer`, and `Ty::Entity` does not look. `coverage famly` passes. | `accounts.ax:52-63` |
| 3 | **Nothing is inferred.** Alex has one employer, so `employer acme` on `alex-401k` states what is already known. The HSA's `coverage family` restates the HDHP Alex joined in the paycheck. The escrow belongs to the mortgage and is declared as a stranger. | `accounts.ax:44-75` |
| 4 | **Nothing is required or counted.** A 529 with no beneficiary, or two, is accepted. `dcfsa` has `owner family`, which is wrong (an FSA belongs to the employee; the dependents are who it is for), and the file cannot say so. | `accounts.ax:59-63` |

A fifth failure sits behind these: the *same fact* is spelled in three places. The mortgage is an `account`, a `contract`
and a `code mortgage-* on mortgage` (`accounts.ax:70`, `accounts.ax:119`, `contracts.ax:4-9`). Jordan is `member family` on the person,
and the household says `children 1` (`axiom.ax:19,27,33,38`). The 401(k) match is typed as an `also` line per contract
(`contracts.ax:50,61,74`).

The design below changes one thing: **a relation is a typed, counted slot of a kind, and a thing's slots are filled by
words the checker places by type.** Everything else follows.

---

## 2. Theory, and the rule each source gives the design

### 2.1 UFO relators and roles (Guizzardi)

*What it says.* A relation that has its own attributes, its own span of time and its own consequences is not an edge: it is
a **relator**, a thing that *mediates* two or more participants. An employment, a marriage, a lease and a 401(k)
membership are relators; "employer" and "tenant" are **roles**, anti-rigid, dependent on the relator, that a thing plays
for a time. A **role mixin** is a classifier that things of different kinds can play (a landlord may be a person or an
org). A **phase** is a partition of a kind by a condition on the thing (a person is a minor until 18).

*Sources.* G. Guizzardi, *Ontological Foundations for Structural Conceptual Models*, PhD thesis, University of Twente, 2005
(**memory**: relator and role definitions). G. Guizzardi, G. Wagner, J. P. A. Almeida, R. S. S. Guizzardi, "Towards
ontological foundations for conceptual modeling: the unified foundational ontology (UFO) story", *Applied Ontology*
10(3-4), 259-271, 2015, DOI 10.3233/AO-150157 (**search**). The lightweight OWL form, gUFO, is **verified** in
`nemo-ufes/gufo` (`gufo.ttl:1222`):

```text
gufo:Relator  rdfs:subClassOf  gufo:ExtrinsicAspect,
              [ owl:onProperty gufo:mediates ;  owl:minQualifiedCardinality 2 ;  owl:onClass gufo:Endurant ]
  rdfs:comment "… Examples of relators include John and Mary's marriage … Mary's employment contract at Nasa …"
```

*What the design takes.*
1. **Employment, plan membership, lease, management, engagement and every position are relators.** A relator has at
   least two participant slots (`gufo:minQualifiedCardinality 2` on `mediates`), checked when the kind is declared: a
   position has `owner` and `with`, a lease `tenant` and `landlord`. A name may not carry a participant.
2. **Roles are kinds a thing plays, so role kinds extend `agent`, not `org`.** `kind landlord : org` (std.ax) rejects a
   person who lets a room; `landlord : agent` does not. `ledgers/expat.ax` has `entity sr-ferreira : landlord // a person`.
3. **A slot's range is a union of kinds** (`person | household`), which is a role mixin written inline.
4. **A phase is a `when`, never a field.** `children` is a number in v4 (`std.ax:140`, `children 1`) and does not age. A
   dependent under 17 on the last day of the year is `born` read on a day. (Closes hh-c01 in change A12.)

*What it does not take.* The rest of UFO: sortals, mixins, modes, qualities, the OntoUML stereotype set, the endurant/
perdurant split. Axiom has kinds, slots, timelines and derived conditions. Cost of the borrowing: none beyond the word
"relator" in the documentation; the language has no new keyword for it, because a kind is a relator exactly when it has
two or more agent slots.

### 2.2 REA: typification, custody, responsibility

*What it says.* The accounting ontology of McCarthy (resources, events, agents) carries, beyond the duality of events, a
**typification** layer (a resource has a type; an agent has a type) and **participation / custody / responsibility**
relations (an agent has custody of a resource; an agent is responsible for another). Geerts and McCarthy add commitments
and contracts.

*Sources.* W. E. McCarthy, "The REA accounting model: a generalized framework for accounting systems in a shared data
environment", *The Accounting Review* 57(3), 554-578, 1982 (**memory**). G. L. Geerts and W. E. McCarthy, "An ontological
analysis of the economic primitives of the extended-REA enterprise information architecture", *International Journal of
Accounting Information Systems* 3(1), 1-16, 2002, DOI 10.1016/S1467-0895(01)00020-3 (**search**).

*What the design takes.* A position is REA **custody**: the owner is the agent for whom the value is held, `with` is the
agent that holds it. `sponsor` (who offers the plan, and so who is responsible for the match) and `beneficiary` (for whose
benefit) are **responsibility**-class slots. Typification gives the kind: `401k` is a position *type*. PROPOSAL K3 already
reads a position as "an owner's standing with an agent" (`Position { owner, with, name, kind, class }`); this design adds the
other slots a kind declares and does not change those two.

*What it does not take.* REA duality and stock-flow objects: that is K4.

### 2.3 ValueFlows: a lesson in what not to add

*What it says.* ValueFlows 0.x had `AgentRelationship { subject, relationship, object }` with `AgentRelationshipRole
{ roleLabel, inverseRoleLabel }`: an untyped triple with a free label ("is member of"). **Verified** in `valueflows/valueflows`:
`CHANGELOG.md` reads "0.16 (December 2025): Remove AgentRelationship, AgentRelationshipRole and related data (breaking)";
1.0.0 is dated February 2026; the schema files are titled `AgentRelationship-DEPRECATED`. What 1.0 keeps are **typed properties
with cardinality**: `vf:provider` and `vf:receiver` are `owl:maxCardinality 1` with range `vf:Agent` (`all_vf.TTL`).

*What the design takes.* There is no generic `relation` statement and no free-label association. Every association is a slot
of some kind, with a range and a count. This is also the user's complaint, in a standard's words: a label nothing declares
has no checkable meaning.

### 2.4 Description logics: role restrictions and qualified number restrictions

*What it says.* A role restriction `∀R.C` says every filler of R is a C; `∃R.C` says one is. A **qualified number
restriction** `≥n R.C`, `≤n R.C` counts only the fillers that are C. OWL 2 has them as `owl:onClass` with
`qualifiedCardinality`. SHACL validates the same shapes closed-world (`sh:class`, `sh:minCount`, `sh:maxCount`).

*Sources.* B. Hollunder and F. Baader, "Qualifying number restrictions in concept languages", KR 1991, 335-346 (**search**).
F. Baader et al., *The Description Logic Handbook*, Cambridge University Press, 2003 (**memory**). W3C, *OWL 2 Web Ontology
Language Structural Specification*, 2012 and *Shapes Constraint Language (SHACL)*, 2017 (**memory**; the cardinality
terms are also in the gUFO file above, **verified**).

*What the design takes.* A slot is `∀R.C` (its range) plus a number restriction (its multiplicity), and the count is
**qualified**: `has dependents person many` counts persons, not whatever else is attached. The multiplicity words are
Alloy's (`one`, `lone`, `some`, `set`; D. Jackson, *Software Abstractions*, MIT Press, 2006, **memory**) renamed for a
non-programmer: `one` is the default (no word), `optional`, `some`, `many`.

*What it does not take.* Open-world inference. A DL would *infer* that `riley` is a person because the 529's range says so.
A ledger must say *error* instead: the check is closed-world, like SHACL, and `riley` has to be declared a person.

### 2.5 Record typing: kinds as records, with narrowing, and interfaces on laws

*What it says.* A record type is a set of labelled slots; **row polymorphism** (M. Wand 1987; D. Rémy 1989; D. Leijen,
"Extensible records with scoped labels", *Trends in Functional Programming* 2005, pp. 179-194, **search**) lets code
accept "any record with at least these slots". Haskell classes with **functional dependencies** (M. P. Jones, "Type classes
with functional dependencies", *ESOP* 2000, LNCS 1782, 230-244, **search**) let one parameter determine another (`owner →
sponsor`). **Bidirectional typing** (J. Dunfield and N. Krishnaswami, "Bidirectional typing", *ACM Computing Surveys* 54(5),
2021, DOI 10.1145/3450952, **search**) splits checking (the expected type pushes into the term) from synthesis (the term
yields a type).

*What the design takes.*
- A subkind **extends** its parent's slots and may **narrow** a range (`401k` narrows `owner` from `person | household` to
  `person`); widening is an error. That is width-and-depth record subtyping, and nothing more of row polymorphism.
- A **law written on a kind reads only the slots the kind declares**; to share a law across kinds (the yearly
  elective-deferral cap covers `401k`, `403b` and `simple-ira`) the kinds name a common parent that declares the slots the
  law reads. That is a typeclass-like interface with the parent as the class. No user-written polymorphic function.
- **Filling a slot is bidirectional.** Where a slot is named (`beneficiary riley`) the range is the expected type and the
  word is *checked*. Where a word is not named (`riley` after `family/529`) it is *synthesized* against every slot and placed
  only if one slot fits (section 4.3).
- A **default** (`has sponsor employer = employment[employee owner].employer`) is a functional dependency: the sponsor is
  determined by the owner and the day.

### 2.6 Other precedents, briefly

- **ORM** (T. Halpin, *Information Modeling and Relational Databases*, 2008, **memory**): fact types with named roles, mandatory
  and uniqueness constraints. `has beneficiary person` is a mandatory role with a uniqueness constraint on it.
- **Daml templates** (Digital Asset; **memory**): a contract template names its `signatory`, `observer` and `controller`
  parties as typed fields. A relator with typed parties, checked at creation, is the same idea in a ledger language that
  shipped.
- **ISO 20022** (**memory**): messages name parties by role (`Dbtr`, `Cdtr`, `DbtrAgt`, `CdtrAgt`, `Dpstr`). Sync (§14) can map a
  message's role names onto a kind's slot names, so the typed slots are also the sync vocabulary.

---

## 3. Slots

### 3.1 Declaration

A kind declares slots with `has`, which v4 already has; v5 adds a **range**, a **multiplicity** and a **default**:

```text
SLOT  := has NAME RANGE [MULT] [as with | as for] [by WEIGHT] [= DEFAULT]
RANGE := KIND (| KIND)*          // a union of kinds: `person | household`
       | one of NAME (| NAME)*   // a closed set of names: `one of self-only | family`
       | VALUE-TYPE             // v4's: amount percent date span number unit bool text (not `entity`, not `name`)
MULT  := (nothing: exactly one) | optional | some | many | N | N..M
WEIGHT:= NAME [UNIT]            // `by share`, `by rent USD`, `by area SQFT`
```

- `one` (no word) means exactly one, `optional` zero or one, `some` one or more, `many` zero or more.
- `as with` marks the slot that the preposition **`with`** (and v4's `at`) fills: the counterparty of a contract, the custodian
  of a position. At most one per kind. `as for` marks the slot that `for THING` fills: the property a mortgage is for, the
  beneficiary of an FSA's dependents. v4 had these prepositions as separate machinery for accounts (`at`) and contracts
  (`with`); here they are one marker on a slot.
- `by WEIGHT` says the values of a `some`/`many` slot carry **weights** (section 3.3).
- `= DEFAULT` is an expression over the slots declared before it, evaluated on the day the thing begins (section 4.4).

`owner` and `with` are slots every position has (PROPOSAL K3: `Position { owner, with, … }`). A kind may narrow them.
`entity` and `name` stop being ranges: `entity` was the weakness, `name` the enum without its members.

### 3.2 What the shipped kinds become

```text
kind agent : entity
kind org : agent
kind taxpayer : agent                           // v4: `taxpayer : entity`; keeps `filing`
kind person : taxpayer
kind household : taxpayer
  has members     person some                   // see 5.4; `member family` on each person disappears
  has dependents  person many
kind bank : org
kind broker : org
kind employer : agent                           // v4 `employer : org`: a person can employ a nanny
kind landlord : agent                           // v4 `landlord : org`
kind tenant   : agent                           // v4 `tenant : org`

kind 401k : tax-deferred                        // v4 us/401k.ax
  has owner     person                          // narrows the position's built-in owner
  has sponsor   employer = employment[employee owner].employer
  has custodian broker as with
  has plan      401k-plan = sponsor.401k-plan   // the sponsor's plan; an error if the sponsor offers none
kind 529 : tax-deferred                         // v4 `529-plan`, renamed so the kind and the usual name agree
  has owner       person | household
  has beneficiary person                        // exactly one: a 529 has one
  has custodian   broker as with
kind hsa : tax-deferred
  has owner     person
  has coverage  one of self-only | family = employment[employee owner].joins(hdhp).tier
  has custodian broker | bank as with
kind dcfsa : tax-deferred                       // v4 `dependent-care-fsa`
  has owner      person                         // the employee: not the family
  has sponsor    employer = employment[employee owner].employer
  has dependents person some                    // the qualifying persons it is for
  has administrator org as with
kind loan : position                            // PROPOSAL 6.2: a debt is a position with a promise body
  has lender   lender as with
  has property asset optional as for
kind mortgage : loan
  has property home as for                      // required here: a mortgage is for a home
  part escrow : escrow optional                 // 5.2
```

Two additions the kinds above need and v4 did not have: **`bank`, `broker`** as institution kinds (v4's `chase` and `fidelity`
are `org`, which does not satisfy `broker | bank`; this is the cost), and the **bracket selector** `KIND[SLOT VALUE]`:
the thing of that kind whose slot has that value on the day, an error unless exactly one (at most one for an `optional` use).
`employment[employee owner].employer` reads "the employer of the employment whose employee is this account's owner".
The selector is already in `ledgers/landlord.ax` (`management[scope unit].manager`); it is a query over the slots, not storage.

### 3.3 Weighted many-slots

A `some`/`many` slot may carry **weights**. Written after each value:

```text
entity lantern-row : s-corp
  owners  dana 60%, theo 40%                    // has owners person | household some by share
contract apartment-rent : lease
  uses    noor 880 SQFT, noor-consulting 120 SQFT    // has uses person | org some by area SQFT
contract lease-2025 : lease
  tenants priya 1_150.00 USD, marcus 950.00 USD, jules 900.00 USD     // has tenants person some by rent USD
```

A weight is a percent, a fraction or an amount of a unit, and the shares are proportional. v4 has two syntaxes for this
(`owner me 60%, theo 40%` for owners, `share 120 SQFT for studio` for use); the weighted slot replaces both. `for X by NAME`
at the end of a flow divides it by the weights of X's slot named `NAME`; without `by`, equally. That is all `sh-b02` in
`ledgers/shared.ax` needs.

### 3.4 Inheritance and narrowing

```text
kind roth-401k : 401k                  // inherits sponsor, custodian, plan
  has owner person                     // same: fine
kind ira : tax-deferred
  has owner person | household         // wider than 401k's: only legal as a parent's declaration
```

A subkind may repeat a slot only to **narrow** its range (a subset of kinds), tighten its multiplicity or add a default.
Anything else is `slot-widening` (section 8). Defaults are inherited and may be replaced.

---

## 4. Filling slots

A declaration provides **words**: role-word lines, the path before the name, the nesting container, bare words in the header.
The checker's job is to place each word in a slot of the thing's kind. Four ways, in this order.

### 4.1 By role word (always works, never ambiguous)

```text
family/529
  beneficiary riley          // role word, then a value
```

A line `ROLE VALUE[, VALUE …]` fills the slot of that name. The range checks the value (`wrong-kind`), the multiplicity counts
it (`too-many`), an unknown role is an error with a suggestion (`unknown-slot`). This is v4's property line with the type
the kind declared.

### 4.2 By structure: nesting and the path

A thing written **inside** another fills a slot with its container, and the **path** words before its name fill slots too.
Both are placed by type (4.3):

```text
entity fidelity : broker
  alex/401k                  // fidelity (container) -> custodian;  alex (path) -> owner
  jordan/bluefin/401k        // bluefin (path) -> sponsor
  family/529
    beneficiary riley
```

The last word is the **name**. It is the thing's kind when the word names a kind (`401k`, `hsa`, `529`, `mortgage`): "the
kind name stands for the name", as in `alex/401k`. A different name needs a kind: `family/checking : deposit`. Two words
that are both kinds are an error. Nothing in a name is a relation: `jordan-401k` and `riley-529` are not needed and a
checker can warn on them (`name-carries-slot`, section 8).

### 4.3 By inference when unique: forced placement, never a guess

Given the unfilled slots `S` of the kind and the unplaced words `W`:

1. For each word `w`, `cand(w)` is the set of slots whose range admits one of `w`'s kinds (a descendant counts).
2. If some `cand(w)` is empty: **`wrong-kind`**.
3. A **placement** puts every word in one of its candidate slots so that no slot is above its upper bound (`one` and `optional`
   take one word, `some` and `many` take any number). If there is no placement: **`too-many`**.
4. A word is **forced** when it sits in the same slot in *every* placement. Forced words are placed. Any word that is not
   forced is **`ambiguous-role`**, naming the slots it could fill and the role word that fixes it.
5. Required slots (`one`, `some`) still empty take their default (4.4); if there is none: **`missing-role`**.

The rule is "place a word only if every way of placing all the words agrees": it never takes the first slot because it came
first. It is computed by unit propagation (a word with one candidate is placed and its slot removed from the others, unless the slot
is `some`/`many`) or, since a kind has a handful of slots and a declaration a handful of words, by enumerating the placements.
It is bidirectional typing in the sense of 2.5: the ranges are the expected types.

Worked, on the 529 (`owner person | household`, `beneficiary person`, `custodian broker`):

| written | words | result |
|---|---|---|
| `family/529` + `beneficiary riley` | container fidelity, path `family`, role word `riley` | fidelity: only `custodian` (a broker is not a person); family: only `owner` (a household is not a person); riley placed by role word. **Accepted.** |
| `riley/529` | fidelity, `riley` | riley fits `owner` and `beneficiary`: **`ambiguous-role`**, fix `beneficiary riley` or `owner riley` |
| `family/529 riley` (bare word in the header) | fidelity, family, riley | family: only `owner`; then `beneficiary` is the only slot left that admits riley. **Accepted**, because propagation removed `owner` from riley's candidates. |
| `acme/529` | fidelity, acme (an employer) | acme fits no slot: **`wrong-kind`**, "acme is an employer; `owner` takes person or household" |

The bare-word case is the user's "easy when simple": `family/529 riley` is complete, because there is only one way for it to
be well-typed. The role word form is the "precise when not".

### 4.4 By default

`= DEFAULT` fills a slot nobody wrote, from the slots already filled. It is evaluated **once, on the day the thing begins**
(its `opened` day, else the first day of the book), and the result is stored as the first entry of that slot's K2 timeline.
Two consequences, both deliberate:

- A default is a **guess about the past**: a 401(k) that predates the book has no employment to read, and a sponsor that
  changed would be mis-read. When the default finds nothing or finds more than one, the error is **`unfilled-default`** at
  the declaration, and the fix is a role word. (`ledgers/household.ax`, mark `hh-u11`.)
- Later changes are *statements*, not re-evaluations: `06-01 jordan/bluefin/401k now sponsor …` is a K2 `now`; the default is
  never run again. History does not move when the employment does.

### 4.5 Positional filling: the verb

Sentences are positional because the pattern names the slots. A relator kind may declare **verbs**:

```text
kind employment : contract
  verb "{employee} works at {employer}"
  verb "{employee} stops working at {employer}"     // ends it, on the day
kind lease : contract
  verb "{tenant} rents {unit} from DATE"
```

`jordan works at pinnacle` is then a statement that begins an `employment` with those slots; `jordan stops working at
bluefin` ends the one whose `employer` is bluefin. Verbs are **sugar** (the long form `contract jordan-pinnacle :
employment with pinnacle` / `employee jordan` always works), and they are the last layer of section 9: they remove no
concept unless they also absorb v4's fixed statement list (`worked`, `used`, `owes`, `now`), which I did not check
the parser for and mark **unsure**.

### 4.6 The ladder: the same fact at five precisions

```text
alex/401k                                          // 1  all inferred: custodian by nesting, sponsor by default
jordan/bluefin/401k                                // 2  sponsor named by the path: needed once a person has two plans
jordan/401k  sponsor bluefin                       // 3  by role word
jordan/401k  sponsor bluefin  custodian fidelity   // 4  every slot written (a statement from the custodian says so)
jordan/401k : 401k  opened 2019-03-04  sponsor bluefin    // 5  kind and open day explicit
```

All five elaborate to the same `Position` and print as the canonical address (section 6) in every error, view and report.
The book gets more precise exactly where precision stops being free.

---

## 5. Relators: what a typed association implies

A relator kind is a kind with two or more agent slots. v4's `contract` is the root of the ones that carry a **schedule** (a
promise), and positions are the ones that carry a **balance**; the layer that does the work is the same. A relator kind may
carry:

| it can carry | word | what it replaces in v4 |
|---|---|---|
| participants | `has` slots | free property lines and `at`/`with` plumbing |
| positions that exist with it | `part` | hand-declared sibling accounts (the escrow); v4's `part of` for asset parts is the same word |
| legs it implies | `also … [when …]` | the same word in v4; now stated **once on the kind**, not per contract |
| consequences of its own life | `law`, with `on start` / `on end` triggers | hand-typed final pay, forfeiture, deposit settlement |
| sentences | `verb` | long-form declarations |

### 5.1 Employment, plan and membership

The employment is the relator between an employee and an employer; the plan is offered by the employer; a **membership** is
the relator between an employment and a plan, with its own attributes (the election), position and laws. The UML name for a
relation with attributes is an *association class*; UFO's name for it is the relator itself.

```text
kind employment : contract
  has employee person
  has employer employer as with
  has joins    plan many                          // each `joins` creates a membership of that plan's kind
  also withholding(gross - sum(joins.deferral) * gross, employee.filing) -> irs #federal-tax
  also 6.2% of gross -> irs #payroll-tax
  also 1.45% of gross -> irs #payroll-tax
  law final-pay on end:  derive … #wages "accrued PTO" at the supplemental rates      // hh-b06, [A3]

kind 401k-plan : plan                           // lives under its sponsor: the nesting fills `sponsor`
  has sponsor   employer
  has custodian broker
  has match     match-tiers = none              // `40% to 10%` or `100% to 3%, 50% to 5%`, a typed value
  has waits     span = 0d
  has vests     schedule = immediate
  membership 401k-membership                    // what `joins 401k-plan` creates

kind 401k-membership : plan-membership          // a relator: employment, plan, and an account
  has deferral percent = 0%                     // the election, an attribute of the membership
  part account : 401k                           // owner = the employee, sponsor = the plan's, custodian = the plan's
  also deferral * gross -> account #deferral         when eligible
  also plan.match(deferral, gross) -> account #match when eligible and employer is owner
  law eligibility:   eligible := date >= employment.from + plan.waits
  law vesting on end: derive (1 - plan.vests(service)) * (match in account) -> plan.sponsor #forfeiture   // hh-c03, [A3]
```

What a paycheck line is after this: `contract alex-pay : employment with acme` with `joins 401k-plan deferral 10%`. The
withholding, both FICA halves, the deferral and the match are the kind's and the membership's `also` lines, applied per
occurrence; the book never writes them. `ledgers/household.ax` shows the line and the legs it derives; `ledgers/small-business.ax`
shows the same kind with the employer's half and FUTA/SUTA (`when employer is owner`).

The one new thing in the sketch is the **lifecycle trigger** `on end`: a norm fired when the relator's span closes. PROPOSAL K6 has
the rule IR (`Derive(LegTemplate)`); the trigger is one more event source beside `on flow`/`in`/`out`/`gain`.

### 5.2 Parts: positions that come with the relator

```text
kind mortgage : loan
  part escrow : escrow optional                 // an optional part exists once the mortgage names it
```

A part is a thing that begins with each instance, **inherits the whole's slots of the same name** (the escrow's owner is the
mortgage's owner, its `with` the lender) and is addressed like any position. An optional part comes into being the first time
it is *named*: `also -> escrow 705.00 USD #escrow`, or `from escrow`, or an opening balance `rocket/escrow 3_410.00 USD`. PROPOSAL 6.2
writes the escrow as a sibling position that the contract names by hand; this is that, with the connection typed.

`part` earns its place by removing two things: the v4 `part of ASSET` property (a unit of a building, a room of a house,
`ledgers/landlord.ax`) and the hand-opened companion account. Both are "a thing that exists because another does".

### 5.3 Leases and management (the landlord)

```text
kind lease : contract
  has tenant   person | org
  has unit     asset
  has landlord landlord = unit.owner            // by default the owner of the unit
  has agent    org optional = management[scope unit].manager
  has deposit  amount optional                  // held by `agent` if there is one, else by the landlord: a claim for the tenant
  law settlement on end: unused prepaid rent, deductions, the 14-day return  // ll-b03, [A3]
  verb "{tenant} rents {unit} from DATE"
```

`management` is a relator between an owner and a manager with a many-slot `scope` of units: `elm-agreement`'s `scope unit-b, unit-c`
is what makes a lease's `agent` default to `elm-pm`, and the manager's 8% fee is the *management's* `also`, not a leg typed on
each lease. The deposit's two places (the claim and the trust it sits in) are the lease's `deposit` and the manager's position,
typed. Before and after, section 7.5.

### 5.4 Households: members and dependents, derived

`household` has `members person some` and `dependents person many`. `member family` on each person and `children 1` on the
household (v4, `axiom.ax:19,27,33,38`) both go: `members` is the household's slot, and the person's side is the same slot read
backwards (`household where members include alex`), a query. `children` is derived on a day: `count(dependents where age < 17)`,
which is what the return reads. In `ledgers/shared.ax` the household's members are *derived* from a lease's tenants (`members
tenants of lease-2025`): the relator is primary and the collective its projection.

---

## 6. Addressing: one form, the path

### 6.1 The rule

A position is named by **words in a fixed order separated by `/`**: its owner, then the values of its other slots in the order the
kind declares them, then its name. That is its **canonical address**, e.g. `jordan/bluefin/fidelity/401k`. Only slots whose filler
is a named thing (an agent or an asset) count; amounts, percents and names do not, and a slot whose default depends only on
other address slots (`plan = sponsor.401k-plan`) adds nothing and is left out. A reference is **any
subsequence** of the canonical address that identifies exactly one position **open on the line's day**:

```text
alex/401k              jordan/bluefin/401k          bluefin/401k   (the 401k at bluefin: one person)
fidelity/401k          // ambiguous: alex's and jordan's        -> error, with both canonical addresses
401k                   // ambiguous while two exist
checking               // unique in the family book; `chase/checking` and `bcp/checking` once expat.ax has two
rocket/escrow          // a part: owner family, custodian rocket, name escrow
```

PROPOSAL 6.2's form (`fidelity/brokerage`, the custodian then the name) is a subsequence of this; it keeps working.

### 6.2 Why this one

I considered three: `jordan's 401k`, `401k of jordan`, `jordan/401k`.

| | `jordan's 401k` | `401k of jordan` | `jordan/bluefin/401k` (chosen) |
|---|---|---|---|
| new lexeme | **yes**: `'s` (an apostrophe is not in the lexer's name or string set) | no | no: `/` is already "names group with `/`" (LANGUAGE §6) |
| collides with existing syntax | no | **yes**: `of` already means a purpose's object (`#insurance of house`), a share (`30% of #income`), a cap (`up to 10% of amount`), and a unit of a building (`part of`). `401k of jordan` and `insurance of house` cannot be told apart until both names resolve | the unit division `USD/HR` (lexically disjoint: units are upper-case, names lower-case) and `us/ca` (a system path, the same idea) |
| slots reachable | the owner only | the owner only | **any**: the sponsor (`bluefin/401k`), the custodian (`fidelity/401k`), the property (`house/mortgage`) |
| a second 401k | `jordan's bluefin 401k`: a new production | `401k of jordan at bluefin`: a new production | `jordan/bluefin/401k`: the same production |
| works as a declaration | no: it is a lookup | no | yes: the same words fill slots on creation (4.2) |
| printed in errors and views | needs a printer | needs a printer | the identity: the canonical address *is* the path |

The deciding point is the fourth row down: addressing and declaring are the same act. A path in a declaration fills slots by
type (4.2); the same path in the journal finds the thing (6.1). The two other forms are lookups only, privilege the owner, and
each needs a new grammar rule.

One thing that is **not** an addressing form: `alex/401k.sponsor` reads a slot with v4's `.field`. Addresses name; fields read.

### 6.3 Resolution is by the line's day

A reference is resolved against positions **open on the line's day**. A sibling that opens later does not make earlier lines
ambiguous. Jordan has `jordan/bluefin/401k` in January; the Pinnacle plan's position opens on 08-01 (60-day wait), so from 08-01
the bare `jordan/401k` is ambiguous and the checker says so on that day's lines, with the two canonical addresses, and on no
earlier line. This is the answer to the obvious fear (adding a thing breaks old lines). It is also a K2 timeline lookup, so the
incremental engine (THEORY.md, DBSP/Salsa) recomputes only the lines on or after the day a position opens.

The remaining risk is real and stated in section 10: the bare form `jordan/401k` is *stable* only until a second one opens.
`axiom fix` can rewrite journal references to their shortest stable form.

### 6.4 Names that are not positions

Entities (`alex`, `fidelity`) and assets (`house`) are flat names, as in v4. Parts are addressed as positions. A contract keeps its
own name (`alex-pay`); its slots are fields (`alex-pay.employer`).

---

## 7. Before and after

All "after" text is the syntax of sections 3-6, and every line of it is used in `ledgers/household.ax`, `small-business.ax` or
`landlord.ax` (the `shared.ax`, `freelancer.ax` and `expat.ax` ledgers use the same forms). Where the "before" is not a file in the repo it is marked *reconstructed*.

### 7.1 `examples/05-family/accounts.ax` and `contracts.ax`: every association

| v4 declaration | v5 | what became typed or inferred |
|---|---|---|
| `account joint-checking : deposit at chase`<br>`  owner family` | `entity chase : bank`<br>`  family/checking : deposit` | custodian = nesting; owner = path word; "joint" is derived (`family.members` has two) |
| `account joint-savings : deposit at chase`<br>`  owner family` | `  family/savings : deposit` | the same. v5 adds the term v4 typed monthly: `interest 3.95% on the daily balance, paid monthly` (hh-c05, A7) |
| `account alex-401k : 401k at fidelity`<br>`  employer acme` | `entity fidelity : broker`<br>`  alex/401k` | owner = path word; custodian = nesting; **sponsor = acme by default** (Alex's one employer on the day) |
| `account jordan-401k : 401k at fidelity`<br>`  owner jordan`<br>`  employer bluefin` | `  jordan/bluefin/401k` | owner and sponsor are path words placed by type: `bluefin` fits `sponsor` only, `jordan` fits `owner` only |
| `account hsa : hsa at fidelity`<br>`  owner me`<br>`  coverage family` | `  alex/hsa` | coverage = the tier of the HDHP Alex joined (`joins hdhp`), an enum, not a word; an HSA with no HDHP behind it is an error |
| `account dcfsa : dependent-care-fsa at acme`<br>`  owner family` | `entity benefit-admin : plan-administrator`<br>`  alex/dcfsa`<br>`    dependents riley` | **owner is Alex**, not the family (the v4 file was wrong and could not know). sponsor = acme by default; `dependents` is a typed `person some` |
| `account riley-529 : 529-plan at fidelity`<br>`  owner family`<br>`  beneficiary riley` | `  family/529 riley`<br>or `  family/529`<br>`    beneficiary riley` | `family` can only be the owner, so `riley` can only be the beneficiary: a bare word works. `beneficiary acme` is `wrong-kind` |
| `account escrow : escrow at lender`<br>`  owner family` | *(nothing)*; `rocket/escrow` is a part of the mortgage | the escrow is the mortgage's, not a stranger; owner and lender are the mortgage's |
| `account mortgage : mortgage at lender`<br>`  owner family`<br>**plus** `contract mortgage-payment with lender`<br>`  loan 406_692.02 USD … for house`<br>**plus** `code mortgage-* on mortgage` | `entity rocket : lender`<br>`  family/mortgage : mortgage 406_692.02 USD on 2025-12-31 at 5.875% over 27y6m for house`<br>`    monthly on 1 from checking`<br>`    also -> escrow 705.00 USD #escrow` | **one debt, not three**: the position, its promise and its code pattern were one fact (PROPOSAL 6.2); `for house` is a typed slot (`property home`) |
| `account car-loan : loan at honda-finance`<br>`  owner family`<br>**plus** `contract car-payment … for crv` | `entity honda-finance : lender`<br>`  family/car-loan : loan 16_976.91 USD on … for crv`<br>`    monthly on 5 from checking` | the same |
| `account card : credit-card at chase`<br>`  owner family` | `  family/card : credit-card`<br>`    in full monthly on 25 from checking` | the forecast sees the bill (PROPOSAL 6.2) |
| `asset house : home`<br>`  owner family` | unchanged | `owner` is typed (`person | household`) |
| `entity chase : org`, `entity fidelity : org` | `entity chase : bank`, `entity fidelity : broker` | **the cost**: custodians need real kinds, because `custodian broker` does not accept `org` |
| `code 529-* on riley-529` | `code 529-* on 529` | by kind, not by the name that encoded the owner |
| `contract alex-pay with acme`<br>`  alex-401k 575 USD #household-deferral`<br>`  hsa 250 USD`<br>`  blue-shield 212.50 USD #pretax-benefit`<br>`  dcfsa 208.33 USD`<br>`  irs 692 USD #federal-tax` … 11 legs<br>`  also acme -> alex-401k 40% of ([alex-401k] up to 10% of amount)` | `contract alex-pay : employment with acme`<br>`  employee alex`<br>`  5_750.00 USD semi-monthly on 15, last into family/checking`<br>`  joins 401k-plan  deferral 10%`<br>`  joins hdhp`<br>`  joins dcfsa  5_000.00 USD yearly`<br>`  -> alex/hsa 250.00 USD` | the withholding, the deferral, the match and the plan's legs are the kinds'; the paycheck is one line in the journal |
| `contract riley-tuition with st-annes-school`<br>`  4_800 USD yearly on 01-10 from riley-529 #education` | `10 family/529 -> st-annes-school 4_800.00 USD #education` | a qualified withdrawal because the 529's beneficiary's school is the payee: read from the slots |

`axiom.ax` (`entity me : person / member family`, `entity jordan / member family`, `entity riley / member family`, and
`family: children 1`):

```text
// v4                                    // v5
entity family : household                entity family : household
  filing   joint                           members     alex, jordan
  children 1                               dependents  riley                // children: derived, on the day
  lives    us/ca                           filing      joint
entity me : person                         lives       us/ca
  born   1988-02-10                      entity alex : person                 // `me`
  member family                            born 1988-02-10
entity jordan : person                   entity jordan : person
  born   1987-06-21                        born 1987-06-21
  member family                          entity riley : person
entity riley : person                      born 2019-08-22
  born   2019-08-22
  member family
```

Count: the v4 `account` declarations are 11 accounts and 25 lines; their relations are 11 `at`, 10 `owner`, 4 free lines. After:
11 positions on 11 lines under 6 institutions, plus 3 role lines the kinds cannot infer (`beneficiary`, `dependents`, and
Jordan's sponsor in the path). **Written relations: 25 -> 3.** The other 22 are placed by the nesting, the path and the
defaults, and checked.

### 7.2 The three the user named, side by side

```text
// v4                                        // v5
account jordan-401k : 401k at fidelity      entity fidelity : broker
  owner jordan                                jordan/bluefin/401k
  employer bluefin                            alex/hsa
account hsa : hsa at fidelity                 family/529 riley
  owner me
  coverage family
account riley-529 : 529-plan at fidelity
  owner family
  beneficiary riley
```

What the second form knows that the first did not: `bluefin` is an employer and the 401(k)'s sponsor; `riley` is a person and the
529's beneficiary; `family` is a household and the owner; `alex/hsa`'s coverage comes from an HDHP; and a plan opened under
the wrong kind, or with a missing or doubled role, is an error that names the line.

### 7.3 Small business: Lantern Row (`ledgers/small-business.ax`)

*Reconstructed* v4 for three of the four people who defer into the plan (the officer, two employees; the third employee
declines):

```text
// v4
entity lantern-row : business
  owner dana 60%, theo 40%
account dana-401k : 401k at guideline
  owner dana
  employer lantern-row
account marisol-401k : 401k at guideline
  owner marisol
  employer lantern-row
account jonah-401k : 401k at guideline
  owner jonah
  employer lantern-row
contract dana-pay with lantern-row
  2_750.00 USD semi-monthly in arrears paid 5, 20 #wages
  dana-401k 275.00 USD #household-deferral                          // 10%, hand-computed every time the pay changes
  irs 206.17 USD #federal-tax …                                     // ~8 withholding legs
  also lantern-row -> dana-401k 100% of ([dana-401k] up to 3%) #contribution
  also lantern-row -> dana-401k 50% of ([dana-401k] from 3% up to 5%) #contribution     // the tiers, repeated per employee
  also lantern-row -> irs 6.2% of amount #payroll-tax …             // the employer's FICA, FUTA, SUTA: per employee
// … and the same for marisol, jonah
```

```text
// v5, as written in ledgers/small-business.ax
entity lantern-row : s-corp
  owners  dana 60%, theo 40%
  401k-plan                                   // named by its kind; sponsor = lantern-row by the nesting
    custodian guideline
    match 100% to 3%, 50% to 5%
contract dana-pay : employment
  employee dana                               // dana is also an owner: two relators between the same two agents
  employer lantern-row
  2_750.00 USD semi-monthly in arrears paid 5, 20
  deferral 10%                                // the plan is inferred: lantern-row offers exactly one plan that takes a deferral
```

Three inferences are visible. `deferral 10%` is a **membership attribute word**: of the plans the employer offers, exactly one
declares `deferral`, so the line is `joins 401k-plan deferral 10%` (the same unit propagation as 4.3, run on attribute words);
with two plans taking a deferral it is `ambiguous-role`. The ledger posts the deferral legs to `guideline` and does not keep each participant's balance; with memberships each would be a
`dana/401k` opened by the membership's `part account : 401k` on the first deferral, and nothing is declared by hand. And the tiers are *a value of the plan*
(`match 100% to 3%, 50% to 5%`), written once. In v4 each employee's contract repeated the tier arithmetic.

*Estimate, not a measurement:* v4 needs about 3 lines (account) plus 14 lines (contract) per deferring employee; v5 needs 5 and
an employee with no plan needs 4.

### 7.4 Freelancer and expat: relators with two sides

```text
contract halcyon-retainer : engagement          // ledgers/freelancer.ax
  provider noor-consulting
  client halcyon
  4_500.00 USD monthly on 1 invoiced due 15d
  includes 30 HR
  overage 150.00 USD/HR
```

v4 treats a retainer as a recurring receipt: a claim exists only once a due day is missed (LANGUAGE §7), so until then `claims` and
`available` do not see it; `invoiced` was proposed in FINDINGS and is not in the spec. An `engagement` is a relator whose `client` is the
debtor of the invoice it issues on the 1st, so the claim exists from the day it is issued and `Due` blames the client (fl-c01, A3).

### 7.5 Landlord: Marta's triplex (`ledgers/landlord.ax`)

*Reconstructed* v4:

```text
// v4
entity elena : tenant                                    // `tenant : org`: a person is not an org
account trust : trust-account at elm-pm
  owner marta
contract lease-b with elena
  1_850.00 USD monthly on 1 into trust #rent of unit-b
  from 2025-07-01 until 2026-06-30
  also trust -> elm-pm 8% of amount #management          // the manager's fee, retyped on every lease
  share 100% for unit-b                                  // which unit's rent it is, as a share
// the deposit: a claim typed by hand, with no link to the lease that created it
entity elm-pm : property-manager
```

```text
// v5
entity elm-pm : property-manager
  marta/trust : trust-account               // custody with the manager: marta's money and the tenants' deposits
contract elm-agreement : management
  owner marta
  manager elm-pm
  scope unit-b, unit-c                      // many-slot of units
  fee 8% of rent #management                // the manager's term: once, for every lease in scope
contract lease-b : lease
  tenant elena
  unit unit-b                               // landlord = unit-b's owner and agent = the manager: both by default
  1_850.00 USD monthly on 1 into trust #rent of unit
  deposit 1_850.00 USD                      // the claim, in the manager's trust, tied for elena
  from 2025-07-01 until 2026-06-30
```

Typed here: `tenant` is a person (the v4 kind said org), `unit` is an asset and its `owner` is the landlord, the fee is the
`management`'s and not the lease's, and the deposit is a slot of the lease whose settlement is the lease's `on end` law. A
vacancy is the absence of a lease relator on a unit: `any lease where unit is self` is a query over the typed slot (ll-c02, A12).

---

## 8. Errors

Every association error is an error at the declaration, never a silent default. Message shapes follow `Diagnostic::error` in the
repository (code, one-line message, span, help).

```text
error[ambiguous-role]: `riley` can fill two roles of this 529: `owner` and `beneficiary`
   --> accounts.ax:4   riley/529
    = note: the household `family` would fill only `owner`; a person fills either
    = help: write the role: `beneficiary riley` or `owner riley`

error[ambiguous-address]: `fidelity/401k` matches two positions on 2026-08-01
    = alex/acme/fidelity/401k
    = jordan/bluefin/fidelity/401k
    = help: `alex/401k` or `jordan/401k`

error[wrong-kind]: `acme` cannot fill a slot of 529 `family/529`
    = acme is an employer; `owner` takes person | household, `beneficiary` takes person
    = help: did you mean `beneficiary riley`?

error[missing-role]: `family/529` has no `beneficiary` (a 529 has exactly one)
    = help: `beneficiary riley` (the only person in `family.dependents`)

error[too-many]: `family/529` has two `beneficiary`: riley, jordan
    = help: a 529 has one: open a second plan, or keep `beneficiary riley`

error[unknown-slot]: a 401k has no `beneficiary`
    = a 401k has: owner, sponsor, custodian, plan
    = did you mean `owner`?

error[unfilled-default]: `jordan/401k`'s `sponsor` found two employers on 2026-06-01: bluefin, pinnacle
    = help: `jordan/bluefin/401k` or `sponsor bluefin`

error[unknown-name]: `employer blufin`: no entity of that name  = did you mean `bluefin`?

error[slot-widening]: `ira` widens `owner` of `tax-deferred` from person to person | household
    = help: a subkind may only narrow a range

warning[name-carries-slot]: `jordan-401k` ends in the kind `401k` and begins with the owner `jordan`
    = help: `jordan/401k`

warning[former-employer]: jordan/bluefin/401k has no employment behind it since 05-29: no match, no new deferrals
    = note: this is what a former-employer account is; it is not an error
```

The last warning is a legitimate state, not a fault. A slot whose filler's relator has ended is **not** an error: the sponsor is
still bluefin, there is just no live employment, and the kind's laws read that.

---

## 9. What changes in the kernels, and the order to build it

### 9.1 Type sketch (PROPOSAL §5)

```rust
// K1 Taxonomy: a kind carries slots, parts, verbs; every other field of v4's Kind stays.
pub struct Kind {
    pub parents: SmallVec<[Id<Kind>; 1]>,
    pub slots:   Vec<Slot>,           // own slots; inherited ones are read through `parents`
    pub parts:   Vec<Part>,           // `part escrow : escrow optional`
    pub verbs:   Vec<Verb>,           // sentence patterns: sugar
    pub props:   Props,               // purpose, pays, basis, deferred …: unchanged
}
pub struct Slot {
    pub name:    Sym,
    pub range:   Range,
    pub mult:    Mult,                // One | Optional | Some | Many | Between(u8, u8)
    pub prep:    Option<Prep>,        // With | For: the preposition that fills it
    pub weight:  Option<Weight>,      // by share | by rent USD | by area SQFT
    pub default: Option<Expr>,        // K6's Expr: evaluated once, on the day the thing begins
}
pub enum Range { Kinds(SmallVec<[Id<Kind>; 2]>), Names(Box<[Sym]>), Value(Ty) }   // `Ty::Entity` is deleted

// K2 Behaviours: a fill is one Timeline per (thing, slot); `now` restates it from a day.
pub enum Fill { None, One(Ref), Many(Vec<(Ref, Weight)>) }
pub enum Ref  { Entity(Id<Entity>), Position(Id<Position>), Asset(Id<Asset>), Contract(Id<Contract>) }
pub type SlotTimeline = Timeline<Fill>;

// K3 Positions: `owner` and `with` stay as fields (hot in the fold); every other slot is read from the timeline.
pub struct Position {
    pub owner: Id<Entity>,  pub with: Id<Entity>,  pub name: Option<Sym>,
    pub kind: Id<Kind>,     pub class: Class,
    pub slots: SlotMap,                // Sym -> SlotTimeline, only the kind's own, small
}

// Elaboration: the resolver. Pure; the same function checks, infers, and builds the canonical address.
pub fn fill(kind: &Kind, said: &[(Sym, Word)], words: &[Word], scope: &Scope, day: Day)
    -> Result<Fills, Vec<Diag>>;     // unit propagation (4.3), then defaults (4.4), then counts (3.1)
pub fn resolve(addr: &[Word], day: Day, book: &Book) -> Resolved;     // subsequence match over positions open on `day` (6.1, 6.3)

// K6 Norms: one more trigger beside on flow/in/out/gain: the end (and start) of a relator's span.
pub enum Trigger { Flow, In, Out, Gain, Start(Id<Kind>), End(Id<Kind>) }
```

Lines: K1 +~250 (slots, narrowing, parts, the enum range), K2 +~80 (the slot timelines reuse `Timeline<T>`), K3 +~150 (`fill`,
`resolve`), K6 +~60 (the triggers). Total about 540, against the 700 lines K1 removes from the v4 `kinds`/`paths` code and the ~120
lines of the `account … at` lowering the junction grammar already deletes. **Unsure:** these line counts are estimates by
analogy with the K1 budget, not measured.

### 9.2 Layered adoption (each layer is shippable alone)

| layer | what | closes | removes |
|---|---|---|---|
| **L0** | typed slots: `has NAME RANGE [MULT]`, the checks of section 8, `Ty::Entity` deleted. Existing free lines still parse; they are checked against the kind. | failures 2 and 4 | `entity`/`name` as slot types |
| **L1** | nesting + path + unit propagation + subsequence addressing + day-aware resolution | failure 1 and the "easy" half of 3 | `account … at`, `owner` lines, names that encode relations, `member`, `children` |
| **L2** | defaults, `part`, `joins`/memberships, `membership` property, `by` weights | the "precise" half of 3, 5; hh-c04/c03 start | per-employee legs and tiers, hand-opened escrows, `part of`, the two weight syntaxes |
| **L3** | `on start`/`on end` triggers; `verb` sentences | hh-c03, hh-b06, ll-b03, sh-b02 | hand-typed final pay, forfeiture and deposit settlement |

L0 + L1 answer the user's complaint and are a single K1/K3 change plus a resolver. L2 is where the line savings are. L3 is
optional and can wait for K6.

### 9.3 What each new word removes (the keyword rule)

| new | removes |
|---|---|
| `has … some/many/optional` | nothing: it extends `has`; no new keyword. |
| `part` | the v4 `part of ASSET` property **and** hand-declared companion accounts. |
| `joins` | the `plan` property, the per-contract deferral/match legs, and the participant's hand-opened account. |
| `by WEIGHT` | `owner … SHARE` **and** `share SHARE for ENTITY`: two syntaxes become one. |
| `as with` / `as for` | v4's separate `at PARTY` (accounts) and `with PARTY` (contracts), and `for ASSET` on loans. |
| `on start` / `on end` | hand-typed lifecycle lines; **one trigger kind**, no keyword if `on` is reused. |
| `verb` | **nothing yet** unless it absorbs the fixed statement list: so it ships last, and may not ship. |
| `one of A | B` | the `name` type with its members unstated. |

---

## 10. Costs, risks, and what I am not sure of

1. **Institutions need kinds.** `entity fidelity : org` stops being enough; `bank`, `broker`, `lender`, `plan-administrator` are
   needed. 41 `entity` lines in `05-family/accounts.ax`, about 6 of them custodians. `axiom fix` can add them.
2. **A bare address is stable only until a second sibling opens** (6.3). Day-aware resolution contains the damage; it does not
   remove it. A report that prints the canonical address is immune.
3. **Defaults are guesses about the past** (4.4). The error and the role word are the answer; a book that starts mid-life needs
   explicit sponsors. `hh-u11`.
4. **Unit propagation has an edge:** when two slots admit the same kind (a lease with `tenant person` and `guarantor person`) *and*
   both words are people, the bare form is ambiguous forever. That is correct and the role word is the fix, but it means
   relators with same-kind slots are never fully "easy". I did not look for a better rule; I believe there is none that does
   not guess.
5. **Roles as kinds are still one level.** `entity oakcraft : vendor` and later a client is `sb-u08` (A12): a thing has one
   declared kind in this design too. The extension (kinds are a *set*, and a role kind may be derived from a relator
   on the day) is in REPORT.md A12; this document does not need it and does not depend on it.
6. **`membership` and the `joins` sketch in 5.1 are the least settled part.** The kinds read well on the household and the small
   business; whether a membership should be a stored thing or an on-demand projection of (employment, plan) was not decided by
   running it. **Unsure.**
7. **The HSA default reads only the owner's own HDHP.** Family coverage through the spouse's plan is the same fact from another
   employment (`hh-u07`).
8. **Nothing here was run.** No toolchain reads these files. The ledgers' examples were checked by hand and by arithmetic scripts
   for balances, not by a parser.
