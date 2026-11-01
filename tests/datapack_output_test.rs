//! Regressions for how `datapack.*` declarations are written:
//! - function tags keep the declared order (bugs 55, 67) and dialog tags
//!   merge like the other typed tags (72)
//! - numbers in resource JSON keep their value (58, 71) and their source map
//!   entry (61), and exponent literals work
//! - continued raw commands keep their source location (60)
//! - an identifier as a resource name uses its value (70), and unsupported
//!   JSON values are named in the language's terms (80)

mod common;

use common::mcsim::{compile, Sim};
use std::fs;
use std::path::Path;
use std::process::Command;

fn cobble() -> Command {
    Command::new(env!("CARGO_BIN_EXE_cobble"))
}

fn read_json(output_dir: &Path, path: &str) -> serde_json::Value {
    let path = output_dir.join(path);
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap()
}

fn compile_error(source: &str) -> String {
    match compile(source) {
        Ok(_) => panic!("expected a compile error for:\n{source}"),
        Err(error) => error,
    }
}

// ---------------------------------------------------------------------------
// Function and dialog tags
// ---------------------------------------------------------------------------

#[test]
fn function_tags_keep_the_declared_order() {
    let (_temp, output) = compile(
        r#"
from stdlib import datapack

datapack.function_tag("order", ["sim:zebra", "sim:apple"])

def zebra():
    /say zebra

def apple():
    /say apple
"#,
    )
    .unwrap();
    assert_eq!(
        read_json(&output, "data/sim/tags/function/order.json")["values"],
        serde_json::json!(["sim:zebra", "sim:apple"])
    );
}

#[test]
fn load_tag_runs_in_declared_order_after_the_runtime_setup() {
    let mut sim = Sim::load(
        r#"
from stdlib import datapack

datapack.function_tag("minecraft:load", ["sim:z_early", "sim:a_late"])
datapack.function_tag("minecraft:load", ["sim:m_last", "sim:z_early"])

counter = 5

def z_early():
    /say z_early

def a_late():
    /say a_late

def m_last():
    /say m_last
"#,
        &[],
    );
    assert_eq!(
        sim.log,
        vec!["Server: z_early", "Server: a_late", "Server: m_last"]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>()
    );
    assert_eq!(sim.var("counter"), 5);
    sim.log.clear();
    sim.reload();
    assert_eq!(sim.log.len(), 3);
}

#[test]
fn load_tag_keeps_init_first_then_listeners_and_entries_in_source_order() {
    let (_temp, output) = compile(
        r#"
from stdlib import datapack, event
import stdlib

datapack.function_tag("minecraft:load", ["sim:b", "sim:a", "sim:b"])
datapack.function_tag("minecraft:tick", ["sim:a"])

counter = 1

def a():
    /say a

def b():
    /say b

def on_load():
    /say load

stdlib.addEventListener(event.LOAD, on_load)
datapack.function_tag("minecraft:load", ["sim:a", "sim:on_load", "sim:c"])

def c():
    /say c

def on_tick():
    /say tick

stdlib.addEventListener(event.TICK, on_tick)
"#,
    )
    .unwrap();
    assert_eq!(
        read_json(&output, "data/minecraft/tags/function/load.json")["values"],
        serde_json::json!(["sim:_cobble_init", "sim:b", "sim:a", "sim:on_load", "sim:c"])
    );
    assert_eq!(
        read_json(&output, "data/minecraft/tags/function/tick.json")["values"],
        serde_json::json!(["sim:a", "sim:on_tick"])
    );
}

#[test]
fn dialog_tags_merge_like_the_other_typed_tags() {
    let (_temp, output) = compile(
        r#"
from stdlib import datapack

datapack.dialog("n1", {"type": "minecraft:notice", "title": "a"})
datapack.dialog("n2", {"type": "minecraft:notice", "title": "b"})
datapack.dialog_tag("dt", [{"id": "sim:n2", "required": True}, "sim:n2"])
datapack.dialog_tag("dt", ["sim:n1"])
"#,
    )
    .unwrap();
    assert_eq!(
        read_json(&output, "data/sim/tags/dialog/dt.json")["values"],
        serde_json::json!(["sim:n1", "sim:n2"])
    );
}

// ---------------------------------------------------------------------------
// Numbers in resource JSON
// ---------------------------------------------------------------------------

/// The `tag` of an item modifier declared with `tag`, as written.
fn custom_data(tag: &str) -> Result<serde_json::Value, String> {
    let source = format!(
        "from stdlib import datapack\n\ndatapack.item_modifier(\"m\", {{\"type\": \"minecraft:set_custom_data\", \"tag\": {tag}}})\n"
    );
    let (_temp, output) = compile(&source)?;
    Ok(read_json(&output, "data/sim/item_modifier/m.json")["tag"].clone())
}

#[test]
fn whole_numbers_beyond_32_bits_keep_their_value() {
    let tag = custom_data(
        r#"{"a": 3000000000, "b": 2147483648, "c": 3000000000.0, "d": 9007199254740993, "e": -9223372036854775808, "f": -2147483649, "g": 3000000000.5, "h": 18446744073709551615, "i": -(2), "j": 2147483647}"#,
    )
    .unwrap();
    assert_eq!(tag["a"].as_i64(), Some(3_000_000_000));
    assert_eq!(tag["b"].as_i64(), Some(2_147_483_648));
    assert_eq!(tag["c"].as_i64(), Some(3_000_000_000));
    assert_eq!(tag["d"].as_i64(), Some(9_007_199_254_740_993));
    assert_eq!(tag["e"].as_i64(), Some(i64::MIN));
    assert_eq!(tag["f"].as_i64(), Some(-2_147_483_649));
    assert_eq!(tag["g"].as_f64(), Some(3_000_000_000.5));
    assert_eq!(tag["h"].as_u64(), Some(u64::MAX));
    assert_eq!(tag["i"].as_i64(), Some(-2));
    assert_eq!(tag["j"].as_i64(), Some(2_147_483_647));
}

#[test]
fn numbers_json_cannot_hold_are_compile_errors() {
    for (tag, expected) in [
        (r#"{"a": 99999999999999999999}"#, "doesn't fit in 64 bits"),
        (r#"{"a": 18446744073709551616}"#, "doesn't fit in 64 bits"),
        (r#"{"a": -9223372036854775809}"#, "doesn't fit in 64 bits"),
        (r#"{"a": 1e999}"#, "JSON has no infinity"),
        (
            r#"{"a": 9007199254740993, "b": 9007199254740992}"#,
            "can't keep them apart",
        ),
    ] {
        let error = custom_data(tag).unwrap_err();
        assert!(error.contains(expected), "{tag}: {error}");
        assert!(error.contains("main.cbl:3:1"), "{tag}: {error}");
    }
}

#[test]
fn a_const_beyond_2_53_is_a_compile_error_not_a_rounded_number() {
    let error = compile_error(
        "from stdlib import datapack\nconst BIG = 9007199254740993\ndatapack.item_modifier(\"m\", {\"type\": \"minecraft:set_custom_data\", \"tag\": {\"a\": BIG}})\n",
    );
    assert!(error.contains("past 2^53"), "{error}");
}

#[test]
fn exponent_literals_work_in_resource_json_and_code() {
    let tag = custom_data(
        r#"{"a": 1e3, "b": 1E3, "c": 1e-3, "d": 1e+3, "e": 2.5e2, "f": 1.7976931348623157E308, "g": 1e20, "h": -1e3}"#,
    )
    .unwrap();
    assert_eq!(tag["a"].as_i64(), Some(1000));
    assert_eq!(tag["b"].as_i64(), Some(1000));
    assert_eq!(tag["c"].as_f64(), Some(0.001));
    assert_eq!(tag["d"].as_i64(), Some(1000));
    assert_eq!(tag["e"].as_i64(), Some(250));
    assert_eq!(tag["f"].as_f64(), Some(f64::MAX));
    assert_eq!(tag["g"].as_f64(), Some(1e20));
    assert_eq!(tag["h"].as_i64(), Some(-1000));

    let mut sim = Sim::load(
        r#"
x = 1e3
y = 2E+2
z = 0

def main():
    global z
    z = 5e1 + x
"#,
        &[],
    );
    assert_eq!(sim.var("x"), 1000);
    assert_eq!(sim.var("y"), 200);
    sim.call("main");
    assert_eq!(sim.var("z"), 1050);
}

#[test]
fn json_resources_keep_their_source_map_entry_whatever_the_number_text() {
    let (_temp, output) = compile(
        r#"from stdlib import datapack

datapack.predicate("p1", {"type":"minecraft:random_chance","chance":0.5})
datapack.predicate("p2", {"type":"minecraft:random_chance","chance":0.50})
datapack.predicate("p3", {"type":"minecraft:random_chance","chance":1.0})
datapack.predicate("p4", {"type":"minecraft:random_chance","chance":5e-1})
"#,
    )
    .unwrap();
    let source_map = read_json(&output, ".cobble/source_map.json");
    for (name, line) in [("p1", 3), ("p2", 4), ("p3", 5), ("p4", 6)] {
        let path = format!("data/sim/predicate/{name}.json");
        let entry = source_map["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["generated_path"] == path.as_str())
            .unwrap_or_else(|| panic!("no entry for {path}"));
        assert_eq!(entry["source"]["line"], line, "{name}: {entry}");
    }
}

// ---------------------------------------------------------------------------
// Continued raw commands
// ---------------------------------------------------------------------------

const CONTINUED: &str =
    "def a():\n    /say a \\\n        b\n    /say c\n    /kill @e[x=1,x=2] \\\n        @s\n";

#[test]
fn continued_raw_commands_keep_their_source_location() {
    let (_temp, output) = compile(CONTINUED).unwrap();
    let source_map = read_json(&output, ".cobble/source_map.json");
    let line_of = |command: &str| {
        source_map["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["command"] == command)
            .unwrap_or_else(|| panic!("no entry for {command}"))["source"]["line"]
            .clone()
    };
    assert_eq!(line_of("say a b"), 2);
    assert_eq!(line_of("say c"), 4);
    assert_eq!(line_of("kill @e[x=1,x=2] @s"), 5);
}

#[test]
fn build_validate_names_the_output_folder_and_the_source_line() {
    if !Path::new("data/commands.json").exists() {
        eprintln!("skipping: data/commands.json is not generated");
        return;
    }
    let temp = tempfile::TempDir::new().unwrap();
    let input = temp.path().join("main.cbl");
    fs::write(&input, CONTINUED).unwrap();
    let output = temp.path().join("out");
    let result = cobble()
        .args(["build", "--namespace", "sim", "--validate", "-q", "-o"])
        .arg(&output)
        .arg(&input)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(!result.status.success());
    assert!(!stderr.contains("cobble-staging"), "{stderr}");
    assert!(
        stderr.contains(&format!(
            "{}:3: Selector argument `x` can appear once",
            output.join("data/sim/function/a.mcfunction").display()
        )),
        "{stderr}"
    );
    assert!(stderr.contains("main.cbl:5:5"), "{stderr}");
}

// ---------------------------------------------------------------------------
// Resource names and unsupported JSON values
// ---------------------------------------------------------------------------

#[test]
fn an_identifier_as_a_resource_name_is_a_compile_error() {
    let error = compile_error(
        r#"from stdlib import datapack

checks = "real_name"
datapack.predicate(checks, {"type": "minecraft:random_chance", "chance": 1})
"#,
    );
    assert!(
        error.contains("Identifier 'checks' cannot be used as the resource name"),
        "{error}"
    );
    assert!(error.contains("main.cbl:4:1"), "{error}");
    let error = compile_error(
        r#"from stdlib import datapack

name = "a"
datapack.block_tag(name, ["minecraft:stone"])
"#,
    );
    assert!(
        error.contains("Identifier 'name' cannot be used as the tag name"),
        "{error}"
    );
}

#[test]
fn arithmetic_in_resource_json_is_named_in_cobble_terms() {
    for (value, shown) in [("1 / 2", "`1/2`"), ("x * 2", "`x*2`"), ("-x", "`-x`")] {
        let error = compile_error(&format!(
            "from stdlib import datapack\n\nx = 1\ndatapack.predicate(\"a\", {{\"type\": \"minecraft:random_chance\", \"chance\": {value}}})\n"
        ));
        assert!(error.contains(shown), "{value}: {error}");
        assert!(
            error.contains("write the result as a literal"),
            "{value}: {error}"
        );
        assert!(
            !error.contains("Binary(") && !error.contains("Unary("),
            "{error}"
        );
    }
    let (_temp, output) = compile(
        "from stdlib import datapack\n\ndatapack.item_modifier(\"a\", {\"type\": \"minecraft:set_name\", \"name\": \"x\" + \"y\"})\n",
    )
    .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        read_json(&output, "data/sim/item_modifier/a.json")["name"],
        "xy"
    );
}

#[test]
fn a_float_literal_of_a_whole_number_past_2_53_keeps_its_digits() {
    // Bugs 58 and 71: Minecraft reads these as the longs they spell.
    let tag = custom_data(
        r#"{"a": 9007199254740993.0, "b": 9.007199254740993e15, "c": 3000000000.5, "d": 1e300}"#,
    )
    .unwrap();
    assert_eq!(tag["a"].as_i64(), Some(9_007_199_254_740_993));
    assert_eq!(tag["b"].as_i64(), Some(9_007_199_254_740_993));
    assert_eq!(tag["c"].as_f64(), Some(3_000_000_000.5));
    assert_eq!(tag["d"].as_f64(), Some(1e300));
}

#[test]
fn numbers_a_score_cannot_hold_in_list_and_map_literals_warn() {
    // Bugs 58 and 71: they were clamped without a word, and a const past a
    // score's range was written as SNBT Minecraft refuses.
    let temp = tempfile::TempDir::new().unwrap();
    let file = temp.path().join("main.cbl");
    fs::write(
        &file,
        "m = {\"big\": 3000000000}\nl = [3000000000]\nconst B = 3000000000\nn = {\"big\": B}\n\ndef go():\n    k = [1, -3000000000]\n    /say {k}\n",
    )
    .unwrap();
    let output = cobble()
        .args(["check", "--json"])
        .arg(&file)
        .output()
        .unwrap();
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let found: Vec<(String, u64)> = report["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|diagnostic| {
            (
                diagnostic["kind"].as_str().unwrap().to_string(),
                diagnostic["line"].as_u64().unwrap(),
            )
        })
        .collect();
    let kind = "number-out-of-range".to_string();
    assert_eq!(
        found,
        [
            (kind.clone(), 1),
            (kind.clone(), 2),
            (kind.clone(), 4),
            (kind, 7)
        ],
        "{report}"
    );
    let mut sim = Sim::load(
        "const B = 3000000000\nn = {\"big\": B}\nl = [3000000000]\n\ndef go():\n    return n.big + l[0] - 2147483647\n",
        &[],
    );
    assert_eq!(sim.call("go"), Some(2_147_483_647));
}

#[test]
fn values_resource_json_cannot_hold_are_named_at_their_line() {
    // Bug 80: no line, an internal name, advice for another place, and Rust
    // names in the message.
    for (source, shown, line) in [
        (
            "from stdlib import datapack\n\n\nx = (1 / 0)\ndatapack.predicate(\"a\", {\"type\": \"minecraft:random_chance\", \"chance\": (1 / 2)})\n",
            "`1/2` can't be worked out in resource JSON",
            "main.cbl:5:1",
        ),
        (
            "from stdlib import datapack\n\ndatapack.predicate(\"a\", {\"type\": \"minecraft:random_chance\", \"chance\": 0.5, \"x\": not True})\n",
            "`not True` can't be worked out in resource JSON",
            "main.cbl:3:1",
        ),
        (
            "from stdlib import datapack\n\ndef f():\n    datapack.predicate(\"a\", {\"type\": \"minecraft:random_chance\", \"chance\": 0.5, \"x\": 1 in [1]})\n",
            "`1 in [1]` can't be worked out in resource JSON",
            "main.cbl:4:5",
        ),
        (
            "from stdlib import datapack\n\ndef f():\n    datapack.predicate(\"a\", {\"type\": \"minecraft:random_chance\", \"chance\": 0.5, \"x\": True and False})\n",
            "`True and False` can't be worked out",
            "main.cbl:4:5",
        ),
        ("const A = 1 < 2\n", "`<` can't be worked out in a const", "main.cbl:1:1"),
        ("def go():\n    if [1]:\n        /say x\n", "Got: `[1]`", "main.cbl:2:5"),
        ("def go():\n    x.y = 2\n", "Assignment target `x.y`", "main.cbl:2:5"),
    ] {
        let error = compile_error(source);
        assert!(error.contains(shown), "{source}: {error}");
        assert!(error.contains(line), "{source}: {error}");
        assert!(
            !error.contains("_cobble_") && !error.contains("Array(") && !error.contains("Attribute("),
            "{error}"
        );
    }
}

#[test]
fn a_parameter_in_a_resource_text_component_is_a_compile_error() {
    // Bug 70: the parameter's name was written as the text.
    for source in [
        "from stdlib import datapack, text\n\ndef f(p):\n    datapack.dialog(\"d\", {\"type\": \"minecraft:notice\", \"title\": text.plain(p)})\n",
        "from stdlib import datapack, text, item_component\n\ndef f(p):\n    datapack.item_modifier(\"m\", {\"type\": \"minecraft:set_components\", \"components\": item_component.components(item_component.custom_name(text.plain(p)))})\n",
    ] {
        let error = compile_error(source);
        assert!(
            error.contains("`p` is a parameter, but a resource is written when the pack is built"),
            "{error}"
        );
        assert!(error.contains("main.cbl:4:5"), "{error}");
    }
}
