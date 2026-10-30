//! Regression tests for `cobble check` semantic-error reporting:
//! multi-file checks keep going past the first failing file, report an
//! error in an imported file once and in that file, report byte-identical
//! errors in unrelated files separately, report warnings without failing,
//! and the shared transpiler-error classifier keeps emitting the documented
//! diagnostic kinds through `check --json`.

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
        "def beta():\n    x = {\"a\": 1}\n    for v in x:\n        /say hi\n",
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
    // b.cbl imports the failing file, which was already checked; its error
    // must not appear a second time.
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
fn cli_check_json_reports_an_imported_files_error_in_that_file() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    // b.cbl is checked first and imports z.cbl, so z.cbl fails while b.cbl
    // is being checked.
    fs::write(
        temp_dir.path().join("b.cbl"),
        "import z\n\ndef beta():\n    /say ok\n",
    )
    .unwrap();
    fs::write(
        temp_dir.path().join("z.cbl"),
        "def alpha():\n    for i in range(1025):\n        /say bad {i}\n",
    )
    .unwrap();

    for input in [temp_dir.path().to_path_buf(), temp_dir.path().join("b.cbl")] {
        let (value, stderr, success) = check_json(&input);

        assert!(!success);
        assert!(
            stderr.contains("Validation failed with 1 error(s)"),
            "stderr: {stderr}"
        );
        let diagnostics = value["diagnostics"].as_array().unwrap();
        assert_eq!(diagnostics.len(), 1, "diagnostics: {diagnostics:?}");
        assert!(diagnostics[0]["file"].as_str().unwrap().ends_with("z.cbl"));
        assert_eq!(diagnostics[0]["kind"], "unroll-limit-exceeded");
        assert_eq!(diagnostics[0]["line"], 2);
        assert_eq!(diagnostics[0]["column"], 5);
    }
}

#[test]
fn cli_check_json_reports_warnings_without_failing() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    fs::write(
        temp_dir.path().join("main.cbl"),
        "score = 0\n\ndef tick():\n    /say tick\n    score = 5\n\ndef spin():\n    while True:\n        /say spin\n\ndef wait():\n    while True:\n        return 1\n",
    )
    .unwrap();

    let (value, stderr, success) = check_json(temp_dir.path());

    assert!(success, "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
    assert_eq!(value["ok"], true);
    assert_eq!(value["error_count"], 0);
    assert_eq!(value["warning_count"], 2);
    let diagnostics = value["diagnostics"].as_array().unwrap();
    let found: Vec<_> = diagnostics
        .iter()
        .map(|diagnostic| {
            (
                diagnostic["severity"].as_str().unwrap(),
                diagnostic["kind"].as_str().unwrap(),
                diagnostic["line"].as_u64().unwrap(),
                diagnostic["column"].as_u64().unwrap(),
            )
        })
        .collect();
    // `wait` has a `return` in its loop, so only `spin` warns.
    assert_eq!(
        found,
        [
            ("warning", "local-shadows-module-variable", 5, 5),
            ("warning", "infinite-loop", 8, 5),
        ]
    );
    assert!(diagnostics[0]["help"]
        .as_str()
        .unwrap()
        .contains("global score"));
}

#[test]
fn cli_check_json_points_a_warning_without_a_compiled_assignment_at_the_def() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    // The loop runs no passes, so its body is never compiled.
    fs::write(
        temp_dir.path().join("main.cbl"),
        "score = 0\n\ndef reset(a):\n    for i in range(0):\n        score = 5\n",
    )
    .unwrap();

    let (value, stderr, success) = check_json(temp_dir.path());

    assert!(success, "stderr: {stderr}");
    let diagnostics = value["diagnostics"].as_array().unwrap();
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0]["kind"], "local-shadows-module-variable");
    assert_eq!(diagnostics[0]["line"], 3);
}

#[test]
fn cli_check_json_gives_locals_and_parameters_their_own_types() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    // `label` and `values` are a module number and a module string, but
    // the function's own `label` and `values` hold other types.
    fs::write(
        temp_dir.path().join("main.cbl"),
        "label = 5\nvalues = \"none\"\n\ndef show(values):\n    first = values[0]\n    label = \"x\"\n    /say {label} {first}\n\ndef main():\n    show([1, 2])\n",
    )
    .unwrap();

    let (value, stderr, success) = check_json(temp_dir.path());

    assert!(success, "stderr: {stderr}\n{value}");
    assert_eq!(value["error_count"], 0);
}

#[test]
fn build_prints_warnings_with_their_location() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    // The warning is in the imported file.
    fs::write(
        temp_dir.path().join("main.cbl"),
        "import util\n\ndef main():\n    util.bump()\n",
    )
    .unwrap();
    fs::write(
        temp_dir.path().join("util.cbl"),
        "level = 1\n\ndef bump():\n    level = 2\n",
    )
    .unwrap();

    let output = cobble()
        .arg("build")
        .arg(temp_dir.path())
        .arg("-o")
        .arg(temp_dir.path().join("out"))
        .output()
        .unwrap();
    let (_, stderr) = output_text(&output);

    assert!(output.status.success(), "stderr: {stderr}");
    assert_eq!(
        stderr
            .matches("warning[local-shadows-module-variable]")
            .count(),
        1,
        "{stderr}"
    );
    assert!(stderr.contains("util.cbl:4:5: warning["), "{stderr}");
    assert!(stderr.contains("4 |     level = 2"), "{stderr}");
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
    // b.cbl and c.cbl both import the failing file; its error must be
    // reported exactly once.
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
