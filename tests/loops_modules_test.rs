//! Loops, calls, and module-level code: a runtime `for` iterates the list
//! it started with, a loop variable may hold an earlier value, module-level
//! runtime code runs in `_cobble_init`, and module names resolve wherever
//! they are declared in the file.

mod common;

use common::mcsim::{compile, Sim};

/// The `say` lines `main` prints.
fn said(source: &str) -> Vec<String> {
    let mut sim = Sim::load(source, &[]);
    sim.call("main");
    lines(&sim)
}

fn lines(sim: &Sim) -> Vec<String> {
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

// --- A runtime `for` iterates the list it started with (bug 43) ---

/// `main` runs `body` (indented by 8) in `for v in a:` with `a = start`,
/// then prints `c` and `len(a)`.
fn rebinding_loop(start: &str, body: &str) -> Vec<String> {
    let body = body
        .lines()
        .map(|line| format!("        {line}\n"))
        .collect::<String>();
    said(&format!(
        "def main():\n    a = {start}\n    c = 0\n    s = 0\n    for v in a:\n        c += 1\n        s += v\n{body}    n = len(a)\n    /say {{c}} {{n}} {{s}}\n"
    ))
}

#[test]
fn rebinding_the_list_in_the_body_keeps_iterating_the_old_list() {
    // Expected values come from Python 3.
    for (start, body, expected) in [
        ("[1, 2]", "a = a + [9]", "2 4 3"),
        ("[1, 2, 3]", "a = [0, 0, 0]", "3 3 6"),
        ("[1, 2, 3]", "a = [9]", "3 1 6"),
        ("[1, 2]", "a = [5, 5, 5, 5, 5]", "2 5 3"),
        ("[1, 2, 3]", "a = []", "3 0 6"),
        ("[1, 2, 3, 4]", "a = a[1:]", "4 0 10"),
        ("[1]", "if c < 5:\n    a = a + [1]", "1 2 1"),
        (
            "[1, 2]",
            "if c == 1:\n    a.append(7)\na = a + [9]",
            "3 6 10",
        ),
        ("[1]", "if c < 4:\n    a += [1]", "4 4 4"),
        ("[1, 2]", "if c == 1:\n    a.append(7)", "3 3 10"),
        ("[1, 2, 3]", "a.pop()", "2 1 3"),
    ] {
        assert_eq!(
            rebinding_loop(start, body),
            vec![expected.to_string()],
            "start {start}, body:\n{body}"
        );
    }
}

#[test]
fn rebinding_through_enumerate_a_parameter_or_a_call_keeps_the_old_list() {
    assert_eq!(
        said("def main():\n    a = [1, 2, 3]\n    c = 0\n    for i, v in enumerate(a):\n        c += 1\n        a = [9]\n    /say {c}\n"),
        vec!["3"]
    );
    assert_eq!(
        said("def f(l):\n    c = 0\n    for v in l:\n        c += 1\n        l = [1]\n    return c\n\ndef main():\n    n = f([1, 2, 3])\n    /say {n}\n"),
        vec!["3"]
    );
    // A function the body calls rebinds the module list, or grows it in
    // place.
    assert_eq!(
        said("a = [1, 2, 3]\n\ndef reset():\n    global a\n    a = []\n\ndef main():\n    c = 0\n    for v in a:\n        c += 1\n        reset()\n    n = len(a)\n    /say {c} {n}\n"),
        vec!["3 0"]
    );
    assert_eq!(
        said("a = [1, 2]\n\ndef grow():\n    global a\n    a.append(7)\n\ndef main():\n    c = 0\n    for v in a:\n        c += 1\n        if c < 2:\n            grow()\n    /say {c}\n"),
        vec!["3"]
    );
}

#[test]
fn the_path_to_the_list_is_worked_out_once() {
    assert_eq!(
        said("def main():\n    g = [[1, 2], [3, 4, 5]]\n    i = 0\n    c = 0\n    for v in g[i]:\n        c += 1\n        i = 1\n    /say {c}\n"),
        vec!["2"]
    );
    assert_eq!(
        said("def main():\n    m = {\"x\": [1, 2], \"y\": [1, 2, 3]}\n    k = \"x\"\n    c = 0\n    for v in m[k]:\n        c += 1\n        k = \"y\"\n    /say {c}\n"),
        vec!["2"]
    );
    // Replacing or moving the list inside its container.
    for change in ["g[0] = []", "g.pop(0)", "g.insert(0, [9])", "g = []"] {
        assert_eq!(
            said(&format!("def main():\n    g = [[1, 2, 3]]\n    c = 0\n    s = 0\n    for v in g[0]:\n        c += 1\n        s += v\n        {change}\n    /say {{c}} {{s}}\n")),
            vec!["3 6"],
            "{change}"
        );
    }
}

#[test]
fn a_dynamic_step_reads_the_length_on_each_pass() {
    assert_eq!(
        said("def main():\n    s = 2\n    a = [1, 2]\n    for v in a by s:\n        /say {v}\n        if v == 1:\n            a.append(3)\n"),
        vec!["1", "3"]
    );
    assert_eq!(
        said("def main():\n    s = 2\n    a = [1, 2, 3, 4, 5]\n    for v in a by s:\n        /say {v}\n        a.pop()\n"),
        vec!["1", "3"]
    );
}

// --- A loop variable with an earlier value (bugs 38, 21) ---

#[test]
fn a_loop_variable_keeps_its_earlier_value_or_takes_the_last_item() {
    // Expected values come from Python 3.
    let call = |args: &str| {
        said(&format!(
            "def f(l):\n    v = 0\n    for v in l:\n        pass\n    return v\n\ndef main():\n    r = f({args})\n    /say {{r}}\n"
        ))
    };
    assert_eq!(call("[7, 8]"), vec!["8"]);
    assert_eq!(call("[]"), vec!["0"]);
    assert_eq!(
        said("def main():\n    a = []\n    v = 5\n    for v in a:\n        pass\n    /tellraw @a {\"text\":\"{v}\"}\n"),
        vec!["5"]
    );
    assert_eq!(
        said("def main():\n    l = []\n    l.append(1)\n    l.append(3)\n    v = 0\n    for v in l:\n        pass\n    /say {v}\n"),
        vec!["3"]
    );
    assert_eq!(
        said("m = {\"k\": [7, 8]}\n\ndef main():\n    v = 0\n    for v in m.k:\n        pass\n    w = v + 1\n    /say {v} {w}\n"),
        vec!["8 9"]
    );
    assert_eq!(
        said("def f(l):\n    s = \"none\"\n    for s in l:\n        pass\n    /say {s}\n\ndef main():\n    f([\"a\", \"b\"])\n    f([])\n"),
        vec!["b", "none"]
    );
    // Overwritten after the loop and never read.
    assert_eq!(
        said("def f(l):\n    v = 0\n    for v in l:\n        pass\n    v = 1\n    return v\n\ndef main():\n    r = f([7, 8])\n    /say {r}\n"),
        vec!["1"]
    );
}

#[test]
fn a_loop_variable_of_another_known_type_read_after_the_loop_is_refused() {
    let error = compile_error(
        "def main():\n    v = \"a\"\n    nums = [1, 2]\n    for v in nums:\n        pass\n    /say {v}\n",
    );
    assert!(error.contains("depends on whether the loop ran"), "{error}");
}
