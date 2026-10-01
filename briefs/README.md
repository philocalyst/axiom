# Lane briefs, audits and research

These are the working documents of the v4 rework, copied from the orchestrating session's scratchpad. Start from [../REMAINING.md](../REMAINING.md), which says what is done, what remains, in what order, and which of these each lane reads.

Some of them still name worktree paths such as `/home/user/axiom/.claude/worktrees/lane-v4`. Those paths were one machine's checkout of the `v4` branch. The branch is now pushed as `claude/great-wozniak-pnqn7x-v4`. "The orchestrator" is whoever runs the lanes and merges them.

| file | what it is |
|---|---|
| [common.md](common.md) | the part of every lane's brief that is the same: context, reading list, code-quality rules, working rules, report format |
| [lane-M4a-model-declarations.md](lane-M4a-model-declarations.md) | the model's declarations, names, kinds, places, purposes, properties, laws, units, norms, `also`, budgets, sources, formats, patterns, and std in v4. Its revision 2 fixes the shape of formats and the encoding of patterns. |
| [lane-M4b-model-journal.md](lane-M4b-model-journal.md) | the model's journal: flows, items, references, statements by verb, contracts, claims, assets, shares. It expects a `notes-M4a.md` handoff from M4a. |
| [lane-E4b-engine-semantics.md](lane-E4b-engine-semantics.md) | the engine's v4 semantics |
| [lane-E4c-engine-units-norms-diagnostics.md](lane-E4c-engine-units-norms-diagnostics.md) | units at run time, norms, filed returns, and the diagnostics that use the whole architecture |
| [lane-R4-report-cli.md](lane-R4-report-cli.md) | the report and cli: views as data, JSON, `fmt`, and the v4 views. Partly done: see REMAINING §4.2. |
| [lane-SY-sync.md](lane-SY-sync.md) | the `sync` crate. The pure layer is done; the binding to the book is REMAINING §4.7. |
| [lane-Y4-systems-examples.md](lane-Y4-systems-examples.md) | `us`, every example and the mistakes corpus in v4 (parts A and B) |
| [notes-S5-syntax.md](notes-S5-syntax.md) | the v4 AST as lane S5 left it, for the model lanes |
| [audit-core-syntax-model.md](audit-core-syntax-model.md) | fifteen ranked findings on core, syntax and model. The model lanes own 1–3, 5–7, 11 and 13. |
| [audit-engine-report-cli.md](audit-engine-report-cli.md) | twelve ranked findings on engine, report and cli. Lane E4a did most of the engine's; R4 did 4, 5, 8 and 10. |
| [theory.md](theory.md) | the research the v4 design rests on: ValueFlows, REA, ACTUS, CSL/POETS, defeasible norms, units of measure |
| [limabean.md](limabean.md) | the benchmark our diagnostics beat: a Beancount implementation's error output, reproduced |
| [types-v4.rs](types-v4.rs), [types-v4b.rs](types-v4b.rs), [types-v4c.rs](types-v4c.rs) | the annotated sources of the v4 public types. The code in `crates/model` and `crates/engine` is authoritative. |
| [loc.py](loc.py) | counts non-test, non-comment lines per crate: `python3 briefs/loc.py .` from `v2/` |
