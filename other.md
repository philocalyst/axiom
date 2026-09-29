# The missing architecture: content-addressed inductive forms

You are pointing at the right missing layer.

The previous design had a strong semantic kernel, but it still lacked a convincing answer to three practical questions:

1. How does an ordinary person keep writing something almost indistinguishable from Beancount?
2. How does the community define invoices, card payments, options, VAT treatments, tax forms, payroll rules, and forecasting contracts without modifying the core?
3. How can Revo remain genuinely first-class without making a young, general-purpose scripting runtime part of the trusted accounting semantics?

The answer should not be “more built-in directives,” “plugins with better schemas,” or “put a static type system inside Revo.”

The stronger design is:

> **A Beancount-compatible surface gradually elaborates into content-addressed inductive forms.**
>
> Each form is simultaneously a type, an authoring surface, an importer target, a semantic elaboration, a query schema, a family of accounting/forecast/tax interpretations, and a versioned migration boundary.
>
> Revo is the first-class language around that core: ingestion, matching, querying, reports, orchestration, experimentation, and package generation—but it cannot fabricate accepted accounting truth.

This gives the system one extension mechanism rather than a plugin zoo.

---

# 1. The research threads converge unusually well

Beancount’s own design history has already identified many of the necessary ingredients independently:

* its parsed, incomplete representation and final booked representation need genuinely distinct types;
* plugins sometimes need to operate before booking because partial input cannot be expressed cleanly after booking;
* rewriting source should be first-class and preserve source structure;
* the query engine should be generalized and separated from the accounting core;
* settlement dates require splitting or “unrooting” transactions;
* budgets are more naturally constraints than fake double-entry accounts;
* custom directives need registered, consistent types rather than bags of unchecked tokens. ([Beancount][1])

The problem is that those ideas remain separate facilities: parser types, plugins, custom directives, importers, query systems, reports, metadata conventions, and booking rules.

The type-theory frontier suggests a way to unify them.

Elixir 1.20 now infers set-theoretic types from ordinary unannotated programs, using unions, intersections, negation, structural map information, guard narrowing, and a `dynamic()` type that can become more precise rather than merely turning checking off. That is an excellent model for the *authoring experience*: users should get progressively stronger guarantees without being forced to write academic type signatures. ([The Elixir programming language][2])

CUE contributes a second crucial idea: types and values can inhabit one lattice, and independently supplied constraints can be unified in an associative, commutative, idempotent, order-independent way. That is exactly what an accounting importer needs when a PDF, JSON API, bank statement, handwritten annotation, and jurisdictional rule all contribute knowledge about the same thing. ([CUE][3])

Inductive-family research contributes the third piece. “Trees That Grow” separates the stable shape of a tree from phase-specific information added during parsing, typing, and elaboration. Ornaments formalize the relationship between a simple inductive type and a richer one, including a lawful forgetful projection back to the simpler type and opportunities to transport generic functions to the richer type. ([arXiv][4])

That relationship is almost exactly what is needed between:

```text
ordinary Beancount transaction
        ↑
generic settlement
        ↑
Stripe settlement
        ↑
Stripe settlement supported by API + PDF + bank evidence
```

Unison contributes the right identity model: definitions are identified by hashes of their normalized syntax and dependencies rather than names alone, allowing incompatible versions to coexist and making dependency identity precise. ([Unison][5])

Finally, Revo is a promising shell for this because it already has pipes, pattern matching, compile-time facilities, procedural macros, an LSP, structural table types with open subtyping and narrowing, ambient declaration files, and C/Zig extension APIs. But it still describes itself as rapidly changing and dynamically typed, and its embedding API remains unfinished. It is therefore appropriate as the flexible query and integration language—not yet as the trusted definition of financial correctness. ([GitHub][6])

---

# 2. One extension primitive: the **form**

A form is not merely a record schema.

It is a content-addressed, bidirectional, inductively defined financial abstraction.

A form may define:

* constructors and recursive structure;
* fields, dimensions, units, and refinements;
* a constrained compact syntax;
* how ordinary postings can be recognized as that form;
* how the form elaborates into the small semantic kernel;
* which generic concepts it satisfies;
* how it projects into books, budgets, forecasts, tax systems, and documents;
* how importers construct observations of it;
* which query relations are generated;
* how edits to a rendered view translate back into proposals;
* how values migrate between versions;
* tests, examples, legal sources, and conformance fixtures.

The important simplification is:

> There is no separate extension mechanism for directives, import schemas, domain objects, query schemas, reports, and migrations.

They are facets of one form definition.

## Three ways forms compose

Forms need only three composition mechanisms.

### Sum: alternatives

```text
PaymentRail =
    Card
  | ACH
  | Wire
  | Cash
  | Crypto
```

### Product: independent facets

```text
Stripe.Settlement
& ImportedFromAPI
& CorroboratedByBank
& RelevantToUS1099K
```

### Ornament: richer information preserving a simpler meaning

```text
Stripe.Settlement
    refines Payment.Settlement
    refines Economic.Exchange
```

An ornament has a lawful forgetful projection:

```text
forget : Stripe.Settlement[c] -> Payment.Settlement[c]
```

That one relation gives enormous reuse.

A generic cash-flow report written for `Payment.Settlement` automatically works for Stripe.

A generic double-entry interpreter written for `Economic.Exchange` automatically works for Stripe.

A richer imported Stripe value can always be rendered as an ordinary Beancount transaction.

A user can leave the rich ecosystem and export plain postings without losing the financial result.

The richer type does not compete with the simpler type. It decorates and refines it.

---

# 3. The type system: inductive set types

I would not use a conventional ML type system, a TypeScript-like structural checker, or full dependent type theory.

The useful center is a deliberately constrained combination:

```text
set-theoretic gradual types
+ inductive families
+ row-polymorphic records and variants
+ nominal content-addressed identity
+ decidable financial refinements
+ phase-indexed values
```

Call these **inductive set types**.

## 3.1 Set-theoretic surface inference

At the authoring boundary, types behave like sets of possible values:

```text
T | U       union
T & U       intersection
not T       negation
bottom      impossible
top         unconstrained
dynamic(T)  runtime/source-bound value known to remain within T
```

A plain transaction might initially have type:

```text
Transaction
```

The accounts and quantities could refine it to:

```text
Transaction
& CashMovement
& ClearingReduction
& ProcessingExpense
```

A source reference such as:

```text
stripe_id: "set_91"
```

could narrow it further:

```text
Transaction
& dynamic(Stripe.Event)
```

When the necessary fields and equations are satisfied:

```text
Transaction
& Stripe.Settlement[USD]
```

The user need not write that type. The editor can display it as a ghost annotation.

This follows Elixir’s most important UX lesson: sophisticated types are useful precisely when programmers do **not** have to think about intersections while doing ordinary work. ([The Elixir programming language][7])

## 3.2 Inductive families

Community packages need more structure than structural records can provide.

For example:

```text
family Stripe.Event[currency] do
  InvoiceCreated
  PaymentIntentCreated
  Authorized
  Captured
  SettlementPaid
  RefundIssued
  DisputeOpened
  DisputeResolved
end
```

Or a contract calculus:

```text
family Contract[value] do
  Zero
  Transfer
  Both
  Scale
  At
  When
  Until
  Choice
  Repeat
end
```

Pattern matching over these families is exhaustive. Recursive values are well founded or, for recurring contracts, checked for productivity.

The ordinary ledger author never sees recursion proofs or type indices. Those are package-author concerns.

## 3.3 Open rows without globally open chaos

Forms need extensible records:

```text
{
  id: Stripe.Id,
  gross: Money[USD],
  net: Money[USD],
  ...extension
}
```

They also need extensible variants for community-defined data.

But globally open constructors would destroy exhaustiveness and create incoherent interpretation rules.

The safer rule is:

* an inductive family’s constructors are sealed by its content-addressed definition;
* independent extensions use open **facets**;
* richer domain types use ornaments;
* behavior is added through explicit interpretation packages;
* “orphan” interpretations do not silently enter global scope.

Recent work combining row-polymorphic extensible data types with ad-hoc polymorphism is directly relevant here: it demonstrates that extensible records/variants and generic interpretation can coexist without reducing everything to untyped maps. ([ACM Digital Library][8])

## 3.4 Refinements, but not arbitrary dependent programming

The type system should understand:

```text
gross = net + fee
amount > 0
start <= end
currency(amount) = currency(account)
captured <= authorized
filled + canceled <= order_quantity
sum(allocations) <= payment_amount
```

It should not attempt to prove arbitrary user programs.

The decidable refinement fragment should include:

* exact decimal and rational arithmetic;
* linear equalities and inequalities;
* dimensions and unit equations;
* currency and instrument compatibility;
* finite enums and sets;
* date and interval arithmetic;
* cardinalities and locally bounded aggregates;
* constructor membership and stage constraints.

Rules involving recursive financial history, statutes, or complex classification belong in the rule engine rather than the typechecker.

This separation keeps errors comprehensible and compilation predictable.

## 3.5 Unknown is not optional

A serious financial type system must distinguish:

```text
Option[T]      the value is known to be present or absent
?x : T         a required value is not yet known
dynamic(T)     an external/runtime value constrained only to T so far
Choice[T]      several materially distinct valid resolutions remain
Conflict[T]    no resolution satisfies the current constraints
```

These are radically different states.

A missing fee is not zero.

A payee absent from a bank export is not known to be “no payee.”

Two candidate invoices are not an optional invoice.

A PDF/API disagreement is not `dynamic()`.

Most importer pain comes from collapsing these states into `None`, `FIXME`, placeholder accounts, or arbitrary defaults.

## 3.6 Phase-indexed values

Imported and authored values move through a controlled family:

```text
Raw[T]
  -> Observed[T]
  -> Resolved[T]
  -> Accepted[T]
  -> Recognized[book, T]
```

### `Raw[T]`

Bytes, source locations, JSON pointers, PDF regions, CSV cells.

### `Observed[T]`

A source or script says that a value has some partially known shape.

### `Resolved[T]`

Material holes and identity questions have been solved sufficiently for its intended use.

### `Accepted[T]`

A user or declared policy has admitted it into the canonical economic theory.

### `Recognized[book, T]`

A specific book interprets it under pinned rules.

The tree can gain phase-specific data without every record becoming a swamp of optional fields. This is where the “Trees That Grow” pattern is particularly useful. ([arXiv][4])

A Revo importer may construct `Observed[Stripe.Invoice]`.

It cannot construct `Accepted[Stripe.Invoice]`.

Only the trusted kernel can do that after validating proofs, decisions, and policy.

---

# 4. The semantic kernel remains tiny

Forms compile into a much smaller kernel than their surface vocabulary suggests.

The central type is a judgment:

```text
Judgment[mode, proposition]
```

The modes are approximately:

```text
Observed[source]
Actual
Required
Permitted[actor]
Assumed[scenario]
Recognized[book]
```

Examples:

```text
Observed[Stripe](Settlement(...))

Actual(Flow(Stripe, Acme, 583 JPM.USD))

Required(
  Flow(Bob, Acme, 400 USD),
  due = 2026-09-27
)

Permitted[
  TechnologyDepartment
](
  Incur(QualifyingTechnologyObligation),
  up_to = 100_000 real_EUR_2026
)

Assumed[
  downside
](
  EURUSD = curve(...)
)

Recognized[
  IFRS
](
  Expense(ProcessingFee, 17 USD)
)
```

The proposition payload is an inductive typed value.

The form layer keeps users from writing these low-level terms.

## The key boundary

```text
forms describe domain meaning
judgments describe semantic status
interpretations produce views
```

A Stripe package defines what a settlement means.

A US-tax package decides what consequences that meaning has under a US-tax context.

An IFRS package decides how to recognize it.

A budget package decides whether it consumes authority.

A forecasting package decides how it contributes to projected cash.

None of those packages rewrites the original Stripe value.

---

# 5. The default authoring surface stays Beancount

This must not become a language where entering lunch requires understanding GADTs.

A completely ordinary Beancount transaction remains valid:

```beancount
2026-09-22 * "Stripe" "Settlement set_91"
    Assets:Bank:JPM              583 USD
    Expenses:ProcessingFees       17 USD
    Assets:StripeClearing       -600 USD
```

Its inferred type can remain merely:

```text
Transaction
```

The system still balances it and reports it.

There is no mandatory migration cliff.

## First progressive enhancement: one type ascription

```beancount
2026-09-22 * "Stripe" "Settlement set_91" :: Stripe.Settlement
    Assets:Bank:JPM              583 USD
    Expenses:ProcessingFees       ?fee
    Assets:StripeClearing       -600 USD

    stripe_id: "set_91"
```

The form supplies:

```text
gross = net + fee
```

Therefore:

```text
?fee = 17 USD
```

The form also validates that:

* the net destination is deposit-like;
* the gross source is clearing-like;
* the fee account accepts processing expenses;
* the currencies agree;
* the settlement identity is well formed.

`?fee` is a typed metavariable, not a runtime null.

The editor can show:

```text
inferred ?fee = 17 USD
```

and offer:

```text
Materialize inferred value
Keep symbolic
Show proof
```

## Second enhancement: collapsed domain syntax

The same semantic value can be authored more compactly:

```text
2026-09-22 Stripe.Settlement "set_91"
  gross 600 USD from Assets:StripeClearing
  net   583 USD into Assets:Bank:JPM
  fee       ?   into Expenses:ProcessingFees
```

This uses a deliberately constrained universal grammar:

```text
DATE FORM-NAME PRIMARY-ARG*
  CLAUSE-NAME TERM*
  CLAUSE-NAME TERM*
```

Community packages cannot arbitrarily modify the lexer or install a new grammar.

They can define:

* available clause names;
* the typed positional shape of each clause;
* which clauses are required, optional, or repeated;
* rendering and alignment preferences.

That avoids the fragility Martin identified when custom directive names could not safely be admitted by the tokenizer, while still giving packages concise domain syntax. ([Google Groups][9])

## The two surfaces are lawful views

These are not two objects:

```text
posting transaction
domain settlement
```

They are two views of one typed value.

Commands can switch views:

```text
bean view --surface postings entry:set_91
bean view --surface form entry:set_91
bean view --surface proof entry:set_91
bean view --surface sources entry:set_91
```

A formatter may preserve the author’s chosen surface rather than always rewriting it.

Exporting to ordinary Beancount uses the form’s forgetful projection.

---

# 6. A form definition

A package-authoring language should be declarative, total, and smaller than Revo.

A proposed definition could look like this:

```text
concept Payment.Settlement[currency] do
  gross : Positive[Money[currency]]
  net   : NonNegative[Money[currency]]
  fee   : NonNegative[Money[currency]]

  require gross = net + fee
end
```

A Stripe refinement:

```text
form Stripe.Settlement[currency]
  refines Payment.Settlement[currency]
do
  id         : Stripe.Id
  settled_at : Instant

  gross : Positive[Money[currency]]
  net   : NonNegative[Money[currency]]
  fee   : NonNegative[Money[currency]] = gross - net

  clearing : Account[Clearing[currency]]
  bank     : Account[Deposit[currency]]
  expenses : Account[Expense]

  require net <= gross

  surface do
    gross $gross from $clearing
    net   $net   into $bank
    fee   $fee   into $expenses
  end

  forget do
    Payment.Settlement {
      gross = gross,
      net   = net,
      fee   = fee
    }
  end

  means do
    actual Flow(Stripe, entity, Deposit(bank), net)
    actual Flow(entity, Stripe, ProcessingService, fee)

    fulfills amount gross
      from Clearing(clearing)
  end
end
```

The precise syntax could improve through prototyping, but the division is important:

* `concept` defines structural semantic capability;
* `form` defines nominal inductive identity;
* `refines` provides a forgetful projection;
* `require` supplies decidable invariants;
* `surface` defines constrained authoring clauses;
* `means` elaborates into semantic judgments.

## Generated facilities

From that one definition, the compiler can derive:

```text
parser schema
pretty-printer
LSP completion
constructor API
Revo declarations
query relation
structural validator
JSON representation
canonical binary representation
generic transaction projection
basic migration skeleton
documentation table
fixture generator
```

No package author writes eight adapters.

---

# 7. Generic interpretations replace mutation plugins

A form does not generate one permanent set of postings.

Different contexts interpret the same form.

```text
interpret Book.IFRS
  for Payment.Settlement[currency]
do
  ...
end
```

```text
interpret CashForecast
  for Payment.Settlement[currency]
do
  ...
end
```

```text
interpret Tax.US
  for ProcessingFee[currency]
do
  ...
end
```

```text
interpret Budget.Actual
  for Payment.Settlement[currency]
do
  ...
end
```

This is closer to algebraic interpretation than a plugin pipeline.

The benefits are substantial:

* no plugin ordering;
* no repeated mutation of a directive stream;
* each interpretation declares its input and output;
* the same form can have many coexisting views;
* incremental dependencies are explicit;
* reports retain rule provenance;
* a package upgrade can identify exactly which interpretations changed.

Beancount’s current plugin model was deliberately simple and valuable, but its ordering and post-booking requirements make partial or richer input awkward. Vnext’s proposal for distinct pre-booking and post-booking types recognizes that pressure. Forms make the distinction structural rather than procedural. ([Beancount][1])

## Interpretation coherence

Interpretations must not be globally discovered by import order.

A book explicitly pins an interpretation set:

```text
book Acme.IFRS do
  use Core.IFRS
  use Finance.Payments.IFRS
  use Finance.Stripe.IFRS
  use Acme.Policy.IFRS

  entity Acme
  functional EUR
  present EUR
end
```

The compiler checks:

* overlapping rules;
* uncovered constructors;
* incompatible outputs;
* ambiguous defaults;
* dependency cycles;
* contradictory measurement policies.

A local package can deliberately resolve overlap:

```text
prefer Acme.Policy.IFRS
  over Finance.Stripe.IFRS
  for Stripe.ProcessingFee
  because policy "capitalize implementation fees"
```

That preference becomes a versioned decision, not an accidental import order.

---

# 8. Revo’s exact role

Revo should be deeply integrated, but the system should draw a hard line around what Revo is trusted to do.

## Revo is first-class for

* import detection and decoding;
* external API adapters;
* fuzzy matching and proposal generation;
* orchestration;
* ad hoc analysis;
* typed queries;
* report layout;
* visualizations;
* simulations;
* package build generators;
* migration assistance;
* interactive review tools.

## Revo is not the authority for

* constructing accepted facts without validation;
* bypassing form invariants;
* defining close-critical semantics imperatively;
* silently choosing among ambiguous matches;
* mutating the canonical ledger;
* introducing an unpinned network result into a closed book;
* fabricating a `Proof[T]`.

This is not a demotion of Revo.

It is what allows Revo to remain pleasant and flexible.

## 8.1 Native extension boundary

The trusted engine exposes a C ABI.

A thin Zig extension provides Revo modules:

```text
bean
bean.query
bean.ingest
bean.report
bean.forms
bean.proof
```

The extension exposes opaque userdata such as:

```text
Snapshot
Form
Observed[T]
Proposal[T]
Query[T]
Frame[row]
Proof[T]
SourceSpan
Blob
Capability
```

Revo may receive and compose these values.

Only native constructors can create proof-bearing phases.

Generated `.d.rv` declaration files describe available forms and result rows. Revo’s current support for ambient declarations and structural table types makes this a good fit. ([GitHub][10])

## 8.2 Practical first implementation

Because Revo’s public embedding support is still marked unfinished, the first implementation should not block on embedding.

Use:

```text
bean CLI
  -> sandboxed Revo subprocess
  -> framed canonical messages over stdin/stdout
  -> native bean extension loaded by Revo
```

Later, contribute a configurable embedding/runtime API upstream.

This also gives a useful security boundary.

## 8.3 Capability-based scripts

An importer receives only the capabilities declared by its package:

```text
requires:
  blob.read
  json.decode
  pdf.text
  emit.observation[Stripe.Invoice]
```

A price fetcher might additionally request:

```text
network["api.example.com"]
clock
```

A tax filing renderer might receive:

```text
snapshot.read
proof.read
report.write
```

It receives no ledger mutation capability.

Since an ordinary Revo process may otherwise access filesystem and environment functionality, trusted execution must combine capability objects with an operating-system sandbox or a restricted runtime preset. Merely following a library convention would not be a security boundary.

## 8.4 Scripts emit proposals, not truth

A Revo matcher can emit:

```text
Proposal[
  Same(
    JPM#line443.movement,
    Stripe#settlement91.cash
  )
]
```

It may include:

```text
confidence
features
explanation
model version
script hash
input hashes
```

But confidence is not accounting truth.

A policy or user accepts the proposal, creating an explicit decision.

---

# 9. Revo as the query language

The native engine should provide a typed relational query IR.

Revo is the ergonomic language that constructs, composes, executes, and renders those queries.

It should not iterate over millions of records one dynamic object at a time.

## Proposed Revo query surface

A procedural macro could provide:

```revo
const overdue = query! do
  from i in Invoice

  let amount = residual(i)

  where i.due < $today
    and amount > zero(amount.currency)

  group { i.customer, amount.currency }

  select {
    customer = i.customer,
    currency = amount.currency,
    due      = sum(amount),
    invoices = collect(i.id),
  }
end
```

The macro compiles to native query IR and returns something conceptually typed as:

```text
Query[
  {
    customer: Party,
    currency: Currency,
    due: Money[currency],
    invoices: List[Invoice.Id]
  }
]
```

Revo’s structural tables are a natural result representation. Its current narrowing and open-subtyping work also means a row may carry additional provenance or display fields without breaking a consumer that only requires `customer` and `due`. ([GitHub][11])

Until the macro interface is sufficiently stable, a fluent API can provide the same functionality:

```revo
const overdue =
  bean:from(Invoice)
  |> bean:let(:amount, fn(i) residual(i) end)
  |> bean:where(fn(row)
       row.i.due < today and
       row.amount > zero(row.amount.currency)
     end)
  |> bean:group(fn(row)
       {row.i.customer, row.amount.currency}
     end)
  |> bean:select(...)
```

## Query contexts are explicit

```revo
overdue
|> bean:at(@2026-09-30)
|> bean:as_known(@2026-10-04T18:00Z)
|> bean:under(Acme.IFRS)
|> bean:run(snapshot)
```

Or:

```revo
bean:under(US.Tax[2026], fn() do
  ...
end)
```

The same query can execute against:

```text
actual history
a historical knowledge cutoff
a closed book
a forecast scenario
a tax jurisdiction
an alternate presentation currency
a real-purchasing-power frame
```

## Provenance is preserved

Queries should use annotated relational evaluation so output rows retain not only which facts contributed, but how they contributed. Provenance semirings provide a general foundation for this kind of relational “how-provenance.” ([ResearchGate][12])

Revo can then ask:

```revo
result
|> bean:why()
|> report:tree()
```

or:

```revo
bean:why_not(query, {
  customer = Bob,
  state = :paid,
})
```

The answer might be:

```text
Invoice INV-42 is not paid because:

  required       1,000 USD
  fulfilled        600 USD
  residual         400 USD

No accepted payment allocation covers the residual.

Unallocated candidate:
  JPM#line591, 400 USD
  candidate score 0.72
```

---

# 10. Dropping a Stripe invoice into a folder

This workflow should be almost boring.

```text
ledger/
  main.bean
  accounts.bean
  inbox/
    stripe/
      in_1ABC.pdf
      in_1ABC.json
  bean.lock
```

The user runs:

```text
bean ingest inbox/stripe
```

## Stage 1: immutable evidence

Every source file is hashed and stored or referenced by content:

```text
blob:#7e4a...
blob:#a901...
```

Exact bytes and semantic decoded values have separate identities.

Changing whitespace in a JSON export may change the blob identity without changing the normalized observation identity.

## Stage 2: detection

Registered Revo detectors receive blob metadata and a small prefix.

They propose:

```text
Stripe.InvoicePDF
Stripe.InvoiceJSON
Generic.InvoicePDF
```

Detection does not commit anything.

## Stage 3: decoding

The winning decoders emit partial observed forms:

```text
Observed[Stripe.Invoice] {
  id       = "in_1ABC",
  seller   = Stripe,
  buyer    = Acme,
  subtotal = 600 USD,
  tax      = 0 USD,
  total    = 600 USD,
  issued   = 2026-09-01,
  due      = 2026-09-15
}
```

Every field retains a source span:

```text
JSON pointer /data/object/amount_due
PDF page 1, rectangle (x1, y1, x2, y2)
CSV file, row 43, columns 6–8
API response object and revision
```

Clicking the amount in the review UI highlights its origin.

## Stage 4: order-independent unification

The PDF and JSON are combined by meet/unification:

```text
PDF observation
& JSON observation
& optional API observation
```

If all say `600 USD`, provenance strengthens.

If one says `600 USD` and one says `650 USD`, there is a conflict.

There is no “the JSON importer ran later, so it wins.”

The order-independent lattice model is one of the most valuable lessons to take from CUE. ([CUE][3])

## Stage 5: identity resolution

The engine proves or proposes:

```text
same PDF invoice and JSON invoice
seller "Stripe Payments Europe" = Party Stripe
buyer customer identifier cus_123 = Entity Acme
```

Exact source IDs can resolve automatically.

Fuzzy identity remains a proposal.

## Stage 6: form elaboration

`Stripe.Invoice` refines generic:

```text
Invoice
ExchangeCommitment
Payable
TaxDocumentCandidate
```

The form elaborates the invoice into:

```text
Stripe must provide described service
Acme must pay 600 USD by 2026-09-15
```

The relevant book decides when and how to recognize expense and payable.

## Stage 7: contextual rules

Configured contexts run:

```text
Acme.IFRS
Acme.Budget
Tax.US.Federal[2026]
Tax.California[2026]
```

Or for another entity:

```text
Book.LocalGAAP
Tax.DE[2026]
Tax.EU.VAT[2026]
```

The source invoice remains one neutral accepted form.

## Stage 8: review

The output should look like:

```text
Stripe invoice in_1ABC

✓ PDF and JSON joined by exact invoice ID
✓ seller, buyer, dates, currency, subtotal, tax, and total agree
✓ subtotal + tax = total
✓ supplier mapped to Party:Stripe
✓ payable terms are complete

? accounting destination

  1  Expenses:Software
  2  Assets:Prepaid:Software

Consequences:

  1  recognizes 600 USD expense on 2026-09-01
  2  recognizes prepaid asset and schedules amortization

Source evidence:
  in_1ABC.pdf, page 1
  in_1ABC.json, /data/object
```

The user chooses once.

That choice becomes a small durable decision:

```text
accept #in_1ABC.destination = Expenses:Software
```

## Stage 9: materialization

The canonical human form might be:

```text
2026-09-01 Stripe.Invoice "in_1ABC"
  seller Stripe
  buyer  Acme

  service "Platform fees — September"

  subtotal 600 USD
  tax        0 USD
  total    600 USD

  due 2026-09-15
  expense Expenses:Software

  source blob:#7e4a...
  source blob:#a901...
```

The IFRS book may render:

```beancount
2026-09-01 * "Stripe" "Invoice in_1ABC"
    Expenses:Software          600 USD
    Liabilities:Payable:Stripe -600 USD
```

No generated posting has to be stored as the primary truth.

## Idempotence

Running `bean ingest` again:

```text
✓ 2 source blobs already known
✓ 1 accepted invoice unchanged
✓ 0 new proposals
```

If Stripe publishes a revised invoice, the new revision does not silently overwrite the accepted one. The review shows the semantic difference and affected books, budgets, forecasts, and tax forms.

---

# 11. Forecasting becomes an interpretation of inductive contracts

This is where inductive types become much more than validation.

A contract is an inductive syntax tree.

A small internal calculus can use constructors analogous to:

```text
Zero
Transfer
Both
Scale
At
When
Until
Choose
Repeat
```

This follows the core insight of compositional financial-contract research: a relatively small combinator language can describe and value a broad class of financial contracts through compositional semantics. ([Microsoft][13])

A user should see domain forms, not those constructors.

```text
2026-01-01 Lease.Indexed "office"
  pay 5_000 EUR monthly to Landlord

  from 2026-01-31
  through 2028-12-31

  index EU.HICP
  lag 3 months
  reset yearly
  floor 0%
  cap 5%

  from Assets:Bank:N26
  expense Expenses:Rent
```

`Lease.Indexed` is an ornament of the generic contract calculus.

## One contract, many folds

Because the contract is inductive, different interpreters can fold over it:

```text
CashflowSchedule
AccrualBook
BudgetCommitment
LiquidityExposure
PresentValue
TaxDeductionSchedule
LeaseDisclosure
ScenarioSimulation
```

The constructors do not contain those outputs.

The interpreters do.

This is the strongest reason to use inductive forms rather than free-form directives.

## Actual versus forecast

Actual history contains:

```text
published index observations
issued invoices
accepted services
payments
settlements
```

A scenario contributes assumptions:

```text
scenario downside from actual @2026-09-30
  FX  use Treasury.DownsideCurve
  CPI use ECB.HighInflation
  sales multiply 0.82
  collections delay 14 days
```

The contract evaluated under that scenario creates projected obligations and cash flows.

It does not create fake actual transactions.

## Multiple uncertainty interpretations

The core contract does not need to mandate one probability model.

The same contract can be interpreted into:

```text
exact deterministic schedule
low/high interval
named scenario vector
probability distribution
Monte Carlo samples
stress-test lattice
```

Revo is particularly appropriate for custom scenario generation and simulation, while the native engine preserves the typed contract structure and units.

## Forecast reconciliation

When a real invoice arrives, the system can relate it to the projected contract occurrence:

```text
actual invoice fulfills projected September lease occurrence
```

Then forecast variance can be decomposed into:

```text
timing
quantity
local price
FX
inflation
scope
cancellation or renewal
classification
```

The user can ask why forecast and actual diverged without manually pairing rows.

---

# 12. Budgeting becomes one specialized interpretation

Budgets should not be a parallel accounting currency unless a particular budgeting method explicitly calls for that.

A hard organizational budget is a bounded permission:

```text
2026 Budget.Authority "Technology"
  allow TechnologyDepartment
    to incur qualifying obligations

  up to 100_000 EUR[EU.HICP @2026-01]

  during 2026

  count actual + committed

  breach hard
```

A soft planning budget is a target:

```text
2026 Budget.Target "Technology"
  prefer spending <= 100_000 EUR
```

A forecast is a scenario result.

These are different forms:

```text
Authority
Target
Forecast
```

Martin Blais’s observation that budget semantics are better expressed as constraints on account changes or balances than as artificial double-entry accounts points in the same direction. ([Google Groups][14])

Because the same economic forms feed the budget interpreter, there is no separate budget transaction-entry workflow.

The budget can choose to count:

```text
actual only
actual + legal commitments
actual + commitments + selected forecast
cash paid
recognized expenses
real-purchasing-power cost
functional-currency cost
```

Its measurement context is explicit.

---

# 13. International currency and inflation fit naturally

Forms and types should distinguish:

```text
USD                 unit of account
JPM.USD             deposit instrument issued by JPM
IBKR.USD            deposit instrument issued by IBKR
Invoice[USD]        obligation denominated in USD
Book[EUR]           functional or presentation context
EUR[EU.HICP@2026-01] purchasing-power measure
```

These are not interchangeable commodities merely because all can be represented by numbers.

## Measurement is an interpretation

A quote is an observed form:

```text
2026-09-20 FX.Quote
  1 USD ~= 0.9200 EUR

  source ECB
  kind reference
  observed 2026-09-20
  published 2026-09-20T16:00Z
```

Another quote may say:

```text
bid 0.9190
ask 0.9210
route JPM
available-to Acme
purpose settlement
```

A book, forecast, or valuation context selects:

* reference versus executable;
* bid, ask, midpoint, or policy rate;
* accessible market;
* transaction, closing, average, or historical date;
* staleness bounds;
* interpolation;
* triangulation;
* source precedence;
* knowledge-time vintage.

No global `price(currency, date)` function can silently make all those choices.

## Real purchasing power

```text
EUR[EU.HICP @2026-01]
```

is a measurement type, not a new tradable currency.

The complete type includes:

```text
nominal unit
index series
region or basket
reference period
vintage policy
publication cutoff
interpolation policy
```

That permits:

* real historical spending;
* indexed contracts;
* real portfolio returns;
* inflation-aware budgets;
* inflation scenarios;
* statutory hyperinflation accounting.

Those remain separate interpretations even though they share index observations.

---

# 14. Tax and jurisdiction packages

Tax systems are exactly where “just let users write plugins” breaks down.

A serious jurisdiction package needs:

```text
inductive definitions
effective dates
publication dates
dated parameters
defaults and exceptions
elections
precedence
source-law references
typed forms
explanation traces
conformance fixtures
property tests
migration rules
filing schemas
```

Catala is an important precedent because it treats statutory default-and-exception structure as first-class rather than flattening legislation into an arbitrary order of `if` statements. Its compiler work also demonstrates the value of connecting executable legal rules to formalized source structure, and CUTECat shows how concolic execution can explore large default-heavy legal programs and generate extensive test cases. ([arXiv][15])

## 14.1 A jurisdiction context

```text
context US.Federal[2026] do
  entity Acme
  tax-year 2026
  residence US
  filing-status ...
  accounting-method accrual

  use Tax.US.Core[2026]
  use Tax.US.Business[2026]
  use Tax.US.Securities[2026]
end
```

The context is itself content-addressed.

## 14.2 Rules are literate and dated

Illustrative syntax:

```text
rule BusinessExpense.Deductibility
  authority "statute-section-reference"
  effective [2026-01-01, 2027-01-01)
do
  default deductible(expense) = 100%
    when ordinary_and_necessary(expense)

  except deductible(expense) = 50%
    when meals(expense)

  except deductible(expense) = 0%
    when disallowed_entertainment(expense)
end
```

The actual legal package would carry precise references and interpretations.

The `default` construct is not an ordinary programming default.

It has explicit legal precedence and exception semantics.

Conflicting applicable rules produce a conflict or an interpretation choice, not “last module wins.”

## 14.3 Elections are explicit choices

```text
may Taxpayer choose
  election SpecificIdentification
  for disposition #sale77
  before election-deadline
```

The choice is durable and versioned.

It is not inferred opportunistically from whichever lots happen to remain.

## 14.4 Tax forms are inductive typed documents

```text
form US.Form8949[year] do
  pages : List[Form8949.Page]
  totals : Form8949.Totals
  ...
end
```

Each box is a query returning a proof-bearing value:

```text
box 1a =
  query disposals
  under US.Federal[2026]
  where category = short_term_reported_basis
  sum proceeds
```

The renderer can produce:

```text
PDF
XML/e-file payload
CSV workpaper
HTML explanation
```

Clicking a box reveals:

```text
economic events
basis rules
elections
FX measurements
adjustments
rule definitions
source-law references
```

## 14.5 Filing is a close

A filing pins:

```text
accepted-data root
decision root
jurisdiction package hashes
dated parameter versions
measurement policies
elections
form schema
rendered output hash
```

An amended return is a semantic diff between two proof contexts.

The system can distinguish:

```text
as filed
as known on filing date
as currently known
proposed amendment
```

## 14.6 Cross-jurisdiction composition

Economic facts remain jurisdiction-neutral.

Separate packages interpret them for:

```text
US federal income tax
California income tax
EU VAT
German corporate tax
treaty credits
transfer pricing
local statutory books
group consolidation
```

Treaties and cross-border packages explicitly combine two or more contexts and define:

```text
source rules
residence rules
credit ordering
withholding
currency translation
tie-breakers
effective periods
```

The core does not contain a universal `tax_category` field.

That would be structurally incapable of representing overlapping jurisdictions.

---

# 15. Content-addressed native versioning

Names are for humans.

Hashes are identity.

```text
finance/stripe
tax/us/federal/2026
book/ifrs
```

resolve through the workspace lock to content hashes:

```text
finance/stripe          = #j8m4...
finance/payment         = #k91c...
tax/us/federal/2026     = #u31a...
book/ifrs               = #f712...
```

A normalized form definition includes hashes of every referenced definition.

Changing a dependency therefore changes the resulting identity.

Recursive groups are hashed as strongly connected definition groups rather than requiring impossible self-hashes.

## 15.1 Values carry constructor identity

An accepted value records:

```text
constructor hash
field values
referenced value identities
source provenance
```

Not merely:

```text
type = "Stripe.Invoice"
version = "2.4.1"
```

Old and new definitions coexist.

A closed book may continue interpreting old values with old rules while current work uses new forms.

## 15.2 Semver is discovery metadata

A package may advertise:

```text
2.4.1
```

but correctness does not depend on trusting that label.

Semver helps humans discover compatible upgrades.

The lock pins hashes.

## 15.3 Three independent versions

Financial systems need to distinguish:

1. **Definition identity**
   Which exact form or rule implementation?

2. **Effective time**
   When does the legal or business rule apply?

3. **Knowledge/publication time**
   When was that rule, parameter, correction, or observation known?

Conflating these is a major source of retrospective-reporting errors.

A tax rate may apply from January, be enacted in March, and be encoded by package revision in April.

All three facts matter.

## 15.4 Structural compatibility versus nominal identity

Two forms may be structurally compatible without being semantically identical.

```text
Tax.US.Interest
Tax.CA.Interest
```

could have identical fields and different authorities.

Therefore:

* structural subtyping controls whether values can be read or transformed;
* nominal hashes preserve legal and semantic identity;
* explicit equivalence or migration proofs connect versions.

## 15.5 Ornaments reduce migration work

If a new version merely adds richer information while preserving a forgetful projection, it can declare itself an ornament of the old form.

Generic interpretations can often be transported automatically.

This is much stronger than rerunning imperative migration scripts over every historical value.

A true breaking change supplies:

```text
migrate OldHash -> NewHash
```

The migration is itself total, tested, and content-addressed.

## 15.6 Upgrade UX

```text
bean upgrade finance/stripe
```

should report:

```text
finance/stripe
  #old -> #new

Type changes:
  + settlement.available_on : Option[Date]
  fee currency narrowed to settlement currency
  dispute reason constructors expanded

Interpretation changes:
  Book.IFRS: none
  CashForecast: 12 projected availability dates changed
  Tax.US: none

Affected accepted values:
  43

Affected closed books:
  0

Migration:
  automatic ornament lift available
```

The user approves the namespace update.

No historical close changes silently.

---

# 16. The community repository

A package could contain:

```text
finance-stripe/
  package.bean

  forms/
    invoice.bean
    payment.bean
    settlement.bean
    refund.bean
    dispute.bean

  semantics/
    payment-relations.bean

  interpretations/
    cash.bean
    ifrs.bean
    us-gaap.bean
    forecast.bean

  ingest/
    stripe-json.rv
    stripe-pdf.rv
    stripe-api.rv

  queries/
    revenue.rv
    disputes.rv

  reports/
    reconciliation.rv

  migrations/
    settlement-v1-v2.bean

  fixtures/
    invoice-basic/
    settlement-fee/
    refund-partial/
    dispute-won/

  sources/
    stripe-schema-notes.md
```

## Package trust is multidimensional

“Installed” should not mean “fully trusted.”

The registry can publish attestations:

```text
build reproducible
form definitions total
all fixtures pass
migration round trips pass
jurisdiction review completed
reviewer signatures
authority conformance suite passes
security sandbox profile
```

A tax package might be labeled:

```text
experimental
community-reviewed
professionally-reviewed
authority-conformance-tested
```

The content hash says *what* the package is.

Attestations say *who has evaluated it and how*.

## No central bottleneck

A registry maps names to content-addressed roots.

Forks can coexist:

```text
community/tax-us
firm-a/tax-us
firm-b/tax-us
```

A workspace chooses and pins one interpretation set.

A regulator, accounting firm, or community group can issue signed attestations over existing package hashes without republishing them.

---

# 17. Implementation architecture

I would use a Rust trusted core with a stable C ABI and a thin Zig adapter for Revo.

Rust gives a strong ecosystem for parsers, exact data modeling, incremental computation, persistent structures, databases, and typed APIs. Revo does not need to care which implementation language owns the semantic kernel.

## 17.1 Compiler pipeline

```text
lossless source CST
    ↓
surface values and metavariables
    ↓
form candidate inference
    ↓
set-theoretic narrowing
    ↓
inductive form elaboration
    ↓
constraint solving
    ↓
semantic judgments
    ↓
accepted theory
    ↓
interpretation/query graph
    ↓
books, forecasts, forms, reports
```

Every stage has a distinct type.

This directly avoids the parsed-versus-booked confusion described in Beancount’s Vnext design. ([Beancount][1])

## 17.2 Lossless parser

The parser should retain:

```text
comments
whitespace
exact decimal spelling
source spans
include boundaries
unresolved expressions
chosen surface syntax
```

The generic grammar recognizes unknown form names without needing tokenizer modifications.

Form packages supply schemas after parsing.

This allows:

```text
parse before package resolution
resolve packages
type form clauses
render without destroying formatting
```

## 17.3 Type representation

Internally:

* hash-consed type DAGs;
* lazy BDD-style representation for unions, intersections, and negations;
* row variables for open records and variants;
* recursive `μ` binders;
* nominal hashes for forms;
* structural subtyping proofs;
* bidirectional inference;
* explicit type annotations required for exported package definitions, but not ordinary ledger entries.

Elixir’s move from simpler normal forms toward lazy BDD representations for set-theoretic types is relevant evidence that representation strategy matters greatly for real-world performance. ([HAL][16])

## 17.4 Constraint engine

The trusted solver should support a narrow certifiable theory:

```text
exact rational/decimal linear arithmetic
unit and dimension equations
dates and intervals
finite domains
constructor constraints
bounded sums
identity equalities and disequalities
```

Solvers may search for solutions.

The kernel checks their certificates.

A third-party Revo script can propose:

```text
?fee = 17 USD
```

but acceptance relies on the native checker proving:

```text
600 = 583 + 17
```

## 17.5 Rule engine

Pure semantic and interpretation rules compile to a typed, stratified relational IR.

Monotonic rules form the default.

Non-monotonic behavior is isolated into:

```text
explicit decisions
legal defaults/exceptions
closed-world coverage declarations
selected interpretations
```

The engine can use incremental dataflow for recursive derived relations and aggregates.

A new payment should update the affected invoice, AR aging, cash report, budget, forecast variance, and tax view—not rerun every rule over the entire ledger.

## 17.6 Provenance

Every derived fact carries a compact proof DAG:

```text
source facts
accepted decisions
form elaborations
rule applications
measurement observations
rounding steps
```

Queries can specialize this provenance into:

```text
why
how
source list
audit trace
confidence boundary
changed-since-close
```

## 17.7 Equality and matching

Keep three relations separate:

```text
same identity
economically corresponds
fulfills or settles
```

Two bank rows can represent the same occurrence.

A payment can correspond to a settlement without being identical to it.

A payment can fulfill several invoice obligations.

An equality engine or union-find structure handles accepted identity.

Fuzzy matchers only generate proposals.

## 17.8 Storage

The workspace can remain text-first:

```text
*.bean          human semantic source and decisions
bean.lock       package roots
inbox/          user-controlled raw files
.bean/objects/  content-addressed blobs and compiled forms
.bean/index.db  rebuildable indexes and caches
```

The database is not the sole source of truth.

Deleting indexes should not destroy accepted accounting history.

Large market datasets can live as immutable Arrow/Parquet blocks referenced by hash:

```text
dataset XNAS.Quotes[2026-09-10]
  file "market/xnas-2026-09-10.parquet"
  hash #71bd...
  rows 48_219_771
```

## 17.9 Incrementality

A Salsa-like demand graph can track:

```text
source -> parse -> form inference -> elaboration -> rule result -> query/report
```

Recursive relation maintenance can use differential-dataflow-style techniques.

The system recomputes only the proof slices affected by:

```text
new source
new decision
package upgrade
changed scenario
new price or index vintage
```

## 17.10 Trusted computing base

Keep the trusted core small:

```text
canonical decoder and hasher
type checker
constraint checker
rule/proof checker
interpretation coherence checker
content-addressed store verifier
```

Revo scripts, importers, renderers, matchers, and simulations are outside it.

They may search, decode, suggest, and display.

They cannot mint proofs.

---

# 18. Better importer composition

Beancount importers traditionally have to perform extraction, categorization, duplicate suppression, transaction construction, and often source merging around a representation that expects complete transactions. The official importer workflow acknowledges that institution formats change and that importers need regression fixtures; community work has repeatedly encountered duplicate handling, revising generated entries, and joining multiple sources. ([Beancount][17])

The form architecture divides this cleanly.

```text
decoder      bytes -> Observed[Form]
normalizer   Observed[T] -> Observed[T]
matcher      observations -> Proposal[Relation]
resolver     accepted relations -> Resolved[T]
elaborator   Resolved[T] -> semantic judgments
interpreter  judgments -> book/tax/budget/forecast views
```

Each stage is independently testable.

An importer does not choose expense accounts unless its declared purpose includes classification.

An API decoder does not decide whether a payment settles an invoice.

A bank importer does not construct a fake complete double-entry transaction around one observed side.

## Multi-source ingestion becomes ordinary intersection

```text
ObservedBy[Stripe](Settlement)
& ObservedBy[JPM](BankMovement)
& UserDecision(SameCashOccurrence)
```

This is dramatically cleaner than permanently merging generated transactions or passing mutable objects through an ordered plugin chain.

---

# 19. Updatable reports through lenses

A report does not have to be a dead table.

Relational lenses study bidirectional views whose type system and functional dependencies determine how valid view edits can be translated back to source updates. ([ResearchGate][18])

That idea can improve accounting UX enormously.

Suppose a report shows:

```text
Invoice INV-42
Outstanding: 400 USD
State: partial
```

The user edits:

```text
State: paid
```

The system must not store `state = paid`.

It can offer the valid causes:

```text
1. Match an existing unallocated 400 USD payment
2. Record a new 400 USD payment
3. Apply a 400 USD credit note
4. Write off 400 USD
5. Correct the original invoice amount
```

Each option is a form constructor or relation.

The selected operation updates the semantic source through a lawful lens.

Likewise, changing a report’s category could mean:

```text
local classification decision
party identity correction
source-observation correction
book-specific reclassification
```

The UI asks which meaning is intended.

Derived state never becomes mutable truth merely because it is displayed in an editable grid.

---

# 20. Editor and CLI UX

The complexity should primarily appear as better assistance.

## LSP information

Hovering a plain transaction might show:

```text
Inferred:
  Transaction
  & Payment.Settlement[USD]
  & Stripe.Settlement[USD]

Status:
  accepted
  fully resolved
  recognized by 3 books
```

Hovering an amount:

```text
17 USD

Derived as:
  600 USD gross
- 583 USD net

Rule:
  Payment.Settlement.gross = net + fee

Sources:
  Stripe settlement JSON
  JPM bank line 443
```

## Code actions

```text
Narrow to Stripe.Settlement
Collapse postings to domain form
Expand domain form to postings
Name inferred hole
Materialize inferred value
Show source evidence
Show accounting proof
Show alternate valid interpretations
Accept match proposal
Pin package version
Generate migration fixture
```

## Commands

```text
bean check
bean holes
bean choices
bean conflicts
bean review
bean ingest
bean query
bean why
bean why-not
bean forecast
bean budget
bean tax
bean close
bean diff
bean upgrade
bean migrate
```

## Diagnostics speak accounting first

Bad:

```text
Type mismatch at constraint C-8842.
```

Good:

```text
Stripe settlement set_91 does not reconcile.

  gross                 600.00 USD
  net                   583.00 USD
  recorded fee           18.00 USD
  implied fee            17.00 USD
  difference              1.00 USD

Conflicting evidence:
  Stripe JSON: 17.00 USD
  handwritten entry: 18.00 USD
```

Bad:

```text
Ambiguous inhabitant of InvoiceId.
```

Good:

```text
Payment pay_91 can validly settle either invoice.

  INV-42  600 USD due Sep 30
  INV-57  600 USD due Sep 30

No available evidence distinguishes them.

Choosing INV-42 changes customer aging but not total assets.
```

## Progressive strictness

The default modes can be:

```text
plain       validate Beancount semantics
suggest     infer forms and offer enrichments
typed       enforce explicitly selected forms
controlled  require accepted sources and decisions
closed      require pinned packages and reproducibility
```

A personal user can stay in `plain` or `suggest`.

A business can make `controlled` mandatory for selected directories or books.

---

# 21. Migration from Beancount

This architecture should be buildable incrementally.

## Phase 1: exact Beancount compatibility

Implement:

* lossless parsing;
* existing directives;
* booking;
* balance assertions;
* inventories;
* standard queries;
* plain export.

Every ordinary transaction receives the generic form:

```text
Core.Transaction
```

## Phase 2: inferred concepts

Infer structural concepts from existing postings:

```text
CashTransfer
ExpensePurchase
SecurityAcquisition
LiabilityPayment
```

These remain suggestions.

No source changes.

## Phase 3: form ascription

Add:

```text
:: Form.Name
```

and typed holes.

Build the form registry and LSP.

## Phase 4: domain surfaces

Permit:

```text
DATE Qualified.Form ...
```

with generic clause syntax.

Add forgetful export to ordinary Beancount.

## Phase 5: evidence ingestion and Revo

Implement:

* blob store;
* observed forms;
* Revo extension;
* source spans;
* proposal review;
* idempotent ingestion.

## Phase 6: interpretations

Move:

```text
book rules
budgets
forecasting
tax packages
domain reports
```

onto typed interpreters.

## Phase 7: content-addressed packages

Add:

* definition hashing;
* package roots;
* namespace histories;
* migrations;
* signed attestations;
* upgrade impact analysis.

At every phase, existing Beancount files remain usable.

---

# 22. What should remain out of scope

The design becomes weaker if it tries to absorb every external system.

It should not become:

* a payment processor;
* an order-routing system;
* a payroll withholding service;
* an OCR model;
* a market-data vendor;
* a tax filing gateway;
* a document-signature system;
* a universal workflow engine.

Those systems supply evidence, execute actions, or consume outputs.

The ledger models their economic meaning and provenance.

A Revo package can connect to them.

The core remains:

```text
forms
judgments
constraints
rules
proofs
versions
```

---

# 23. The core laws

The architecture should be held to a small set of laws.

## Surface round trip

```text
parse(print(value)) = value
```

up to non-semantic formatting.

## Forgetful coherence

For every ornament:

```text
interpret_generic(forget(rich))
=
interpret_rich(rich)
```

where the generic interpretation is applicable.

## Order-independent evidence

For compatible observations:

```text
a ∧ b = b ∧ a
(a ∧ b) ∧ c = a ∧ (b ∧ c)
a ∧ a = a
```

## No silent non-monotonicity

Adding evidence may refine knowledge or expose contradiction.

It may not silently replace a prior accepted choice.

## Explicit ambiguity

If materially distinct valid models remain, the result is a choice—not an arbitrary winner.

## Proven output

Every recognized book value and filing field can identify:

```text
accepted inputs
rules
measurements
decisions
rounding
package versions
```

## Closed reproducibility

A close or filing can be recomputed from pinned roots without network access, current time, or mutable external state.

## Plain export

Every form claiming to refine ordinary accounting must have a defined projection to generic economic meaning and, where applicable, balanced postings.

---

# 24. The strongest concise formulation

The earlier designs treated a richer ledger as:

```text
more semantic objects
+ more rules
+ more workflows
```

This design makes the extension boundary much smaller:

```text
one tiny judgment kernel
one content-addressed form calculus
many pure interpretations
Revo at every flexible edge
```

The decisive abstraction is the **inductive form**.

A form is:

```text
a data type
+ a surface grammar
+ a refinement
+ an ornament
+ an importer target
+ a semantic elaboration
+ a query relation
+ an interpretation boundary
+ a migration unit
+ a versioned community artifact
```

The ordinary user still writes:

```beancount
2026-09-22 * "Stripe" "Settlement"
    Assets:Bank:JPM              583 USD
    Expenses:ProcessingFees       17 USD
    Assets:StripeClearing       -600 USD
```

The user who wants one extra guarantee writes:

```beancount
... :: Stripe.Settlement
```

The user who wants compact domain syntax writes:

```text
2026-09-22 Stripe.Settlement "set_91"
  gross 600 USD from Assets:StripeClearing
  net   583 USD into Assets:Bank:JPM
  fee       ?   into Expenses:ProcessingFees
```

The importer author writes Revo.

The package author defines an inductive form.

The accountant selects books and policies.

The tax expert authors a literate jurisdiction package.

The forecaster evaluates contracts under scenarios.

The auditor follows proofs.

And all of them operate over the same accepted economic history without forcing its richest semantics into the syntax used by its least sophisticated author.

That is the architecture that can remain as pleasant as Beancount at the bottom while scaling into a real international financial language at the top.

[1]: https://beancount.github.io/docs/beancount_v3/ "https://beancount.github.io/docs/beancount_v3/"
[2]: https://elixir-lang.org/blog/2026/01/09/type-inference-of-all-and-next-15/ "https://elixir-lang.org/blog/2026/01/09/type-inference-of-all-and-next-15/"
[3]: https://cuelang.org/ "https://cuelang.org/"
[4]: https://arxiv.org/abs/1610.04799 "https://arxiv.org/abs/1610.04799"
[5]: https://www.unison-lang.org/ "https://www.unison-lang.org/"
[6]: https://github.com/if-not-nil/revo "https://github.com/if-not-nil/revo"
[7]: https://elixir-lang.org/blog/2022/10/05/my-future-with-elixir-set-theoretic-types/ "https://elixir-lang.org/blog/2022/10/05/my-future-with-elixir-set-theoretic-types/"
[8]: https://dl.acm.org/doi/10.1145/3776662 "https://dl.acm.org/doi/10.1145/3776662"
[9]: https://groups.google.com/g/beancount/c/R9wWrdP6mVU "https://groups.google.com/g/beancount/c/R9wWrdP6mVU"
[10]: https://raw.githubusercontent.com/if-not-nil/revo/main/TODO.md "https://raw.githubusercontent.com/if-not-nil/revo/main/TODO.md"
[11]: https://github.com/if-not-nil/revo/blob/main/CHANGELOG.md "https://github.com/if-not-nil/revo/blob/main/CHANGELOG.md"
[12]: https://www.researchgate.net/publication/221559651_Provenance_Semirings "https://www.researchgate.net/publication/221559651_Provenance_Semirings"
[13]: https://www.microsoft.com/en-us/research/publication/composing-contracts-an-adventure-in-financial-engineering/ "https://www.microsoft.com/en-us/research/publication/composing-contracts-an-adventure-in-financial-engineering/"
[14]: https://groups.google.com/g/beancount/c/X2DLI8gSXyw "https://groups.google.com/g/beancount/c/X2DLI8gSXyw"
[15]: https://arxiv.org/abs/2103.03198 "https://arxiv.org/abs/2103.03198"
[16]: https://hal.science/hal-05369012v1/document "https://hal.science/hal-05369012v1/document"
[17]: https://beancount.github.io/docs/importing_external_data/ "https://beancount.github.io/docs/importing_external_data/"
[18]: https://www.researchgate.net/publication/221559630_Relational_lenses_A_language_for_updatable_views "https://www.researchgate.net/publication/221559630_Relational_lenses_A_language_for_updatable_views"

