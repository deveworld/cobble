//! Regressions for pack behavior found by re-reading the generated output:
//! - the runtime setup handler runs before other `minecraft:load` entries
//! - raw `unless A or B` headers compile to `unless A unless B`
//! - `say` text with variables keeps `say`'s own output
//! - only packs with a runtime loop raise `max_command_sequence_length`
//! - entity templates apply the selector's `tag=` values
//! - compiler errors point at the statement that caused them
//! - `--zip` refuses a world's `datapacks` folder

mod common;

use common::mcsim::{compile, load_functions, Sim};
use std::fs;
use std::path::Path;
use std::process::Command;

fn cobble() -> Command {
    Command::new(env!("CARGO_BIN_EXE_cobble"))
}

fn all_commands(output_dir: &Path) -> String {
    let mut functions: Vec<_> = load_functions(output_dir).into_iter().collect();
    functions.sort();
    functions
        .into_iter()
        .map(|(name, commands)| format!("# {name}\n{}\n", commands.join("\n")))
        .collect()
}

// ---------------------------------------------------------------------------
// load order
// ---------------------------------------------------------------------------

#[test]
fn load_handler_with_runtime_setup_runs_before_function_tag_entries() {
    // `a_setup` sorts before `on_load`, which creates the objective and
    // initializes `count`. Run first, `a_setup` would hit a missing objective
    // (the simulator panics) and `on_load` would then reset `count` to 0.
    let mut sim = Sim::load(
        r#"from stdlib import event, datapack

count = 0

def a_setup():
    /scoreboard players add count temp 1

def on_load():
    /say loaded

datapack.function_tag("minecraft:load", ["sim:a_setup"])
stdlib.addEventListener(event.LOAD, on_load)
"#,
        &[],
    );
    assert_eq!(sim.var("count"), 1);
    sim.reload();
    assert_eq!(sim.var("count"), 1);
}

// ---------------------------------------------------------------------------
// raw unless with or
// ---------------------------------------------------------------------------

#[test]
fn raw_unless_or_runs_only_when_no_alternative_matches() {
    let source = r#"from stdlib import event

def setup():
    /scoreboard objectives add flags dummy

stdlib.addEventListener(event.LOAD, setup)

def check():
    unless score fa flags matches 1 or score fb flags matches 1:
        /say neither
"#;
    let mut sim = Sim::load(source, &[]);
    assert!(
        sim.function_text("check")
            .contains("execute unless score fa flags matches 1 unless score fb flags matches 1 run say neither"),
        "{}",
        sim.function_text("check")
    );

    sim.set_score("fa", "flags", 0);
    sim.set_score("fb", "flags", 0);
    sim.call("check");
    assert_eq!(sim.log, vec!["Server: neither"]);

    sim.log.clear();
    sim.set_score("fb", "flags", 1);
    sim.call("check");
    assert!(sim.log.is_empty());

    sim.set_score("fa", "flags", 1);
    sim.set_score("fb", "flags", 0);
    sim.call("check");
    assert!(sim.log.is_empty());
}

#[test]
fn raw_unless_with_and_inside_or_alternative_keeps_precedence() {
    // unless ((A and B) or C) = unless C, and not (A and B).
    let source = r#"from stdlib import event

def setup():
    /scoreboard objectives add flags dummy

stdlib.addEventListener(event.LOAD, setup)

def check():
    unless score fa flags matches 1 and score fb flags matches 1 or score fc flags matches 1:
        /say pass
"#;
    let mut sim = Sim::load(source, &[]);
    for (a, b, c, expected) in [
        (0, 0, 0, true),
        (1, 0, 0, true),
        (1, 1, 0, false),
        (0, 0, 1, false),
        (1, 1, 1, false),
    ] {
        sim.log.clear();
        sim.set_score("fa", "flags", a);
        sim.set_score("fb", "flags", b);
        sim.set_score("fc", "flags", c);
        sim.call("check");
        assert_eq!(
            !sim.log.is_empty(),
            expected,
            "a={a} b={b} c={c}: {:?}",
            sim.log
        );
    }
}

// ---------------------------------------------------------------------------
// say with variables
// ---------------------------------------------------------------------------

#[test]
fn say_with_variables_runs_as_say_for_the_executor() {
    let mut sim = Sim::load(
        r#"count = 3
name = "Steve"

def show():
    /say count is {count}, name {name}

def everyone():
    as @a:
        /say hi {count}
"#,
        &["alex", "steve"],
    );

    sim.call_as("show", "alex");
    assert_eq!(sim.log, vec!["alex: count is 3, name Steve"]);
    assert!(!sim.function_text("show").contains("tellraw"));

    sim.log.clear();
    sim.call("everyone");
    assert_eq!(sim.log, vec!["alex: hi 3", "steve: hi 3"]);
}

#[test]
fn say_with_variables_and_parameters_copies_both() {
    let mut sim = Sim::load(
        r#"def tell(n):
    total = n + 1
    /say got {total} from {n} and $(n)

def run():
    tell(5)
"#,
        &[],
    );
    sim.call("run");
    assert_eq!(sim.log, vec!["Server: got 6 from 5 and 5"]);
}

#[test]
fn say_with_escaped_braces_keeps_them_literal() {
    let mut sim = Sim::load(
        r#"count = 2

def show():
    /say {{count}} is {count}
"#,
        &[],
    );
    sim.call("show");
    assert_eq!(sim.log, vec!["Server: {count} is 2"]);
}

#[test]
fn say_without_variables_is_unchanged() {
    let (_temp, output_dir) = compile(
        r#"def show():
    /say plain text
"#,
    )
    .unwrap();
    let functions = load_functions(&output_dir);
    assert_eq!(functions["show"], vec!["say plain text"]);
    assert!(!functions
        .keys()
        .any(|name| name.starts_with("_cobble_say_")));
}

// ---------------------------------------------------------------------------
// max_command_sequence_length
// ---------------------------------------------------------------------------

#[test]
fn packs_without_runtime_loops_leave_the_command_limit_alone() {
    let (_temp, output_dir) = compile(
        r#"from stdlib import event

score = 1

def on_load():
    /gamerule max_command_sequence_length 1000000000
    /say loaded

def show():
    for i in range(3):
        /say {i}

stdlib.addEventListener(event.LOAD, on_load)
"#,
    )
    .unwrap();
    // The user's own gamerule line stays, and Cobble adds no other one.
    let commands = all_commands(&output_dir);
    assert_eq!(commands.matches("gamerule ").count(), 1, "{commands}");
    let on_load = &load_functions(&output_dir)["on_load"];
    let gamerule = on_load
        .iter()
        .position(|command| command == "gamerule max_command_sequence_length 1000000000");
    let say = on_load.iter().position(|command| command == "say loaded");
    assert!(gamerule.is_some() && gamerule < say, "{on_load:?}");
}

#[test]
fn packs_with_runtime_loops_raise_the_command_limit_first() {
    for source in [
        "def count_down(n):\n    i = n\n    while i > 0:\n        i -= 1\n",
        "def total(limit):\n    sum = 0\n    for i in range(limit):\n        sum += i\n",
    ] {
        let (_temp, output_dir) = compile(source).unwrap();
        let functions = load_functions(&output_dir);
        let init = &functions["_cobble_init"];
        assert_eq!(
            init.first().map(String::as_str),
            Some("gamerule max_command_sequence_length 1000000000"),
            "{source}\n{init:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// entity templates
// ---------------------------------------------------------------------------

#[test]
fn entity_templates_add_selector_tags_to_the_summon() {
    let (_temp, output_dir) = compile(
        r#"define @Guard = @e[type=zombie,tag=guard,tag=!retired,tag=,scores={hp=1..}]
create {"NoAI": True}
end

define @Keeper = @e[type=villager,tag=shop]
create {"Tags": ["shop", "extra"]}
end

def spawn():
    create @Guard
    create @Keeper
"#,
    )
    .unwrap();
    let functions = load_functions(&output_dir);
    assert_eq!(
        functions["spawn"],
        vec![
            r#"summon minecraft:zombie ~ ~ ~ {NoAI:1b,Tags:["guard"]}"#,
            r#"summon minecraft:villager ~ ~ ~ {Tags:["shop","extra"]}"#,
        ]
    );
}

// ---------------------------------------------------------------------------
// error locations
// ---------------------------------------------------------------------------

fn check_json(input: &Path) -> serde_json::Value {
    let output = cobble()
        .arg("check")
        .arg("--json")
        .arg(input)
        .output()
        .unwrap();
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "check --json did not print JSON ({error}): {}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

#[test]
fn compiler_errors_point_at_the_failing_statement() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = temp_dir.path().join("main.cbl");
    fs::write(
        &input,
        "n = 3\n\ndef f():\n    /say ok\n    if n > 1:\n        /give @a minecraft:diamond {n}\n",
    )
    .unwrap();

    let report = check_json(&input);
    let diagnostic = &report["diagnostics"][0];
    assert_eq!(diagnostic["line"], 6, "{report}");
    assert_eq!(diagnostic["column"], 9, "{report}");
    let message = diagnostic["message"].as_str().unwrap();
    assert!(
        message.contains("\n    def show_value(val):\n        /command {val}\n    show_value(n)"),
        "the example keeps its indentation: {message}"
    );

    let output = cobble()
        .arg("build")
        .arg(&input)
        .arg("-o")
        .arg(temp_dir.path().join("out"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Transpilation failed at ") && stderr.contains("main.cbl:6:9: "),
        "{stderr}"
    );
}

#[test]
fn errors_in_imported_files_point_at_the_imported_file() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let main = temp_dir.path().join("main.cbl");
    fs::write(&main, "import helper\n\ndef main():\n    /say ok\n").unwrap();
    fs::write(
        temp_dir.path().join("helper.cbl"),
        "n = 3\n\n\n\ndef f():\n    /give @a minecraft:diamond {n}\n",
    )
    .unwrap();

    let report = check_json(&main);
    let diagnostic = &report["diagnostics"][0];
    assert!(
        diagnostic["message"]
            .as_str()
            .unwrap()
            .contains("Cannot interpolate"),
        "{report}"
    );
    assert!(
        diagnostic["file"].as_str().unwrap().ends_with("helper.cbl"),
        "{report}"
    );
    assert_eq!(diagnostic["line"], 6, "{report}");
    assert_eq!(report["diagnostics"].as_array().unwrap().len(), 1);
}

// ---------------------------------------------------------------------------
// --zip into datapacks
// ---------------------------------------------------------------------------

#[test]
fn zip_build_refuses_a_worlds_datapacks_folder() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = temp_dir.path().join("main.cbl");
    fs::write(&input, "def main():\n    /say hi\n").unwrap();
    let datapacks = temp_dir.path().join("world/datapacks");
    fs::create_dir_all(&datapacks).unwrap();
    fs::write(temp_dir.path().join("world/level.dat"), b"").unwrap();

    let output = cobble()
        .arg("build")
        .arg(&input)
        .arg("--zip")
        .arg("-o")
        .arg(datapacks.join("pack"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--zip cannot write into a world's datapacks folder"),
        "{stderr}"
    );
    assert_eq!(fs::read_dir(&datapacks).unwrap().count(), 0);

    // Outside a world, even in a folder named `datapacks`, --zip still
    // writes the folder and the ZIP.
    let output = cobble()
        .arg("build")
        .arg(&input)
        .arg("--zip")
        .arg("-o")
        .arg(temp_dir.path().join("dist/datapacks/pack"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(temp_dir.path().join("dist/datapacks/cobble.zip").exists());
}

// ---------------------------------------------------------------------------
// command tree cache
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
#[test]
fn default_command_tree_is_cached_outside_the_current_folder() {
    let repo_tree = Path::new(env!("CARGO_MANIFEST_DIR")).join("data/commands.json");
    if !repo_tree.exists() {
        eprintln!("skipping: data/commands.json is not generated");
        return;
    }
    let temp_dir = tempfile::TempDir::new().unwrap();
    let work_dir = temp_dir.path().join("work");
    let cache_dir = temp_dir.path().join("cache");
    fs::create_dir_all(&work_dir).unwrap();
    fs::write(work_dir.join("main.cbl"), "def main():\n    /say hi\n").unwrap();

    let output = cobble()
        .current_dir(&work_dir)
        .env("XDG_CACHE_HOME", &cache_dir)
        .env(
            "COBBLE_COMMANDS_JSON_URL",
            format!("file://{}", repo_tree.display()),
        )
        .env_remove("COBBLE_COMMANDS_JSON_AUTO_DOWNLOAD")
        .args(["build", "main.cbl", "--validate", "-o", "out1"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let cached = cache_dir.join("cobble/26.3/commands.json");
    assert!(cached.exists());
    assert!(!work_dir.join("data").exists());

    // The second build reuses the cached tree; fetching is turned off.
    let output = cobble()
        .current_dir(&work_dir)
        .env("XDG_CACHE_HOME", &cache_dir)
        .env("COBBLE_COMMANDS_JSON_AUTO_DOWNLOAD", "0")
        .args(["build", "main.cbl", "--validate", "-o", "out2"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Validation: "), "{stdout}");
}

// ---------------------------------------------------------------------------
// cobble migrate
// ---------------------------------------------------------------------------

#[test]
fn migrate_to_0_11_reports_lines_whose_output_changes() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let source_dir = temp_dir.path().join("src");
    fs::create_dir_all(&source_dir).unwrap();
    fs::write(
        source_dir.join("main.cbl"),
        r#"from stdlib import event, datapack, resource_pack

count = 0

define @Guard = @e[type=zombie,tag=guard]
create {"NoAI": True}
end

define @Any = @e[type=zombie,tag=!guard]
create {"NoAI": True}
end

def show():
    /say count {count}
    /say escaped {{count}}
    /say plain
    # /say {count} in a comment
    /scoreboard players add count temp 1

datapack.function_tag("minecraft:load", ["main:show"])
resource_pack.item_model("main:wand", {"parent": "minecraft:item/handheld"})
"#,
    )
    .unwrap();

    let output = cobble()
        .arg("migrate")
        .arg(temp_dir.path())
        .arg("--json")
        .output()
        .unwrap();
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let mut found = Vec::new();
    fn walk(value: &serde_json::Value, found: &mut Vec<(u64, u64, String)>) {
        match value {
            serde_json::Value::Object(map) => {
                if let (Some(line), Some(column), Some(kind)) = (
                    map.get("line").and_then(|v| v.as_u64()),
                    map.get("column").and_then(|v| v.as_u64()),
                    map.get("kind").and_then(|v| v.as_str()),
                ) {
                    found.push((line, column, kind.to_string()));
                }
                map.values().for_each(|v| walk(v, found));
            }
            serde_json::Value::Array(items) => items.iter().for_each(|v| walk(v, found)),
            _ => {}
        }
    }
    walk(&report, &mut found);
    found
        .retain(|(_, _, kind)| kind != "stdlib-module-import" && kind != "resource-pack-reference");
    found.sort();
    assert_eq!(
        found,
        vec![
            (5, 1, "entity-template-tag".to_string()),
            (14, 5, "say-variable-text".to_string()),
            (18, 35, "temp-objective-reference".to_string()),
            (20, 1, "load-function-tag".to_string()),
            (21, 1, "item-model-definition".to_string()),
        ],
        "{report}"
    );
}

#[test]
fn migrate_to_0_11_reports_module_variables_that_become_function_locals() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let source_dir = temp_dir.path().join("src");
    fs::create_dir_all(&source_dir).unwrap();
    fs::write(
        source_dir.join("main.cbl"),
        r#"score = 0
rounds = 0

def add():
    /say hi
    score += 5

def keep():
    global rounds
    rounds += 1

def count():
    for score in range(3):
        rounds = score
"#,
    )
    .unwrap();

    let output = cobble()
        .arg("migrate")
        .arg(temp_dir.path())
        .arg("--json")
        .output()
        .unwrap();
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let mut found = Vec::new();
    fn walk(value: &serde_json::Value, found: &mut Vec<(u64, u64, String)>) {
        match value {
            serde_json::Value::Object(map) => {
                if map.get("kind").and_then(|v| v.as_str()) == Some("function-local-variable") {
                    found.push((
                        map["line"].as_u64().unwrap(),
                        map["column"].as_u64().unwrap(),
                        map["message"].as_str().unwrap().to_string(),
                    ));
                }
                map.values().for_each(|v| walk(v, found));
            }
            serde_json::Value::Array(items) => items.iter().for_each(|v| walk(v, found)),
            _ => {}
        }
    }
    walk(&report, &mut found);
    found.sort();
    let locations: Vec<_> = found
        .iter()
        .map(|(line, column, _)| (*line, *column))
        .collect();
    assert_eq!(locations, [(6, 5), (13, 5), (14, 9)], "{report}");
    assert!(found[0].2.starts_with("`add` assigns `score`"), "{report}");
    assert!(
        found[1].2.starts_with("`count` assigns `score`"),
        "{report}"
    );
    assert!(
        found[2].2.starts_with("`count` assigns `rounds`"),
        "{report}"
    );
}

// ---------------------------------------------------------------------------
// review follow-ups
// ---------------------------------------------------------------------------

#[test]
fn quoted_or_and_and_stay_inside_their_condition() {
    let (_temp, output_dir) = compile(
        r#"def f():
    unless entity @a[name="Tom or Jerry"]:
        /say nobody
    if entity @a[name="Tom and Jerry"]:
        /say pair
"#,
    )
    .unwrap();
    let f = &load_functions(&output_dir)["f"];
    assert_eq!(
        f,
        &vec![
            r#"execute unless entity @a[name="Tom or Jerry"] run say nobody"#.to_string(),
            r#"execute if entity @a[name="Tom and Jerry"] run say pair"#.to_string(),
        ]
    );
}

#[test]
fn raw_if_or_with_and_alternative_runs_the_whole_chain() {
    let source = r#"from stdlib import event

def setup():
    /scoreboard objectives add flags dummy

stdlib.addEventListener(event.LOAD, setup)

def check():
    if score fa flags matches 1 and score fb flags matches 1 or score fc flags matches 1:
        /say pass
"#;
    let mut sim = Sim::load(source, &[]);
    for (a, b, c, expected) in [
        (0, 0, 0, false),
        (1, 0, 0, false),
        (1, 1, 0, true),
        (0, 0, 1, true),
    ] {
        sim.log.clear();
        sim.set_score("fa", "flags", a);
        sim.set_score("fb", "flags", b);
        sim.set_score("fc", "flags", c);
        sim.call("check");
        assert_eq!(!sim.log.is_empty(), expected, "a={a} b={b} c={c}");
    }
}

#[test]
fn user_load_functions_keep_their_order_after_the_setup() {
    // `on_load` uses an objective that `a_objectives` creates; both keep the
    // sorted order they had in 0.10, behind `_cobble_init`.
    let sim = Sim::load(
        r#"from stdlib import event, datapack

count = 0

def a_objectives():
    /scoreboard objectives add flags dummy

def on_load():
    /scoreboard players set started flags 1

datapack.function_tag("minecraft:load", ["sim:a_objectives"])
stdlib.addEventListener(event.LOAD, on_load)
"#,
        &[],
    );
    assert_eq!(sim.score("started", "flags"), 1);
    let (_temp, output_dir) = compile(
        r#"from stdlib import event, datapack
count = 0
def a_objectives():
    /scoreboard objectives add flags dummy
def on_load():
    /say hi
datapack.function_tag("minecraft:load", ["sim:a_objectives"])
stdlib.addEventListener(event.LOAD, on_load)
"#,
    )
    .unwrap();
    let tag: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(output_dir.join("data/minecraft/tags/function/load.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        tag["values"],
        serde_json::json!(["sim:_cobble_init", "sim:a_objectives", "sim:on_load"])
    );
    assert_eq!(load_functions(&output_dir)["on_load"], vec!["say hi"]);
}

#[test]
fn runtime_power_raises_the_command_limit() {
    let (_temp, output_dir) = compile("def f(e):\n    base = 2\n    x = base ^ e\n").unwrap();
    assert_eq!(
        load_functions(&output_dir)["_cobble_init"]
            .first()
            .map(String::as_str),
        Some("gamerule max_command_sequence_length 1000000000")
    );
}

#[test]
fn store_block_around_say_with_a_variable_stores_says_result() {
    let mut sim = Sim::load(
        r#"count = 3
out = 0

def f():
    global out
    store result score out temp:
        /say value {count}
"#,
        &[],
    );
    sim.call("f");
    assert_eq!(sim.log, vec!["Server: value 3"]);
    assert_eq!(sim.var("out"), 1);
}

#[test]
fn hand_written_macro_argument_keeps_the_tellraw_form() {
    let (_temp, output_dir) = compile(
        r#"count = 1

def ext():
    /say external $(x) and {count}
"#,
    )
    .unwrap();
    let functions = load_functions(&output_dir);
    let ext = functions["ext"].join("\n");
    assert!(ext.starts_with("$tellraw @a "), "{ext}");
    assert!(!ext.contains("_say_args"), "{ext}");
}

#[test]
fn unrolled_commands_keep_their_own_source_locations() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = temp_dir.path().join("main.cbl");
    fs::write(
        &input,
        "count = 1\ndef f():\n    for i in range(2):\n        /give @a minecraft:diamond {count}\ndef g():\n    /give @a minecraft:diamond {count}\n",
    )
    .unwrap();
    let report = check_json(&input);
    assert_eq!(report["diagnostics"][0]["line"], 4, "{report}");
    assert_eq!(report["diagnostics"][0]["column"], 9, "{report}");

    fs::write(
        &input,
        "count = 1\ndef f():\n    for i in range(2):\n        /say a {i}\n        /say u {i} {count}\ndef g():\n    /say a 0\n",
    )
    .unwrap();
    let output_dir = temp_dir.path().join("out");
    let output = cobble()
        .arg("build")
        .arg(&input)
        .arg("-o")
        .arg(&output_dir)
        .output()
        .unwrap();
    assert!(output.status.success());
    let source_map: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(output_dir.join(".cobble/source_map.json")).unwrap(),
    )
    .unwrap();
    let line_of = |path_end: &str, command: &str| {
        source_map["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| {
                entry["generated_path"]
                    .as_str()
                    .unwrap()
                    .ends_with(path_end)
                    && entry["command"] == command
            })
            .and_then(|entry| entry["source"]["line"].as_u64())
    };
    assert_eq!(line_of("/f.mcfunction", "say a 0"), Some(4));
    assert_eq!(line_of("/g.mcfunction", "say a 0"), Some(7));
    assert_eq!(
        line_of("/_cobble_say_0.mcfunction", "$say u 0 $(count)"),
        Some(5)
    );
}
