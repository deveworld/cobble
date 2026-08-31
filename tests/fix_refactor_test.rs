//! Regression tests for the transpiler refactor:
//! - paren-free Python execute conditions are handled by the AST condition
//!   translator (the legacy string-based translator is retired) and compile
//!   identically to their parenthesized forms
//! - selector and macro-parameter operands in execute conditions still work
//!   through operand masking
//! - storage-backed non-integer rejection still fires inside execute blocks
//! - the consolidated storage-path machinery emits stable dynamic-path
//!   commands

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

fn read_all_functions(output_dir: &Path) -> String {
    let function_dir = output_dir.join("data/cobble/function");
    let mut combined = String::new();
    let mut entries: Vec<_> = fs::read_dir(&function_dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {}", function_dir.display(), e))
        .map(|entry| entry.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        if path.extension().is_some_and(|ext| ext == "mcfunction") {
            combined.push_str(&fs::read_to_string(&path).unwrap());
            combined.push('\n');
        }
    }
    combined
}

#[test]
fn test_paren_free_and_condition_matches_parenthesized_form() {
    let paren_free = r#"
def check():
    x = 10
    y = 1
    as @a if x > 5 and y < 2:
        /say both
"#;
    let parenthesized = r#"
def check():
    x = 10
    y = 1
    as @a if (x > 5 and y < 2):
        /say both
"#;

    let (_t1, out1) = compile_source(paren_free).unwrap();
    let (_t2, out2) = compile_source(parenthesized).unwrap();
    let free = read_function(&out1, "check");
    let parens = read_function(&out2, "check");
    assert_eq!(
        free, parens,
        "paren-free and parenthesized forms must match"
    );
    assert!(
        free.contains(
            "execute as @a if score x temp matches 6.. if score y temp matches ..1 run say both"
        ),
        "{free}"
    );
}

#[test]
fn test_paren_free_or_condition_matches_parenthesized_form() {
    let paren_free = r#"
def check():
    x = 1
    y = 2
    as @a if x == 1 or y == 2:
        /say either
"#;
    let parenthesized = r#"
def check():
    x = 1
    y = 2
    as @a if (x == 1 or y == 2):
        /say either
"#;

    let (_t1, out1) = compile_source(paren_free).unwrap();
    let (_t2, out2) = compile_source(parenthesized).unwrap();
    let free = read_function(&out1, "check");
    let parens = read_function(&out2, "check");
    assert_eq!(
        free, parens,
        "paren-free and parenthesized forms must match"
    );
    assert!(
        free.contains("if score x temp matches 1 run scoreboard players set @s temp 1")
            && free.contains("if score y temp matches 2 run scoreboard players set @s temp 1")
            && free.contains("execute as @a if score @s temp matches 1 run say either"),
        "{free}"
    );
}

#[test]
fn test_paren_free_not_condition_translates() {
    let paren_free = r#"
def check():
    done = 0
    as @a if not done:
        /say idle
"#;
    let parenthesized = r#"
def check():
    done = 0
    as @a if (not done):
        /say idle
"#;

    let (_t1, out1) = compile_source(paren_free).unwrap();
    let (_t2, out2) = compile_source(parenthesized).unwrap();
    let free = read_function(&out1, "check");
    let parens = read_function(&out2, "check");
    assert_eq!(
        free, parens,
        "paren-free and parenthesized forms must match"
    );
    assert!(
        free.contains("execute as @a unless score done temp matches 1.. run say idle"),
        "{free}"
    );
}

#[test]
fn test_paren_free_not_equal_condition_matches_parenthesized_form() {
    let paren_free = r#"
def check():
    x = 0
    as @a if x != 3:
        /say differs
"#;
    let parenthesized = r#"
def check():
    x = 0
    as @a if (x != 3):
        /say differs
"#;

    let (_t1, out1) = compile_source(paren_free).unwrap();
    let (_t2, out2) = compile_source(parenthesized).unwrap();
    let free = read_function(&out1, "check");
    let parens = read_function(&out2, "check");
    assert_eq!(
        free, parens,
        "paren-free and parenthesized forms must match"
    );
    assert!(
        free.contains("execute as @a unless score x temp matches 3 run say differs"),
        "{free}"
    );
}

#[test]
fn test_selector_operand_condition_still_translates() {
    // `@s` cannot be parsed as a Python identifier; the execute path masks
    // selector operands before parsing and restores them afterwards.
    let source = r#"
def check():
    as @a if @s > 5:
        /say high score
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "check");
    assert!(
        content.contains("execute as @a if score @s temp matches 6.. run say high score"),
        "{content}"
    );
}

#[test]
fn test_storage_string_condition_rejected_in_execute_block() {
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

    let unless_source = r#"
def check():
    name = "abc"
    as @a unless name > 3:
        /say nope
"#;

    let error = compile_source(unless_source).unwrap_err();
    assert!(
        error.contains("storage-backed (string)") && error.contains("'name'"),
        "expected storage-backed rejection in unless condition, got: {error}"
    );
}

#[test]
fn test_storage_path_plan_commands_are_stable() {
    // Golden-ish pin on the consolidated storage-path machinery: static
    // field writes, dynamic map keys from parameters, and the macro helper
    // read must keep their exact command shapes.
    let source = r#"
def lookup(key):
    config = {"chance": 7, "other": 2}
    selected = config[key]
    config["chance"] = 9
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "lookup");
    let all_functions = read_all_functions(&output_dir);

    assert!(
        content.contains("data modify storage cobble:global vars.config.chance set value 9"),
        "{content}"
    );
    assert!(
        content.contains("data modify storage cobble:global _cobble_dyn_")
            && content.contains(".key0 set from storage cobble:global args.key"),
        "{content}"
    );
    assert!(
        all_functions.contains(
            "$execute store result score selected temp run data get storage cobble:global vars.config.$(key0)"
        ),
        "{all_functions}"
    );
}
