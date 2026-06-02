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

fn output_text(output: &Output) -> (String, String) {
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
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
