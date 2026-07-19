use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn temporary_directory(test_name: &str) -> std::path::PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "sse-cli-{test_name}-{}-{nonce}",
        std::process::id()
    ))
}

fn tiny_config() -> &'static str {
    r#"schema_version: sse-run-v1
name: CLI integration test
model:
  kind: tfim
  geometry:
    kind: chain
    length: 2
    boundary: open
  j: 1.0
  h: 0.5
simulation:
  beta: 1.0
  operator_string_length: 16
  thermalization_sweeps: 2
  measurement_sweeps: 8
  sweeps_per_measurement: 1
execution:
  chains: 2
  threads: 2
  seed: 11
initial_state: down
"#
}

#[test]
fn validates_runs_inspects_and_resumes() {
    let root = temporary_directory("workflow");
    let config_path = root.join("input.yaml");
    let output = root.join("run");
    fs::create_dir_all(&root).unwrap();
    fs::write(&config_path, tiny_config()).unwrap();

    let validate = Command::new(env!("CARGO_BIN_EXE_sse"))
        .args(["validate", "--config"])
        .arg(&config_path)
        .output()
        .unwrap();
    assert!(validate.status.success());
    assert!(String::from_utf8(validate.stdout)
        .unwrap()
        .contains("valid: tfim model"));

    let run = Command::new(env!("CARGO_BIN_EXE_sse"))
        .args(["run", "--config"])
        .arg(&config_path)
        .arg("--output")
        .arg(&output)
        .arg("--quiet")
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(output.join("summary.json").is_file());
    assert!(output.join("measurements.csv").is_file());

    let inspect = Command::new(env!("CARGO_BIN_EXE_sse"))
        .arg("inspect")
        .arg(&output)
        .arg("--json")
        .output()
        .unwrap();
    assert!(inspect.status.success());
    let inspection: serde_json::Value = serde_json::from_slice(&inspect.stdout).unwrap();
    assert_eq!(inspection["manifest"]["status"], "complete");
    assert_eq!(inspection["summary"]["chains"], 2);

    let resume = Command::new(env!("CARGO_BIN_EXE_sse"))
        .args(["run", "--config"])
        .arg(&config_path)
        .arg("--output")
        .arg(&output)
        .args(["--resume", "--quiet"])
        .output()
        .unwrap();
    assert!(
        resume.status.success(),
        "{}",
        String::from_utf8_lossy(&resume.stderr)
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn fresh_run_refuses_an_existing_output_directory() {
    let root = temporary_directory("existing-output");
    let config_path = root.join("input.yaml");
    let output = root.join("run");
    fs::create_dir_all(&output).unwrap();
    fs::write(&config_path, tiny_config()).unwrap();

    let result = Command::new(env!("CARGO_BIN_EXE_sse"))
        .args(["run", "--config"])
        .arg(&config_path)
        .arg("--output")
        .arg(&output)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8(result.stderr)
        .unwrap()
        .contains("already exists"));

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn force_refuses_a_non_sse_directory() {
    let root = temporary_directory("force-safety");
    let config_path = root.join("input.yaml");
    let output = root.join("important-data");
    fs::create_dir_all(&output).unwrap();
    fs::write(&config_path, tiny_config()).unwrap();
    fs::write(output.join("keep.txt"), "must survive").unwrap();

    let result = Command::new(env!("CARGO_BIN_EXE_sse"))
        .args(["run", "--config"])
        .arg(&config_path)
        .arg("--output")
        .arg(&output)
        .arg("--force")
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(output.join("keep.txt").is_file());
    assert!(String::from_utf8(result.stderr)
        .unwrap()
        .contains("non-SSE directory"));

    fs::remove_dir_all(root).unwrap();
}
