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
use axiom_ledger::hir::{
    AstDeclaration, AstDeclarationKind, AstModule, AstType, ModulePath, Name, QualifiedName,
    Span as HirSpan,
};
use axiom_ledger::incremental::{IncrementalDb, MemoOutcome, QueryError, QueryKey};
use axiom_ledger::ir::{Atom, Nominal, NominalKind, Term, Var};
use axiom_ledger::logic::{Clause, Goal, Literal, Program, SemanticContext, Solver, Truth};
use axiom_ledger::model::{Quantity, Unit};
use axiom_ledger::ontology::{
    Endpoint, ExchangeLeg, ExchangeRecord, Instrument, InstrumentKind, Obligation, OntologyError,
    Role, RoleAssignment, RoleAssignments, SatisfactionAllocation, Settlement, SettlementState,
    validate_satisfaction_network,
};
use axiom_ledger::package::{LotCandidate, PolicyPackage, Selection};
use axiom_ledger::package_compiler::{
    CompiledArtifact, FormFieldMappingV1, FormSurfaceV1, FormTemplateV1, PackageInput,
    SchemaCapability,
};
use axiom_ledger::package_lock::{
    Dependency, LockedPackage, Lockfile, PackageManifest, Version, VersionReq,
};
use axiom_ledger::render::render_why;
use axiom_ledger::settlement_projection::SettlementProjectionError;
use axiom_ledger::store::PolicyPackage as StorePolicyPackage;
use axiom_ledger::surface::SurfaceFile;
use axiom_ledger::workspace::{Workspace, WorkspaceError};

const SCHEMA: &str = "axiom-bench/v1";
const DEFAULT_SCALE: u64 = 1;
const FULL_SAMPLES: usize = 3;
const QUICK_SAMPLES: usize = 1;
/// Large package-authored corpora are measured as independent source commits.
/// This keeps the authoritative Workspace/object-store path intact while
/// bounding the live CST, elaborated forms, proof, and store to one batch.
const BOUNDED_BATCH_SIZE: usize = 256;

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
    SettlementStateProof,
}

impl Workload {
    const ALL: [Self; 13] = [
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
        Self::SettlementStateProof,
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
            Self::SettlementStateProof => "settlement-state-proof",
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
            Self::SettlementStateProof => Target::IncrementalSeconds,
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
            Self::SettlementStateProof => {
                "package-bound settlement histories through projection and independent proof persistence"
            }
        }
    }
}

/// Authoring surface used by the bounded SettlementStateV1 workload.  The
/// default is the historical direct schema spelling; compact mode is an
/// explicit opt-in so its v2 proof timings and identities cannot be confused
/// with the direct v1 baseline.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SettlementSurface {
    DirectV1,
    CompactV1,
}

impl SettlementSurface {
    fn parse(value: &str) -> Result<Self, String> {
        match value.trim().to_ascii_lowercase().replace('_', "-").as_str() {
            "direct" | "direct-v1" => Ok(Self::DirectV1),
            "compact" | "compact-v1" | "compact-v1-proof-v2" => Ok(Self::CompactV1),
            _ => Err(format!(
                "unknown settlement surface {value}; choose direct-v1 or compact-v1"
            )),
        }
    }

    fn profile(self) -> &'static str {
        match self {
            Self::DirectV1 => "direct_v1",
            Self::CompactV1 => "compact_v1_proof_v2",
        }
    }

    fn proof_version(self) -> &'static str {
        match self {
            Self::DirectV1 => axiom_ledger::settlement_proof::SETTLEMENT_STATE_PROOF_VERSION,
            Self::CompactV1 => axiom_ledger::settlement_proof::SETTLEMENT_STATE_PROOF_VERSION_V2,
        }
    }

    fn is_compact(self) -> bool {
        matches!(self, Self::CompactV1)
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
    settlement_setup_ns: Option<u128>,
    settlement_projection_call_ns: Option<u128>,
    settlement_persistence_boundary_ns: Option<u128>,
    settlement_proof_check_ns: Option<u128>,
    settlement_store_verify_ns: Option<u128>,
    settlement_source_revision_ns: Option<u128>,
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
    settlement_coverage_count: Option<usize>,
    settlement_coverage_hash: Option<String>,
    settlement_source_commit_hash: Option<String>,
    settlement_artifact_id_hash: Option<String>,
    settlement_artifact_hash: Option<String>,
    settlement_proof_hash: Option<String>,
    settlement_proof_bytes: Option<usize>,
    settlement_projection_commit_hash: Option<String>,
    settlement_binding_verified: Option<bool>,
    settlement_proof_check_verified: Option<bool>,
    settlement_store_verify_verified: Option<bool>,
    settlement_source_revision_verified: Option<bool>,
    settlement_atomic_negative_verified: Option<bool>,
    settlement_batch_source_commits_hash: Option<String>,
    settlement_batch_artifact_ids_hash: Option<String>,
    settlement_batch_artifacts_hash: Option<String>,
    settlement_batch_proofs_hash: Option<String>,
    settlement_batch_proof_bytes_total: Option<usize>,
    settlement_batch_projection_commits_hash: Option<String>,
    settlement_batch_coverage_hash: Option<String>,
    settlement_surface_profile: Option<&'static str>,
    settlement_proof_version: Option<&'static str>,
    execution_mode: Option<&'static str>,
    provenance_path: Option<&'static str>,
    batch_count: Option<usize>,
    batch_size: Option<usize>,
    max_batch_forms: Option<usize>,
    authority_binding_checks: Option<usize>,
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

/// The content-addressed identities that make the settlement workload a
/// reproducibility check rather than a timing-only probe.  Every field is
/// derived from a fresh workspace/store path and compared across runs.
#[derive(Clone, Debug, Eq, PartialEq)]
struct SettlementOracle {
    source_commit_hash: String,
    artifact_id_hash: String,
    artifact_hash: String,
    proof_hash: String,
    proof_bytes: usize,
    projection_commit_hash: String,
    coverage_hash: String,
    coverage_count: usize,
    proof_version: String,
}

struct SettlementExecution {
    workspace: Workspace,
    source_commit: axiom_ledger::store::CommitId,
    proof_id: axiom_ledger::store::SettlementStateProofId,
    forms: usize,
    oracle: SettlementOracle,
}

struct BoundedSettlementExecution {
    first: SettlementExecution,
    oracle: SettlementOracle,
    forms: usize,
    max_batch_forms: usize,
    authority_binding_checks: usize,
    source_revision_verified: Option<bool>,
    atomic_negative_verified: Option<bool>,
}

struct SettlementTimings {
    setup_ns: u128,
    document_elaboration_ns: u128,
    projection_call_ns: u128,
    persistence_boundary_ns: u128,
    proof_check_ns: u128,
    store_verify_ns: u128,
    source_revision_ns: Option<u128>,
}

#[derive(Clone, Debug)]
struct Options {
    quick: bool,
    scale: u64,
    workload: Option<Workload>,
    self_test: bool,
    help: bool,
    rss_probe: bool,
    settlement_boundary_probe: bool,
    settlement_surface: SettlementSurface,
    settlement_surface_explicit: bool,
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
    if options.settlement_surface_explicit
        && options.workload != Some(Workload::SettlementStateProof)
    {
        return Err("--settlement-surface requires --workload settlement-state-proof".to_string());
    }
    if options.settlement_surface_explicit && options.self_test {
        return Err("--settlement-surface cannot be combined with --self-test".to_string());
    }
    if options.rss_probe {
        let workload = options
            .workload
            .ok_or_else(|| "--rss-probe requires --workload".to_string())?;
        // One representative run keeps RSS a workload measurement rather
        // than the peak of a timing suite.
        let _ = measure_workload(
            workload,
            options.quick,
            options.scale,
            1,
            false,
            options.settlement_surface,
        )?;
        let peak = process_peak_memory_bytes();
        if workload == Workload::SettlementStateProof {
            println!(
                "{{\"schema\":\"{SCHEMA}\",\"kind\":\"rss_probe\",\"workload\":\"{}\",\"peak_memory_bytes\":{},\"settlement_surface_profile\":\"{}\",\"settlement_proof_version\":\"{}\"}}",
                workload.name(),
                option_number(peak.map(|value| value as u128)),
                options.settlement_surface.profile(),
                options.settlement_surface.proof_version(),
            );
        } else {
            println!(
                "{{\"schema\":\"{SCHEMA}\",\"kind\":\"rss_probe\",\"workload\":\"{}\",\"peak_memory_bytes\":{}}}",
                workload.name(),
                option_number(peak.map(|value| value as u128))
            );
        }
        return Ok(());
    }
    if options.settlement_boundary_probe {
        if options.workload != Some(Workload::SettlementStateProof) {
            return Err(
                "--settlement-boundary-probe requires --workload settlement-state-proof".into(),
            );
        }
        let profile = bounded_profile(
            Workload::SettlementStateProof,
            options.quick,
            options.scale,
            options.settlement_surface,
        )?;
        let source =
            settlement_state_batch(0, profile.total_items, false, profile.surface.is_compact())?;
        let execution = execute_settlement_path(&source, 0, profile.surface)?;
        println!(
            "{{\"schema\":\"{SCHEMA}\",\"kind\":\"settlement_boundary_probe\",\"workload\":\"settlement-state-proof\",\"forms\":{},\"proof_bytes\":{},\"settlement_surface_profile\":\"{}\",\"settlement_proof_version\":\"{}\"}}",
            execution.forms,
            execution.oracle.proof_bytes,
            profile.surface.profile(),
            profile.surface.proof_version(),
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
        let record = measure_workload(
            workload,
            options.quick,
            options.scale,
            samples,
            true,
            options.settlement_surface,
        )?;
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
            settlement_boundary_probe: false,
            settlement_surface: SettlementSurface::DirectV1,
            settlement_surface_explicit: false,
        };
        let args: Vec<String> = args.collect();
        let mut index = 0;
        while index < args.len() {
            match args[index].as_str() {
                "--quick" => options.quick = true,
                "--self-test" => options.self_test = true,
                "--rss-probe" => options.rss_probe = true,
                "--settlement-boundary-probe" => options.settlement_boundary_probe = true,
                "--settlement-surface" => {
                    index += 1;
                    let value = args.get(index).ok_or_else(|| {
                        "--settlement-surface requires direct-v1 or compact-v1".to_string()
                    })?;
                    options.settlement_surface = SettlementSurface::parse(value)?;
                    options.settlement_surface_explicit = true;
                }
                value if value.starts_with("--settlement-surface=") => {
                    options.settlement_surface = SettlementSurface::parse(
                        value.trim_start_matches("--settlement-surface="),
                    )?;
                    options.settlement_surface_explicit = true;
                }
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
--settlement-surface NAME\n\
                 settlement-state-proof authoring profile: direct-v1 (default) or compact-v1\n\
--self-test      run deterministic corpus/parser/proof/incremental checks\n\
--settlement-boundary-probe\n\
                 run only the public SettlementStateV1 persistence boundary"
    );
}

fn measure_workload(
    workload: Workload,
    quick: bool,
    scale: u64,
    samples: usize,
    isolate_peak_memory: bool,
    settlement_surface: SettlementSurface,
) -> Result<ResultRecord, String> {
    let generated = generate(workload, quick, scale, settlement_surface)?;
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
    if workload == Workload::SettlementStateProof {
        return measure_settlement_state_proof_workload(
            generated,
            workload,
            quick,
            scale,
            samples,
            isolate_peak_memory,
            settlement_surface,
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
        isolated_peak_memory_bytes(workload, quick, scale, SettlementSurface::DirectV1)?
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
    let profile = bounded_profile(workload, quick, scale, SettlementSurface::DirectV1)?;
    let mut runs = Vec::with_capacity(samples);
    for _ in 0..samples {
        runs.push(measure_generic_batches(profile)?);
    }
    let run = runs
        .last()
        .ok_or_else(|| "generic bounded benchmark produced no run".to_owned())?;
    let source_hash = run.source_hash.clone();
    let changed_source_hash = Some(run.changed_source_hash.clone());
    let median_field = |field: fn(&GenericBatchMeasurements) -> u128| {
        median(runs.iter().map(field).collect::<Vec<_>>())
    };
    let timing = Timing {
        generation_ns: median_field(|run| run.generation_ns),
        normalization_ns: Some(median_field(|run| run.normalization_ns)),
        parse_ns: Some(median_field(|run| run.parse_ns)),
        semantic_probe_ns: Some(median_field(|run| run.document_elaboration_ns)),
        package_compile_ns: Some(median_field(|run| run.package_compile_ns)),
        document_elaboration_ns: Some(median_field(|run| run.document_elaboration_ns)),
        schema_lookup_ns: Some(median_field(|run| run.schema_lookup_ns)),
        canonical_values_ns: Some(median_field(|run| run.canonical_values_ns)),
        changed_document_elaboration_ns: Some(median_field(|run| {
            run.changed_document_elaboration_ns
        })),
        ..Timing::default()
    };
    let mut metrics = Metrics {
        same_process_cache_replay_equal: None,
        independent_clean_recompute_equal: None,
        semantic_probe_items: Some(profile.total_items),
        semantic_probe_results: Some(run.forms),
        semantic_probe_api: Some("surface.package.document_elaboration"),
        canonical_value_count: Some(run.forms),
        schema_lookup_count: Some(run.forms),
        document_result_count: Some(run.forms),
        revision_result_count: Some(run.changed_forms),
        revision_mode: Some("bounded_batch_workspace_re_elaboration"),
        authority_binding_verified: Some(true),
        execution_mode: Some("bounded_batches"),
        provenance_path: Some("Workspace::elaborate_package_forms"),
        batch_count: Some(profile.batch_count()),
        batch_size: Some(profile.batch_size),
        max_batch_forms: Some(run.max_batch_forms),
        authority_binding_checks: Some(run.authority_binding_checks),
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
            "semantic_supported: every bounded batch is a lossless SurfaceFile loaded into a fresh Workspace, pinned to its persisted compiled artifact, then elaborated through Workspace::elaborate_package_forms; package_compile_ns includes package compilation/persistence per batch; document_elaboration_ns is the sum of authoritative batch elaboration scopes; source revision timings are bounded fresh-workspace re-elaboration and are not incremental-cache claims; peak_memory_bytes is isolated child-process peak RSS",
        ),
        ..Metrics::default()
    };

    metrics.peak_memory_bytes = if isolate_peak_memory {
        isolated_peak_memory_bytes(workload, quick, scale, SettlementSurface::DirectV1)?
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
            source_bytes: run.source_bytes,
            source_lines: run.source_lines,
            forms: Some(run.forms),
            changed_source_bytes: Some(run.changed_source_bytes),
            dependency_graph_nodes: None,
            dependency_graph_edges: None,
        },
        metrics,
        unsupported_reason: None,
        target: workload.target(),
        description: workload.description(),
    })
}

fn measure_settlement_state_proof_workload(
    generated: GeneratedWorkload,
    workload: Workload,
    quick: bool,
    scale: u64,
    samples: usize,
    isolate_peak_memory: bool,
    settlement_surface: SettlementSurface,
) -> Result<ResultRecord, String> {
    let profile = bounded_profile(workload, quick, scale, settlement_surface)?;
    let (source_hash, source_bytes, source_lines) = bounded_source_stats(profile, false)?;
    let (changed_hash, changed_source_bytes, _) = bounded_source_stats(profile, true)?;
    let changed_source_hash = Some(changed_hash);
    let execution = execute_bounded_settlement(profile, true)?;
    let independent = execute_bounded_settlement(profile, false)?;
    let deterministic = execution.oracle == independent.oracle;
    if !deterministic {
        return Err("settlement path is not deterministic across fresh workspaces".into());
    }

    let revised = execution.source_revision_verified;
    let atomic_negative = execution.atomic_negative_verified.unwrap_or(false);
    let single_batch = profile.batch_count() == 1;

    let settlement_timings =
        median_settlement_timings(profile, generated.changed_source.as_deref(), samples)?;
    let timing = Timing {
        generation_ns: median_bounded_generation(profile, samples)?,
        normalization_ns: Some(median_bounded_normalization(profile, samples)?),
        parse_ns: Some(median_bounded_surface_parse(profile, samples)?),
        settlement_setup_ns: Some(settlement_timings.setup_ns),
        document_elaboration_ns: Some(settlement_timings.document_elaboration_ns),
        settlement_projection_call_ns: Some(settlement_timings.projection_call_ns),
        settlement_persistence_boundary_ns: Some(settlement_timings.persistence_boundary_ns),
        settlement_proof_check_ns: Some(settlement_timings.proof_check_ns),
        settlement_store_verify_ns: Some(settlement_timings.store_verify_ns),
        settlement_source_revision_ns: settlement_timings.source_revision_ns,
        ..Timing::default()
    };

    let metrics = Metrics {
        // Settlement persistence is a Workspace/ObjectStore boundary, not a
        // semantic probe.  Leave semantic-probe fields unset because no
        // independent probe is timed for this workload.
        independent_clean_recompute_equal: Some(deterministic),
        settlement_coverage_count: Some(execution.oracle.coverage_count),
        settlement_coverage_hash: single_batch.then(|| execution.oracle.coverage_hash.clone()),
        settlement_source_commit_hash: single_batch
            .then(|| execution.oracle.source_commit_hash.clone()),
        settlement_artifact_id_hash: single_batch
            .then(|| execution.oracle.artifact_id_hash.clone()),
        settlement_artifact_hash: single_batch.then(|| execution.oracle.artifact_hash.clone()),
        settlement_proof_hash: single_batch.then(|| execution.oracle.proof_hash.clone()),
        settlement_proof_bytes: single_batch.then_some(execution.oracle.proof_bytes),
        settlement_projection_commit_hash: single_batch
            .then(|| execution.oracle.projection_commit_hash.clone()),
        settlement_binding_verified: Some(true),
        settlement_proof_check_verified: Some(true),
        settlement_store_verify_verified: Some(true),
        settlement_source_revision_verified: revised,
        settlement_atomic_negative_verified: Some(atomic_negative),
        settlement_batch_source_commits_hash: Some(execution.oracle.source_commit_hash.clone()),
        settlement_batch_artifact_ids_hash: Some(execution.oracle.artifact_id_hash.clone()),
        settlement_batch_artifacts_hash: Some(execution.oracle.artifact_hash.clone()),
        settlement_batch_proofs_hash: Some(execution.oracle.proof_hash.clone()),
        settlement_batch_proof_bytes_total: Some(execution.oracle.proof_bytes),
        settlement_batch_projection_commits_hash: Some(
            execution.oracle.projection_commit_hash.clone(),
        ),
        settlement_batch_coverage_hash: Some(execution.oracle.coverage_hash.clone()),
        settlement_surface_profile: Some(settlement_surface.profile()),
        settlement_proof_version: Some(settlement_surface.proof_version()),
        peak_memory_bytes: if isolate_peak_memory {
            isolated_peak_memory_bytes(workload, quick, scale, settlement_surface)?
        } else {
            None
        },
        revision_result_count: revised.map(|_| execution.forms),
        revision_mode: revised.map(|_| "source_revision_persistence"),
        authority_binding_verified: Some(true),
        execution_mode: Some("bounded_batches"),
        provenance_path: Some("Workspace::persist_settlement_state_proof"),
        batch_count: Some(profile.batch_count()),
        batch_size: Some(profile.batch_size),
        max_batch_forms: Some(execution.max_batch_forms),
        authority_binding_checks: Some(execution.authority_binding_checks),
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
            "settlement_state_proof: this is a bounded corpus of independent authoritative ledgers, never one combined proof; singular settlement authority fields are populated only for a one-batch run, while settlement_batch_* fields are deterministic corpus aggregates; every batch uses a fresh Workspace, persisted package compilation, source-commit artifact pinning, Workspace::persist_settlement_state_proof, proof checking, ObjectStore::verify, source revision, and atomic-negative validation; timing fields sum their named work across batches; peak_memory_bytes is isolated child-process peak RSS",
        ),
        ..Metrics::default()
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
            source_bytes,
            source_lines,
            forms: Some(execution.forms),
            changed_source_bytes: Some(changed_source_bytes),
            dependency_graph_nodes: None,
            dependency_graph_edges: None,
        },
        metrics,
        unsupported_reason: None,
        target: workload.target(),
        description: workload.description(),
    })
}

fn execute_bounded_settlement(
    profile: BoundedProfile,
    verify_mutations: bool,
) -> Result<BoundedSettlementExecution, String> {
    let mut first = None;
    let mut source_hash = HashState::new();
    let mut artifact_id_hash = HashState::new();
    let mut artifact_hash = HashState::new();
    let mut proof_hash = HashState::new();
    let mut projection_hash = HashState::new();
    let mut coverage_hash = HashState::new();
    let mut forms: usize = 0;
    let mut proof_bytes: usize = 0;
    let mut max_batch_forms = 0;
    let mut authority_binding_checks: usize = 0;
    let mut source_revision_verified = verify_mutations.then_some(true);
    let mut atomic_negative_verified = verify_mutations.then_some(true);
    for start in (0..profile.total_items).step_by(profile.batch_size) {
        let count = (profile.total_items - start).min(profile.batch_size);
        let source = bounded_source(profile, start, count, false)?;
        let execution = execute_settlement_path(&source, start, profile.surface)?;
        if execution.forms != count {
            return Err(format!(
                "settlement bounded batch at {start} returned {} forms, expected {count}",
                execution.forms
            ));
        }
        source_hash.update(execution.oracle.source_commit_hash.as_bytes());
        artifact_id_hash.update(execution.oracle.artifact_id_hash.as_bytes());
        artifact_hash.update(execution.oracle.artifact_hash.as_bytes());
        proof_hash.update(execution.oracle.proof_hash.as_bytes());
        projection_hash.update(execution.oracle.projection_commit_hash.as_bytes());
        coverage_hash.update(execution.oracle.coverage_hash.as_bytes());
        forms = forms.saturating_add(execution.forms);
        proof_bytes = proof_bytes.saturating_add(execution.oracle.proof_bytes);
        max_batch_forms = max_batch_forms.max(execution.forms);
        authority_binding_checks = authority_binding_checks.saturating_add(1);
        if verify_mutations {
            let changed = bounded_source(profile, start, count, true)?;
            source_revision_verified = Some(
                source_revision_verified.unwrap_or(true)
                    && validate_settlement_source_revision(&execution, &changed)?,
            );
            let malformed = if profile.surface.is_compact() {
                source.replacen(&format!("a {}", 100 + start), "a -1", 1)
            } else {
                source.replacen(&format!("amount {}", 100 + start), "amount -1", 1)
            };
            atomic_negative_verified = Some(
                atomic_negative_verified.unwrap_or(true)
                    && validate_settlement_atomic_negative(&source, malformed, profile.surface)?,
            );
        }
        if first.is_none() {
            first = Some(execution);
        }
    }
    let first = first.ok_or_else(|| "settlement bounded corpus is empty".to_owned())?;
    let oracle = SettlementOracle {
        source_commit_hash: source_hash.finish(),
        artifact_id_hash: artifact_id_hash.finish(),
        artifact_hash: artifact_hash.finish(),
        proof_hash: proof_hash.finish(),
        proof_bytes,
        projection_commit_hash: projection_hash.finish(),
        coverage_hash: coverage_hash.finish(),
        coverage_count: forms,
        proof_version: first.oracle.proof_version.clone(),
    };
    Ok(BoundedSettlementExecution {
        first,
        oracle,
        forms,
        max_batch_forms,
        authority_binding_checks,
        source_revision_verified,
        atomic_negative_verified,
    })
}

fn execute_settlement_path(
    source: &str,
    occurrence_start: usize,
    settlement_surface: SettlementSurface,
) -> Result<SettlementExecution, String> {
    let (mut workspace, source_commit, artifact_id, artifact) =
        settlement_workspace_fixture(source, settlement_surface)?;
    let bound = workspace
        .elaborate_package_forms(source_commit)
        .map_err(|error| format!("settlement document elaboration failed: {error}"))?;
    if bound.source_commit() != source_commit
        || bound.compiled_artifact() != artifact_id
        || bound.artifact_hash() != artifact.artifact_hash()
    {
        return Err("settlement forms lost source/artifact authority binding".into());
    }
    let forms = bound.forms().len();
    if forms == 0 {
        return Err(format!(
            "settlement workload has {forms} forms; expected at least one"
        ));
    }
    let projection = workspace
        .project_settlement_states(source_commit)
        .map_err(|error| format!("settlement projection failed: {error}"))?;
    if projection.source_commit() != source_commit
        || projection.compiled_artifact() != artifact_id
        || projection.artifact_hash() != artifact.artifact_hash()
        || projection.records().len() != forms
    {
        return Err("settlement projection lost source/artifact binding or coverage".into());
    }
    let expected_occurrences = (occurrence_start..occurrence_start + forms)
        .map(|index| format!("settlement/{index:06}"))
        .collect::<Vec<_>>();
    let actual_occurrences = projection
        .records()
        .map(|record| record.occurrence.as_str().to_owned())
        .collect::<Vec<_>>();
    if actual_occurrences != expected_occurrences {
        return Err("settlement projection changed exact source order".into());
    }

    let persisted = workspace
        .persist_settlement_state_proof(source_commit)
        .map_err(|error| format!("settlement proof persistence failed: {error}"))?;
    let proof = workspace
        .store()
        .settlement_state_proof(persisted.proof_id)
        .map_err(|error| format!("settlement proof lookup failed: {error}"))?;
    if proof.version != settlement_surface.proof_version() {
        return Err(format!(
            "settlement profile {} persisted unexpected proof version {}",
            settlement_surface.profile(),
            proof.version
        ));
    }
    proof
        .check(workspace.store())
        .map_err(|error| format!("independent settlement proof check failed: {error}"))?;
    let source_value = workspace
        .store()
        .commit(source_commit)
        .map_err(|error| format!("settlement source lookup failed: {error}"))?;
    let child = workspace
        .store()
        .commit(persisted.projection_commit)
        .map_err(|error| format!("settlement child lookup failed: {error}"))?;
    if persisted.source_commit != source_commit
        || proof.source_commit != source_commit
        || proof.compiled_artifact != artifact_id
        || proof.compiled_artifact_hash != artifact.artifact_hash()
        || child.parents != vec![source_commit]
        || child.settlement_proofs != vec![persisted.proof_id]
        || !child.evidence.is_empty()
        || child.compiled_artifact != source_value.compiled_artifact
        || child.packages != source_value.packages
    {
        return Err("settlement proof or typed child commit is not bound to source".into());
    }
    workspace
        .store()
        .verify()
        .map_err(|error| format!("settlement store verification failed: {error}"))?;

    let oracle = SettlementOracle {
        source_commit_hash: source_commit.hash().to_string(),
        artifact_id_hash: artifact_id.hash().to_string(),
        artifact_hash: artifact.artifact_hash().to_string(),
        proof_hash: persisted.proof_id.hash().to_string(),
        projection_commit_hash: persisted.projection_commit.hash().to_string(),
        coverage_hash: proof.coverage_hash.to_string(),
        coverage_count: proof.coverage().len(),
        proof_bytes: proof.canonical_bytes().len(),
        proof_version: proof.version.clone(),
    };
    Ok(SettlementExecution {
        workspace,
        source_commit,
        proof_id: persisted.proof_id,
        forms,
        oracle,
    })
}

fn validate_settlement_source_revision(
    execution: &SettlementExecution,
    changed: &str,
) -> Result<bool, String> {
    let mut workspace = execution.workspace.clone();
    let corrected_source = workspace
        .correct_source(execution.source_commit, changed.as_bytes())
        .map_err(|error| format!("settlement source revision failed: {error}"))?;
    let corrected = workspace
        .persist_settlement_state_proof(corrected_source.commit_id())
        .map_err(|error| format!("settlement corrected proof failed: {error}"))?;
    let proof = workspace
        .store()
        .settlement_state_proof(corrected.proof_id)
        .map_err(|error| format!("settlement corrected proof lookup failed: {error}"))?;
    proof
        .check(workspace.store())
        .map_err(|error| format!("settlement corrected proof check failed: {error}"))?;
    let child = workspace
        .store()
        .commit(corrected.projection_commit)
        .map_err(|error| format!("settlement corrected child lookup failed: {error}"))?;
    let original = workspace
        .store()
        .settlement_state_proof(execution.proof_id)
        .map_err(|error| format!("settlement original proof lookup failed: {error}"))?;
    original
        .check(workspace.store())
        .map_err(|error| format!("settlement original proof was not retained: {error}"))?;
    workspace
        .store()
        .verify()
        .map_err(|error| format!("settlement corrected store verification failed: {error}"))?;
    Ok(corrected.source_commit != execution.source_commit
        && corrected.proof_id != execution.proof_id
        && child.parents == vec![corrected.source_commit]
        && child.settlement_proofs == vec![corrected.proof_id])
}

fn validate_settlement_atomic_negative(
    source: &str,
    invalid_source: String,
    settlement_surface: SettlementSurface,
) -> Result<bool, String> {
    let (mut workspace, source_commit, _, _) =
        settlement_workspace_fixture(source, settlement_surface)?;
    let corrected = workspace
        .correct_source(source_commit, invalid_source.as_bytes())
        .map_err(|error| format!("settlement negative correction failed: {error}"))?;
    let before = workspace.store().len();
    let failed = match workspace.persist_settlement_state_proof(corrected.commit_id()) {
        Err(WorkspaceError::SettlementProjection(SettlementProjectionError::Ontology(
            OntologyError::InvalidQuantity {
                context: "a settlement state amount",
            },
        ))) => true,
        Err(error) => {
            return Err(format!(
                "settlement negative amount returned the wrong public error: {error}"
            ));
        }
        Ok(_) => {
            return Err("settlement negative amount unexpectedly persisted".into());
        }
    };
    let unchanged = workspace.store().len() == before;
    workspace
        .store()
        .verify()
        .map_err(|error| format!("settlement negative store verification failed: {error}"))?;
    if !failed || !unchanged {
        return Err("settlement negative amount violated atomic persistence".into());
    }
    Ok(true)
}

fn settlement_workspace_fixture(
    source: &str,
    settlement_surface: SettlementSurface,
) -> Result<
    (
        Workspace,
        axiom_ledger::store::CommitId,
        axiom_ledger::store::CompiledArtifactId,
        CompiledArtifact,
    ),
    String,
> {
    let mut workspace = Workspace::new();
    let (package, lockfile) = settlement_state_package_input(settlement_surface)?;
    let (source_commit, artifact_id, artifact) =
        pin_settlement_workspace(&mut workspace, source, package, &lockfile)?;
    Ok((workspace, source_commit, artifact_id, artifact))
}

fn pin_settlement_workspace(
    workspace: &mut Workspace,
    source: &str,
    package: PackageInput,
    lockfile: &Lockfile,
) -> Result<
    (
        axiom_ledger::store::CommitId,
        axiom_ledger::store::CompiledArtifactId,
        CompiledArtifact,
    ),
    String,
> {
    let loaded = workspace
        .load_source("benchmark/settlement-state-proof", source.as_bytes())
        .map_err(|error| format!("settlement source load failed: {error}"))?;
    let (artifact_id, artifact) = workspace
        .compile_packages_persisted([package], lockfile)
        .map_err(|error| format!("settlement package persistence failed: {error}"))?;
    let pinned = workspace
        .commit_with_compiled_artifact(loaded.commit_id(), artifact_id)
        .map_err(|error| format!("settlement artifact pinning failed: {error}"))?;
    Ok((pinned.commit_id(), artifact_id, artifact))
}

fn settlement_state_package_input(
    settlement_surface: SettlementSurface,
) -> Result<(PackageInput, Lockfile), String> {
    let manifest = PackageManifest::new(
        "payments",
        Version::new(1, 0, 0),
        "settlement-state-benchmark-v1",
    );
    let schema = QualifiedName {
        module: ModulePath::root(Name::new("types").map_err(|error| format!("{error:?}"))?),
        name: Name::new("SettlementState").map_err(|error| format!("{error:?}"))?,
    };
    let module = axiom_ledger::hir::lower(AstModule {
        path: ModulePath::root(Name::new("types").map_err(|error| format!("{error:?}"))?),
        declarations: vec![AstDeclaration {
            name: "SettlementState".to_owned(),
            kind: AstDeclarationKind::Type(AstType::Record {
                fields: vec![
                    ("settlement".to_owned(), AstType::Text),
                    ("kind".to_owned(), AstType::Text),
                    ("state".to_owned(), AstType::Text),
                    ("at".to_owned(), AstType::Text),
                    ("from".to_owned(), AstType::Text),
                    ("to".to_owned(), AstType::Text),
                    ("instrument".to_owned(), AstType::Text),
                    ("amount".to_owned(), AstType::Decimal),
                ],
                open_tail: None,
            }),
            span: HirSpan::default(),
        }],
    });
    let mut package = PackageInput::new(manifest.clone(), [module])
        .with_schema_capability(schema.clone(), SchemaCapability::SettlementStateV1);
    if settlement_surface.is_compact() {
        let template = QualifiedName {
            module: ModulePath::root(Name::new("forms").map_err(|error| format!("{error:?}"))?),
            name: Name::new("CompactSettlement").map_err(|error| format!("{error:?}"))?,
        };
        package = package.with_form_surface(FormSurfaceV1::new([FormTemplateV1::new(
            template,
            schema,
            [
                ("s", "settlement"),
                ("k", "kind"),
                ("x", "state"),
                ("d", "at"),
                ("f", "from"),
                ("t", "to"),
                ("i", "instrument"),
                ("a", "amount"),
            ]
            .into_iter()
            .map(|(source, target)| FormFieldMappingV1::new(source, target)),
        )]));
    }
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
    Ok((package, lockfile))
}

fn median_settlement_timings(
    profile: BoundedProfile,
    changed: Option<&str>,
    samples: usize,
) -> Result<SettlementTimings, String> {
    let mut setup = Vec::with_capacity(samples);
    let mut document = Vec::with_capacity(samples);
    let mut projection_call = Vec::with_capacity(samples);
    let mut persistence_boundary = Vec::with_capacity(samples);
    let mut proof_check = Vec::with_capacity(samples);
    let mut store_verify = Vec::with_capacity(samples);
    let mut source_revision = Vec::with_capacity(samples);
    for _ in 0..samples {
        let mut setup_total = 0;
        let mut document_total = 0;
        let mut projection_call_total = 0;
        let mut persistence_total = 0;
        let mut proof_total = 0;
        let mut verify_total = 0;
        for start_index in (0..profile.total_items).step_by(profile.batch_size) {
            let count = (profile.total_items - start_index).min(profile.batch_size);
            let source = bounded_source(profile, start_index, count, false)?;
            let mut workspace = Workspace::new();
            let (package_input, lockfile) = settlement_state_package_input(profile.surface)?;
            let start = Instant::now();
            let (commit, artifact_id, artifact) =
                pin_settlement_workspace(&mut workspace, &source, package_input, &lockfile)?;
            setup_total += start.elapsed().as_nanos();
            let document_start = Instant::now();
            let forms = workspace
                .elaborate_package_forms(commit)
                .map_err(|error| format!("settlement document elaboration failed: {error}"))?;
            if forms.source_commit() != commit
                || forms.compiled_artifact() != artifact_id
                || forms.artifact_hash() != artifact.artifact_hash()
                || forms.forms().len() != count
            {
                return Err("settlement timing batch lost source/artifact binding".into());
            }
            black_box((forms.forms().len(), artifact.artifact_hash()));
            document_total += document_start.elapsed().as_nanos();
            // Time the full public projection call. It internally re-elaborates
            // the pinned document, so this intentionally overlaps
            // document_elaboration_ns; no bypass API is used.
            let projection_call_start = Instant::now();
            let projected = workspace
                .project_settlement_states(commit)
                .map_err(|error| format!("settlement projection failed: {error}"))?;
            if projected.records().len() != count {
                return Err("settlement timing batch changed projection coverage".into());
            }
            black_box(projected.records().len());
            projection_call_total += projection_call_start.elapsed().as_nanos();
            let start = Instant::now();
            let persisted = workspace
                .persist_settlement_state_proof(commit)
                .map_err(|error| format!("settlement proof persistence failed: {error}"))?;
            black_box(persisted.proof_id.hash());
            persistence_total += start.elapsed().as_nanos();
            let proof = workspace
                .store()
                .settlement_state_proof(persisted.proof_id)
                .map_err(|error| format!("settlement proof lookup failed: {error}"))?;
            let start = Instant::now();
            proof
                .check(workspace.store())
                .map_err(|error| format!("settlement proof check failed: {error}"))?;
            black_box(proof.coverage().len());
            proof_total += start.elapsed().as_nanos();
            let start = Instant::now();
            workspace
                .store()
                .verify()
                .map_err(|error| format!("settlement store verification failed: {error}"))?;
            verify_total += start.elapsed().as_nanos();
        }
        setup.push(setup_total);
        document.push(document_total);
        projection_call.push(projection_call_total);
        persistence_boundary.push(persistence_total);
        proof_check.push(proof_total);
        store_verify.push(verify_total);
        if changed.is_some() {
            let start = Instant::now();
            for start_index in (0..profile.total_items).step_by(profile.batch_size) {
                let count = (profile.total_items - start_index).min(profile.batch_size);
                let source = bounded_source(profile, start_index, count, true)?;
                let (mut workspace, commit, _, _) =
                    settlement_workspace_fixture(&source, profile.surface)?;
                let corrected = workspace
                    .persist_settlement_state_proof(commit)
                    .map_err(|error| format!("settlement source revision failed: {error}"))?;
                black_box(corrected.proof_id.hash());
            }
            source_revision.push(start.elapsed().as_nanos());
        }
    }
    Ok(SettlementTimings {
        setup_ns: median(setup),
        document_elaboration_ns: median(document),
        projection_call_ns: median(projection_call),
        persistence_boundary_ns: median(persistence_boundary),
        proof_check_ns: median(proof_check),
        store_verify_ns: median(store_verify),
        source_revision_ns: (!source_revision.is_empty()).then(|| median(source_revision)),
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BoundedCorpus {
    GenericForms,
    SettlementStates,
}

#[derive(Clone, Copy, Debug)]
struct BoundedProfile {
    corpus: BoundedCorpus,
    surface: SettlementSurface,
    total_items: usize,
    batch_size: usize,
}

impl BoundedProfile {
    fn batch_count(self) -> usize {
        self.total_items.div_ceil(self.batch_size)
    }
}

fn bounded_profile(
    workload: Workload,
    quick: bool,
    scale: u64,
    settlement_surface: SettlementSurface,
) -> Result<BoundedProfile, String> {
    let base: u64 = 1000;
    let total = base
        .checked_mul(scale)
        .ok_or_else(|| format!("{}: --scale overflows row count", workload.name()))?;
    let total = usize::try_from(total)
        .map_err(|_| format!("{}: row count exceeds usize", workload.name()))?;
    match workload {
        Workload::GenericFormElaboration => Ok(BoundedProfile {
            corpus: BoundedCorpus::GenericForms,
            surface: SettlementSurface::DirectV1,
            total_items: total,
            batch_size: BOUNDED_BATCH_SIZE,
        }),
        Workload::SettlementStateProof => {
            let base: u64 = if quick { 1 } else { 64 };
            let total = base
                .checked_mul(scale)
                .ok_or_else(|| format!("{}: --scale overflows row count", workload.name()))?;
            let total = usize::try_from(total)
                .map_err(|_| format!("{}: row count exceeds usize", workload.name()))?;
            Ok(BoundedProfile {
                corpus: BoundedCorpus::SettlementStates,
                surface: settlement_surface,
                total_items: total,
                batch_size: BOUNDED_BATCH_SIZE,
            })
        }
        _ => Err(format!("{} is not a bounded benchmark", workload.name())),
    }
}

fn bounded_source(
    profile: BoundedProfile,
    start: usize,
    count: usize,
    changed: bool,
) -> Result<String, String> {
    match profile.corpus {
        BoundedCorpus::GenericForms => generic_form_batch(start, count, changed),
        BoundedCorpus::SettlementStates => {
            settlement_state_batch(start, count, changed, profile.surface.is_compact())
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct HashState {
    first: u64,
    second: u64,
}

impl HashState {
    fn new() -> Self {
        Self {
            first: 0xcbf29ce484222325,
            second: 0x84222325cbf29ce4,
        }
    }

    fn update(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.first ^= u64::from(*byte);
            self.first = self.first.wrapping_mul(0x100000001b3);
            self.second ^= u64::from(byte.wrapping_add(0x9d));
            self.second = self.second.rotate_left(7).wrapping_mul(0x100000001b3);
        }
    }

    fn finish(self) -> String {
        format!("{:016x}{:016x}", self.first, self.second)
    }
}

fn bounded_source_stats(
    profile: BoundedProfile,
    changed: bool,
) -> Result<(String, usize, usize), String> {
    let mut hash = HashState::new();
    let mut bytes: usize = 0;
    let mut lines: usize = 0;
    for start in (0..profile.total_items).step_by(profile.batch_size) {
        let count = (profile.total_items - start).min(profile.batch_size);
        let source = bounded_source(profile, start, count, changed && start == 0)?;
        hash.update(source.as_bytes());
        bytes = bytes.saturating_add(source.len());
        lines = lines.saturating_add(source.lines().count());
    }
    Ok((hash.finish(), bytes, lines))
}

#[derive(Clone, Debug, Default)]
struct GenericBatchMeasurements {
    source_hash: String,
    changed_source_hash: String,
    source_bytes: usize,
    source_lines: usize,
    changed_source_bytes: usize,
    forms: usize,
    changed_forms: usize,
    max_batch_forms: usize,
    authority_binding_checks: usize,
    generation_ns: u128,
    normalization_ns: u128,
    parse_ns: u128,
    package_compile_ns: u128,
    document_elaboration_ns: u128,
    schema_lookup_ns: u128,
    canonical_values_ns: u128,
    changed_document_elaboration_ns: u128,
    first_value_hash: Option<String>,
    changed_first_value_hash: Option<String>,
}

fn measure_generic_batches(profile: BoundedProfile) -> Result<GenericBatchMeasurements, String> {
    let (source_hash, source_bytes, source_lines) = bounded_source_stats(profile, false)?;
    let (changed_source_hash, changed_source_bytes, _) = bounded_source_stats(profile, true)?;
    let mut result = GenericBatchMeasurements {
        source_hash,
        changed_source_hash,
        source_bytes,
        source_lines,
        changed_source_bytes,
        ..GenericBatchMeasurements::default()
    };

    let generation_start = Instant::now();
    for start in (0..profile.total_items).step_by(profile.batch_size) {
        let count = (profile.total_items - start).min(profile.batch_size);
        black_box(bounded_source(profile, start, count, false)?.len());
    }
    result.generation_ns = generation_start.elapsed().as_nanos();

    for start in (0..profile.total_items).step_by(profile.batch_size) {
        let count = (profile.total_items - start).min(profile.batch_size);
        let source = bounded_source(profile, start, count, false)?;
        result.max_batch_forms = result.max_batch_forms.max(count);
        let parse_start = Instant::now();
        let surface = SurfaceFile::parse(source.clone());
        let parsed_forms = surface.forms().count();
        result.parse_ns = result
            .parse_ns
            .saturating_add(parse_start.elapsed().as_nanos());
        if parsed_forms != count {
            return Err(format!(
                "generic bounded batch at {start} parsed {parsed_forms} forms, expected {count}"
            ));
        }
        let normalization_start = Instant::now();
        black_box(surface.canonical().len());
        result.normalization_ns = result
            .normalization_ns
            .saturating_add(normalization_start.elapsed().as_nanos());

        let compile_start = Instant::now();
        let (workspace, commit, artifact) = generic_workspace_fixture(&source)?;
        result.package_compile_ns = result
            .package_compile_ns
            .saturating_add(compile_start.elapsed().as_nanos());
        let document_start = Instant::now();
        let bound = workspace
            .elaborate_package_forms(commit)
            .map_err(|error| format!("generic bounded document elaboration failed: {error}"))?;
        result.document_elaboration_ns = result
            .document_elaboration_ns
            .saturating_add(document_start.elapsed().as_nanos());
        let committed_artifact = workspace
            .store()
            .commit(commit)
            .map_err(|error| format!("generic bounded commit lookup failed: {error}"))?
            .compiled_artifact
            .ok_or_else(|| "generic bounded commit lost artifact pin".to_owned())?;
        if bound.source_commit() != commit
            || bound.artifact_hash() != artifact.artifact_hash()
            || bound.compiled_artifact() != committed_artifact
        {
            return Err("generic bounded forms lost source/artifact authority binding".into());
        }
        let forms = bound.forms();
        if forms.len() != count {
            return Err(format!(
                "generic bounded elaboration returned {} forms, expected {count}",
                forms.len()
            ));
        }
        let package_root = artifact.package_roots()[0];
        let schema = generic_form_schema_name()?;
        let schema_start = Instant::now();
        for form in forms {
            let resolved = artifact
                .resolve_record_schema(package_root, &schema)
                .map_err(|error| format!("generic bounded schema lookup failed: {error}"))?;
            if form.package_root() != package_root
                || form.schema().schema_id() != resolved.schema_id()
            {
                return Err("generic bounded form schema escaped the pinned package root".into());
            }
            black_box(resolved.schema_id());
        }
        result.schema_lookup_ns = result
            .schema_lookup_ns
            .saturating_add(schema_start.elapsed().as_nanos());
        let canonical_start = Instant::now();
        let mut first_value_hash = None;
        for form in forms {
            let canonical = form
                .schema()
                .check_concrete_record(form.value().record())
                .map_err(|error| {
                    format!("generic bounded canonical value check failed: {error}")
                })?;
            if start == 0 && first_value_hash.is_none() {
                first_value_hash = Some(canonical.content_hash().to_string());
            }
            black_box(canonical.content_hash());
        }
        result.canonical_values_ns = result
            .canonical_values_ns
            .saturating_add(canonical_start.elapsed().as_nanos());
        result.forms = result.forms.saturating_add(forms.len());
        result.authority_binding_checks = result.authority_binding_checks.saturating_add(1);
        result.first_value_hash = result.first_value_hash.or(first_value_hash);
    }

    for start in (0..profile.total_items).step_by(profile.batch_size) {
        let count = (profile.total_items - start).min(profile.batch_size);
        let source = bounded_source(profile, start, count, true)?;
        let (workspace, commit, artifact) = generic_workspace_fixture(&source)?;
        let revision_start = Instant::now();
        let bound = workspace
            .elaborate_package_forms(commit)
            .map_err(|error| format!("generic bounded changed elaboration failed: {error}"))?;
        result.changed_document_elaboration_ns = result
            .changed_document_elaboration_ns
            .saturating_add(revision_start.elapsed().as_nanos());
        if bound.source_commit() != commit || bound.artifact_hash() != artifact.artifact_hash() {
            return Err(
                "generic bounded changed forms lost source/artifact authority binding".into(),
            );
        }
        result.changed_forms = result.changed_forms.saturating_add(bound.forms().len());
        if bound.forms().len() != count {
            return Err("generic bounded changed batch form count mismatch".into());
        }
        if start == 0 {
            result.changed_first_value_hash = bound
                .forms()
                .first()
                .map(|form| form.value().content_hash().to_string());
        }
    }
    if result.forms != profile.total_items || result.changed_forms != profile.total_items {
        return Err(format!(
            "generic bounded corpus count mismatch: {} / {} vs {}",
            result.forms, result.changed_forms, profile.total_items
        ));
    }
    if result.first_value_hash == result.changed_first_value_hash {
        return Err("generic bounded source revision did not change a canonical value".into());
    }
    Ok(result)
}

fn median_bounded_surface_parse(profile: BoundedProfile, samples: usize) -> Result<u128, String> {
    let mut values = Vec::with_capacity(samples);
    for _ in 0..samples {
        let start = Instant::now();
        for batch_start in (0..profile.total_items).step_by(profile.batch_size) {
            let count = (profile.total_items - batch_start).min(profile.batch_size);
            let source = bounded_source(profile, batch_start, count, false)?;
            let surface = SurfaceFile::parse(source);
            if surface.forms().count() != count {
                return Err("bounded source parser count mismatch".into());
            }
            black_box(surface.forms().count());
        }
        values.push(start.elapsed().as_nanos());
    }
    Ok(median(values))
}

fn median_bounded_generation(profile: BoundedProfile, samples: usize) -> Result<u128, String> {
    let mut values = Vec::with_capacity(samples);
    for _ in 0..samples {
        let start = Instant::now();
        for batch_start in (0..profile.total_items).step_by(profile.batch_size) {
            let count = (profile.total_items - batch_start).min(profile.batch_size);
            black_box(bounded_source(profile, batch_start, count, false)?.len());
        }
        values.push(start.elapsed().as_nanos());
    }
    Ok(median(values))
}

fn median_bounded_normalization(profile: BoundedProfile, samples: usize) -> Result<u128, String> {
    let mut values = Vec::with_capacity(samples);
    for _ in 0..samples {
        let start = Instant::now();
        for batch_start in (0..profile.total_items).step_by(profile.batch_size) {
            let count = (profile.total_items - batch_start).min(profile.batch_size);
            let source = bounded_source(profile, batch_start, count, false)?;
            black_box(SurfaceFile::parse(source).canonical().len());
        }
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
        let generated = generate(workload, quick, scale, SettlementSurface::DirectV1)?;
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
    settlement_surface: SettlementSurface,
) -> Result<Option<usize>, String> {
    let executable = env::current_exe()
        .map_err(|error| format!("cannot locate benchmark executable for RSS probe: {error}"))?;
    let mut command = Command::new(executable);
    command
        .arg("--rss-probe")
        .arg("--workload")
        .arg(workload.name());
    if settlement_surface.is_compact() {
        command.arg("--settlement-surface").arg("compact-v1");
    }
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
        "{{\"schema\":\"{SCHEMA}\",\"kind\":\"rss_probe\",\"workload\":\"{}\",",
        workload.name()
    );
    let payload = line
        .strip_prefix(&prefix)
        .ok_or_else(|| format!("RSS probe returned malformed JSON: {line}"))?;
    let value = payload
        .strip_prefix("\"peak_memory_bytes\":")
        .and_then(|value| value.split([',', '}']).next())
        .ok_or_else(|| format!("RSS probe returned malformed JSON: {line}"))?;
    if workload == Workload::SettlementStateProof {
        let profile = format!(
            "\"settlement_surface_profile\":\"{}\"",
            settlement_surface.profile()
        );
        let version = format!(
            "\"settlement_proof_version\":\"{}\"",
            settlement_surface.proof_version()
        );
        if !line.contains(&profile) || !line.contains(&version) {
            return Err(format!(
                "RSS probe provenance does not match {}: {line}",
                settlement_surface.profile()
            ));
        }
    }
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
        let left = generate(workload, false, test_scale, SettlementSurface::DirectV1)?;
        let right = generate(workload, false, test_scale, SettlementSurface::DirectV1)?;
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
        if workload == Workload::SettlementStateProof {
            checks += settlement_state_proof_self_test(&left, test_scale)?;
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
    let worker_probe = generate(
        Workload::HighFrequencyLots,
        false,
        test_scale,
        SettlementSurface::DirectV1,
    )?;
    let (_, workers_equivalent) =
        median_independent_worker_comparison(Workload::HighFrequencyLots, &worker_probe.source, 1)?;
    if !workers_equivalent {
        return Err("concurrent independent worker differs from the serial result".into());
    }
    checks += 1;
    let one = generate(
        Workload::OneRowCloseChange,
        false,
        test_scale,
        SettlementSurface::DirectV1,
    )?;
    if one.changed_source == Some(one.source.clone()) {
        return Err("one-row close change did not change source".into());
    }
    checks += 1;
    if checks < 7 {
        return Err("self-test did not execute its minimum checks".into());
    }
    Ok(checks)
}

fn generic_form_self_test(_generated: &GeneratedWorkload, scale: u64) -> Result<usize, String> {
    let profile = bounded_profile(
        Workload::GenericFormElaboration,
        false,
        scale,
        SettlementSurface::DirectV1,
    )?;
    let measured = measure_generic_batches(profile)?;
    if measured.forms != profile.total_items
        || measured.changed_forms != profile.total_items
        || measured.authority_binding_checks != profile.batch_count()
        || measured.first_value_hash == measured.changed_first_value_hash
    {
        return Err("generic bounded self-test did not validate every authoritative batch".into());
    }
    Ok(4)
}

fn settlement_state_proof_self_test(
    generated: &GeneratedWorkload,
    scale: u64,
) -> Result<usize, String> {
    let profile = bounded_profile(
        Workload::SettlementStateProof,
        false,
        scale,
        SettlementSurface::DirectV1,
    )?;
    let execution = execute_bounded_settlement(profile, true)?;
    if execution.forms != profile.total_items || execution.oracle.coverage_count != execution.forms
    {
        return Err("settlement self-test coverage count mismatch".into());
    }
    let independent = execute_bounded_settlement(profile, false)?;
    if execution.oracle != independent.oracle {
        return Err("settlement self-test fresh-workspace oracle mismatch".into());
    }
    let changed = generated
        .changed_source
        .as_deref()
        .ok_or_else(|| "settlement self-test has no correction source".to_string())?;
    if !validate_settlement_source_revision(&execution.first, changed)?
        || !validate_settlement_atomic_negative(
            &generated.source,
            generated.source.replacen("amount 100", "amount -1", 1),
            SettlementSurface::DirectV1,
        )?
    {
        return Err("settlement self-test correction/atomic negative failed".into());
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
        "\"generation_ns\":{},\"normalization_ns\":{},\"parse_ns\":{},\"semantic_probe_ns\":{},\"solve_cold_ns\":{},\"independent_clean_solve_ns\":{},\"workspace_replay_ns\":{},\"proof_check_ns\":{},\"explanation_ns\":{},\"changed_incremental_solve_ns\":{},\"changed_full_solve_ns\":{},\"parallel_solve_ns\":null,\"independent_workers_ns\":{},\"package_compile_ns\":{},\"document_elaboration_ns\":{},\"schema_lookup_ns\":{},\"canonical_values_ns\":{},\"changed_document_elaboration_ns\":{},\"settlement_setup_ns\":{},\"settlement_projection_call_ns\":{},\"settlement_persistence_boundary_ns\":{},\"settlement_proof_check_ns\":{},\"settlement_store_verify_ns\":{},\"settlement_source_revision_ns\":{}",
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
        option_number(record.timing.settlement_setup_ns),
        option_number(record.timing.settlement_projection_call_ns),
        option_number(record.timing.settlement_persistence_boundary_ns),
        option_number(record.timing.settlement_proof_check_ns),
        option_number(record.timing.settlement_store_verify_ns),
        option_number(record.timing.settlement_source_revision_ns),
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
        "\"cache_hits\":{},\"cache_misses\":{},\"invalidated_queries\":{},\"proof_nodes\":{},\"proof_roots\":{},\"semantic_dependency_edges\":{},\"semantic_invalidation_edges\":{},\"explanation_bytes\":{},\"cycle_errors\":{},\"semantic_probe_items\":{},\"semantic_probe_results\":{},\"semantic_probe_api\":{},\"determinism_across_thread_counts\":null,\"same_process_cache_replay_equal\":{},\"independent_clean_recompute_equal\":{},\"peak_memory_bytes\":{},\"thread_count_equivalence\":null,\"parallel_thread_count\":null,\"independent_worker_determinism\":{},\"independent_worker_equivalence\":{},\"concurrent_worker_count\":{},\"canonical_value_count\":{},\"schema_lookup_count\":{},\"document_result_count\":{},\"revision_result_count\":{},\"revision_mode\":{},\"authority_binding_verified\":{},\"execution_mode\":{},\"provenance_path\":{},\"batch_count\":{},\"batch_size\":{},\"max_batch_forms\":{},\"authority_binding_checks\":{},\"build_profile\":{},\"resource_profile\":{},\"note\":{},\"unsupported_reason\":{},\"settlement_coverage_count\":{},\"settlement_coverage_hash\":{},\"settlement_source_commit_hash\":{},\"settlement_artifact_id_hash\":{},\"settlement_artifact_hash\":{},\"settlement_proof_hash\":{},\"settlement_proof_bytes\":{},\"settlement_projection_commit_hash\":{},\"settlement_binding_verified\":{},\"settlement_proof_check_verified\":{},\"settlement_store_verify_verified\":{},\"settlement_source_revision_verified\":{},\"settlement_atomic_negative_verified\":{},\"settlement_batch_source_commits_hash\":{},\"settlement_batch_artifact_ids_hash\":{},\"settlement_batch_artifacts_hash\":{},\"settlement_batch_proofs_hash\":{},\"settlement_batch_proof_bytes_total\":{},\"settlement_batch_projection_commits_hash\":{},\"settlement_batch_coverage_hash\":{}",
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
        option_string(record.metrics.execution_mode),
        option_string(record.metrics.provenance_path),
        option_number(record.metrics.batch_count.map(|value| value as u128)),
        option_number(record.metrics.batch_size.map(|value| value as u128)),
        option_number(record.metrics.max_batch_forms.map(|value| value as u128)),
        option_number(
            record
                .metrics
                .authority_binding_checks
                .map(|value| value as u128),
        ),
        option_string(record.metrics.build_profile),
        option_string(record.metrics.resource_profile),
        option_string(record.metrics.note),
        option_string(record.unsupported_reason),
        option_number(
            record
                .metrics
                .settlement_coverage_count
                .map(|value| value as u128)
        ),
        option_string(record.metrics.settlement_coverage_hash.as_deref()),
        option_string(record.metrics.settlement_source_commit_hash.as_deref()),
        option_string(record.metrics.settlement_artifact_id_hash.as_deref()),
        option_string(record.metrics.settlement_artifact_hash.as_deref()),
        option_string(record.metrics.settlement_proof_hash.as_deref()),
        option_number(
            record
                .metrics
                .settlement_proof_bytes
                .map(|value| value as u128)
        ),
        option_string(record.metrics.settlement_projection_commit_hash.as_deref()),
        option_bool(record.metrics.settlement_binding_verified),
        option_bool(record.metrics.settlement_proof_check_verified),
        option_bool(record.metrics.settlement_store_verify_verified),
        option_bool(record.metrics.settlement_source_revision_verified),
        option_bool(record.metrics.settlement_atomic_negative_verified),
        option_string(
            record
                .metrics
                .settlement_batch_source_commits_hash
                .as_deref(),
        ),
        option_string(record.metrics.settlement_batch_artifact_ids_hash.as_deref(),),
        option_string(record.metrics.settlement_batch_artifacts_hash.as_deref(),),
        option_string(record.metrics.settlement_batch_proofs_hash.as_deref()),
        option_number(
            record
                .metrics
                .settlement_batch_proof_bytes_total
                .map(|value| value as u128),
        ),
        option_string(
            record
                .metrics
                .settlement_batch_projection_commits_hash
                .as_deref(),
        ),
        option_string(record.metrics.settlement_batch_coverage_hash.as_deref()),
    );
    let _ = write!(
        output,
        ",\"settlement_surface_profile\":{},\"settlement_proof_version\":{}",
        option_string(record.metrics.settlement_surface_profile),
        option_string(record.metrics.settlement_proof_version),
    );
    output.push_str("}}");
    println!("{output}");
}

fn print_human_summary(records: &[ResultRecord]) {
    eprintln!("Axiom benchmark (measurements; targets are engineering goals, not assertions)");
    eprintln!(
        "workload                         forms    cold ms  replay ms  proof  settle ms  proof B  cache  invalid  target"
    );
    eprintln!(
        "-------------------------------- -------- --------- --------- ------ --------- -------- ------ -------- ------------------------"
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
        let settlement = record
            .timing
            .settlement_persistence_boundary_ns
            .map_or_else(|| "N/A".to_string(), format_ms);
        let proof_bytes = record
            .metrics
            .settlement_proof_bytes
            .map_or_else(|| "N/A".to_string(), |value| value.to_string());
        let cache = record
            .metrics
            .cache_hits
            .map_or_else(|| "null".to_string(), |value| value.to_string());
        let invalid = record
            .metrics
            .invalidated_queries
            .map_or_else(|| "null".to_string(), |value| value.to_string());
        eprintln!(
            "{:<32} {:>8} {:>9} {:>9} {:>6} {:>9} {:>8} {:>6} {:>8} {}",
            record.workload.name(),
            forms,
            cold,
            replay,
            proof,
            settlement,
            proof_bytes,
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

fn generate(
    workload: Workload,
    quick: bool,
    scale: u64,
    settlement_surface: SettlementSurface,
) -> Result<GeneratedWorkload, String> {
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
        // Quick mode deliberately uses one row so --scale maps directly to
        // the dedicated 4,096-row proof boundary.
        Workload::SettlementStateProof => {
            settlement_state_proof(base(64, 1)?, settlement_surface.is_compact())
        }
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
    let batch_count = count.min(BOUNDED_BATCH_SIZE);
    let source = generic_form_batch(0, batch_count, false)?;
    let changed = generic_form_batch(0, batch_count, true)?;
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

fn generic_form_batch(start: usize, count: usize, changed: bool) -> Result<String, String> {
    let mut source = String::with_capacity(count.saturating_mul(150));
    for index in start..start.saturating_add(count) {
        let note = if changed && index == 0 {
            "row-000000-edited".to_owned()
        } else {
            format!("row-{index:06}")
        };
        let _ = writeln!(
            source,
            "form generic/{index:06} : forms::types::Row\n  approved {}\n  count {index}\n  note \"{note}\"\n  total {}\n",
            index % 2 == 0,
            index * 3 + 7,
        );
    }
    Ok(source)
}

fn settlement_state_proof(count: usize, compact: bool) -> Result<GeneratedWorkload, String> {
    let batch_count = count.min(BOUNDED_BATCH_SIZE);
    let source = settlement_state_batch(0, batch_count, false, compact)?;
    let changed = settlement_state_batch(0, batch_count, true, compact)?;
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

fn settlement_state_batch(
    start: usize,
    count: usize,
    changed: bool,
    compact: bool,
) -> Result<String, String> {
    let mut source = String::with_capacity(count.saturating_mul(220));
    let schema = if compact {
        "payments::forms::CompactSettlement"
    } else {
        "payments::types::SettlementState"
    };
    for index in start..start.saturating_add(count) {
        let amount = if changed && index == start {
            "100.5".to_owned()
        } else {
            (100 + index).to_string()
        };
        if compact {
            let _ = writeln!(
                source,
                "form settlement/{index:06} : {schema}\n  s payment/{index:06}\n  k ach\n  x issued\n  d 2026-01-01\n  f customer/{index:06}\n  t merchant/{index:06}\n  i USD\n  a {amount}\n",
            );
        } else {
            let _ = writeln!(
                source,
                "form settlement/{index:06} : {schema}\n  settlement payment/{index:06}\n  kind ach\n  state issued\n  at 2026-01-01\n  from customer/{index:06}\n  to merchant/{index:06}\n  instrument USD\n  amount {amount}\n",
            );
        }
    }
    Ok(source)
}
