//! Versioned, physics-first configuration for reproducible command-line runs.

use std::error::Error;
use std::fmt;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{BoundaryCondition, Geometry, LocalSseModel, SimulationConfig, Spin, SseModel};

/// Configuration schema accepted by this release.
pub const RUN_SCHEMA_VERSION: &str = "sse-run-v1";

/// Complete input required to reproduce one simulation campaign.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunConfig {
    /// Schema discriminator. Must equal [`RUN_SCHEMA_VERSION`].
    pub schema_version: String,
    /// Human-readable run name included in result metadata.
    pub name: String,
    /// Hamiltonian and geometry definition.
    pub model: ModelConfig,
    /// Monte Carlo schedule and inverse temperature.
    pub simulation: SimulationSettings,
    /// Independent-chain and worker settings.
    #[serde(default)]
    pub execution: ExecutionSettings,
    /// Initial basis state shared by all chains.
    #[serde(default)]
    pub initial_state: InitialState,
}

impl RunConfig {
    /// Reads and deserializes a YAML configuration file.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] for I/O, YAML, schema, or physical validation
    /// failures.
    pub fn from_yaml_file(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let path = path.as_ref();
        let input = fs::read_to_string(path).map_err(|source| ConfigError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        Self::from_yaml_str(&input)
    }

    /// Deserializes and validates a YAML configuration string.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Yaml`] or [`ConfigError::Validation`].
    pub fn from_yaml_str(input: &str) -> Result<Self, ConfigError> {
        let config: Self = serde_yaml_ng::from_str(input).map_err(ConfigError::Yaml)?;
        config.validate()?;
        Ok(config)
    }

    /// Serializes this configuration in canonical field order.
    ///
    /// # Errors
    ///
    /// Returns a YAML serialization failure.
    pub fn to_yaml_string(&self) -> Result<String, ConfigError> {
        serde_yaml_ng::to_string(self).map_err(ConfigError::Yaml)
    }

    /// Checks schema, numerical, geometry, and model invariants.
    ///
    /// # Errors
    ///
    /// Returns the first invalid field or unsupported combination.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.schema_version != RUN_SCHEMA_VERSION {
            return Err(ConfigError::validation(format!(
                "schema_version must be {RUN_SCHEMA_VERSION:?}; got {:?}",
                self.schema_version
            )));
        }
        if self.name.trim().is_empty() {
            return Err(ConfigError::validation("name must not be empty"));
        }
        self.simulation.validate()?;
        self.execution.validate()?;

        let geometry = self.model.geometry().build()?;
        let model = self.model.build_model(&geometry)?;
        self.initial_state.build(model.num_sites())?;
        Ok(())
    }
}

/// Supported physical Hamiltonians.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ModelConfig {
    /// Ferromagnetic transverse-field Ising model.
    Tfim {
        /// Simulation geometry.
        geometry: GeometryConfig,
        /// Non-negative nearest-neighbour Ising coupling.
        j: f64,
        /// Non-negative transverse field.
        h: f64,
    },
    /// Long-range Rydberg model.
    Rydberg {
        /// Simulation geometry.
        geometry: GeometryConfig,
        /// Non-negative Rabi frequency.
        omega: f64,
        /// Signed detuning.
        detuning: f64,
        /// Signed van der Waals coefficient.
        c6: f64,
        /// World-line update used during sampling.
        #[serde(default)]
        update: RydbergUpdate,
    },
}

impl ModelConfig {
    /// Returns the geometry portion of this model configuration.
    #[must_use]
    pub fn geometry(&self) -> &GeometryConfig {
        match self {
            Self::Tfim { geometry, .. } | Self::Rydberg { geometry, .. } => geometry,
        }
    }

    /// Returns a stable lowercase model-family name.
    #[must_use]
    pub fn kind_name(&self) -> &'static str {
        match self {
            Self::Tfim { .. } => "tfim",
            Self::Rydberg { .. } => "rydberg",
        }
    }

    /// Constructs the validated local SSE decomposition.
    ///
    /// # Errors
    ///
    /// Returns a configuration error when geometry or model construction
    /// fails.
    pub fn build_model(&self, geometry: &Geometry) -> Result<LocalSseModel, ConfigError> {
        match self {
            Self::Tfim { j, h, .. } => {
                let pairs = geometry
                    .pairs_at_distance_squared(1.0, 1.0e-12)
                    .map_err(|error| ConfigError::validation(error.to_string()))?;
                LocalSseModel::tfim(geometry, &pairs, *j, *h)
                    .map_err(|error| ConfigError::validation(error.to_string()))
            }
            Self::Rydberg {
                omega,
                detuning,
                c6,
                ..
            } => LocalSseModel::rydberg(geometry, *omega, *detuning, *c6)
                .map_err(|error| ConfigError::validation(error.to_string())),
        }
    }

    /// Returns the selected Rydberg update, or `None` for TFIM.
    #[must_use]
    pub fn rydberg_update(&self) -> Option<RydbergUpdate> {
        match self {
            Self::Rydberg { update, .. } => Some(*update),
            Self::Tfim { .. } => None,
        }
    }
}

/// Geometry encodings supported by the configuration schema.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum GeometryConfig {
    /// Unit-spaced one-dimensional chain.
    Chain {
        /// Number of sites.
        length: usize,
        /// Boundary condition along the chain.
        boundary: BoundaryConfig,
    },
    /// Unit-spaced rectangular lattice.
    Rectangular {
        /// Number of sites along x.
        lx: usize,
        /// Number of sites along y.
        ly: usize,
        /// Boundary condition along x.
        boundary_x: BoundaryConfig,
        /// Boundary condition along y.
        boundary_y: BoundaryConfig,
    },
    /// Arbitrary two-dimensional coordinates with open boundaries.
    Custom {
        /// Cartesian positions indexed by site ID.
        coordinates: Vec<[f64; 2]>,
    },
}

impl GeometryConfig {
    /// Constructs the validated core geometry.
    ///
    /// # Errors
    ///
    /// Returns a configuration error for invalid dimensions or coordinates.
    pub fn build(&self) -> Result<Geometry, ConfigError> {
        let result = match self {
            Self::Chain { length, boundary } => Geometry::chain(*length, (*boundary).into()),
            Self::Rectangular {
                lx,
                ly,
                boundary_x,
                boundary_y,
            } => Geometry::rectangular(*lx, *ly, (*boundary_x).into(), (*boundary_y).into()),
            Self::Custom { coordinates } => Geometry::custom(coordinates.clone()),
        };
        result.map_err(|error| ConfigError::validation(error.to_string()))
    }
}

/// Serializable boundary condition.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoundaryConfig {
    /// Open boundary.
    Open,
    /// Periodic boundary.
    Periodic,
}

impl From<BoundaryConfig> for BoundaryCondition {
    fn from(value: BoundaryConfig) -> Self {
        match value {
            BoundaryConfig::Open => Self::Open,
            BoundaryConfig::Periodic => Self::Periodic,
        }
    }
}

/// Rydberg off-diagonal update selection.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RydbergUpdate {
    /// Production local world-line update.
    #[default]
    Local,
    /// Global Metropolis-corrected reference update for validation.
    GlobalReference,
}

/// Thermalization and measurement schedule.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SimulationSettings {
    /// Positive inverse temperature.
    pub beta: f64,
    /// Initial padded operator-string cutoff.
    pub operator_string_length: usize,
    /// Sweeps discarded before measurement.
    pub thermalization_sweeps: usize,
    /// Number of recorded expansion-order measurements per chain.
    pub measurement_sweeps: usize,
    /// Complete updates between consecutive measurements.
    #[serde(default = "default_sweeps_per_measurement")]
    pub sweeps_per_measurement: usize,
}

impl SimulationSettings {
    fn validate(self) -> Result<(), ConfigError> {
        if !self.beta.is_finite() || self.beta <= 0.0 {
            return Err(ConfigError::validation(
                "simulation.beta must be finite and positive",
            ));
        }
        if self.operator_string_length == 0 {
            return Err(ConfigError::validation(
                "simulation.operator_string_length must be greater than zero",
            ));
        }
        if self.measurement_sweeps == 0 {
            return Err(ConfigError::validation(
                "simulation.measurement_sweeps must be greater than zero",
            ));
        }
        if self.sweeps_per_measurement == 0 {
            return Err(ConfigError::validation(
                "simulation.sweeps_per_measurement must be greater than zero",
            ));
        }
        Ok(())
    }
}

impl From<SimulationSettings> for SimulationConfig {
    fn from(value: SimulationSettings) -> Self {
        Self {
            thermalization_sweeps: value.thermalization_sweeps,
            measurement_sweeps: value.measurement_sweeps,
            sweeps_per_measurement: value.sweeps_per_measurement,
        }
    }
}

const fn default_sweeps_per_measurement() -> usize {
    1
}

/// Parallel execution settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ExecutionSettings {
    /// Number of statistically independent chains.
    pub chains: usize,
    /// Maximum number of concurrent worker threads.
    pub threads: usize,
    /// Master seed from which chain seeds are deterministically derived.
    pub seed: u64,
}

impl Default for ExecutionSettings {
    fn default() -> Self {
        Self {
            chains: 4,
            threads: 1,
            seed: 0,
        }
    }
}

impl ExecutionSettings {
    fn validate(self) -> Result<(), ConfigError> {
        if self.chains == 0 {
            return Err(ConfigError::validation(
                "execution.chains must be greater than zero",
            ));
        }
        if self.threads == 0 {
            return Err(ConfigError::validation(
                "execution.threads must be greater than zero",
            ));
        }
        Ok(())
    }
}

/// Shared initial basis-state prescription.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InitialState {
    /// Every site starts in [`Spin::Down`].
    #[default]
    Down,
    /// Every site starts in [`Spin::Up`].
    Up,
    /// Even sites start up and odd sites start down.
    Alternating,
}

impl InitialState {
    /// Materializes one spin per site.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty model.
    pub fn build(self, num_sites: usize) -> Result<Vec<Spin>, ConfigError> {
        if num_sites == 0 {
            return Err(ConfigError::validation(
                "initial state requires at least one site",
            ));
        }
        Ok(match self {
            Self::Down => vec![Spin::Down; num_sites],
            Self::Up => vec![Spin::Up; num_sites],
            Self::Alternating => (0..num_sites)
                .map(|site| if site % 2 == 0 { Spin::Up } else { Spin::Down })
                .collect(),
        })
    }
}

/// Failure while reading, parsing, or validating a run configuration.
#[derive(Debug)]
pub enum ConfigError {
    /// File-system operation failed.
    Io {
        /// File involved in the failed operation.
        path: std::path::PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },
    /// YAML syntax or type mismatch.
    Yaml(serde_yaml_ng::Error),
    /// Schema or physical invariant violation.
    Validation(String),
}

impl ConfigError {
    fn validation(message: impl Into<String>) -> Self {
        Self::Validation(message.into())
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(f, "failed to read {}: {source}", path.display())
            }
            Self::Yaml(error) => write!(f, "invalid YAML configuration: {error}"),
            Self::Validation(message) => write!(f, "invalid run configuration: {message}"),
        }
    }
}

impl Error for ConfigError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Yaml(source) => Some(source),
            Self::Validation(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TFIM: &str = r#"
schema_version: sse-run-v1
name: four-site TFIM
model:
  kind: tfim
  geometry:
    kind: chain
    length: 4
    boundary: periodic
  j: 1.0
  h: 0.5
simulation:
  beta: 2.0
  operator_string_length: 64
  thermalization_sweeps: 10
  measurement_sweeps: 20
execution:
  chains: 2
  threads: 1
  seed: 7
initial_state: alternating
"#;

    #[test]
    fn parses_and_validates_tfim() {
        let config = RunConfig::from_yaml_str(TFIM).unwrap();
        assert_eq!(config.model.kind_name(), "tfim");
        assert_eq!(config.simulation.sweeps_per_measurement, 1);
        assert_eq!(config.initial_state, InitialState::Alternating);
    }

    #[test]
    fn rejects_unknown_fields() {
        let input = TFIM.replace("  beta: 2.0", "  beta: 2.0\n  typo: true");
        let error = RunConfig::from_yaml_str(&input).unwrap_err().to_string();
        assert!(error.contains("unknown field `typo`"));
    }

    #[test]
    fn rejects_unknown_schema() {
        let input = TFIM.replace(RUN_SCHEMA_VERSION, "sse-run-v2");
        let error = RunConfig::from_yaml_str(&input).unwrap_err().to_string();
        assert!(error.contains(RUN_SCHEMA_VERSION));
    }

    #[test]
    fn round_trip_preserves_configuration() {
        let config = RunConfig::from_yaml_str(TFIM).unwrap();
        let yaml = config.to_yaml_string().unwrap();
        assert_eq!(RunConfig::from_yaml_str(&yaml).unwrap(), config);
    }
}
