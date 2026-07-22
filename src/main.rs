//! Physicist-facing command-line interface for reproducible SSE simulations.
//!
//! The Monte Carlo engine is the published
//! [`qslib-quantum`](https://crates.io/crates/qslib-quantum) SSE backend; this
//! binary owns configuration, orchestration, durable artifacts, and
//! statistical reporting.

mod artifacts;
mod config;
mod model;
mod runner;

use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use crate::artifacts::{inspect_run, RunStatus};
use crate::config::RunConfig;
use crate::runner::{run_to_directory, RunMode};

#[derive(Debug, Parser)]
#[command(
    name = "sse",
    version,
    about = "Finite-temperature stochastic series expansion quantum Monte Carlo",
    subcommand_required = true,
    arg_required_else_help = true
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Validate a run configuration without starting Monte Carlo sampling.
    Validate {
        /// YAML configuration file.
        #[arg(short, long, value_name = "FILE")]
        config: PathBuf,
        /// Print the canonical validated configuration.
        #[arg(long)]
        print_resolved: bool,
    },
    /// Execute a configured simulation and write durable artifacts.
    Run {
        /// YAML configuration file.
        #[arg(short, long, value_name = "FILE")]
        config: PathBuf,
        /// New or resumable output directory.
        #[arg(short, long, value_name = "DIRECTORY")]
        output: PathBuf,
        /// Reuse completed chains from a matching interrupted run.
        #[arg(long, conflicts_with = "force")]
        resume: bool,
        /// Replace an existing output directory after safety checks.
        #[arg(long, conflicts_with = "resume")]
        force: bool,
        /// Suppress informational progress output.
        #[arg(short, long)]
        quiet: bool,
    },
    /// Display the status and aggregate results of a run directory.
    Inspect {
        /// Run artifact directory.
        #[arg(value_name = "DIRECTORY")]
        directory: PathBuf,
        /// Emit the complete inspection object as JSON.
        #[arg(long)]
        json: bool,
    },
}

fn main() -> ExitCode {
    match execute(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn execute(cli: Cli) -> Result<(), Box<dyn Error>> {
    match cli.command {
        Command::Validate {
            config,
            print_resolved,
        } => {
            let config = RunConfig::from_yaml_file(&config)?;
            let geometry = config.model.geometry().build()?;
            if print_resolved {
                print!("{}", config.to_yaml_string()?);
            } else {
                println!(
                    "valid: {} model, {} sites, {} independent chain(s)",
                    config.model.kind_name(),
                    geometry.num_sites(),
                    config.execution.chains
                );
            }
        }
        Command::Run {
            config,
            output,
            resume,
            force,
            quiet,
        } => {
            let input = fs::read_to_string(&config)?;
            let config = RunConfig::from_yaml_str(&input)?;
            let mode = if resume {
                RunMode::Resume
            } else if force {
                RunMode::Force
            } else {
                RunMode::Fresh
            };
            if !quiet {
                eprintln!(
                    "running {} chain(s) with {} worker thread(s)",
                    config.execution.chains, config.execution.threads
                );
            }
            let outcome = run_to_directory(&config, Some(&input), &output, mode)?;
            if !quiet {
                if outcome.reused_chains > 0 {
                    eprintln!("reused {} completed chain(s)", outcome.reused_chains);
                }
                eprintln!("artifacts: {}", outcome.directory.display());
            }
            if let Some(standard_error) = outcome.summary.chain_standard_error {
                println!(
                    "energy/site = {:.12} +/- {:.6e}",
                    outcome.summary.energy_per_site, standard_error
                );
            } else {
                println!(
                    "energy/site = {:.12} (between-chain standard error unavailable)",
                    outcome.summary.energy_per_site
                );
            }
            if !outcome.summary.warnings.is_empty() && !quiet {
                eprintln!("diagnostic warnings:");
                for warning in &outcome.summary.warnings {
                    eprintln!("  - {warning}");
                }
            }
        }
        Command::Inspect { directory, json } => {
            let inspection = inspect_run(directory)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&inspection)?);
            } else {
                let status = match inspection.manifest.status {
                    RunStatus::Running => "running",
                    RunStatus::Complete => "complete",
                    RunStatus::Failed => "failed",
                };
                println!("name: {}", inspection.manifest.name);
                println!("status: {status}");
                println!(
                    "chains: {}/{}",
                    inspection.manifest.completed_chains, inspection.manifest.requested_chains
                );
                println!("software: {}", inspection.manifest.software_version);
                if let Some(revision) = &inspection.manifest.git_revision {
                    println!("revision: {revision}");
                }
                if let Some(summary) = inspection.summary {
                    if let Some(standard_error) = summary.chain_standard_error {
                        println!(
                            "energy/site: {:.12} +/- {:.6e}",
                            summary.energy_per_site, standard_error
                        );
                    } else {
                        println!(
                            "energy/site: {:.12} (between-chain standard error unavailable)",
                            summary.energy_per_site
                        );
                    }
                    if let Some(r_hat) = summary.split_r_hat {
                        println!("split R-hat: {r_hat:.6}");
                    }
                    println!(
                        "minimum effective sample size: {:.1}",
                        summary.minimum_effective_sample_size
                    );
                }
                if let Some(error) = inspection.manifest.error {
                    println!("last error: {error}");
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::error::ErrorKind;

    #[test]
    fn resume_and_force_are_mutually_exclusive() {
        let error = Cli::try_parse_from([
            "sse",
            "run",
            "--config",
            "input.yaml",
            "--output",
            "run",
            "--resume",
            "--force",
        ])
        .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::ArgumentConflict);
    }

    #[test]
    fn subcommand_is_required() {
        let error = Cli::try_parse_from(["sse"]).unwrap_err();
        assert_eq!(
            error.kind(),
            ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        );
    }
}
