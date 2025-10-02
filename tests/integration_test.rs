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

    // Check while loop function
    let while_content = read_function(&output_dir, "while_temp_0");
    assert!(while_content.contains("execute if score i temp matches ..4 run say counting"));
    assert!(while_content
        .contains("execute if score i temp matches ..4 run scoreboard players add i temp 1"));
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

    // Check for loop function (name may vary, find it dynamically)
    let loop_files: Vec<_> = fs::read_dir(output_dir.join("data/cobble/function"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("loop_temp_"))
        .collect();

    assert!(!loop_files.is_empty(), "No loop function generated");

    let loop_content = fs::read_to_string(loop_files[0].path()).unwrap();
    assert!(loop_content.contains("say hello"));
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

    // Find the loop function
    let loop_files: Vec<_> = fs::read_dir(output_dir.join("data/cobble/function"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("loop_temp_"))
        .collect();

    assert!(!loop_files.is_empty(), "No loop function generated");

    let loop_content = fs::read_to_string(loop_files[0].path()).unwrap();

    // MUST use loop_counter for i, not temp!
    assert!(loop_content.contains("scoreboard players operation total temp += i loop_counter"));
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

    // Objective MUST be created first
    let lines: Vec<&str> = init.lines().collect();
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

    // Check while loop function
    let while_content = read_function(&output_dir, "while_temp_0");

    // Should have chained conditions in the while loop
    assert!(while_content.contains("execute if score x temp matches ..4 if score y temp matches ..9 run say Loop running"));
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

    // Find the loop function
    let loop_files: Vec<_> = fs::read_dir(output_dir.join("data/cobble/function"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("loop_temp_"))
        .collect();

    assert!(!loop_files.is_empty(), "No loop function generated");

    let loop_content = fs::read_to_string(loop_files[0].path()).unwrap();

    // i should be converted to scoreboard JSON component
    assert!(loop_content.contains("\"score\""));
    assert!(loop_content.contains("\"name\":\"i\""));
    assert!(loop_content.contains("\"objective\":\"loop_counter\""));
    // Should NOT contain literal {i}
    assert!(!loop_content.contains("Value: {i}"));
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
