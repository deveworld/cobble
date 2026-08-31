use std::fs;
use std::path::{Path, PathBuf};

use cobble::parser::parse;
use cobble::transpiler::Transpiler;
use serde_json::Value;

fn compile_source(source: &str) -> Result<(tempfile::TempDir, PathBuf), String> {
    let temp_dir = tempfile::tempdir().map_err(|error| error.to_string())?;
    let output_dir = temp_dir.path().join("output");
    let program = parse(source).map_err(|errors| errors.join("\n"))?;

    let mut transpiler = Transpiler::new("unroll".to_string(), output_dir.clone());
    transpiler
        .transpile(&program)
        .map_err(|error| error.to_string())?;
    transpiler
        .write_data_pack()
        .map_err(|error| error.to_string())?;

    Ok((temp_dir, output_dir))
}

fn compile_error(source: &str) -> String {
    let program = parse(source).unwrap();
    let mut transpiler = Transpiler::new("unroll".to_string(), PathBuf::from("unused-output"));
    transpiler.transpile(&program).unwrap_err().to_string()
}

fn read_function(output_dir: &Path, name: &str) -> String {
    fs::read_to_string(output_dir.join(format!("data/unroll/function/{name}.mcfunction"))).unwrap()
}

fn read_first_function_with_prefix(output_dir: &Path, prefix: &str) -> String {
    let function_dir = output_dir.join("data/unroll/function");
    let entry = fs::read_dir(function_dir)
        .unwrap()
        .filter_map(Result::ok)
        .find(|entry| entry.file_name().to_string_lossy().starts_with(prefix))
        .unwrap_or_else(|| panic!("No function with prefix {prefix} generated"));
    fs::read_to_string(entry.path()).unwrap()
}

fn read_all_functions(output_dir: &Path) -> String {
    let function_dir = output_dir.join("data/unroll/function");
    let mut content = String::new();
    for entry in fs::read_dir(function_dir).unwrap().filter_map(Result::ok) {
        if entry.path().extension().and_then(|ext| ext.to_str()) == Some("mcfunction") {
            content.push_str(&fs::read_to_string(entry.path()).unwrap());
            content.push('\n');
        }
    }
    content
}

fn function_lines(output_dir: &Path, name: &str) -> Vec<String> {
    read_function(output_dir, name)
        .lines()
        .filter(|line| !line.starts_with("scoreboard players set #return_flag_"))
        .map(str::to_string)
        .collect()
}

fn assert_no_loop_helpers(output_dir: &Path) {
    let function_dir = output_dir.join("data/unroll/function");
    assert!(!fs::read_dir(function_dir)
        .unwrap()
        .filter_map(Result::ok)
        .any(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with("loop_temp_")
                || name.starts_with("loop_body_")
                || name.starts_with("loop_wrapper_")
        }));
}

#[test]
fn unrolls_range_start_stop_step_with_const_bounds() {
    let source = r#"
const STOP = 8

def test():
    for i in range(1, STOP, 3):
        /say value {i}
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    assert_eq!(
        function_lines(&output_dir, "test"),
        vec!["say value 1", "say value 4", "say value 7"]
    );
    assert_no_loop_helpers(&output_dir);
}

#[test]
fn unrolls_literal_arrays() {
    let source = r#"
def test():
    for value in ["north", "south", False]:
        /say {value}
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    assert_eq!(
        function_lines(&output_dir, "test"),
        vec!["say north", "say south", "say false"]
    );
    assert_no_loop_helpers(&output_dir);
}

#[test]
fn unrolled_commands_are_marked_in_source_map_and_manifest() {
    let source = r#"
def test():
    for i in range(2):
        /say index {i}
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let source_map: Value = serde_json::from_str(
        &fs::read_to_string(output_dir.join(".cobble/source_map.json")).unwrap(),
    )
    .unwrap();
    let entries = source_map["entries"].as_array().unwrap();
    let unrolled: Vec<_> = entries
        .iter()
        .filter(|entry| entry["kind"] == "Unrolled")
        .collect();
    assert_eq!(unrolled.len(), 2);
    assert!(unrolled
        .iter()
        .all(|entry| entry["generated_path"] == "data/unroll/function/test.mcfunction"));

    let manifest: Value = serde_json::from_str(
        &fs::read_to_string(output_dir.join(".cobble/build_manifest.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(manifest["unrolled_loops"], 1);
}

#[test]
fn rejects_identifier_iterables_without_storage_backing() {
    let error = compile_error(
        r#"
def test():
    for i in values:
        /say {i}
"#,
    );

    assert!(error.contains("unroll-non-literal"), "{error}");
}

#[test]
fn compiles_literal_arrays_with_runtime_values_as_runtime_list_loop() {
    let (_temp, output_dir) = compile_source(
        r#"
def plus_one(value):
    return value + 1

def test():
    first = 2
    total = 0
    for value in [first, plus_one(3), 5]:
        total = total + value
"#,
    )
    .unwrap();

    let content = read_function(&output_dir, "test");
    let body = read_first_function_with_prefix(&output_dir, "for_list_body_");
    let all_functions = read_all_functions(&output_dir);

    assert!(content.contains("data modify storage unroll:global _cobble_for_iter_"));
    assert!(content.contains("function unroll:for_list_temp_"));
    assert!(body.contains("function unroll:_cobble_storage_dyn_"));
    assert!(all_functions.contains("$data modify storage unroll:global vars.value set from storage unroll:global _cobble_for_iter_"));
}

#[test]
fn compiles_dynamic_range_step_as_runtime_loop() {
    let (_temp, output_dir) = compile_source(
        r#"
def test():
    limit = 10
    step = 2
    for i in range(0, limit, step):
        /say {i}
"#,
    )
    .unwrap();

    let content = read_function(&output_dir, "test");
    assert!(content.contains("#for_range_step_"));
    assert!(content.contains("function unroll:for_range_temp_"));
}

#[test]
fn rejects_unrolls_over_the_limit() {
    let error = compile_error(
        r#"
def test():
    for i in range(1025):
        /say {i}
"#,
    );

    assert!(error.contains("unroll-limit-exceeded"), "{error}");
}

#[test]
fn rejects_nested_unrolls_over_the_aggregate_limit() {
    let error = compile_error(
        r#"
def test():
    for i in range(64):
        for j in range(64):
            for k in range(64):
                /say {i} {j} {k}
"#,
    );

    assert!(error.contains("unroll-limit-exceeded"), "{error}");
    assert!(error.contains("nested unrolling"), "{error}");
}

#[test]
fn rejects_unrolled_command_output_over_the_aggregate_limit() {
    let repeated_commands = (0..65)
        .map(|index| format!("        /say command {index} {{i}}\n"))
        .collect::<String>();
    let source = format!(
        "def test():\n    for i in range(1024):\n{}",
        repeated_commands
    );
    let error = compile_error(&source);

    assert!(error.contains("unroll-limit-exceeded"), "{error}");
    assert!(error.contains("unrolling generated"), "{error}");
}

#[test]
fn rejects_bad_range_step() {
    let error = compile_error(
        r#"
def test():
    for i in range(0, 10, 0):
        /say {i}
"#,
    );

    assert!(error.contains("unroll-bad-step"), "{error}");
}
