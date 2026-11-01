mod common;

use common::mcsim::Sim;

#[test]
fn raw_and_mixed_elif_branches_are_lazy_and_exclusive() {
    let mut sim = Sim::load(
        r#"out = 0
calls = 0
def test():
    global calls
    calls += 1
    return 1
def choose(c):
    global out
    if c == 1:
        out = 1
    elif entity @a and test():
        out = 2
    else:
        out = 3
def main():
    choose(1)
    /say {out} {calls}
    choose(0)
    /say {out} {calls}
    if entity @a[tag=absent]:
        /say wrong
    elif entity @a:
        /say chosen
    else:
        /say wrong
"#,
        &["Alex"],
    );
    sim.call("main");
    assert_eq!(sim.log, ["Server: 1 0", "Server: 2 1", "Server: chosen"]);
}

#[test]
fn raw_and_mixed_while_conditions_are_rechecked_each_iteration() {
    let mut sim = Sim::load(
        r#"n = 0
calls = 0
box = {"k": 1}
def test():
    global calls
    calls += 1
    return calls < 3
def main():
    global n
    /tag @a add active
    while entity @a[tag=active] and n < 5:
        n += 1
        if n == 2:
            /tag @a remove active
    /say {n}
    while score n temp matches ..3:
        n += 1
    /say {n}
    while data storage sim:global vars.box.k:
        n += 1
        /data remove storage sim:global vars.box.k
    while function sim:test:
        n += 1
    /say {n} {calls}
"#,
        &["Alex"],
    );
    sim.call("main");
    assert_eq!(sim.log, ["Server: 2", "Server: 4", "Server: 7 3"]);
}

#[test]
fn minecraft_headers_keep_parameters_unrolled_values_and_entity_context() {
    let mut sim = Sim::load(
        r#"def check(flag):
    if entity @s[tag=missing]:
        /say first
    elif entity @s[tag={flag}]:
        /say tagged
def main():
    /tag @a add chosen
    as @a:
        check("chosen")
        if entity @s[tag=chosen]:
            /say context
        else:
            /say wrong
    for i in [1, 2]:
        n = 0
        while entity @a and n < i:
            n += 1
        /say {n}
"#,
        &["Alex"],
    );
    sim.call("main");
    assert_eq!(
        sim.log,
        ["Alex: tagged", "Alex: context", "Server: 1", "Server: 2"]
    );
}

#[test]
fn minecraft_conditions_combine_with_membership_in_elif_and_while() {
    let mut sim = Sim::load(
        r#"def choose(n):
    if n == 5:
        /say wrong
    elif entity @a and n in [0, 1] and n not in [3, 4]:
        /say member
    else:
        /say other
def main():
    n = 0
    while entity @a and n in [0, 1] and n not in [3, 4]:
        choose(n)
        n += 1
    choose(n)
    while not entity @a[tag=missing] and n not in [4, 5]:
        n += 1
    /say {n}
"#,
        &["Alex"],
    );
    sim.call("main");
    assert_eq!(
        sim.log,
        [
            "Server: member",
            "Server: member",
            "Server: other",
            "Server: 4"
        ]
    );
}

#[test]
fn minecraft_headers_still_check_python_names_calls_and_local_binding() {
    for header in [
        "while entity @a and missing > 0:",
        "while entity @a and missing():",
        "if entity @a[tag=absent]:\n        /say first\n    elif entity @a and missing > 0:",
        "if entity @a[tag=absent]:\n        /say first\n    elif entity @a and missing():",
    ] {
        let source = format!("def main():\n    {header}\n        /say body\n");
        let diagnostics = cobble::diagnostics::analyze_source(&source);
        assert!(
            diagnostics.iter().any(|d| d.message.contains("missing")),
            "{source}\n{diagnostics:?}"
        );
        assert!(common::mcsim::compile(&source).is_err(), "{source}");
    }
    let error = common::mcsim::compile(
        "x = 5\ndef main():\n    while entity @a and x > 0:\n        x = 1\n",
    )
    .unwrap_err();
    assert!(
        error.contains("before") || error.contains("unbound"),
        "{error}"
    );
}

#[test]
fn minecraft_headers_keep_returns_and_recursive_call_frames() {
    let mut sim = Sim::load(
        r#"out = []
def recurse(n):
    held = n
    if entity @a[tag=missing]:
        return 99
    elif entity @a and n > 0:
        recurse(n - 1)
    out.append(held)
    while entity @a:
        return held + 4
    return 99
def main():
    out.append(recurse(2))
"#,
        &["Alex"],
    );
    sim.call("main");
    assert_eq!(sim.storage["vars"]["out"], serde_json::json!([0, 1, 2, 6]));
}
