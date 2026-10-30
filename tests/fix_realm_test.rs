//! Regression tests for bugs found while building a large server pack
//! (economy, shop, quests, arena, world boss) and playing it on a Minecraft
//! 26.3 server with test bots:
//! - `tellraw` and `title` showed string variables in quotes (`"Bread"`),
//!   because Minecraft 26.3 prints an `nbt` text component as SNBT
//! - `text.tellraw()` and the other text helpers printed `{name}`
//!   placeholders as they were
//! - the storage check rejected reading an item of a `persist` list, of a
//!   list imported with `from other import name`, and a field of a loop
//!   variable that walks a list of maps
//! - the validator rejected macro lines with a token such as `kills_$(mob)`
//! - an unknown `@Name` in a raw command, such as a typo or an entity
//!   template, reached the pack as it was, and an alias inside a `selector`
//!   text component was not expanded; Minecraft can't load either command
//! - `@Alias[...]` in a raw command expanded to `@a[...][...]`
//! - advancement `player` and `entity` conditions in pre-26.3 shapes made a
//!   server reject every pack on `/reload`
//! - `if 2 in items:` compiled to `execute if 2 in items`, which Minecraft
//!   can't load
//! - `elif` and `else` after `if entity ...:` gave a bare parse error
//! - `module.variable` asked for a map literal instead of an import
//! - the build summary counted only the entry files as source files
//! - a string parameter with a `"` in `tellraw` text broke the JSON, and
//!   Minecraft then ran none of the function
//! - `grid[r][c] = v` with parameter indexes wrote a map key of `grid[r]`
//! - `for ch in word` over a string parameter left `ch` empty

mod common;

use common::mcsim::{compile, compile_project, load_functions, Sim};
use std::fs;
use std::path::Path;
use std::process::Command;

#[test]
fn string_values_show_without_quotes_in_text_commands() {
    let mut sim = Sim::load(
        r#"
title = "Boss"
items = ["Bread", "Iron Sword"]

def label(value):
    copy = value
    /tellraw @a {"text":"Value: {copy}","color":"gold"}

def main():
    /tellraw @a {"text":"Title: {title}"}
    name = items[1]
    /tellraw @a ["Item: {name}"]
    label("Steak")
    label(42)
    label([1, 2])
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(
        sim.log,
        [
            "Title: Boss",
            "Item: Iron Sword",
            "Value: Steak",
            "Value: 42",
            "Value: [1, 2]"
        ]
    );
    let main = sim.function_text("main");
    assert!(
        main.contains(r#"{"interpret":true,"nbt":"vars.title""#),
        "{main}"
    );
}

#[test]
fn text_helpers_show_variables() {
    let source = r#"
from stdlib import text

def main():
    left = 3
    text.tellraw("@a", {"text": "Starts in {left}", "color": "yellow"})
"#;
    let mut sim = Sim::load(source, &[]);
    sim.call("main");
    assert_eq!(sim.log, ["Starts in 3"]);

    let (_temp, output_dir) = compile(
        r#"
from stdlib import text

def main():
    left = 3
    text.actionbar("@a", "Wait {left}s")
"#,
    )
    .unwrap_or_else(|error| panic!("build failed: {error}"));
    assert_eq!(
        load_functions(&output_dir)["main"].last().unwrap(),
        r##"title @a actionbar {"extra":[{"score":{"name":"#main.left","objective":"sim.vars"}},{"text":"s"}],"text":"Wait "}"##
    );
}

#[test]
fn items_of_persistent_and_imported_lists_can_be_assigned() {
    let mut sim = Sim::load(
        r#"
persist names = ["Ann", "Bo"]
catalog = [{"price": 3}, {"price": 4}]

def main():
    i = 1
    name = names[i]
    /say {name}
    total = 0
    for item in catalog:
        total += item.price
    return total
"#,
        &[],
    );
    assert_eq!(sim.call("main"), Some(7));
    assert_eq!(sim.log, ["Server: Bo"]);

    let mut sim = Sim::load_project(
        &[
            ("a.cbl", "items = [4, 5]\nconfig = {\"limit\": 9}\n"),
            (
                "main.cbl",
                "from a import items, config\n\ndef main():\n    x = items[1]\n    y = config.limit\n    return x + y\n",
            ),
        ],
        &[],
    );
    assert_eq!(sim.call("main"), Some(14));
}

#[test]
fn macro_lines_may_mix_text_and_placeholders_in_a_token() {
    let commands_json = Path::new("data/commands.json");
    if !commands_json.exists() {
        eprintln!("Skipping: data/commands.json not found");
        return;
    }
    let validator = cobble::validator::CommandValidator::from_file(commands_json).unwrap();
    assert!(validator
        .validate_command("$scoreboard players get @s kills_$(mob)")
        .is_ok());
    assert!(validator
        .validate_command("$scoreboard players add @s $(prefix)_coins 5")
        .is_ok());
    assert!(validator
        .validate_command("scoreboard players get @s kills_$(mob)")
        .is_err());
}

#[test]
fn unknown_selectors_in_raw_commands_are_errors() {
    let error = compile(
        r#"
@Spawn = @e[type=minecraft:marker,tag=spawn,limit=1]

def main():
    /tp @s @Spwan
"#,
    )
    .unwrap_err();
    assert!(error.contains("Unknown selector `@Spwan`"), "{error}");

    let error = compile(
        r#"
define @Boss = @e[type=minecraft:zombie,tag=boss]
create {"NoAI": True}
end

def main():
    /kill @Boss
"#,
    )
    .unwrap_err();
    assert!(error.contains("`@Boss` is an entity template"), "{error}");

    let error = compile(
        r#"
@Bosses = @e[type=minecraft:zombie,tag=boss]

def main():
    /execute as @a at @s if entity @Bosses[distance=..8] run say near
"#,
    )
    .unwrap_err();
    assert!(
        error.contains("`@Bosses` can't take more arguments"),
        "{error}"
    );
}

#[test]
fn chat_text_and_selector_components_keep_working() {
    let (_temp, output_dir) = compile(
        r#"
@Fighters = @a[tag=fighter]

def main():
    /say hi @Bob and @Fighters
    /msg @Fighters hello @Bob
    /tell @Fighters @Bob is here
    /tellraw @a {"text":"@Bob"}
    /summon minecraft:pig ~ ~ ~ {CustomName:'@Pig'}
    /tellraw @a ["Fighters: ",{"selector":"@Fighters"}]
"#,
    )
    .unwrap_or_else(|error| panic!("build failed: {error}"));
    assert_eq!(
        load_functions(&output_dir)["main"],
        [
            "say hi @Bob and @a[tag=fighter]",
            "msg @a[tag=fighter] hello @Bob",
            "tell @a[tag=fighter] @Bob is here",
            r#"tellraw @a {"text":"@Bob"}"#,
            "summon minecraft:pig ~ ~ ~ {CustomName:'@Pig'}",
            r#"tellraw @a ["Fighters: ",{"selector":"@a[tag=fighter]"}]"#,
        ]
    );
}

#[test]
fn advancement_conditions_use_one_typed_predicate() {
    let list = compile(
        r#"
from stdlib import datapack

datapack.advancement("slayer", {"criteria": {"kill": {
    "trigger": "minecraft:player_killed_entity",
    "conditions": {"entity": [{"condition": "minecraft:entity_properties", "entity": "this", "predicate": {}}]}
}}})
"#,
    )
    .unwrap_err();
    assert!(
        list.contains("`criteria.kill.conditions.entity` is now one predicate, not a list"),
        "{list}"
    );
    assert!(
        list.contains("`criteria.kill.conditions.entity[0].condition` is now `type`"),
        "{list}"
    );

    let bare = compile(
        r#"
from stdlib import datapack

datapack.advancement("sneaky", {"criteria": {"kill": {
    "trigger": "minecraft:player_killed_entity",
    "conditions": {"player": {"flags": {"is_sneaking": True}}}
}}})
"#,
    )
    .unwrap_err();
    assert!(
        bare.contains("`criteria.kill.conditions.player` is now a predicate with a `type`"),
        "{bare}"
    );

    compile_project(&[(
        "main.cbl",
        r#"
from stdlib import datapack

datapack.advancement("slayer", {"criteria": {"kill": {
    "trigger": "minecraft:player_killed_entity",
    "conditions": {"entity": {"type": "minecraft:entity_properties", "entity": "this", "predicate": {}}}
}}})
datapack.advancement("diamond", {"criteria": {"got": {
    "trigger": "minecraft:inventory_changed",
    "conditions": {"items": [{"items": "minecraft:diamond"}]}
}}})
"#,
    )])
    .unwrap_or_else(|error| panic!("build failed: {error}"));
}

#[test]
fn conditions_without_a_minecraft_keyword_are_errors() {
    let error = compile(
        r#"
items = [1, 2]

def main():
    if 2 in items:
        /say yes
"#,
    )
    .unwrap_err();
    assert!(
        error.contains("Unsupported condition `2 in items`: Cobble has no `in` operator"),
        "{error}"
    );

    let (_temp, output_dir) = compile(
        r#"
def main():
    if entity @s in minecraft:the_nether:
        /say nether
    if stopwatch sim:race 1..:
        /say running
"#,
    )
    .unwrap_or_else(|error| panic!("build failed: {error}"));
    assert_eq!(
        load_functions(&output_dir)["main"],
        [
            "execute if entity @s in minecraft:the_nether run say nether",
            "execute if stopwatch sim:race 1.. run say running",
        ]
    );
}

#[test]
fn else_after_a_minecraft_condition_explains_the_rewrite() {
    let diagnostics = cobble::diagnostics::analyze_source(
        r#"
def main():
    if entity @s[tag=a]:
        /say a
    elif entity @s[tag=b]:
        /say b
    else:
        /say c
"#,
    );
    let messages: Vec<_> = diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.line, diagnostic.message.clone()))
        .collect();
    assert_eq!(
        messages,
        [(
            5,
            "`elif` can't follow an `if` with a Minecraft condition, which runs as an execute block"
                .to_string()
        )]
    );
}

#[test]
fn module_variables_read_through_the_module_name_suggest_an_import() {
    let error = compile_project(&[
        ("a.cbl", "count = 5\n\ndef bump():\n    return 1\n"),
        (
            "main.cbl",
            "import a\n\ndef main():\n    a.bump()\n    x = a.count + 1\n    return x\n",
        ),
    ])
    .unwrap_err();
    assert!(
        error.contains("`a.count` names a variable of the module `a`")
            && error.contains("from a import count"),
        "{error}"
    );
}

#[test]
fn build_summary_counts_imported_files() {
    let temp = tempfile::TempDir::new().unwrap();
    let source = temp.path().join("src");
    fs::create_dir_all(&source).unwrap();
    fs::write(
        temp.path().join("cobble.toml"),
        "[project]\nname = \"two\"\ndescription = \"Two files\"\nnamespace = \"two\"\nversion = \"1.0.0\"\npack_format = \"121.0\"\n\n[build]\nentry_points = [\"main.cbl\"]\n",
    )
    .unwrap();
    fs::write(source.join("util.cbl"), "def helper():\n    /say hi\n").unwrap();
    fs::write(
        source.join("main.cbl"),
        "import util\n\ndef main():\n    helper()\n",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_cobble"))
        .arg("build")
        .current_dir(temp.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{stdout}{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(stdout.contains("  Source files: 2\n"), "{stdout}");
}

#[test]
fn parameters_with_quotes_show_in_text_commands() {
    let mut sim = Sim::load(
        r#"
def announce(msg):
    /tellraw @a {"text":"[News] {msg}","color":"gold"}
    /say {msg}

def main():
    announce("He said \"hi\"")
    announce(42)
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(
        sim.log,
        [
            "[News] He said \"hi\"",
            "Server: He said \"hi\"",
            "[News] 42",
            "Server: 42"
        ]
    );
    let announce = sim.function_text("announce");
    assert!(
        announce.lines().any(|line| line.starts_with("tellraw @a ")),
        "{announce}"
    );
}

#[test]
fn nested_list_items_take_parameter_indexes() {
    let mut sim = Sim::load(
        r#"
grid = [[0, 0, 0], [0, 0, 0]]

def set_cell(r, c, v):
    grid[r][c] = v

def main():
    set_cell(1, 2, 7)
    set_cell(0, 0, 3)
    total = 0
    for row in grid:
        for cell in row:
            total += cell
    return total * 100 + grid[1][2]
"#,
        &[],
    );
    assert_eq!(sim.call("main"), Some(1007));
}

#[test]
fn loops_over_a_string_parameter_give_its_characters() {
    let mut sim = Sim::load(
        r#"
def spell(word):
    for ch in word:
        /say {ch}

def main():
    spell("hey")
    spell(["x", "y"])
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(
        sim.log,
        [
            "Server: h",
            "Server: e",
            "Server: y",
            "Server: x",
            "Server: y"
        ]
    );
}
