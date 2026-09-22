//! Reproducible benchmark corpus for section XVII of confirmed-direction.md.
//!
//! The binary intentionally has no benchmark framework dependency. It emits
//! one JSON object per line on stdout and a compact human table on stderr.
//! Timings are observations from this process; engineering goals are emitted
//! separately and are never used as pass/fail performance assertions.

use std::env;
use std::fmt::Write as _;
use std::hint::black_box;
use std::process::Command;
use std::thread;
use std::time::Instant;

use axiom_ledger::contracts::{
    CorporateAction, Dividend, DividendLeg, Merge, Spinoff, SpinoffLeg, Split, TransformationLeg,
};
use axiom_ledger::elaboration::ElaboratedForm;
use axiom_ledger::hir::{
    AstDeclaration, AstDeclarationKind, AstModule, AstType, ModulePath, Name, QualifiedName,
    Span as HirSpan,
};
use axiom_ledger::incremental::{IncrementalDb, MemoOutcome, QueryError, QueryKey};
use axiom_ledger::ir::{Atom, Nominal, NominalKind, Term, Var};
use axiom_ledger::logic::{Clause, Goal, Literal, Program, SemanticContext, Solver, Truth};
use axiom_ledger::model::{ContentHash, Quantity, Unit};
use axiom_ledger::ontology::{
    Endpoint, ExchangeLeg, ExchangeRecord, Instrument, InstrumentKind, Obligation, Role,
    RoleAssignment, RoleAssignments, SatisfactionAllocation, Settlement, SettlementState,
    validate_satisfaction_network,
};
use axiom_ledger::package::{LotCandidate, PolicyPackage, Selection};
use axiom_ledger::package_compiler::{CompiledArtifact, PackageInput};
use axiom_ledger::package_lock::{
    Dependency, LockedPackage, Lockfile, PackageManifest, Version, VersionReq,
};
use axiom_ledger::render::render_why;
use axiom_ledger::store::PolicyPackage as StorePolicyPackage;
use axiom_ledger::surface::SurfaceFile;
use axiom_ledger::workspace::Workspace;

const SCHEMA: &str = "axiom-bench/v1";
const DEFAULT_SCALE: u64 = 1;
const FULL_SAMPLES: usize = 3;
const QUICK_SAMPLES: usize = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Workload {
    TenYearPersonalHistory,
    HighFrequencyLots,
    MultiCurrency,
    CorporateActions,
    InvoicePaymentGraph,
    OwnershipNetwork,
    ConflictingImports,
    OneRowCloseChange,
    PackageUpgrade,
    AdversarialRecursion,
    LargeProofExplanation,
    GenericFormElaboration,
}

impl Workload {
    const ALL: [Self; 12] = [
        Self::TenYearPersonalHistory,
        Self::HighFrequencyLots,
        Self::MultiCurrency,
        Self::CorporateActions,
        Self::InvoicePaymentGraph,
        Self::OwnershipNetwork,
        Self::ConflictingImports,
        Self::OneRowCloseChange,
        Self::PackageUpgrade,
        Self::AdversarialRecursion,
        Self::LargeProofExplanation,
        Self::GenericFormElaboration,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::TenYearPersonalHistory => "ten-year-personal-history",
            Self::HighFrequencyLots => "high-frequency-lots",
            Self::MultiCurrency => "multi-currency",
            Self::CorporateActions => "corporate-actions",
            Self::InvoicePaymentGraph => "invoice-payment-graph",
            Self::OwnershipNetwork => "ownership-network",
            Self::ConflictingImports => "conflicting-imports",
            Self::OneRowCloseChange => "one-row-close-change",
            Self::PackageUpgrade => "package-upgrade",
            Self::AdversarialRecursion => "adversarial-recursion",
            Self::LargeProofExplanation => "large-proof-explanation",
            Self::GenericFormElaboration => "generic-form-elaboration",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        let normalized = name.trim().to_ascii_lowercase().replace('_', "-");
        Self::ALL
            .into_iter()
            .find(|workload| workload.name() == normalized)
    }

    fn target(self) -> Target {
        match self {
            Self::TenYearPersonalHistory
            | Self::MultiCurrency
            | Self::ConflictingImports
            | Self::OneRowCloseChange
            | Self::PackageUpgrade => Target::WarmInteractive,
            Self::HighFrequencyLots
            | Self::CorporateActions
            | Self::InvoicePaymentGraph
            | Self::OwnershipNetwork
            | Self::AdversarialRecursion
            | Self::LargeProofExplanation => Target::IncrementalSeconds,
            Self::GenericFormElaboration => Target::WarmInteractive,
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::TenYearPersonalHistory => "ten years of monthly personal activity",
            Self::HighFrequencyLots => "many acquisitions competing for one sale",
            Self::MultiCurrency => {
                "multi-currency evidence plus ontology exchange and unit validation"
            }
            Self::CorporateActions => {
                "portfolio activity plus contract-validated corporate actions"
            }
            Self::InvoicePaymentGraph => "invoice and payment evidence graph",
            Self::OwnershipNetwork => "ownership evidence plus ontology role/share validation",
            Self::ConflictingImports => "contradictory imported observations",
            Self::OneRowCloseChange => "single evidence row changed near close",
            Self::PackageUpgrade => "policy package input changed between revisions",
            Self::AdversarialRecursion => {
                "recursive evidence plus logic fixed-point and cycle probes"
            }
            Self::LargeProofExplanation => "large proof DAG and source explanation",
            Self::GenericFormElaboration => {
                "package-bound generic record forms through surface/document elaboration"
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Target {
    WarmInteractive,
    IncrementalSeconds,
}

impl Target {
    fn json(self) -> &'static str {
        match self {
            Self::WarmInteractive => {
                "{\"kind\":\"engineering_goal\",\"warm_query_ms_max\":100,\"source\":\"confirmed-direction.md XVII\"}"
            }
            Self::IncrementalSeconds => {
                "{\"kind\":\"engineering_goal\",\"incremental_query_ms_max\":1000,\"source\":\"confirmed-direction.md XVII\"}"
            }
        }
    }

    fn human(self) -> &'static str {
        match self {
            Self::WarmInteractive => "warm <100 ms (goal)",
            Self::IncrementalSeconds => "incremental <1 s (goal)",
        }
    }
}

impl ChangedKind {
    fn json(self) -> &'static str {
        match self {
            Self::EvidenceRow => "evidence_row",
            Self::Package => "package",
        }
    }
}

#[derive(Clone, Debug)]
struct GeneratedWorkload {
    source: String,
    changed_source: Option<String>,
    changed_kind: Option<ChangedKind>,
    explain_goal: Option<String>,
    semantic_supported: bool,
    unsupported_reason: Option<&'static str>,
    semantic_probe: Option<SemanticProbeKind>,
    semantic_probe_items: usize,
}

/// A benchmark-domain probe exercises a real public semantic API in addition
/// to the source-ledger Workspace path.  The source grammar intentionally
/// remains small; these probes keep workloads honest when the domain API is
/// already available even though no source spelling exists yet.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SemanticProbeKind {
    CurrencyExchange,
    CorporateActions,
    InvoicePaymentGraph,
    OwnershipRoles,
    PackageUpgrade,
    RecursiveLogic,
}

impl SemanticProbeKind {
    fn name(self) -> &'static str {
        match self {
            Self::CurrencyExchange => "ontology.exchange_unit_validation",
            Self::CorporateActions => "contracts.corporate_action_validation",
            Self::InvoicePaymentGraph => "ontology.satisfaction_network_validation",
            Self::OwnershipRoles => "ontology.role_assignment_validation",
            Self::PackageUpgrade => "package.versioned_selection_validation",
            Self::RecursiveLogic => "logic.positive_fixed_point_validation",
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct SemanticProbeResult {
    items: usize,
    results: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ChangedKind {
    EvidenceRow,
    Package,
}

#[derive(Clone, Debug, Default)]
struct Timing {
    generation_ns: u128,
    normalization_ns: Option<u128>,
    parse_ns: Option<u128>,
    semantic_probe_ns: Option<u128>,
    solve_cold_ns: Option<u128>,
    independent_clean_solve_ns: Option<u128>,
    workspace_replay_ns: Option<u128>,
    proof_check_ns: Option<u128>,
    explanation_ns: Option<u128>,
    changed_incremental_solve_ns: Option<u128>,
    changed_full_solve_ns: Option<u128>,
    independent_workers_ns: Option<u128>,
    package_compile_ns: Option<u128>,
    document_elaboration_ns: Option<u128>,
    schema_lookup_ns: Option<u128>,
    canonical_values_ns: Option<u128>,
    changed_document_elaboration_ns: Option<u128>,
}

#[derive(Clone, Debug, Default)]
struct Sizes {
    source_bytes: usize,
    source_lines: usize,
    forms: Option<usize>,
    changed_source_bytes: Option<usize>,
    dependency_graph_nodes: Option<usize>,
    dependency_graph_edges: Option<usize>,
}

#[derive(Clone, Debug, Default)]
struct Metrics {
    cache_hits: Option<usize>,
    cache_misses: Option<usize>,
    invalidated_queries: Option<usize>,
    proof_nodes: Option<usize>,
    proof_roots: Option<usize>,
    semantic_dependency_edges: Option<usize>,
    semantic_invalidation_edges: Option<usize>,
    explanation_bytes: Option<usize>,
    cycle_errors: Option<usize>,
    independent_worker_determinism: Option<bool>,
    same_process_cache_replay_equal: Option<bool>,
    independent_clean_recompute_equal: Option<bool>,
    peak_memory_bytes: Option<usize>,
    independent_worker_equivalence: Option<bool>,
    concurrent_worker_count: Option<usize>,
    canonical_value_count: Option<usize>,
    schema_lookup_count: Option<usize>,
    document_result_count: Option<usize>,
    revision_result_count: Option<usize>,
    revision_mode: Option<&'static str>,
    authority_binding_verified: Option<bool>,
    semantic_probe_items: Option<usize>,
    semantic_probe_results: Option<usize>,
    semantic_probe_api: Option<&'static str>,
    build_profile: Option<&'static str>,
    resource_profile: Option<&'static str>,
    note: Option<&'static str>,
}

#[derive(Clone, Debug)]
struct ResultRecord {
    workload: Workload,
    quick: bool,
    scale: u64,
    samples: usize,
    source_hash: String,
    changed_source_hash: Option<String>,
    changed_kind: Option<ChangedKind>,
    semantic_supported: bool,
    status: &'static str,
    timing: Timing,
    sizes: Sizes,
    metrics: Metrics,
    unsupported_reason: Option<&'static str>,
    target: Target,
    description: &'static str,
}

#[derive(Clone, Debug)]
struct Options {
    quick: bool,
    scale: u64,
    workload: Option<Workload>,
    self_test: bool,
    help: bool,
    rss_probe: bool,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("axiom-bench: {error}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let options = Options::parse(env::args().skip(1))?;
    if options.help {
        print_help();
        return Ok(());
    }
    if options.rss_probe {
        let workload = options
            .workload
            .ok_or_else(|| "--rss-probe requires --workload".to_string())?;
        // One representative run keeps RSS a workload measurement rather
        // than the peak of a timing suite.
        let _ = measure_workload(workload, options.quick, options.scale, 1, false)?;
        let peak = process_peak_memory_bytes();
        println!(
            "{{\"schema\":\"{SCHEMA}\",\"kind\":\"rss_probe\",\"workload\":\"{}\",\"peak_memory_bytes\":{}}}",
            workload.name(),
            option_number(peak.map(|value| value as u128))
        );
        return Ok(());
    }
    if options.self_test {
        let checks = self_test(options.quick, options.scale)?;
        println!(
            "{{\"schema\":\"{SCHEMA}\",\"kind\":\"self_test\",\"status\":\"ok\",\"checks\":{checks}}}"
        );
        if options.workload.is_none() {
            return Ok(());
        }
    }

    let workloads: Vec<Workload> = match options.workload {
        Some(workload) => vec![workload],
        None => Workload::ALL.to_vec(),
    };
    let samples = if options.quick {
        QUICK_SAMPLES
    } else {
        FULL_SAMPLES
    };

    let mut records = Vec::with_capacity(workloads.len());
    for workload in workloads {
        let record = measure_workload(workload, options.quick, options.scale, samples, true)?;
        print_json_line(&record);
        records.push(record);
    }
    print_human_summary(&records);
    Ok(())
}

impl Options {
    fn parse(args: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut options = Self {
            quick: false,
            scale: DEFAULT_SCALE,
            workload: None,
            self_test: false,
            help: false,
            rss_probe: false,
        };
        let args: Vec<String> = args.collect();
        let mut index = 0;
        while index < args.len() {
            match args[index].as_str() {
                "--quick" => options.quick = true,
                "--self-test" => options.self_test = true,
                "--rss-probe" => options.rss_probe = true,
                "-h" | "--help" => options.help = true,
                "--scale" => {
                    index += 1;
                    let value = args
                        .get(index)
                        .ok_or_else(|| "--scale requires a positive integer".to_string())?;
                    options.scale = value
                        .parse::<u64>()
                        .map_err(|_| "--scale requires a positive integer".to_string())?;
                    if options.scale == 0 {
                        return Err("--scale must be greater than zero".into());
                    }
                }
                value if value.starts_with("--scale=") => {
                    let value = value.trim_start_matches("--scale=");
                    options.scale = value
                        .parse::<u64>()
                        .map_err(|_| "--scale requires a positive integer".to_string())?;
                    if options.scale == 0 {
                        return Err("--scale must be greater than zero".into());
                    }
                }
                "--workload" => {
                    index += 1;
                    let value = args
                        .get(index)
                        .ok_or_else(|| "--workload requires a name".to_string())?;
                    options.workload = Some(parse_workload(value)?);
                }
                value if value.starts_with("--workload=") => {
                    options.workload =
                        Some(parse_workload(value.trim_start_matches("--workload="))?);
                }
                value => return Err(format!("unknown argument {value} (try --help)")),
            }
            index += 1;
        }
        Ok(options)
    }
}

fn parse_workload(name: &str) -> Result<Workload, String> {
    if name == "all" {
        return Err("--workload all is the default; omit the option".into());
    }
    Workload::from_name(name).ok_or_else(|| {
        format!(
            "unknown workload {name}; choose one of {}",
            Workload::ALL
                .iter()
                .map(|workload| workload.name())
                .collect::<Vec<_>>()
                .join(", ")
        )
    })
}

fn print_help() {
    eprintln!(
        "axiom-bench [--quick] [--scale N] [--workload NAME] [--self-test]\n\n\
Generates the deterministic section XVII corpus. JSON Lines are written to stdout;\n\
the human summary is written to stderr. Timings are measurements, not assertions.\n\
--quick          one timing sample and workload-specific reduced row counts\n\
--scale N        multiply deterministic row counts (default: 1)\n\
--workload NAME  run one named workload (underscores are accepted)\n\
--self-test      run deterministic corpus/parser/proof/incremental checks"
    );
}

fn measure_workload(
    workload: Workload,
    quick: bool,
    scale: u64,
    samples: usize,
    isolate_peak_memory: bool,
) -> Result<ResultRecord, String> {
    let generated = generate(workload, quick, scale)?;
    if workload == Workload::GenericFormElaboration {
        return measure_generic_form_workload(
            generated,
            workload,
            quick,
            scale,
            samples,
            isolate_peak_memory,
        );
    }
    let source_hash = stable_hash(generated.source.as_bytes());
    let changed_source_hash = generated
        .changed_source
        .as_ref()
        .map(|source| stable_hash(source.as_bytes()));
    let mut timing = Timing {
        generation_ns: median_generation(workload, quick, scale, samples)?,
        ..Timing::default()
    };
    let mut sizes = Sizes {
        source_bytes: generated.source.len(),
        source_lines: generated.source.lines().count(),
        forms: None,
        changed_source_bytes: generated.changed_source.as_ref().map(String::len),
        dependency_graph_nodes: None,
        dependency_graph_edges: None,
    };
    let mut metrics = Metrics {
        independent_worker_determinism: None,
        same_process_cache_replay_equal: None,
        peak_memory_bytes: None,
        independent_worker_equivalence: None,
        semantic_probe_items: None,
        semantic_probe_results: None,
        semantic_probe_api: None,
        build_profile: Some(if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }),
        resource_profile: Some(if process_peak_memory_bytes().is_some() {
            "per-workload child-process peak RSS via getrusage"
        } else {
            "per-workload child-process peak RSS unavailable"
        }),
        note: Some(if process_peak_memory_bytes().is_some() {
            if generated.semantic_supported {
                "peak_memory_bytes is the isolated workload child-process peak RSS; semantic_probe_ns measures the separately reported public ontology/contracts/logic/store/package probe when semantic_probe_api is present; changed_incremental_solve_ns times the warm input update/commit plus analysis after the base analysis; changed_full_solve_ns times clean recomputation; independent-worker metrics compare serial and concurrent clean Workspace analyses, not a shared parallel engine"
            } else {
                "shape_only: parsed metrics describe only the accepted V0 evidence projection, not the richer domain shape; peak_memory_bytes is the isolated workload child-process peak RSS; changed_incremental_solve_ns times the warm input update/commit plus analysis after the base analysis; changed_full_solve_ns times clean recomputation; independent-worker metrics compare serial and concurrent clean Workspace analyses, not a shared parallel engine"
            }
        } else {
            if generated.semantic_supported {
                "peak RSS is unavailable on this platform; semantic_probe_ns measures the separately reported public ontology/contracts/logic/store/package probe when semantic_probe_api is present; changed_incremental_solve_ns times the warm input update/commit plus analysis after the base analysis; changed_full_solve_ns times clean recomputation; independent-worker metrics compare serial and concurrent clean Workspace analyses, not a shared parallel engine"
            } else {
                "shape_only: parsed metrics describe only the accepted V0 evidence projection, not the richer domain shape; peak RSS is unavailable on this platform; changed_incremental_solve_ns times the warm input update/commit plus analysis after the base analysis; changed_full_solve_ns times clean recomputation; independent-worker metrics compare serial and concurrent clean Workspace analyses, not a shared parallel engine"
            }
        }),
        ..Metrics::default()
    };

    if let Some(kind) = generated.semantic_probe {
        let start = Instant::now();
        let result = run_semantic_probe(kind, generated.semantic_probe_items)?;
        timing.semantic_probe_ns = Some(start.elapsed().as_nanos());
        metrics.semantic_probe_items = Some(result.items);
        metrics.semantic_probe_results = Some(result.results);
        metrics.semantic_probe_api = Some(kind.name());
    }

    let mut workspace = Workspace::new();
    let mut source = workspace
        .load_source(
            format!("benchmark/{}", workload.name()),
            generated.source.as_bytes(),
        )
        .map_err(|error| format!("{}: source load failed: {error}", workload.name()))?;
    if workload == Workload::PackageUpgrade {
        let package = workspace
            .put_policy_package(StorePolicyPackage::new(
                "lots/fifo",
                "1.0.0",
                b"selector=earliest_acquisition\ntie=ambiguous".to_vec(),
            ))
            .map_err(|error| format!("{}: package v1 failed: {error}", workload.name()))?;
        source = workspace
            .commit_with_packages(source.commit_id(), [package])
            .map_err(|error| format!("{}: package commit failed: {error}", workload.name()))?;
    }
    let elaborated = workspace
        .elaborate_commit(source.commit_id())
        .map_err(|error| format!("{}: parse failed: {error}", workload.name()))?;
    sizes.forms = Some(elaborated.forms.len());
    let analysis = workspace
        .analyze_commit(source.commit_id())
        .map_err(|error| format!("{}: analysis failed: {error}", workload.name()))?;
    analysis
        .check_proof()
        .map_err(|error| format!("{}: proof check failed: {error}", workload.name()))?;

    timing.normalization_ns = Some(median_normalization(&generated.source, samples)?);
    timing.parse_ns = Some(median_parse(&generated.source, samples)?);
    workspace.clear_incremental_trace();
    timing.solve_cold_ns = Some(median_cold_solve(workload, &generated.source, samples)?);
    timing.workspace_replay_ns = Some(median_workspace_replay(
        &mut workspace,
        source.commit_id(),
        samples,
    )?);
    let replay_metrics = workspace.incremental_metrics();
    let replay = workspace
        .analyze_commit(source.commit_id())
        .map_err(|error| format!("{}: deterministic replay failed: {error}", workload.name()))?;
    metrics.same_process_cache_replay_equal = Some(replay == analysis);
    let independent = independent_clean_analysis(
        workload,
        format!("benchmark/{}", workload.name()),
        &generated.source,
    )?;
    metrics.independent_clean_recompute_equal = Some(independent.analysis == *analysis);
    timing.independent_clean_solve_ns = Some(median_independent_clean_solve(
        workload,
        &generated.source,
        samples,
    )?);
    let (independent_workers_ns, workers_equivalent) =
        median_independent_worker_comparison(workload, &generated.source, samples)?;
    timing.independent_workers_ns = Some(independent_workers_ns);
    metrics.independent_worker_determinism = Some(workers_equivalent);
    metrics.independent_worker_equivalence = Some(workers_equivalent);
    metrics.concurrent_worker_count = Some(2);
    timing.proof_check_ns = Some(median_proof_check(&analysis, samples));
    metrics.proof_nodes = Some(analysis.proof.nodes.len());
    metrics.proof_roots = Some(analysis.proof.roots.len());
    metrics.semantic_dependency_edges =
        Some(analysis.dependencies.values().map(Vec::len).sum::<usize>());
    metrics.semantic_invalidation_edges =
        Some(analysis.invalidations.values().map(Vec::len).sum::<usize>());
    sizes.dependency_graph_nodes = Some(analysis.dependencies.len() + analysis.invalidations.len());
    sizes.dependency_graph_edges = Some(
        analysis.dependencies.values().map(Vec::len).sum::<usize>()
            + analysis.invalidations.values().map(Vec::len).sum::<usize>(),
    );

    if let Some(goal) = generated.explain_goal.as_deref() {
        let explanation = render_why(&analysis, goal)
            .map_err(|error| format!("{}: explanation failed: {error}", workload.name()))?;
        metrics.explanation_bytes = Some(explanation.len());
        timing.explanation_ns = Some(median_explanation(&analysis, goal, samples)?);
    }

    if let Some(changed) = generated.changed_source.as_deref() {
        let (changed_analysis, clean_store) = if workload == Workload::PackageUpgrade {
            workspace.clear_incremental_trace();
            let package = workspace
                .put_policy_package(StorePolicyPackage::new(
                    "lots/fifo",
                    "1.1.0",
                    b"selector=latest_acquisition\ntie=ambiguous".to_vec(),
                ))
                .map_err(|error| format!("{}: package v2 failed: {error}", workload.name()))?;
            let upgraded = workspace
                .commit_with_packages(source.commit_id(), [package])
                .map_err(|error| format!("{}: package upgrade failed: {error}", workload.name()))?;
            let clean_store = workspace.store().clone();
            let analysis = workspace
                .analyze_commit(upgraded.commit_id())
                .map_err(|error| {
                    format!("{}: incremental upgrade failed: {error}", workload.name())
                })?;
            (analysis, clean_store)
        } else {
            workspace.clear_incremental_trace();
            let changed_source = workspace
                .load_source(format!("benchmark/{}", workload.name()), changed.as_bytes())
                .map_err(|error| {
                    format!("{}: changed source load failed: {error}", workload.name())
                })?;
            let clean_store = workspace.store().clone();
            let analysis = workspace
                .analyze_commit(changed_source.commit_id())
                .map_err(|error| {
                    format!(
                        "{}: incremental changed solve failed: {error}",
                        workload.name()
                    )
                })?;
            (analysis, clean_store)
        };
        let changed_metrics = workspace.incremental_metrics();
        metrics.cache_hits = Some(replay_metrics.cache_hits + changed_metrics.cache_hits);
        metrics.cache_misses = Some(replay_metrics.cache_misses + changed_metrics.cache_misses);
        metrics.invalidated_queries = Some(changed_metrics.invalidated_queries);
        let mut clean_workspace = Workspace::from_store(clean_store);
        let clean = clean_workspace
            .analyze_commit(changed_analysis.source_commit())
            .map_err(|error| format!("{}: clean recomputation failed: {error}", workload.name()))?;
        if changed_analysis != clean {
            return Err(format!(
                "{}: incremental output differs from clean recomputation",
                workload.name()
            ));
        }
        timing.changed_incremental_solve_ns = Some(median_incremental_solve(
            workload,
            &generated.source,
            changed,
            samples,
        )?);
        timing.changed_full_solve_ns = Some(if workload == Workload::PackageUpgrade {
            median_package_upgrade_solve(&generated.source, samples)?
        } else {
            median_cold_solve(workload, changed, samples)?
        });
    } else if workload == Workload::AdversarialRecursion {
        metrics.cycle_errors = Some(recursive_cycle_probe()?);
    } else {
        metrics.cache_hits = Some(replay_metrics.cache_hits);
        metrics.cache_misses = Some(replay_metrics.cache_misses);
    }
    metrics.peak_memory_bytes = if isolate_peak_memory {
        isolated_peak_memory_bytes(workload, quick, scale)?
    } else {
        None
    };

    Ok(ResultRecord {
        workload,
        quick,
        scale,
        samples,
        source_hash,
        changed_source_hash,
        changed_kind: generated.changed_kind,
        semantic_supported: generated.semantic_supported,
        status: if generated.semantic_supported {
            "measured"
        } else {
            "shape_only"
        },
        timing,
        sizes,
        metrics,
        unsupported_reason: generated.unsupported_reason,
        target: workload.target(),
        description: workload.description(),
    })
}

/// Measure the real generic-form path separately from the strict ledger
/// parser.  Generic forms are intentionally authoring-surface input, so they
/// do not have a `book` declaration and cannot be routed through
/// `Workspace::analyze_commit` until the domain parser grows a generic-form
/// lowering.  This path still crosses the public package compiler and
/// `elaborate_document` boundary, and every reported result is schema-bound.
fn measure_generic_form_workload(
    generated: GeneratedWorkload,
    workload: Workload,
    quick: bool,
    scale: u64,
    samples: usize,
    isolate_peak_memory: bool,
) -> Result<ResultRecord, String> {
    let source_hash = stable_hash(generated.source.as_bytes());
    let changed_source_hash = generated
        .changed_source
        .as_ref()
        .map(|source| stable_hash(source.as_bytes()));
    let item_count = generated.semantic_probe_items;
    let (mut workspace, base_commit, artifact) = generic_workspace_fixture(&generated.source)?;
    let base_bound = workspace
        .elaborate_package_forms(base_commit)
        .map_err(|error| format!("{}: document elaboration failed: {error}", workload.name()))?;
    if base_bound.source_commit() != base_commit
        || base_bound.artifact_hash() != artifact.artifact_hash()
    {
        return Err(format!(
            "{}: elaborated forms lost their source/artifact authority binding",
            workload.name()
        ));
    }
    let base_forms = base_bound.into_forms();
    let package_root = artifact.package_roots()[0];
    let qualified_name = generic_form_schema_name()?;
    if base_forms.len() != item_count {
        return Err(format!(
            "{}: document elaboration returned {} forms, expected {item_count}",
            workload.name(),
            base_forms.len()
        ));
    }

    let document_elaboration_ns = median_workspace_document(&workspace, base_commit, samples)?;
    let schema_lookup_ns = median_generic_schema_lookup(
        &artifact,
        package_root,
        &qualified_name,
        item_count,
        samples,
    )?;
    let canonical_values_ns = median_generic_canonical_values(&base_forms, samples)?;
    let parse_ns = median_surface_parse(&generated.source, item_count, samples);
    let normalization_ns = median_generic_normalization(&generated.source, samples);
    let package_compile_ns = median_generic_package_compile(samples)?;

    let mut timing = Timing {
        generation_ns: median_generation(workload, quick, scale, samples)?,
        normalization_ns: Some(normalization_ns),
        parse_ns: Some(parse_ns),
        semantic_probe_ns: Some(document_elaboration_ns),
        package_compile_ns: Some(package_compile_ns),
        document_elaboration_ns: Some(document_elaboration_ns),
        schema_lookup_ns: Some(schema_lookup_ns),
        canonical_values_ns: Some(canonical_values_ns),
        changed_document_elaboration_ns: None,
        ..Timing::default()
    };
    let mut metrics = Metrics {
        same_process_cache_replay_equal: None,
        independent_clean_recompute_equal: None,
        semantic_probe_items: Some(item_count),
        semantic_probe_results: Some(base_forms.len()),
        semantic_probe_api: Some("surface.package.document_elaboration"),
        canonical_value_count: Some(base_forms.len()),
        schema_lookup_count: Some(item_count),
        document_result_count: Some(base_forms.len()),
        revision_result_count: None,
        revision_mode: None,
        authority_binding_verified: Some(true),
        build_profile: Some(if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }),
        resource_profile: Some(if process_peak_memory_bytes().is_some() {
            "per-workload child-process peak RSS via getrusage"
        } else {
            "per-workload child-process peak RSS unavailable"
        }),
        note: Some(
            "semantic_supported: package-compiled record schema + lossless SurfaceFile + Workspace::elaborate_package_forms on a commit pinned to a persisted artifact; package_compile_ns includes deterministic fixture construction, compilation, and object-store persistence; document_elaboration_ns includes typed schema validation and canonical value construction; schema_lookup_ns measures exact package-root schema resolution; canonical_values_ns rechecks canonical SchemaBoundRecord values; source revision timings are warm re-elaboration measurements because Workspace::analyze_commit does not yet lower generic forms and therefore are not incremental-cache claims",
        ),
        ..Metrics::default()
    };

    if let Some(changed) = generated.changed_source.as_deref() {
        let changed_commit = workspace
            .load_source("benchmark/generic-form-elaboration", changed.as_bytes())
            .map_err(|error| format!("{}: changed source load failed: {error}", workload.name()))?;
        let changed_bound = workspace
            .elaborate_package_forms(changed_commit.commit_id())
            .map_err(|error| {
                format!(
                    "{}: changed document elaboration failed: {error}",
                    workload.name()
                )
            })?;
        let changed_forms = changed_bound.forms();
        if changed_forms.len() != base_forms.len() {
            return Err(format!(
                "{}: source revision changed form count from {} to {}",
                workload.name(),
                base_forms.len(),
                changed_forms.len()
            ));
        }
        if base_forms
            .first()
            .zip(changed_forms.first())
            .is_some_and(|(left, right)| {
                left.value().content_hash() == right.value().content_hash()
            })
        {
            return Err(format!(
                "{}: source revision did not change a canonical form value",
                workload.name()
            ));
        }
        let changed_ns = median_generic_workspace_revision(&generated.source, changed, samples)?;
        timing.changed_document_elaboration_ns = Some(changed_ns);
        metrics.revision_result_count = Some(changed_forms.len());
        metrics.revision_mode = Some("warm_surface_re_elaboration");
    }

    metrics.peak_memory_bytes = if isolate_peak_memory {
        isolated_peak_memory_bytes(workload, quick, scale)?
    } else {
        None
    };

    Ok(ResultRecord {
        workload,
        quick,
        scale,
        samples,
        source_hash,
        changed_source_hash,
        changed_kind: generated.changed_kind,
        semantic_supported: true,
        status: "measured",
        timing,
        sizes: Sizes {
            source_bytes: generated.source.len(),
            source_lines: generated.source.lines().count(),
            forms: Some(base_forms.len()),
            changed_source_bytes: generated.changed_source.as_ref().map(String::len),
            dependency_graph_nodes: None,
            dependency_graph_edges: None,
        },
        metrics,
        unsupported_reason: None,
        target: workload.target(),
        description: workload.description(),
    })
}

fn generic_form_schema_name() -> Result<QualifiedName, String> {
    Ok(QualifiedName {
        module: ModulePath::root(Name::new("types").map_err(|error| format!("{error:?}"))?),
        name: Name::new("Row").map_err(|error| format!("{error:?}"))?,
    })
}

fn generic_form_package_input() -> Result<(PackageInput, Lockfile), String> {
    let manifest = PackageManifest::new(
        "forms",
        Version::new(1, 0, 0),
        "generic-record-benchmark-v1",
    );
    let lockfile = Lockfile {
        roots: vec![Dependency::new(
            manifest.name.clone(),
            VersionReq::Exact(manifest.version),
        )],
        packages: vec![LockedPackage {
            name: manifest.name.clone(),
            version: manifest.version,
            hash: manifest.hash(),
            dependencies: manifest.dependencies.clone(),
        }],
    };
    let module = axiom_ledger::hir::lower(AstModule {
        path: ModulePath::root(Name::new("types").map_err(|error| format!("{error:?}"))?),
        declarations: vec![AstDeclaration {
            name: "Row".to_owned(),
            kind: AstDeclarationKind::Type(AstType::Record {
                fields: vec![
                    ("approved".to_owned(), AstType::Bool),
                    ("count".to_owned(), AstType::Integer),
                    ("note".to_owned(), AstType::Text),
                    ("total".to_owned(), AstType::Decimal),
                ],
                open_tail: None,
            }),
            span: HirSpan::default(),
        }],
    });
    Ok((PackageInput::new(manifest, [module]), lockfile))
}

fn generic_workspace_fixture(
    source: &str,
) -> Result<(Workspace, axiom_ledger::store::CommitId, CompiledArtifact), String> {
    let mut workspace = Workspace::new();
    let loaded = workspace
        .load_source("benchmark/generic-form-elaboration", source.as_bytes())
        .map_err(|error| format!("generic form source load failed: {error}"))?;
    let (package, lockfile) = generic_form_package_input()?;
    let (artifact_id, artifact) = workspace
        .compile_packages_persisted([package], &lockfile)
        .map_err(|error| format!("generic form package persistence failed: {error}"))?;
    let pinned = workspace
        .commit_with_compiled_artifact(loaded.commit_id(), artifact_id)
        .map_err(|error| format!("generic form artifact pinning failed: {error}"))?;
    Ok((workspace, pinned.commit_id(), artifact))
}

fn median_generic_package_compile(samples: usize) -> Result<u128, String> {
    let mut values = Vec::with_capacity(samples);
    for _ in 0..samples {
        let start = Instant::now();
        let mut workspace = Workspace::new();
        let (package, lockfile) = generic_form_package_input()?;
        let (_, artifact) = workspace
            .compile_packages_persisted([package], &lockfile)
            .map_err(|error| format!("generic form package compilation failed: {error}"))?;
        black_box(artifact.artifact_hash());
        values.push(start.elapsed().as_nanos());
    }
    Ok(median(values))
}

fn median_surface_parse(source: &str, expected_forms: usize, samples: usize) -> u128 {
    let mut values = Vec::with_capacity(samples);
    for _ in 0..samples {
        let start = Instant::now();
        let surface = SurfaceFile::parse(source);
        black_box(surface.forms().count() == expected_forms);
        values.push(start.elapsed().as_nanos());
    }
    median(values)
}

fn median_generic_normalization(source: &str, samples: usize) -> u128 {
    let mut values = Vec::with_capacity(samples);
    for _ in 0..samples {
        let start = Instant::now();
        let surface = SurfaceFile::parse(source);
        black_box(surface.canonical().len());
        values.push(start.elapsed().as_nanos());
    }
    median(values)
}

fn median_workspace_document(
    workspace: &Workspace,
    commit: axiom_ledger::store::CommitId,
    samples: usize,
) -> Result<u128, String> {
    let mut values = Vec::with_capacity(samples);
    for _ in 0..samples {
        let start = Instant::now();
        let elaborated = workspace
            .elaborate_package_forms(commit)
            .map_err(|error| format!("generic document elaboration failed: {error}"))?;
        black_box(elaborated.forms().len());
        values.push(start.elapsed().as_nanos());
    }
    Ok(median(values))
}

fn median_generic_schema_lookup(
    artifact: &CompiledArtifact,
    package_root: ContentHash,
    qualified_name: &QualifiedName,
    count: usize,
    samples: usize,
) -> Result<u128, String> {
    let mut values = Vec::with_capacity(samples);
    for _ in 0..samples {
        let start = Instant::now();
        for _ in 0..count {
            let schema = artifact
                .resolve_record_schema(package_root, qualified_name)
                .map_err(|error| format!("generic schema lookup failed: {error}"))?;
            black_box(schema.schema_id());
        }
        values.push(start.elapsed().as_nanos());
    }
    Ok(median(values))
}

fn median_generic_canonical_values(
    forms: &[ElaboratedForm],
    samples: usize,
) -> Result<u128, String> {
    let mut values = Vec::with_capacity(samples);
    for _ in 0..samples {
        let start = Instant::now();
        for form in forms {
            let canonical = form
                .schema()
                .check_concrete_record(form.value().record())
                .map_err(|error| format!("generic canonical value check failed: {error}"))?;
            black_box(canonical.content_hash());
        }
        values.push(start.elapsed().as_nanos());
    }
    Ok(median(values))
}

fn median_generic_workspace_revision(
    source: &str,
    changed: &str,
    samples: usize,
) -> Result<u128, String> {
    let mut values = Vec::with_capacity(samples);
    for _ in 0..samples {
        let (mut workspace, _base_commit, _) = generic_workspace_fixture(source)?;
        let start = Instant::now();
        let changed_commit = workspace
            .load_source("benchmark/generic-form-elaboration", changed.as_bytes())
            .map_err(|error| format!("generic source revision failed: {error}"))?;
        let elaborated = workspace
            .elaborate_package_forms(changed_commit.commit_id())
            .map_err(|error| format!("generic source revision failed: {error}"))?;
        black_box(elaborated.forms().len());
        values.push(start.elapsed().as_nanos());
    }
    Ok(median(values))
}

fn median_generation(
    workload: Workload,
    quick: bool,
    scale: u64,
    samples: usize,
) -> Result<u128, String> {
    let mut values = Vec::with_capacity(samples);
    for _ in 0..samples {
        let start = Instant::now();
        let generated = generate(workload, quick, scale)?;
        black_box(generated.source.len());
        values.push(start.elapsed().as_nanos());
    }
    Ok(median(values))
}

fn median_parse(source: &str, samples: usize) -> Result<u128, String> {
    let mut workspace = Workspace::new();
    let loaded = workspace
        .load_source("benchmark/parse", source.as_bytes())
        .map_err(|error| error.to_string())?;
    let mut values = Vec::with_capacity(samples);
    for _ in 0..samples {
        let start = Instant::now();
        let parsed = workspace
            .elaborate_commit(loaded.commit_id())
            .map_err(|error| error.to_string())?;
        black_box(parsed.forms.len());
        values.push(start.elapsed().as_nanos());
    }
    Ok(median(values))
}

fn median_normalization(source: &str, samples: usize) -> Result<u128, String> {
    let mut values = Vec::with_capacity(samples);
    for _ in 0..samples {
        let start = Instant::now();
        let surface = SurfaceFile::parse(source);
        black_box(surface.lossless().len());
        values.push(start.elapsed().as_nanos());
    }
    Ok(median(values))
}

fn median_package_upgrade_solve(source: &str, samples: usize) -> Result<u128, String> {
    let mut values = Vec::with_capacity(samples);
    for _ in 0..samples {
        let start = Instant::now();
        let mut workspace = Workspace::new();
        let loaded = workspace
            .load_source("benchmark/package-cold", source.as_bytes())
            .map_err(|error| error.to_string())?;
        let package = workspace
            .put_policy_package(StorePolicyPackage::new(
                "lots/fifo",
                "1.1.0",
                b"selector=latest_acquisition\ntie=ambiguous".to_vec(),
            ))
            .map_err(|error| error.to_string())?;
        let committed = workspace
            .commit_with_packages(loaded.commit_id(), [package])
            .map_err(|error| error.to_string())?;
        let analysis = workspace
            .analyze_commit(committed.commit_id())
            .map_err(|error| error.to_string())?;
        black_box(analysis.proof.nodes.len());
        values.push(start.elapsed().as_nanos());
    }
    Ok(median(values))
}

fn median_incremental_solve(
    workload: Workload,
    source: &str,
    changed: &str,
    samples: usize,
) -> Result<u128, String> {
    let mut values = Vec::with_capacity(samples);
    for _ in 0..samples {
        let mut workspace = Workspace::new();
        let mut loaded = workspace
            .load_source("benchmark/incremental", source.as_bytes())
            .map_err(|error| error.to_string())?;
        if workload == Workload::PackageUpgrade {
            let package = workspace
                .put_policy_package(StorePolicyPackage::new(
                    "lots/fifo",
                    "1.0.0",
                    b"selector=earliest_acquisition\ntie=ambiguous".to_vec(),
                ))
                .map_err(|error| error.to_string())?;
            loaded = workspace
                .commit_with_packages(loaded.commit_id(), [package])
                .map_err(|error| error.to_string())?;
        }
        workspace
            .analyze_commit(loaded.commit_id())
            .map_err(|error| error.to_string())?;
        workspace.clear_incremental_trace();
        // Start before the revised input is committed/updated.  The reported
        // incremental time therefore covers the warm input revision as well
        // as analysis of the resulting commit.
        let start = Instant::now();
        let changed_commit = if workload == Workload::PackageUpgrade {
            let package = workspace
                .put_policy_package(StorePolicyPackage::new(
                    "lots/fifo",
                    "1.1.0",
                    b"selector=latest_acquisition\ntie=ambiguous".to_vec(),
                ))
                .map_err(|error| error.to_string())?;
            workspace
                .commit_with_packages(loaded.commit_id(), [package])
                .map_err(|error| error.to_string())?
        } else {
            workspace
                .load_source("benchmark/incremental", changed.as_bytes())
                .map_err(|error| error.to_string())?
        };
        let analysis = workspace
            .analyze_commit(changed_commit.commit_id())
            .map_err(|error| error.to_string())?;
        black_box(analysis.proof.nodes.len());
        values.push(start.elapsed().as_nanos());
    }
    Ok(median(values))
}

fn median_cold_solve(workload: Workload, source: &str, samples: usize) -> Result<u128, String> {
    let mut values = Vec::with_capacity(samples);
    for _ in 0..samples {
        let start = Instant::now();
        let analysis = independent_clean_analysis(workload, "benchmark/cold", source)?;
        black_box(analysis.proof.nodes.len());
        values.push(start.elapsed().as_nanos());
    }
    Ok(median(values))
}

fn independent_clean_analysis(
    workload: Workload,
    source_name: impl Into<String>,
    source: &str,
) -> Result<axiom_ledger::workspace::CommitAnalysis, String> {
    let mut workspace = Workspace::new();
    let mut loaded = workspace
        .load_source(source_name, source.as_bytes())
        .map_err(|error| error.to_string())?;
    if workload == Workload::PackageUpgrade {
        let package = workspace
            .put_policy_package(StorePolicyPackage::new(
                "lots/fifo",
                "1.0.0",
                b"selector=earliest_acquisition\ntie=ambiguous".to_vec(),
            ))
            .map_err(|error| error.to_string())?;
        loaded = workspace
            .commit_with_packages(loaded.commit_id(), [package])
            .map_err(|error| error.to_string())?;
    }
    workspace
        .analyze_commit(loaded.commit_id())
        .map_err(|error| error.to_string())
}

fn median_independent_clean_solve(
    workload: Workload,
    source: &str,
    samples: usize,
) -> Result<u128, String> {
    let mut values = Vec::with_capacity(samples);
    for _ in 0..samples {
        let start = Instant::now();
        let analysis =
            independent_clean_analysis(workload, format!("benchmark/{}", workload.name()), source)?;
        black_box(analysis.proof.nodes.len());
        values.push(start.elapsed().as_nanos());
    }
    Ok(median(values))
}

/// Compare one worker with two concurrent workers using the same clean
/// workspace recipe.  The production workspace is deliberately not shared
/// across threads: this is a benchmark-level parallel equivalence probe for
/// independent goals, and does not claim that `Workspace` itself is `Sync` or
/// that the engine has a shared parallel execution path.
fn median_independent_worker_comparison(
    workload: Workload,
    source: &str,
    samples: usize,
) -> Result<(u128, bool), String> {
    let mut values = Vec::with_capacity(samples);
    let mut equivalent = true;
    for _ in 0..samples {
        let single = independent_clean_analyses(workload, source, 1)?;
        let start = Instant::now();
        let concurrent = independent_clean_analyses(workload, source, 2)?;
        values.push(start.elapsed().as_nanos());
        let Some(reference) = single.first() else {
            return Err("worker comparison produced no serial result".into());
        };
        equivalent &= concurrent.iter().all(|candidate| candidate == reference);
    }
    Ok((median(values), equivalent))
}

fn independent_clean_analyses(
    workload: Workload,
    source: &str,
    thread_count: usize,
) -> Result<Vec<axiom_ledger::workspace::CommitAnalysis>, String> {
    if thread_count == 0 {
        return Err("worker comparison requires at least one worker".into());
    }
    thread::scope(|scope| {
        let handles = (0..thread_count)
            .map(|_| {
                scope.spawn(|| independent_clean_analysis(workload, "benchmark/worker", source))
            })
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .map(|handle| {
                handle
                    .join()
                    .map_err(|_| "independent benchmark worker panicked".to_string())?
            })
            .collect()
    })
}

fn median_workspace_replay(
    workspace: &mut Workspace,
    commit: axiom_ledger::store::CommitId,
    samples: usize,
) -> Result<u128, String> {
    let mut values = Vec::with_capacity(samples);
    for _ in 0..samples {
        let start = Instant::now();
        let analysis = workspace
            .analyze_commit(commit)
            .map_err(|error| error.to_string())?;
        black_box(analysis.proof.nodes.len());
        values.push(start.elapsed().as_nanos());
    }
    Ok(median(values))
}

fn median_proof_check(analysis: &axiom_ledger::Analysis, samples: usize) -> u128 {
    let mut values = Vec::with_capacity(samples);
    for _ in 0..samples {
        let start = Instant::now();
        let result = analysis.check_proof();
        black_box(result.is_ok());
        values.push(start.elapsed().as_nanos());
    }
    median(values)
}

fn median_explanation(
    analysis: &axiom_ledger::Analysis,
    goal: &str,
    samples: usize,
) -> Result<u128, String> {
    let mut values = Vec::with_capacity(samples);
    for _ in 0..samples {
        let start = Instant::now();
        let rendered = render_why(analysis, goal).map_err(|error| error.to_string())?;
        black_box(rendered.len());
        values.push(start.elapsed().as_nanos());
    }
    Ok(median(values))
}

fn median(mut values: Vec<u128>) -> u128 {
    values.sort_unstable();
    values[values.len() / 2]
}

/// Measure one workload in a fresh process so unrelated earlier workloads
/// cannot inflate its `ru_maxrss`. The child emits one bounded protocol line;
/// using `output()` keeps stdout/stderr collection joined and deadlock-free.
fn isolated_peak_memory_bytes(
    workload: Workload,
    quick: bool,
    scale: u64,
) -> Result<Option<usize>, String> {
    let executable = env::current_exe()
        .map_err(|error| format!("cannot locate benchmark executable for RSS probe: {error}"))?;
    let mut command = Command::new(executable);
    command
        .arg("--rss-probe")
        .arg("--workload")
        .arg(workload.name());
    if quick {
        command.arg("--quick");
    }
    command.arg("--scale").arg(scale.to_string());
    let output = command
        .output()
        .map_err(|error| format!("RSS probe failed to start: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "RSS probe failed for {} ({}): {}",
            workload.name(),
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let stdout = String::from_utf8(output.stdout)
        .map_err(|_| "RSS probe emitted non-UTF-8 stdout".to_string())?;
    let lines = stdout.lines().collect::<Vec<_>>();
    if lines.len() != 1 {
        return Err(format!(
            "RSS probe emitted {} protocol lines for {}",
            lines.len(),
            workload.name()
        ));
    }
    let line = lines[0];
    let prefix = format!(
        "{{\"schema\":\"{SCHEMA}\",\"kind\":\"rss_probe\",\"workload\":\"{}\",\"peak_memory_bytes\":",
        workload.name()
    );
    let value = line
        .strip_prefix(&prefix)
        .and_then(|value| value.strip_suffix('}'))
        .ok_or_else(|| format!("RSS probe returned malformed JSON: {line}"))?;
    if value == "null" {
        return Ok(None);
    }
    value
        .parse::<usize>()
        .map(Some)
        .map_err(|_| format!("RSS probe returned invalid peak bytes: {line}"))
}

/// Return this process's peak resident set size when the target exposes a
/// stable `getrusage` contract.  `ru_maxrss` is KiB on Linux and bytes on
/// macOS.  Other targets intentionally report `None` instead of guessing at
/// units or relying on a non-portable shell utility.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn process_peak_memory_bytes() -> Option<usize> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::zeroed();
    // SAFETY: `getrusage` initializes the supplied `rusage` structure when it
    // returns zero, and the pointer is valid for the duration of the call.
    let result = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) };
    if result != 0 {
        return None;
    }
    // SAFETY: the successful call above initialized `usage`.
    let usage = unsafe { usage.assume_init() };
    let rss = u128::try_from(usage.ru_maxrss).ok()?;
    let multiplier = if cfg!(target_os = "linux") { 1024 } else { 1 };
    rss.checked_mul(multiplier)?.try_into().ok()
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn process_peak_memory_bytes() -> Option<usize> {
    None
}

fn self_test(_quick: bool, scale: u64) -> Result<usize, String> {
    // Self-test is an assurance check for the canonical corpus.  Keep the
    // corpus full-sized even when the CLI's --quick flag is present; --quick
    // is for timing runs, not a weaker correctness check.
    let test_scale = scale.min(2);
    let mut checks = 0usize;
    for workload in Workload::ALL {
        let left = generate(workload, false, test_scale)?;
        let right = generate(workload, false, test_scale)?;
        if left.source != right.source || left.changed_source != right.changed_source {
            return Err(format!(
                "{}: generator is not deterministic",
                workload.name()
            ));
        }
        checks += 1;
        if workload == Workload::GenericFormElaboration {
            checks += generic_form_self_test(&left, test_scale)?;
            continue;
        }
        let mut workspace = Workspace::new();
        let mut source = workspace
            .load_source(
                format!("benchmark/self-test/{}", workload.name()),
                left.source.as_bytes(),
            )
            .map_err(|error| {
                format!("{}: self-test source load failed: {error}", workload.name())
            })?;
        if workload == Workload::PackageUpgrade {
            let package = workspace
                .put_policy_package(StorePolicyPackage::new(
                    "lots/fifo",
                    "1.0.0",
                    b"selector=earliest_acquisition\ntie=ambiguous".to_vec(),
                ))
                .map_err(|error| {
                    format!("{}: self-test package failed: {error}", workload.name())
                })?;
            source = workspace
                .commit_with_packages(source.commit_id(), [package])
                .map_err(|error| {
                    format!(
                        "{}: self-test package commit failed: {error}",
                        workload.name()
                    )
                })?;
        }
        let analysis = workspace
            .analyze_commit(source.commit_id())
            .map_err(|error| format!("{}: self-test analysis failed: {error}", workload.name()))?;
        analysis
            .check_proof()
            .map_err(|error| format!("{}: self-test proof failed: {error}", workload.name()))?;
        if let Some(kind) = left.semantic_probe {
            let probe = run_semantic_probe(kind, left.semantic_probe_items).map_err(|error| {
                format!(
                    "{}: self-test semantic probe failed: {error}",
                    workload.name()
                )
            })?;
            if probe.items != left.semantic_probe_items || probe.results == 0 {
                return Err(format!(
                    "{}: self-test semantic probe returned no results",
                    workload.name()
                ));
            }
        }
        checks += 1;
        let independent = independent_clean_analysis(
            workload,
            format!("benchmark/self-test/{}", workload.name()),
            &left.source,
        )
        .map_err(|error| {
            format!(
                "{}: self-test independent recomputation failed: {error}",
                workload.name()
            )
        })?;
        if independent.analysis != *analysis {
            return Err(format!(
                "{}: self-test independent recomputation mismatch",
                workload.name()
            ));
        }
        checks += 1;
        if workload == Workload::OneRowCloseChange || workload == Workload::PackageUpgrade {
            workspace.clear_incremental_trace();
            let changed = if workload == Workload::PackageUpgrade {
                let package = workspace
                    .put_policy_package(StorePolicyPackage::new(
                        "lots/fifo",
                        "1.1.0",
                        b"selector=latest_acquisition\ntie=ambiguous".to_vec(),
                    ))
                    .map_err(|error| {
                        format!(
                            "{}: self-test upgraded package failed: {error}",
                            workload.name()
                        )
                    })?;
                workspace
                    .commit_with_packages(source.commit_id(), [package])
                    .map_err(|error| {
                        format!(
                            "{}: self-test package upgrade commit failed: {error}",
                            workload.name()
                        )
                    })?
            } else {
                workspace
                    .load_source(
                        format!("benchmark/self-test/{}", workload.name()),
                        left.changed_source
                            .as_deref()
                            .unwrap_or_default()
                            .as_bytes(),
                    )
                    .map_err(|error| {
                        format!(
                            "{}: self-test changed source failed: {error}",
                            workload.name()
                        )
                    })?
            };
            let clean_store = workspace.store().clone();
            let changed_analysis =
                workspace
                    .analyze_commit(changed.commit_id())
                    .map_err(|error| {
                        format!("{}: self-test incremental failed: {error}", workload.name())
                    })?;
            if workspace.incremental_metrics().invalidated_queries == 0 {
                return Err(format!(
                    "{}: self-test saw no invalidation",
                    workload.name()
                ));
            }
            let mut clean = Workspace::from_store(clean_store);
            let full = clean.analyze_commit(changed.commit_id()).map_err(|error| {
                format!(
                    "{}: self-test clean replay failed: {error}",
                    workload.name()
                )
            })?;
            if changed_analysis != full {
                return Err(format!(
                    "{}: self-test incremental output mismatch",
                    workload.name()
                ));
            }
            checks += 2;
        }
    }
    let cycle = recursive_cycle_probe()?;
    if cycle != 1 {
        return Err(format!(
            "recursive cycle probe returned {cycle}, expected 1"
        ));
    }
    checks += 1;
    let worker_probe = generate(Workload::HighFrequencyLots, false, test_scale)?;
    let (_, workers_equivalent) =
        median_independent_worker_comparison(Workload::HighFrequencyLots, &worker_probe.source, 1)?;
    if !workers_equivalent {
        return Err("concurrent independent worker differs from the serial result".into());
    }
    checks += 1;
    let one = generate(Workload::OneRowCloseChange, false, test_scale)?;
    if one.changed_source == Some(one.source.clone()) {
        return Err("one-row close change did not change source".into());
    }
    checks += 1;
    if checks < 7 {
        return Err("self-test did not execute its minimum checks".into());
    }
    Ok(checks)
}

fn generic_form_self_test(generated: &GeneratedWorkload, _scale: u64) -> Result<usize, String> {
    let (mut workspace, base_commit, artifact) = generic_workspace_fixture(&generated.source)?;
    let bound = workspace
        .elaborate_package_forms(base_commit)
        .map_err(|error| format!("generic form self-test elaboration failed: {error}"))?;
    let forms = bound.forms();
    if forms.len() != generated.semantic_probe_items {
        return Err(format!(
            "generic form self-test returned {} forms, expected {}",
            forms.len(),
            generated.semantic_probe_items
        ));
    }
    let canonical = forms
        .iter()
        .map(|form| {
            form.schema()
                .check_concrete_record(form.value().record())
                .map(|value| value.content_hash())
                .map_err(|error| format!("generic form self-test canonical value failed: {error}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if canonical.len() != forms.len() {
        return Err("generic form self-test did not canonicalize every value".into());
    }
    let package_root = artifact.package_roots()[0];
    let schema = generic_form_schema_name()?;
    if artifact
        .resolve_record_schema(package_root, &schema)
        .is_err()
    {
        return Err("generic form self-test schema lookup failed".into());
    }
    let changed = generated
        .changed_source
        .as_deref()
        .ok_or_else(|| "generic form self-test has no source revision".to_string())?;
    let changed_commit = workspace
        .load_source("benchmark/generic-form-elaboration", changed.as_bytes())
        .map_err(|error| format!("generic form self-test changed source failed: {error}"))?;
    let changed_bound = workspace
        .elaborate_package_forms(changed_commit.commit_id())
        .map_err(|error| format!("generic form self-test changed elaboration failed: {error}"))?;
    let changed_forms = changed_bound.forms();
    if changed_forms.len() != forms.len()
        || changed_forms
            .first()
            .zip(forms.first())
            .is_none_or(|(left, right)| left.value().content_hash() == right.value().content_hash())
    {
        return Err(
            "generic form self-test source revision did not change a canonical value".into(),
        );
    }
    Ok(4)
}

fn recursive_cycle_probe() -> Result<usize, String> {
    let first = QueryKey::new("recursive/first").map_err(|error| error.to_string())?;
    let second = QueryKey::new("recursive/second").map_err(|error| error.to_string())?;
    let mut db = IncrementalDb::new();
    let outcome = db.evaluate(first.clone(), |context| {
        context.query(second.clone(), |context| {
            context.query(first.clone(), |_context| {
                MemoOutcome::value(b"never".to_vec())
            })
        })
    });
    Ok(usize::from(matches!(
        outcome,
        MemoOutcome::Error(QueryError::Cycle { .. })
    )))
}

fn run_semantic_probe(
    kind: SemanticProbeKind,
    items: usize,
) -> Result<SemanticProbeResult, String> {
    match kind {
        SemanticProbeKind::CurrencyExchange => currency_exchange_probe(items),
        SemanticProbeKind::CorporateActions => corporate_actions_probe(items),
        SemanticProbeKind::InvoicePaymentGraph => invoice_payment_graph_probe(items),
        SemanticProbeKind::OwnershipRoles => ownership_roles_probe(items),
        SemanticProbeKind::PackageUpgrade => package_upgrade_probe(items),
        SemanticProbeKind::RecursiveLogic => recursive_logic_probe(items),
    }
}

fn currency_exchange_probe(items: usize) -> Result<SemanticProbeResult, String> {
    let currencies = ["USD", "EUR", "GBP", "BTC"];
    for index in 0..items {
        let give = currencies[index % currencies.len()];
        let receive = currencies[(index + 1) % currencies.len()];
        let give_instrument = Instrument::new(format!("currency/{give}"), InstrumentKind::Currency)
            .denominated(Unit::new(give).map_err(|error| error.to_string())?);
        let receive_instrument =
            Instrument::new(format!("currency/{receive}"), InstrumentKind::Currency)
                .denominated(Unit::new(receive).map_err(|error| error.to_string())?);
        give_instrument
            .validate()
            .map_err(|error| format!("currency give instrument {give}: {error}"))?;
        receive_instrument
            .validate()
            .map_err(|error| format!("currency receive instrument {receive}: {error}"))?;
        let trader = Endpoint::entity(format!("trader/{index:06}"));
        let venue = Endpoint::entity(format!("venue/{index:06}"));
        let give_quantity = Quantity::with_unit(
            axiom_ledger::exact::ExactNumber::integer((index % 17 + 1) as i128),
            give,
        )
        .map_err(|error| format!("currency give quantity {index}: {error}"))?;
        let receive_quantity = Quantity::with_unit(
            axiom_ledger::exact::ExactNumber::integer((index % 23 + 1) as i128),
            receive,
        )
        .map_err(|error| format!("currency receive quantity {index}: {error}"))?;
        let exchange = ExchangeRecord::new(
            format!("fx/exchange/{index:06}"),
            vec![
                ExchangeLeg::give(
                    trader.clone(),
                    venue.clone(),
                    give_instrument.id.clone(),
                    give_quantity,
                ),
                ExchangeLeg::receive(
                    venue,
                    trader,
                    receive_instrument.id.clone(),
                    receive_quantity,
                ),
            ],
        );
        exchange
            .validate_with_instruments(&[give_instrument, receive_instrument])
            .map_err(|error| format!("currency exchange {index}: {error}"))?;
    }
    Ok(SemanticProbeResult {
        items,
        results: items,
    })
}

fn corporate_actions_probe(items: usize) -> Result<SemanticProbeResult, String> {
    for index in 0..items {
        let id = format!("contract/action/{index:06}");
        let action = match index % 4 {
            0 => {
                let before =
                    Quantity::with_unit(axiom_ledger::exact::ExactNumber::integer(2), "FUND")
                        .map_err(|error| format!("split before {index}: {error}"))?;
                let after =
                    Quantity::with_unit(axiom_ledger::exact::ExactNumber::integer(4), "FUND-SPLIT")
                        .map_err(|error| format!("split after {index}: {error}"))?;
                CorporateAction::Split(
                    Split::new(
                        id,
                        "FUND",
                        "FUND-SPLIT",
                        axiom_ledger::exact::ExactNumber::integer(2),
                        vec![TransformationLeg::new("custody", before, after)],
                    )
                    .with_quantum(
                        Quantity::with_unit(axiom_ledger::exact::ExactNumber::integer(1), "FUND")
                            .map_err(|error| format!("split source quantum {index}: {error}"))?,
                        Quantity::with_unit(
                            axiom_ledger::exact::ExactNumber::integer(1),
                            "FUND-SPLIT",
                        )
                        .map_err(|error| format!("split destination quantum {index}: {error}"))?,
                    ),
                )
            }
            1 => {
                let before = Quantity::with_unit(
                    axiom_ledger::exact::ExactNumber::integer(4),
                    "FUND-REVERSE",
                )
                .map_err(|error| format!("merge before {index}: {error}"))?;
                let after =
                    Quantity::with_unit(axiom_ledger::exact::ExactNumber::integer(2), "FUND")
                        .map_err(|error| format!("merge after {index}: {error}"))?;
                CorporateAction::Merge(Merge::new(
                    id,
                    "FUND-REVERSE",
                    "FUND",
                    axiom_ledger::exact::ExactNumber::rational(1, 2)
                        .map_err(|error| format!("merge ratio {index}: {error}"))?,
                    vec![TransformationLeg::new("custody", before, after)],
                ))
            }
            2 => {
                let parent_before =
                    Quantity::with_unit(axiom_ledger::exact::ExactNumber::integer(2), "FUND")
                        .map_err(|error| format!("spinoff parent before {index}: {error}"))?;
                let parent_after =
                    Quantity::with_unit(axiom_ledger::exact::ExactNumber::integer(2), "FUND")
                        .map_err(|error| format!("spinoff parent after {index}: {error}"))?;
                let child =
                    Quantity::with_unit(axiom_ledger::exact::ExactNumber::integer(1), "FUND-SPIN")
                        .map_err(|error| format!("spinoff child {index}: {error}"))?;
                CorporateAction::Spinoff(Spinoff::new(
                    id,
                    "FUND",
                    "FUND-SPIN",
                    axiom_ledger::exact::ExactNumber::rational(1, 2)
                        .map_err(|error| format!("spinoff ratio {index}: {error}"))?,
                    vec![SpinoffLeg::new(
                        "custody",
                        parent_before,
                        parent_after,
                        child,
                    )],
                ))
            }
            _ => {
                let funding =
                    Quantity::with_unit(axiom_ledger::exact::ExactNumber::integer(10), "USD")
                        .map_err(|error| format!("dividend funding {index}: {error}"))?;
                let first =
                    Quantity::with_unit(axiom_ledger::exact::ExactNumber::integer(6), "USD")
                        .map_err(|error| format!("dividend first leg {index}: {error}"))?;
                let second =
                    Quantity::with_unit(axiom_ledger::exact::ExactNumber::integer(4), "USD")
                        .map_err(|error| format!("dividend second leg {index}: {error}"))?;
                CorporateAction::Dividend(Dividend::new(
                    id,
                    "issuer",
                    funding,
                    vec![
                        DividendLeg::new("holder-a", first),
                        DividendLeg::new("holder-b", second),
                    ],
                ))
            }
        };
        action
            .validate()
            .map_err(|error| format!("corporate action {index}: {error}"))?;
    }
    Ok(SemanticProbeResult {
        items,
        results: items,
    })
}

fn invoice_payment_graph_probe(items: usize) -> Result<SemanticProbeResult, String> {
    let mut obligations = Vec::with_capacity(items);
    let mut settlements = Vec::with_capacity(items);
    let mut allocations = Vec::with_capacity(items);
    for index in 0..items {
        let customer = format!("customer/{index:06}");
        let amount = (100 + index % 37) as i128;
        let quantity =
            Quantity::with_unit(axiom_ledger::exact::ExactNumber::integer(amount), "USD")
                .map_err(|error| format!("invoice quantity {index}: {error}"))?;
        let obligation = Obligation::transfer(
            format!("inv/{index:06}"),
            customer.clone(),
            "merchant",
            "USD",
            quantity.clone(),
        )
        .map_err(|error| format!("invoice obligation {index}: {error}"))?;
        obligation
            .validate()
            .map_err(|error| format!("invoice validation {index}: {error}"))?;
        let mut settlement = Settlement::new_with_kind(
            format!("payment/{index:06}"),
            axiom_ledger::model::SettlementKind::Ach,
            Endpoint::entity(customer),
            Endpoint::entity("merchant"),
            "USD",
            quantity.clone(),
        )
        .map_err(|error| format!("payment construction {index}: {error}"))?;
        settlement
            .transition(SettlementState::Presented, None, None)
            .map_err(|error| format!("payment presented transition {index}: {error}"))?;
        settlement
            .transition(SettlementState::Settled, None, None)
            .map_err(|error| format!("payment settled transition {index}: {error}"))?;
        settlement
            .validate()
            .map_err(|error| format!("payment validation {index}: {error}"))?;
        let allocation = SatisfactionAllocation::new(
            format!("allocation/{index:06}"),
            obligation.id.clone(),
            settlement.id.clone(),
            quantity,
        )
        .map_err(|error| format!("allocation construction {index}: {error}"))?
        .applied();
        allocations.push(allocation);
        obligations.push(obligation);
        settlements.push(settlement);
    }

    let summary = validate_satisfaction_network(&obligations, &settlements, &allocations)
        .map_err(|error| format!("invoice/payment graph validation: {error}"))?;
    if summary
        .obligation_remaining
        .values()
        .any(|quantity| !quantity.is_zero())
        || summary
            .settlement_unused
            .values()
            .any(|quantity| !quantity.is_zero())
    {
        return Err("invoice/payment graph did not fully allocate settled payments".into());
    }
    Ok(SemanticProbeResult {
        items,
        results: summary.obligation_remaining.len() + summary.settlement_unused.len(),
    })
}

fn ownership_roles_probe(items: usize) -> Result<SemanticProbeResult, String> {
    let half = axiom_ledger::exact::ExactNumber::rational(1, 2)
        .map_err(|error| format!("ownership share: {error}"))?;
    let mut assignments = RoleAssignments::new();
    for index in 0..items {
        let subject = format!("entity/company/{index:06}");
        assignments.push(
            RoleAssignment::new(
                subject.clone(),
                Role::BeneficialOwner,
                format!("entity/owner/{index:06}/a"),
            )
            .with_share(half.clone()),
        );
        assignments.push(
            RoleAssignment::new(
                subject,
                Role::BeneficialOwner,
                format!("entity/owner/{index:06}/b"),
            )
            .with_share(half.clone()),
        );
    }
    assignments
        .validate()
        .map_err(|error| format!("ownership role validation: {error}"))?;
    let results = assignments.assignments.len();
    Ok(SemanticProbeResult { items, results })
}

fn package_upgrade_probe(items: usize) -> Result<SemanticProbeResult, String> {
    let earliest = PolicyPackage::new(
        "lots/fifo",
        "1.0.0",
        "selector=earliest_acquisition\ntie=ambiguous",
    );
    let latest = PolicyPackage::new(
        "lots/fifo",
        "1.1.0",
        "selector=latest_acquisition\ntie=ambiguous",
    );
    let earliest_program = earliest
        .compile()
        .map_err(|error| format!("package v1 compile: {error}"))?;
    let latest_program = latest
        .compile()
        .map_err(|error| format!("package v2 compile: {error}"))?;
    if earliest.hash() == latest.hash() {
        return Err("package upgrade probe produced identical package identities".into());
    }
    let mut results = 0usize;
    for index in 0..items {
        let early = LotCandidate::new(
            format!("upgrade/lot/{index:06}/early"),
            axiom_ledger::model::Date::new(2020, 1, 1)
                .map_err(|error| format!("package candidate date {index}: {error}"))?,
        );
        let late = LotCandidate::new(
            format!("upgrade/lot/{index:06}/late"),
            axiom_ledger::model::Date::new(2020, 2, 1)
                .map_err(|error| format!("package candidate date {index}: {error}"))?,
        );
        let candidates = [early, late];
        let selected_early = earliest_program.evaluate(candidates.iter());
        let selected_late = latest_program.evaluate(candidates.iter());
        if selected_early != Selection::Unique(candidates[0].id.clone())
            || selected_late != Selection::Unique(candidates[1].id.clone())
        {
            return Err(format!(
                "package upgrade changed no deterministic selection for row {index}"
            ));
        }
        results += 2;
    }
    Ok(SemanticProbeResult { items, results })
}

fn recursive_logic_probe(items: usize) -> Result<SemanticProbeResult, String> {
    fn value(name: impl Into<String>) -> Term {
        Term::Text(name.into())
    }
    fn atom(name: &str, arguments: Vec<Term>) -> Atom {
        Atom::new(Nominal::new(NominalKind::Predicate, name), arguments)
    }
    fn positive(name: &str, arguments: Vec<Term>) -> Goal {
        Goal::atom(Literal::positive(atom(name, arguments)))
    }

    let mut program = Program::new();
    for index in 0..items {
        program
            .add_fact_named(
                format!("edge/{index:06}"),
                Literal::positive(atom(
                    "edge",
                    vec![
                        value(format!("node/{index:06}")),
                        value(format!("node/{:06}", index + 1)),
                    ],
                )),
            )
            .map_err(|error| format!("recursive edge {index}: {error}"))?;
    }
    let x = Var::inference(1);
    let y = Var::inference(2);
    let z = Var::inference(3);
    program.add_clause(Clause::new(
        Literal::positive(atom(
            "reachable",
            vec![Term::var(x.clone()), Term::var(y.clone())],
        )),
        positive("edge", vec![Term::var(x.clone()), Term::var(y.clone())]),
    ));
    program.add_clause(Clause::new(
        Literal::positive(atom(
            "reachable",
            vec![Term::var(x.clone()), Term::var(z.clone())],
        )),
        Goal::and(vec![
            positive("edge", vec![Term::var(x), Term::var(y.clone())]),
            positive("reachable", vec![Term::var(y), Term::var(z)]),
        ]),
    ));
    program
        .validate()
        .map_err(|error| format!("recursive program validation: {error}"))?;
    // Keep one bounded chain query in the default resource profile while all
    // generated edges still participate in program construction.  A query
    // spanning hundreds of links is a stress test for the solver's resource
    // boundary, not a useful fixed-point workload measurement.
    let path_length = items.clamp(1, 8);
    let goal = Goal::atom(Literal::positive(atom(
        "reachable",
        vec![
            value("node/000000"),
            value(format!("node/{path_length:06}")),
        ],
    )));
    let mut solver = Solver::new();
    let result = solver.solve(&program, &goal, &SemanticContext::default());
    if result.truth() != Truth::TrueOnly {
        return Err(format!(
            "recursive path did not resolve as true: {:?}",
            result.truth()
        ));
    }
    result
        .check_proofs()
        .map_err(|error| format!("recursive proof check: {error}"))?;
    let cached = solver.solve(&program, &goal, &SemanticContext::default());
    if !cached
        .trace()
        .iter()
        .any(|event| matches!(event, axiom_ledger::logic::TraceEvent::CacheHit))
    {
        return Err("recursive logic probe did not exercise solver memoization".into());
    }

    // A positive cycle with no base fact is complete but derives nothing; it
    // is the real logic analogue of the benchmark's adversarial cycle.
    let loop_var = Var::inference(7);
    let loop_atom = atom("loop", vec![Term::var(loop_var.clone())]);
    let mut cycle = Program::new();
    cycle.add_clause(Clause::new(
        Literal::positive(loop_atom.clone()),
        Goal::atom(Literal::positive(loop_atom)),
    ));
    cycle
        .validate()
        .map_err(|error| format!("cycle program validation: {error}"))?;
    let cycle_result = Solver::new().solve(
        &cycle,
        &Goal::atom(Literal::positive(atom("loop", vec![value("cycle")]))),
        &SemanticContext::default(),
    );
    if cycle_result.truth() != Truth::Neither {
        return Err(format!(
            "base-less positive cycle changed truth unexpectedly: {:?}",
            cycle_result.truth()
        ));
    }
    Ok(SemanticProbeResult {
        items,
        results: result.candidates().len() + 1,
    })
}

fn print_json_line(record: &ResultRecord) {
    let mut output = String::new();
    let _ = write!(
        output,
        "{{\"schema\":\"{SCHEMA}\",\"kind\":\"measurement\",\"workload\":\"{}\",\"description\":\"{}\",\"quick\":{},\"scale\":{},\"samples\":{},\"status\":\"{}\",\"semantic_supported\":{},\"source_hash\":\"{}\",\"changed_source_hash\":{},\"changed_kind\":{},\"targets\":{},\"measurements\":{{",
        json_escape(record.workload.name()),
        json_escape(record.description),
        record.quick,
        record.scale,
        record.samples,
        record.status,
        record.semantic_supported,
        record.source_hash,
        option_string(record.changed_source_hash.as_deref()),
        record
            .changed_kind
            .map_or_else(|| "null".into(), |kind| format!("\"{}\"", kind.json())),
        record.target.json()
    );
    let _ = write!(
        output,
        "\"generation_ns\":{},\"normalization_ns\":{},\"parse_ns\":{},\"semantic_probe_ns\":{},\"solve_cold_ns\":{},\"independent_clean_solve_ns\":{},\"workspace_replay_ns\":{},\"proof_check_ns\":{},\"explanation_ns\":{},\"changed_incremental_solve_ns\":{},\"changed_full_solve_ns\":{},\"parallel_solve_ns\":null,\"independent_workers_ns\":{},\"package_compile_ns\":{},\"document_elaboration_ns\":{},\"schema_lookup_ns\":{},\"canonical_values_ns\":{},\"changed_document_elaboration_ns\":{}",
        record.timing.generation_ns,
        option_number(record.timing.normalization_ns),
        option_number(record.timing.parse_ns),
        option_number(record.timing.semantic_probe_ns),
        option_number(record.timing.solve_cold_ns),
        option_number(record.timing.independent_clean_solve_ns),
        option_number(record.timing.workspace_replay_ns),
        option_number(record.timing.proof_check_ns),
        option_number(record.timing.explanation_ns),
        option_number(record.timing.changed_incremental_solve_ns),
        option_number(record.timing.changed_full_solve_ns),
        option_number(record.timing.independent_workers_ns),
        option_number(record.timing.package_compile_ns),
        option_number(record.timing.document_elaboration_ns),
        option_number(record.timing.schema_lookup_ns),
        option_number(record.timing.canonical_values_ns),
        option_number(record.timing.changed_document_elaboration_ns),
    );
    output.push_str("},\"sizes\":{");
    let _ = write!(
        output,
        "\"source_bytes\":{},\"source_lines\":{},\"forms\":{},\"changed_source_bytes\":{},\"semantic_relation_nodes\":null,\"semantic_relation_edges\":null,\"dependency_graph_nodes\":{},\"dependency_graph_edges\":{}",
        record.sizes.source_bytes,
        record.sizes.source_lines,
        option_number(record.sizes.forms.map(|value| value as u128)),
        option_number(record.sizes.changed_source_bytes.map(|value| value as u128)),
        option_number(
            record
                .sizes
                .dependency_graph_nodes
                .map(|value| value as u128)
        ),
        option_number(
            record
                .sizes
                .dependency_graph_edges
                .map(|value| value as u128)
        ),
    );
    output.push_str("},\"metrics\":{");
    let _ = write!(
        output,
        "\"cache_hits\":{},\"cache_misses\":{},\"invalidated_queries\":{},\"proof_nodes\":{},\"proof_roots\":{},\"semantic_dependency_edges\":{},\"semantic_invalidation_edges\":{},\"explanation_bytes\":{},\"cycle_errors\":{},\"semantic_probe_items\":{},\"semantic_probe_results\":{},\"semantic_probe_api\":{},\"determinism_across_thread_counts\":null,\"same_process_cache_replay_equal\":{},\"independent_clean_recompute_equal\":{},\"peak_memory_bytes\":{},\"thread_count_equivalence\":null,\"parallel_thread_count\":null,\"independent_worker_determinism\":{},\"independent_worker_equivalence\":{},\"concurrent_worker_count\":{},\"canonical_value_count\":{},\"schema_lookup_count\":{},\"document_result_count\":{},\"revision_result_count\":{},\"revision_mode\":{},\"authority_binding_verified\":{},\"build_profile\":{},\"resource_profile\":{},\"note\":{},\"unsupported_reason\":{}",
        option_number(record.metrics.cache_hits.map(|value| value as u128)),
        option_number(record.metrics.cache_misses.map(|value| value as u128)),
        option_number(
            record
                .metrics
                .invalidated_queries
                .map(|value| value as u128)
        ),
        option_number(record.metrics.proof_nodes.map(|value| value as u128)),
        option_number(record.metrics.proof_roots.map(|value| value as u128)),
        option_number(
            record
                .metrics
                .semantic_dependency_edges
                .map(|value| value as u128)
        ),
        option_number(
            record
                .metrics
                .semantic_invalidation_edges
                .map(|value| value as u128)
        ),
        option_number(record.metrics.explanation_bytes.map(|value| value as u128)),
        option_number(record.metrics.cycle_errors.map(|value| value as u128)),
        option_number(
            record
                .metrics
                .semantic_probe_items
                .map(|value| value as u128)
        ),
        option_number(
            record
                .metrics
                .semantic_probe_results
                .map(|value| value as u128)
        ),
        option_string(record.metrics.semantic_probe_api),
        option_bool(record.metrics.same_process_cache_replay_equal),
        option_bool(record.metrics.independent_clean_recompute_equal),
        option_number(record.metrics.peak_memory_bytes.map(|value| value as u128)),
        option_bool(record.metrics.independent_worker_determinism),
        option_bool(record.metrics.independent_worker_equivalence),
        option_number(
            record
                .metrics
                .concurrent_worker_count
                .map(|value| value as u128)
        ),
        option_number(
            record
                .metrics
                .canonical_value_count
                .map(|value| value as u128)
        ),
        option_number(
            record
                .metrics
                .schema_lookup_count
                .map(|value| value as u128)
        ),
        option_number(
            record
                .metrics
                .document_result_count
                .map(|value| value as u128)
        ),
        option_number(
            record
                .metrics
                .revision_result_count
                .map(|value| value as u128)
        ),
        option_string(record.metrics.revision_mode),
        option_bool(record.metrics.authority_binding_verified),
        option_string(record.metrics.build_profile),
        option_string(record.metrics.resource_profile),
        option_string(record.metrics.note),
        option_string(record.unsupported_reason),
    );
    output.push_str("}}");
    println!("{output}");
}

fn print_human_summary(records: &[ResultRecord]) {
    eprintln!("Axiom benchmark (measurements; targets are engineering goals, not assertions)");
    eprintln!(
        "workload                         forms    cold ms  replay ms  proof   cache  invalid  target"
    );
    eprintln!(
        "-------------------------------- -------- --------- --------- ------- ------ -------- ------------------------"
    );
    for record in records {
        let forms = record
            .sizes
            .forms
            .map_or_else(|| "-".to_string(), |value| value.to_string());
        let cold = record
            .timing
            .solve_cold_ns
            .map_or_else(|| "-".to_string(), format_ms);
        let replay = record
            .timing
            .workspace_replay_ns
            .map_or_else(|| "-".to_string(), format_ms);
        let proof = record
            .metrics
            .proof_nodes
            .map_or_else(|| "-".to_string(), |value| value.to_string());
        let cache = record
            .metrics
            .cache_hits
            .map_or_else(|| "null".to_string(), |value| value.to_string());
        let invalid = record
            .metrics
            .invalidated_queries
            .map_or_else(|| "null".to_string(), |value| value.to_string());
        eprintln!(
            "{:<32} {:>8} {:>9} {:>9} {:>7} {:>6} {:>8} {}",
            record.workload.name(),
            forms,
            cold,
            replay,
            proof,
            cache,
            invalid,
            record.target.human()
        );
    }
    eprintln!(
        "replay is end-to-end Workspace analysis, including materialization and content-addressed proof persistence"
    );
    eprintln!(
        "--quick uses workload-specific reduced row counts (not a uniform ratio); --scale multiplies the selected row counts"
    );
}

fn format_ms(nanoseconds: u128) -> String {
    let milliseconds = nanoseconds / 1_000_000;
    let remainder = nanoseconds % 1_000_000;
    format!("{milliseconds}.{remainder:06}")
}

fn option_number(value: Option<u128>) -> String {
    value.map_or_else(|| "null".into(), |value| value.to_string())
}

fn option_string(value: Option<&str>) -> String {
    value.map_or_else(
        || "null".into(),
        |value| format!("\"{}\"", json_escape(value)),
    )
}

fn option_bool(value: Option<bool>) -> String {
    value.map_or_else(|| "null".into(), |value| value.to_string())
}

fn json_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            character if character.is_control() => {
                let _ = write!(escaped, "\\u{:04x}", character as u32);
            }
            character => escaped.push(character),
        }
    }
    escaped
}

fn stable_hash(bytes: &[u8]) -> String {
    let mut first = 0xcbf29ce484222325u64;
    let mut second = 0x84222325cbf29ce4u64;
    for byte in bytes {
        first ^= u64::from(*byte);
        first = first.wrapping_mul(0x100000001b3);
        second ^= u64::from(byte.wrapping_add(0x9d));
        second = second.rotate_left(7).wrapping_mul(0x100000001b3);
    }
    format!("{first:016x}{second:016x}")
}

fn generate(workload: Workload, quick: bool, scale: u64) -> Result<GeneratedWorkload, String> {
    let base = |full: u64, small: u64| -> Result<usize, String> {
        let count = if quick { small } else { full };
        let count = count
            .checked_mul(scale)
            .ok_or_else(|| format!("{}: --scale overflows row count", workload.name()))?;
        usize::try_from(count).map_err(|_| format!("{}: row count exceeds usize", workload.name()))
    };
    match workload {
        Workload::TenYearPersonalHistory => personal_history(base(120, 12)?),
        Workload::HighFrequencyLots => high_frequency_lots(base(300, 30)?),
        Workload::MultiCurrency => multi_currency(base(72, 12)?),
        Workload::CorporateActions => corporate_actions(base(96, 16)?),
        Workload::InvoicePaymentGraph => invoice_payment_graph(base(300, 30)?),
        Workload::OwnershipNetwork => ownership_network(base(250, 25)?),
        Workload::ConflictingImports => conflicting_imports(base(40, 8)?),
        Workload::OneRowCloseChange => one_row_close_change(base(120, 12)?),
        Workload::PackageUpgrade => package_upgrade(base(64, 8)?),
        Workload::AdversarialRecursion => adversarial_recursion(base(100, 10)?),
        Workload::LargeProofExplanation => large_proof(base(1000, 100)?),
        // Keep the default corpus at 1,000 forms; --scale 10 and --scale 100
        // are the explicit 10k/100k form-elaboration profiles.
        Workload::GenericFormElaboration => generic_form_elaboration(base(1000, 1000)?),
    }
}

fn header(book: &str, comments: &[&str]) -> String {
    let mut source = format!("; corpus generated by axiom-bench\nbook {book}\n");
    for comment in comments {
        let _ = writeln!(source, "; {comment}");
    }
    source.push('\n');
    source
}

#[allow(clippy::too_many_arguments)]
fn append_buy(
    source: &mut String,
    id: &str,
    date: String,
    quantity: u64,
    asset: &str,
    account: &str,
    cost: u64,
    currency: &str,
) {
    let _ = writeln!(
        source,
        "buy {id} on {date}\n  {quantity} {asset} into {account}\n  for {cost} {currency}"
    );
}

#[allow(clippy::too_many_arguments)]
fn append_sell(
    source: &mut String,
    id: &str,
    date: &str,
    quantity: u64,
    asset: &str,
    account: &str,
    proceeds: u64,
    currency: &str,
    lot: &str,
) {
    let _ = writeln!(
        source,
        "sell {id} on {date}\n  {quantity} {asset} from {account}\n  for {proceeds} {currency}\n  lot {lot}"
    );
}

fn append_quote(
    source: &mut String,
    id: &str,
    date: String,
    base: u64,
    asset: &str,
    quote: u64,
    currency: &str,
) {
    let _ = writeln!(
        source,
        "quote {id} on {date}\n  {base} {asset} = {quote} {currency}"
    );
}

fn date(index: usize, first_year: u16) -> String {
    let year = first_year + ((index / 12) % 10) as u16;
    let month = (index % 12) + 1;
    let day = (index % 27) + 1;
    format!("{year:04}-{month:02}-{day:02}")
}

fn personal_history(count: usize) -> Result<GeneratedWorkload, String> {
    let mut source = header(
        "personal-us",
        &["shape: monthly bank imports over ten years"],
    );
    for index in 0..count {
        append_buy(
            &mut source,
            &format!("personal/buy/{index:05}"),
            date(index, 2016),
            1 + (index % 4) as u64,
            "INDEX",
            "brokerage",
            100 + (index % 17) as u64,
            "USD",
        );
    }
    append_sell(
        &mut source,
        "personal/sale/close",
        "2025-12-31",
        1,
        "INDEX",
        "brokerage",
        250,
        "USD",
        "?lot",
    );
    source.push_str(
        "use lots/fifo for personal-us\nobserve settlement personal/sale/close 250 USD into checking\n",
    );
    Ok(GeneratedWorkload {
        source,
        changed_source: None,
        changed_kind: None,
        explain_goal: Some("gain:personal/sale/close".into()),
        semantic_supported: true,
        unsupported_reason: None,
        semantic_probe: None,
        semantic_probe_items: 0,
    })
}

fn high_frequency_lots(count: usize) -> Result<GeneratedWorkload, String> {
    let mut source = header(
        "brokerage-us",
        &["shape: high-frequency acquisitions and lot competition"],
    );
    for index in 0..count {
        append_buy(
            &mut source,
            &format!("trade/buy/{index:06}"),
            date(index, 2020),
            1,
            "ACME",
            "brokerage",
            10 + (index % 13) as u64,
            "USD",
        );
    }
    append_sell(
        &mut source,
        "trade/sale/close",
        "2029-12-31",
        1,
        "ACME",
        "brokerage",
        80,
        "USD",
        "?lot",
    );
    source.push_str("use lots/fifo for brokerage-us\n");
    Ok(GeneratedWorkload {
        source,
        changed_source: None,
        changed_kind: None,
        explain_goal: Some("gain:trade/sale/close".into()),
        semantic_supported: true,
        unsupported_reason: None,
        semantic_probe: None,
        semantic_probe_items: 0,
    })
}

fn multi_currency(count: usize) -> Result<GeneratedWorkload, String> {
    let mut source = header(
        "business-multi",
        &[
            "semantic: USD, EUR, GBP, and BTC exchange legs are unit-checked by the ontology API",
            "source: dated quotes remain evidence observations; the domain probe does not claim FX valuation",
        ],
    );
    let assets = ["SERV", "MACH", "DATA"];
    // Keep the crypto rail in the rotation even in the quick corpus; its
    // twelve rows cover all four currencies three times.
    let currencies = ["USD", "EUR", "GBP", "BTC"];
    for index in 0..count {
        let asset = assets[index % assets.len()];
        let currency = currencies[index % currencies.len()];
        append_buy(
            &mut source,
            &format!("fx/buy/{index:05}"),
            date(index, 2018),
            1,
            asset,
            "multi-broker",
            90 + (index % 23) as u64,
            currency,
        );
        if index % 3 == 0 {
            append_quote(
                &mut source,
                &format!("fx/quote/{index:05}"),
                date(index, 2018),
                1,
                currency,
                1 + (index % 7) as u64,
                "USD",
            );
        }
    }
    append_sell(
        &mut source,
        "fx/sale/close",
        "2029-12-31",
        1,
        "SERV",
        "multi-broker",
        120,
        "USD",
        "?lot",
    );
    source.push_str("observe settlement fx/sale/close 120 USD into multi-cash\n");
    Ok(GeneratedWorkload {
        source,
        changed_source: None,
        changed_kind: None,
        explain_goal: None,
        semantic_probe: Some(SemanticProbeKind::CurrencyExchange),
        semantic_probe_items: count,
        semantic_supported: true,
        unsupported_reason: None,
    })
}

fn corporate_actions(count: usize) -> Result<GeneratedWorkload, String> {
    let mut source = header(
        "portfolio-actions",
        &[
            "semantic: split, dividend, merge, and spin-off contracts are validated by the public contract API",
            "source: action annotations remain evidence; the domain probe checks exact conservation and ratios",
        ],
    );
    for index in 0..count {
        append_buy(
            &mut source,
            &format!("actions/buy/{index:05}"),
            date(index, 2017),
            2 + (index % 3) as u64,
            "FUND",
            "custody",
            200 + (index % 31) as u64,
            "USD",
        );
        if index % 8 == 0 {
            let _ = writeln!(
                source,
                "; corporate-action split/{} ratio 2:1 effective {}",
                index / 8,
                date(index, 2022)
            );
        }
        if index % 11 == 0 {
            let _ = writeln!(
                source,
                "observe settlement dividend/{index:05} {} USD into cash",
                5 + index % 19
            );
        }
        if index % 13 == 0 {
            let _ = writeln!(source, "; corporate-action merger/{:05}", index / 13);
        }
        if index % 17 == 0 {
            let _ = writeln!(source, "; corporate-action spin-off/{:05}", index / 17);
        }
    }
    append_sell(
        &mut source,
        "actions/sale/close",
        "2029-12-31",
        2,
        "FUND",
        "custody",
        400,
        "USD",
        "?lot",
    );
    source.push_str("observe settlement actions/sale/close 400 USD into cash\n");
    Ok(GeneratedWorkload {
        source,
        changed_source: None,
        changed_kind: None,
        explain_goal: None,
        semantic_supported: true,
        unsupported_reason: None,
        semantic_probe: Some(SemanticProbeKind::CorporateActions),
        semantic_probe_items: count,
    })
}

fn invoice_payment_graph(count: usize) -> Result<GeneratedWorkload, String> {
    let mut source = header(
        "invoice-payments",
        &[
            "shape: invoice -> settlement -> allocation graph (issued history is the authorization evidence)",
            "V0 representation: obligation, settlement, and satisfaction forms with stable references",
        ],
    );
    for index in 0..count {
        let customer = format!("customer/{:04}", index % 97);
        let amount = 100 + index % 37;
        let _ = writeln!(
            source,
            "obligation inv/{index:06}\n  debtor {customer}\n  creditor merchant\n  performance transfer {amount} USD\n  due 2028-12-31"
        );
        let _ = writeln!(
            source,
            "settlement payment/{index:06}\n  kind ach\n  from {customer}\n  to merchant\n  instrument USD\n  amount {amount} USD\n  state issued at 2028-01-01\n  state settled at 2028-01-03"
        );
        let _ = writeln!(
            source,
            "satisfy allocation/{index:06}\n  obligation inv/{index:06}\n  settlement payment/{index:06}\n  amount {amount} USD\n  state applied"
        );
    }
    Ok(GeneratedWorkload {
        source,
        changed_source: None,
        changed_kind: None,
        explain_goal: Some("obligation:inv/000000".into()),
        semantic_supported: true,
        unsupported_reason: None,
        semantic_probe: Some(SemanticProbeKind::InvoicePaymentGraph),
        semantic_probe_items: count,
    })
}

fn ownership_network(count: usize) -> Result<GeneratedWorkload, String> {
    let mut source = header(
        "ownership-network",
        &[
            "semantic: beneficial-owner role assignments and fractional shares are validated by the ontology API",
            "source: ownership edges remain evidence comments; the domain probe checks explicit role relations, not transitive closure",
        ],
    );
    for index in 0..count {
        let parent = index % 41;
        let child = (index * 7 + 3) % 113;
        let _ = writeln!(
            source,
            "; owns entity/{parent:03} entity/{child:03} weight {}%",
            1 + (index % 99)
        );
        let _ = writeln!(
            source,
            "observe position entity/{parent:03} {} SHARES",
            // Keep repeated observations for an entity consistent in the
            // accepted V0 projection; the ownership edge itself remains a
            // comment and is not parsed as a closure relation.
            1 + (parent % 23)
        );
    }
    Ok(GeneratedWorkload {
        source,
        changed_source: None,
        changed_kind: None,
        explain_goal: None,
        semantic_supported: true,
        unsupported_reason: None,
        semantic_probe: Some(SemanticProbeKind::OwnershipRoles),
        semantic_probe_items: count,
    })
}

fn conflicting_imports(count: usize) -> Result<GeneratedWorkload, String> {
    let mut source = header(
        "imports-conflict",
        &["shape: bank, broker, and custodian imports retain conflicting source claims"],
    );
    for index in 0..count {
        let _ = writeln!(source, "observe position checking {} USD", 100 + index);
        let _ = writeln!(source, "observe position checking {} USD", 200 + index);
        append_quote(
            &mut source,
            &format!("conflict/quote/a/{index:05}"),
            date(index, 2024),
            1,
            "ACME",
            50,
            "USD",
        );
        append_quote(
            &mut source,
            &format!("conflict/quote/b/{index:05}"),
            date(index, 2024),
            1,
            "ACME",
            51,
            "USD",
        );
    }
    append_buy(
        &mut source,
        "conflict/buy/0",
        "2024-01-01".into(),
        1,
        "ACME",
        "brokerage",
        20,
        "USD",
    );
    append_sell(
        &mut source,
        "conflict/sale/0",
        "2025-01-01",
        1,
        "ACME",
        "brokerage",
        30,
        "USD",
        "?lot",
    );
    source.push_str("decide conflict/sale/0 lot conflict/buy/0\ndecide conflict/sale/0 lot conflict/other\nobserve settlement conflict/sale/0 30 USD into checking\n");
    Ok(GeneratedWorkload {
        source,
        changed_source: None,
        changed_kind: None,
        explain_goal: Some("gain:conflict/sale/0".into()),
        semantic_supported: true,
        unsupported_reason: None,
        semantic_probe: None,
        semantic_probe_items: 0,
    })
}

fn one_row_close_change(count: usize) -> Result<GeneratedWorkload, String> {
    let mut source = header(
        "period-close",
        &["shape: one evidence row changes immediately before close"],
    );
    for index in 0..count {
        append_buy(
            &mut source,
            &format!("close/buy/{index:05}"),
            date(index, 2016),
            1,
            "INDEX",
            "brokerage",
            100,
            "USD",
        );
    }
    append_sell(
        &mut source,
        "close/sale",
        "2025-12-30",
        1,
        "INDEX",
        "brokerage",
        150,
        "USD",
        "?lot",
    );
    source.push_str(
        "use lots/fifo for period-close\nobserve settlement close/sale 150 USD into checking\n",
    );
    // Change the existing settlement row in place.  Keeping the row count
    // and its stable identity fixed makes this an input revision rather than
    // a synthetic append of a second close observation.
    let old_row = "observe settlement close/sale 150 USD into checking";
    let new_row = "observe settlement close/sale 151 USD into checking";
    if source.matches(old_row).count() != 1 {
        return Err("one-row close change source is missing its existing evidence row".into());
    }
    let changed = source.replacen(old_row, new_row, 1);
    if changed.matches(old_row).next().is_some()
        || changed.matches(new_row).count() != 1
        || changed.lines().count() != source.lines().count()
    {
        return Err("one-row close change did not replace exactly one evidence row".into());
    }
    Ok(GeneratedWorkload {
        source,
        changed_source: Some(changed),
        changed_kind: Some(ChangedKind::EvidenceRow),
        explain_goal: Some("gain:close/sale".into()),
        semantic_supported: true,
        unsupported_reason: None,
        semantic_probe: None,
        semantic_probe_items: 0,
    })
}

fn package_upgrade(count: usize) -> Result<GeneratedWorkload, String> {
    let mut source = header(
        "package-upgrade",
        &[
            "package: lots/fifo@1.0.0",
            "the package identity is supplied separately to the incremental probe",
        ],
    );
    for index in 0..count {
        append_buy(
            &mut source,
            &format!("upgrade/buy/{index:05}"),
            date(index, 2020),
            1,
            "ACME",
            "brokerage",
            10 + index as u64 % 9,
            "USD",
        );
    }
    append_sell(
        &mut source,
        "upgrade/sale",
        "2029-12-31",
        1,
        "ACME",
        "brokerage",
        40,
        "USD",
        "?lot",
    );
    source.push_str(
        "use lots/fifo for package-upgrade\nobserve settlement upgrade/sale 40 USD into checking\n",
    );
    // The package upgrade is a committed package object, not a source-text
    // edit.  Keep the source hash unchanged so the measurement cannot be
    // mistaken for a synthetic comment-only change.
    let changed_source = source.clone();
    Ok(GeneratedWorkload {
        source,
        changed_source: Some(changed_source),
        changed_kind: Some(ChangedKind::Package),
        explain_goal: Some("gain:upgrade/sale".into()),
        semantic_supported: true,
        unsupported_reason: None,
        semantic_probe: Some(SemanticProbeKind::PackageUpgrade),
        semantic_probe_items: count,
    })
}

fn adversarial_recursion(count: usize) -> Result<GeneratedWorkload, String> {
    let mut source = header(
        "recursive-rules",
        &[
            "semantic: positive recursive fixed-point rules are solved by the public logic API",
            "source: the evidence projection remains ordinary observations; the domain probe checks a real recursive program and cycle-without-base behavior",
        ],
    );
    for index in 0..count {
        let _ = writeln!(
            source,
            "observe position recursive/{index:04} {} TOKENS",
            1 + index % 17
        );
    }
    Ok(GeneratedWorkload {
        source,
        changed_source: None,
        changed_kind: None,
        explain_goal: None,
        semantic_supported: true,
        unsupported_reason: None,
        semantic_probe: Some(SemanticProbeKind::RecursiveLogic),
        // The Workspace source still scales to the requested corpus size;
        // keep the recursive solver witness bounded so the probe measures
        // fixed-point behavior rather than exhausting its finite resource
        // profile on a long transitive closure.
        semantic_probe_items: count.min(16),
    })
}

fn large_proof(count: usize) -> Result<GeneratedWorkload, String> {
    let mut source = header(
        "proof-explanation",
        &["shape: many acquisition roots feeding one explanation"],
    );
    for index in 0..count {
        append_buy(
            &mut source,
            &format!("proof/buy/{index:06}"),
            date(index, 2010),
            1,
            "PROOF",
            "brokerage",
            10 + index as u64 % 37,
            "USD",
        );
    }
    append_sell(
        &mut source,
        "proof/sale",
        "2029-12-31",
        1,
        "PROOF",
        "brokerage",
        100,
        "USD",
        "?lot",
    );
    source.push_str("use lots/fifo for proof-explanation\nobserve settlement proof/sale 100 USD into checking\n");
    Ok(GeneratedWorkload {
        source,
        changed_source: None,
        changed_kind: None,
        explain_goal: Some("gain:proof/sale".into()),
        semantic_supported: true,
        unsupported_reason: None,
        semantic_probe: None,
        semantic_probe_items: 0,
    })
}

fn generic_form_elaboration(count: usize) -> Result<GeneratedWorkload, String> {
    let mut source = String::with_capacity(count.saturating_mul(150));
    for index in 0..count {
        let _ = writeln!(
            source,
            "form generic/{index:06} : forms::types::Row\n  approved {}\n  count {index}\n  note \"row-{index:06}\"\n  total {}\n",
            index % 2 == 0,
            index * 3 + 7,
        );
    }
    // Edit one existing data row in place.  The form identity, schema and row
    // count remain stable while the canonical text value changes.
    let old_row = "  note \"row-000000\"\n";
    let new_row = "  note \"row-000000-edited\"\n";
    if source.matches(old_row).count() != 1 {
        return Err("generic form revision source is missing its first data row".into());
    }
    let changed = source.replacen(old_row, new_row, 1);
    if changed.matches(old_row).next().is_some()
        || changed.matches(new_row).count() != 1
        || changed.lines().count() != source.lines().count()
    {
        return Err("generic form revision did not replace exactly one data row".into());
    }
    Ok(GeneratedWorkload {
        source,
        changed_source: Some(changed),
        changed_kind: Some(ChangedKind::EvidenceRow),
        explain_goal: None,
        semantic_supported: true,
        unsupported_reason: None,
        semantic_probe: None,
        semantic_probe_items: count,
    })
}
