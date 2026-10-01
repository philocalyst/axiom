# Sync cutover handoff

This records the new SY2 work on `cutover/sync`. It is an implementation handoff,
not a claim that the full cutover or v4 migration is complete.

## Decisions reflected in LANGUAGE §14

- A structured code is the code itself, with an optional leading `^` removed and
  ASCII letters folded to lowercase. A code rule's matching text does not invent
  a prefix. A structured code selects a counterparty only when it identifies an
  open claim; otherwise structured `party`, structured `via`, then memo
  recognition provide the counterparty. `party` names who the transaction was
  with; `via` can identify who the money was for when `party` is absent or
  unresolved. A distinct memo party becomes the intermediary. Memo ambiguity
  does not defeat an authoritative structured counterparty, while an ambiguous
  structured value remains an error.
- A posted balance belongs to its row. The feed considers the latest posted,
  own-currency day with a balance and emits one assertion only if the available
  balances imply one unambiguous day-end amount. Conflicts emit no assertion;
  pending and foreign-currency rows do not determine it. Tagged statements do
  not have a statement-level balance.
- CAMT memo text joins `AddtlNtryInf` and `RmtInf/Ustrd` in declared order,
  matching the normal row reader's multi-column memo behavior. Text split by
  XML comments or CDATA inside one element is joined too; adjacent fragments
  concatenate, while a whitespace boundary contributes one space.

## Code and tests

`axiom_model::sync` is the declaration schema consumed by the reader and matcher;
the new cutover is not a blind merge of the denied historical SY branch. Source
reading is borrowing-first, with ownership only for decoded/combined values.
`read_memos` is local and read-only for `check`; it returns reader diagnostics and
never executes a `run` source. `matching_paths(root, pattern)` exposes the
project-confined, sorted `read` glob expansion without reading file contents.
The isolated harness at `/tmp/axiom-sync-primitives-forecast` includes the exact
`cell.rs`, `amount.rs`, `csv.rs`, `tagged.rs`, `paths.rs`, `write.rs`, and `sink.rs` source
files and links the repository's real `axiom_core`, `axiom_syntax`, and `memchr`
dependencies. Its log is in the sibling task evidence directory at
`../verification/cutover/sync-primitives-forecast-tests.log`.
The current run passes 40 tests, with 3 timing tests ignored. It is evidence for
those modules only, not a substitute for the crate or workspace build.

Source regression fixtures are in `v2/crates/sync/src/world/tests.rs` and
`v2/crates/sync/src/format/tests.rs`. They cover structured party/via/code
precedence, memo intermediaries, balance ordering and conflict refusal,
CAMT's two memo fields, and `check` reading without reconciliation. The helper's
owned-first-cell promotion also has direct tests in `v2/crates/sync/src/cell.rs`.
These test cases still need to run in the integrated crate after the model build
is green.

## Remaining integration

The canonical source builder and engine promise/open-claim monitor are still
being completed on their owning lanes. The sync runtime must consume their
typed `Book::sources`, `Run` promise flows and open claims; it must not rebuild
contract occurrences or settlement in a parallel path. The CLI's `sync [NAME…]
[--dry]` command should call a plan-only boundary, leaving diff rendering and
file application to the CLI. Until those pieces are integrated and verified,
the old runtime `World`/session compatibility layer and the end-to-end CLI are
not evidence of a complete canonical cutover.
