# Report client API and JSON contract

The report crate exposes typed view data, a borrowed source-position interface,
and renderer boundaries. A terminal, editor, or GUI can build and render the
same `Report`; renderers do not perform ledger calculations.

## Build reports coherently

For a client that serves more than one query, create one `Context` from the
book and its `Options`. It owns the plan, run, checkpoint, and resolved owner
scope together, so reports cannot accidentally combine state from different
runs. Each query then uses that shared context:

```rust,ignore
let context = Context::new(&book, options, whose)?;
let report = context.report_with_sources(&query, &sources)?;
let json = JsonRenderer.render(&report, &sources);
```

`Context::new` takes the same `today` and `relaxed` settings used to define the
run. Pass `whose` once to keep report scope consistent across queries. A client
that already owns a `Run` can use the free `report(book, run, query, whose)`
function; that compatibility path continues to use the supplied run.

`Query` is the client-facing description of the currently implemented views:
balance, register, flow, available, budget, limits, claims, tax, gains, lots,
forecast, and why. `Query::Why` can also express `why FILE:LINE`; the source
provider resolves it to a `Loc` without requiring the report crate to read
files or depend on CLI code.

## Sources and renderer boundary

Implement `SourceProvider` for the source catalog owned by the client:

```rust,ignore
impl SourceProvider for MySources {
    fn locate(&self, path: &str, line: usize) -> Option<Loc> { /* ... */ }
    fn describe(&self, loc: Loc) -> Option<SourcePosition<'_>> { /* ... */ }
}
```

`locate` accepts one-based line numbers. `describe` returns a borrowed path and
one-based line and column; columns count Unicode scalar values, with a tab
counting as one column. Return `None` for a missing source, reversed or
out-of-bounds byte ranges, or offsets inside a UTF-8 character. This keeps
source links useful to clients while preserving exact byte ranges for edits.

Implement `ReportRenderer` to draw `Report` data in a client-specific way. The
report-side `JsonRenderer` and CLI terminal renderer are two such consumers.
`Cell` retains whether a value is text, an amount with its unit and scale, a
day, a percentage, a source location, or blank; the renderer does not need to
parse display strings to recover those types.

## JSON report document

`axiom_report::json::render` and `JsonRenderer` serialize the report structures
without re-running a view. The current document has this shape:

```json
{
  "title": "...",
  "sections": [{
    "heading": "... or null",
    "columns": [{"title": "...", "align": "left or right"}],
    "rows": [{"depth": 0, "style": "normal", "cells": []}],
    "notes": []
  }]
}
```

Cells are tagged objects: `blank`; `text` with a string `value`; `amount` with
a display-string `value` and a separate `unit`; `day` with an ISO date string;
`percent` with the same percentage text used by the text renderer; and `source`
with its location. A source location includes `file`, `line`, and `column`
when the provider can describe it, plus the raw `file_id`, `start_byte`, and
`end_byte`. Unknown display positions are `null`; raw location identity and
byte offsets remain present.

All text is JSON-escaped, including control characters and quotes in units.
Reports are one JSON document followed by a newline. The document serializes
the data available in the current report API; it does not claim an XBRL facts
or sentence model.

## Diagnostic NDJSON

`axiom_report::json::diagnostics` writes one JSON object per diagnostic, each
followed by a newline. `axiom check --json` uses this form on standard output,
including when there are no diagnostics (an empty byte stream). Each object
contains `code`, `severity`, `headline` (the first message line), the full
`message`, `labels`, `notes`, `helps`, and `fixes`. Locations retain byte
offsets. Each fix also includes `end_line` and `end_column` for the exclusive
end cursor, and its `replacement` text. Unresolved display positions are
`null`, while their file IDs and byte ranges remain available to machine
clients.

The current API represents report tables and typed cells. The broader v4
facts, sentences, and XBRL concepts described in the rework brief are not yet
implemented, nor are future views such as contracts or measures. They should
be added when their model and engine producers exist rather than inferred by a
renderer from today's cell strings.
