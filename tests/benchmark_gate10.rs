use std::process::Command;

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
    let line = stdout
        .lines()
        .find(|line| line.contains("\"kind\":\"measurement\""))
        .unwrap_or_else(|| panic!("benchmark emitted no measurement for {workload}"))
        .to_owned();
    assert!(line.starts_with('{') && line.ends_with('}'), "{line}");
    assert_eq!(line.matches("\"schema\":").count(), 1, "{line}");
    assert_eq!(line.matches("\"workload\":").count(), 1, "{line}");
    assert!(
        line.contains(&format!("\"workload\":\"{workload}\"")),
        "benchmark returned the wrong workload: {line}"
    );
    line
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
}

#[test]
fn unsupported_domain_shapes_stay_explicitly_shape_only() {
    for workload in [
        "multi-currency",
        "corporate-actions",
        "ownership-network",
        "adversarial-recursion",
    ] {
        let line = measurement(workload);
        assert!(line.contains("\"status\":\"shape_only\""), "{line}");
        assert!(line.contains("\"semantic_supported\":false"), "{line}");
        assert!(!line.contains("\"unsupported_reason\":null"), "{line}");
    }
}

#[test]
fn revision_workloads_report_incremental_timing_and_invalidation() {
    for (workload, changed_kind) in [
        ("one-row-close-change", "evidence_row"),
        ("package-upgrade", "package"),
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
        assert!(
            !line.contains("\"invalidated_queries\":null")
                && !line.contains("\"invalidated_queries\":0"),
            "revision invalidation is missing for {workload}: {line}"
        );
        assert!(
            line.contains("changed_incremental_solve_ns times warm revision commit plus analysis")
                && line.contains("changed_full_solve_ns times clean recomputation"),
            "revision timing semantics are not documented in the measurement: {line}"
        );
    }
}

#[test]
fn peak_rss_is_isolated_per_workload_and_schema_stays_stable() {
    let first = measurement("invoice-payment-graph");
    let second = measurement("invoice-payment-graph");
    for line in [&first, &second] {
        assert!(
            line.contains("\"resource_profile\":\"per-workload child-process peak RSS"),
            "RSS provenance is missing: {line}"
        );
        assert!(
            line.contains("\"peak_memory_bytes\":") && !line.contains("\"peak_memory_bytes\":null"),
            "isolated RSS was not measured: {line}"
        );
        assert!(
            line.contains("isolated workload child-process peak RSS"),
            "RSS note does not identify the measurement boundary: {line}"
        );
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
