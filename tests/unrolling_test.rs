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
    transpiler.transpile(&program)?;
    transpiler
        .write_data_pack()
        .map_err(|error| error.to_string())?;

    Ok((temp_dir, output_dir))
}

fn compile_error(source: &str) -> String {
    let program = parse(source).unwrap();
    let mut transpiler = Transpiler::new("unroll".to_string(), PathBuf::from("unused-output"));
    transpiler.transpile(&program).unwrap_err()
}

fn read_function(output_dir: &Path, name: &str) -> String {
    fs::read_to_string(output_dir.join(format!("data/unroll/function/{name}.mcfunction"))).unwrap()
}

fn function_lines(output_dir: &Path, name: &str) -> Vec<String> {
    read_function(output_dir, name)
        .lines()
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
fn rejects_non_literal_iterables() {
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
