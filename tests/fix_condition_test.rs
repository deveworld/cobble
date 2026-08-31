//! Regression tests for condition translation fixes:
//! - storage-backed non-integer variables are rejected in scoreboard conditions
//!   instead of silently compiling to score checks that are never written
//! - raw execute conditions starting with a Minecraft keyword pass through even
//!   when they contain parentheses (selector names, SNBT strings)
//! - `execute as` condition-scope objectives are pooled by (depth, slot)
//!   instead of growing by one objective per call site

use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// Compile a Cobble source and return the tempdir plus the output directory.
fn compile_source(source: &str) -> Result<(TempDir, PathBuf), String> {
    let temp_dir = TempDir::new().unwrap();
    let input_file = temp_dir.path().join("test.cbl");
    let output_dir = temp_dir.path().join("output");

    let full_source = format!("import stdlib\n{source}");
    fs::write(&input_file, &full_source).unwrap();

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
fn test_storage_string_truthiness_condition_is_rejected() {
    let source = r#"
def check():
    name = "abc"
    if name:
        /say truthy
"#;

    let error = compile_source(source).unwrap_err();
    assert!(
        error.contains("storage-backed (string)") && error.contains("'name'"),
        "expected storage-backed rejection naming the variable, got: {error}"
    );
}

#[test]
fn test_storage_string_comparisons_are_rejected() {
    let eq_source = r#"
def check():
    name = "abc"
    other = "abc"
    if name == other:
        /say eq
"#;
    let error = compile_source(eq_source).unwrap_err();
    assert!(
        error.contains("storage-backed (string)") && error.contains("'name'"),
        "expected storage-backed rejection for ==, got: {error}"
    );

    let gt_source = r#"
def check():
    name = "abc"
    if name > 3:
        /say gt
"#;
    let error = compile_source(gt_source).unwrap_err();
    assert!(
        error.contains("storage-backed (string)") && error.contains("'name'"),
        "expected storage-backed rejection for >, got: {error}"
    );
}

#[test]
fn test_storage_string_in_execute_condition_fails_build() {
    // Paren-free execute conditions are parsed via capture_raw_python_condition;
    // it must apply the same storage-backed rejection instead of emitting a
    // score check on a never-written score.
    let source = r#"
def check():
    name = "abc"
    as @a if name > 3:
        /say nope
"#;

    let error = compile_source(source).unwrap_err();
    assert!(
        error.contains("storage-backed (string)") && error.contains("'name'"),
        "expected storage-backed rejection in execute condition, got: {error}"
    );
}

#[test]
fn test_integer_score_condition_still_compiles() {
    let source = r#"
def check():
    x = 5
    if x > 3:
        /say big
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "check");
    assert!(
        content.contains("execute if score x temp matches 4.. run say big"),
        "{content}"
    );
}

#[test]
fn test_runtime_list_loop_target_condition_still_compiles() {
    // Integer list-loop targets are registered in both the storage and
    // scoreboard worlds; they must keep compiling in scoreboard conditions.
    let source = r#"
def check():
    nums = [1, 2, 3]
    for v in nums:
        if v == 2:
            /say two
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "check");
    assert!(!content.is_empty());

    // A string literal-array loop target read as a score would only expose
    // the string length, so using it in a scoreboard condition now errors.
    let source = r#"
def check():
    names = ["a", "bb", "ccc"]
    for v in names:
        if v == 2:
            /say two
"#;

    let error = compile_source(source).unwrap_err();
    assert!(
        error.contains("storage-backed (string)") && error.contains("'v'"),
        "expected storage-backed rejection for string loop target, got: {error}"
    );
}

#[test]
fn test_raw_execute_condition_with_parens_in_selector_compiles() {
    let source = r#"
def greet():
    as @a if entity @s[name="Bob (Admin)"]:
        /say hi
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "greet");
    assert!(
        content.contains(r#"execute as @a if entity @s[name="Bob (Admin)"] run say hi"#),
        "{content}"
    );
}

#[test]
fn test_python_call_condition_in_execute_still_detected() {
    let source = r#"
def ready():
    return 1

def check():
    as @a if ready():
        /say go
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "check");
    assert!(
        content.contains("execute as @a store result score @s cblc")
            && content.contains("run function cobble:ready"),
        "{content}"
    );
    assert!(
        content.contains("execute as @a if score @s cblc") && content.contains("run say go"),
        "{content}"
    );
}

#[test]
fn test_execute_as_condition_objectives_are_pooled() {
    // Four call sites, one with two holders: the objective pool must be
    // bounded by the maximum number of concurrently live holders (2), not by
    // the number of call sites (5 objectives before the fix).
    let source = r#"
def ready():
    return 1

def tick():
    execute as @a if ready():
        /say a
    execute as @a if ready():
        /say b
    execute as @a if ready():
        /say c
    execute as @a if ready() and ready():
        /say d
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let init = read_function(&output_dir, "_cobble_init");
    let cblc_objectives: Vec<&str> = init
        .lines()
        .filter(|line| line.starts_with("scoreboard objectives add cblc"))
        .collect();
    assert_eq!(
        cblc_objectives.len(),
        2,
        "expected the (depth, slot) pool to register exactly 2 objectives, got: {init}"
    );
    assert!(
        init.contains("scoreboard objectives add cblc0_0 dummy")
            && init.contains("scoreboard objectives add cblc0_1 dummy"),
        "{init}"
    );

    // Every site must store its holder before reading it, per entity via @s.
    let tick = read_function(&output_dir, "tick");
    assert_eq!(
        tick.matches("store result score @s cblc0_0").count(),
        4,
        "{tick}"
    );
    assert_eq!(
        tick.matches("store result score @s cblc0_1").count(),
        1,
        "{tick}"
    );
    assert!(
        tick.contains("if score @s cblc0_0 matches 1.. if score @s cblc0_1 matches 1.. run say d"),
        "{tick}"
    );
}

#[test]
fn test_paren_free_python_comparison_in_execute_still_translates() {
    // Pins the paren-free Python condition path (AST ConditionTranslator): a paren-free Python
    // comparison in an execute condition is not handled by
    // capture_raw_python_condition (which requires parentheses) and must keep
    // translating through try_translate_python_expression.
    let source = r#"
def check():
    x = 10
    as @a if x > 5:
        /say big
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "check");
    assert!(
        content.contains("execute as @a if score x temp matches 6.. run say big"),
        "{content}"
    );
}

#[test]
fn test_or_condition_in_execute_still_works() {
    let source = r#"
def check():
    x = 1
    as @a if x == 1 or x == 2:
        /say either
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "check");
    assert!(
        content.contains("execute as @a run scoreboard players set @s temp 0"),
        "OR flag should reset per executor: {content}"
    );
    assert!(
        content.contains("if score x temp matches 1 run scoreboard players set @s temp 1")
            && content.contains("if score x temp matches 2 run scoreboard players set @s temp 1"),
        "{content}"
    );
    assert!(
        content.contains("execute as @a if score @s temp matches 1 run say either"),
        "{content}"
    );
}

#[test]
fn test_failed_condition_error_restores_masked_operand_text() {
    // Selector operands are masked with synthetic identifiers before the raw
    // condition is parsed as a Python expression. When translation then
    // fails, the error must name the original operand (`@s`) instead of
    // leaking the `__cobble_cond_operand_N__` placeholder.
    let source = r#"
def check():
    x = 1
    as @a if x and [@s]:
        /say hi
"#;

    let error = compile_source(source).unwrap_err();
    assert!(
        !error.contains("__cobble_cond_operand_"),
        "error must not leak masked operand placeholders: {error}"
    );
    assert!(
        error.contains(r#"Identifier("@s")"#),
        "error should show the original operand spelling: {error}"
    );
}
