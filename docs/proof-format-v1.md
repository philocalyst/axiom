# Axiom canonical proof format, v1

This document is the executable-format contract for the proof boundary in
`src/proof.rs`.  The Rust implementation is the producer and checker; the
format below is deliberately independent of Rust data layouts.  A reader may
implement the byte encoder and the structural checks from this document and
compare its result with the conformance tests in
`tests/proof_format_conformance.rs`.

## Scope and versioning

The format version is part of each domain label.  The labels are exact,
case-sensitive UTF-8 byte strings:

| use | domain label |
| --- | --- |
| node content address | `axiom/proof-node/v1` |
| compact proof content address | `axiom/proof/v1` |
| complete persisted proof bytes | `axiom/canonical-proof/v1` |

Changing a field order, tag spelling, integer representation, or validation
rule requires a new version label.  A v1 consumer must reject an unknown
version before interpreting the payload.  Domain labels are length-prefixed,
so a node hash, proof hash, and serialized proof cannot accidentally share a
hashing namespace.

This is a canonical *encoding* and checker contract, not a transport format:
the current crate exposes `Proof::canonical_bytes()` and `Proof::check()`, but
does not yet expose a decoder.  No claim is made here that arbitrary bytes can
currently be loaded by the crate.  A future decoder must reject truncation,
invalid UTF-8, unknown operation tags, and trailing bytes.

## Primitive encoding

All lengths and counts are unsigned 64-bit integers in big-endian order.  No
integer is varint encoded.

* `u64(x)` is the eight-byte big-endian representation of `x`.
* `bytes(x)` is `u64(len(x)) || x`.
* `string(s)` is `bytes(UTF-8(s))`. Rust strings are UTF-8.
* `id(i)` is the 32 raw bytes of the proof ID, in byte order (not hex text).
* `optional-string(None)` is `0x00`; `Some(s)` is `0x01 || string(s)`.
* `optional-id(None)` is `0x00`; `Some(i)` is `0x01 || id(i)`.
* `ids(xs)` is `u64(count(xs)) || id(x0) || ... || id(xn)`.
* `transitions(xs)` is `u64(count(xs))`, followed by each transition's
  `string(state)` and `optional-string(at)`.
* `journal-lines(xs)` is `u64(count(xs))`, followed by each line's
  `string(side) || string(account) || string(amount) || string(unit)`.

Every exact number in an operation is encoded as `string(number.canonical_string())`.
For v1, `canonical_string` is a normalized rational: an integer is decimal
signed base-10 with no leading zeroes (except `0`), and a non-integer is
`numerator/denominator`, with a reduced positive denominator.  Source decimal
scale is not retained in a proof hash.

## Operation encoding

An operation is `bytes(tag) || payload`.  The following table gives the exact
tag and payload order.  Names in parentheses are the public Rust fields.

| operation | tag | payload |
| --- | --- | --- |
| `Observation { source }` | `observation` | `string(source)` |
| `QuoteObservation` | `quote-observation` | `string(quote) || string(date) || string(base) || string(base_unit) || string(quote_amount) || string(quote_unit)` |
| `PositionObservation` | `position-observation` | `string(account) || string(quantity) || string(unit)` |
| `CashSettlementObservation` | `cash-settlement-observation` | `string(reference) || string(amount) || string(unit) || optional-string(into)` |
| `BlockedSale` | `blocked-sale` | `string(sale) || id(source_proof) || string(quantity) || string(proceeds) || string(quantity_unit) || string(value_unit) || string(account) || string(asset) || string(reason)` |
| `SettlementReconciliation` | `settlement-reconciliation` | `string(settlement) || id(source_proof) || string(sale) || id(sale_proof) || string(observed) || string(expected) || string(unit) || string(status)` |
| `LotObservation` | `lot-observation` | `string(lot) || string(source) || string(account) || string(asset) || string(quantity)` |
| `SaleObservation` | `sale-observation` | `string(sale) || string(source) || string(account) || string(asset) || string(quantity) || string(proceeds) || string(quantity_unit) || string(value_unit)` |
| `ObligationObservation` | `obligation-observation` | `string(obligation) || string(debtor) || string(creditor) || string(promised) || string(unit) || optional-string(due)` |
| `SettlementObservation` | `settlement-observation` | `string(settlement) || string(kind) || string(from) || string(to) || string(instrument) || string(amount) || string(unit) || transitions(history)` |
| `SatisfactionObservation` | `satisfaction-observation` | `string(satisfaction) || string(obligation) || string(settlement) || string(amount) || string(unit) || string(state)` |
| `Derive { rule }` | `derive` | `string(rule)` |
| `Arithmetic` | `arithmetic` | `string(rule) || string(minuend) || string(subtrahend) || string(result) || string(unit)` |
| `LotAllocation` | `lot-allocation` | `string(lot) || string(sale) || id(lot_proof) || id(sale_proof) || string(available) || string(allocated) || string(remaining) || string(sale_quantity) || string(sale_proceeds) || string(available_basis) || string(allocated_proceeds) || string(allocated_basis) || string(gain) || string(quantity_unit) || string(value_unit)` |
| `InventoryConservation` | `inventory-conservation` | `string(lot) || id(lot_proof) || string(before) || string(consumed) || string(after) || string(unit) || optional-id(predecessor) || optional-id(allocation)` |
| `Recognition` | `recognition` | `string(sale) || id(sale_proof) || string(account) || string(asset) || string(quantity) || string(proceeds) || string(basis) || string(gain) || string(quantity_unit) || string(value_unit)` |
| `SettlementHistory` | `settlement-history` | `string(settlement) || id(settlement_proof) || string(kind) || string(from) || string(to) || string(instrument) || string(amount) || string(unit) || transitions(history) || string(current) || one byte: `0x00` false, `0x01` true |
| `SatisfactionAllocation` | `satisfaction-allocation` | `string(satisfaction) || id(satisfaction_proof) || string(obligation) || id(obligation_proof) || string(settlement) || id(settlement_proof) || string(amount) || string(unit) || string(state)` |
| `ObligationBalance` | `obligation-balance` | `string(obligation) || id(obligation_proof) || string(promised) || string(allocated) || string(remaining) || string(unit) || ids(allocations)` |
| `SettlementBalance` | `settlement-balance` | `string(settlement) || id(settlement_proof) || string(amount) || string(allocated) || string(unused) || string(unit) || ids(allocations)` |
| `PositionReconciliation` | `position-reconciliation` | `string(account) || id(source_proof) || string(observed) || string(calculated) || string(result) || string(unit) || string(status)` |
| `JournalEntry` | `journal-entry` | `string(sale) || id(recognition_proof) || id(settlement_proof) || string(inventory_account) || string(asset) || journal-lines(lines)` |
| `Decision` | `decision` | `string(subject) || string(answer)` |
| `Policy` | `policy` | `string(subject) || string(policy) || string(answer)` |
| `Conflict` | `conflict` | `string(subject) || string(reason)` |

The tags and payloads are a closed v1 set.  A future operation must not reuse
a v1 tag with a different payload.

## Node identity

Let `M_hash` be node metadata with the entry whose key is exactly `location`
removed.  Metadata is ordered by the map's UTF-8 string ordering.  The node
ID is the 32-byte BLAKE3 digest of:

```text
bytes("axiom/proof-node/v1")
|| string(statement)
|| operation
|| u64(count(inputs)) || id(input_0) || ... || id(input_n)
|| u64(count(M_hash))
|| string(key_0) || string(value_0) || ... || string(key_n) || string(value_n)
```

Inputs must be strictly increasing by their 32-byte ID and duplicate-free.
Metadata keys must be non-empty.  `location` is the only metadata key omitted
from the node hash; it remains in the complete serialization below.

## Complete proof bytes and compact proof identity

`Proof::canonical_bytes()` is:

```text
bytes("axiom/canonical-proof/v1")
|| u64(count(roots)) || id(root_0) || ... || id(root_n)
|| u64(count(nodes))
|| for each (map_key, node) in nodes in ascending map-key order:
     id(map_key) || id(node.id) || string(node.statement) || operation
     || u64(count(node.inputs)) || id(input_0) || ... || id(input_n)
     || u64(count(node.metadata))
     || string(key_0) || string(value_0) || ... || string(key_n) || string(value_n)
```

Roots are strictly increasing and duplicate-free.  The node map is ordered by
the 32-byte map key.  `map_key` must equal `node.id`, and each node ID must
equal the node hash above.

The compact content hash is the BLAKE3 digest of:

```text
bytes("axiom/proof/v1")
|| u64(count(roots)) || id(root_0) || ... || id(root_n)
|| u64(count(nodes))
|| for each (map_key, node) in nodes in ascending map-key order:
     id(map_key) || id(node.id)
```

This intentionally indexes node identities rather than repeating node
payloads.  It is a cache/content root, not a replacement for validating the
complete canonical bytes.

## Checker boundary

The trusted consumer is `Proof::check()` and performs, in addition to the
identity checks above:

1. canonical roots, map keys, sorted inputs, non-empty metadata keys, and
   missing-input detection;
2. acyclicity and reachability from a root (typed certificate nodes may not be
   unreachable);
3. exact arithmetic for `Arithmetic` and the typed lot, inventory,
   recognition, obligation, settlement, position, journal, and satisfaction
   certificates;
4. typed source validation, duplicate source-observation detection, endpoint
   and proof-ID binding, conservation, and status/history rules defined by the
   corresponding certificate structs in `src/proof.rs`.

The checker does not execute the engine or treat statement text and arbitrary
metadata as proof semantics.  `location` is diagnostic metadata.  A caller
must call `check()` (or `check_member()`) before trusting a non-zero ID.

## Conformance and limitations

`tests/proof_format_conformance.rs` is a small executable reference encoder
for `Observation` and `Derive`, plus fixed hash vectors.  It checks canonical
ordering, tamper rejection, version labels, and domain separation against the
public producer/checker API.  It is intentionally not a second implementation
of every semantic certificate rule.  The current test suite therefore proves
wire stability and the checker boundary for representative operations; it
does not prove a formal soundness theorem, provide a parser, or replace
property/fuzz testing of every operation variant.
