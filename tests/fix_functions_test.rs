//! Regression tests for user-function and statement-processor fixes:
//! - recursion (direct and mutual) is rejected with a clear error
//! - functions without `return` skip the return-flag prologue and
//!   `_cobble_init`
//! - a single multi-command statement in an `if` branch routes through a
//!   helper function so the condition is evaluated exactly once
//! - `match` on a storage-backed string scrutinee errors instead of
//!   silently miscompiling
//! - a string literal-array loop target used in arithmetic errors
//! - an unknown identifier passed as a call argument errors (library path)

use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// Compile a Cobble source through the build pipeline (diagnostics included).
fn compile_source(source: &str) -> Result<(TempDir, PathBuf), String> {
    let temp_dir = TempDir::new().unwrap();
    let input_file = temp_dir.path().join("test.cbl");
    let output_dir = temp_dir.path().join("output");

    fs::write(&input_file, source).unwrap();

    cobble::commands::build::build(cobble::commands::build::BuildOptions {
        input: Some(input_file),
        output: Some(output_dir.clone()),
        namespace: None,
        pack_format: None,
        description: None,
        verbose: false,
        quiet: false,
        zip: false,
        experimental_resource_pack: false,
        validate: false,
        dry_run: false,
        commands_json: PathBuf::from("data/commands.json"),
    })?;

    Ok((temp_dir, output_dir))
}

fn read_function(output_dir: &Path, function_name: &str) -> String {
    let function_path = output_dir
        .join("data/cobble/function")
        .join(format!("{}.mcfunction", function_name));
    fs::read_to_string(&function_path)
        .unwrap_or_else(|e| panic!("cannot read {}: {}", function_path.display(), e))
}

#[test]
fn direct_recursion_is_rejected() {
    let error = compile_source(
        r#"
def g(n):
    if n <= 0:
        return 0
    g(n - 1)
    return n

def main():
    x = g(2)
"#,
    )
    .unwrap_err();

    assert!(
        error.contains("Recursive call to 'g' is not supported"),
        "unexpected error: {error}"
    );
    assert!(error.contains("Rewrite the recursion as a loop"));
}

#[test]
fn mutual_recursion_cycle_is_rejected() {
    let error = compile_source(
        r#"
def ping(n):
    pong(n)

def pong(n):
    ping(n)

def main():
    ping(3)
"#,
    )
    .unwrap_err();

    assert!(
        error.contains("Recursive function calls are not supported"),
        "unexpected error: {error}"
    );
    assert!(
        error.contains("ping -> pong -> ping") || error.contains("pong -> ping -> pong"),
        "cycle path missing from error: {error}"
    );
}

#[test]
fn pass_only_function_has_no_prologue_and_no_init() {
    let (_temp, output_dir) = compile_source(
        r#"
def noop():
    pass
"#,
    )
    .unwrap();

    let body = read_function(&output_dir, "noop");
    assert!(
        !body.contains("#return_flag_"),
        "pass-only function must not carry the return-flag prologue:\n{body}"
    );
    assert_eq!(body.trim(), "", "pass-only function body should be empty");
    assert!(
        !output_dir
            .join("data/cobble/function/_cobble_init.mcfunction")
            .exists(),
        "pass-only pack must not generate _cobble_init"
    );
}

#[test]
fn returning_function_keeps_prologue_but_plain_sibling_skips_it() {
    let (_temp, output_dir) = compile_source(
        r#"
def noop():
    pass

def chooser():
    return 1
"#,
    )
    .unwrap();

    let noop = read_function(&output_dir, "noop");
    assert!(
        !noop.contains("#return_flag_"),
        "no-return function must not carry the return-flag prologue:\n{noop}"
    );

    let chooser = read_function(&output_dir, "chooser");
    assert!(
        chooser.contains("scoreboard players set #return_flag_"),
        "returning function keeps the return-flag prologue:\n{chooser}"
    );
}

#[test]
fn multi_command_single_statement_if_branch_routes_through_helper() {
    let (_temp, output_dir) = compile_source(
        r#"
def tick():
    x = 2
    if x > 1:
        as @a at @s:
            x = 0
            /say a2
"#,
    )
    .unwrap();

    let tick = read_function(&output_dir, "tick");
    let condition_lines = tick
        .lines()
        .filter(|line| line.starts_with("execute if score x temp"))
        .count();
    assert_eq!(
        condition_lines, 1,
        "the if condition must be evaluated exactly once:\n{tick}"
    );

    let helper_line = tick
        .lines()
        .find(|line| line.contains("run function cobble:if_temp_"))
        .unwrap_or_else(|| panic!("expected helper routing in tick:\n{tick}"));
    let helper_name = helper_line
        .rsplit_once(':')
        .map(|(_, name)| name.trim())
        .unwrap();
    let helper = read_function(&output_dir, helper_name);
    assert!(
        helper.contains("say a2"),
        "helper must contain the branch commands:\n{helper}"
    );
    assert!(
        helper.contains("scoreboard players set x temp 0"),
        "helper must contain the branch commands:\n{helper}"
    );
}

#[test]
fn match_on_storage_string_scrutinee_errors() {
    let error = compile_source(
        r#"
def main():
    name = "a"
    match name:
        case 1:
            /say one
        case _:
            /say other
"#,
    )
    .unwrap_err();

    assert!(
        error.contains("Cannot match on 'name'"),
        "unexpected error: {error}"
    );
    assert!(error.contains("match only supports integer scoreboard values"));
}

#[test]
fn string_list_loop_target_arithmetic_errors() {
    let error = compile_source(
        r#"
def main():
    names = ["a", "bb", "ccc"]
    total = 0
    for v in names:
        total = total + v
"#,
    )
    .unwrap_err();

    assert!(
        error.contains("Variable 'v' holds a storage-backed string"),
        "unexpected error: {error}"
    );

    // Integer literal-array loops keep compiling.
    compile_source(
        r#"
def main():
    nums = [1, 2, 3]
    total = 0
    for w in nums:
        total = total + w
"#,
    )
    .expect("integer list loop must keep compiling");
}

#[test]
fn unknown_identifier_call_argument_errors_in_library_path() {
    // Bypass the CLI text diagnostics (which catch this earlier as
    // `undefined-variable`) and hit the transpiler directly, the way a
    // library consumer would.
    let program =
        cobble::parser::parse("def greet(who):\n    /say hi\n\ndef main():\n    greet(stranger)\n")
            .map_err(|errors| errors.join("\n"))
            .expect("parse succeeds");

    let temp_dir = TempDir::new().unwrap();
    let mut transpiler =
        cobble::transpiler::Transpiler::new("cobble".to_string(), temp_dir.path().join("output"));
    let error = transpiler.transpile(&program).unwrap_err();

    assert!(
        error.contains("Unknown identifier 'stranger' passed as argument to function 'greet'"),
        "unexpected error: {error}"
    );
    assert!(error.contains("quote it (\"stranger\")"));
}
