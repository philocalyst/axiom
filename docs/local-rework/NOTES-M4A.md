# M4a to M4b model handoff

This note records the boundary between declaration/law lowering (M4a) and
journal lowering (M4b) on `cutover/model`.

`sources::arrange` produces `Vec<Site<'a, 's>>`. Each `Site` borrows one
`Source<'s>` and carries its resolved `home` and source `layout`; each
`Source` owns one `axiom_syntax::File<'s>`. S5 references are local to that
file: index a `Ref<T>` with `&file[id]`, a `Many<T>` with `&file[range]`, and
expressions with `file.exprs[id]`. Do not use the old v3 `Entry::Plan` or
`Surveyed::journal` chunks to lower S5 journal items.

The declaration path consumes declaration nodes and builds `World<'s>`.
`World` is crate-visible and owns the mutable `Book`, name resolver state,
kind/property state, and source locations. Its `resolve::World` methods include
`commodity(&str)`, `kind(Home, Word)`, `entity(Home, Word)`, `place(Word)`, and
`amount(Dec, unit, Loc)`. `Book` exposes resolved lookups for places, entities,
kinds, purposes, assets, and contracts. `Lookup` and its maps are crate-visible
for typed registration during lowering.

M4a owns declaration lowering for kinds, purposes, entities, accounts, assets,
commodities, properties/defaults, parameters, laws, budgets, code rules, sync
sources, formats, and patterns. It also owns contract-independent laws and the
shared typed model schema. M4b owns journal items and contract declarations.

Contract lowering has three phases before final journal ordering: first scan
all contract headers and reserve their names and typed IDs in `Book.contracts`
and `Lookup.contracts`; next compile each contract's party, terms, templates,
and nested laws; only then lower dated statements and occurrences that refer to
those contracts. This lets files refer forward to a contract without string
copies or order-dependent resolution.

The shared flow API is in `model/src/journal.rs` and `book.rs`. `Flow` stores
header and local code `Run<Sym>` handles plus selector and detail handles.
`Book::flow_view` borrows Book-owned metadata; `FlowView::codes()` yields
transaction-header codes before local codes. Forecasts that transform rare
details use `RuntimeFlow` and an engine-owned `Arena<RuntimeDetail>`, resolved
by `Book::runtime_flow_view`; ordinary flows continue to borrow Book detail
records.
