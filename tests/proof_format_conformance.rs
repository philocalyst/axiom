//! Executable conformance checks for `docs/proof-format-v1.md`.
//!
//! The encoder below is deliberately independent of `src/proof.rs`'s private
//! helpers.  It covers the envelope and two small operation variants, while
//! the production checker remains responsible for all certificate semantics.

use std::collections::BTreeMap;

use axiom_ledger::exact::Exact;
use axiom_ledger::model::ContentHash;
use axiom_ledger::proof::{CheckError, Node, Operation, Proof, ProofId, Statement};

fn put_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn put_bytes(out: &mut Vec<u8>, value: &[u8]) {
    put_u64(out, value.len() as u64);
    out.extend_from_slice(value);
}

fn put_string(out: &mut Vec<u8>, value: &str) {
    put_bytes(out, value.as_bytes());
}

fn put_id(out: &mut Vec<u8>, id: ProofId) {
    out.extend_from_slice(&id.0);
}

fn put_optional_id(out: &mut Vec<u8>, id: Option<ProofId>) {
    match id {
        Some(id) => {
            out.push(1);
            put_id(out, id);
        }
        None => out.push(0),
    }
}

fn independent_operation(out: &mut Vec<u8>, operation: &Operation) {
    match operation {
        Operation::Observation { source } => {
            put_bytes(out, b"observation");
            put_string(out, source);
        }
        Operation::CommitBinding(certificate) => {
            put_bytes(out, b"commit-binding");
            out.extend_from_slice(certificate.commit.as_bytes());
        }
        Operation::Derive { rule } => {
            put_bytes(out, b"derive");
            put_string(out, rule);
        }
        Operation::InventoryConservation {
            lot,
            lot_proof,
            before,
            consumed,
            after,
            unit,
            predecessor,
            allocation,
        } => {
            put_bytes(out, b"inventory-conservation");
            put_string(out, lot);
            put_id(out, *lot_proof);
            put_string(out, &before.canonical_string());
            put_string(out, &consumed.canonical_string());
            put_string(out, &after.canonical_string());
            put_string(out, unit);
            put_optional_id(out, *predecessor);
            put_optional_id(out, *allocation);
        }
        unsupported => {
            panic!("fixture operation is outside the independent subset: {unsupported:?}")
        }
    }
}

fn node_payload(
    statement: &Statement,
    operation: &Operation,
    inputs: &[ProofId],
    metadata: &BTreeMap<String, String>,
) -> Vec<u8> {
    let mut out = Vec::new();
    put_string(&mut out, statement.as_str());
    independent_operation(&mut out, operation);
    put_u64(&mut out, inputs.len() as u64);
    for input in inputs {
        put_id(&mut out, *input);
    }
    let metadata = metadata
        .iter()
        .filter(|(key, _)| key.as_str() != "location")
        .collect::<Vec<_>>();
    put_u64(&mut out, metadata.len() as u64);
    for (key, value) in metadata {
        put_string(&mut out, key);
        put_string(&mut out, value);
    }
    out
}

fn digest_domain(domain: &[u8], payload: &[u8]) -> ProofId {
    let mut bytes = Vec::new();
    put_bytes(&mut bytes, domain);
    bytes.extend_from_slice(payload);
    ProofId(*blake3::hash(&bytes).as_bytes())
}

fn independent_node_id(
    statement: &Statement,
    operation: &Operation,
    inputs: &[ProofId],
    metadata: &BTreeMap<String, String>,
) -> ProofId {
    digest_domain(
        b"axiom/proof-node/v1",
        &node_payload(statement, operation, inputs, metadata),
    )
}

fn independent_canonical_bytes(proof: &Proof, domain: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    put_bytes(&mut out, domain);
    put_u64(&mut out, proof.roots.len() as u64);
    for root in &proof.roots {
        put_id(&mut out, *root);
    }
    put_u64(&mut out, proof.nodes.len() as u64);
    for (map_key, node) in &proof.nodes {
        put_id(&mut out, *map_key);
        put_id(&mut out, node.id);
        put_string(&mut out, node.statement.as_str());
        independent_operation(&mut out, &node.operation);
        put_u64(&mut out, node.inputs.len() as u64);
        for input in &node.inputs {
            put_id(&mut out, *input);
        }
        put_u64(&mut out, node.metadata.len() as u64);
        for (key, value) in &node.metadata {
            put_string(&mut out, key);
            put_string(&mut out, value);
        }
    }
    out
}

fn independent_content_hash(proof: &Proof, domain: &[u8]) -> ProofId {
    let mut payload = Vec::new();
    put_u64(&mut payload, proof.roots.len() as u64);
    for root in &proof.roots {
        put_id(&mut payload, *root);
    }
    put_u64(&mut payload, proof.nodes.len() as u64);
    for (map_key, node) in &proof.nodes {
        put_id(&mut payload, *map_key);
        put_id(&mut payload, node.id);
    }
    digest_domain(domain, &payload)
}

fn metadata(items: &[(&str, &str)]) -> BTreeMap<String, String> {
    items
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect()
}

fn fixture(insert_reverse: bool) -> (Proof, ProofId, ProofId) {
    let leaf = Node::new(
        "leaf",
        Operation::Observation {
            source: "source/one".into(),
        },
        vec![],
        metadata(&[("location", "line 9"), ("z", "last"), ("a", "first")]),
    );
    let derived = Node::new(
        "answer",
        Operation::Derive {
            rule: "identity".into(),
        },
        vec![leaf.id],
        metadata(&[("proof-role", "answer")]),
    );
    let mut proof = Proof::new();
    if insert_reverse {
        proof.insert(leaf.clone());
        proof.insert(derived.clone());
    } else {
        proof.insert(derived.clone());
        proof.insert(leaf.clone());
    }
    proof.root(derived.id);
    (proof, leaf.id, derived.id)
}

#[test]
fn commit_binding_encoding_matches_independent_encoder() {
    let commit = ContentHash::domain_separated("test/commit", b"immutable-source");
    let node = Node::new(
        "source commit",
        Operation::CommitBinding(axiom_ledger::proof::CommitBindingCertificate { commit }),
        vec![],
        metadata(&[("location", "line 1")]),
    );
    let mut proof = Proof::new();
    let root = proof.insert(node);
    proof.root(root);
    assert!(proof.check().is_ok());
    assert_eq!(
        proof.canonical_bytes(),
        independent_canonical_bytes(&proof, b"axiom/canonical-proof/v1")
    );
    assert_eq!(
        proof.content_hash(),
        independent_content_hash(&proof, b"axiom/proof/v1")
    );
}

#[test]
fn v1_encoder_matches_an_independent_encoder_and_literal_vectors() {
    let (proof, leaf_id, _) = fixture(false);
    assert!(proof.check().is_ok());

    let leaf = proof.node(leaf_id).expect("fixture leaf");
    assert_eq!(
        leaf.id,
        independent_node_id(
            &leaf.statement,
            &leaf.operation,
            &leaf.inputs,
            &leaf.metadata
        )
    );
    assert_eq!(
        proof.canonical_bytes(),
        independent_canonical_bytes(&proof, b"axiom/canonical-proof/v1")
    );
    assert_eq!(
        proof.content_hash(),
        independent_content_hash(&proof, b"axiom/proof/v1")
    );

    // These vectors make an accidental field reorder or domain-label change
    // visible even if both the producer and this small reference encoder are
    // changed together in a future edit.
    assert_eq!(
        leaf.id.hex(),
        "46a3849d377e07e1889835d2d0dbe0bd84f1ed4e65866f5a5f6626412632985d"
    );
    assert_eq!(
        proof.content_hash().hex(),
        "d8ed6cfac4faf07a455fd160e6ac4a453f1789411ae478a5234433ae78a5946c"
    );
}

#[test]
fn optional_proof_ids_have_one_presence_marker() {
    let operation = Operation::InventoryConservation {
        lot: "lot/one".into(),
        lot_proof: ProofId([1; 32]),
        before: Exact::from(3_i64),
        consumed: Exact::from(1_i64),
        after: Exact::from(2_i64),
        unit: "ABC".into(),
        predecessor: Some(ProofId([2; 32])),
        allocation: None,
    };
    let node = Node::new("remaining lot", operation, vec![], BTreeMap::new());
    assert_eq!(
        node.id,
        independent_node_id(
            &node.statement,
            &node.operation,
            &node.inputs,
            &node.metadata,
        )
    );
}

#[test]
fn insertion_order_and_diagnostic_location_do_not_change_content_identity() {
    let (proof, leaf_id, _) = fixture(false);
    let (reordered, reordered_leaf_id, _) = fixture(true);
    assert_eq!(leaf_id, reordered_leaf_id);
    assert_eq!(proof, reordered);
    assert_eq!(proof.canonical_bytes(), reordered.canonical_bytes());
    assert_eq!(proof.content_hash(), reordered.content_hash());

    // `location` is deliberately excluded from node identity, but remains in
    // persisted canonical bytes as diagnostic data.
    let mut moved = proof.clone();
    moved
        .nodes
        .get_mut(&leaf_id)
        .expect("fixture leaf")
        .metadata
        .insert("location".into(), "line 900".into());
    assert!(moved.check().is_ok());
    assert_eq!(moved.content_hash(), proof.content_hash());
    assert_ne!(moved.canonical_bytes(), proof.canonical_bytes());
}

#[test]
fn checker_rejects_node_and_graph_tampering() {
    let (proof, _, answer_id) = fixture(false);

    let mut statement_tampered = proof.clone();
    statement_tampered
        .nodes
        .get_mut(&answer_id)
        .expect("fixture answer")
        .statement = Statement::new("forged answer");
    assert!(matches!(
        statement_tampered.check(),
        Err(CheckError::TamperedNode { id }) if id == answer_id
    ));

    let mut roots_tampered = proof;
    roots_tampered.roots.push(ProofId::ZERO);
    assert!(matches!(
        roots_tampered.check(),
        Err(CheckError::NonCanonicalRoots)
    ));
}

#[test]
fn version_and_hash_domains_are_separated() {
    let (proof, leaf_id, _) = fixture(false);
    let leaf = proof.node(leaf_id).expect("fixture leaf");

    let v1_bytes = independent_canonical_bytes(&proof, b"axiom/canonical-proof/v1");
    let v2_bytes = independent_canonical_bytes(&proof, b"axiom/canonical-proof/v2");
    assert_ne!(
        v1_bytes, v2_bytes,
        "version is part of the serialized envelope"
    );

    let v1_content = independent_content_hash(&proof, b"axiom/proof/v1");
    let v2_content = independent_content_hash(&proof, b"axiom/proof/v2");
    assert_eq!(proof.content_hash(), v1_content);
    assert_ne!(
        v1_content, v2_content,
        "version is part of the compact hash"
    );

    let node_payload = node_payload(
        &leaf.statement,
        &leaf.operation,
        &leaf.inputs,
        &leaf.metadata,
    );
    let node_domain = digest_domain(b"axiom/proof-node/v1", &node_payload);
    let proof_domain = digest_domain(b"axiom/proof/v1", &node_payload);
    assert_eq!(leaf.id, node_domain);
    assert_ne!(
        node_domain, proof_domain,
        "node and proof hash domains are distinct"
    );
    assert_ne!(
        proof.content_hash(),
        ProofId(*blake3::hash(&proof.canonical_bytes()).as_bytes())
    );
}
