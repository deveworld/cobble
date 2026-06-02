use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn cobble() -> Command {
    Command::new(env!("CARGO_BIN_EXE_cobble"))
}

fn write_source(temp_dir: &Path, source: &str) -> PathBuf {
    let input = temp_dir.join("main.cbl");
    fs::write(&input, source).unwrap();
    input
}

fn write_say_commands_json(temp_dir: &Path) -> PathBuf {
    let commands_json = temp_dir.join("commands-fixture.json");
    fs::write(
        &commands_json,
        r#"{
            "type": "root",
            "children": {
                "say": {
                    "type": "literal",
                    "children": {
                        "message": {
                            "type": "argument",
                            "parser": "minecraft:message",
                            "executable": true
                        }
                    }
                }
            }
        }"#,
    )
    .unwrap();
    commands_json
}

fn output_text(output: &Output) -> (String, String) {
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn cli_doctor_reports_project_and_command_tree_status() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let commands_json = write_say_commands_json(temp_dir.path());
    fs::create_dir_all(temp_dir.path().join("src")).unwrap();
    fs::write(
        temp_dir.path().join("cobble.toml"),
        r#"
[project]
name = "doctor_project"
description = "Doctor regression project"
namespace = "doctor_project"
version = "1.0.0"
pack_format = "101.1"

[build]
source = "src"
output = "output"
entry_points = []
"#,
    )
    .unwrap();

    let output = cobble()
        .arg("doctor")
        .arg(temp_dir.path())
        .arg("--commands-json")
        .arg(&commands_json)
        .output()
        .unwrap();

    let (stdout, stderr) = output_text(&output);
    assert!(
        output.status.success(),
        "doctor failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(stdout.contains("Cobble doctor"));
    assert!(stdout.contains("Project: doctor_project"));
    assert!(stdout.contains("Namespace: doctor_project"));
    assert!(stdout.contains("Command tree:"));
    assert!(stdout.contains(commands_json.to_string_lossy().as_ref()));
    assert!(stdout.contains("SHA-1:"));
}

#[test]
fn cli_doctor_reports_missing_command_tree_without_download() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let missing_commands = temp_dir.path().join("missing-commands.json");

    let output = cobble()
        .arg("doctor")
        .arg(temp_dir.path())
        .arg("--commands-json")
        .arg(&missing_commands)
        .output()
        .unwrap();

    let (stdout, stderr) = output_text(&output);
    assert!(
        output.status.success(),
        "doctor failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(stdout.contains("Command tree: missing"));
    assert!(stdout.contains(missing_commands.to_string_lossy().as_ref()));
}

#[test]
fn cli_build_dry_run_does_not_write_final_output() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(temp_dir.path(), "def main():\n    /say dry run\n");
    let output_dir = temp_dir.path().join("output");

    let output = cobble()
        .arg("build")
        .arg(&input)
        .arg("--namespace")
        .arg("cli_regression")
        .arg("--output")
        .arg(&output_dir)
        .arg("--dry-run")
        .output()
        .unwrap();

    let (stdout, stderr) = output_text(&output);
    assert!(
        output.status.success(),
        "dry-run failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(stdout.contains("Dry run: final output will not be written"));
    assert!(stdout.contains("Build summary:"));
    assert!(stdout.contains("Output: not written (--dry-run)"));
    assert!(
        !output_dir.exists(),
        "dry-run should not create the final output directory"
    );
}

#[test]
fn cli_build_dry_run_validate_reports_validation_summary() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let commands_json = write_say_commands_json(temp_dir.path());
    let input = write_source(temp_dir.path(), "def main():\n    /say valid\n");
    let output_dir = temp_dir.path().join("output");

    let output = cobble()
        .arg("build")
        .arg(&input)
        .arg("--namespace")
        .arg("cli_regression")
        .arg("--output")
        .arg(&output_dir)
        .arg("--dry-run")
        .arg("--validate")
        .arg("--commands-json")
        .arg(&commands_json)
        .output()
        .unwrap();

    let (stdout, stderr) = output_text(&output);
    assert!(
        output.status.success(),
        "dry-run validation failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(stdout.contains("Validating generated commands"));
    assert!(stdout.contains("All commands valid"));
    assert!(stdout.contains("Validation:"));
    assert!(stdout.contains("Output: not written (--dry-run)"));
    assert!(!output_dir.exists());
}

#[test]
fn cli_build_rejects_dry_run_with_zip() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(temp_dir.path(), "def main():\n    /say invalid options\n");

    let output = cobble()
        .arg("build")
        .arg(&input)
        .arg("--dry-run")
        .arg("--zip")
        .output()
        .unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("--dry-run cannot be combined with --zip"));
}

#[test]
fn cli_build_validate_prints_source_mapped_diagnostics() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let commands_json = write_say_commands_json(temp_dir.path());
    let input = write_source(temp_dir.path(), "def main():\n    /not_a_command\n");
    let output_dir = temp_dir.path().join("output");

    let output = cobble()
        .arg("build")
        .arg(&input)
        .arg("--namespace")
        .arg("cli_regression")
        .arg("--output")
        .arg(&output_dir)
        .arg("--validate")
        .arg("--commands-json")
        .arg(&commands_json)
        .output()
        .unwrap();

    let (stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stdout.contains("Validating generated commands"));
    assert!(stderr.contains("not_a_command"));
    assert!(stderr.contains("^"));
    assert!(stderr.contains("source: main.cbl:2:5"));
    assert!(stderr.contains("validation error(s) found"));
}

#[test]
fn cli_inspect_json_reports_manifest_summary() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(temp_dir.path(), "def main():\n    /say inspect\n");
    let output_dir = temp_dir.path().join("output");

    let build = cobble()
        .arg("build")
        .arg(&input)
        .arg("--namespace")
        .arg("cli_regression")
        .arg("--output")
        .arg(&output_dir)
        .arg("--quiet")
        .output()
        .unwrap();
    let (build_stdout, build_stderr) = output_text(&build);
    assert!(
        build.status.success(),
        "build failed\nstdout:\n{build_stdout}\nstderr:\n{build_stderr}"
    );

    let inspect = cobble()
        .arg("inspect")
        .arg(&output_dir)
        .arg("--json")
        .output()
        .unwrap();
    let (stdout, stderr) = output_text(&inspect);
    assert!(
        inspect.status.success(),
        "inspect failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    let value: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["manifest"]["namespace"], "cli_regression");
    assert_eq!(value["manifest"]["generated"]["functions"], 1);
    assert_eq!(value["manifest"]["generated"]["commands"], 1);
    assert_eq!(value["source_map_entries"], 1);
}

#[test]
fn cli_inspect_human_output_reports_manifest_summary() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(temp_dir.path(), "def main():\n    /say inspect\n");
    let output_dir = temp_dir.path().join("output");

    let build = cobble()
        .arg("build")
        .arg(&input)
        .arg("--namespace")
        .arg("cli_regression")
        .arg("--output")
        .arg(&output_dir)
        .arg("--quiet")
        .output()
        .unwrap();
    let (build_stdout, build_stderr) = output_text(&build);
    assert!(
        build.status.success(),
        "build failed\nstdout:\n{build_stdout}\nstderr:\n{build_stderr}"
    );

    let inspect = cobble().arg("inspect").arg(&output_dir).output().unwrap();
    let (stdout, stderr) = output_text(&inspect);
    assert!(
        inspect.status.success(),
        "inspect failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(stdout.contains("Cobble inspect:"));
    assert!(stdout.contains("Namespace: cli_regression"));
    assert!(stdout.contains("Functions: 1"));
    assert!(stdout.contains("Commands: 1"));
    assert!(stdout.contains("Validation: not recorded"));
}

#[test]
fn cli_inspect_missing_manifest_fails_with_actionable_error() {
    let temp_dir = tempfile::TempDir::new().unwrap();

    let output = cobble()
        .arg("inspect")
        .arg(temp_dir.path())
        .output()
        .unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("No Cobble build manifest found"));
    assert!(stderr.contains("Run `cobble build`"));
}

#[test]
fn cli_inspect_malformed_manifest_reports_parse_error() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let cobble_dir = temp_dir.path().join(".cobble");
    fs::create_dir_all(&cobble_dir).unwrap();
    fs::write(cobble_dir.join("build_manifest.json"), "{not json").unwrap();

    let output = cobble()
        .arg("inspect")
        .arg(temp_dir.path())
        .output()
        .unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("Failed to parse"));
    assert!(stderr.contains("build_manifest.json"));
}
