# Lane S5 (syntax) notes for the model lanes

The v4 branch is at 455f819: S5's syntax (32826f8) plus main e643ea5, with core's `On` and `Period` re-exported from `ast.rs`. `cargo test -p axiom-syntax` passes (81 tests). The whole v4 sketch parses, `std-sketch.ax` included.

## AST changes the model must follow

- **Types renamed or replaced:**
  - `Id<T>` is now `Ref<T>`, and `ExprId = Ref<Expr>`. `Ref<T>` and `Many<T>` live in `refs.rs`; `&file[ref_or_many]` indexes them.
  - The old `Amount(&str)` is `Literal`. The new `Amount` is `Literal(Literal) | Computed(ExprId)`.
  - Strings are `Text`. `Quantity::Fixed` is now `Quantity::Amount`.
  - `ItemKind` loses `Assert`, `Event`, `Price`, `Split`, `Claim`, `Occurrence` and `Ending`. It gains `Statement`, `Pattern` and `Format`. `Setting` gains `Currency(Name)` and `Rates(Rates::Spot | Param(Name))`.
- **`Statement { date, subject, verb, tail, body }`:**
  - `subject: Name | Code | Purpose | Unit`.
  - `verb`: `Occurrence(Option<Amount>)`, `Value(Amount)`, `Owes { creditor, amount: Option }`, `Now(Change)`, `Worked(Literal)`, `Used(Literal)`, `Waived`, `Ends`, `Event(EventState)`, `Split { numerator, denominator }`, `Basis { amount, since }`, `Filed(year)`.
  - `Change` is `Terms(Ref<Terms>) | Property(Prop) | Budget(Ref<Allowance>) | Amendment`.
  - The tail has `Against`, `Via`, `Until`, `Since`, `For::Last(Relative)`, `For::Whom`, `Price(Literal)` and `Description(Text)`. `For::Entity` is now `For::Whom`.
  - `Opening.claims` is `Many<Statement>`: owes statements, dated with the opening's day.
- **Amounts:**
  - A share alone is `Computed(root)` with a `Pct` root, meaning "of the header's amount".
  - `N% of REF` is `Of(Pct, ref)`. `X up to Y` is `Binary(UpTo)`. `REF @ PRICE` is `At(quantity, price)`. `^code[HR]` is `Index`. A bare `[x]` is `Select`.
  - `ExprKind::Fraction(u32, u32)` and `Month(Day)` are new.
  - A leg `NAME = AMOUNT` is `Quantity::Target`. The model decides whether it is an input or a target balance.
- **`Flow` and `Body`:** `Flow.legs` is now `Flow.body: Body { legs, items }`. `LineItem { doc, sign, amount, tail, loc }` is the new line item.
- **`Contract`:**
  - New fields: `party: Option`, `schedule` and `standing` (each a `Schedule { at, terms: Terms }`), `purpose`, `description`, `deadline: Option<Deadline { span, otherwise: Option<LineItem> }>`, and `alsos: Many<Also>`.
  - `Terms { about, payment, cadence, on, holding: Option<Holding> }`. A change restating terms may leave out the holding.
  - `props` are generic `Prop { name, args: Many<ExprId>, lines: Many<Nested>, loc }`. The model decodes them, including the nested loan lines (`resets`, `prepay`) and `share … for …`.
- **`Decl`:** adds `purpose`, `budget: Option<Ref<Allowance>>`, `known_as: Many<Pattern>` and `alsos`.
- **Budgets:** `Budget { purpose, allowance: Allowance { limit: Limit::Amount(Amount) | Share{percent, of}, per, carries, funded: Option<Funding{from, into}> } }`.
- **Laws:**
  - `Law.overrides` and `StepKind::Unless` are new.
  - `Require.otherwise` is `Many<Effect>` in order, and `message` is `Text`.
  - `BinOp::UpTo` is new.
  - A law in a purpose without a trigger gets `Trigger::Flow`, with `trigger_loc` set to the `law` header.
- **Other decl-level nodes:**
  - `CodeRule.known_as: Many<Pattern>` and `Param.unit: Option<Name>` are new.
  - `Sync { name, read, run, into, format: Option<Ref<Format>> }`.
  - `Format { name, lines: Many<FormatLine { key, args: Many<FormatArg::Word | Quoted>, loc }> }`. The lines are raw words: the model decodes them.
  - Patterns are `NamedPattern`, `Pattern { choices }`, `Sequence { terms }` and `PatternTerm { capture, atom: Literal | Class | Named | Group, repeat }`.
- **Shared tables and helpers to use:** `Policy::WORDS`, `Class::WORDS`, `MONTHS`, `Folder::of(path)`, `File::iter::<T>()` and `&file[ref_or_many]`. Drop the model's duplicate tables. Type-word tables stay model-only.
- **Forgotten arrow:** `DATE A B 5 USD` is an `expected-arrow` error with a fix, so it never reaches the model as a statement.
- **Raw lines:** `run` and `into` text ends at a `//` that follows a blank.
- **Formatter:** `axiom_syntax::format(src, &file)` is the house style. Sync writers and `axiom fmt` use it.
