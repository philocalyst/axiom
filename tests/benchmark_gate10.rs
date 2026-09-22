use std::process::Command;

use serde_json::Value;

fn assert_json_object(line: &str) {
    let bytes = line.as_bytes();
    assert!(
        bytes.first() == Some(&b'{') && bytes.last() == Some(&b'}'),
        "{line}"
    );

    let mut object_depth = 0i32;
    let mut array_depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    for byte in bytes {
        if in_string {
            if escaped {
                escaped = false;
            } else if *byte == b'\\' {
                escaped = true;
            } else if *byte == b'"' {
                in_string = false;
            } else {
                assert!(*byte >= 0x20, "control byte in JSON string: {line}");
            }
            continue;
        }

        match *byte {
            b'"' => in_string = true,
            b'{' => object_depth += 1,
            b'}' => {
                object_depth -= 1;
                assert!(object_depth >= 0, "unbalanced JSON object: {line}");
            }
            b'[' => array_depth += 1,
            b']' => {
                array_depth -= 1;
                assert!(array_depth >= 0, "unbalanced JSON array: {line}");
            }
            _ => {}
        }
    }
    assert!(!in_string && !escaped, "unterminated JSON string: {line}");
    assert_eq!(object_depth, 0, "unbalanced JSON object: {line}");
    assert_eq!(array_depth, 0, "unbalanced JSON array: {line}");
}

fn assert_field_once(line: &str, field: &str) {
    let needle = format!("\"{field}\":");
    assert_eq!(
        line.matches(&needle).count(),
        1,
        "missing/duplicate {field}: {line}"
    );
}

fn field_value<'a>(line: &'a str, field: &str) -> &'a str {
    let needle = format!("\"{field}\":");
    let value = line
        .split_once(&needle)
        .and_then(|(_, rest)| rest.split_once([',', '}']).map(|(value, _)| value))
        .unwrap_or_else(|| panic!("missing field {field}: {line}"));
    value.trim()
}

fn parsed_json(line: &str) -> Value {
    let value: Value = serde_json::from_str(line).expect("benchmark line must be valid JSON");
    let object = value
        .as_object()
        .expect("measurement must be a JSON object");
    assert_eq!(
        object.get("schema").and_then(Value::as_str),
        Some("axiom-bench/v1")
    );
    assert!(object.get("workload").and_then(Value::as_str).is_some());
    assert!(object.get("quick").and_then(Value::as_bool).is_some());
    assert!(object.get("scale").and_then(Value::as_u64).is_some());
    assert!(object.get("samples").and_then(Value::as_u64).is_some());
    assert!(object.get("status").and_then(Value::as_str).is_some());
    assert!(
        object
            .get("semantic_supported")
            .and_then(Value::as_bool)
            .is_some()
    );
    assert!(object.get("source_hash").and_then(Value::as_str).is_some());
    assert!(object.get("targets").and_then(Value::as_object).is_some());
    assert!(
        object
            .get("measurements")
            .and_then(Value::as_object)
            .is_some()
    );
    assert!(object.get("sizes").and_then(Value::as_object).is_some());
    assert!(object.get("metrics").and_then(Value::as_object).is_some());
    value
}

fn measurement(workload: &str) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_axiom-bench"))
        .args(["--quick", "--workload", workload])
        .output()
        .expect("axiom-bench should start");
    assert!(
        output.status.success(),
        "benchmark failed for {workload}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !String::from_utf8_lossy(&output.stderr).contains("\"kind\":\"rss_probe\""),
        "RSS child protocol leaked into the human table: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("benchmark JSON is UTF-8");
    let lines = stdout.lines().collect::<Vec<_>>();
    assert_eq!(
        lines.len(),
        1,
        "benchmark must emit exactly one JSONL record: {stdout}"
    );
    let line = lines
        .first()
        .copied()
        .unwrap_or_else(|| panic!("benchmark emitted no measurement for {workload}"));
    assert_json_object(line);
    let parsed = parsed_json(line);
    for field in [
        "schema",
        "workload",
        "description",
        "quick",
        "scale",
        "samples",
        "status",
        "semantic_supported",
        "source_hash",
        "changed_source_hash",
        "changed_kind",
        "targets",
        "measurements",
        "sizes",
        "metrics",
    ] {
        assert_field_once(line, field);
    }
    for field in [
        "settlement_setup_ns",
        "settlement_document_projection_ns",
        "settlement_projection_call_ns",
        "settlement_persistence_boundary_ns",
        "settlement_proof_check_ns",
        "settlement_store_verify_ns",
        "settlement_source_revision_ns",
        "settlement_coverage_count",
        "settlement_coverage_hash",
        "settlement_source_commit_hash",
        "settlement_artifact_id_hash",
        "settlement_artifact_hash",
        "settlement_proof_hash",
        "settlement_proof_bytes",
        "settlement_projection_commit_hash",
        "settlement_binding_verified",
        "settlement_proof_check_verified",
        "settlement_store_verify_verified",
        "settlement_source_revision_verified",
        "settlement_atomic_negative_verified",
        "execution_mode",
        "provenance_path",
        "batch_count",
        "batch_size",
        "max_batch_forms",
        "authority_binding_checks",
    ] {
        assert_field_once(line, field);
    }
    // One kind identifies this record and the nested target kind identifies
    // the engineering goal; both are required and neither may be duplicated.
    assert_eq!(
        line.matches("\"kind\":").count(),
        2,
        "missing/duplicate kind: {line}"
    );
    assert!(line.contains("\"schema\":\"axiom-bench/v1\""), "{line}");
    assert!(line.contains("\"kind\":\"measurement\""), "{line}");
    assert!(
        line.contains(&format!("\"workload\":\"{workload}\"")),
        "benchmark returned the wrong workload: {line}"
    );
    let metrics = parsed["metrics"].as_object().expect("metrics object");
    for field in [
        "peak_memory_bytes",
        "batch_count",
        "batch_size",
        "max_batch_forms",
    ] {
        assert!(
            metrics.get(field).is_some(),
            "missing typed metric {field}: {line}"
        );
        assert!(
            metrics[field].is_number() || metrics[field].is_null(),
            "metric {field} is not numeric/null: {line}"
        );
    }
    line.to_owned()
}

#[test]
fn independent_worker_probe_reports_equivalent_serial_and_concurrent_results() {
    for workload in ["high-frequency-lots", "invoice-payment-graph"] {
        let line = measurement(workload);
        assert!(
            line.contains("\"independent_worker_determinism\":true"),
            "independent-worker determinism was not established for {workload}: {line}"
        );
        assert!(
            line.contains("\"independent_worker_equivalence\":true"),
            "worker equivalence was not established for {workload}: {line}"
        );
        assert!(
            line.contains("\"concurrent_worker_count\":2")
                && !line.contains("\"independent_workers_ns\":null"),
            "independent-worker timing metadata is missing for {workload}: {line}"
        );
        assert!(line.contains("\"parallel_solve_ns\":null"), "{line}");
        assert!(
            line.contains("\"determinism_across_thread_counts\":null"),
            "{line}"
        );
        assert!(line.contains("\"thread_count_equivalence\":null"), "{line}");
        assert!(line.contains("\"parallel_thread_count\":null"), "{line}");
        assert!(line.contains("\"semantic_relation_nodes\":null"), "{line}");
        assert!(line.contains("\"semantic_relation_edges\":null"), "{line}");
        assert!(!line.contains("\"dependency_graph_nodes\":null"), "{line}");
        assert!(!line.contains("\"dependency_graph_edges\":null"), "{line}");
    }
}

#[test]
fn invoice_graph_uses_supported_semantic_forms() {
    let line = measurement("invoice-payment-graph");
    assert!(line.contains("\"status\":\"measured\""), "{line}");
    assert!(line.contains("\"semantic_supported\":true"), "{line}");
    assert!(line.contains("\"unsupported_reason\":null"), "{line}");
    assert!(!line.contains("\"explanation_bytes\":null"), "{line}");
    assert_eq!(field_value(&line, "samples"), "1", "{line}");
    assert_eq!(field_value(&line, "scale"), "1", "{line}");
}

#[test]
fn domain_api_workloads_report_real_semantic_probes() {
    for (workload, api) in [
        ("multi-currency", "ontology.exchange_unit_validation"),
        ("corporate-actions", "contracts.corporate_action_validation"),
        (
            "invoice-payment-graph",
            "ontology.satisfaction_network_validation",
        ),
        ("ownership-network", "ontology.role_assignment_validation"),
        (
            "adversarial-recursion",
            "logic.positive_fixed_point_validation",
        ),
    ] {
        let line = measurement(workload);
        assert!(line.contains("\"status\":\"measured\""), "{line}");
        assert!(line.contains("\"semantic_supported\":true"), "{line}");
        assert!(line.contains("\"unsupported_reason\":null"), "{line}");
        assert!(
            line.contains("\"semantic_probe_ns\":") && !line.contains("\"semantic_probe_ns\":null"),
            "semantic probe timing is missing for {workload}: {line}"
        );
        assert!(
            line.contains(&format!("\"semantic_probe_api\":\"{api}\"")),
            "semantic probe API is missing for {workload}: {line}"
        );
        assert!(
            !line.contains("shape-only") && !line.contains("shape_only"),
            "domain workload still carries a projection-only label: {line}"
        );
        assert!(
            field_value(&line, "semantic_probe_items")
                .parse::<u64>()
                .expect("semantic_probe_items is numeric")
                > 0
                && field_value(&line, "semantic_probe_results")
                    .parse::<u64>()
                    .expect("semantic_probe_results is numeric")
                    > 0,
            "semantic probe did not produce validated results: {line}"
        );
    }
}

#[test]
fn generic_form_workload_uses_pinned_document_elaboration_and_canonical_values() {
    let line = measurement("generic-form-elaboration");
    assert!(line.contains("\"status\":\"measured\""), "{line}");
    assert!(line.contains("\"semantic_supported\":true"), "{line}");
    assert!(
        line.contains("\"semantic_probe_api\":\"surface.package.document_elaboration\""),
        "{line}"
    );
    assert!(
        line.contains("Workspace::elaborate_package_forms"),
        "{line}"
    );
    assert_eq!(
        field_value(&line, "authority_binding_verified"),
        "true",
        "{line}"
    );
    assert_eq!(field_value(&line, "forms"), "1000", "{line}");
    for field in [
        "package_compile_ns",
        "document_elaboration_ns",
        "schema_lookup_ns",
        "canonical_values_ns",
        "changed_document_elaboration_ns",
    ] {
        assert_ne!(field_value(&line, field), "null", "{field}: {line}");
    }
    assert_eq!(
        field_value(&line, "canonical_value_count"),
        "1000",
        "{line}"
    );
    assert_eq!(field_value(&line, "schema_lookup_count"), "1000", "{line}");
    assert_eq!(
        field_value(&line, "document_result_count"),
        "1000",
        "{line}"
    );
    assert_eq!(
        field_value(&line, "revision_result_count"),
        "1000",
        "{line}"
    );
    assert!(
        line.contains("\"revision_mode\":\"bounded_batch_workspace_re_elaboration\""),
        "{line}"
    );
    assert!(line.contains("\"changed_kind\":\"evidence_row\""), "{line}");
    assert!(
        line.contains("\"changed_incremental_solve_ns\":null"),
        "{line}"
    );
    assert!(line.contains("\"changed_full_solve_ns\":null"), "{line}");
    assert!(
        line.contains("\"independent_clean_solve_ns\":null"),
        "{line}"
    );
    assert!(
        line.contains("\"same_process_cache_replay_equal\":null"),
        "{line}"
    );
    assert!(
        line.contains("\"independent_clean_recompute_equal\":null"),
        "{line}"
    );
    let source_hash = field_value(&line, "source_hash");
    let changed_hash = field_value(&line, "changed_source_hash");
    assert_ne!(
        source_hash, changed_hash,
        "generic source edit did not change input"
    );
}

#[test]
fn settlement_state_proof_workload_uses_public_persisted_path_and_oracle() {
    let line = measurement("settlement-state-proof");
    assert!(line.contains("\"status\":\"measured\""), "{line}");
    assert!(line.contains("\"semantic_supported\":true"), "{line}");
    assert!(line.contains("\"semantic_probe_api\":null"), "{line}");
    assert!(line.contains("\"semantic_probe_items\":null"), "{line}");
    assert!(line.contains("\"semantic_probe_results\":null"), "{line}");
    assert_eq!(field_value(&line, "forms"), "1", "{line}");
    assert_eq!(
        field_value(&line, "settlement_coverage_count"),
        "1",
        "{line}"
    );
    assert_eq!(field_value(&line, "package_compile_ns"), "null", "{line}");
    assert!(!line.contains("\"settlement_projection_ns\":"), "{line}");
    for field in [
        "document_elaboration_ns",
        "settlement_setup_ns",
        "settlement_document_projection_ns",
        "settlement_projection_call_ns",
        "settlement_persistence_boundary_ns",
        "settlement_proof_check_ns",
        "settlement_store_verify_ns",
        "settlement_source_revision_ns",
        "settlement_coverage_hash",
        "settlement_source_commit_hash",
        "settlement_artifact_id_hash",
        "settlement_artifact_hash",
        "settlement_proof_hash",
        "settlement_proof_bytes",
        "settlement_projection_commit_hash",
    ] {
        assert_ne!(field_value(&line, field), "null", "{field}: {line}");
    }
    for field in [
        "settlement_binding_verified",
        "settlement_proof_check_verified",
        "settlement_store_verify_verified",
        "settlement_source_revision_verified",
        "settlement_atomic_negative_verified",
        "independent_clean_recompute_equal",
        "authority_binding_verified",
    ] {
        assert_eq!(field_value(&line, field), "true", "{field}: {line}");
    }
    assert!(line.contains("\"changed_kind\":\"evidence_row\""), "{line}");
    assert!(line.contains("\"settlement_coverage_hash\":\""), "{line}");
    assert!(line.contains("\"settlement_proof_bytes\":"), "{line}");
    let value = parsed_json(&line);
    assert_eq!(
        value["measurements"]["settlement_document_projection_ns"],
        value["measurements"]["settlement_projection_call_ns"],
        "deprecated timing alias diverged: {line}"
    );
}

#[test]
fn settlement_compact_surface_reports_v2_provenance_without_masquerading_as_v1() {
    let output = Command::new(env!("CARGO_BIN_EXE_axiom-bench"))
        .args([
            "--quick",
            "--workload",
            "settlement-state-proof",
            "--settlement-surface",
            "compact-v1",
        ])
        .output()
        .expect("compact settlement benchmark should start");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let line = String::from_utf8(output.stdout).expect("benchmark JSON is UTF-8");
    let value = parsed_json(line.trim());
    let metrics = &value["metrics"];
    assert_eq!(
        metrics["settlement_surface_profile"].as_str(),
        Some("compact_v1_proof_v2")
    );
    assert_eq!(
        metrics["settlement_proof_version"].as_str(),
        Some("axiom/settlement-state-proof/v2")
    );
    assert_eq!(
        metrics["settlement_proof_check_verified"].as_bool(),
        Some(true)
    );
    assert_eq!(
        metrics["settlement_store_verify_verified"].as_bool(),
        Some(true)
    );
    assert_eq!(
        metrics["settlement_source_revision_verified"].as_bool(),
        Some(true)
    );
    assert_eq!(
        metrics["settlement_atomic_negative_verified"].as_bool(),
        Some(true)
    );
    assert!(metrics["settlement_proof_bytes"].as_u64().unwrap_or(0) > 0);
    assert!(metrics["settlement_artifact_hash"].is_string());
    assert!(metrics["settlement_batch_artifacts_hash"].is_string());
}

#[test]
fn settlement_probe_outputs_identify_direct_and_compact_authority() {
    for (compact, profile, version) in [
        (false, "direct_v1", "axiom/settlement-state-proof/v1"),
        (
            true,
            "compact_v1_proof_v2",
            "axiom/settlement-state-proof/v2",
        ),
    ] {
        let mut rss_args = vec![
            "--quick".to_owned(),
            "--rss-probe".to_owned(),
            "--workload".to_owned(),
            "settlement-state-proof".to_owned(),
        ];
        let mut boundary_args = vec![
            "--quick".to_owned(),
            "--settlement-boundary-probe".to_owned(),
            "--workload".to_owned(),
            "settlement-state-proof".to_owned(),
        ];
        if compact {
            rss_args.extend(["--settlement-surface".to_owned(), "compact-v1".to_owned()]);
            boundary_args.extend(["--settlement-surface".to_owned(), "compact-v1".to_owned()]);
        }
        for args in [rss_args, boundary_args] {
            let output = Command::new(env!("CARGO_BIN_EXE_axiom-bench"))
                .args(args)
                .output()
                .expect("settlement probe should start");
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let value: Value = serde_json::from_slice(&output.stdout).expect("probe JSON");
            assert_eq!(value["settlement_surface_profile"].as_str(), Some(profile));
            assert_eq!(value["settlement_proof_version"].as_str(), Some(version));
        }
    }
}

#[test]
fn settlement_surface_flag_rejects_unrelated_and_self_test_invocations() {
    for (args, expected) in [
        (
            vec!["--quick", "--settlement-surface", "compact-v1"],
            "--settlement-surface requires --workload settlement-state-proof",
        ),
        (
            vec![
                "--quick",
                "--workload",
                "invoice-payment-graph",
                "--settlement-surface",
                "compact-v1",
            ],
            "--settlement-surface requires --workload settlement-state-proof",
        ),
        (
            vec![
                "--self-test",
                "--workload",
                "settlement-state-proof",
                "--settlement-surface",
                "compact-v1",
            ],
            "--settlement-surface cannot be combined with --self-test",
        ),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_axiom-bench"))
            .args(args)
            .output()
            .expect("benchmark should start");
        assert!(!output.status.success());
        assert_eq!(
            String::from_utf8_lossy(&output.stderr).trim(),
            format!("axiom-bench: {expected}")
        );
    }
}

#[test]
#[ignore = "release stress gate for the discovered public settlement-persistence boundary"]
fn settlement_state_proof_scale_boundary_discovers_maximum() {
    fn run(scale: u64) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_axiom-bench"))
            .args([
                "--quick",
                "--scale",
                &scale.to_string(),
                "--workload",
                "settlement-state-proof",
                "--settlement-boundary-probe",
            ])
            .output()
            .expect("settlement benchmark should start")
    }

    // The row cap is only an architectural upper bound.  The public
    // persistence path also enforces the canonical proof-byte limit, so find
    // the actual accepted boundary instead of assuming that 4,096 rows fit.
    let mut low = 1_u64;
    let mut high = 4096_u64;
    while low < high {
        let middle = low + (high - low).div_ceil(2);
        if run(middle).status.success() {
            low = middle;
        } else {
            high = middle - 1;
        }
    }
    let accepted = run(low);
    assert!(
        accepted.status.success(),
        "discovered boundary {low} failed: {}",
        String::from_utf8_lossy(&accepted.stderr)
    );
    assert_eq!(low, 3309, "unexpected discovered canonical-byte boundary");

    let expected = "axiom-bench: settlement proof persistence failed: workspace settlement proof error: invalid SettlementStateV1 proof: resource limit: canonical proof payload is too large";
    let rejected = run(low + 1);
    assert_eq!(low + 1, 3310);
    assert!(!rejected.status.success());
    assert_eq!(String::from_utf8_lossy(&rejected.stderr).trim(), expected);

    // This is intentionally a public-persistence rejection as well; the
    // benchmark has no local row preflight that could mask the authority.
    let architectural_limit = run(4096);
    assert!(!architectural_limit.status.success());
    assert_eq!(
        String::from_utf8_lossy(&architectural_limit.stderr).trim(),
        expected
    );
}

#[test]
#[ignore = "release stress gate for the explicit 10k and 100k generic-form profiles"]
fn generic_form_scale_profiles_complete() {
    for (scale, expected) in [("10", "10000"), ("100", "100000")] {
        let output = Command::new(env!("CARGO_BIN_EXE_axiom-bench"))
            .args([
                "--quick",
                "--scale",
                scale,
                "--workload",
                "generic-form-elaboration",
            ])
            .output()
            .expect("scaled generic-form benchmark should start");
        assert!(
            output.status.success(),
            "scale {scale} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let line = String::from_utf8(output.stdout).expect("benchmark JSON is UTF-8");
        assert_eq!(field_value(line.trim(), "forms"), expected, "{line}");
        assert_eq!(
            field_value(line.trim(), "authority_binding_verified"),
            "true",
            "{line}"
        );
        assert_eq!(
            field_value(line.trim(), "execution_mode"),
            "\"bounded_batches\"",
            "{line}"
        );
        assert_eq!(field_value(line.trim(), "batch_size"), "256", "{line}");
        assert!(
            field_value(line.trim(), "max_batch_forms")
                .parse::<u64>()
                .expect("max batch forms")
                <= 256,
            "batch bound exceeded: {line}"
        );
        let peak = field_value(line.trim(), "peak_memory_bytes")
            .parse::<u64>()
            .expect("peak RSS");
        assert!(peak < 128 * 1024 * 1024, "bounded RSS regression: {line}");
    }
}

#[test]
fn bounded_generic_batches_have_structural_scale_and_provenance_guards() {
    let output = Command::new(env!("CARGO_BIN_EXE_axiom-bench"))
        .args([
            "--quick",
            "--scale",
            "2",
            "--workload",
            "generic-form-elaboration",
        ])
        .output()
        .expect("bounded generic benchmark should start");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let line = String::from_utf8(output.stdout).expect("benchmark JSON is UTF-8");
    let value = parsed_json(line.trim());
    assert_eq!(value["sizes"]["forms"].as_u64(), Some(2000));
    assert_eq!(value["metrics"]["batch_count"].as_u64(), Some(8));
    assert_eq!(value["metrics"]["batch_size"].as_u64(), Some(256));
    assert_eq!(value["metrics"]["max_batch_forms"].as_u64(), Some(256));
    assert_eq!(
        value["metrics"]["provenance_path"].as_str(),
        Some("Workspace::elaborate_package_forms")
    );
    assert_eq!(
        value["metrics"]["authority_binding_verified"].as_bool(),
        Some(true)
    );
    assert_eq!(
        value["metrics"]["authority_binding_checks"].as_u64(),
        Some(8)
    );
}

#[test]
fn bounded_settlement_batches_do_not_masquerade_as_one_proof() {
    let output = Command::new(env!("CARGO_BIN_EXE_axiom-bench"))
        .args([
            "--quick",
            "--scale",
            "300",
            "--workload",
            "settlement-state-proof",
        ])
        .output()
        .expect("bounded settlement benchmark should start");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let line = String::from_utf8(output.stdout).expect("benchmark JSON is UTF-8");
    let value = parsed_json(line.trim());
    let metrics = &value["metrics"];
    assert_eq!(value["sizes"]["forms"].as_u64(), Some(300));
    assert_eq!(metrics["batch_count"].as_u64(), Some(2));
    assert_eq!(metrics["max_batch_forms"].as_u64(), Some(256));
    assert!(metrics["settlement_proof_hash"].is_null());
    assert!(metrics["settlement_source_commit_hash"].is_null());
    assert!(metrics["settlement_batch_proofs_hash"].is_string());
    assert!(metrics["settlement_batch_source_commits_hash"].is_string());
    assert_eq!(
        metrics["settlement_source_revision_verified"].as_bool(),
        Some(true)
    );
    assert_eq!(
        metrics["settlement_atomic_negative_verified"].as_bool(),
        Some(true)
    );
}

#[test]
fn revision_workloads_report_incremental_timing_and_invalidation() {
    for (workload, changed_kind, api) in [
        ("one-row-close-change", "evidence_row", None),
        (
            "package-upgrade",
            "package",
            Some("package.versioned_selection_validation"),
        ),
    ] {
        let line = measurement(workload);
        assert!(
            line.contains(&format!("\"changed_kind\":\"{changed_kind}\"")),
            "{line}"
        );
        assert!(
            !line.contains("\"changed_incremental_solve_ns\":null")
                && !line.contains("\"changed_full_solve_ns\":null"),
            "revision timing is incomplete for {workload}: {line}"
        );
        if let Some(api) = api {
            assert!(
                !line.contains("\"semantic_probe_ns\":null")
                    && line.contains(&format!("\"semantic_probe_api\":\"{api}\"")),
                "revision semantic probe is missing for {workload}: {line}"
            );
            assert!(
                field_value(&line, "semantic_probe_items")
                    .parse::<u64>()
                    .expect("semantic_probe_items is numeric")
                    > 0
                    && field_value(&line, "semantic_probe_results")
                        .parse::<u64>()
                        .expect("semantic_probe_results is numeric")
                        > 0,
                "revision semantic probe did not produce validated results: {line}"
            );
        }
        assert!(
            !line.contains("\"invalidated_queries\":null")
                && !line.contains("\"invalidated_queries\":0"),
            "revision invalidation is missing for {workload}: {line}"
        );
        assert!(
            line.contains(
                "changed_incremental_solve_ns times the warm input update/commit plus analysis"
            ) && line.contains("changed_full_solve_ns times clean recomputation"),
            "revision timing semantics are not documented in the measurement: {line}"
        );
        assert!(
            field_value(&line, "changed_incremental_solve_ns") != "null",
            "{line}"
        );
        assert!(
            field_value(&line, "changed_full_solve_ns") != "null",
            "{line}"
        );
        assert!(
            field_value(&line, "invalidated_queries")
                .parse::<u64>()
                .expect("invalidated_queries is numeric")
                > 0,
            "{line}"
        );
        if workload == "one-row-close-change" {
            assert!(
                !line.contains("\"changed_source_hash\":null")
                    && !line.contains("\"changed_source_bytes\":null"),
                "one-row revision must carry a changed source row: {line}"
            );
            let source_hash = field_value(&line, "source_hash");
            let changed_hash = field_value(&line, "changed_source_hash");
            assert_ne!(
                source_hash, changed_hash,
                "one-row revision did not replace input"
            );
        }
    }
}

#[test]
fn peak_rss_is_isolated_per_workload_and_schema_stays_stable() {
    for workload in ["invoice-payment-graph", "settlement-state-proof"] {
        let first = measurement(workload);
        let second = measurement(workload);
        for line in [&first, &second] {
            if cfg!(any(target_os = "linux", target_os = "macos")) {
                assert!(
                    field_value(line, "resource_profile")
                        == "\"per-workload child-process peak RSS via getrusage\"",
                    "RSS provenance is missing: {line}"
                );
                assert!(
                    line.contains("\"peak_memory_bytes\":")
                        && !line.contains("\"peak_memory_bytes\":null"),
                    "isolated RSS was not measured: {line}"
                );
            } else {
                assert!(line.contains("\"peak_memory_bytes\":null"));
                assert_eq!(
                    field_value(line, "resource_profile"),
                    "\"per-workload child-process peak RSS unavailable\""
                );
            }
        }
        // Generation is deterministic even though timings and RSS are
        // naturally observations and may differ between invocations.
        let hash = |line: &str| {
            line.split("\"source_hash\":\"")
                .nth(1)
                .and_then(|tail| tail.split('"').next())
                .expect("source hash")
                .to_owned()
        };
        assert_eq!(hash(&first), hash(&second));
        assert_eq!(first.matches("\"schema\":").count(), 1);
        assert_eq!(second.matches("\"schema\":").count(), 1);
    }
}

#[test]
fn full_corpus_self_test_is_not_replaced_by_quick_scaling() {
    let output = Command::new(env!("CARGO_BIN_EXE_axiom-bench"))
        .arg("--self-test")
        .output()
        .expect("full benchmark self-test should start");
    assert!(
        output.status.success(),
        "full benchmark self-test failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("self-test JSON is UTF-8");
    assert_eq!(stdout.lines().count(), 1, "{stdout}");
    assert_json_object(stdout.trim());
    assert!(stdout.contains("\"kind\":\"self_test\""), "{stdout}");
    assert!(stdout.contains("\"status\":\"ok\""), "{stdout}");
    assert!(stdout.contains("\"checks\":50"), "{stdout}");
}
