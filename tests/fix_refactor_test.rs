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
            "execute as @a if score x cobble.vars matches 6.. if score y cobble.vars matches ..1 run say both"
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
        free.contains(
            "if score x cobble.vars matches 1 run scoreboard players set @s cobble.vars 1"
        ) && free.contains(
            "if score y cobble.vars matches 2 run scoreboard players set @s cobble.vars 1"
        ) && free.contains("execute as @a if score @s cobble.vars matches 1 run say either"),
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
        free.contains("execute as @a unless score done cobble.vars matches 1.. run say idle"),
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
        free.contains("execute as @a unless score x cobble.vars matches 3 run say differs"),
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
        content.contains("execute as @a if score @s cobble.vars matches 6.. run say high score"),
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
            "$execute store result score selected cobble.vars run data get storage cobble:global vars.config.$(key0)"
        ),
        "{all_functions}"
    );
}

#[test]
fn test_runtime_loop_driver_names_and_shape_are_stable() {
    // Pins the shared runtime-loop driver behind `for range(...)`, storage-list
    // `for`, and `while`: helper naming, temp-id allocation order, condition
    // re-evaluation before the body call AND before the recursive call, and
    // list-item binding at the top of the list body.
    let source = r#"
config = {"count": 1}
values = [1, 2, 3]
total = 0

def main():
    global total
    n = config.count + 5
    for i in range(1, n):
        total = total + i
    for v in values:
        total = total + v
    k = 0
    while k * 2 < n:
        k = k + 1
"#;
    let (_dir, output) = compile_source(source).expect("compile");

    // Temp ids pin the allocation order: the range loop allocates its stop
    // var (0) before the loop/body helpers (1, 2); the list loop allocates
    // index/len (3, 4), helpers (5, 6), then the dynamic item path (7);
    // `while` allocates its loop helper (8) BEFORE lowering the condition
    // (9), then the body helper (10).
    let main = read_function(&output, "main");
    assert!(main.contains("function cobble:for_range_temp_1"));
    assert!(main.contains("function cobble:for_list_temp_5"));
    assert!(main.contains("function cobble:while_temp_8"));

    let range_loop = read_function(&output, "for_range_temp_1");
    assert_eq!(
        range_loop,
        "execute if score i cobble.vars < #for_range_stop_0 cobble.vars run function cobble:for_range_body_2\n\
         execute if score i cobble.vars < #for_range_stop_0 cobble.vars run function cobble:for_range_temp_1\n"
    );

    let list_loop = read_function(&output, "for_list_temp_5");
    assert_eq!(
        list_loop,
        "execute if score #for_list_index_3 cobble.vars < #for_list_len_4 cobble.vars run function cobble:for_list_body_6\n\
         execute if score #for_list_index_3 cobble.vars < #for_list_len_4 cobble.vars run function cobble:for_list_temp_5\n"
    );

    let while_loop = read_function(&output, "while_temp_8");
    assert_eq!(
        while_loop,
        "scoreboard players operation expr_cond_temp_9 cobble.vars = k cobble.vars\n\
         scoreboard players set #multiplier cobble.vars 2\n\
         scoreboard players operation expr_cond_temp_9 cobble.vars *= #multiplier cobble.vars\n\
         execute if score expr_cond_temp_9 cobble.vars < n cobble.vars run function cobble:while_body_10\n\
         scoreboard players operation expr_cond_temp_9 cobble.vars = k cobble.vars\n\
         scoreboard players set #multiplier cobble.vars 2\n\
         scoreboard players operation expr_cond_temp_9 cobble.vars *= #multiplier cobble.vars\n\
         execute if score expr_cond_temp_9 cobble.vars < n cobble.vars run function cobble:while_temp_8\n"
    );

    let list_body = read_function(&output, "for_list_body_6");
    assert!(list_body.starts_with("data remove storage cobble:global _cobble_dyn_7\n"));
    assert!(list_body.contains(
        "function cobble:_cobble_storage_dyn_7 with storage cobble:global _cobble_dyn_7"
    ));
    assert!(list_body.ends_with("scoreboard players add #for_list_index_3 cobble.vars 1\n"));
}

#[test]
fn test_assignment_lowering_order_is_stable() {
    // Pins the load-bearing assignment classification order: const folding
    // beats condition-flag lowering (`folded` becomes a folded 1, not an
    // execute-based flag), runtime comparisons lower to 0/1 flags (`flag`),
    // self-referential simple binaries skip the redundant copy (`x = x + 1`),
    // number-op-variable keeps its set-then-operation shape (`y = 7 - x`),
    // and identifier copies pick storage vs scoreboard by the source's
    // backing (`copy = items` vs `b = x`).
    let source = r#"
def check():
    folded = 2 > 1
    x = 10
    flag = x > 5
    x = x + 1
    y = 7 - x
    items = [1, 2]
    copy = items
    b = x
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "check");
    assert_eq!(
        content,
        "scoreboard players set folded cobble.vars 1\n\
         scoreboard players set x cobble.vars 10\n\
         scoreboard players set flag cobble.vars 0\n\
         execute if score x cobble.vars matches 6.. run scoreboard players set flag cobble.vars 1\n\
         scoreboard players add x cobble.vars 1\n\
         scoreboard players set y cobble.vars 7\n\
         scoreboard players operation y cobble.vars -= x cobble.vars\n\
         data modify storage cobble:global vars.items set value [1,2]\n\
         data modify storage cobble:global vars.copy set from storage cobble:global vars.items\n\
         scoreboard players operation b cobble.vars = x cobble.vars\n"
    );
}

#[test]
fn test_assignment_param_and_complex_values_use_expression_evaluator() {
    // Macro-parameter reads and nested arithmetic both route through the
    // expression evaluator rather than the simple scoreboard ladder.
    let source = r#"
def take(p):
    q = p
    z = (p + 1) * 2
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "take");
    assert_eq!(
        content,
        "execute store result score q cobble.vars run data get storage cobble:global args.p\n\
         execute store result score z cobble.vars run data get storage cobble:global args.p\n\
         scoreboard players add z cobble.vars 1\n\
         scoreboard players set #multiplier cobble.vars 2\n\
         scoreboard players operation z cobble.vars *= #multiplier cobble.vars\n"
    );
}

#[test]
fn test_assignment_division_by_zero_error_is_stable() {
    // `y / 0` cannot const fold (try_eval_const declines division by zero),
    // so it reaches the simple scoreboard ladder, which reports the exact
    // historical error string.
    let source = r#"
def check():
    y = 4
    x = y / 0
"#;

    let error = compile_source(source).unwrap_err();
    assert!(
        error.contains("Division by zero in assignment: x = y / 0"),
        "{error}"
    );
}
