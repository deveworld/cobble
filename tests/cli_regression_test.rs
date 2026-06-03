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
fn cli_check_reports_language_diagnostic_location() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
def reward(player, amount=1):
    /say reward
"#,
    );

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stdout.contains("Checking 1 file"));
    assert!(stderr.contains("unsupported-function-parameter"));
    assert!(stderr.contains("Default parameter values are not supported"));
    assert!(stderr.contains("Use explicit arguments at each call site"));
    assert!(stderr.contains("main.cbl:2:"));
}

#[test]
fn cli_check_reports_structural_syntax_diagnostic_location() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
def main():
    value = (1 + 2
"#,
    );

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("unclosed-delimiter"));
    assert!(stderr.contains("Opening delimiter `(` is not closed"));
    assert!(stderr.contains("main.cbl:3:13"));
}

#[test]
fn cli_check_reports_duplicate_function_parameters() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
def greet(player, player):
    /say duplicate
"#,
    );

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("duplicate-function-parameter"));
    assert!(stderr.contains("Duplicate function parameter `player`"));
    assert!(stderr.contains("main.cbl:2:19"));
}

#[test]
fn cli_build_rejects_language_diagnostic_before_output() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
def main():
    score += 1
"#,
    );
    let output_dir = temp_dir.path().join("output");

    let output = cobble()
        .arg("build")
        .arg(&input)
        .arg("--output")
        .arg(&output_dir)
        .output()
        .unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("Language diagnostics failed"));
    assert!(stderr.contains("unsupported-assignment"));
    assert!(stderr.contains("Compound assignment `+=` is not supported"));
    assert!(stderr.contains("3 |     score += 1"));
    assert!(stderr.contains("^"));
    assert!(!output_dir.exists());
}

#[test]
fn cli_check_rejects_for_else_without_rejecting_if_else() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
def main():
    if True:
        pass
    else:
        pass
    for i in range(3):
        pass
    else:
        pass
"#,
    );

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert_eq!(stderr.matches("unsupported-control-flow").count(), 1);
    assert!(stderr.contains("`for ... else` blocks are not supported"));
}

#[test]
fn cli_check_reports_semantic_preflight_diagnostics() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
def helper():
    pass

def helper():
    return

def main():
    result = helper()
"#,
    );

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("duplicate-function"));
    assert!(stderr.contains("unsupported-return"));
    assert!(stderr.contains("unsupported-function-call-expression"));
    assert!(stderr.contains("Minecraft functions cannot return early"));
}

#[test]
fn cli_check_reports_undefined_variable_diagnostics() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
def main():
    total = missing_score + 1
"#,
    );

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("undefined-variable"));
    assert!(stderr.contains("Undefined variable `missing_score`"));
    assert!(stderr.contains("Define or import `missing_score`"));
}

#[test]
fn cli_check_json_reports_success_summary() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(temp_dir.path(), "def main():\n    /say ok\n");

    let output = cobble()
        .arg("check")
        .arg("--json")
        .arg(&input)
        .output()
        .unwrap();

    let (stdout, stderr) = output_text(&output);
    assert!(
        output.status.success(),
        "check --json failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    let value: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["ok"], true);
    assert_eq!(value["files_checked"], 1);
    assert_eq!(value["error_count"], 0);
    assert_eq!(value["diagnostics"].as_array().unwrap().len(), 0);
    assert_eq!(value["files"][0]["functions"], 1);
}

#[test]
fn cli_check_json_reports_structured_diagnostics() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
def main():
    total = missing_score + 1
"#,
    );

    let output = cobble()
        .arg("check")
        .arg("--json")
        .arg(&input)
        .output()
        .unwrap();

    let (stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("Validation failed with 1 error"));
    let value: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["ok"], false);
    assert_eq!(value["files_checked"], 1);
    assert_eq!(value["error_count"], 1);
    assert_eq!(value["diagnostics"][0]["kind"], "undefined-variable");
    assert_eq!(value["diagnostics"][0]["line"], 3);
    assert_eq!(value["diagnostics"][0]["column"], 13);
    assert!(value["diagnostics"][0]["formatted"]
        .as_str()
        .unwrap()
        .contains("missing_score"));
}

#[test]
fn cli_check_reports_undefined_standalone_call_arguments() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
def main():
    score.set("points", missing_score)
"#,
    );

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("undefined-variable"));
    assert!(stderr.contains("Undefined variable `missing_score`"));
}

#[test]
fn cli_check_reports_unsupported_none_usage() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
def main():
    storage.set("state", {"note": None})
"#,
    );

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("unsupported-none"));
    assert!(stderr.contains("None/null is only supported in data pack JSON resource helper values"));
}

#[test]
fn cli_check_rejects_lowercase_null_in_json_resources() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
datapack.predicate("maybe", {"condition": "minecraft:random_chance", "chance": null})
"#,
    );

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("unsupported-none"));
    assert!(stderr.contains("None/null is only supported in data pack JSON resource helper values"));
}

#[test]
fn cli_check_reports_storage_access_and_type_mismatch() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
def main():
    items = [1, 2, 3]
    first = items[i]
    value = 1
    value = "one"
"#,
    );

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("unsupported-storage-access"));
    assert!(stderr.contains("type-mismatch"));
}

#[test]
fn cli_check_reports_unsupported_storage_access() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
def main():
    items = [1, 2, 3]
    first = items[i]
"#,
    );

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("unsupported-storage-access"));
    assert!(stderr.contains("Dynamic storage-backed subscript indexes are not supported"));
}

#[test]
fn cli_check_reports_noop_expression_diagnostics() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
def main():
    score + 1
"#,
    );

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("no-op-expression"));
    assert!(stderr.contains("Standalone expression does not generate Minecraft commands"));
    assert!(stderr.contains("main.cbl:3:5"));
}

#[test]
fn cli_check_reports_type_mismatch_diagnostics() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
def main():
    items = ["sword"]
    items = 3
"#,
    );

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("type-mismatch"));
    assert!(stderr.contains("Type mismatch for variable 'items'"));
    assert!(stderr.contains("previously defined as type: list"));
    assert!(stderr.contains("Cannot reassign to type: integer"));
}

#[test]
fn cli_check_reports_datapack_resource_id_diagnostics() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
datapack.function_tag("minecraft/load", ["cli_regression:setup"])
datapack.item_tag("rewards", ["minecraft/diamond"])
"#,
    );

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("datapack-resource-id"));
    assert!(stderr.contains("Use 'minecraft:load' instead"));
    assert!(stderr.contains("Invalid tag value"));
    assert!(stderr.contains("minecraft:diamond"));
}

#[test]
fn cli_check_reports_multiline_datapack_tag_diagnostics() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
datapack.item_tag(
    "rewards",
    ["minecraft/diamond", 1],
)
"#,
    );

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("datapack-resource-id"));
    assert!(stderr.contains("minecraft:diamond"));
    assert!(stderr.contains("Tag values must be string resource IDs"));
}

#[test]
fn cli_check_reports_user_function_argument_count() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
def main():
    greet("@a")

def greet(player, message):
    /tellraw {player} {"text":"{message}"}
"#,
    );

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("function-argument-count"));
    assert!(stderr.contains("Function `greet` expects 2 argument(s), but 1 provided"));
    assert!(stderr.contains("Expected parameters: (player, message)"));
}

#[test]
fn cli_check_reports_undefined_user_function_calls() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
def main():
    missing("x")
"#,
    );

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("undefined-function"));
    assert!(stderr.contains("Undefined function `missing`"));
}

#[test]
fn cli_check_reports_unknown_dotted_helper_calls() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
def main():
    helper.do()
"#,
    );

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("undefined-function"));
    assert!(stderr.contains("Unknown helper function `helper.do`"));
}

#[test]
fn cli_check_reports_nested_function_call_arguments() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
def main():
    greet(make_name())

def make_name():
    pass

def greet(name):
    /say {name}
"#,
    );

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("unsupported-function-call-argument"));
    assert!(stderr.contains("Function `greet` arguments cannot contain function call expressions"));
}

#[test]
fn cli_check_reports_undefined_command_placeholders() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
def main(player):
    /tellraw {player} {"text":"{message}"}
"#,
    );

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("undefined-placeholder"));
    assert!(stderr.contains("Undefined command placeholder `message`"));
    assert!(stderr.contains("main.cbl:3:"));
}

#[test]
fn cli_check_reports_forward_command_placeholders() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
def main():
    /say {message}
    message = "hi"
"#,
    );

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("undefined-placeholder"));
    assert!(stderr.contains("Undefined command placeholder `message`"));
}

#[test]
fn cli_check_reports_invalid_command_placeholders() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
def main():
    /say {bad-name}
"#,
    );

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("invalid-placeholder"));
    assert!(stderr.contains("Invalid command placeholder `bad-name`"));
}

#[test]
fn cli_check_rejects_imported_function_command_placeholders() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
from helper import greet

def main():
    /say {greet}
"#,
    );
    fs::write(
        temp_dir.path().join("helper.cbl"),
        "def greet():\n    /say hi\n",
    )
    .unwrap();

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("unsupported-placeholder-symbol"));
    assert!(stderr.contains("Imported function `greet` cannot be used as a command placeholder"));
}

#[test]
fn cli_check_allows_imported_command_placeholders() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
from helper import imported_score

def main():
    /say {imported_score}
"#,
    );
    fs::write(
        temp_dir.path().join("helper.cbl"),
        "imported_score = 1\n\ndef helper():\n    pass\n",
    )
    .unwrap();

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (stdout, stderr) = output_text(&output);
    assert!(
        output.status.success(),
        "check failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(!stderr.contains("undefined-placeholder"));
}

#[test]
fn cli_check_reports_missing_import_with_importing_file() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(temp_dir.path(), "import missing\n\ndef main():\n    pass\n");

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("missing-import"));
    assert!(stderr.contains("Cannot import 'missing'"));
    assert!(stderr.contains(input.to_string_lossy().as_ref()));
}

#[test]
fn cli_check_reports_missing_from_import_item() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        "from helper import greet, missing\n\ndef main():\n    pass\n",
    );
    fs::write(
        temp_dir.path().join("helper.cbl"),
        "def greet():\n    /say hi\n",
    )
    .unwrap();

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("missing-import-item"));
    assert!(stderr.contains("Cannot import `missing` from `helper`"));
    assert!(stderr.contains("Available symbols: greet"));
    assert!(stderr.contains("main.cbl:1:27"));
}

#[test]
fn cli_check_reports_cross_file_duplicate_functions() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        "import helper\n\ndef greet():\n    /say from main\n",
    );
    fs::write(
        temp_dir.path().join("helper.cbl"),
        "def greet():\n    /say from helper\n",
    )
    .unwrap();

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("duplicate-function"));
    assert!(stderr.contains("Duplicate function definition `greet` across imported files"));
    assert!(stderr.contains("helper.cbl"));
}

#[test]
fn cli_check_reports_directory_duplicate_functions() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    fs::write(
        temp_dir.path().join("first.cbl"),
        "def same():\n    /say first\n",
    )
    .unwrap();
    fs::write(
        temp_dir.path().join("second.cbl"),
        "def same():\n    /say second\n",
    )
    .unwrap();

    let output = cobble().arg("check").arg(temp_dir.path()).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("duplicate-function"));
    assert!(stderr.contains("Duplicate function definition `same` across imported files"));
    assert!(stderr.contains("first.cbl"));
    assert!(stderr.contains("second.cbl"));
}

#[test]
fn cli_check_honors_configured_entry_points_without_explicit_input() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let src_dir = temp_dir.path().join("src");
    fs::create_dir_all(&src_dir).unwrap();
    fs::write(
        temp_dir.path().join("cobble.toml"),
        r#"
[project]
name = "entry_points"
description = "entry point check parity"
namespace = "entry_points"
version = "1.0.0"
pack_format = "101.1"

[build]
source = "src"
output = "output"
entry_points = ["main.cbl"]
"#,
    )
    .unwrap();
    fs::write(src_dir.join("main.cbl"), "def main():\n    /say selected\n").unwrap();
    fs::write(
        src_dir.join("unused.cbl"),
        "def unused(value=1):\n    /say should not be checked\n",
    )
    .unwrap();

    let output = cobble()
        .current_dir(temp_dir.path())
        .arg("check")
        .output()
        .unwrap();

    let (stdout, stderr) = output_text(&output);
    assert!(
        output.status.success(),
        "check failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(stdout.contains("Checking 1 file"));
    assert!(!stderr.contains("unsupported-function-parameter"));
}

#[test]
fn cli_build_reports_directory_duplicate_functions_before_output() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    fs::write(
        temp_dir.path().join("first.cbl"),
        "def same():\n    /say first\n",
    )
    .unwrap();
    fs::write(
        temp_dir.path().join("second.cbl"),
        "def same():\n    /say second\n",
    )
    .unwrap();
    let output_dir = temp_dir.path().join("output");

    let output = cobble()
        .arg("build")
        .arg(temp_dir.path())
        .arg("--namespace")
        .arg("cli_regression")
        .arg("--output")
        .arg(&output_dir)
        .arg("--dry-run")
        .output()
        .unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("Language diagnostics failed"));
    assert!(stderr.contains("duplicate-function"));
    assert!(stderr.contains("Duplicate function definition `same` across imported files"));
    assert!(!output_dir.exists());
}

#[test]
fn cli_check_reports_cross_file_duplicate_selector_aliases() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        "import helper\n\n@Players = @a\n\ndef main():\n    pass\n",
    );
    fs::write(
        temp_dir.path().join("helper.cbl"),
        "@Players = @p\n\ndef helper():\n    pass\n",
    )
    .unwrap();

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("duplicate-symbol"));
    assert!(stderr.contains("Duplicate selector alias `@Players` across imported files"));
    assert!(stderr.contains("helper.cbl"));
}

#[test]
fn cli_check_reports_imported_function_argument_count() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        "import helper\n\ndef main():\n    greet(\"@a\")\n",
    );
    fs::write(
        temp_dir.path().join("helper.cbl"),
        "def greet(player, message):\n    /tellraw {player} {\"text\":\"{message}\"}\n",
    )
    .unwrap();

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("function-argument-count"));
    assert!(stderr.contains("Function `greet` expects 2 argument(s), but 1 provided"));
    assert!(stderr.contains("helper.cbl"));
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
