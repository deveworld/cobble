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
    .expect_err("a module variable in two files must not build");
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

#[test]
fn module_variable_that_lowering_changes_resolves_above_its_assignment() {
    // `[0] * 3` becomes a list literal and `"ab" * 2` text only when they
    // are lowered; a function above them still sees their types.
    let source = |order_first: bool| {
        let function = "def f():\n    n = len(zeros)\n    t = word\n    /say {n} {zeros} {t}\n";
        let module = "zeros = [0] * 3\nword = \"ab\" * 2\n";
        if order_first {
            format!("{module}\n{function}")
        } else {
            format!("{function}\n{module}")
        }
    };
    for order_first in [false, true] {
        let mut sim = Sim::load(&source(order_first), &[]);
        sim.call("f");
        assert_eq!(sim.log, vec!["Server: 3 [0,0,0] abab"], "{order_first}");
    }
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

#[test]
fn def_with_the_reserved_cobble_prefix_is_an_error() {
    for source in [
        "x = 1\n\ndef _cobble_say_0():\n    /say mine\n\ndef a():\n    /say {x}\n",
        "def _cobble_init():\n    /say mine\n\nx = 1\n",
    ] {
        let error = build_error(source);
        assert!(
            error.contains("main.cbl:1:1") || error.contains("main.cbl:3:1"),
            "{error}"
        );
        assert!(
            error.contains("starts with `_cobble`, which Cobble reserves"),
            "{error}"
        );
    }
}

#[test]
fn def_named_like_a_generated_helper_is_an_error() {
    // The helper comes after the def, and before it.
    for (source, line) in [
        (
            "x = 1\n\ndef if_temp_0():\n    /say mine\n\ndef a():\n    if x > 0:\n        /say pos\n        /say pos2\n",
            3,
        ),
        (
            "x = 1\n\ndef a():\n    if x > 0:\n        /say pos\n        /say pos2\n\ndef if_temp_0():\n    /say mine\n",
            8,
        ),
    ] {
        let error = build_error(source);
        assert!(error.contains(&format!("main.cbl:{line}:1")), "{error}");
        assert!(
            error.contains("`if_temp_0` is also the name of a helper function"),
            "{error}"
        );
    }
}

#[test]
fn long_def_names_build_while_the_file_name_fits() {
    let name = "a".repeat(244);
    let (_dir, output) = compile(&format!("def {name}():\n    /say a\n")).unwrap();
    let file = output.join(format!("data/sim/function/{name}.mcfunction"));
    assert!(file.exists(), "{}", file.display());

    let longer = "a".repeat(245);
    let error = build_error(&format!("def {longer}():\n    /say a\n"));
    assert!(error.contains("main.cbl:1:1"), "{error}");
    assert!(
        error.contains(&format!("Function name `{longer}` is 245 bytes long")),
        "{error}"
    );
    assert!(error.contains("at most 244 bytes"), "{error}");

    // A helper named after the function must fit too.
    let recursive = "b".repeat(240);
    let error = build_error(&format!(
        "def {recursive}(k):\n    if k > 0:\n        return {recursive}(k - 1)\n    return 0\n"
    ));
    assert!(
        error.contains(&format!("the helper `_cobble_exit_{recursive}`")),
        "{error}"
    );
}

// ---------------------------------------------------------------------------
// Loops and calls
// ---------------------------------------------------------------------------

#[test]
fn expanded_for_target_holds_its_last_value_after_the_loop() {
    let mut sim = Sim::load(
        r#"
mod = 5
out = []

def go():
    for v in [1, 2]:
        pass
    w = v + 1
    /say w={w} v={v}

def go_global():
    global mod
    for mod in [1, 2]:
        pass
    /say global={mod}

def go_local():
    for mod in [1, 2]:
        pass
    /say local={mod}

def go_range():
    i = 5
    for i in range(2):
        pass
    /say range={i}

def go_append():
    for j in [7, 8]:
        pass
    out.append(j)
    /say out={out}

def go_text():
    for s in ["a", "b"]:
        /say in {s}
    /say last={s}
"#,
        &[],
    );
    for function in [
        "go",
        "go_global",
        "go_local",
        "go_range",
        "go_append",
        "go_text",
    ] {
        sim.call(function);
    }
    assert_eq!(
        sim.log,
        vec![
            "Server: w=3 v=2",
            "Server: global=2",
            "Server: local=2",
            "Server: range=1",
            "Server: out=[8]",
            "Server: in a",
            "Server: in b",
            "Server: last=b",
        ]
    );
    // `global` wrote the module variable; the local loop didn't.
    assert_eq!(sim.var("mod"), 2);
}

#[test]
fn expanded_for_body_may_assign_its_target() {
    let mut sim = Sim::load(
        r#"
def go():
    total = 0
    for i in range(3):
        i = i * 10
        total += i
        /say {i}
    /say total={total} i={i}
"#,
        &[],
    );
    sim.call("go");
    assert_eq!(
        sim.log,
        vec![
            "Server: 0",
            "Server: 10",
            "Server: 20",
            "Server: total=30 i=20"
        ]
    );
}

#[test]
fn expanded_for_global_target_is_seen_by_a_called_function() {
    let mut sim = Sim::load(
        r#"
step = 0

def show():
    /say step {step}

def go():
    global step
    for step in [3, 4]:
        show()
"#,
        &[],
    );
    sim.call("go");
    assert_eq!(sim.log, vec!["Server: step 3", "Server: step 4"]);
}
