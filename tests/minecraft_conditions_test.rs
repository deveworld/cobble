mod common;

use common::mcsim::{compile, load_functions, Sim};
use std::sync::mpsc;
use std::time::Duration;

fn condition_function(output: &std::path::Path, name: &str) -> String {
    load_functions(output)[name].join("\n")
}

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

#[test]
fn tag_references_and_raw_conditions_compile_in_execute_headers() {
    // Run on a worker thread: the structural check used to spin forever on
    // `#ns:tag`, and a hang should fail the test rather than stall the suite.
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let result = compile(
            r#"
def main():
    if block 0 99 0 #demo:floor:
        /say floor
    as @a if items entity @s weapon.mainhand #minecraft:swords:
        /say sword
    if predicate demo:checks/ok:
        /say predicate
    if score @s kills matches 1..:
        /say kills
    # demo:not_a_tag stays a comment
    /say done
"#,
        )
        .map(|(temp, output_dir)| (temp, load_functions(&output_dir)));
        let _ = sender.send(result);
    });
    let (_temp, functions) = receiver
        .recv_timeout(Duration::from_secs(30))
        .expect("compiling `#ns:tag` conditions hung")
        .unwrap_or_else(|error| panic!("build failed: {error}"));

    assert_eq!(
        functions["main"],
        [
            "execute if block 0 99 0 #demo:floor run say floor",
            "execute as @a if items entity @s weapon.mainhand #minecraft:swords run say sword",
            "execute if predicate demo:checks/ok run say predicate",
            "execute if score @s kills matches 1.. run say kills",
            "say done",
        ]
    );
}

#[test]
fn unknown_execute_selectors_are_rejected() {
    let error = compile(
        r#"
define @Guard = @e[type=minecraft:armor_stand,tag=guard]
create {"Tags": ["guard"]}
end

def main():
    as @Guard:
        /say guard
"#,
    )
    .unwrap_err();
    assert!(error.contains("Unknown selector `@Guard`"), "{error}");
    assert!(error.contains("entity template"), "{error}");

    let error = compile("def main():\n    at @Typo:\n        /say typo\n").unwrap_err();
    assert!(error.contains("Unknown selector `@Typo`"), "{error}");

    let (_temp, output_dir) = compile(
        r#"
@Guards = @e[type=minecraft:armor_stand,tag=guard]

def main():
    as @Guards at @n[type=minecraft:marker]:
        /say ok
"#,
    )
    .unwrap_or_else(|error| panic!("build failed: {error}"));
    assert_eq!(
        load_functions(&output_dir)["main"],
        ["execute as @e[type=minecraft:armor_stand,tag=guard] at @n[type=minecraft:marker] run say ok"]
    );
}

#[test]
fn test_raw_execute_condition_with_parens_in_selector_compiles() {
    let source = r#"
def greet():
    as @a if entity @s[name="Bob (Admin)"]:
        /say hi
"#;

    let (_temp, output_dir) = common::mcsim::compile(source).unwrap();
    let content = condition_function(&output_dir, "greet");
    assert!(
        content.contains(r#"execute as @a if entity @s[name="Bob (Admin)"] run say hi"#),
        "{content}"
    );
}

#[test]
fn test_failed_condition_error_restores_masked_operand_text() {
    // Selector operands are masked with synthetic identifiers before the raw
    // condition is parsed as a Python expression. When translation then
    // fails, the error must name the original operand (`@s`) instead of
    // leaking the `__cobble_cond_operand_N__` placeholder.
    let source = r#"
def check():
    x = 1
    as @a if x and [@s]:
        /say hi
"#;

    let error = common::mcsim::compile(source).unwrap_err();
    assert!(
        !error.contains("__cobble_cond_operand_"),
        "error must not leak masked operand placeholders: {error}"
    );
    assert!(
        error.contains("Got: `[@s]`"),
        "error should show the original operand spelling: {error}"
    );
}

#[test]
fn python_execute_conditions_keep_parenthesized_forms_and_selector_operands() {
    for (condition, wanted) in [
        ("x > 5 and y < 2", vec!["Alex: yes"]),
        ("x > 5 and y > 2", vec![]),
        ("x == 10 or y == 2", vec!["Alex: yes"]),
        ("x == 0 or y == 2", vec![]),
        ("not done", vec!["Alex: yes"]),
        ("not x", vec![]),
        ("x != 3", vec!["Alex: yes"]),
        ("x != 10", vec![]),
        ("@s > 5", vec!["Alex: yes"]),
        ("@s < 5", vec![]),
        ("ready()", vec!["Alex: yes"]),
    ] {
        for condition in [condition.to_string(), format!("({condition})")] {
            let source = format!(
                "def ready():\n    return 1\ndef main():\n    x = 10\n    y = 1\n    done = 0\n    /scoreboard players set Alex temp 7\n    as @a if {condition}:\n        /say yes\n"
            );
            let mut sim = Sim::load(&source, &["Alex"]);
            sim.call("main");
            assert_eq!(sim.log, wanted, "{condition}");
        }
    }
}

#[test]
fn string_ordering_is_rejected_in_plain_and_execute_conditions() {
    for header in ["if name > 3", "as @a if name > 3", "as @a unless name > 3"] {
        let source =
            format!("def main():\n    name = \"abc\"\n    {header}:\n        /say wrong\n");
        let error = compile(&source).unwrap_err();
        assert!(
            error.contains("storage-backed (string)") && error.contains("'name'"),
            "{header}: {error}"
        );
    }
}

#[test]
fn execute_conditions_share_a_bounded_number_of_entity_objectives() {
    let mut sim = Sim::load(
        "def ready():\n    return 1\ndef main():\n    as @a if ready():\n        /say a\n    as @a if ready():\n        /say b\n    as @a if ready():\n        /say c\n    as @a if ready() and ready():\n        /say d\n",
        &["Alex", "Bo"],
    );
    let pooled = sim
        .function_text("_cobble_init")
        .lines()
        .filter(|line| {
            line.starts_with("scoreboard objectives add ") && !line.contains("sim.vars dummy")
        })
        .count();
    assert_eq!(
        pooled, 2,
        "condition objectives should be reused across call sites"
    );
    sim.call("main");
    assert_eq!(
        sim.log,
        ["Alex: a", "Bo: a", "Alex: b", "Bo: b", "Alex: c", "Bo: c", "Alex: d", "Bo: d"]
    );
}
