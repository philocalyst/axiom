use std::process::Command;

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
        line.contains("\"revision_mode\":\"warm_surface_re_elaboration\""),
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
    }
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
    let first = measurement("invoice-payment-graph");
    let second = measurement("invoice-payment-graph");
    for line in [&first, &second] {
        if cfg!(any(target_os = "linux", target_os = "macos")) {
            assert!(
                line.contains("\"resource_profile\":\"per-workload child-process peak RSS"),
                "RSS provenance is missing: {line}"
            );
            assert!(
                line.contains("\"peak_memory_bytes\":")
                    && !line.contains("\"peak_memory_bytes\":null"),
                "isolated RSS was not measured: {line}"
            );
            assert!(
                line.contains("isolated workload child-process peak RSS"),
                "RSS note does not identify the measurement boundary: {line}"
            );
        } else {
            assert!(line.contains("\"peak_memory_bytes\":null"));
            assert!(line.contains("peak RSS unavailable"));
        }
    }
    // Generation is deterministic even though timings and RSS are naturally
    // observations and may differ between invocations.
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
    assert!(stdout.contains("\"checks\":45"), "{stdout}");
}
