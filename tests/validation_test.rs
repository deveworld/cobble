//! Integration test that validates all transpiler output against Minecraft's actual command tree.
//! Requires data/commands.json to be present (run scripts/setup_commands_json.sh first).

use std::fs;
use std::path::Path;

fn get_validator() -> Option<cobble::validator::CommandValidator> {
    let commands_json = Path::new("data/commands.json");
    if !commands_json.exists() {
        eprintln!("Skipping validation tests: data/commands.json not found");
        eprintln!("  Run: scripts/setup_commands_json.sh");
        return None;
    }
    Some(cobble::validator::CommandValidator::from_file(commands_json).unwrap())
}

/// Helper: compile source and validate all output .mcfunction files
fn compile_and_validate(source: &str) -> Vec<(String, cobble::validator::ValidationError)> {
    let validator = match get_validator() {
        Some(v) => v,
        None => return vec![],
    };

    let temp_dir = tempfile::TempDir::new().unwrap();
    let input_file = temp_dir.path().join("test.cbl");
    let output_dir = temp_dir.path().join("output");
    fs::write(&input_file, source).unwrap();

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
        commands_json: std::path::PathBuf::from("data/commands.json"),
    })
    .unwrap();

    let report = validator.validate_datapack(&output_dir);
    report
        .errors
        .into_iter()
        .map(|(path, err)| (path.display().to_string(), err))
        .collect()
}

fn format_errors(errors: &[(String, cobble::validator::ValidationError)]) -> String {
    errors
        .iter()
        .map(|(f, e)| {
            format!(
                "  {}:{}: {}\n    | {}",
                f, e.line_number, e.message, e.command
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn test_validate_simple_assignment() {
    let errors = compile_and_validate(
        r#"
def test():
    x = 10
    y = 20
    z = x + y
"#,
    );
    assert!(
        errors.is_empty(),
        "Validation errors:\n{}",
        format_errors(&errors)
    );
}

#[test]
fn test_validate_control_flow() {
    let errors = compile_and_validate(
        r#"
def test():
    x = 5
    if x > 3:
        /say big
    elif x > 1:
        /say medium
    else:
        /say small

    for i in range(5):
        /say count {i}

    while x > 0:
        x = x - 1
"#,
    );
    assert!(
        errors.is_empty(),
        "Validation errors:\n{}",
        format_errors(&errors)
    );
}

#[test]
fn test_validate_execute_blocks() {
    let errors = compile_and_validate(
        r#"
score = 0
def test():
    as @a at @s:
        /particle flame ~ ~1 ~
    asat @s:
        /say hello
    positioned ~ ~1 ~:
        /say above
    as @a at @s:
        if score >= 10:
            /say high
"#,
    );
    assert!(
        errors.is_empty(),
        "Validation errors:\n{}",
        format_errors(&errors)
    );
}

#[test]
fn test_validate_function_calls() {
    let errors = compile_and_validate(
        r#"
def greet(player, msg):
    /tellraw {player} {"text":f"{msg}"}

def test():
    greet("@a", "Hello")
"#,
    );
    assert!(
        errors.is_empty(),
        "Validation errors:\n{}",
        format_errors(&errors)
    );
}

#[test]
fn test_validate_match_statement() {
    let errors = compile_and_validate(
        r#"
def test():
    score = 75
    match score:
        case 0 to 59:
            /say F
        case 60 to 79:
            /say C
        case 80 to 100:
            /say A
        case _:
            /say invalid
"#,
    );
    assert!(
        errors.is_empty(),
        "Validation errors:\n{}",
        format_errors(&errors)
    );
}

#[test]
fn test_validate_event_system() {
    let errors = compile_and_validate(
        r#"
import stdlib
from stdlib import event

health = 100
active = True

def on_load():
    /say loaded

def on_tick():
    if health > 0:
        /say alive

stdlib.addEventListener(event.LOAD, on_load)
stdlib.addEventListener(event.TICK, on_tick)
"#,
    );
    assert!(
        errors.is_empty(),
        "Validation errors:\n{}",
        format_errors(&errors)
    );
}

#[test]
fn test_validate_tellraw_commands() {
    let errors = compile_and_validate(
        r#"
score = 42
def test():
    /say Hello World
    /say Score is {score}
    /tellraw @a {"text":"Hello","color":"green"}
    /tellraw @a [{"text":"Click "},{"text":"here","click_event":{"action":"run_command","command":"/say hi"}}]
"#,
    );
    assert!(
        errors.is_empty(),
        "Validation errors:\n{}",
        format_errors(&errors)
    );
}

#[test]
fn test_validate_minecraft_26_commands() {
    let errors = compile_and_validate(
        r#"
def test():
    /dialog clear @a
    /dialog show @a minecraft:server_links
    /fetchprofile name Notch
    /fetchprofile id 123e4567-e89b-12d3-a456-426614174000
    /waypoint list
    /waypoint modify @s color red
    /waypoint modify @s color hex ff00aa
    /waypoint modify @s style set minecraft:default
    /stopwatch create cobble:test
    /stopwatch query cobble:test 20.0
    /swing @a mainhand
    /version
    /return run say ok
    /test run minecraft:sample 1 true
"#,
    );
    assert!(
        errors.is_empty(),
        "Validation errors:\n{}",
        format_errors(&errors)
    );
}

#[test]
fn test_validate_minecraft_26_3_command_changes() {
    // Commands whose syntax changed or appeared between 26.1.2 and 26.3.
    let errors = compile_and_validate(
        r#"
def test():
    /item replace entity @s container.0 with minecraft:stone
    /item replace entity @s armor.head with minecraft:iron_helmet
    /item fill entity @s container.* with minecraft:stone
    /item override block ~ ~ ~ container.0 with minecraft:stone
    /item modify entity @s weapon {type:"minecraft:set_count",count:2}
    /execute if items entity @s weapon minecraft:diamond run say has diamond
    /execute if slots entity @s container.* run say has slots
    /compute default integer {type:"minecraft:constant",value:5}
    /compute default float {type:"minecraft:uniform",min:0.0,max:1.0} 10
    /data modify storage cobble:test value set compute default integer {type:"minecraft:constant",value:3}
    /team modify red color dark_purple
    /team modify red color reset
    /waypoint modify @s color red
    /waypoint modify @s color reset
    /swing @a mainhand stab
    /schedule clear cobble:tick
"#,
    );
    assert!(
        errors.is_empty(),
        "Validation errors:\n{}",
        format_errors(&errors)
    );
}

#[test]
fn test_validate_placeholders_that_stand_for_several_arguments() {
    let Some(validator) = get_validator() else {
        return;
    };
    // Each of these ran on a 26.3 server with values such as
    // `{from:"0 0 0", to:"1 1 1"}` and `{to:"a:b path"}`.
    for command in [
        "$fill $(from) $(to) $(block) replace $(filter) $(mode)",
        "$fill $(from) $(to) $(block) replace $(filter) strict",
        "$fill $(from) $(to) $(block) hollow",
        "$particle minecraft:heart $(pos) $(delta) 0 1 force @a",
        "$particle $(p) $(pos) $(delta) $(speed) $(count) force @a",
        "$spreadplayers $(center) $(dist) $(range) false @a",
        "$clone $(a) $(b) $(c) replace force",
        "$data modify storage $(to) set from storage game_data $(storage)[{id:$(pnum)}]",
        "$data modify storage $(to) set value 1",
        "$execute store result storage $(target) int 1 run say a",
    ] {
        assert_eq!(validator.validate_command(command), Ok(()), "{command}");
    }
    // What comes before the first placeholder is the final command's, and
    // what follows one still has to fit somewhere after it.
    for command in [
        "$execute if score $generation.k bs.lambda< #generation.w bs.data positioned ~$(x) $(y) $(z) run say a",
        "$nope $(from) $(to) stone",
        "$fill $(from) $(to) $(block) replace stone nope",
        "$fill $(from) $(to) stone replace stone nope",
        "$swing $(who) bogus",
        "$scoreboard players set $(who) 1 2 3 4",
        "$item modify block ~ ~ ~ container.$(slot) {\"function\":\"minecraft:set_count\",\"count\":1}",
    ] {
        assert!(validator.validate_command(command).is_err(), "{command}");
    }
}

#[test]
fn test_validate_rejects_commands_above_function_permission_level() {
    // `transfer` (admins) and `stop` (owners) parse in chat but not in a data
    // pack function at the default function-permission-level of 2.
    let errors = compile_and_validate(
        r#"
def test():
    /transfer example.org 25565 @a
    /stop
"#,
    );
    if errors.is_empty() && !std::path::Path::new("data/commands.json").exists() {
        return;
    }
    assert_eq!(errors.len(), 2, "{}", format_errors(&errors));
    assert!(errors
        .iter()
        .all(|(_, error)| error.message.contains("requires permission level")));
}

#[test]
fn test_validate_comprehensive() {
    let errors = compile_and_validate(
        r#"
import stdlib
from stdlib import event

const MAX_HP = 100
@Player = @a[gamemode=survival]

health = MAX_HP
score = 0
active = True
items = ["sword", "shield"]

def init():
    x = 10
    y = 20
    z = x + y
    w = x * y + 5
    r = x % 3
    p = x ^ 2
    /tellraw @a {"text":"Loaded!","color":"green"}

def game_tick():
    if score >= 100:
        /say Winner!
    elif score >= 50:
        /say Halfway

    as @Player at @s:
        /particle flame ~ ~1 ~

    for i in range(3):
        /say tick {i}

def greet(player, msg):
    /tellraw {player} {"text":f"{msg}"}

def use_greet():
    greet("@a", "Welcome")

stdlib.addEventListener(event.LOAD, init)
stdlib.addEventListener(event.TICK, game_tick)
"#,
    );
    assert!(
        errors.is_empty(),
        "Validation errors found:\n{}",
        format_errors(&errors)
    );
}

#[test]
fn test_validate_references_of_an_inline_predicate() {
    let Some(_) = get_validator() else {
        return;
    };
    // Refuted 14: Minecraft looks these up when it loads the function, and
    // a missing one stops the function from loading.
    let errors = compile_and_validate(
        r##"from stdlib import datapack
datapack.item_tag("mine", ["minecraft:stone"])
datapack.predicate("chance", {"type": "minecraft:random_chance", "chance": 0.5})

def go():
    /execute if predicate {type:"minecraft:match_tool",predicate:{items:"#minecraft:nope"}} run say a
    /execute if predicate {type:"minecraft:all_of",terms:["cobble:nope"]} run say b
    /execute if predicate {type:"minecraft:match_tool",predicate:{items:"#cobble:mine"}} run say c
    /execute if predicate {type:"minecraft:match_tool",predicate:{items:"#minecraft:logs"}} run say d
    /execute if predicate {type:"minecraft:all_of",terms:["cobble:chance", "othermod:any"]} run say e
"##,
    );
    let messages: Vec<&str> = errors
        .iter()
        .map(|(_, error)| error.message.as_str())
        .collect();
    assert_eq!(messages.len(), 2, "{}", format_errors(&errors));
    assert!(
        messages[0].contains("Unknown item tag `#minecraft:nope` at `predicate.items`"),
        "{messages:?}"
    );
    assert!(
        messages[1].contains("Unknown predicate `cobble:nope` at `terms[0]`"),
        "{messages:?}"
    );
}
