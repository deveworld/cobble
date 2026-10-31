//! Fixes from the independent review after the backlog round: Minecraft
//! conditions joined to calls, whole-text placeholders holding quotes,
//! runtime loop variables after the loop, imported maps copied to locals,
//! `in` beside a Minecraft condition, late module reads at scale, module
//! loops in two files, joins in stdlib helper arguments, and per-entity
//! condition flags.

mod common;

use common::mcsim::{compile, Sim};

fn compile_error(source: &str) -> String {
    match compile(source) {
        Ok(_) => panic!("expected a compile error for:\n{source}"),
        Err(error) => error,
    }
}

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

// --- A parameter that fills a tellraw or title text ---

#[test]
fn a_whole_text_parameter_shows_quotes_and_backslashes() {
    let mut sim = Sim::load(
        r#"def show(msg):
    /tellraw @a {msg}
    /tellraw @a note: {msg}
def bar(msg):
    /title @a actionbar {msg}
def raw(msg):
    /tellraw @a $(msg)
def main():
    show("say \"hi\" a\\b")
    show("plain sentence")
    show(7)
    raw("{\"text\":\"ready\"}")
"#,
        &["Alex"],
    );
    let show = sim.function_text("show");
    assert!(!show.contains("$(msg)"), "{show}");
    let bar = sim.function_text("bar");
    assert!(
        bar.contains("\ntitle @a actionbar [{") && bar.contains(r#""nbt":"_text.msg""#),
        "{bar}"
    );
    // An author's `$(msg)` stays the raw macro text.
    assert_eq!(sim.function_text("raw"), "$tellraw @a $(msg)");
    sim.call("main");
    assert_eq!(
        sim.log,
        [
            r#"say "hi" a\b"#,
            r#"note: say "hi" a\b"#,
            "plain sentence",
            "note: plain sentence",
            "7",
            "note: 7",
            "ready",
        ]
    );
}

// --- A runtime loop's variable after the loop ---

#[test]
fn a_runtime_loop_variable_holds_its_last_value_after_the_loop() {
    // Python prints `b 6 1`, `x=b`, then `3 0` and `7`.
    let mut sim = Sim::load(
        r#"names = ["a", "b"]
nums = [5, 6]
def main():
    for n in names:
        pass
    for v in nums:
        pass
    for i in range(len(nums)):
        pass
    /say {n} {v} {i}
    x = n
    /say x={x}
    y = v + i
    k = len(nums) + 2
    for d in range(k, 0, -1):
        pass
    for e in range(k - 4, k, 2):
        /say e={e}
    /say {d} {e}
    w = 7
    for w in range(k - 4):
        pass
    /say {w}
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(chat(&sim), ["b 6 1", "x=b", "e=0", "e=2", "1 2", "7"]);
    assert_eq!(sim.var("#main.y"), 7);
}

#[test]
fn a_loop_variable_of_another_type_before_the_loop_is_a_compile_error() {
    let error = compile_error(
        r#"names = ["a", "b"]
def main():
    n = 3
    for n in names:
        pass
    /say {n}
"#,
    );
    assert!(
        error.contains("`n` holds a number before this loop and text in it"),
        "{error}"
    );
    assert!(error.contains("main.cbl:4"), "{error}");
}
