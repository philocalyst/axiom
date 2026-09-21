//! Reproducible benchmark corpus for section XVII of confirmed-direction.md.
//!
//! The binary intentionally has no benchmark framework dependency. It emits
//! one JSON object per line on stdout and a compact human table on stderr.
//! Timings are observations from this process; engineering goals are emitted
//! separately and are never used as pass/fail performance assertions.

use std::env;
use std::fmt::Write as _;
use std::hint::black_box;
use std::time::Instant;

use axiom_ledger::incremental::{IncrementalDb, MemoOutcome, QueryError, QueryKey};
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
}

impl Workload {
    const ALL: [Self; 11] = [
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
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::TenYearPersonalHistory => "ten years of monthly personal activity",
            Self::HighFrequencyLots => "many acquisitions competing for one sale",
            Self::MultiCurrency => "business activity across several currencies",
            Self::CorporateActions => "portfolio activity with split/dividend annotations",
            Self::InvoicePaymentGraph => "invoice and payment evidence graph",
            Self::OwnershipNetwork => "recursive ownership evidence network",
            Self::ConflictingImports => "contradictory imported observations",
            Self::OneRowCloseChange => "single evidence row changed near close",
            Self::PackageUpgrade => "policy package input changed between revisions",
            Self::AdversarialRecursion => "recursive rule shape with an explicit cycle probe",
            Self::LargeProofExplanation => "large proof DAG and source explanation",
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
    solve_cold_ns: Option<u128>,
    independent_clean_solve_ns: Option<u128>,
    workspace_replay_ns: Option<u128>,
    proof_check_ns: Option<u128>,
    explanation_ns: Option<u128>,
    changed_full_solve_ns: Option<u128>,
}

#[derive(Clone, Debug, Default)]
struct Sizes {
    source_bytes: usize,
    source_lines: usize,
    forms: Option<usize>,
    changed_source_bytes: Option<usize>,
    semantic_relation_nodes: Option<usize>,
    semantic_relation_edges: Option<usize>,
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
    determinism_across_thread_counts: Option<bool>,
    same_process_cache_replay_equal: Option<bool>,
    independent_clean_recompute_equal: Option<bool>,
    peak_memory_bytes: Option<usize>,
    thread_count_equivalence: Option<bool>,
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
        let record = measure_workload(workload, options.quick, options.scale, samples)?;
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
        };
        let args: Vec<String> = args.collect();
        let mut index = 0;
        while index < args.len() {
            match args[index].as_str() {
                "--quick" => options.quick = true,
                "--self-test" => options.self_test = true,
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
--quick          one timing sample and reduced row counts\n\
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
) -> Result<ResultRecord, String> {
    let generated = generate(workload, quick, scale)?;
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
        semantic_relation_nodes: None,
        semantic_relation_edges: None,
    };
    let mut metrics = Metrics {
        determinism_across_thread_counts: None,
        same_process_cache_replay_equal: None,
        peak_memory_bytes: None,
        thread_count_equivalence: None,
        build_profile: Some(if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }),
        resource_profile: Some(if process_peak_memory_bytes().is_some() {
            "single-threaded; process peak RSS via getrusage"
        } else {
            "single-threaded; process peak RSS unavailable"
        }),
        note: Some(if process_peak_memory_bytes().is_some() {
            "peak_memory_bytes is process-lifetime peak RSS; parallel/thread-count metrics remain null because no parallel engine path exists"
        } else {
            "peak RSS is unavailable on this platform; parallel/thread-count metrics remain null because no parallel engine path exists"
        }),
        ..Metrics::default()
    };

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
    timing.proof_check_ns = Some(median_proof_check(&analysis, samples));
    metrics.proof_nodes = Some(analysis.proof.nodes.len());
    metrics.proof_roots = Some(analysis.proof.roots.len());
    metrics.semantic_dependency_edges =
        Some(analysis.dependencies.values().map(Vec::len).sum::<usize>());
    metrics.semantic_invalidation_edges =
        Some(analysis.invalidations.values().map(Vec::len).sum::<usize>());
    sizes.semantic_relation_nodes =
        Some(analysis.dependencies.len() + analysis.invalidations.len());
    sizes.semantic_relation_edges = Some(
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
    metrics.peak_memory_bytes = process_peak_memory_bytes();

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

fn self_test(quick: bool, scale: u64) -> Result<usize, String> {
    let test_scale = scale.min(2);
    let mut checks = 0usize;
    for workload in Workload::ALL {
        let left = generate(workload, true, test_scale)?;
        let right = generate(workload, true, test_scale)?;
        if left.source != right.source || left.changed_source != right.changed_source {
            return Err(format!(
                "{}: generator is not deterministic",
                workload.name()
            ));
        }
        checks += 1;
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
    if !quick {
        let one = generate(Workload::OneRowCloseChange, false, test_scale)?;
        if one.changed_source == Some(one.source.clone()) {
            return Err("one-row close change did not change source".into());
        }
        checks += 1;
    }
    if checks < 7 {
        return Err("self-test did not execute its minimum checks".into());
    }
    Ok(checks)
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
        "\"generation_ns\":{},\"normalization_ns\":{},\"parse_ns\":{},\"solve_cold_ns\":{},\"independent_clean_solve_ns\":{},\"workspace_replay_ns\":{},\"proof_check_ns\":{},\"explanation_ns\":{},\"changed_full_solve_ns\":{}",
        record.timing.generation_ns,
        option_number(record.timing.normalization_ns),
        option_number(record.timing.parse_ns),
        option_number(record.timing.solve_cold_ns),
        option_number(record.timing.independent_clean_solve_ns),
        option_number(record.timing.workspace_replay_ns),
        option_number(record.timing.proof_check_ns),
        option_number(record.timing.explanation_ns),
        option_number(record.timing.changed_full_solve_ns),
    );
    output.push_str("},\"sizes\":{");
    let _ = write!(
        output,
        "\"source_bytes\":{},\"source_lines\":{},\"forms\":{},\"changed_source_bytes\":{},\"semantic_relation_nodes\":{},\"semantic_relation_edges\":{}",
        record.sizes.source_bytes,
        record.sizes.source_lines,
        option_number(record.sizes.forms.map(|value| value as u128)),
        option_number(record.sizes.changed_source_bytes.map(|value| value as u128)),
        option_number(
            record
                .sizes
                .semantic_relation_nodes
                .map(|value| value as u128)
        ),
        option_number(
            record
                .sizes
                .semantic_relation_edges
                .map(|value| value as u128)
        ),
    );
    output.push_str("},\"metrics\":{");
    let _ = write!(
        output,
        "\"cache_hits\":{},\"cache_misses\":{},\"invalidated_queries\":{},\"proof_nodes\":{},\"proof_roots\":{},\"semantic_dependency_edges\":{},\"semantic_invalidation_edges\":{},\"explanation_bytes\":{},\"cycle_errors\":{},\"determinism_across_thread_counts\":{},\"same_process_cache_replay_equal\":{},\"independent_clean_recompute_equal\":{},\"peak_memory_bytes\":{},\"thread_count_equivalence\":{},\"build_profile\":{},\"resource_profile\":{},\"note\":{},\"unsupported_reason\":{}",
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
        option_bool(record.metrics.determinism_across_thread_counts),
        option_bool(record.metrics.same_process_cache_replay_equal),
        option_bool(record.metrics.independent_clean_recompute_equal),
        option_number(record.metrics.peak_memory_bytes.map(|value| value as u128)),
        option_bool(record.metrics.thread_count_equivalence),
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
        "full corpus sizes are deterministic row counts (quick uses 1/10 scale); --scale multiplies them"
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
    })
}

fn multi_currency(count: usize) -> Result<GeneratedWorkload, String> {
    let mut source = header(
        "business-multi",
        &["shape: USD, EUR, GBP, and BTC positions with dated quotes"],
    );
    let assets = ["SERV", "MACH", "DATA"];
    let currencies = ["USD", "EUR", "GBP"];
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
        explain_goal: Some("gain:fx/sale/close".into()),
        semantic_supported: false,
        unsupported_reason: Some(
            "V0 does not value a complete multi-currency book; quote observations remain independent",
        ),
    })
}

fn corporate_actions(count: usize) -> Result<GeneratedWorkload, String> {
    let mut source = header(
        "portfolio-actions",
        &[
            "shape: split, dividend, merger, and spin-off rows are immutable evidence annotations",
            "V0 representation: ordinary lots plus labelled observation rows",
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
        explain_goal: Some("gain:actions/sale/close".into()),
        semantic_supported: false,
        unsupported_reason: Some(
            "corporate-action annotations are evidence comments until an action-aware ontology is available",
        ),
    })
}

fn invoice_payment_graph(count: usize) -> Result<GeneratedWorkload, String> {
    let mut source = header(
        "invoice-payments",
        &[
            "shape: invoice -> authorization -> settlement -> allocation graph",
            "V0 representation: stable references on settlement observations",
        ],
    );
    for index in 0..count {
        let _ = writeln!(
            source,
            "; invoice inv/{index:06} customer customer/{:04} due 2028-12-31",
            index % 97
        );
        let _ = writeln!(
            source,
            "observe settlement payment/{index:06} {} USD into receivables",
            100 + index % 37
        );
        if index % 5 == 0 {
            let _ = writeln!(
                source,
                "observe position customer/{:04} {} USD",
                index % 97,
                100 + index % 37
            );
        }
    }
    Ok(GeneratedWorkload {
        source,
        changed_source: None,
        changed_kind: None,
        explain_goal: None,
        semantic_supported: false,
        unsupported_reason: Some(
            "invoice allocation edges are labelled evidence, not a V0 graph relation",
        ),
    })
}

fn ownership_network(count: usize) -> Result<GeneratedWorkload, String> {
    let mut source = header(
        "ownership-network",
        &[
            "shape: ownership edges and beneficial-owner declarations",
            "V0 representation: deterministic position observations with edge labels",
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
            1 + (index % 23)
        );
    }
    Ok(GeneratedWorkload {
        source,
        changed_source: None,
        changed_kind: None,
        explain_goal: None,
        semantic_supported: false,
        unsupported_reason: Some(
            "ownership closure is not a V0 relation; observations are measured without claiming closure",
        ),
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
    let mut changed = source.clone();
    changed.push_str("observe position checking 151 USD\n");
    Ok(GeneratedWorkload {
        source,
        changed_source: Some(changed),
        changed_kind: Some(ChangedKind::EvidenceRow),
        explain_goal: Some("gain:close/sale".into()),
        semantic_supported: true,
        unsupported_reason: None,
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
    })
}

fn adversarial_recursion(count: usize) -> Result<GeneratedWorkload, String> {
    let mut source = header(
        "recursive-rules",
        &[
            "shape: p(X) :- p(X), plus mutually recursive aliases",
            "the V0 parser has no rule syntax; the cycle is probed through IncrementalDb",
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
        semantic_supported: false,
        unsupported_reason: Some(
            "recursive rule solving is not exposed by the V0 source parser; only the explicit cycle probe is measured",
        ),
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
    })
}
