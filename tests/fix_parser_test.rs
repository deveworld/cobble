//! Regression tests for parser/command-processor fixes:
//! - `case 1.5:` produced a compiler panic instead of a parse error
//! - `exec_condition` re-stringification broke dotted paths (`path.sub`)
//!   and rendered SNBT booleans capitalized (`{x:True}`)
//! - a stray closing delimiter drove the tokenizer's paren depth negative
//!   and suppressed Newline emission for the rest of the file

use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// Helper function to compile a Cobble source and return the generated functions
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

/// Helper to read a function file
fn read_function(output_dir: &Path, function_name: &str) -> String {
    let function_path = output_dir
        .join("data/cobble/function")
        .join(format!("{}.mcfunction", function_name));
    fs::read_to_string(function_path).unwrap()
}

#[test]
fn case_float_pattern_reports_error_instead_of_panicking() {
    // Previously: `n.parse::<i32>().unwrap()` panicked with ParseIntError.
    let result = compile_source(
        "def tick():\n    x = 1\n    match x:\n        case 1.5:\n            /say a\n        case _:\n            /say b\n",
    );
    let error = result.expect_err("non-integer case pattern must fail the build, not panic");
    assert!(
        error.contains("match case patterns must be integers"),
        "expected the integer-pattern diagnostic, got: {error}"
    );
}

#[test]
fn case_float_pattern_parser_api_reports_error() {
    let source = "match x:\n    case 1.5:\n        pass\n";
    let errors = cobble::parser::parse(source).expect_err("parse must fail");
    assert!(
        errors
            .iter()
            .any(|error| error.contains("match case patterns must be integers, got 1.5")),
        "expected the integer-pattern diagnostic, got: {errors:?}"
    );
}

#[test]
fn case_range_end_float_reports_error_instead_of_panicking() {
    let source = "match x:\n    case 1 to 2.5:\n        pass\n";
    let errors = cobble::parser::parse(source).expect_err("parse must fail");
    assert!(
        errors
            .iter()
            .any(|error| error.contains("match case patterns must be integers, got 2.5")),
        "expected the integer-pattern diagnostic, got: {errors:?}"
    );
}

#[test]
fn execute_condition_preserves_dotted_paths() {
    let (_temp, output_dir) = compile_source(
        "def main():\n    execute if data storage foo:bar path.sub:\n        /say c\n    execute if predicate ns:foo.bar:\n        /say g\n",
    )
    .unwrap();

    let content = read_function(&output_dir, "main");
    assert!(
        content.contains("if data storage foo:bar path.sub run"),
        "dotted NBT path must round-trip: {content}"
    );
    assert!(
        content.contains("if predicate ns:foo.bar run"),
        "dotted resource path must round-trip: {content}"
    );
    assert!(
        !content.contains(". "),
        "no space may be inserted after a path dot: {content}"
    );
}

#[test]
fn execute_condition_score_ranges_still_correct() {
    let (_temp, output_dir) = compile_source(
        "def main():\n    x = 1\n    execute if score x temp matches 1..:\n        /say open\n    execute if score x temp matches 1..5:\n        /say closed\n",
    )
    .unwrap();

    let content = read_function(&output_dir, "main");
    assert!(
        content.contains("matches 1.. run"),
        "open range must stay glued: {content}"
    );
    assert!(
        content.contains("matches 1..5 run"),
        "closed range must stay glued: {content}"
    );
}

#[test]
fn execute_condition_lowercases_snbt_booleans() {
    let (_temp, output_dir) = compile_source(
        "def main():\n    execute if entity @s[nbt={x:True}]:\n        /say f\n    execute if entity @s[nbt={y:False}]:\n        /say g\n",
    )
    .unwrap();

    let content = read_function(&output_dir, "main");
    assert!(
        content.contains("@s[nbt={x:true}]"),
        "True must be emitted as SNBT `true`: {content}"
    );
    assert!(
        content.contains("@s[nbt={y:false}]"),
        "False must be emitted as SNBT `false`: {content}"
    );
}

#[test]
fn tokenizer_paren_underflow_does_not_swallow_rest_of_file() {
    // A stray `)` used to push paren_depth to -1, suppressing Newline
    // emission for every following line (library/tooling paths call the
    // tokenizer directly, without the CLI's structural pre-check).
    let tokens = cobble::parser::tokenize("x = 1)\ny = 2\n").unwrap();
    let newline_count = tokens
        .iter()
        .filter(|token| matches!(token, cobble::parser::Token::Newline))
        .count();
    assert_eq!(
        newline_count, 2,
        "each line must still emit its Newline after a stray ')': {tokens:?}"
    );
}
