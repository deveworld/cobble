//! Fixes from the independent review after the backlog round: Minecraft
//! conditions joined to calls, whole-text placeholders holding quotes,
//! runtime loop variables after the loop, imported maps copied to locals,
//! `in` beside a Minecraft condition, late module reads at scale, module
//! loops in two files, joins in stdlib helper arguments, and per-entity
//! condition flags.

mod common;

use common::mcsim::Sim;

fn tag(sim: &mut Sim, player: &str, tag: &str) {
    sim.tags
        .entry(player.to_string())
        .or_default()
        .insert(tag.to_string());
}

/// Chat lines with the `Server: ` prefix of a line said by no one removed.
fn chat(sim: &Sim) -> Vec<String> {
    sim.log
        .iter()
        .map(|line| line.strip_prefix("Server: ").unwrap_or(line).to_string())
        .collect()
}

// --- A Minecraft condition joined to a call by `and` / `or` ---

const MIXED_CALLS: &str = r#"def f():
    /say f-called
    return 1
def g():
    /say g-called
    return 0
def main():
    if entity @e[tag=t] and f():
        /say and-ran
    if entity @e[tag=t] or g():
        /say or-ran
def left_call():
    if f() and entity @e[tag=t]:
        /say left-ran
def negated():
    if not entity @e[tag=t] and f():
        /say not-ran
def grouped():
    x = 1
    if x > 0 and (entity @e[tag=t] or g()):
        /say grouped-ran
def each():
    as @a if entity @s[tag=t] or f():
        /say each-ran
"#;

#[test]
fn a_minecraft_condition_and_a_call_short_circuit_like_python() {
    // Nobody is tagged: `and` skips f, `or` calls g, neither body runs.
    let mut sim = Sim::load(MIXED_CALLS, &["Alex", "Bo"]);
    sim.call("main");
    assert_eq!(chat(&sim), ["g-called"]);

    // Alex is tagged: `and` calls f and runs, `or` skips g and runs.
    let mut sim = Sim::load(MIXED_CALLS, &["Alex", "Bo"]);
    tag(&mut sim, "Alex", "t");
    sim.call("main");
    assert_eq!(chat(&sim), ["f-called", "and-ran", "or-ran"]);
}

#[test]
fn a_minecraft_condition_on_either_side_of_a_call_is_tested() {
    let mut sim = Sim::load(MIXED_CALLS, &["Alex"]);
    sim.call("left_call");
    sim.call("negated");
    sim.call("grouped");
    assert_eq!(chat(&sim), ["f-called", "f-called", "not-ran", "g-called"]);

    let mut sim = Sim::load(MIXED_CALLS, &["Alex"]);
    tag(&mut sim, "Alex", "t");
    sim.call("left_call");
    sim.call("negated");
    sim.call("grouped");
    assert_eq!(chat(&sim), ["f-called", "left-ran", "grouped-ran"]);
}

#[test]
fn a_per_entity_minecraft_condition_or_a_call_runs_the_call_only_when_needed() {
    let mut sim = Sim::load(MIXED_CALLS, &["Alex", "Bo"]);
    tag(&mut sim, "Alex", "t");
    sim.call("each");
    assert_eq!(
        chat(&sim),
        ["Bo: f-called", "Alex: each-ran", "Bo: each-ran"]
    );
}
