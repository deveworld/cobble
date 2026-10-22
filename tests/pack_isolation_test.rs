//! Per-pack variable objectives and `persist` module variables, checked by
//! running the generated pack in the command simulator.

mod common;

use common::mcsim::{compile, Sim};
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

#[test]
fn variables_live_on_the_pack_objective() {
    let mut sim = Sim::load(
        r#"
round = 3

def next_round():
    global round
    round = round * 2 + 1
"#,
        &[],
    );
    sim.call("next_round");
    assert_eq!(sim.score("round", "sim.vars"), 7);
    assert!(sim.try_score("round", "temp").is_none());
    assert!(sim
        .function_text("_cobble_init")
        .contains("scoreboard objectives add sim.vars dummy"));
}

#[test]
fn source_level_temp_maps_to_the_pack_objective() {
    let mut sim = Sim::load(
        r#"
def read_level():
    level = 0
    store result score level temp:
        /scoreboard players get @s lvl
    /scoreboard players add level temp 10
    /execute if score level temp matches 12 run say twelve
"#,
        &["alex"],
    );
    sim.set_score("alex", "lvl", 2);
    let body = sim.function_text("read_level");
    assert!(!body.contains(" temp"), "temp should be mapped:\n{body}");
    assert!(body.contains("store result score level sim.vars"), "{body}");
    assert!(
        body.contains("scoreboard players add level sim.vars 10"),
        "{body}"
    );

    sim.call_as("read_level", "alex");
    assert_eq!(sim.var("level"), 12);
    assert_eq!(sim.log, vec!["alex: twelve"]);
}

#[test]
fn two_packs_with_the_same_variable_names_use_separate_objectives() {
    let source = "round = 1\n\ndef bump():\n    global round\n    round = round + 1\n";
    let alpha = build_with_namespace(source, "alpha");
    let beta = build_with_namespace(source, "beta");

    let alpha_init = read(&alpha.1, "alpha", "_cobble_init");
    let beta_init = read(&beta.1, "beta", "_cobble_init");
    assert!(alpha_init.contains("scoreboard players set round alpha.vars 1"));
    assert!(beta_init.contains("scoreboard players set round beta.vars 1"));
    assert!(read(&alpha.1, "alpha", "bump").contains("scoreboard players add round alpha.vars 1"));
    assert!(read(&beta.1, "beta", "bump").contains("scoreboard players add round beta.vars 1"));
}

#[test]
fn persist_score_keeps_its_value_across_reload() {
    let mut sim = Sim::load(
        r#"
persist best = 0
round = 1

def win():
    global best
    global round
    best = best + 5
    round = round + 1
"#,
        &[],
    );
    sim.call("win");
    assert_eq!(sim.var("best"), 5);
    assert_eq!(sim.var("round"), 2);

    sim.reload();
    assert_eq!(sim.var("best"), 5, "persist keeps its value");
    assert_eq!(sim.var("round"), 1, "plain module variables reset on load");
}

#[test]
fn persist_storage_value_keeps_its_value_across_reload() {
    let mut sim = Sim::load(
        r#"
persist champion = "nobody"
title = "none"

def crown():
    global champion
    global title
    champion = "alex"
    title = "king"
"#,
        &[],
    );
    assert_eq!(sim.storage["vars"]["champion"], "nobody");
    sim.call("crown");
    sim.reload();
    assert_eq!(sim.storage["vars"]["champion"], "alex");
    assert_eq!(sim.storage["vars"]["title"], "none");

    let init = sim.function_text("_cobble_init");
    assert!(init.contains(
        "execute unless data storage sim:global vars.champion run data modify storage sim:global vars.champion set value \"nobody\""
    ));
}

#[test]
fn persist_requires_module_level_and_a_constant() {
    let inside = compile("def f():\n    persist x = 1\n").unwrap_err();
    assert!(inside.contains("only allowed at module level"), "{inside}");

    let computed = compile("y = 2\npersist x = y + 1\n").unwrap_err();
    assert!(
        computed.contains("needs a constant initializer"),
        "{computed}"
    );
}

#[test]
fn persist_is_still_a_usable_variable_name() {
    let mut sim = Sim::load(
        "persist = 5\n\ndef f():\n    global persist\n    persist = persist + 1\n",
        &[],
    );
    sim.call("f");
    assert_eq!(sim.var("persist"), 6);
}

fn build_with_namespace(source: &str, namespace: &str) -> (TempDir, PathBuf) {
    let temp_dir = TempDir::new().unwrap();
    let input_file = temp_dir.path().join("main.cbl");
    let output_dir = temp_dir.path().join("output");
    fs::write(&input_file, source).unwrap();
    cobble::commands::build::build(cobble::commands::build::BuildOptions {
        input: Some(input_file),
        output: Some(output_dir.clone()),
        namespace: Some(namespace.to_string()),
        pack_format: None,
        description: None,
        verbose: false,
        quiet: true,
        zip: false,
        experimental_resource_pack: false,
        validate: false,
        dry_run: false,
        commands_json: PathBuf::from("data/commands.json"),
    })
    .unwrap();
    (temp_dir, output_dir)
}

fn read(output_dir: &std::path::Path, namespace: &str, function: &str) -> String {
    fs::read_to_string(
        output_dir
            .join("data")
            .join(namespace)
            .join("function")
            .join(format!("{function}.mcfunction")),
    )
    .unwrap()
}
