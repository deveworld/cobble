// Additional tests for negative step loops and macro syntax
// These tests verify the bug fixes for:
// - Bug #1: Negative-step range() initialization
// - Bug #4: Macro $ prefix detection

use std::fs;
use std::path::{Path, PathBuf};

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

fn read_function(output_dir: &Path, name: &str) -> String {
    let path = output_dir.join(format!("data/cobble/function/{}.mcfunction", name));
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

    assert!(content.contains("say Value: 9"), "{content}");
    assert!(content.contains("say Value: 7"), "{content}");
    assert!(content.contains("say Value: 5"), "{content}");
    assert!(content.contains("say Value: 3"), "{content}");
    assert!(content.contains("say Value: 1"), "{content}");
    assert!(!content.contains("say Value: 8"), "{content}");
    assert!(!content.contains("loop_counter"), "{content}");
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

    assert!(content.contains("say Value: 9"), "{content}");
    assert!(content.contains("say Value: 6"), "{content}");
    assert!(content.contains("say Value: 3"), "{content}");
    assert!(content.contains("say Value: 0"), "{content}");
    assert!(!content.contains("say Value: 7"), "{content}");
    assert!(!content.contains("loop_counter"), "{content}");
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

    assert!(content.contains("say Value: 19"), "{content}");
    assert!(content.contains("say Value: 14"), "{content}");
    assert!(content.contains("say Value: 9"), "{content}");
    assert!(content.contains("say Value: 4"), "{content}");
    assert!(!content.contains("say Value: 15"), "{content}");
    assert!(!content.contains("loop_counter"), "{content}");
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
    assert!(
        content.starts_with("$give"),
        "Macro function with $(param) should have $ line prefix"
    );
    assert!(
        content.contains("$(player)"),
        "Parameter should remain as $(player)"
    );
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
    assert!(
        lines[0].starts_with("$give"),
        "First line should have $ prefix"
    );
    assert!(
        lines[1].starts_with("$tellraw"),
        "Second line should have $ prefix"
    );
}
