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
fn load_tag_keeps_init_first_and_event_handlers_before_declared_entries() {
    let (_temp, output) = compile(
        r#"
from stdlib import datapack, event
import stdlib

datapack.function_tag("minecraft:load", ["sim:b", "sim:a", "sim:b"])

counter = 1

def a():
    /say a

def b():
    /say b

def on_load():
    /say load

stdlib.addEventListener(event.LOAD, on_load)
"#,
    )
    .unwrap();
    assert_eq!(
        read_json(&output, "data/minecraft/tags/function/load.json")["values"],
        serde_json::json!(["sim:_cobble_init", "sim:on_load", "sim:b", "sim:a"])
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
