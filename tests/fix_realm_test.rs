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
//! - recipes and advancement icons in pre-26.3 shapes and types 26.3 doesn't
//!   have passed the resource checks, and `/reload` then failed
//! - the validator called commands valid that Minecraft can't load: unknown
//!   ids, missing pack resources, bad text components, and bad values
//! - `tellraw @a "text {name}"` showed the quotes around the text, and a `"`
//!   in a string parameter broke the macro line
//! - `def giveReward():` passed `cobble check`, then failed `cobble build`
//!   with no source line
//! - map keys with a colon, a space, or a dot gave broken storage paths
//! - `return x > 3` and `a < b < c` were errors with no useful message, and a
//!   multi-line `if (...)` header was rejected
//! - `and` and `or` used as a value silently differed from Python
//! - a comment inside a `datapack.*_tag` list counted as a tag value

mod common;

use common::mcsim::{compile, compile_project, load_functions, Sim};
use std::fs;
use std::path::Path;
use std::process::Command;

#[test]
fn a_quoted_message_with_a_variable_shows_without_its_quotes() {
    let mut sim = Sim::load(
        r#"
title = "Boss"
count = 3

def show(name):
    /tellraw @a "Hello {name}"

def main():
    /tellraw @a "Plain"
    /tellraw @a "Fight {title} {count}"
    /tellraw @a 'Single {title}'
    /tellraw @a "A \"quoted\" {title}"
    show("Steve")
    show("say \"hi\"")
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(
        sim.log,
        [
            "Plain",
            "Fight Boss 3",
            "Single Boss",
            "A \"quoted\" Boss",
            "Hello Steve",
            "Hello say \"hi\""
        ]
    );
}

#[test]
fn a_function_name_with_capitals_is_an_error_at_its_line() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    fs::write(
        temp_dir.path().join("main.cbl"),
        "def main():\n    /say hi\n\ndef giveReward():\n    /say reward\n",
    )
    .unwrap();

    for command in ["check", "build"] {
        let output = Command::new(env!("CARGO_BIN_EXE_cobble"))
            .current_dir(temp_dir.path())
            .env("NO_COLOR", "1")
            .args([command, "main.cbl"])
            .output()
            .unwrap();
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!output.status.success(), "{command}: {text}");
        assert!(text.contains("give_reward"), "{command}: {text}");
        assert!(text.contains('4'), "{command}: {text}");
    }
}

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

#[test]
fn resource_shapes_and_types_minecraft_rejects_are_reported() {
    for (resource, expected) in [
        (
            r##"datapack.recipe("a", {"type": "minecraft:crafting_shaped", "pattern": ["#"], "key": {"#": {"item": "minecraft:stick"}}, "result": {"id": "minecraft:diamond"}})"##,
            "`key.#` is now the item id itself",
        ),
        (
            r#"datapack.recipe("a", {"type": "minecraft:crafting_shapeless", "ingredients": [{"tag": "minecraft:logs"}], "result": {"id": "minecraft:diamond"}})"#,
            "`ingredients[0]` is now the tag as a string",
        ),
        (
            r#"datapack.recipe("a", {"type": "minecraft:crafting_shapeless", "ingredients": ["minecraft:dirt"], "result": {"item": "minecraft:diamond"}})"#,
            "`result.item` is now `id`",
        ),
        (
            r#"datapack.recipe("a", {"type": "minecraft:crafting_bogus", "result": {"id": "minecraft:diamond"}})"#,
            "`minecraft:crafting_bogus` is not a recipe type",
        ),
        (
            r#"datapack.advancement("a", {"criteria": {"x": {"trigger": "minecraft:tick"}}, "parent": "minecraft:story/root", "display": {"icon": {"item": "minecraft:diamond"}, "title": "T", "description": "D"}})"#,
            "`display.icon.item` is now `id`",
        ),
        (
            r#"datapack.advancement("a", {"criteria": {"x": {"trigger": "minecraft:bogus"}}})"#,
            "`minecraft:bogus` is not an advancement trigger",
        ),
        (
            r#"datapack.item_modifier("a", {"type": "minecraft:set_nbt", "tag": "{a:1}"})"#,
            "(use `minecraft:set_custom_data`)",
        ),
        (
            r#"datapack.predicate("a", {"type": "minecraft:alternative", "terms": []})"#,
            "(use `minecraft:any_of`)",
        ),
        (
            r#"datapack.loot_table("a", {"type": "minecraft:chest", "pools": [{"rolls": 1, "entries": [{"type": "minecraft:bogus"}]}]})"#,
            "`minecraft:bogus` is not a loot entry type",
        ),
        (
            r#"datapack.loot_table("a", {"type": "minecraft:chest", "pools": [{"rolls": 1, "entries": [{"type": "minecraft:item", "name": "minecraft:stick", "modifier": [{"type": "minecraft:copy_nbt"}]}]}]})"#,
            "(use `minecraft:copy_custom_data`)",
        ),
    ] {
        let error = compile(&format!("from stdlib import datapack\n{resource}\n")).unwrap_err();
        assert!(error.contains(expected), "{resource}: {error}");
    }

    compile(
        r##"
from stdlib import datapack

datapack.recipe("shaped", {"type": "minecraft:crafting_shaped", "pattern": ["#"], "key": {"#": "minecraft:stick"}, "result": {"id": "minecraft:diamond", "count": 1}})
datapack.recipe("shapeless", {"type": "minecraft:crafting_shapeless", "ingredients": ["#minecraft:logs", ["minecraft:dirt", "minecraft:sand"]], "result": {"id": "minecraft:diamond"}})
datapack.recipe("smelt", {"type": "minecraft:smelting", "ingredient": "minecraft:dirt", "result": "minecraft:diamond", "cookingtime": 100})
datapack.advancement("tick", {"criteria": {"x": {"trigger": "tick"}}, "parent": "minecraft:story/root", "display": {"icon": {"id": "minecraft:diamond"}, "title": "T", "description": "D"}})
datapack.predicate("any", {"type": "minecraft:any_of", "terms": [{"type": "minecraft:random_chance", "chance": 0.5}]})
"##,
    )
    .unwrap_or_else(|error| panic!("build failed: {error}"));
}

#[test]
fn validator_reports_what_stops_a_function_from_loading() {
    let commands_json = Path::new("data/commands.json");
    if !commands_json.exists() {
        eprintln!("Skipping: data/commands.json not found");
        return;
    }
    let validator = cobble::validator::CommandValidator::from_file(commands_json).unwrap();
    let error = |command: &str| {
        validator
            .validate_command_detailed(command)
            .expect_err(command)
            .message
    };
    for (command, expected) in [
        ("give @s minecraft:diamnd", "Unknown item"),
        ("summon minecraft:zombi", "Unknown entity type"),
        ("kill @e[type=minecraft:zombi]", "Unknown entity type"),
        (
            "setblock ~ ~ ~ minecraft:oak_stairs[facing=up]",
            "no `facing=up`",
        ),
        ("effect give @s minecraft:sped", "Unknown effect"),
        ("particle minecraft:hartt ~ ~ ~", "Unknown particle"),
        ("enchant @s minecraft:sharpnes", "Unknown enchantment"),
        ("scoreboard objectives add x bogus", "scoreboard criterion"),
        (
            r#"tellraw @a {"text":"x","color":"pink"}"#,
            "`pink` is not a color",
        ),
        (
            r#"bossbar add a:b {"text":"x","bold":"y"}"#,
            "`bold` must be",
        ),
        ("tp @s ^ ~ ^", "mixes"),
        ("setblock ~ ~ 1.5 minecraft:stone", "must be a whole number"),
        (
            "execute if score @s a matches 5..1 run say x",
            "low end above",
        ),
        ("data get entity @s a..b", "not a valid NBT path"),
        ("kill @e[limit=1,limit=2]", "can appear once"),
    ] {
        let found = error(command);
        assert!(found.contains(expected), "{command}: {found}");
    }
    for command in [
        "item replace entity @s villager.7 with minecraft:stone",
        "item replace entity @s bogus.slot with minecraft:stone",
        "execute if items entity @s * minecraft:stone run say x",
    ] {
        assert!(validator.validate_command(command).is_err(), "{command}");
    }
    for command in [
        "give @s diamond",
        "give @s mymod:ruby",
        "$give @s minecraft:$(item)",
        "item replace entity @s mob.inventory.7 with minecraft:stone",
        "item replace entity @s ns:hands with minecraft:stone",
        r#"tellraw @a {"text":"x","color":"$(color)"}"#,
        "execute if items entity @s weapon.mainhand * run say x",
    ] {
        assert!(validator.validate_command(command).is_ok(), "{command}");
    }

    let (_temp, output_dir) = compile(
        r#"
def main():
    /loot give @s loot sim:missing
    /execute if predicate sim:gone run say x
    /loot give @s loot minecraft:chests/simple_dungeon
    /loot give @s loot other_pack:elsewhere
"#,
    )
    .unwrap_or_else(|error| panic!("build failed: {error}"));
    let report = validator.validate_datapack(&output_dir);
    let messages: Vec<&str> = report
        .errors
        .iter()
        .map(|(_, error)| error.message.as_str())
        .collect();
    assert_eq!(
        messages,
        [
            "Unknown loot table `sim:missing`: the pack doesn't define it",
            "Unknown predicate `sim:gone`: the pack doesn't define it",
        ]
    );
}

#[test]
fn map_keys_can_be_ids_and_names_with_spaces() {
    let mut sim = Sim::load(
        r#"
prices = {"minecraft:diamond": 100, "Iron Sword": 7, "a.b": 3}
shop = {"Iron Sword": {"cost": 5}}

def price(id):
    return prices[id]

def set_price(id, value):
    prices[id] = value

def main():
    a = prices["minecraft:diamond"]
    b = price("Iron Sword")
    c = prices["a.b"]
    set_price("new key", 11)
    d = price("new key")
    prices["a.b"] += 1
    e = price("a.b")
    f = shop["Iron Sword"]["cost"]
    n = len(prices)
    /tellraw @a ["{a} {b} {c} {d} {e} {f} {n}"]
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(sim.log, ["100 7 3 11 4 5 4"]);
}

#[test]
fn conditions_return_one_or_zero_and_comparisons_chain() {
    let mut sim = Sim::load(
        r#"
x = 5

def big():
    return x > 3

def small():
    return not (x > 3) or x == 0

def between(low, high):
    return low < x < high

def count_down():
    n = 8
    while 0 < n < 9:
        n -= 3
    return n

def main():
    a = big()
    b = small()
    c = between(1, 9)
    d = between(6, 9)
    e = count_down()
    f = 0
    if big() and 1 < x <= 5 < 6:
        f = 1
    if (x > 0
        and x < 9):
        f += 1
    /tellraw @a ["{a} {b} {c} {d} {e} {f}"]
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(sim.log, ["1 0 1 0 -1 2"]);
}

#[test]
fn a_chained_comparison_does_not_run_a_call_twice() {
    let Err(error) = Sim::try_load(
        r#"
def f():
    return 2

def main():
    a = 1 < f() < 3
    /say {a}
"#,
        &[],
    ) else {
        panic!("the call would run twice");
    };
    assert!(error.contains("`<` gives 1 or 0"), "{error}");
}

#[test]
fn python_operators_cobble_replaces_report_what_to_use() {
    let error = compile("def main():\n    a = 3\n    b = a // 2\n    c = a ** 2\n")
        .err()
        .unwrap();
    assert!(error.contains("`//` is not supported"), "{error}");
    assert!(error.contains("`**` is not supported"), "{error}");
    assert!(compile("def main():\n    /say 1 // 2 ** 3\n").is_ok());
}

#[test]
fn and_or_as_a_value_warn_for_plain_numbers() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let source = temp_dir.path().join("main.cbl");
    let warnings = |body: &str| {
        fs::write(
            &source,
            format!("def main():\n    a = 7\n{body}\n    /say {{x}}\n"),
        )
        .unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_cobble"))
            .current_dir(temp_dir.path())
            .args(["check", "main.cbl", "--json"])
            .output()
            .unwrap();
        assert!(output.status.success());
        String::from_utf8_lossy(&output.stdout).contains("logical-operator-value")
    };

    assert!(warnings("    x = a or 5"));
    assert!(warnings("    x = 0 or 5"));
    assert!(!warnings("    x = a > 1 or a < 0"));
    assert!(!warnings("    x = True and False"));
}

#[test]
fn comments_inside_multi_line_tag_lists_are_not_values() {
    let source = r##"from stdlib import datapack

datapack.block_tag("passable", [  # blocks a ray crosses
    # plain blocks
    "minecraft:air",  # the common case
    "#minecraft:leaves",  # a tag, with a # in the comment
])

def main():
    /say hi
"##;
    let (_temp_dir, output_dir) = compile(source).unwrap();
    let tag = fs::read_to_string(output_dir.join("data/sim/tags/block/passable.json")).unwrap();
    assert!(tag.contains("minecraft:air"), "{tag}");
    assert!(tag.contains("#minecraft:leaves"), "{tag}");
}

#[test]
fn names_outside_ascii_report_that_names_are_ascii() {
    let error = compile("점수 = 0\n").err().unwrap();
    assert!(error.contains("Names use ASCII letters"), "{error}");
}
