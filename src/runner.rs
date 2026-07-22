//! Reusable orchestration for validated configurations and durable artifacts.
//!
//! The Monte Carlo engine is the published `qslib-quantum` SSE backend; this
//! module owns everything the library deliberately does not: per-chain
//! artifact files, resumable scheduling, wall-clock timing, and aggregate
//! statistical reporting.

use std::error::Error;
use std::fmt;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use qslib::sse::{
    derive_chain_seed, BasisSseState, ClusterSweepStats, DiagonalSweepStats, LocalSseModel,
    Operator, SseModel, SseSampler, UpdateScheme,
};
use qslib::variational::{autocorrelation, r_hat};
use rand_chacha::ChaCha20Rng;
use rand_core::SeedableRng;
use rayon::prelude::*;

use crate::artifacts::{
    read_json, write_json_atomic, write_text_atomic, ArtifactError, ChainArtifact,
    ChainDiagnostics, ChainSummary, CheckpointIndex, RunManifest, RunStatus, RunSummary,
    ThermodynamicArtifact, TimingArtifact, UpdateStatistics, ARTIFACT_SCHEMA_VERSION,
};
use crate::config::{ConfigError, ModelConfig, RunConfig, RydbergUpdate};
use crate::model::initial_bits;

/// Output-directory behavior requested by a caller.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RunMode {
    /// Require a path that does not already exist.
    #[default]
    Fresh,
    /// Reuse completed chain artifacts and execute only missing chains.
    Resume,
    /// Replace an existing, explicitly named output directory.
    Force,
}

/// Successful result of [`run_to_directory`].
#[derive(Clone, Debug)]
pub struct RunOutcome {
    /// Output directory containing all artifacts.
    pub directory: PathBuf,
    /// Completed aggregate summary.
    pub summary: RunSummary,
    /// Number of chains reused from an earlier interrupted run.
    pub reused_chains: usize,
}

/// Executes a validated run and writes its versioned artifact set.
///
/// Completed chains are written atomically and form the restart boundary. On
/// resume, completed chains are reused while any interrupted chain restarts
/// from its deterministic seed.
///
/// `input_yaml` should contain the exact user input when available. The runner
/// always writes a canonical `config.resolved.yaml` independently.
///
/// # Errors
///
/// Returns a validation, output-policy, execution, or artifact failure. Once a
/// manifest exists, execution failures are also recorded there.
pub fn run_to_directory(
    config: &RunConfig,
    input_yaml: Option<&str>,
    directory: impl AsRef<Path>,
    mode: RunMode,
) -> Result<RunOutcome, RunnerError> {
    config.validate()?;
    let directory = directory.as_ref();
    prepare_directory(directory, mode)?;

    let resolved_yaml = config.to_yaml_string()?;
    if mode == RunMode::Resume {
        let stored =
            fs::read_to_string(directory.join("config.resolved.yaml")).map_err(|source| {
                RunnerError::Io {
                    path: directory.join("config.resolved.yaml"),
                    source,
                }
            })?;
        let stored_config = RunConfig::from_yaml_str(&stored)?;
        if &stored_config != config {
            return Err(RunnerError::ResumeConfigurationMismatch);
        }
    } else {
        if let Some(input) = input_yaml {
            write_text_atomic(&directory.join("config.input.yaml"), input)?;
        }
        write_text_atomic(&directory.join("config.resolved.yaml"), &resolved_yaml)?;
    }

    fs::create_dir_all(directory.join("chains")).map_err(|source| RunnerError::Io {
        path: directory.join("chains"),
        source,
    })?;

    let mut manifest = if mode == RunMode::Resume && directory.join("manifest.json").exists() {
        read_json::<RunManifest>(&directory.join("manifest.json"))?
    } else {
        new_manifest(config)
    };
    manifest.status = RunStatus::Running;
    manifest.completed_unix_seconds = None;
    manifest.error = None;
    manifest.attempts = manifest.attempts.saturating_add(1);
    manifest.completed_chains = count_completed_chain_files(directory, config.execution.chains);
    write_json_atomic(&directory.join("manifest.json"), &manifest)?;

    match run_inner(config, directory) {
        Ok((summary, reused_chains)) => {
            manifest.status = RunStatus::Complete;
            manifest.completed_unix_seconds = Some(unix_seconds());
            manifest.completed_chains = config.execution.chains;
            manifest.files = vec![
                "config.resolved.yaml".to_string(),
                "checkpoint.json".to_string(),
                "measurements.csv".to_string(),
                "summary.json".to_string(),
            ];
            write_json_atomic(&directory.join("manifest.json"), &manifest)?;
            Ok(RunOutcome {
                directory: directory.to_path_buf(),
                summary,
                reused_chains,
            })
        }
        Err(error) => {
            manifest.status = RunStatus::Failed;
            manifest.completed_chains =
                count_completed_chain_files(directory, config.execution.chains);
            manifest.error = Some(error.to_string());
            let _ = write_json_atomic(&directory.join("manifest.json"), &manifest);
            Err(error)
        }
    }
}

fn run_inner(config: &RunConfig, directory: &Path) -> Result<(RunSummary, usize), RunnerError> {
    let geometry = config.model.geometry().build()?;
    let model = Arc::new(config.model.build_model(&geometry)?);
    let spins = config.initial_state.build(model.num_sites())?;
    let initial_state = Arc::new(initial_bits(config.model.legacy_kind(), &spins));

    let mut chains = Vec::with_capacity(config.execution.chains);
    let mut pending = Vec::new();
    for chain_index in 0..config.execution.chains {
        let path = chain_path(directory, chain_index);
        if path.exists() {
            let chain: ChainArtifact = read_json(&path)?;
            validate_chain_artifact(config, chain_index, &chain)?;
            chains.push(chain);
        } else {
            pending.push(chain_index);
        }
    }
    let reused_chains = chains.len();

    if !pending.is_empty() {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(config.execution.threads)
            .build()
            .map_err(|error| RunnerError::ThreadPool(error.to_string()))?;
        let new_chains = pool.install(|| {
            pending
                .into_par_iter()
                .map(|chain_index| {
                    let chain = run_chain(
                        config,
                        Arc::clone(&model),
                        initial_state.as_slice(),
                        chain_index,
                    )?;
                    write_json_atomic(&chain_path(directory, chain_index), &chain)?;
                    Ok(chain)
                })
                .collect::<Result<Vec<_>, RunnerError>>()
        })?;
        chains.extend(new_chains);
    }
    chains.sort_by_key(|chain| chain.chain_index);

    let summary = summarize(config, model.num_sites(), &chains);
    write_measurements_csv(directory, &chains)?;
    write_json_atomic(&directory.join("summary.json"), &summary)?;
    write_json_atomic(
        &directory.join("checkpoint.json"),
        &CheckpointIndex {
            artifact_schema_version: ARTIFACT_SCHEMA_VERSION.to_string(),
            completed_chains: chains.iter().map(|chain| chain.chain_index).collect(),
        },
    )?;
    Ok((summary, reused_chains))
}

fn update_scheme(model: &ModelConfig) -> UpdateScheme {
    match model {
        ModelConfig::Tfim { .. } => UpdateScheme::TfimCluster,
        ModelConfig::Rydberg { update, .. } => match update {
            RydbergUpdate::Local => UpdateScheme::RydbergLocal,
            RydbergUpdate::GlobalReference => UpdateScheme::RydbergGlobalReference,
        },
    }
}

fn run_chain(
    config: &RunConfig,
    model: Arc<LocalSseModel>,
    initial_state: &[qslib::BasisBit],
    chain_index: usize,
) -> Result<ChainArtifact, RunnerError> {
    let chain_error = |message: String| RunnerError::Chain {
        chain_index,
        message,
    };
    let seed = derive_chain_seed(config.execution.seed, chain_index as u64);
    let state = BasisSseState::new(
        initial_state.to_vec(),
        vec![Operator::identity(); config.simulation.operator_string_length],
    )
    .map_err(|error| chain_error(error.to_string()))?;
    let mut sampler = SseSampler::new(
        (*model).clone(),
        state,
        config.simulation.beta,
        ChaCha20Rng::from_seed(seed),
    )
    .map_err(|error| chain_error(error.to_string()))?;
    let scheme = update_scheme(&config.model);

    let run_started = Instant::now();
    let mut diagonal_total = DiagonalSweepStats::default();
    let mut cluster_total = ClusterSweepStats::default();
    let mut diagonal_seconds = 0.0;
    let mut cluster_seconds = 0.0;
    let mut accumulation_seconds = 0.0;

    let sweep = |sampler: &mut SseSampler<LocalSseModel, ChaCha20Rng>,
                 diagonal_total: &mut DiagonalSweepStats,
                 cluster_total: &mut ClusterSweepStats,
                 diagonal_seconds: &mut f64,
                 cluster_seconds: &mut f64|
     -> Result<(), RunnerError> {
        sampler.ensure_operator_headroom();
        let started = Instant::now();
        let diagonal = sampler
            .diagonal_sweep()
            .map_err(|error| chain_error(error.to_string()))?;
        *diagonal_seconds += started.elapsed().as_secs_f64();
        let started = Instant::now();
        let clusters = match scheme {
            UpdateScheme::TfimCluster => sampler.tfim_cluster_sweep(),
            UpdateScheme::RydbergLocal => sampler.rydberg_local_sweep(),
            UpdateScheme::RydbergGlobalReference => sampler.rydberg_global_cluster_sweep(),
            UpdateScheme::Local => unreachable!("runner never selects the plain local scheme"),
        }
        .map_err(|error| chain_error(error.to_string()))?;
        *cluster_seconds += started.elapsed().as_secs_f64();
        add_diagonal(diagonal_total, diagonal);
        add_clusters(cluster_total, clusters);
        Ok(())
    };

    let thermalization_started = Instant::now();
    for _ in 0..config.simulation.thermalization_sweeps {
        sweep(
            &mut sampler,
            &mut diagonal_total,
            &mut cluster_total,
            &mut diagonal_seconds,
            &mut cluster_seconds,
        )?;
    }
    let thermalization_seconds = thermalization_started.elapsed().as_secs_f64();

    let mut accumulator = qslib::sse::ThermodynamicAccumulator::default();
    let mut expansion_orders = Vec::with_capacity(config.simulation.measurement_sweeps);
    let measurement_started = Instant::now();
    for _ in 0..config.simulation.measurement_sweeps {
        for _ in 0..config.simulation.sweeps_per_measurement {
            sweep(
                &mut sampler,
                &mut diagonal_total,
                &mut cluster_total,
                &mut diagonal_seconds,
                &mut cluster_seconds,
            )?;
        }
        let started = Instant::now();
        let order = sampler.state().expansion_order();
        accumulator.record(order);
        expansion_orders.push(order);
        accumulation_seconds += started.elapsed().as_secs_f64();
    }
    let measurement_seconds = measurement_started.elapsed().as_secs_f64();

    let thermodynamics = accumulator
        .results(
            config.simulation.beta,
            sampler.model().energy_shift(),
            sampler.model().num_sites(),
        )
        .ok_or_else(|| chain_error("no measurements were recorded".to_string()))?;
    let diagnostics = diagnose_chain(
        &expansion_orders,
        config.simulation.beta,
        sampler.model().num_sites(),
    );

    Ok(ChainArtifact {
        artifact_schema_version: ARTIFACT_SCHEMA_VERSION.to_string(),
        chain_index,
        seed: seed_hex(&seed),
        thermodynamics: ThermodynamicArtifact {
            samples: thermodynamics.samples,
            mean_expansion_order: thermodynamics.mean_expansion_order,
            energy: thermodynamics.energy,
            energy_per_site: thermodynamics.energy_per_site,
            heat_capacity: thermodynamics.heat_capacity,
            heat_capacity_per_site: thermodynamics.heat_capacity_per_site,
        },
        updates: UpdateStatistics {
            insertions_proposed: diagonal_total.insertions_proposed,
            insertions_accepted: diagonal_total.insertions_accepted,
            removals_proposed: diagonal_total.removals_proposed,
            removals_accepted: diagonal_total.removals_accepted,
            clusters: cluster_total.clusters,
            flipped_clusters: cluster_total.flipped_clusters,
            vertices_toggled: cluster_total.vertices_toggled,
            proposals: cluster_total.proposals,
            proposals_accepted: cluster_total.proposals_accepted,
        },
        timing: TimingArtifact {
            total_seconds: run_started.elapsed().as_secs_f64(),
            thermalization_seconds,
            measurement_seconds,
            diagonal_update_seconds: diagonal_seconds,
            off_diagonal_update_seconds: cluster_seconds,
            accumulation_seconds,
        },
        diagnostics,
        expansion_orders,
    })
}

fn add_diagonal(total: &mut DiagonalSweepStats, sweep: DiagonalSweepStats) {
    total.insertions_proposed += sweep.insertions_proposed;
    total.insertions_accepted += sweep.insertions_accepted;
    total.removals_proposed += sweep.removals_proposed;
    total.removals_accepted += sweep.removals_accepted;
}

fn add_clusters(total: &mut ClusterSweepStats, sweep: ClusterSweepStats) {
    total.clusters += sweep.clusters;
    total.flipped_clusters += sweep.flipped_clusters;
    total.vertices_toggled += sweep.vertices_toggled;
    total.proposals += sweep.proposals;
    total.proposals_accepted += sweep.proposals_accepted;
}

fn seed_hex(seed: &[u8; 32]) -> String {
    let mut text = String::with_capacity(64);
    for byte in seed {
        write!(text, "{byte:02x}").expect("writing to a String cannot fail");
    }
    text
}

fn validate_chain_artifact(
    config: &RunConfig,
    expected_index: usize,
    chain: &ChainArtifact,
) -> Result<(), RunnerError> {
    let expected_seed = seed_hex(&derive_chain_seed(
        config.execution.seed,
        expected_index as u64,
    ));
    if chain.artifact_schema_version != ARTIFACT_SCHEMA_VERSION
        || chain.chain_index != expected_index
        || chain.seed != expected_seed
        || chain.expansion_orders.len() != config.simulation.measurement_sweeps
    {
        return Err(RunnerError::InvalidCheckpoint {
            chain_index: expected_index,
        });
    }
    Ok(())
}

/// Computes serial-correlation diagnostics with the qslib Geyer estimator.
///
/// The integrated autocorrelation time follows the qslib convention (floored
/// at one, effective sample size `N / tau`). Degenerate series fall back to
/// fully independent defaults.
fn diagnose_chain(orders: &[usize], beta: f64, num_sites: usize) -> ChainDiagnostics {
    let count = orders.len();
    let count_f = count as f64;
    let independent = |sample_variance: f64| ChainDiagnostics {
        expansion_order_variance: sample_variance,
        integrated_autocorrelation_time: 1.0,
        effective_sample_size: count_f,
        energy_standard_error_per_site: 0.0,
    };
    if count < 2 {
        return independent(0.0);
    }
    let mean = orders.iter().map(|&value| value as f64).sum::<f64>() / count_f;
    let squared_deviations = orders
        .iter()
        .map(|&value| {
            let difference = value as f64 - mean;
            difference * difference
        })
        .sum::<f64>();
    let sample_variance = squared_deviations / (count_f - 1.0);
    let population_variance = squared_deviations / count_f;
    if population_variance <= f64::EPSILON {
        return independent(sample_variance);
    }

    let series: Vec<f64> = orders.iter().map(|&value| value as f64).collect();
    let maximum_lag = (count / 2).clamp(1, 10_000);
    let Ok(estimate) = autocorrelation(&series, maximum_lag) else {
        return independent(sample_variance);
    };
    let effective = estimate.effective_sample_size().clamp(1.0, count_f);
    ChainDiagnostics {
        expansion_order_variance: sample_variance,
        integrated_autocorrelation_time: estimate.integrated_time(),
        effective_sample_size: effective,
        energy_standard_error_per_site: (population_variance / effective).sqrt()
            / (beta * num_sites as f64),
    }
}

fn summarize(config: &RunConfig, num_sites: usize, chains: &[ChainArtifact]) -> RunSummary {
    let count = chains.len() as f64;
    let energy_per_site = chains
        .iter()
        .map(|chain| chain.thermodynamics.energy_per_site)
        .sum::<f64>()
        / count;
    let between_variance = if chains.len() > 1 {
        chains
            .iter()
            .map(|chain| {
                let difference = chain.thermodynamics.energy_per_site - energy_per_site;
                difference * difference
            })
            .sum::<f64>()
            / (count - 1.0)
    } else {
        0.0
    };
    let split_r_hat = split_r_hat(chains);
    let minimum_effective_sample_size = chains
        .iter()
        .map(|chain| chain.diagnostics.effective_sample_size)
        .fold(f64::INFINITY, f64::min);
    let mut warnings = Vec::new();
    if chains.len() < 4 {
        warnings.push("fewer than four independent chains limit convergence assessment".into());
    }
    if config.simulation.thermalization_sweeps == 0 {
        warnings.push("no thermalization sweeps were discarded".into());
    }
    if minimum_effective_sample_size < 100.0 {
        warnings.push("at least one chain has estimated effective sample size below 100".into());
    }
    match split_r_hat {
        Some(value) if value > 1.01 => warnings.push(format!(
            "split R-hat for expansion order is {value:.4}, above the 1.01 diagnostic threshold"
        )),
        None => warnings.push("split R-hat is unavailable; use more chains or measurements".into()),
        Some(_) => {}
    }

    RunSummary {
        artifact_schema_version: ARTIFACT_SCHEMA_VERSION.to_string(),
        name: config.name.clone(),
        model: config.model.kind_name().to_string(),
        num_sites,
        chains: chains.len(),
        samples: chains
            .iter()
            .map(|chain| chain.thermodynamics.samples)
            .sum(),
        energy_per_site,
        chain_standard_error: (chains.len() > 1).then(|| (between_variance / count).sqrt()),
        split_r_hat,
        minimum_effective_sample_size,
        warnings,
        chain_results: chains
            .iter()
            .map(|chain| ChainSummary {
                chain_index: chain.chain_index,
                seed: chain.seed.clone(),
                energy_per_site: chain.thermodynamics.energy_per_site,
                heat_capacity_per_site: chain.thermodynamics.heat_capacity_per_site,
                effective_sample_size: chain.diagnostics.effective_sample_size,
                integrated_autocorrelation_time: chain.diagnostics.integrated_autocorrelation_time,
                wall_time_seconds: chain.timing.total_seconds,
            })
            .collect(),
    }
}

/// Split-chain potential scale reduction through the qslib classic estimator.
///
/// Each chain's expansion-order series is split into equal first and last
/// halves, so the classic equal-length R-hat over the split sequences equals
/// the split-chain diagnostic.
fn split_r_hat(chains: &[ChainArtifact]) -> Option<f64> {
    let half_length = chains
        .iter()
        .map(|chain| chain.expansion_orders.len() / 2)
        .min()?;
    if chains.len() < 2 || half_length < 2 {
        return None;
    }
    let sequences: Vec<Vec<f64>> = chains
        .iter()
        .flat_map(|chain| {
            let values = chain.expansion_orders.as_slice();
            [
                values[..half_length]
                    .iter()
                    .map(|&value| value as f64)
                    .collect::<Vec<_>>(),
                values[values.len() - half_length..]
                    .iter()
                    .map(|&value| value as f64)
                    .collect::<Vec<_>>(),
            ]
        })
        .collect();
    r_hat(&sequences).ok().map(|diagnostic| diagnostic.value())
}

fn write_measurements_csv(directory: &Path, chains: &[ChainArtifact]) -> Result<(), RunnerError> {
    let estimated_rows = chains
        .iter()
        .map(|chain| chain.expansion_orders.len())
        .sum::<usize>();
    let mut csv = String::with_capacity(48 + estimated_rows * 20);
    csv.push_str("chain_index,measurement_index,expansion_order\n");
    for chain in chains {
        for (measurement_index, expansion_order) in chain.expansion_orders.iter().enumerate() {
            writeln!(
                csv,
                "{},{measurement_index},{expansion_order}",
                chain.chain_index
            )
            .expect("writing to a String cannot fail");
        }
    }
    write_text_atomic(&directory.join("measurements.csv"), &csv)?;
    Ok(())
}

fn prepare_directory(directory: &Path, mode: RunMode) -> Result<(), RunnerError> {
    match mode {
        RunMode::Fresh if directory.exists() => {
            return Err(RunnerError::OutputExists(directory.to_path_buf()));
        }
        RunMode::Resume if !directory.is_dir() => {
            return Err(RunnerError::ResumeDirectoryMissing(directory.to_path_buf()));
        }
        RunMode::Force if directory.exists() => {
            reject_dangerous_force_target(directory)?;
            let mut entries = fs::read_dir(directory).map_err(|source| RunnerError::Io {
                path: directory.to_path_buf(),
                source,
            })?;
            if entries.next().is_some() {
                let manifest: RunManifest = read_json(&directory.join("manifest.json"))
                    .map_err(|_| RunnerError::UnrecognizedForceTarget(directory.to_path_buf()))?;
                if manifest.artifact_schema_version != ARTIFACT_SCHEMA_VERSION {
                    return Err(RunnerError::UnrecognizedForceTarget(
                        directory.to_path_buf(),
                    ));
                }
            }
            fs::remove_dir_all(directory).map_err(|source| RunnerError::Io {
                path: directory.to_path_buf(),
                source,
            })?;
        }
        RunMode::Fresh | RunMode::Resume | RunMode::Force => {}
    }
    fs::create_dir_all(directory).map_err(|source| RunnerError::Io {
        path: directory.to_path_buf(),
        source,
    })
}

fn reject_dangerous_force_target(directory: &Path) -> Result<(), RunnerError> {
    let current = std::env::current_dir().map_err(|source| RunnerError::Io {
        path: PathBuf::from("."),
        source,
    })?;
    let canonical = directory.canonicalize().map_err(|source| RunnerError::Io {
        path: directory.to_path_buf(),
        source,
    })?;
    if canonical == current || canonical.parent().is_none() {
        return Err(RunnerError::UnsafeForceTarget(directory.to_path_buf()));
    }
    Ok(())
}

fn chain_path(directory: &Path, chain_index: usize) -> PathBuf {
    directory
        .join("chains")
        .join(format!("chain-{chain_index:06}.json"))
}

fn count_completed_chain_files(directory: &Path, chains: usize) -> usize {
    (0..chains)
        .filter(|&chain_index| chain_path(directory, chain_index).is_file())
        .count()
}

fn new_manifest(config: &RunConfig) -> RunManifest {
    let embedded_revision = option_env!("SSE_GIT_REVISION")
        .filter(|revision| !revision.is_empty() && *revision != "unknown")
        .map(str::to_string);
    RunManifest {
        artifact_schema_version: ARTIFACT_SCHEMA_VERSION.to_string(),
        run_schema_version: config.schema_version.clone(),
        name: config.name.clone(),
        model: config.model.kind_name().to_string(),
        software_version: env!("CARGO_PKG_VERSION").to_string(),
        git_revision: embedded_revision,
        status: RunStatus::Running,
        started_unix_seconds: unix_seconds(),
        completed_unix_seconds: None,
        attempts: 0,
        requested_chains: config.execution.chains,
        completed_chains: 0,
        error: None,
        files: Vec::new(),
    }
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Failure while preparing, executing, or resuming a configured run.
#[derive(Debug)]
pub enum RunnerError {
    /// Input configuration was invalid.
    Config(ConfigError),
    /// Artifact serialization or durable write failed.
    Artifact(ArtifactError),
    /// An unstructured file-system operation failed.
    Io {
        /// Path involved in the operation.
        path: PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },
    /// Fresh mode was given an existing output path.
    OutputExists(PathBuf),
    /// Resume mode was given a missing or non-directory path.
    ResumeDirectoryMissing(PathBuf),
    /// Resume configuration differed from the immutable resolved input.
    ResumeConfigurationMismatch,
    /// Force mode targeted the working directory or file-system root.
    UnsafeForceTarget(PathBuf),
    /// Force mode targeted a non-empty directory not recognized as an SSE run.
    UnrecognizedForceTarget(PathBuf),
    /// A durable per-chain checkpoint failed integrity checks.
    InvalidCheckpoint {
        /// Index of the malformed chain artifact.
        chain_index: usize,
    },
    /// Rayon could not create the requested worker pool.
    ThreadPool(String),
    /// One chain failed during construction or sampling.
    Chain {
        /// Stable chain index.
        chain_index: usize,
        /// Underlying sampler message.
        message: String,
    },
}

impl fmt::Display for RunnerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(error) => error.fmt(f),
            Self::Artifact(error) => error.fmt(f),
            Self::Io { path, source } => write!(f, "I/O failed at {}: {source}", path.display()),
            Self::OutputExists(path) => write!(
                f,
                "output directory {} already exists; use --resume or --force",
                path.display()
            ),
            Self::ResumeDirectoryMissing(path) => {
                write!(f, "resume directory {} does not exist", path.display())
            }
            Self::ResumeConfigurationMismatch => {
                write!(f, "resume configuration differs from config.resolved.yaml")
            }
            Self::UnsafeForceTarget(path) => {
                write!(
                    f,
                    "refusing to replace unsafe output path {}",
                    path.display()
                )
            }
            Self::UnrecognizedForceTarget(path) => write!(
                f,
                "refusing to replace non-SSE directory {}; remove it manually if intended",
                path.display()
            ),
            Self::InvalidCheckpoint { chain_index } => {
                write!(f, "chain checkpoint {chain_index} failed integrity checks")
            }
            Self::ThreadPool(message) => write!(f, "failed to create worker pool: {message}"),
            Self::Chain {
                chain_index,
                message,
            } => write!(f, "chain {chain_index} failed: {message}"),
        }
    }
}

impl Error for RunnerError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Config(source) => Some(source),
            Self::Artifact(source) => Some(source),
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl From<ConfigError> for RunnerError {
    fn from(value: ConfigError) -> Self {
        Self::Config(value)
    }
}

impl From<ArtifactError> for RunnerError {
    fn from(value: ArtifactError) -> Self {
        Self::Artifact(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        BoundaryConfig, ExecutionSettings, GeometryConfig, InitialState, SimulationSettings,
        RUN_SCHEMA_VERSION,
    };

    fn tiny_config() -> RunConfig {
        RunConfig {
            schema_version: RUN_SCHEMA_VERSION.to_string(),
            name: "runner test".into(),
            model: ModelConfig::Tfim {
                geometry: GeometryConfig::Chain {
                    length: 2,
                    boundary: BoundaryConfig::Open,
                },
                j: 1.0,
                h: 0.5,
            },
            simulation: SimulationSettings {
                beta: 1.0,
                operator_string_length: 16,
                thermalization_sweeps: 2,
                measurement_sweeps: 8,
                sweeps_per_measurement: 1,
            },
            execution: ExecutionSettings {
                chains: 2,
                threads: 2,
                seed: 7,
            },
            initial_state: InitialState::Down,
        }
    }

    #[test]
    fn writes_and_resumes_complete_artifact_set() {
        let directory = std::env::temp_dir().join(format!(
            "sse-runner-test-{}-{}",
            std::process::id(),
            unix_seconds()
        ));
        if directory.exists() {
            fs::remove_dir_all(&directory).unwrap();
        }
        let config = tiny_config();
        let first = run_to_directory(&config, Some("input"), &directory, RunMode::Fresh).unwrap();
        assert_eq!(first.reused_chains, 0);
        assert_eq!(first.summary.chains, 2);
        assert!(directory.join("measurements.csv").is_file());

        let resumed = run_to_directory(&config, None, &directory, RunMode::Resume).unwrap();
        assert_eq!(resumed.reused_chains, 2);
        assert_eq!(
            resumed.summary.energy_per_site,
            first.summary.energy_per_site
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn runs_configured_rydberg_chains() {
        let directory = std::env::temp_dir().join(format!(
            "sse-runner-rydberg-test-{}-{}",
            std::process::id(),
            unix_seconds()
        ));
        if directory.exists() {
            fs::remove_dir_all(&directory).unwrap();
        }
        let mut config = tiny_config();
        config.model = ModelConfig::Rydberg {
            geometry: GeometryConfig::Chain {
                length: 2,
                boundary: BoundaryConfig::Open,
            },
            omega: 1.0,
            detuning: 0.5,
            c6: 1.0,
            update: crate::config::RydbergUpdate::Local,
        };

        let outcome = run_to_directory(&config, None, &directory, RunMode::Fresh).unwrap();
        assert_eq!(outcome.summary.model, "rydberg");
        assert_eq!(outcome.summary.samples, 16);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn one_site_tfim_chain_matches_exact_thermal_energy() {
        let directory = std::env::temp_dir().join(format!(
            "sse-runner-exact-test-{}-{}",
            std::process::id(),
            unix_seconds()
        ));
        if directory.exists() {
            fs::remove_dir_all(&directory).unwrap();
        }
        let mut config = tiny_config();
        config.model = ModelConfig::Tfim {
            geometry: GeometryConfig::Chain {
                length: 1,
                boundary: BoundaryConfig::Open,
            },
            j: 0.0,
            h: 1.0,
        };
        config.simulation.beta = 2.0;
        config.simulation.operator_string_length = 32;
        config.simulation.thermalization_sweeps = 2_000;
        config.simulation.measurement_sweeps = 30_000;
        config.execution.chains = 1;
        config.execution.threads = 1;

        let outcome = run_to_directory(&config, None, &directory, RunMode::Fresh).unwrap();
        let exact = -2.0_f64.tanh();
        assert!((outcome.summary.energy_per_site - exact).abs() < 0.03);
        fs::remove_dir_all(directory).unwrap();
    }
}
