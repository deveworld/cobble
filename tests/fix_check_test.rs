//! Regression tests for `cobble check` semantic-error reporting:
//! multi-file checks keep going past the first failing file (with
//! cascading-import duplicates removed, while unrelated files that raise
//! byte-identical errors are all reported), and the shared transpiler-error
//! classifier keeps emitting the documented diagnostic kinds through
//! `check --json`.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn cobble() -> Command {
    Command::new(env!("CARGO_BIN_EXE_cobble"))
}

fn output_text(output: &Output) -> (String, String) {
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn check_json(input: &Path) -> (serde_json::Value, String, bool) {
    let output = cobble()
        .arg("check")
        .arg("--json")
        .arg(input)
        .output()
        .unwrap();
    let (stdout, stderr) = output_text(&output);
    let value = serde_json::from_str(&stdout)
        .unwrap_or_else(|error| panic!("invalid check --json output ({error})\nstdout:\n{stdout}"));
    (value, stderr, output.status.success())
}

#[test]
fn cli_check_json_reports_semantic_errors_across_all_files() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    fs::write(
        temp_dir.path().join("a.cbl"),
        "def alpha():\n    for i in range(1025):\n        /say bad {i}\n",
    )
    .unwrap();
    fs::write(
        temp_dir.path().join("b.cbl"),
        "def beta():\n    x = \"abc\"\n    for v in x:\n        /say hi\n",
    )
    .unwrap();

    let (value, stderr, success) = check_json(temp_dir.path());

    assert!(!success);
    assert!(
        stderr.contains("Validation failed with 2 error(s)"),
        "stderr: {stderr}"
    );
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["ok"], false);
    assert_eq!(value["files_checked"], 2);
    assert_eq!(value["error_count"], 2);

    let diagnostics = value["diagnostics"].as_array().unwrap();
    assert_eq!(
        diagnostics.len(),
        2,
        "both files' semantic errors must be reported in one run: {diagnostics:?}"
    );
    assert!(diagnostics[0]["file"].as_str().unwrap().ends_with("a.cbl"));
    assert_eq!(diagnostics[0]["kind"], "unroll-limit-exceeded");
    assert_eq!(diagnostics[0]["line"], 2);
    assert_eq!(diagnostics[0]["column"], 5);
    assert!(diagnostics[1]["file"].as_str().unwrap().ends_with("b.cbl"));
    assert_eq!(diagnostics[1]["kind"], "runtime-for-unsupported");
    assert_eq!(diagnostics[1]["line"], 3);
    assert_eq!(diagnostics[1]["column"], 5);
}

#[test]
fn cli_check_json_deduplicates_cascading_import_errors() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    fs::write(
        temp_dir.path().join("a.cbl"),
        "def alpha():\n    for i in range(1025):\n        /say bad {i}\n",
    )
    .unwrap();
    // b.cbl imports the failing file; its transpile replays the same
    // underlying error, which must not appear a second time.
    fs::write(
        temp_dir.path().join("b.cbl"),
        "import a\n\ndef beta():\n    /say ok\n",
    )
    .unwrap();

    let (value, stderr, success) = check_json(temp_dir.path());

    assert!(!success);
    assert!(
        stderr.contains("Validation failed with 1 error(s)"),
        "stderr: {stderr}"
    );
    assert_eq!(value["error_count"], 1);
    let diagnostics = value["diagnostics"].as_array().unwrap();
    assert_eq!(diagnostics.len(), 1, "diagnostics: {diagnostics:?}");
    assert!(diagnostics[0]["file"].as_str().unwrap().ends_with("a.cbl"));
    assert_eq!(diagnostics[0]["kind"], "unroll-limit-exceeded");
}

#[test]
fn cli_check_json_reports_identical_errors_in_unrelated_files() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    // Different function names, byte-identical failing loop bodies: the raw
    // transpiler error text collides, but the defects are independent, so
    // both files must be reported.
    fs::write(
        temp_dir.path().join("a.cbl"),
        "def alpha():\n    for i in range(1025):\n        /say bad {i}\n",
    )
    .unwrap();
    fs::write(
        temp_dir.path().join("c.cbl"),
        "def gamma():\n    for i in range(1025):\n        /say bad {i}\n",
    )
    .unwrap();

    let (value, stderr, success) = check_json(temp_dir.path());

    assert!(!success);
    assert!(
        stderr.contains("Validation failed with 2 error(s)"),
        "stderr: {stderr}"
    );
    assert_eq!(value["files_checked"], 2);
    assert_eq!(value["error_count"], 2);
    let diagnostics = value["diagnostics"].as_array().unwrap();
    assert_eq!(
        diagnostics.len(),
        2,
        "identical errors in unrelated files must both be reported: {diagnostics:?}"
    );
    assert!(diagnostics[0]["file"].as_str().unwrap().ends_with("a.cbl"));
    assert_eq!(diagnostics[0]["kind"], "unroll-limit-exceeded");
    assert!(diagnostics[1]["file"].as_str().unwrap().ends_with("c.cbl"));
    assert_eq!(diagnostics[1]["kind"], "unroll-limit-exceeded");
}

#[test]
fn cli_check_json_deduplicates_diamond_import_errors() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    fs::write(
        temp_dir.path().join("a.cbl"),
        "def alpha():\n    for i in range(1025):\n        /say bad {i}\n",
    )
    .unwrap();
    // b.cbl and c.cbl both import the failing file; each transpile replays
    // the same underlying error, which must be reported exactly once.
    fs::write(
        temp_dir.path().join("b.cbl"),
        "import a\n\ndef beta():\n    /say ok\n",
    )
    .unwrap();
    fs::write(
        temp_dir.path().join("c.cbl"),
        "import a\n\ndef gamma():\n    /say ok\n",
    )
    .unwrap();

    let (value, stderr, success) = check_json(temp_dir.path());

    assert!(!success);
    assert!(
        stderr.contains("Validation failed with 1 error(s)"),
        "stderr: {stderr}"
    );
    assert_eq!(value["files_checked"], 3);
    assert_eq!(value["error_count"], 1);
    let diagnostics = value["diagnostics"].as_array().unwrap();
    assert_eq!(diagnostics.len(), 1, "diagnostics: {diagnostics:?}");
    assert!(diagnostics[0]["file"].as_str().unwrap().ends_with("a.cbl"));
    assert_eq!(diagnostics[0]["kind"], "unroll-limit-exceeded");
}

#[test]
fn cli_check_json_keeps_missing_stdlib_module_classification() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = temp_dir.path().join("main.cbl");
    fs::write(
        &input,
        r#"from stdlib import datapack, item_component

datapack.item_modifier("named", {
    "type": "minecraft:set_components",
    "components": item_component.custom_name(text.plain("Name"))
})

def main():
    /say ok
"#,
    )
    .unwrap();

    let (value, _stderr, success) = check_json(&input);

    assert!(!success);
    assert_eq!(value["error_count"], 1);
    let diagnostic = &value["diagnostics"][0];
    assert_eq!(diagnostic["kind"], "missing-stdlib-module");
    assert_eq!(diagnostic["line"], 5);
    assert_eq!(diagnostic["column"], 46);
    assert!(diagnostic["message"]
        .as_str()
        .unwrap()
        .starts_with("module 'text' not imported."));
}

#[test]
fn cli_check_json_keeps_unknown_stdlib_module_classification() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = temp_dir.path().join("main.cbl");
    fs::write(
        &input,
        "from stdlib import bogus\n\ndef main():\n    /say ok\n",
    )
    .unwrap();

    let (value, _stderr, success) = check_json(&input);

    assert!(!success);
    assert_eq!(value["error_count"], 1);
    let diagnostic = &value["diagnostics"][0];
    assert_eq!(diagnostic["kind"], "unknown-stdlib-module");
    assert_eq!(diagnostic["line"], 1);
    assert_eq!(diagnostic["column"], 20);
    assert!(diagnostic["message"]
        .as_str()
        .unwrap()
        .starts_with("Unknown stdlib module 'bogus'"));
}
