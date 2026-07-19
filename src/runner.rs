//! Reusable orchestration for validated configurations and durable artifacts.

use std::error::Error;
use std::fmt;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use rand::{rngs::StdRng, SeedableRng};
use rayon::prelude::*;

use crate::artifacts::{read_json, write_json_atomic, write_text_atomic};
use crate::{
    derive_chain_seed, ArtifactError, ChainArtifact, ChainDiagnostics, ChainSummary,
    CheckpointIndex, ConfigError, LocalSseModel, ModelConfig, RunConfig, RunManifest, RunStatus,
    RunSummary, RydbergUpdate, SSEState, SimulationConfig, SseModel, SseSampler,
    ThermodynamicArtifact, TimingArtifact, UpdateStatistics, ARTIFACT_SCHEMA_VERSION,
};

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
    let initial_state = Arc::new(config.initial_state.build(model.num_sites())?);

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

fn run_chain(
    config: &RunConfig,
    model: Arc<LocalSseModel>,
    initial_state: &[crate::Spin],
    chain_index: usize,
) -> Result<ChainArtifact, RunnerError> {
    let seed = derive_chain_seed(config.execution.seed, chain_index as u64);
    let state = SSEState::new(
        model.as_ref(),
        initial_state.to_vec(),
        config.simulation.operator_string_length,
    )
    .map_err(|error| RunnerError::Chain {
        chain_index,
        message: error.to_string(),
    })?;
    let rng = StdRng::seed_from_u64(seed);
    let mut sampler = SseSampler::with_shared_model(model, state, config.simulation.beta, rng)
        .map_err(|error| RunnerError::Chain {
            chain_index,
            message: error.to_string(),
        })?;
    let schedule = SimulationConfig::from(config.simulation);
    let recorded = match &config.model {
        ModelConfig::Tfim { .. } => sampler.run_tfim_recorded(schedule),
        ModelConfig::Rydberg { update, .. } => match update {
            RydbergUpdate::Local => sampler.run_rydberg_recorded(schedule),
            RydbergUpdate::GlobalReference => {
                sampler.run_rydberg_global_reference_recorded(schedule)
            }
        },
    }
    .map_err(|error| RunnerError::Chain {
        chain_index,
        message: error.to_string(),
    })?;

    let expansion_orders: Vec<_> = recorded
        .measurements
        .iter()
        .map(|measurement| measurement.expansion_order)
        .collect();
    let diagnostics = diagnose_chain(
        &expansion_orders,
        config.simulation.beta,
        sampler.model().num_sites(),
    );
    let result = recorded.simulation;
    Ok(ChainArtifact {
        artifact_schema_version: ARTIFACT_SCHEMA_VERSION.to_string(),
        chain_index,
        seed,
        thermodynamics: ThermodynamicArtifact {
            samples: result.thermodynamics.samples,
            mean_expansion_order: result.thermodynamics.mean_expansion_order,
            energy: result.thermodynamics.energy,
            energy_per_site: result.thermodynamics.energy_per_site,
            heat_capacity: result.thermodynamics.heat_capacity,
            heat_capacity_per_site: result.thermodynamics.heat_capacity_per_site,
        },
        updates: UpdateStatistics {
            insertions_proposed: result.diagonal.insertions_proposed,
            insertions_accepted: result.diagonal.insertions_accepted,
            removals_proposed: result.diagonal.removals_proposed,
            removals_accepted: result.diagonal.removals_accepted,
            clusters: result.clusters.clusters,
            flipped_clusters: result.clusters.flipped_clusters,
            vertices_toggled: result.clusters.vertices_toggled,
            proposals: result.clusters.proposals,
            proposals_accepted: result.clusters.proposals_accepted,
        },
        timing: TimingArtifact {
            total_seconds: result.timing.total.as_secs_f64(),
            thermalization_seconds: result.timing.thermalization.as_secs_f64(),
            measurement_seconds: result.timing.measurement.as_secs_f64(),
            diagonal_update_seconds: result.timing.diagonal_updates.as_secs_f64(),
            off_diagonal_update_seconds: result.timing.cluster_updates.as_secs_f64(),
            accumulation_seconds: result.timing.accumulation.as_secs_f64(),
        },
        diagnostics,
        expansion_orders,
    })
}

fn validate_chain_artifact(
    config: &RunConfig,
    expected_index: usize,
    chain: &ChainArtifact,
) -> Result<(), RunnerError> {
    let expected_seed = derive_chain_seed(config.execution.seed, expected_index as u64);
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

fn diagnose_chain(orders: &[usize], beta: f64, num_sites: usize) -> ChainDiagnostics {
    let count = orders.len();
    if count < 2 {
        return ChainDiagnostics {
            expansion_order_variance: 0.0,
            integrated_autocorrelation_time: 0.5,
            effective_sample_size: count as f64,
            energy_standard_error_per_site: 0.0,
        };
    }
    let count_f = count as f64;
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
        return ChainDiagnostics {
            expansion_order_variance: sample_variance,
            integrated_autocorrelation_time: 0.5,
            effective_sample_size: count_f,
            energy_standard_error_per_site: 0.0,
        };
    }

    let mut correlation_sum = 0.0;
    let maximum_lag = (count / 2).min(10_000);
    for lag in 1..=maximum_lag {
        let covariance = orders[..count - lag]
            .iter()
            .zip(&orders[lag..])
            .map(|(&left, &right)| (left as f64 - mean) * (right as f64 - mean))
            .sum::<f64>()
            / (count - lag) as f64;
        let correlation = covariance / population_variance;
        if !correlation.is_finite() || correlation <= 0.0 {
            break;
        }
        correlation_sum += correlation;
    }
    let tau = 0.5 + correlation_sum;
    let effective = (count_f / (2.0 * tau)).clamp(1.0, count_f);
    ChainDiagnostics {
        expansion_order_variance: sample_variance,
        integrated_autocorrelation_time: tau,
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
                seed: chain.seed,
                energy_per_site: chain.thermodynamics.energy_per_site,
                heat_capacity_per_site: chain.thermodynamics.heat_capacity_per_site,
                effective_sample_size: chain.diagnostics.effective_sample_size,
                integrated_autocorrelation_time: chain.diagnostics.integrated_autocorrelation_time,
                wall_time_seconds: chain.timing.total_seconds,
            })
            .collect(),
    }
}

fn split_r_hat(chains: &[ChainArtifact]) -> Option<f64> {
    let half_length = chains
        .iter()
        .map(|chain| chain.expansion_orders.len() / 2)
        .min()?;
    if chains.len() < 2 || half_length < 2 {
        return None;
    }
    let sequences: Vec<&[usize]> = chains
        .iter()
        .flat_map(|chain| {
            let values = chain.expansion_orders.as_slice();
            [
                &values[..half_length],
                &values[values.len() - half_length..],
            ]
        })
        .collect();
    let means: Vec<f64> = sequences
        .iter()
        .map(|values| values.iter().map(|&value| value as f64).sum::<f64>() / half_length as f64)
        .collect();
    let variances: Vec<f64> = sequences
        .iter()
        .zip(&means)
        .map(|(values, mean)| {
            values
                .iter()
                .map(|&value| (value as f64 - mean).powi(2))
                .sum::<f64>()
                / (half_length - 1) as f64
        })
        .collect();
    let sequence_count = sequences.len() as f64;
    let mean_of_means = means.iter().sum::<f64>() / sequence_count;
    let between = half_length as f64
        * means
            .iter()
            .map(|mean| (mean - mean_of_means).powi(2))
            .sum::<f64>()
        / (sequence_count - 1.0);
    let within = variances.iter().sum::<f64>() / sequence_count;
    if within <= f64::EPSILON {
        return None;
    }
    let estimated =
        ((half_length - 1) as f64 / half_length as f64) * within + between / half_length as f64;
    Some((estimated / within).sqrt())
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
    use crate::{
        BoundaryConfig, ExecutionSettings, GeometryConfig, InitialState, SimulationSettings,
    };

    fn tiny_config() -> RunConfig {
        RunConfig {
            schema_version: crate::RUN_SCHEMA_VERSION.to_string(),
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
            update: RydbergUpdate::Local,
        };

        let outcome = run_to_directory(&config, None, &directory, RunMode::Fresh).unwrap();
        assert_eq!(outcome.summary.model, "rydberg");
        assert_eq!(outcome.summary.samples, 16);
        fs::remove_dir_all(directory).unwrap();
    }
}
