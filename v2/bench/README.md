# Native benchmark generator

`gen.py` retains the seeded v3 financial simulation and passes its output through
`nativeize.py` to write native v4 purposes, parties, contracts, prices, and
journal syntax. Run it with the commands in `run.sh`; the old `REPORT.md` is a
historical v3 measurement and does not claim v4 performance.

## Opening balances

The source simulation starts each person with four USD balances: $30,000 in
checking, $20,000 in savings, $60,000 in the 401(k), and $10,000 in the 529
account. It represented these as four ordinary flows from a synthetic
`equity/.../opening` source. `nativeize.py` writes them as four holding lines
under one `opening DATE` block. This preserves the seeded amounts and keeps
opening state out of ordinary laws, as required by the language semantics.
There are still four model holding rows per person; the native syntax adds one
opening header. `MANIFEST` keeps `flows` and `journal_lines` from the simulation
and records the rewritten physical size separately as `native_journal_lines`
and the number of initial holdings as `opening_holdings`.

The original simulation has no opening security quantities, parcel basis, or
acquisition dates to carry over: all four opening positions are amounts in
USD, and all commodity positions start at zero. The converter therefore does
not invent lot metadata. Purchases later in the generated journal continue to
create lots with their actual dates, quantities, and exact cash amounts.

The old 529 qualification marker applied only to the four `edu/tuition/*`
destinations; books, supplies, and fees were not qualified. The native source
keeps `pN-edu-tuition` under the standard `education` purpose and keeps the
other edu leaves under `pN-edu`. A per-owner law on the spending root adds the
other edu leaves under `pN-edu`. Two owner-filtered laws apply the same
combined monthly cap: one under each of those purpose subtrees sums
`total(#pN-edu, month)` and `total(#pN-edu-tuition, month)` before comparing
the result with the former $12,000 edu-category allowance. This preserves the
combined allowance and narrower 529 qualification rule without making other
education-category spending qualified or attaching one rule to the global
spending root.

This representation uses two law IDs because the source language currently
attaches a purpose law to one purpose subtree. If both branches contribute to
an over-limit month, both law IDs can produce a warning for that month. The
financial limit is the same combined $12,000 cap; warning identity/count and
the budget report's per-purpose rows are not equivalent to the old single
category-budget record.

## Timing interpretation

The CLI has no phase-only timing switch. These generated projects have no
declared sync jobs, so `sync` covers project loading, parsing, and model
construction without the engine. In these fixtures, `check - sync` is an
approximation of the engine cost and `report - check` approximates report work.
For other projects, `sync` can also run declared scripts and is not a general
parse-only command. `run.sh` uses the native account names (`p1-checking`,
`p1-brokerage`) emitted by the converter.
