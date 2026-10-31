//! Follow-ups to the container, call, and diagnostics fixes: map keys no
//! command can name, a missing call argument, a line break in a literal
//! key, text keys of a list, parameters changed in place, and undefined
//! names in execute headers.

mod common;

use cobble::diagnostics::analyze_source;
use common::mcsim::{compile, compile_project, Sim};
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

// --- A missing call argument ---

#[test]
fn a_missing_argument_reads_as_an_empty_value_and_the_call_runs() {
    let mut sim = Sim::load(
        r#"cfg = {}
names = ["a"]
rows = [[1]]
word = "hi"
ran = 0
got = 5

def show(v):
    global ran
    ran += 1
    /say got {v}

def keep(v):
    global got
    got = v

def main():
    show(cfg.nick)
    show(names[3])
    show(rows[0][4])
    show(word[7])
    keep(cfg.nick)
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(sim.var("ran"), 4);
    assert_eq!(sim.var("got"), 0);
    let said: Vec<String> = sim
        .log
        .iter()
        .map(|line| line.strip_prefix("Server: ").unwrap_or(line).to_string())
        .collect();
    assert_eq!(said, ["got 0", "got ", "got 0", "got "]);
}

#[test]
fn a_present_argument_still_passes_its_value() {
    assert_eq!(
        said(
            r#"cfg = {"nick": "Steve", "tags": ["a", "b"]}

def show(v):
    /say got {v}

def main():
    show(cfg.nick)
    show(cfg.tags[1])
"#
        ),
        ["got Steve", "got b"]
    );
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

// --- A parameter changed in place ---

#[test]
fn a_parameter_changed_in_place_shows_its_current_value() {
    let lines = said(
        r#"def edit(t):
    /say before {t}
    t[0] = 9
    t.append(3)
    /say after {t}

def rename(cfg):
    cfg.name = "Alex"
    /say name {cfg}

def main():
    edit([1, 2])
    rename({"name": "Steve"})
"#,
    );
    assert_eq!(lines[..2], ["before [1,2]", "after [9,2,3]"]);
    assert!(
        lines[2].contains("Alex") && !lines[2].contains("Steve"),
        "{lines:?}"
    );
}

// --- Undefined names in execute headers ---

#[test]
fn undefined_names_in_execute_header_conditions_are_reported() {
    let source = r#"ok = 1

def main():
    as @a if nope > 3:
        /say a
    as @a unless entity @s[tag=x] if missing == ok at @s:
        /say b
    as @a if entity @p and lost:
        /say c
    if entity @a and gone > 1:
        /say d
    as @a if ok > 0 if score @s foo matches 1:
        /say e
"#;
    let found: Vec<(String, usize, usize)> = analyze_source(source)
        .into_iter()
        .filter(|diagnostic| diagnostic.kind == "undefined-variable")
        .map(|diagnostic| (diagnostic.message, diagnostic.line, diagnostic.column))
        .collect();
    assert_eq!(
        found,
        [
            ("Undefined variable `nope`".to_string(), 4, 14),
            ("Undefined variable `missing`".to_string(), 6, 38),
            ("Undefined variable `lost`".to_string(), 8, 28),
            ("Undefined variable `gone`".to_string(), 10, 22),
        ]
    );
}

// --- A fractional item read into a score ---

fn cli_build(source: &str) -> (bool, String) {
    let temp_dir = tempfile::tempdir().unwrap();
    let input = temp_dir.path().join("main.cbl");
    std::fs::write(&input, source).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_cobble"))
        .arg("build")
        .arg(&input)
        .arg("-o")
        .arg(temp_dir.path().join("output"))
        .output()
        .unwrap();
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn a_fractional_item_read_into_a_score_warns() {
    let (ok, stderr) = cli_build(
        r#"y = [0.6, 2.5]
cfg = {"rate": 1.5, "n": 2}

def show(v):
    /say show {v}

def main():
    show(y[0])
    show(cfg.rate)
    show(cfg.n)
    x = y[1]
    /say {x}
"#,
    );
    assert!(ok, "{stderr}");
    for expected in [
        "main.cbl:8:5: warning[fractional-number] Scores hold integers, so y[0], which holds 0.6, is read as 0.",
        "main.cbl:9:5: warning[fractional-number] Scores hold integers, so cfg.rate, which holds 1.5, is read as 1.",
        "main.cbl:11:5: warning[fractional-number] Scores hold integers, so y[1], which holds 2.5, is read as 2.",
    ] {
        assert!(stderr.contains(expected), "{expected}\n{stderr}");
    }
    assert_eq!(stderr.matches("warning[").count(), 3, "{stderr}");
}

// --- A module variable of a file compiled later ---

#[test]
fn reading_a_list_of_the_importing_file_is_a_compile_error() {
    // In a folder `lib.cbl` compiles before `main.cbl`, and `zlib.cbl` with
    // the imports of `main.cbl`: both before the module code of `main.cbl`.
    for lib in ["lib", "zlib"] {
        let main = format!(
            "import {lib}\n\nnames = [\"a\", \"b\"]\ncount = 3\n\ndef main():\n    show()\n"
        );
        let file = format!("{lib}.cbl");
        let error = compile_project(&[
            ("main.cbl", main.as_str()),
            (
                file.as_str(),
                "def show():\n    global count\n    c = count\n    global names\n    /say {names}\n",
            ),
        ])
        .unwrap_err();
        assert!(error.contains(&format!("{lib}.cbl:4:5")), "{error}");
        assert!(
            error
                .contains("`names` is a list declared in main.cbl, which this file doesn't import"),
            "{error}"
        );
    }
}

#[test]
fn a_list_of_an_imported_file_and_a_shared_number_still_work() {
    let mut sim = Sim::load_project(
        &[
            ("main.cbl", "import lib\n\ncount = 3\n\ndef main():\n    show()\n"),
            (
                "lib.cbl",
                "names = [\"a\", \"b\"]\n\ndef show():\n    global count\n    c = count\n    n = len(names)\n    /say {c} {n}\n",
            ),
        ],
        &[],
    );
    sim.call("main");
    assert_eq!(sim.log, ["Server: 3 2"]);
}
