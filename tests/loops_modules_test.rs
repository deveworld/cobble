//! Loops, calls, and module-level code: a runtime `for` iterates the list
//! it started with, a loop variable may hold an earlier value, module-level
//! runtime code runs in `_cobble_init`, and module names resolve wherever
//! they are declared in the file.

mod common;

use common::mcsim::{compile, compile_project, Sim};

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

// --- Module-level code runs in `_cobble_init` (bugs 21, 38, 51) ---

#[test]
fn module_level_runtime_loops_run_in_source_order() {
    // Expected values come from Python 3.
    assert_eq!(
        said("lst = [1, 2]\ntotal = 0\nfor v in lst:\n    total += v\n\ndef main():\n    w = v + 1\n    /say {total} {v} {w}\n"),
        vec!["3 2 3"]
    );
    assert_eq!(
        said("total = 0\nwhile total < 3:\n    total += 1\n\ndef main():\n    /say {total}\n"),
        vec!["3"]
    );
    assert_eq!(
        said("n = 3\nfor i in range(n):\n    pass\n\ndef main():\n    w = i + 1\n    /tellraw @a {\"text\":\"{i} {w}\"}\n"),
        vec!["2 3"]
    );
    assert_eq!(
        said(
            "a = 2\ntotal = 0\nfor v in [a, 2]:\n    total += v\n\ndef main():\n    /say {total}\n"
        ),
        vec!["4"]
    );
    // A function above the loop reads its variable.
    assert_eq!(
        said("def main():\n    /say {v}\n\nl = [4, 6]\nfor v in l:\n    pass\n"),
        vec!["6"]
    );
}

#[test]
fn module_level_calls_and_commands_run_at_load() {
    let sim = Sim::load(
        "lst = [1, 2]\nfor v in lst:\n    /say {v}\nout = []\nout.append(8)\n/say {out}\n",
        &[],
    );
    assert_eq!(lines(&sim), vec!["1", "2", "[8]"]);
    assert_eq!(
        said(
            "out = []\nfor j in [7, 8]:\n    pass\nout.append(j)\n\ndef main():\n    /say {out}\n"
        ),
        vec!["[8]"]
    );
    assert_eq!(
        said("out = []\nn = 3\nfor i in range(n):\n    pass\n\ndef main():\n    out.append(i)\n    /say {out}\n"),
        vec!["[2]"]
    );
    // The init runs again on `/reload`, from the declared values.
    let mut sim = Sim::load("x = 2\ntotal = 0\nfor v in [1, x]:\n    total += v\n", &[]);
    sim.reload();
    assert_eq!(sim.var("total"), 3);
}

#[test]
fn module_level_code_reading_a_later_variable_is_refused() {
    let error = compile_error("for v in later:\n    pass\nlater = [1, 2]\n");
    assert!(error.contains("before its assignment"), "{error}");
    assert!(error.contains("main.cbl:1"), "{error}");
}

// --- A `const` declared below the function that reads it (bugs 35, 47) ---

/// `main` with `body` (indented by 4) above `const LATER = 3`.
fn above_later(body: &str) -> Vec<String> {
    let body = body
        .lines()
        .map(|line| format!("    {line}\n"))
        .collect::<String>();
    said(&format!(
        "def h(v):\n    return v + 1\n\ndef main():\n{body}\n\nconst LATER = 1 + 2\n"
    ))
}

#[test]
fn a_const_declared_below_a_function_has_its_value_there() {
    // Expected values come from Python 3.
    for (body, expected) in [
        ("x = LATER\n/say {x}", "3"),
        ("/say {LATER}", "3"),
        ("/tellraw @a {\"text\":\"{LATER}\"}", "3"),
        ("y = LATER + 1\n/say {y}", "4"),
        ("x = -LATER\n/say {x}", "-3"),
        ("x = abs(LATER)\n/say {x}", "3"),
        ("x = 5\nx += LATER\n/say {x}", "8"),
        ("if LATER == 3:\n    /say yes", "yes"),
        ("if LATER in [1, 2, 3]:\n    /say yes", "yes"),
        ("if LATER > 1:\n    /say yes\nelse:\n    /say no", "yes"),
        ("x = [10, 11, 12, 13]\ny = x[LATER]\n/say {y}", "13"),
        ("y = h(LATER)\n/say {y}", "4"),
        ("l = [LATER]\n/say {l}", "[3]"),
        ("m = {\"a\": LATER}\ny = m.a\n/say {y}", "3"),
        ("y = sum([LATER, 1])\n/say {y}", "4"),
        ("/scoreboard players set x temp {LATER}\n/scoreboard players add x temp 1\nx = 0\n/say done", "done"),
    ] {
        assert_eq!(above_later(body), vec![expected.to_string()], "{body}");
    }
    assert_eq!(
        above_later("for i in range(LATER):\n    /say {i}"),
        vec!["0", "1", "2"]
    );
    assert_eq!(
        above_later("i = 0\nwhile i < LATER:\n    i += 1\n/say {i}"),
        vec!["3"]
    );
    assert_eq!(
        said("def f():\n    return LATER\n\ndef main():\n    r = f()\n    /say {r}\n\nconst LATER = 3\n"),
        vec!["3"]
    );
}

#[test]
fn module_level_code_reading_a_later_const_is_refused() {
    for source in [
        "x = LATER + 1\nconst LATER = 3\n",
        "const A = B + 1\nconst B = 2\n",
    ] {
        let error = compile_error(source);
        assert!(error.contains("before its assignment"), "{source}: {error}");
    }
}

#[test]
fn assigning_a_const_name_is_refused() {
    for source in [
        "const LIMIT = 2\nLIMIT = 1\n",
        "const LIMIT = 2\n\ndef main():\n    global LIMIT\n    LIMIT = 1\n",
    ] {
        let error = compile_error(source);
        assert!(error.contains("is declared `const`"), "{source}: {error}");
    }
}

// --- A name assigned only under `global` (bugs 35, 47) ---

#[test]
fn a_name_assigned_only_under_global_has_its_value_in_either_order() {
    let setup = "def calc():\n    return 5\n\ndef setup(v):\n    global n, note, lst, word, joined, copy\n    n = calc()\n    note = v\n    lst = sorted([3, 1])\n    a = \"x\"\n    word = a + \"y\"\n    joined = [1] + [2]\n    s = \"hello\"\n    copy = s\n";
    let reader = "def main():\n    setup(\"hi\")\n    y = note\n    m = n + 1\n    k = lst[0]\n    w = word\n    j = len(joined)\n    c = copy\n    /say {n} {m} {y} {note} {k} {w} {j} {c}\n";
    // Expected values come from Python 3.
    for source in [format!("{setup}\n{reader}"), format!("{reader}\n{setup}")] {
        assert_eq!(said(&source), vec!["5 6 hi hi 1 xy 2 hello"], "{source}");
    }
}

// --- Module names in two files (bugs 27, 36) ---

#[test]
fn a_const_and_a_variable_of_one_name_in_two_files_are_refused() {
    for main in [
        "import other\ntotal = 20\n\ndef go():\n    /say {total}\n",
        "from other import total\ntotal = 20\n\ndef go():\n    /say {total}\n",
    ] {
        let error = compile_project(&[
            ("main.cbl", main),
            (
                "other.cbl",
                "const total = 10\n\ndef other_get():\n    /say {total}\n",
            ),
        ])
        .err()
        .unwrap_or_else(|| panic!("expected a compile error for:\n{main}"));
        assert!(error.contains("is defined in both"), "{error}");
    }
    // The same `const` with the same value is one value.
    compile_project(&[
        (
            "main.cbl",
            "import other\nconst SIZE = 4\n\ndef go():\n    /say {SIZE}\n",
        ),
        (
            "other.cbl",
            "const SIZE = 4\n\ndef other_get():\n    /say {SIZE}\n",
        ),
    ])
    .unwrap();
}

#[test]
fn module_level_runtime_loops_in_two_files_share_no_variable() {
    let error = compile_project(&[
        (
            "main.cbl",
            "import other\nlst = [7, 8, 9]\nfor i in lst:\n    pass\n",
        ),
        (
            "other.cbl",
            "items = [1, 2]\nfor i in items:\n    pass\n\ndef other_get():\n    /say {i}\n",
        ),
    ])
    .map(|_| ())
    .expect_err("expected a compile error");
    assert!(
        error.contains("Module variable `i` is defined in both"),
        "{error}"
    );
}

// --- A `store` block stores the last call or command in it (bug 34) ---

#[test]
fn a_store_block_stores_the_last_call_not_its_cleanup() {
    for (body, expected) in [
        ("double(4)", 8),
        ("y = double(4)", 8),
        ("/scoreboard players set b temp 5\ndouble(4)", 8),
        ("double(4)\ndouble(5)", 10),
        ("seven()\ndouble(4)", 8),
        ("for i in [3, 4]:\n    double(i)", 8),
        ("if c == 1:\n    double(4)", 8),
        ("if c == 2:\n    double(4)\nelse:\n    double(6)", 12),
        ("if c == 1:\n    seven()", 7),
        ("/scoreboard players set b temp 5", 5),
    ] {
        let body = body
            .lines()
            .map(|line| format!("        {line}\n"))
            .collect::<String>();
        let mut sim = Sim::load(
            &format!(
                "def double(n):\n    return n * 2\n\ndef seven():\n    return 7\n\ndef main():\n    c = 1\n    store result score x temp:\n{body}"
            ),
            &[],
        );
        sim.call("main");
        assert_eq!(sim.var("x"), expected, "{body}");
    }
}
