//! Versioned, machine-readable artifacts produced by command-line runs.

use std::error::Error;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{de::DeserializeOwned, Deserialize, Serialize};

/// Artifact schema emitted by this release.
pub const ARTIFACT_SCHEMA_VERSION: &str = "sse-artifacts-v1";

static TEMP_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Lifecycle state recorded in `manifest.json`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    /// At least one requested chain has not completed.
    Running,
    /// Every chain and aggregate artifact completed successfully.
    Complete,
    /// The most recent execution attempt returned an error.
    Failed,
}

/// Top-level provenance and lifecycle record.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunManifest {
    /// Artifact format discriminator.
    pub artifact_schema_version: String,
    /// Input configuration schema discriminator.
    pub run_schema_version: String,
    /// Human-readable run name.
    pub name: String,
    /// Model family, currently `tfim` or `rydberg`.
    pub model: String,
    /// Crate version used to execute the run.
    pub software_version: String,
    /// Source revision embedded at build time, when available.
    pub git_revision: Option<String>,
    /// Current lifecycle state.
    pub status: RunStatus,
    /// Unix timestamp of initial output-directory creation.
    pub started_unix_seconds: u64,
    /// Unix timestamp of successful completion.
    pub completed_unix_seconds: Option<u64>,
    /// Number of execution attempts, including resumes.
    pub attempts: u32,
    /// Requested number of independent chains.
    pub requested_chains: usize,
    /// Number of durable completed-chain artifacts.
    pub completed_chains: usize,
    /// Last execution error, if the run is failed.
    pub error: Option<String>,
    /// Relative paths of aggregate artifacts present after completion.
    pub files: Vec<String>,
}

/// Acceptance counts stored for one chain.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateStatistics {
    /// Proposed diagonal insertions.
    pub insertions_proposed: usize,
    /// Accepted diagonal insertions.
    pub insertions_accepted: usize,
    /// Proposed diagonal removals.
    pub removals_proposed: usize,
    /// Accepted diagonal removals.
    pub removals_accepted: usize,
    /// Identified clusters or world lines.
    pub clusters: usize,
    /// Flipped clusters or world lines.
    pub flipped_clusters: usize,
    /// Toggled vertices.
    pub vertices_toggled: usize,
    /// Metropolis-corrected proposals.
    pub proposals: usize,
    /// Accepted corrected proposals.
    pub proposals_accepted: usize,
}

/// Wall-clock timing stored in portable second units.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimingArtifact {
    /// Complete chain runtime.
    pub total_seconds: f64,
    /// Thermalization runtime.
    pub thermalization_seconds: f64,
    /// Measurement runtime.
    pub measurement_seconds: f64,
    /// Diagonal-update runtime.
    pub diagonal_update_seconds: f64,
    /// Cluster or world-line update runtime.
    pub off_diagonal_update_seconds: f64,
    /// Measurement accumulation runtime.
    pub accumulation_seconds: f64,
}

/// Thermodynamic estimates stored for one chain.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThermodynamicArtifact {
    /// Number of measurements.
    pub samples: u64,
    /// Mean SSE expansion order.
    pub mean_expansion_order: f64,
    /// Total energy.
    pub energy: f64,
    /// Energy per site.
    pub energy_per_site: f64,
    /// Total heat-capacity estimator.
    pub heat_capacity: f64,
    /// Heat capacity per site.
    pub heat_capacity_per_site: f64,
}

/// Correlation diagnostics calculated from one expansion-order series.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChainDiagnostics {
    /// Sample variance of expansion order.
    pub expansion_order_variance: f64,
    /// Positive-window estimate of integrated autocorrelation time.
    pub integrated_autocorrelation_time: f64,
    /// Approximate effective sample count.
    pub effective_sample_size: f64,
    /// Autocorrelation-adjusted standard error of energy per site.
    pub energy_standard_error_per_site: f64,
}

/// One durable, independently resumable chain result.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChainArtifact {
    /// Artifact format discriminator.
    pub artifact_schema_version: String,
    /// Stable zero-based chain index.
    pub chain_index: usize,
    /// Deterministically derived chain seed.
    pub seed: u64,
    /// Thermodynamic estimates.
    pub thermodynamics: ThermodynamicArtifact,
    /// Update proposal and acceptance counts.
    pub updates: UpdateStatistics,
    /// Runtime breakdown.
    pub timing: TimingArtifact,
    /// Serial-correlation diagnostics.
    pub diagnostics: ChainDiagnostics,
    /// Expansion orders in measurement order.
    pub expansion_orders: Vec<usize>,
}

/// Aggregate result written to `summary.json`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunSummary {
    /// Artifact format discriminator.
    pub artifact_schema_version: String,
    /// Human-readable run name.
    pub name: String,
    /// Model family.
    pub model: String,
    /// Number of sites.
    pub num_sites: usize,
    /// Number of independent chains.
    pub chains: usize,
    /// Total number of recorded measurements.
    pub samples: u64,
    /// Unweighted mean of independent-chain energy-density estimates.
    pub energy_per_site: f64,
    /// Standard error across independent-chain means, unavailable for one chain.
    pub chain_standard_error: Option<f64>,
    /// Split-chain potential scale-reduction factor for expansion order.
    pub split_r_hat: Option<f64>,
    /// Smallest effective sample-size estimate among chains.
    pub minimum_effective_sample_size: f64,
    /// Non-fatal statistical cautions requiring user assessment.
    pub warnings: Vec<String>,
    /// Results sorted by chain index.
    pub chain_results: Vec<ChainSummary>,
}

/// Compact per-chain entry embedded in [`RunSummary`].
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChainSummary {
    /// Stable zero-based chain index.
    pub chain_index: usize,
    /// Chain seed.
    pub seed: u64,
    /// Energy per site.
    pub energy_per_site: f64,
    /// Heat capacity per site.
    pub heat_capacity_per_site: f64,
    /// Effective expansion-order sample count.
    pub effective_sample_size: f64,
    /// Integrated autocorrelation time.
    pub integrated_autocorrelation_time: f64,
    /// Chain runtime.
    pub wall_time_seconds: f64,
}

/// Completed chain indices recorded after aggregate assembly.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckpointIndex {
    /// Artifact format discriminator.
    pub artifact_schema_version: String,
    /// Completed chain indices in ascending order.
    pub completed_chains: Vec<usize>,
}

/// Read-only view returned by `sse inspect` and [`inspect_run`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunInspection {
    /// Run directory inspected.
    pub directory: PathBuf,
    /// Current manifest.
    pub manifest: RunManifest,
    /// Aggregate summary when the run reached completion.
    pub summary: Option<RunSummary>,
}

/// Reads and validates the manifest and optional summary from a run directory.
///
/// # Errors
///
/// Returns an artifact error for missing, unreadable, or malformed files.
pub fn inspect_run(directory: impl AsRef<Path>) -> Result<RunInspection, ArtifactError> {
    let directory = directory.as_ref();
    let manifest = read_json(&directory.join("manifest.json"))?;
    let summary_path = directory.join("summary.json");
    let summary = if summary_path.exists() {
        Some(read_json(&summary_path)?)
    } else {
        None
    };
    Ok(RunInspection {
        directory: directory.to_path_buf(),
        manifest,
        summary,
    })
}

pub(crate) fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, ArtifactError> {
    let bytes = fs::read(path).map_err(|source| ArtifactError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|source| ArtifactError::Json {
        path: path.to_path_buf(),
        source,
    })
}

pub(crate) fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<(), ArtifactError> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|source| ArtifactError::Json {
        path: path.to_path_buf(),
        source,
    })?;
    bytes.push(b'\n');
    write_bytes_atomic(path, &bytes)
}

pub(crate) fn write_text_atomic(path: &Path, text: &str) -> Result<(), ArtifactError> {
    write_bytes_atomic(path, text.as_bytes())
}

fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> Result<(), ArtifactError> {
    let parent = path
        .parent()
        .ok_or_else(|| ArtifactError::InvalidPath(path.to_path_buf()))?;
    fs::create_dir_all(parent).map_err(|source| ArtifactError::Io {
        path: parent.to_path_buf(),
        source,
    })?;

    let file_name = path
        .file_name()
        .ok_or_else(|| ArtifactError::InvalidPath(path.to_path_buf()))?
        .to_string_lossy();
    let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(
        ".{file_name}.{}.{}.tmp",
        std::process::id(),
        sequence
    ));
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .map_err(|source| ArtifactError::Io {
                path: temporary.clone(),
                source,
            })?;
        file.write_all(bytes).map_err(|source| ArtifactError::Io {
            path: temporary.clone(),
            source,
        })?;
        file.sync_all().map_err(|source| ArtifactError::Io {
            path: temporary.clone(),
            source,
        })?;
        fs::rename(&temporary, path).map_err(|source| ArtifactError::Io {
            path: path.to_path_buf(),
            source,
        })
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

/// Failure while reading or writing versioned run artifacts.
#[derive(Debug)]
pub enum ArtifactError {
    /// File-system operation failed.
    Io {
        /// Path involved in the operation.
        path: PathBuf,
        /// Underlying I/O failure.
        source: std::io::Error,
    },
    /// JSON serialization or parsing failed.
    Json {
        /// Artifact involved in the operation.
        path: PathBuf,
        /// Underlying JSON failure.
        source: serde_json::Error,
    },
    /// A path had no usable parent or file name.
    InvalidPath(PathBuf),
}

impl fmt::Display for ArtifactError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(f, "artifact I/O failed at {}: {source}", path.display())
            }
            Self::Json { path, source } => {
                write!(f, "invalid JSON artifact {}: {source}", path.display())
            }
            Self::InvalidPath(path) => write!(f, "invalid artifact path {}", path.display()),
        }
    }
}

impl Error for ArtifactError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Json { source, .. } => Some(source),
            Self::InvalidPath(_) => None,
        }
    }
}
