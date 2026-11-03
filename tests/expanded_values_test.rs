mod common;

use common::mcsim::Sim;
use serde_json::json;

fn run(source: &str) -> Sim {
    let mut sim = Sim::load(source, &["Alex"]);
    sim.call("main");
    sim
}

#[test]
fn safe_message_entries_keep_source_locations() {
    let (_temp, output) = common::mcsim::compile(
        "def show(msg):\n    /tellraw @a {msg}\ndef main():\n    show(\"hi\")\n",
    )
    .unwrap();
    let map: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(output.join(".cobble/source_map.json")).unwrap(),
    )
    .unwrap();
    let entries: Vec<_> = map["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| {
            entry["generated_path"]
                .as_str()
                .unwrap()
                .contains("_cobble_text_entry_")
        })
        .collect();
    assert!(!entries.is_empty());
    for entry in entries {
        assert_eq!(entry["source"]["line"], 2, "{entry}");
    }
}

#[test]
fn plain_message_parameters_keep_quotes_backslashes_and_forwarded_values() {
    let mut sim = Sim::load(
        r#"def plain(msg):
    /tellraw @a {msg}
    /tellraw @a Hi {msg}
    /execute as @a run tellraw @s {msg}
def forward(msg):
    plain(msg)
def recursive(msg, n):
    if n > 0:
        recursive(msg, n - 1)
    else:
        plain(msg)
def main():
    forward("say \"hi\" \\ end")
    recursive("한글 \\" , 1)
"#,
        &["Alex"],
    );
    sim.call("main");
    sim.call_with("plain", json!({"msg":"inline text"}));
    assert_eq!(
        sim.log,
        [
            "say \"hi\" \\ end",
            "Hi say \"hi\" \\ end",
            "say \"hi\" \\ end",
            "한글 \\",
            "Hi 한글 \\",
            "한글 \\",
            "inline text",
            "Hi inline text",
            "inline text",
        ]
    );
}

#[test]
fn safe_message_entries_preserve_recursive_arguments_and_return_values() {
    let mut sim = Sim::load(
        r#"out = []
def show(msg, n):
    /tellraw @a {msg}
    if n > 0:
        show("child \\", n - 1)
    /tellraw @a after {msg}
    return n + 4
def main():
    out.append(show("parent \"quoted\"", 1))
"#,
        &["Alex"],
    );
    sim.call("main");
    assert_eq!(
        sim.log,
        [
            "parent \"quoted\"",
            "child \\",
            "after child \\",
            "after parent \"quoted\""
        ]
    );
    assert_eq!(sim.storage["vars"]["out"], json!([5]));
}

#[test]
fn imported_plain_messages_use_safe_entries() {
    let mut sim = Sim::load_project(
        &[
            (
                "main.cbl",
                "from ui import show\ndef main():\n    show(\"a\\\"b\\\\c\")\n",
            ),
            ("ui.cbl", "def show(msg):\n    /tellraw @a Hi {msg}\n"),
        ],
        &["Alex"],
    );
    sim.call("main");
    assert_eq!(sim.log, ["Hi a\"b\\c"]);
}

#[test]
fn conditional_arguments_and_containers_keep_the_selected_value_type() {
    let sim = run(r#"out = []
def record(value):
    out.append(value)
def choose(c):
    record("x" if c else 1)
    record(1 if c else [2])
    record({"k": 3} if c else "s")
    x = "chosen" if c else [4]
    record(x)
    record([{"v": "nested" if c else 7}])
def main():
    choose(1)
    choose(0)
"#);
    assert_eq!(
        sim.storage["vars"]["out"],
        json!(["x",1,{"k":3},"chosen",[{"v":"nested"}],1,[2],"s",[4],[{"v":7}]])
    );
}

#[test]
fn selected_values_support_indexing_lengths_copies_and_comparisons() {
    let sim = run(r#"out = []
def choose(c):
    x = "abc" if c else [4, 5]
    y = x
    out.append(x[0])
    out.append(len(y))
    out.append(1 if x == "abc" else 0)
    rows = ["text" if c else {"k": 7}]
    out.append(rows[0])
def main():
    choose(1)
    choose(0)
"#);
    assert_eq!(
        sim.storage["vars"]["out"],
        json!(["a",3,1,"text",4,2,0,{"k":7}])
    );
}

#[test]
fn mixed_choice_reassignments_respect_existing_variable_types() {
    for value in ["\"text\" if c else 7", "c and \"text\"", "c or [7]"] {
        let source = format!(
            r#"out = []
def choose(c):
    x = 0
    x = {value}
    out.append(x)
def main():
    choose(1)
    choose(0)
"#
        );
        let error = common::mcsim::compile(&source).unwrap_err();
        assert!(error.contains("Type mismatch for variable 'x'"), "{error}");
    }
}

#[test]
fn later_numeric_writes_keep_earlier_dynamic_reads_current_in_loops() {
    let sim = run(r#"out = []
def choose(given):
    items = [9]
    a = given or items.count(9)
    n = 0
    while n < 2:
        if a > 0:
            a = 1
            a /= -1
            out.append(a)
        n += 1
def main():
    choose(3)
    choose(0)
"#);
    assert_eq!(sim.storage["vars"]["out"], json!([-1.0, -1.0]));
}

#[test]
fn mixed_operand_choices_short_circuit_and_keep_the_operand() {
    let sim = run(r#"out = []
calls = 0
def effect():
    global calls
    calls += 1
    return 8
def choose(a, b):
    out.append(a and b)
    out.append(a or b)
def main():
    choose([], {"k": 1})
    choose([2], "s")
    out.append([] and effect())
    out.append([3] or effect())
    out.append("" or effect())
"#);
    assert_eq!(sim.var("calls"), 1);
    assert_eq!(
        sim.storage["vars"]["out"],
        json!([[],{"k":1},"s",[2],[],[3],8])
    );
}

#[test]
fn mixed_conditional_evaluates_only_one_branch_and_survives_recursion() {
    let sim = run(r#"out = []
calls = 0
def effect():
    global calls
    calls += 1
    return 9
def recurse(n):
    held = "text" if n else [5]
    if n > 0:
        recurse(n - 1)
    out.append(held)
def choose(c):
    out.append(effect() if c else "no")
def main():
    choose(0)
    choose(1)
    recurse(2)
"#);
    assert_eq!(sim.var("calls"), 1);
    assert_eq!(
        sim.storage["vars"]["out"],
        json!(["no", 9, [5], "text", "text"])
    );
}

#[test]
fn an_integer_and_boolean_choice_assigns_to_a_fresh_name() {
    // Both branches are scores, but the two branch assignments must not be
    // type-checked against each other: the running pack picks one.
    let sim = run(r#"out = []
def choose(c):
    x = 7 if c else (c > 0)
    out.append(x)
def main():
    choose(1)
    choose(0)
"#);
    assert_eq!(sim.storage["vars"]["out"], json!([7, 0]));
}

#[test]
fn a_for_loop_refuses_a_choice_that_is_not_a_list_at_run_time() {
    // A value of unknown type may be a map or a number, which `for` can't
    // walk. It shows a refusal naming the source line and runs no pass,
    // instead of walking a map's size with an empty item.
    let mut sim = Sim::load(
        r#"out = []
def over_list(c):
    x = [1, 2] if c else {"a": 1}
    for item in x:
        out.append(item)
def over_map(c):
    y = [1] if c else {"a": 1, "b": 2}
    for item in y:
        out.append(item)
def over_number(c):
    z = [1] if c else 3
    for item in z:
        out.append(item)
def over_text():
    t = "ab"
    for ch in t:
        out.append(ch)
def main():
    over_list(1)
    over_map(0)
    over_number(0)
    over_text()
"#,
        &["Alex"],
    );
    sim.call("main");
    assert_eq!(sim.storage["vars"]["out"], json!([1, 2, "a", "b"]));
    let messages: Vec<&str> = sim
        .log
        .iter()
        .map(String::as_str)
        .filter(|line| line.contains("a for loop needs a list or text"))
        .collect();
    assert_eq!(messages.len(), 2, "one refusal per bad loop: {messages:?}");
    assert!(
        messages[0].contains(":8:"),
        "names the source line: {messages:?}"
    );
    assert!(
        messages[1].contains(":12:"),
        "names the source line: {messages:?}"
    );
}
