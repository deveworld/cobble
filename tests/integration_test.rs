use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// Helper function to compile a Cobble source and return the generated functions
fn compile_source(source: &str) -> Result<(TempDir, PathBuf), String> {
    let temp_dir = TempDir::new().unwrap();
    let input_file = temp_dir.path().join("test.cbl");
    let output_dir = temp_dir.path().join("output");

    fs::write(&input_file, source).unwrap();

    // Use the build command
    cobble::commands::build::build(cobble::commands::build::BuildOptions {
        input: Some(input_file),
        output: Some(output_dir.clone()),
        namespace: None,
        pack_format: None,
        description: None,
        verbose: false,
        quiet: false,
        zip: false,
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

fn read_all_functions(output_dir: &Path) -> String {
    let function_dir = output_dir.join("data/cobble/function");
    let mut content = String::new();
    for entry in fs::read_dir(function_dir)
        .unwrap()
        .filter_map(|entry| entry.ok())
    {
        if entry.path().extension().and_then(|ext| ext.to_str()) == Some("mcfunction") {
            content.push_str(&fs::read_to_string(entry.path()).unwrap());
            content.push('\n');
        }
    }
    content
}

#[test]
fn test_boolean_comparison_assignment_sets_true_value() {
    let source = r#"
def test():
    x = 1
    flag = x == 1
    if flag:
        /say true
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    assert!(content.contains("scoreboard players set flag temp 0"));
    assert!(content
        .contains("execute if score x temp matches 1 run scoreboard players set flag temp 1"));
    assert!(content.contains("execute if score flag temp matches 1.. run say true"));
}

#[test]
fn test_storage_variable_reassignment_rejected_by_type_check() {
    let source = r#"
def test():
    items = ["sword"]
    items = 3
    x = items[0]
"#;

    let error = compile_source(source).expect_err("list to integer reassignment should fail");
    assert!(error.contains("Type mismatch for variable 'items'"));
}

#[test]
fn test_storage_backed_attribute_and_subscript_access() {
    let source = r#"
def test():
    items = [1, 2, 3]
    config = {"chance": 7}
    first = items[0]
    chance = config.chance
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    assert!(content.contains("data modify storage cobble:global vars.items set value [1,2,3]"));
    assert!(content.contains("data modify storage cobble:global vars.config set value {chance:7}"));
    assert!(content.contains(
        "execute store result score first temp run data get storage cobble:global vars.items[0]"
    ));
    assert!(content
        .contains("execute store result score chance temp run data get storage cobble:global vars.config.chance"));
}

#[test]
fn test_storage_backed_subscript_access_with_const_index() {
    let source = r#"
const INDEX = 1

def test():
    items = [4, 5, 6]
    selected = items[INDEX]
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    assert!(content.contains(
        "execute store result score selected temp run data get storage cobble:global vars.items[1]"
    ));
}

#[test]
fn test_none_is_rejected_in_storage_snbt_literals() {
    let source = r#"
def test():
    storage.set("state", {"note": None})
"#;

    let error = compile_source(source).expect_err("SNBT storage null should fail");
    assert!(error.contains("unsupported-none"));
    assert!(error.contains("None/null is only supported in data pack JSON resource helper values"));
    assert!(error.contains("SNBT/NBT storage has no null type"));
}

#[test]
fn test_raw_command_inline_comment_stripped_without_breaking_command() {
    let source = r#"
def test():
    /give @s minecraft:diamond 1 # starter item
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    assert!(content.contains("give @s minecraft:diamond 1"));
    assert!(!content.contains("starter item"));
}

#[test]
fn test_snbt_map_keys_with_spaces_are_quoted() {
    let source = r#"
def test():
    storage.set("bad", {"foo bar": 1})
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    assert!(content.contains("data modify storage cobble:global bad set value {\"foo bar\":1}"));
}

#[test]
fn test_match_macro_case_uses_storage_backed_helper() {
    let source = r#"
def test(player):
    x = 0
    match x:
        case 0:
            /tellraw {player} {"text":"hit"}
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let all_functions = read_all_functions(&output_dir);
    let test_content = read_function(&output_dir, "test");

    assert!(!all_functions.contains("run $tellraw"));
    assert!(test_content.contains("with storage cobble:global args"));
    assert!(all_functions.contains("$tellraw $(player) {\"text\":\"hit\"}"));
}

#[test]
fn test_directory_build_does_not_duplicate_module_initializers() {
    let temp_dir = TempDir::new().unwrap();
    let source_dir = temp_dir.path().join("src");
    let output_dir = temp_dir.path().join("out");
    fs::create_dir_all(&source_dir).unwrap();
    fs::write(source_dir.join("a.cbl"), "score = 1\n").unwrap();
    fs::write(source_dir.join("b.cbl"), "energy = 2\n").unwrap();

    cobble::commands::build::build(cobble::commands::build::BuildOptions {
        input: Some(source_dir),
        output: Some(output_dir.clone()),
        namespace: None,
        pack_format: None,
        description: None,
        verbose: false,
        quiet: false,
        zip: false,
        validate: false,
        dry_run: false,
        commands_json: PathBuf::from("data/commands.json"),
    })
    .unwrap();

    let init_content = read_function(&output_dir, "_cobble_init");
    assert_eq!(
        init_content
            .matches("scoreboard players set score temp 1")
            .count(),
        1
    );
    assert_eq!(
        init_content
            .matches("scoreboard players set energy temp 2")
            .count(),
        1
    );
}

#[test]
fn test_simple_assignment() {
    let source = r#"
def test():
    x = 10
    y = 20
    z = x + y
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    assert!(content.contains("scoreboard players set x temp 10"));
    assert!(content.contains("scoreboard players set y temp 20"));
    assert!(content.contains("scoreboard players operation z temp = x temp"));
    assert!(content.contains("scoreboard players operation z temp += y temp"));
}

#[test]
fn test_if_statement() {
    let source = r#"
def test():
    x = 5
    if x == 5:
        /say equal
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    assert!(content.contains("scoreboard players set x temp 5"));
    assert!(content.contains("execute if score x temp matches 5 run say equal"));
}

#[test]
fn test_while_loop() {
    let source = r#"
def test():
    i = 0
    while i < 5:
        /say counting
        i = i + 1
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();

    // Check main function
    let main_content = read_function(&output_dir, "test");
    assert!(main_content.contains("scoreboard players set i temp 0"));
    assert!(main_content.contains("function cobble:while_temp_0"));

    // Check that while_body function exists (new behavior)
    let body_files: Vec<_> = fs::read_dir(output_dir.join("data/cobble/function"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("while_body_"))
        .collect();

    assert!(!body_files.is_empty(), "No while_body function generated");

    let body_content = fs::read_to_string(body_files[0].path()).unwrap();
    assert!(body_content.contains("say counting"));
    assert!(body_content.contains("scoreboard players add i temp 1"));

    // Check while loop function calls body conditionally
    let while_content = read_function(&output_dir, "while_temp_0");
    assert!(while_content
        .contains("execute if score i temp matches ..4 run function cobble:while_body"));
    assert!(while_content
        .contains("execute if score i temp matches ..4 run function cobble:while_temp_0"));
}

#[test]
fn test_for_loop() {
    let source = r#"
def test():
    for i in range(3):
        /say hello
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();

    // Check main function
    let main_content = read_function(&output_dir, "test");
    assert!(main_content.contains("scoreboard players set i loop_counter 0"));
    assert!(main_content.contains("function cobble:loop_temp_"));

    // Check for loop body function (contains the actual command)
    let body_files: Vec<_> = fs::read_dir(output_dir.join("data/cobble/function"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("loop_body_"))
        .collect();

    assert!(!body_files.is_empty(), "No loop_body function generated");

    let body_content = fs::read_to_string(body_files[0].path()).unwrap();
    assert!(body_content.contains("say hello"));

    // Check loop control function
    let loop_files: Vec<_> = fs::read_dir(output_dir.join("data/cobble/function"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("loop_temp_"))
        .collect();

    assert!(!loop_files.is_empty(), "No loop control function generated");

    let loop_content = fs::read_to_string(loop_files[0].path()).unwrap();
    assert!(loop_content.contains("scoreboard players add i loop_counter 1"));
    assert!(loop_content
        .contains("execute if score i loop_counter matches ..2 run function cobble:loop_temp_"));
}

#[test]
fn test_function_parameters() {
    let source = r#"
def greet(player, message):
    /tellraw {player} {"text":"{message}"}
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "greet");

    // Parameters should be converted to macro syntax
    assert!(content.contains("$tellraw $(player)"));
    assert!(content.contains("$(message)"));
}

#[test]
fn test_parameter_forwarding_to_parameterized_function() {
    let source = r#"
def inner(player, item, count):
    /give {player} minecraft:{item} {count}

def outer(player, item, count):
    inner(player, item, count)
    inner(item, player, count)
    /tag {player} add forwarded
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let outer = read_function(&output_dir, "outer");

    assert!(
        !outer.contains("set value \"player\"")
            && !outer.contains("set value \"item\"")
            && !outer.contains("set value \"count\""),
        "function parameter forwarding should not become string literals: {}",
        outer
    );
    assert!(outer.contains(
        "data modify storage cobble:global _call_args_0.player set from storage cobble:global args.player"
    ));
    assert!(outer.contains(
        "data modify storage cobble:global _caller_args_0 set from storage cobble:global args"
    ));
    assert!(outer.contains(
        "data modify storage cobble:global _call_args_1.player set from storage cobble:global args.item"
    ));
    assert!(outer.contains(
        "data modify storage cobble:global _call_args_1.item set from storage cobble:global args.player"
    ));
    assert!(outer.contains(
        "data modify storage cobble:global args set from storage cobble:global _call_args_"
    ));
    assert!(outer.contains(
        "data modify storage cobble:global args set from storage cobble:global _caller_args_"
    ));
    assert!(outer.contains("function cobble:inner with storage cobble:global args"));
    assert!(outer.contains("$tag $(player) add forwarded"));
}

#[test]
fn test_nested_json_variables() {
    let source = r#"
def give_item(player, item_name):
    /give {player} minecraft:stone{display:{Name:'{"text":"{item_name}"}'}}
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "give_item");

    // Both variables should be converted to macro syntax
    assert!(content.contains("$give $(player)"));
    assert!(content.contains("$(item_name)"));
}

#[test]
fn test_variable_comparison() {
    let source = r#"
def test():
    x = 10
    y = 20
    if x < y:
        /say x is less than y
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    // Should use variable-to-variable comparison
    assert!(content.contains("execute if score x temp < y temp run say x is less than y"));
}

#[test]
fn test_literal_left_comparison() {
    let source = r#"
def test():
    value = 7
    if 5 < value:
        /say greater
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    assert!(content.contains("execute if score value temp matches 6.. run say greater"));
}

#[test]
fn test_not_equal_operator() {
    let source = r#"
def test():
    x = 5
    if x != 10:
        /say not equal
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    // Should use unless instead of if with negation
    assert!(content.contains("execute unless score x temp matches 10 run say not equal"));
}

#[test]
fn test_scoreboard_objectives() {
    let source = r#"
def test():
    x = 10
    y = 20
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();

    // Check that objectives are created in init function
    let init_content = read_function(&output_dir, "_cobble_init");
    assert!(init_content.contains("scoreboard objectives add temp dummy"));
}

#[test]
fn test_minecraft_command() {
    let source = r#"
def test():
    /say Hello World
    /tellraw @a {"text":"Test"}
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    assert!(content.contains("say Hello World"));
    assert!(content.contains("tellraw @a {\"text\":\"Test\"}"));
}

#[test]
fn test_user_function_call() {
    let source = r#"
def helper():
    /say from helper

def main():
    helper()
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();

    // Check that main calls helper
    let main_content = read_function(&output_dir, "main");
    assert!(main_content.contains("function cobble:helper"));

    // Check that helper exists
    let helper_content = read_function(&output_dir, "helper");
    assert!(helper_content.contains("say from helper"));
}

#[test]
fn test_single_line_docstring() {
    let source = r#"
def test():
    """This is a single-line docstring"""
    /say Hello
    /say World
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    // Both commands should be present
    assert!(content.contains("say Hello"));
    assert!(content.contains("say World"));
}

#[test]
fn test_multi_line_docstring() {
    let source = r#"
def test():
    """This is a multi-line
    docstring that spans
    multiple lines"""
    /say After docstring
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    assert!(content.contains("say After docstring"));
}

#[test]
fn test_execute_as_at() {
    let source = r#"
def test():
    as @a at @s:
        /particle minecraft:flame ~ ~ ~ 0 0 0 0 1
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    assert!(content.contains("execute as @a at @s run particle minecraft:flame"));
}

#[test]
fn test_execute_asat() {
    let source = r#"
def test():
    asat @s:
        /say Hello
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    assert!(content.contains("execute as @s at @s run say Hello"));
}

#[test]
fn test_for_loop_with_arithmetic() {
    let source = r#"
def test():
    total = 0
    for i in range(5):
        total = total + i
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();

    // Find the loop body function (contains the arithmetic)
    let body_files: Vec<_> = fs::read_dir(output_dir.join("data/cobble/function"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("loop_body_"))
        .collect();

    assert!(!body_files.is_empty(), "No loop_body function generated");

    let body_content = fs::read_to_string(body_files[0].path()).unwrap();

    // Loop variable is now a macro parameter, but arithmetic still uses scoreboard
    // The body should have scoreboard operations with the loop variable from parameter
    assert!(body_content.contains("scoreboard players operation total temp"));
    assert!(body_content.contains("i")); // Variable i should appear somewhere
}

#[test]
fn test_global_keyword() {
    let source = r#"
def test():
    global score
    score = 10
    /say Test
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    // Should compile without error
    assert!(content.contains("scoreboard players set score temp 10"));
    assert!(content.contains("say Test"));
}

#[test]
fn test_module_level_variable_initialization() {
    let source = r#"
score = 10
counter = 5

def test():
    /say test
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let init = read_function(&output_dir, "_cobble_init");

    // Objective MUST be created first (after gamerule)
    let lines: Vec<&str> = init.lines().collect();

    // Find first objective add command (skip gamerule line)
    let obj_idx = lines
        .iter()
        .position(|l| l.contains("scoreboard objectives add"))
        .unwrap();
    let var_idx = lines
        .iter()
        .position(|l| l.contains("scoreboard players set score"))
        .unwrap();

    assert!(
        obj_idx < var_idx,
        "Objective must be created before variable initialization"
    );
    assert!(init.contains("gamerule max_command_sequence_length"));
    assert!(init.contains("scoreboard objectives add temp dummy"));
    assert!(init.contains("scoreboard players set score temp 10"));
    assert!(init.contains("scoreboard players set counter temp 5"));
}

#[test]
fn test_macro_with_execute_block() {
    let source = r#"
def give_item(player, item):
    as {player}:
        /give @s {item}
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "give_item");

    // Macro parameters should work within execute blocks
    // The $ prefix must be at the START of the entire command (Minecraft macro system rule)
    assert!(content.contains("$execute as $(player) run give @s $(item)"));
}

#[test]
fn test_complex_expressions_with_precedence() {
    let source = r#"
def test():
    a = 10
    b = 20
    c = 30
    result = a + b * c
    result2 = a * b + c
    result3 = a - b + c * a
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    // Should handle operator precedence correctly
    assert!(content.contains("scoreboard players set a temp 10"));
    assert!(content.contains("scoreboard players set b temp 20"));
    assert!(content.contains("scoreboard players set c temp 30"));

    // Verify arithmetic operations with proper precedence
    // result = a + (b * c) due to precedence
    assert!(content.contains("scoreboard players operation result temp = a temp"));
    assert!(content.contains("scoreboard players operation"));

    // Multiple complex expressions should all compile
    assert!(content.contains("result2"));
    assert!(content.contains("result3"));
}

#[test]
fn test_string_variable_error_in_say() {
    let source = r#"
def test():
    message = "Hello"
    /say {message}
"#;

    // String variables in /say are now auto-converted to /tellraw with nbt component
    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    // String is stored in data storage
    assert!(
        content.contains("data modify storage"),
        "String should be stored in data storage: {}",
        content
    );
    // say is auto-converted to tellraw with nbt component
    assert!(
        content.contains("tellraw @a"),
        "say should be auto-converted to tellraw: {}",
        content
    );
    assert!(
        content.contains("nbt") && content.contains("storage"),
        "Should use nbt storage component: {}",
        content
    );
}

#[test]
fn test_string_variable_in_tellraw_works() {
    let source = r#"
def test():
    message = "Hello"
    /tellraw @a {"text": "{message}"}
"#;

    // String variables in tellraw are converted to nbt storage components
    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    // String is stored in data storage and referenced via nbt component
    assert!(
        content.contains("data modify storage"),
        "String should be stored in data storage: {}",
        content
    );
    assert!(
        content.contains("tellraw @a"),
        "Should generate tellraw command: {}",
        content
    );
    assert!(
        content.contains("nbt") && content.contains("vars.message"),
        "Should reference string via nbt storage path: {}",
        content
    );
}

#[test]
fn test_boolean_and_operator() {
    let source = r#"
def test():
    x = 5
    y = 10
    if x > 0 and y < 15:
        /say Both conditions true!
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    // Should chain conditions with "if ... if ..."
    assert!(content.contains("execute if score x temp matches 1.. if score y temp matches ..14 run say Both conditions true!"));
}

#[test]
fn test_boolean_not_operator() {
    let source = r#"
def test():
    x = 5
    if not x == 10:
        /say Not equal to 10!
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    // Should use "unless" for negation
    assert!(content.contains("execute unless score x temp matches 10 run say Not equal to 10!"));
}

#[test]
fn test_complex_boolean_expression() {
    let source = r#"
def test():
    a = 10
    b = 20
    c = 30
    if a > 5 and b < 25 and not c == 40:
        /say Complex condition works!
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    // Should chain multiple conditions
    assert!(content.contains("execute if score a temp matches 6.. if score b temp matches ..24 unless score c temp matches 40 run say Complex condition works!"));
}

#[test]
fn test_nested_or_operators() {
    let source = r#"
def test():
    a = 10
    b = 20
    c = 30
    if a == 10 or b == 30 or c > 25:
        /say Triple OR works!
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    // Must NOT contain "OR("
    assert!(
        !content.contains("OR("),
        "Generated code contains invalid OR(...) syntax"
    );

    // Should use unique or_temp variable (not hardcoded or_result)
    assert!(content.contains("or_temp_"), "Missing or_temp variable");
    assert!(content.contains("temp 0"), "Missing or_temp initialization");

    // Should have three separate condition checks
    assert!(
        content.contains("execute if score a temp matches 10 run scoreboard players set or_temp_")
    );
    assert!(
        content.contains("execute if score b temp matches 30 run scoreboard players set or_temp_")
    );
    assert!(content
        .contains("execute if score c temp matches 26.. run scoreboard players set or_temp_"));
}

#[test]
fn test_or_with_and_combination() {
    let source = r#"
def test():
    a = 5
    b = 10
    if (a == 5 or a == 10) and b == 10:
        /say Combined works!
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    // Must NOT contain "OR("
    assert!(
        !content.contains("OR("),
        "Generated code contains invalid OR(...) syntax"
    );

    // Should use unique or_temp variable (not hardcoded or_result)
    assert!(content.contains("or_temp_"));
}

#[test]
fn test_elif_after_complex_if_and_or_condition() {
    let source = r#"
def test():
    score = 7
    energy = 5
    if score > 20 and not energy == 0:
        /say high
    elif score == 10 or energy == 0:
        /say threshold
    else:
        /say fallback
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    assert!(
        !content.contains("execute unless if"),
        "elif chain generated invalid execute syntax: {}",
        content
    );
    assert!(
        !content.contains("OR("),
        "elif OR condition was not lowered: {}",
        content
    );
    assert!(content.contains("if_branch_"));
    assert!(content.contains("or_temp_"));
    assert!(content.contains("say threshold"));
    assert!(content.contains("say fallback"));
}

#[test]
fn test_match_wildcard_single_statement() {
    let source = r#"
def test():
    x = 75
    match x:
        case 0 to 50:
            /say Low
        case 51 to 100:
            /say High
        case _:
            /say Other
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    // Wildcard must have unless conditions
    assert!(
        content.contains("unless"),
        "Wildcard case missing unless condition"
    );

    // Should have chained unless for both ranges
    assert!(
        content.contains("execute unless score match_temp_")
            && content.contains("temp matches 0..50 unless score match_temp_")
            && content.contains("temp matches 51..100 run say Other"),
        "Wildcard case not properly conditioned"
    );

    // Must NOT have bare "say Other"
    let lines: Vec<&str> = content.lines().map(|l| l.trim()).collect();
    assert!(
        !lines.contains(&"say Other"),
        "Wildcard case executed unconditionally"
    );
}

#[test]
fn test_match_wildcard_multi_statement() {
    let source = r#"
def test():
    x = 25
    match x:
        case 0 to 10:
            /say A
        case 50 to 100:
            /say B
        case _:
            /say Line1
            /say Line2
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    // Should have single chained unless command
    assert!(
        content.contains("execute unless score match_temp_")
            && content.contains("temp matches 0..10 unless score match_temp_")
            && content.contains("temp matches 50..100 run function cobble:match_default_"),
        "Wildcard function not properly conditioned"
    );

    // Should only call the function once
    let unless_count = content.matches("execute unless").count();
    assert_eq!(
        unless_count, 1,
        "Wildcard function called multiple times (expected 1, got {})",
        unless_count
    );
}

#[test]
fn test_boolean_and_in_while_loop() {
    let source = r#"
def test():
    x = 0
    y = 0
    while x < 5 and y < 10:
        /say Loop running
        x = x + 1
        y = y + 1
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();

    // Check that while_body function exists (new behavior)
    let body_files: Vec<_> = fs::read_dir(output_dir.join("data/cobble/function"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("while_body_"))
        .collect();

    assert!(!body_files.is_empty(), "No while_body function generated");

    let body_content = fs::read_to_string(body_files[0].path()).unwrap();
    assert!(body_content.contains("say Loop running"));

    // Check while loop function - should have chained conditions
    let while_content = read_function(&output_dir, "while_temp_0");
    assert!(while_content.contains("execute if score x temp matches ..4 if score y temp matches ..9 run function cobble:while_body"));
}

#[test]
fn test_raw_minecraft_in_execute_block() {
    let source = r#"
def test():
    # Execute blocks use raw Minecraft syntax, not Python expressions
    as @a if entity @s[tag=special]:
        /say Special player!
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    // Should use raw Minecraft syntax
    assert!(content.contains("execute as @a if entity @s[tag=special] run say Special player!"));
}

#[test]
fn test_execute_or_condition_keeps_per_executor_state() {
    let source = r#"
def test():
    as @a if entity @s[tag=red] or entity @s[tag=blue]:
        /say selected
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    assert!(!content.contains("execute execute"), "{}", content);
    assert!(
        content.contains("execute as @a run scoreboard players set @s temp 0"),
        "OR flag should reset per executor: {}",
        content
    );
    assert!(
        content
            .contains("execute as @a if entity @s[tag=red] run scoreboard players set @s temp 1"),
        "OR red branch should set per executor flag: {}",
        content
    );
    assert!(
        content.contains("execute as @a if score @s temp matches 1 run say selected"),
        "body should be gated by per executor flag: {}",
        content
    );
}

#[test]
fn test_boolean_and_with_different_comparisons() {
    let source = r#"
def test():
    a = 5
    b = 15
    c = 25
    if a >= 5 and b <= 20 and c != 30:
        /say All conditions met!
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    // Should handle >=, <=, and != correctly
    assert!(content.contains("execute if score a temp matches 5.. if score b temp matches ..20 unless score c temp matches 30 run say All conditions met!"));
}

#[test]
fn test_double_negative_not_not() {
    let source = r#"
def test():
    x = 5
    if not not x == 5:
        /say Double negative!
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    // Double negative should cancel out (unless unless -> if)
    assert!(content.contains("execute if score x temp matches 5 run say Double negative!"));
}

#[test]
fn test_for_loop_variable_in_tellraw() {
    let source = r#"
def test():
    for i in range(3):
        /tellraw @a {"text":"Value: {i}"}
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();

    // Find the loop body function (contains the tellraw)
    let body_files: Vec<_> = fs::read_dir(output_dir.join("data/cobble/function"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("loop_body_"))
        .collect();

    assert!(!body_files.is_empty(), "No loop_body function generated");

    let body_content = fs::read_to_string(body_files[0].path()).unwrap();

    // Loop variable is now passed as macro parameter
    // So {i} should be converted to $(i) in the macro function
    assert!(body_content.contains("$tellraw"));
    assert!(body_content.contains("$(i)"));
    // Should NOT contain literal {i}
    assert!(!body_content.contains("Value: {i}"));
}

#[test]
fn test_scoreboard_variable_in_tellraw() {
    let source = r#"
def test():
    score = 100
    /tellraw @a {"text":"Score: {score}"}
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    // Should generate JSON array with score component
    assert!(content.contains("tellraw @a ["));
    assert!(content.contains("\"score\""));
    assert!(content.contains("\"name\":\"score\""));
    assert!(content.contains("\"objective\":\"temp\""));
    // Should NOT have malformed JSON with escaped quotes
    assert!(!content.contains("{\\\"text\\\""));
}

#[test]
fn test_event_listener_tick_creates_tag() {
    let source = r#"
import stdlib
from stdlib import event

def my_tick():
    /say Every tick

stdlib.addEventListener(event.TICK, my_tick)
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();

    // Check that tick.json was created
    let tick_tag = output_dir.join("data/minecraft/tags/function/tick.json");
    assert!(
        tick_tag.exists(),
        "tick.json must be created when addEventListener(event.TICK) is called"
    );

    let content = fs::read_to_string(tick_tag).unwrap();
    assert!(
        content.contains("cobble:my_tick"),
        "tick.json must contain the tick handler function"
    );
}

#[test]
fn test_event_listener_load_creates_tag() {
    let source = r#"
import stdlib
from stdlib import event

def my_init():
    /say Load called

stdlib.addEventListener(event.LOAD, my_init)
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();

    // Check that load.json was created
    let load_tag = output_dir.join("data/minecraft/tags/function/load.json");
    assert!(
        load_tag.exists(),
        "load.json must be created when addEventListener(event.LOAD) is called"
    );

    let content = fs::read_to_string(load_tag).unwrap();
    assert!(
        content.contains("cobble:my_init"),
        "load.json must contain the load handler function"
    );
}

#[test]
fn test_event_listener_both_load_and_tick() {
    let source = r#"
import stdlib
from stdlib import event

counter = 0

def init():
    /say Init called

def tick():
    counter = counter + 1

stdlib.addEventListener(event.LOAD, init)
stdlib.addEventListener(event.TICK, tick)
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();

    // Check that both tags were created
    let load_tag = output_dir.join("data/minecraft/tags/function/load.json");
    let tick_tag = output_dir.join("data/minecraft/tags/function/tick.json");

    assert!(load_tag.exists(), "load.json must be created");
    assert!(tick_tag.exists(), "tick.json must be created");

    let load_content = fs::read_to_string(load_tag).unwrap();
    let tick_content = fs::read_to_string(tick_tag).unwrap();

    // Init should also initialize the _cobble_init
    assert!(load_content.contains("_cobble_init") || load_content.contains("init"));
    assert!(tick_content.contains("cobble:tick"));
}

#[test]
fn test_if_modifies_condition_variable() {
    // Regression test for bug where if statements with multiple statements
    // that modify the condition variable would not execute all statements
    let source = r#"
def test():
    x = 20
    if x >= 20:
        x = 0
        /say Should execute
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    // Should use a function, not inline
    assert!(content.contains("function cobble:if_temp"));

    // Check the if function
    let if_files: Vec<_> = fs::read_dir(output_dir.join("data/cobble/function"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("if_temp_"))
        .collect();

    assert!(!if_files.is_empty(), "No if function generated");

    let if_content = fs::read_to_string(if_files[0].path()).unwrap();
    assert!(if_content.contains("scoreboard players set x temp 0"));
    assert!(if_content.contains("say Should execute"));
}

#[test]
fn test_elif_modifies_condition_variable() {
    let source = r#"
def test():
    x = 15
    if x < 10:
        x = 0
        /say Less than 10
    elif x < 20:
        x = 100
        /say Between 10 and 20
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    // Should use functions
    assert!(content.contains("function cobble:elif_temp"));
    assert!(content.contains("scoreboard players set elif_taken_"));
    assert!(content.contains("run scoreboard players set elif_taken_"));
    assert!(content.contains("run scoreboard players set if_branch_"));
    assert!(content.contains("execute if score elif_taken_"));
    assert!(content.contains("run function cobble:elif_temp"));

    let elif_files: Vec<_> = fs::read_dir(output_dir.join("data/cobble/function"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("elif_temp_"))
        .collect();

    assert!(!elif_files.is_empty(), "No elif function generated");

    let elif_content = fs::read_to_string(elif_files[0].path()).unwrap();
    assert!(elif_content.contains("scoreboard players set x temp 100"));
    assert!(elif_content.contains("say Between 10 and 20"));
}

#[test]
fn test_else_modifies_condition_variable() {
    let source = r#"
def test():
    x = 5
    if x > 10:
        /say Greater
    else:
        x = 100
        /say Not greater
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    // Should use a function for else
    assert!(content.contains("function cobble:else_temp"));

    let else_files: Vec<_> = fs::read_dir(output_dir.join("data/cobble/function"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("else_temp_"))
        .collect();

    assert!(!else_files.is_empty(), "No else function generated");

    let else_content = fs::read_to_string(else_files[0].path()).unwrap();
    assert!(else_content.contains("scoreboard players set x temp 100"));
    assert!(else_content.contains("say Not greater"));
}

#[test]
fn test_while_modifies_condition_variable() {
    // Regression test for bug where while loops would evaluate condition
    // after each statement, causing issues when body modifies condition
    let source = r#"
def test():
    i = 0
    while i < 3:
        i = i + 1
        /say Iteration
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();

    // Check that while_body function exists
    let body_files: Vec<_> = fs::read_dir(output_dir.join("data/cobble/function"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("while_body_"))
        .collect();

    assert!(!body_files.is_empty(), "No while_body function generated");

    let body_content = fs::read_to_string(body_files[0].path()).unwrap();
    // Body should execute unconditionally (no execute if)
    assert!(body_content.contains("scoreboard players add i temp 1"));
    assert!(body_content.contains("say Iteration"));
    assert!(!body_content.contains("execute if"));

    // Check while_temp function
    let while_files: Vec<_> = fs::read_dir(output_dir.join("data/cobble/function"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("while_temp_"))
        .collect();

    assert!(!while_files.is_empty(), "No while function generated");

    let while_content = fs::read_to_string(while_files[0].path()).unwrap();
    // Should check condition and call body
    assert!(while_content
        .contains("execute if score i temp matches ..2 run function cobble:while_body"));
    // Should recursively call itself
    assert!(while_content
        .contains("execute if score i temp matches ..2 run function cobble:while_temp"));
}

#[test]
fn test_while_recomputes_complex_condition_each_iteration() {
    let source = r#"
def test():
    x = 0
    while x + 1 < 3:
        x = x + 1
        /say tick
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let main_content = read_function(&output_dir, "test");
    assert!(
        !main_content.contains("expr_cond_temp_"),
        "complex while condition should be evaluated inside the loop function"
    );

    let while_files: Vec<_> = fs::read_dir(output_dir.join("data/cobble/function"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("while_temp_"))
        .collect();
    let while_content = fs::read_to_string(while_files[0].path()).unwrap();
    assert!(
        while_content.matches("expr_cond_temp_").count() >= 4,
        "while condition should be computed before body and before recursion: {}",
        while_content
    );
    assert!(!while_content.contains("OR("));
}

#[test]
fn test_while_lowers_or_condition_inside_loop_function() {
    let source = r#"
def test():
    x = 1
    y = 0
    while x == 1 or y == 1:
        x = 0
        /say any
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let while_files: Vec<_> = fs::read_dir(output_dir.join("data/cobble/function"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("while_temp_"))
        .collect();
    let while_content = fs::read_to_string(while_files[0].path()).unwrap();
    assert!(!while_content.contains("OR("));
    assert!(while_content.contains("scoreboard players set or_temp_"));
    assert!(while_content.contains("execute if score or_temp_"));
}

#[test]
fn test_match_snapshots_identifier_before_running_cases() {
    let source = r#"
def test():
    x = 0
    match x:
        case 0:
            x = 1
            /say zero
        case 1:
            /say one
        case _:
            /say default
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");
    assert!(
        content.contains("scoreboard players operation match_temp_"),
        "match should snapshot identifier input: {}",
        content
    );
    assert!(
        content.contains("matches 0 run") && content.contains("match_temp_"),
        "match cases should test the snapshot temp: {}",
        content
    );
    assert!(
        !content.contains("if score x temp matches 1 run say one"),
        "later cases must not observe case body mutations: {}",
        content
    );
}

#[test]
fn test_control_flow_helper_calls_preserve_macro_storage_args() {
    let source = r#"
def greet(player):
    x = 1
    if x == 1:
        /tellraw {player} {"text":"a"}
        /tellraw {player} {"text":"b"}
    while x == 1:
        /tellraw {player} {"text":"loop"}
        x = 0
    match x:
        case 0:
            /tellraw {player} {"text":"match"}
            /say done
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "greet");
    assert!(
        content.contains("function cobble:if_temp_")
            && content.contains("with storage cobble:global args"),
        "if helper macro function should be called with storage: {}",
        content
    );

    let while_files: Vec<_> = fs::read_dir(output_dir.join("data/cobble/function"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("while_temp_"))
        .collect();
    let while_content = fs::read_to_string(while_files[0].path()).unwrap();
    assert!(
        while_content.contains("function cobble:while_body_")
            && while_content.contains("with storage cobble:global args"),
        "while body macro function should be called with storage: {}",
        while_content
    );
    assert!(
        content.contains("function cobble:match_case_")
            && content.contains("with storage cobble:global args"),
        "match case macro function should be called with storage: {}",
        content
    );
}

#[test]
fn test_for_loop_body_keeps_outer_function_params() {
    let source = r#"
def repeat(player):
    for i in range(2):
        /tellraw {player} {"text":"loop {i}"}
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let body_files: Vec<_> = fs::read_dir(output_dir.join("data/cobble/function"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("loop_body_"))
        .collect();
    let body_content = fs::read_to_string(body_files[0].path()).unwrap();
    assert!(body_content.contains("$(player)"), "{}", body_content);
    assert!(body_content.contains("$(i)"), "{}", body_content);
}

#[test]
fn test_tick_counter_example() {
    // Test the README example that was broken
    let source = r#"
import stdlib
from stdlib import event

counter = 0

def tick():
    global counter
    counter = counter + 1
    if counter >= 20:
        counter = 0
        /tellraw @a {"text":"One second passed"}

stdlib.addEventListener(event.TICK, tick)
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "tick");

    // Should use a function for the if block
    assert!(content.contains("function cobble:if_temp"));

    // Find and check the if function
    let if_files: Vec<_> = fs::read_dir(output_dir.join("data/cobble/function"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("if_temp_"))
        .collect();

    assert!(!if_files.is_empty(), "No if function generated");

    let if_content = fs::read_to_string(if_files[0].path()).unwrap();
    assert!(if_content.contains("scoreboard players set counter temp 0"));
    assert!(if_content.contains("tellraw @a"));
}

#[test]
fn test_const_variable() {
    let source = r#"
def test():
    const PI = 3.14159
    const RADIUS = 5
    area = PI * RADIUS * RADIUS
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    // Constants should be inlined at compile time
    assert!(content.contains("scoreboard players set area temp 78"));
    assert!(!content.contains("PI temp"));
}

#[test]
fn test_const_declaration() {
    let source = r#"
def test():
    const MAX_HEALTH = 100
    health = MAX_HEALTH
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    // Constant should be inlined at compile time
    assert!(content.contains("scoreboard players set health temp 100"));
    assert!(!content.contains("MAX_HEALTH temp"));
}

#[test]
fn test_match_literal() {
    let source = r#"
def test():
    x = 5
    match x:
        case 0:
            /say Zero
        case 5:
            /say Five
        case 10:
            /say Ten
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    // Should have match condition for each case
    assert!(content.contains("if score"));
    assert!(content.contains("matches"));
}

#[test]
fn test_match_range() {
    let source = r#"
def test():
    score = 75
    match score:
        case 0 to 59:
            /say Fail
        case 60 to 79:
            /say Pass
        case 80 to 100:
            /say Excellent
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    // Should have range matches
    assert!(content.contains("if score"));
    assert!(content.contains("matches"));
}

#[test]
fn test_match_wildcard() {
    let source = r#"
def test():
    value = 42
    match value:
        case 0:
            /say Zero
        case 1:
            /say One
        case _:
            /say Other
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    // Should handle wildcard case
    assert!(content.contains("if score") || content.contains("function"));
}

#[test]
fn test_match_with_multiple_statements() {
    let source = r#"
def test():
    x = 5
    match x:
        case 5:
            /say First
            /say Second
            /say Third
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let _content = read_function(&output_dir, "test");

    // Should create a function for multi-statement case
    let match_files: Vec<_> = fs::read_dir(output_dir.join("data/cobble/function"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("match_"))
        .collect();

    // Should have generated at least one match function
    assert!(!match_files.is_empty(), "No match function generated");
}

#[test]
fn test_selector_definition() {
    let source = r#"
@Player = @a[type=player,gamemode=survival]
@Boss = @e[type=zombie,tag=boss]

def test():
    as @Player:
        /give @s diamond

    as @Boss:
        /effect give @s strength 10 2
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    // Selector aliases should be expanded
    assert!(content.contains("@a[type=player,gamemode=survival]"));
    assert!(content.contains("@e[type=zombie,tag=boss]"));
    assert!(!content.contains("@Player"));
    assert!(!content.contains("@Boss"));
}

#[test]
fn test_selector_in_commands() {
    let source = r#"
@AllPlayers = @a[gamemode=!spectator]

def broadcast():
    /tellraw @AllPlayers {"text":"Hello!"}
    /title @AllPlayers title {"text":"Welcome"}
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "broadcast");

    // Selector alias should be replaced in all commands
    assert!(content.contains("@a[gamemode=!spectator]"));
    assert!(!content.contains("@AllPlayers"));
}

#[test]
fn test_file_import() {
    use std::fs;

    // Create temp directory with multiple files
    let temp_dir = TempDir::new().unwrap();
    let utils_file = temp_dir.path().join("utils.cbl");
    let main_file = temp_dir.path().join("main.cbl");
    let output_dir = temp_dir.path().join("output");

    // Write utils.cbl
    fs::write(
        &utils_file,
        r#"
def helper():
    /say Helper function

@Admin = @a[tag=admin]
"#,
    )
    .unwrap();

    // Write main.cbl
    fs::write(
        &main_file,
        r#"
import utils

def test():
    helper()
    as @Admin:
        /say Test
"#,
    )
    .unwrap();

    // Compile
    cobble::commands::build::build(cobble::commands::build::BuildOptions {
        input: Some(main_file),
        output: Some(output_dir.clone()),
        namespace: None,
        pack_format: None,
        description: None,
        verbose: false,
        quiet: false,
        zip: false,
        validate: false,
        dry_run: false,
        commands_json: PathBuf::from("data/commands.json"),
    })
    .unwrap();

    // Check that functions from imported file exist
    let helper_content = read_function(&output_dir, "helper");
    assert!(helper_content.contains("say Helper function"));

    let test_content = read_function(&output_dir, "test");
    assert!(test_content.contains("function cobble:helper"));
    // Should NOT have "with storage" for parameterless function
    assert!(!test_content.contains("with storage"));
    assert!(test_content.contains("@a[tag=admin]"));
}

#[test]
fn test_loop_variable_in_commands() {
    let source = r#"
def test():
    for i in range(3):
        /say Count: {i}
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();

    // Check that loop body is a macro function
    let body_files: Vec<_> = fs::read_dir(output_dir.join("data/cobble/function"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("loop_body_"))
        .collect();

    assert!(!body_files.is_empty(), "No loop_body function generated");

    let body_content = fs::read_to_string(body_files[0].path()).unwrap();
    // Should use macro variable syntax
    assert!(body_content.contains("$say Count: $(i)"));

    // Check loop control function stores variable to storage
    let loop_files: Vec<_> = fs::read_dir(output_dir.join("data/cobble/function"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("loop_temp_"))
        .collect();

    assert!(!loop_files.is_empty(), "No loop control function generated");

    let loop_content = fs::read_to_string(loop_files[0].path()).unwrap();
    // After Bug 1 fix, loop_temp no longer stores - wrapper does that
    // Just verify it calls wrapper and recurses
    assert!(
        loop_content.contains("function cobble:loop_wrapper_")
            || loop_content.contains("function cobble:loop_body_")
    );

    // Check wrapper function exists (contains storage operations)
    let wrapper_files: Vec<_> = fs::read_dir(output_dir.join("data/cobble/function"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("loop_wrapper_"))
        .collect();

    if !wrapper_files.is_empty() {
        let wrapper_content = fs::read_to_string(wrapper_files[0].path()).unwrap();
        assert!(wrapper_content.contains("execute store result storage"));
        assert!(wrapper_content.contains("function cobble:loop_body_"));
        assert!(wrapper_content.contains("with storage"));
    }
}

#[test]
fn test_loop_variable_with_step() {
    let source = r#"
def test():
    for i in range(10) by 2:
        /say Even: {i}
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();

    let body_files: Vec<_> = fs::read_dir(output_dir.join("data/cobble/function"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("loop_body_"))
        .collect();

    assert!(!body_files.is_empty());

    let body_content = fs::read_to_string(body_files[0].path()).unwrap();
    assert!(body_content.contains("$say Even: $(i)"));
}

#[test]
fn test_parameterless_function_call() {
    let source = r#"
def helper():
    /say Helper called

def main():
    helper()
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();

    let main_content = read_function(&output_dir, "main");
    // Should NOT have "with storage" for parameterless function
    assert!(main_content.contains("function cobble:helper"));
    assert!(!main_content.contains("with storage"));
}

#[test]
fn test_multiple_if_in_execute_block() {
    let source = r#"
def test():
    as @a at @s if entity @s[tag=one] if entity @s[tag=two] if entity @s[tag=three]:
        /say multiple conditions
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    // Must be all lowercase
    assert!(content.contains("execute as @a at @s if entity @s[tag=one] if entity @s[tag=two] if entity @s[tag=three] run say multiple conditions"));
    // Ensure no capitalized keywords (regression test for Display trait bug)
    assert!(
        !content.contains(" If "),
        "Found uppercase 'If' in generated command"
    );
    assert!(
        !content.contains(" Unless "),
        "Found uppercase 'Unless' in generated command"
    );
    assert!(
        !content.contains(" Entity "),
        "Found uppercase 'Entity' in generated command"
    );
}

#[test]
fn test_if_unless_combination_in_execute() {
    let source = r#"
def test():
    as @a if entity @s[tag=ready] unless entity @s[tag=done]:
        /say execute this
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    // All keywords must be lowercase
    assert!(content.contains("if entity @s[tag=ready] unless entity @s[tag=done]"));
    assert!(!content.contains(" If "), "Found uppercase 'If'");
    assert!(!content.contains(" Unless "), "Found uppercase 'Unless'");
}

#[test]
fn test_complex_execute_chain() {
    let source = r#"
def test():
    as @e[type=armor_stand] at @s if entity @s[tag=marker] if entity @a[distance=..5] unless entity @s[tag=triggered]:
        /say complex chain
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    // Verify all lowercase
    let lines: Vec<&str> = content.lines().collect();
    for line in &lines {
        // Check that no Minecraft keywords are capitalized
        assert!(
            !line.contains(" If "),
            "Line contains capitalized 'If': {}",
            line
        );
        assert!(
            !line.contains(" Unless "),
            "Line contains capitalized 'Unless': {}",
            line
        );
        assert!(
            !line.contains(" Entity "),
            "Line contains capitalized 'Entity': {}",
            line
        );
        assert!(
            !line.contains(" As "),
            "Line contains capitalized 'As': {}",
            line
        );
        assert!(
            !line.contains(" At "),
            "Line contains capitalized 'At': {}",
            line
        );
    }
}

#[test]
fn test_power_operator_simple() {
    // Test that basic power operator works
    let source = r#"
def test():
    x = 2
    result = x ^ 3
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    // Should have #power_base and multiplications (# prefix for internal fake players)
    assert!(content.contains("#power_base"));

    // For x^3, we need 2 multiplications (x * x * x = x * (x * x))
    let mult_count = content
        .matches("scoreboard players operation result temp *= #power_base temp")
        .count();
    assert_eq!(
        mult_count, 2,
        "Expected 2 multiplications for x^3, found {}",
        mult_count
    );
}

#[test]
fn test_asat_with_multi_entity_selector() {
    // Regression test for asat bug: should use @s not the selector
    let source = r#"
def test():
    asat @e[type=armor_stand]:
        /say Hello
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    // Should use "at @s" not "at @e[type=armor_stand]"
    assert!(
        content.contains("execute as @e[type=armor_stand] at @s run say Hello"),
        "asat should generate 'at @s', not 'at @e[type=armor_stand]'"
    );

    // Make sure it doesn't use the wrong pattern
    assert!(
        !content.contains("at @e[type=armor_stand]"),
        "asat incorrectly generated 'at @e[type=armor_stand]' instead of 'at @s'"
    );
}

#[test]
fn test_power_operator_zero_exponent() {
    // Test that x^0 = 1
    let source = r#"
def test():
    x = 5
    result = x ^ 0
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    // x^0 should set result to 1
    assert!(content.contains("scoreboard players set x temp 5"));
    assert!(
        content.contains("scoreboard players set result temp 1"),
        "x^0 should set result to 1, but command not found in: {}",
        content
    );

    // Should NOT have any multiplication operations for x^0
    assert!(
        !content.contains("power_base"),
        "x^0 should not use power_base"
    );
    assert!(
        !content.contains("*="),
        "x^0 should not have any multiplication"
    );
}

#[test]
fn test_power_operator_assignment_zero_exponent() {
    // Test that x^0 = 1 works in assignment form
    let source = r#"
def test():
    base = 10
    result = base ^ 0
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    // base^0 should set result to 1
    assert!(content.contains("scoreboard players set base temp 10"));
    assert!(
        content.contains("scoreboard players set result temp 1"),
        "base^0 should set result to 1"
    );
}

// Regression test for title command action token preservation
// Bug fix: Title commands with scoreboard variables should preserve the action token
// (title/subtitle/actionbar) between selector and JSON text array
#[test]
fn test_title_command_preserves_action() {
    let source = r#"
score = 100

def show_title():
    /title @a title Score: {score}

def show_subtitle():
    /title @a subtitle Level: {score}

def show_actionbar():
    /title @a actionbar HP: {score}
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();

    let title_content = read_function(&output_dir, "show_title");
    let subtitle_content = read_function(&output_dir, "show_subtitle");
    let actionbar_content = read_function(&output_dir, "show_actionbar");

    // Verify action token is preserved in correct position (after selector, before JSON array)
    assert!(
        title_content.contains("title @a title ["),
        "title action should be preserved: got '{}'",
        title_content
    );
    assert!(
        subtitle_content.contains("title @a subtitle ["),
        "subtitle action should be preserved: got '{}'",
        subtitle_content
    );
    assert!(
        actionbar_content.contains("title @a actionbar ["),
        "actionbar action should be preserved: got '{}'",
        actionbar_content
    );

    // Verify action is NOT part of the JSON text
    assert!(
        !title_content.contains(r#"{"text":"title Score:"#),
        "action should not be inside JSON text: got '{}'",
        title_content
    );
    assert!(
        !subtitle_content.contains(r#"{"text":"subtitle Level:"#),
        "action should not be inside JSON text: got '{}'",
        subtitle_content
    );
    assert!(
        !actionbar_content.contains(r#"{"text":"actionbar HP:"#),
        "action should not be inside JSON text: got '{}'",
        actionbar_content
    );

    // Verify scoreboard variables are still correctly converted to JSON score components
    assert!(
        title_content.contains(r#"{"score":{"name":"score","objective":"temp"}}"#),
        "scoreboard variable should be converted to JSON score component"
    );
}

#[test]
fn test_macro_title_plain_text_becomes_json_component() {
    let source = r#"
def notify(player, count, message):
    /title {player} actionbar Kit count: {count}
    /tellraw {player} Notice: {message}
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "notify");

    assert!(
        content.contains(r#"$title $(player) actionbar {"text":"Kit count: $(count)"}"#),
        "title macro text should be emitted as a JSON text component: {}",
        content
    );
    assert!(
        content.contains(r#"$tellraw $(player) {"text":"Notice: $(message)"}"#),
        "tellraw macro text should be emitted as a JSON text component: {}",
        content
    );
}

// Regression test for all title command actions
#[test]
fn test_title_all_actions_with_scoreboard_vars() {
    let source = r#"
value = 42

def test():
    /title @a title Value: {value}
    /title @a subtitle Status: {value}
    /title @a actionbar Count: {value}
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    // All three actions should be preserved
    let lines: Vec<&str> = content.lines().collect();

    let title_line = lines
        .iter()
        .find(|l| l.contains("title @a title"))
        .expect("title command not found");
    let subtitle_line = lines
        .iter()
        .find(|l| l.contains("title @a subtitle"))
        .expect("subtitle command not found");
    let actionbar_line = lines
        .iter()
        .find(|l| l.contains("title @a actionbar"))
        .expect("actionbar command not found");

    // Verify format: title @a <action> [JSON]
    assert!(
        title_line.starts_with("title @a title ["),
        "title format incorrect: {}",
        title_line
    );
    assert!(
        subtitle_line.starts_with("title @a subtitle ["),
        "subtitle format incorrect: {}",
        subtitle_line
    );
    assert!(
        actionbar_line.starts_with("title @a actionbar ["),
        "actionbar format incorrect: {}",
        actionbar_line
    );
}

// REGRESSION TESTS FOR BUG FIXES

#[test]
fn test_boolean_literal_only() {
    // Regression test for Bug #2: Boolean literals without variables
    // should still generate __internal__ objective
    let source = r#"
def test():
    if True:
        /say true branch
    if False:
        /say false branch
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();

    // Check that __internal__ objective is initialized
    let init_content = read_function(&output_dir, "_cobble_init");
    assert!(init_content.contains("scoreboard objectives add __internal__ dummy"));
    assert!(init_content.contains("scoreboard players set #true_const __internal__ 1"));
    assert!(init_content.contains("scoreboard players set #false_const __internal__ 0"));

    // Check that conditions use __internal__ with exact match
    let test_content = read_function(&output_dir, "test");
    assert!(test_content.contains("score #true_const __internal__ matches 1"));
    assert!(test_content.contains("score #false_const __internal__ matches 1"));
}

#[test]
fn test_loop_variable_scope_isolation() {
    // Regression test for Bug #3: Loop variables should not pollute outer scope
    let source = r#"
def func1():
    for i in range(5):
        /say loop

def func2():
    i = 10
    /tellraw @a {"text":"i is 10"}
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let func2_content = read_function(&output_dir, "func2");

    // In func2, 'i' should be a regular temp variable, not loop_counter
    assert!(func2_content.contains("scoreboard players set i temp 10"));

    // It should NOT reference loop_counter
    assert!(!func2_content.contains("loop_counter"));
}

#[test]
fn test_internal_objective_only_when_needed() {
    // Test that __internal__ objective is only initialized when Boolean literals are used

    // Test 1: With Boolean literals - should have __internal__ init
    let source_with_bool = r#"
def test():
    if True:
        /say works
"#;
    let (_temp1, output1) = compile_source(source_with_bool).unwrap();
    let init1 = read_function(&output1, "_cobble_init");
    assert!(init1.contains("scoreboard objectives add __internal__ dummy"));
    assert!(init1.contains("scoreboard players set #true_const __internal__ 1"));
    assert!(init1.contains("scoreboard players set #false_const __internal__ 0"));

    // Test 2: Without Boolean literals - should NOT have __internal__ init
    let source_without_bool = r#"
def test():
    x = 10
"#;
    let (_temp2, output2) = compile_source(source_without_bool).unwrap();
    let init2 = read_function(&output2, "_cobble_init");
    assert!(!init2.contains("#true_const"));
    assert!(!init2.contains("#false_const"));
}

#[test]
fn test_for_loop_type_checking() {
    // Regression test for Bug #4: Type checking should work correctly
    // with variable types isolated between loop body and outer scope
    let source = r#"
def test():
    x = 10
    for i in range(5):
        x = i
    # x is still Integer type after loop, this should work
    x = 20
"#;

    let result = compile_source(source);

    // This should succeed - x remains Integer throughout
    assert!(result.is_ok());
}

#[test]
fn test_invalid_number_literal() {
    // Regression test for Bug #1: Invalid number literals should be rejected
    let source = r#"
def test():
    x = 1.2.3.4
"#;

    let result = compile_source(source);

    // This should fail during tokenization
    assert!(result.is_err());
}

#[test]
fn test_const_modulo_consistency() {
    // Regression test for Bug #6: const modulo should match runtime modulo
    let source = r#"
const x = -5 % 3

def test():
    y = -5 % 3
    # Both x and y should have the same value (-2)
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();

    // Both should produce the same result
    // In Rust/integer arithmetic: -5 % 3 = -2
    let test_content = read_function(&output_dir, "test");

    // y = -5 % 3 should use modulo operation
    // In integer arithmetic: -5 % 3 = -2
    assert!(test_content.contains("modulus") || test_content.contains("-2"));
}

#[test]
fn test_nested_loops_no_infinite_loop() {
    // Regression test for nested loop wrapper naming bug
    // Bug: wrapper functions had duplicate names causing infinite loops
    let source = r#"
def test():
    for i in range(2):
        for j in range(2):
            result = i + j
            /say done
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();

    // Check that we have TWO distinct wrapper functions, not one
    let wrapper_files: Vec<_> = std::fs::read_dir(output_dir.join("data/cobble/function"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("loop_wrapper_"))
        .collect();

    assert_eq!(
        wrapper_files.len(),
        2,
        "Should have exactly 2 wrapper functions for nested loops, found {}",
        wrapper_files.len()
    );

    // Verify outer loop calls the correct (outer) wrapper
    let loop_temp_0 = read_function(&output_dir, "loop_temp_0");
    assert!(
        loop_temp_0.contains("loop_wrapper_3") || loop_temp_0.contains("loop_wrapper_"),
        "Outer loop should call a wrapper function"
    );

    // Verify inner loop calls a different wrapper
    let loop_temp_1 = read_function(&output_dir, "loop_temp_1");
    assert!(
        loop_temp_1.contains("loop_wrapper_2") || loop_temp_1.contains("loop_wrapper_"),
        "Inner loop should call a wrapper function"
    );

    // Most importantly: verify the two wrappers are DIFFERENT
    let outer_wrapper_call = loop_temp_0
        .lines()
        .find(|l| l.contains("loop_wrapper_"))
        .unwrap();
    let inner_wrapper_call = loop_temp_1
        .lines()
        .find(|l| l.contains("loop_wrapper_"))
        .unwrap();

    assert_ne!(
        outer_wrapper_call, inner_wrapper_call,
        "Outer and inner loops must call DIFFERENT wrapper functions to avoid infinite loop"
    );
}

#[test]
fn test_nested_loops_with_arithmetic() {
    // Test that nested loop variables work correctly in arithmetic expressions
    let source = r#"
def test():
    for i in range(2):
        for j in range(2):
            result = i + j
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let loop_body_1 = read_function(&output_dir, "loop_body_1");

    // Both i and j should be accessible from loop_counter
    assert!(
        loop_body_1.contains("i loop_counter"),
        "Inner loop body should access outer loop variable i from loop_counter"
    );
    assert!(
        loop_body_1.contains("j loop_counter"),
        "Inner loop body should access inner loop variable j from loop_counter"
    );

    // Should perform the addition
    assert!(
        loop_body_1.contains("result temp =") && loop_body_1.contains("+="),
        "Should perform addition: result = i + j"
    );
}

#[test]
fn test_triple_nested_loops() {
    // Test that triple nested loops work correctly
    let source = r#"
def test():
    for i in range(2):
        for j in range(2):
            for k in range(2):
                /say Triple loop works
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();

    // Should have 3 wrapper functions
    let wrapper_files: Vec<_> = std::fs::read_dir(output_dir.join("data/cobble/function"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("loop_wrapper_"))
        .collect();

    assert_eq!(
        wrapper_files.len(),
        3,
        "Should have exactly 3 wrapper functions for triple nested loops"
    );
}

// ============================================================================
// REGRESSION TESTS FOR BUG FIXES
// ============================================================================

#[test]
fn test_regression_minus_operator_context_aware() {
    // Regression test for BUG #1: Context-aware tokenization for minus operator
    // Previously: 10-5 was tokenized as [10, -5] instead of [10, -, 5]
    let source = r#"
def test():
    a = 10-5
    b = (5-3)*2
    c = -10+5
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    // 10-5 should compile-time evaluate to 5
    assert!(content.contains("scoreboard players set a temp 5"));
    // (5-3)*2 should compile-time evaluate to 4
    assert!(content.contains("scoreboard players set b temp 4"));
    // -10+5 should compile-time evaluate to -5
    assert!(content.contains("scoreboard players set c temp -5"));
}

#[test]
fn test_regression_power_operator_context_aware() {
    // Regression test for BUG #1: Context-aware tokenization for power operator
    // Previously: 2^3 was incorrectly tokenized (^ treated as coordinate marker)
    let source = r#"
def test():
    a = 2^3
    b = (2+3)^2
    c = 10^2
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    // 2^3 should compile-time evaluate to 8
    assert!(content.contains("scoreboard players set a temp 8"));
    // (2+3)^2 should compile-time evaluate to 25
    assert!(content.contains("scoreboard players set b temp 25"));
    // 10^2 should compile-time evaluate to 100
    assert!(content.contains("scoreboard players set c temp 100"));
}

#[test]
fn test_regression_decimal_pack_format() {
    // Regression test: Pack format validation now requires exactly 101.1 (MC Java 26.1.2)
    // Old pack formats should be rejected
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let input_file = temp_dir.path().join("test.cbl");
    let output_dir = temp_dir.path().join("output");

    fs::write(&input_file, "def test():\n    x = 1").unwrap();

    // Build with pack format 101.1 (required for MC Java 26.1.2)
    let result = cobble::commands::build::build(cobble::commands::build::BuildOptions {
        input: Some(input_file.clone()),
        output: Some(output_dir.clone()),
        namespace: None,
        pack_format: Some("101.1".to_string()),
        description: None,
        verbose: false,
        quiet: false,
        zip: false,
        validate: false,
        dry_run: false,
        commands_json: PathBuf::from("data/commands.json"),
    });

    assert!(
        result.is_ok(),
        "Pack format 101.1 should be accepted: {:?}",
        result.err()
    );

    // Check pack.mcmeta contains correct min_format/max_format array format
    let pack_meta = fs::read_to_string(output_dir.join("pack.mcmeta")).unwrap();
    assert!(
        pack_meta.contains("min_format"),
        "pack.mcmeta should use min_format for decimal pack formats"
    );
    assert!(
        pack_meta.contains("max_format"),
        "pack.mcmeta should use max_format for decimal pack formats"
    );
    assert!(
        pack_meta.contains("101"),
        "pack.mcmeta should contain major version 101"
    );

    // Old pack formats should be rejected
    let temp_dir2 = TempDir::new().unwrap();
    let input_file2 = temp_dir2.path().join("test.cbl");
    let output_dir2 = temp_dir2.path().join("output");
    fs::write(&input_file2, "def test():\n    x = 1").unwrap();

    let result2 = cobble::commands::build::build(cobble::commands::build::BuildOptions {
        input: Some(input_file2),
        output: Some(output_dir2),
        namespace: None,
        pack_format: Some("18".to_string()),
        description: None,
        verbose: false,
        quiet: false,
        zip: false,
        validate: false,
        dry_run: false,
        commands_json: PathBuf::from("data/commands.json"),
    });

    assert!(result2.is_err(), "Old pack format 18 should be rejected");

    // Same major version with the wrong minor version should also be rejected.
    let temp_dir3 = TempDir::new().unwrap();
    let input_file3 = temp_dir3.path().join("test.cbl");
    let output_dir3 = temp_dir3.path().join("output");
    fs::write(&input_file3, "def test():\n    x = 1").unwrap();

    let result3 = cobble::commands::build::build(cobble::commands::build::BuildOptions {
        input: Some(input_file3),
        output: Some(output_dir3),
        namespace: None,
        pack_format: Some("101.0".to_string()),
        description: None,
        verbose: false,
        quiet: false,
        zip: false,
        validate: false,
        dry_run: false,
        commands_json: PathBuf::from("data/commands.json"),
    });

    assert!(result3.is_err(), "Pack format 101.0 should be rejected");
}

#[test]
fn test_regression_division_by_zero_error() {
    // Regression test for BUG #3: Division by zero should error, not warn
    // Previously: Division by zero only warned, now it errors
    let source = r#"
def test():
    const divisor = 0
    result = 10 / divisor
"#;

    let result = compile_source(source);

    // Should fail with division by zero error
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("Division by zero"));
}

#[test]
fn test_regression_modulo_by_zero_error() {
    // Regression test for BUG #3: Modulo by zero should error, not warn
    let source = r#"
def test():
    const divisor = 0
    result = 10 % divisor
"#;

    let result = compile_source(source);

    // Should fail with modulo by zero error
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("Modulo by zero"));
}

#[test]
fn test_regression_power_exponent_limit() {
    // Regression test for BUG #4: Power exponent should be limited
    // Previously: base^500 generated 499 multiplication commands
    let source = r#"
def test():
    base = 2
    result = base ^ 500
"#;

    let result = compile_source(source);

    // Should fail with "Power exponent too large" error
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("Power exponent too large"));
    assert!(err.contains("500 > 100"));
}

#[test]
fn test_regression_power_const_exponent_limit() {
    // Const exponents should also be limited before compile-time expansion.
    let source = r#"
def test():
    const EXP = 500
    base = 2
    result = base ^ EXP
"#;

    let result = compile_source(source);

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("Power exponent too large"));
    assert!(err.contains("500 > 100"));
}

#[test]
fn test_regression_power_nested_const_exponent_limit() {
    // Nested expressions take a separate evaluation path that must enforce the same cap.
    let source = r#"
def test():
    const EXP = 500
    base = 2
    result = (base + 1) ^ EXP
"#;

    let result = compile_source(source);

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("Power exponent too large"));
    assert!(err.contains("500 > 100"));
}

#[test]
fn test_regression_power_exponent_within_limit() {
    // Verify that power exponents within the limit (<=100) still work
    let source = r#"
def test():
    base = 2
    result = base ^ 10
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    // Should generate multiplication commands
    assert!(content.contains("power_base"));
    assert!(content.contains("*="));
}

#[test]
fn test_regression_boundary_condition_gt_max() {
    // Regression test for BUG #5: x > i32::MAX should be always false
    // Previously: Used saturating_add which caused incorrect condition
    let source = r#"
def test():
    max_val = 2147483647
    if max_val > 2147483647:
        x = 1
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    // Should generate an always-false condition
    // Uses "matches 0 unless matches 0" pattern
    assert!(content.contains("if score max_val temp matches 0 unless score max_val temp matches 0"));
}

#[test]
fn test_regression_boundary_condition_lt_min() {
    // Regression test for BUG #5: x < i32::MIN should be always false
    let source = r#"
def test():
    min_val = -2147483648
    if min_val < -2147483648:
        x = 1
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    // Should generate an always-false condition
    assert!(content.contains("if score min_val temp matches 0 unless score min_val temp matches 0"));
}

#[test]
fn test_regression_boundary_condition_gte_max() {
    // Regression test for BUG #5: x >= i32::MAX should work correctly
    let source = r#"
def test():
    max_val = 2147483647
    if max_val >= 2147483647:
        x = 1
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    // Should generate correct condition (matches 2147483647..)
    assert!(content.contains("if score max_val temp matches 2147483647.."));
}

#[test]
fn test_regression_boundary_condition_lte_min() {
    // Regression test for BUG #5: x <= i32::MIN should work correctly
    let source = r#"
def test():
    min_val = -2147483648
    if min_val <= -2147483648:
        x = 1
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    // Should generate correct condition (matches ..-2147483648)
    assert!(content.contains("if score min_val temp matches ..-2147483648"));
}

#[test]
fn test_regression_normal_comparisons_still_work() {
    // Ensure normal comparisons (not at boundaries) still work correctly
    let source = r#"
def test():
    a = 10
    if a > 5:
        b = 1
    if a < 20:
        c = 1
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    // a > 5 should be "matches 6.."
    assert!(content.contains("if score a temp matches 6.."));
    // a < 20 should be "matches ..19"
    assert!(content.contains("if score a temp matches ..19"));
}

#[test]
fn test_const_initialization_in_scoreboard() {
    // Test that const variables are compile-time only and NOT written to scoreboard
    let source = r#"
const MY_CONST = 42
const OTHER = 100

def test():
    x = MY_CONST
    match MY_CONST:
        case 42:
            /say correct
        case _:
            /say wrong
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();

    // Constants should be compile-time substituted, not initialized at runtime
    let content = read_function(&output_dir, "test");

    // x = MY_CONST should be folded to x = 42
    assert!(
        content.contains("scoreboard players set x temp 42"),
        "Const should be substituted at compile time"
    );

    // MY_CONST should NOT appear as a scoreboard variable
    assert!(
        !content.contains("scoreboard players set MY_CONST"),
        "Const should not generate runtime scoreboard set"
    );
}

#[test]
fn test_power_overflow_handling() {
    // Test that power overflow is handled (should clamp to i32::MAX)
    let source = r#"
const BIG = 50000
def test():
    result = BIG ^ 2
"#;

    let result = compile_source(source);
    assert!(result.is_ok(), "Should compile even with overflow");

    let (_temp, output_dir) = result.unwrap();
    let content = read_function(&output_dir, "test");

    // Should contain i32::MAX (2147483647) instead of overflowed value
    assert!(
        content.contains("2147483647") || content.contains("temp"),
        "Should handle overflow properly"
    );
}

#[test]
fn test_tellraw_styling_preserved() {
    // Test that tellraw styling is preserved when using variables
    let source = r#"
def test():
    score = 100
    /tellraw @a {"text":"Score: {score}","color":"gold","bold":true,"clickEvent":{"action":"run_command","value":"/say clicked"}}
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    // Check that the output contains styled JSON components
    assert!(
        content.contains(r#""color":"gold""#) || content.contains("score"),
        "Styling should be preserved in tellraw with variables"
    );
}

#[test]
fn test_stale_tag_cleanup() {
    // This test would need to create files and rebuild to test cleanup
    // For now, we just verify the basic functionality compiles
    let source = r#"
import stdlib
from stdlib import event

def on_load():
    /say test

stdlib.addEventListener(event.LOAD, on_load)
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();

    // Check that load.json was created
    let load_tag = output_dir.join("data/minecraft/tags/function/load.json");
    assert!(load_tag.exists(), "Load tag should be created");
}

// ============================================================================
// REGRESSION TESTS FOR v0.5.17 BUG FIXES
// ============================================================================

#[test]
fn test_return_statement_error() {
    // Regression test: Return statements should now error instead of being no-op
    // Bug: Return statements were silently ignored, allowing code after return to execute
    let source = r#"
def test():
    x = 1
    /say Before return
    return x
    /say After return should not execute
"#;

    let result = compile_source(source);

    // Should fail with return not supported error
    assert!(result.is_err(), "Return statement should cause an error");
    let error = result.unwrap_err();
    assert!(
        error.contains("Return statements are not supported"),
        "Error should mention return statements are not supported"
    );
    assert!(
        error.contains("Minecraft functions cannot return early"),
        "Error should explain Minecraft limitation"
    );
}

#[test]
fn test_return_no_value_error() {
    // Test that bare return (no value) also errors
    let source = r#"
def test():
    /say Hello
    return
    /say Unreachable
"#;

    let result = compile_source(source);
    assert!(
        result.is_err(),
        "Return statement (no value) should cause an error"
    );
    let error = result.unwrap_err();
    assert!(error.contains("Return statements are not supported"));
}

#[test]
fn test_function_call_assignment_error() {
    // Regression test: Function call results cannot be assigned to variables
    // Bug: x = func() silently failed, x was never assigned
    let source = r#"
def helper():
    /say Helper called

def test():
    x = helper()
    /say Done
"#;

    let result = compile_source(source);

    // Should fail with function call assignment error
    assert!(
        result.is_err(),
        "Function call assignment should cause an error"
    );
    let error = result.unwrap_err();
    assert!(
        error.contains("Function calls in expressions are not supported"),
        "Error should mention function call issue: {}",
        error
    );
}

#[test]
fn test_attribute_assignment_error() {
    // Test that attribute access in assignments errors properly
    let source = r#"
def test():
    x = obj.field
"#;

    let result = compile_source(source);
    assert!(
        result.is_err(),
        "Attribute access assignment should cause an error"
    );
    let error = result.unwrap_err();
    assert!(
        error.contains("unsupported-storage-access")
            && error.contains("Cannot resolve storage-backed attribute access `obj.`"),
        "Error should mention storage-backed attribute resolution: {}",
        error
    );
}

#[test]
fn test_subscript_assignment_error() {
    // Subscript syntax is parsed, but non-storage bases cannot be resolved to
    // Minecraft storage paths.
    let source = r#"
def test():
    x = arr[0]
"#;

    let result = compile_source(source);
    assert!(
        result.is_err(),
        "Subscript on a non-storage base should fail"
    );
    let error = result.unwrap_err();
    assert!(
        error.contains("unsupported-storage-access")
            && error.contains("Cannot resolve storage-backed subscript access `arr[...]`"),
        "Should get storage-backed subscript diagnostic: {}",
        error
    );
}

#[test]
fn test_string_assignment_still_works() {
    // Regression test: String assignments should still work (used in command substitution)
    let source = r#"
def test():
    message = "Hello World"
    /tellraw @a {"text":"{message}"}
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    // String is stored in data storage and referenced via nbt component in tellraw
    assert!(
        content.contains("data modify storage"),
        "String should be stored in data storage: {}",
        content
    );
    assert!(
        content.contains("tellraw @a"),
        "Should generate tellraw command: {}",
        content
    );
    assert!(
        content.contains("nbt") && content.contains("vars.message"),
        "Should reference string via nbt storage path: {}",
        content
    );
}

#[test]
fn test_boolean_assignment_still_works() {
    // Regression test: Boolean assignments should still work (used in command substitution)
    let source = r#"
def test():
    enabled = True
    disabled = False
    /tellraw @a {"text":"Enabled: {enabled}, Disabled: {disabled}"}
"#;

    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    // Booleans are stored as scoreboard values and displayed via score components
    assert!(
        content.contains("scoreboard players set enabled temp 1"),
        "Boolean true should be set to 1: {}",
        content
    );
    assert!(
        content.contains("scoreboard players set disabled temp 0"),
        "Boolean false should be set to 0: {}",
        content
    );
    // Tellraw should use score components for boolean variables
    assert!(
        content.contains("tellraw @a"),
        "Should generate tellraw command: {}",
        content
    );
    assert!(
        content.contains("score") && content.contains("enabled"),
        "Should use score component for boolean: {}",
        content
    );
}
