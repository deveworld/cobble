//! Follow-ups to the container, call, and diagnostics fixes: map keys no
//! command can name, a missing call argument, a line break in a literal
//! key, text keys of a list, parameters changed in place, and undefined
//! names in execute headers.

mod common;

use cobble::diagnostics::analyze_source;
use common::mcsim::{compile, Sim};
use serde_json::{json, Value};

fn storage(sim: &Sim, name: &str) -> Value {
    sim.storage["vars"][name].clone()
}

/// The `say` lines `main` prints.
fn said(source: &str) -> Vec<String> {
    let mut sim = Sim::load(source, &[]);
    sim.call("main");
    sim.log
        .iter()
        .map(|line| line.strip_prefix("Server: ").unwrap_or(line).to_string())
        .collect()
}

fn compile_error(source: &str) -> String {
    match compile(source) {
        Ok(_) => panic!("expected a compile error for:\n{source}"),
        Err(error) => error,
    }
}

// --- An empty literal map key ---

#[test]
fn an_empty_literal_key_is_a_compile_error_at_its_line() {
    for (source, at) in [
        (
            "m = {\"a\": 1}\n\ndef main():\n    m[\"\"] = 1\n",
            "main.cbl:4",
        ),
        (
            "m = {\"a\": 1}\n\ndef main():\n    x = m[\"\"]\n",
            "main.cbl:4",
        ),
        (
            "m = {\"a\": {\"b\": 1}}\n\ndef main():\n    m.a[\"\"] += 1\n",
            "main.cbl:4",
        ),
        ("n = {\"\": 1}\n\ndef main():\n    pass\n", "main.cbl:1"),
        (
            "m = {}\n\ndef main():\n    m.a = {\"x\": {\"\": 2}}\n",
            "main.cbl:4",
        ),
        (
            "m = {}\n\ndef main():\n    v = 3\n    m.a = {\"\": v}\n",
            "main.cbl:5",
        ),
    ] {
        let error = compile_error(source);
        assert!(error.contains("A map key can't be empty"), "{error}");
        assert!(error.contains(at), "{at}: {error}");
    }
}

#[test]
fn an_empty_key_in_a_membership_test_is_still_false() {
    let mut sim = Sim::load(
        "m = {\"a\": 1}\nfound = 0\n\ndef main():\n    global found\n    if \"\" in m:\n        found = 1\n",
        &[],
    );
    sim.call("main");
    assert_eq!(sim.var("found"), 0);
}

// --- A line break in a literal key ---

#[test]
fn a_literal_key_with_a_line_break_reads_and_writes_the_field() {
    let mut sim = Sim::load(
        r#"m = {"x": 0}
lit = {"c\nd": 4}
x = 0
y = 0

def main():
    global x, y
    m["a\nb"] = 1
    m["a\nb"] += 2
    x = m["a\nb"]
    y = lit["c\nd"]
    m["t\tz"] = 8
    m["q\"\\"] = 7
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(sim.var("x"), 3);
    assert_eq!(sim.var("y"), 4);
    assert_eq!(
        storage(&sim, "m"),
        json!({"x": 0, "a\nb": 3, "t\tz": 8, "q\"\\": 7})
    );
    // No line of a function holds an escape a path key can't read.
    for (name, lines) in &sim.functions {
        for line in lines {
            assert!(!line.contains("\"a\\nb\" set"), "{name}: {line}");
            assert!(!line.contains("\\t"), "{name}: {line}");
        }
    }
}

#[test]
fn a_runtime_value_under_a_literal_key_with_a_line_break_is_a_compile_error() {
    let error = compile_error("m = {}\n\ndef main():\n    v = 3\n    m.a = {\"a\\nb\": v}\n");
    assert!(error.contains("main.cbl:5"), "{error}");
    assert!(error.contains("has a line break"), "{error}");
}

// --- A text key of a list ---

#[test]
fn a_text_key_of_a_list_is_a_compile_error_at_any_depth() {
    for (source, at) in [
        (
            "rows = [[1, 2]]\n\ndef main():\n    x = rows[0][\"k\"]\n",
            "main.cbl:4",
        ),
        (
            "rows = [[1, 2]]\n\ndef main():\n    rows[0][\"k\"] = 3\n",
            "main.cbl:4",
        ),
        (
            "grid = {\"r\": [[1]]}\n\ndef main():\n    x = grid.r[0][\"k\"]\n",
            "main.cbl:4",
        ),
        (
            "flat = [1, 2]\n\ndef main():\n    flat[\"k\"] = 3\n",
            "main.cbl:4",
        ),
        (
            "rows = [[1, 2]]\n\ndef main():\n    key = \"k\"\n    x = rows[0][key]\n",
            "main.cbl:5",
        ),
    ] {
        let error = compile_error(source);
        assert!(
            error.contains("List indexes must be integers, not text"),
            "{error}"
        );
        assert!(error.contains(at), "{at}: {error}");
    }
}

#[test]
fn a_text_key_of_a_map_inside_a_list_still_works() {
    let mut sim = Sim::load(
        "rows = [{\"k\": 1}]\nx = 0\n\ndef main():\n    global x\n    rows[0][\"k\"] = 5\n    x = rows[0][\"k\"]\n",
        &[],
    );
    sim.call("main");
    assert_eq!(sim.var("x"), 5);
}

