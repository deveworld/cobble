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
        zip: false,
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
    assert!(while_content.contains("execute if score i temp matches ..4 run function cobble:while_body"));
    assert!(while_content.contains("execute if score i temp matches ..4 run function cobble:while_temp_0"));
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
    assert!(init.contains("gamerule maxCommandChainLength"));
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

    // Should fail with helpful error message
    let result = compile_source(source);
    assert!(result.is_err());
    let error = result.unwrap_err();
    assert!(error.contains("Cannot use string variable 'message'"));
    assert!(error.contains("Solutions"));
}

#[test]
fn test_string_variable_in_tellraw_works() {
    let source = r#"
def test():
    message = "Hello"
    /tellraw @a {"text": "{message}"}
"#;

    // Should succeed because tellraw with JSON supports strings
    let (_temp, output_dir) = compile_source(source).unwrap();
    let content = read_function(&output_dir, "test");

    // Verify the output is valid JSON with correct string replacement
    assert!(content.contains("tellraw @a {\"text\": \"Hello\"}"));
    // Should NOT have double quotes
    assert!(!content.contains("\"\"Hello\"\""));
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
    assert!(!content.contains("OR("), "Generated code contains invalid OR(...) syntax");

    // Should use or_result variable
    assert!(content.contains("or_result"), "Missing or_result variable");
    assert!(content.contains("scoreboard players set or_result temp 0"), "Missing or_result initialization");

    // Should have three separate condition checks
    assert!(content.contains("execute if score a temp matches 10 run scoreboard players set or_result temp 1"));
    assert!(content.contains("execute if score b temp matches 30 run scoreboard players set or_result temp 1"));
    assert!(content.contains("execute if score c temp matches 26.. run scoreboard players set or_result temp 1"));
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
    assert!(!content.contains("OR("), "Generated code contains invalid OR(...) syntax");

    // Should use or_result
    assert!(content.contains("or_result"));
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
    assert!(content.contains("unless"), "Wildcard case missing unless condition");

    // Should have chained unless for both ranges
    assert!(content.contains("execute unless score x temp matches 0..50 unless score x temp matches 51..100 run say Other"),
            "Wildcard case not properly conditioned");

    // Must NOT have bare "say Other"
    let lines: Vec<&str> = content.lines().map(|l| l.trim()).collect();
    assert!(!lines.iter().any(|line| *line == "say Other"),
            "Wildcard case executed unconditionally");
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
    assert!(content.contains("execute unless score x temp matches 0..10 unless score x temp matches 50..100 run function cobble:match_default_"),
            "Wildcard function not properly conditioned");

    // Should only call the function once
    let unless_count = content.matches("execute unless").count();
    assert_eq!(unless_count, 1, "Wildcard function called multiple times (expected 1, got {})", unless_count);
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
    assert!(content.contains("cobble:my_tick"), "tick.json must contain the tick handler function");
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
    assert!(content.contains("cobble:my_init"), "load.json must contain the load handler function");
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
    assert!(while_content.contains("execute if score i temp matches ..2 run function cobble:while_body"));
    // Should recursively call itself
    assert!(while_content.contains("execute if score i temp matches ..2 run function cobble:while_temp"));
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
    // PI * RADIUS * RADIUS should be evaluated
    assert!(content.contains("area"));
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

    // Constant should work in assignments
    assert!(content.contains("health"));
    // Should reference MAX_HEALTH (currently treated as variable)
    assert!(content.contains("MAX_HEALTH") || content.contains("100"));
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
        zip: false,
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
    assert!(loop_content.contains("execute store result storage"));
    assert!(loop_content.contains("function cobble:loop_body_"));
    assert!(loop_content.contains("with storage"));
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
    assert!(!content.contains(" If "), "Found uppercase 'If' in generated command");
    assert!(!content.contains(" Unless "), "Found uppercase 'Unless' in generated command");
    assert!(!content.contains(" Entity "), "Found uppercase 'Entity' in generated command");
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
        assert!(!line.contains(" If "), "Line contains capitalized 'If': {}", line);
        assert!(!line.contains(" Unless "), "Line contains capitalized 'Unless': {}", line);
        assert!(!line.contains(" Entity "), "Line contains capitalized 'Entity': {}", line);
        assert!(!line.contains(" As "), "Line contains capitalized 'As': {}", line);
        assert!(!line.contains(" At "), "Line contains capitalized 'At': {}", line);
    }
}
