//! Module scope, loops, and calls: regressions checked in the command
//! simulator (`tests/common/mcsim.rs`) and against the compiler's errors.

mod common;

use common::mcsim::{compile, compile_project, Sim};

fn build_error(source: &str) -> String {
    match compile(source) {
        Ok(_) => panic!("expected a build error for:\n{source}"),
        Err(error) => error,
    }
}

// ---------------------------------------------------------------------------
// Modules
// ---------------------------------------------------------------------------

#[test]
fn module_variables_declared_below_a_function_resolve() {
    let mut sim = Sim::load(
        r#"
def go():
    x = later_text
    y = later_list
    n = len(later_list)
    later_list.append(5)
    total = 0
    for v in later_list:
        total += v
    m = later_map.k
    /say x={x} y={y} n={n} total={total} m={m} t={later_text}


later_text = "abc"
later_list = [1]
later_map = {"k": "v"}
"#,
        &[],
    );
    sim.call("go");
    assert_eq!(sim.log, vec!["Server: x=abc y=[1] n=1 total=6 m=v t=abc"]);
}

#[test]
fn module_variable_assigned_under_global_in_a_later_function_resolves() {
    let mut sim = Sim::load(
        r#"
def f():
    note.append(3)
    n = len(note)
    /say {n} {note}


def g():
    global note
    note = [1, 2]
"#,
        &[],
    );
    sim.call("g");
    sim.call("f");
    assert_eq!(sim.log, vec!["Server: 3 [1,2,3]"]);
}

#[test]
fn module_variable_defined_in_two_files_is_an_error() {
    let error = compile_project(&[
        (
            "other.cbl",
            "shared = 2\n\ndef other_get():\n    return shared\n",
        ),
        (
            "main.cbl",
            "import other\n\nshared = 1\n\ndef test():\n    a = shared\n    b = other_get()\n    /say main {a} other {b}\n",
        ),
    ])
    .err()
    .expect("a module variable in two files must not build");
    assert!(
        error.contains("Module variable `shared` is defined in both other.cbl and main.cbl"),
        "{error}"
    );
}

#[test]
fn module_variable_imported_from_another_file_is_shared() {
    let mut sim = Sim::load_project(
        &[
            ("state.cbl", "total = 10\n\ndef state_total():\n    return total\n"),
            (
                "main.cbl",
                "from state import total, state_total\n\ndef go():\n    global total\n    total += 5\n    x = state_total()\n    /say {x} {total}\n",
            ),
        ],
        &[],
    );
    sim.call("go");
    assert_eq!(sim.log, vec!["Server: 15 15"]);
}

#[test]
fn module_level_code_cannot_read_a_variable_assigned_below_it() {
    let error = build_error(
        r#"
copy = later
later = [1]

def f():
    /say {copy}
"#,
    );
    assert!(
        error.contains("Module variable `later` is read here before its assignment"),
        "{error}"
    );
}

/// Builds `source` as `main.cbl` with the `cobble` binary. Returns whether
/// the build succeeded and what it printed to stderr.
fn cli_build(source: &str, extra_args: &[&str]) -> (bool, String) {
    let temp_dir = tempfile::tempdir().unwrap();
    let input = temp_dir.path().join("main.cbl");
    std::fs::write(&input, source).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_cobble"))
        .arg("build")
        .arg(&input)
        .arg("-o")
        .arg(temp_dir.path().join("output"))
        .args(extra_args)
        .output()
        .unwrap();
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn out_of_range_module_variable_warns_at_its_own_line() {
    let (ok, stderr) = cli_build(
        "from stdlib import datapack\n\n\n\nhuge = 9999999999\nsmall = -9999999999\n",
        &[],
    );
    assert!(ok, "{stderr}");
    assert!(
        stderr.contains("main.cbl:5:1: warning[number-out-of-range] 9999999999"),
        "{stderr}"
    );
    assert!(stderr.contains("5 | huge = 9999999999"), "{stderr}");
    assert!(
        stderr.contains("main.cbl:6:1: warning[number-out-of-range]"),
        "{stderr}"
    );
}
