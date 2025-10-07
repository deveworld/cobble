// Additional tests for negative step loops and macro syntax
// These tests verify the bug fixes for:
// - Bug #1: Negative-step range() initialization
// - Bug #4: Macro $ prefix detection

use std::fs;
use std::path::PathBuf;

// Helper functions from integration_test.rs
fn compile_source(source: &str) -> Result<(tempfile::TempDir, PathBuf), String> {
    use cobble::parser::parse;
    use cobble::transpiler::Transpiler;

    let temp_dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let output_dir = temp_dir.path().join("output");

    let program = parse(source).map_err(|errors| errors.join("\n"))?;

    let mut transpiler = Transpiler::new("cobble".to_string(), output_dir.clone());
    transpiler.transpile(&program)?;
    transpiler.write_data_pack().map_err(|e| e.to_string())?;

    Ok((temp_dir, output_dir))
}

fn read_function(output_dir: &PathBuf, name: &str) -> String {
    let path = output_dir.join(format!("data/cobble/functions/{}.mcfunction", name));
    fs::read_to_string(path).unwrap()
}

#[test]
fn test_for_loop_negative_step_minus_two() {
    let source = r#"
def test():
    for i in range(10) by -2:
        /say Value: {i}
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    // Bug fix: Negative step loops should start at count - 1 (not count + step)
    // range(10) by -2 should start at 9 (not 8)
    assert!(content.contains("scoreboard players set i loop_counter 9"),
            "range(10) by -2 should start at 9 (count - 1), not 8 (count + step)");

    // Check loop control function exists and uses correct decrement
    let loop_files: Vec<_> = fs::read_dir(output_dir.join("data/cobble/functions"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("loop_temp_"))
        .collect();

    assert!(!loop_files.is_empty(), "Loop temp functions should exist");

    let loop_content = fs::read_to_string(loop_files[0].path()).unwrap();
    assert!(loop_content.contains("scoreboard players remove i loop_counter 2"),
            "Loop should decrement by 2");
    assert!(loop_content.contains("matches 0.."),
            "Loop should continue while i >= 0");
}

#[test]
fn test_for_loop_negative_step_minus_three() {
    let source = r#"
def test():
    for i in range(10) by -3:
        /say Value: {i}
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    // Bug fix: range(10) by -3 should start at 9 (not 7)
    // Expected iterations: 9, 6, 3, 0 (4 times)
    // Previously buggy: 7, 4, 1 (3 times)
    assert!(content.contains("scoreboard players set i loop_counter 9"),
            "range(10) by -3 should start at 9 (count - 1), not 7 (count + step)");

    let loop_files: Vec<_> = fs::read_dir(output_dir.join("data/cobble/functions"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("loop_temp_"))
        .collect();

    let loop_content = fs::read_to_string(loop_files[0].path()).unwrap();
    assert!(loop_content.contains("scoreboard players remove i loop_counter 3"),
            "Loop should decrement by 3");
}

#[test]
fn test_for_loop_negative_step_minus_five() {
    let source = r#"
def test():
    for i in range(20) by -5:
        /say Value: {i}
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    // range(20) by -5 should start at 19 (not 15)
    // Expected iterations: 19, 14, 9, 4 (stops before -1)
    assert!(content.contains("scoreboard players set i loop_counter 19"),
            "range(20) by -5 should start at 19 (count - 1), not 15 (count + step)");
}

#[test]
fn test_macro_dollar_syntax_direct() {
    let source = r#"
def test(player):
    /give $(player) diamond 1
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    // Bug fix: Commands with $(param) should have $ line prefix
    assert!(content.starts_with("$give"),
            "Macro function with $(param) should have $ line prefix");
    assert!(content.contains("$(player)"),
            "Parameter should remain as $(player)");
}

#[test]
fn test_macro_mixed_syntax() {
    let source = r#"
def test(player, item):
    /give {player} {item} 1
    /tellraw $(player) {"text":"Given item"}
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    // Both lines should have $ prefix
    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines.len(), 2, "Should have exactly 2 lines");
    assert!(lines[0].starts_with("$give"), "First line should have $ prefix");
    assert!(lines[1].starts_with("$tellraw"), "Second line should have $ prefix");
}
