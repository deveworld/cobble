mod common;

use common::mcsim::Sim;

#[test]
fn mixed_headers_keep_python_boolean_literals() {
    let mut sim = Sim::load(
        r#"def main():
    if False or entity @a[tag=missing]:
        /say wrong
    else:
        /say false
    if entity @a and not False and True:
        /say true
    n = 0
    while entity @a and n < 2 and False:
        n += 1
    /say {n}
    as @a if entity @s and False:
        /say wrong
"#,
        &["Alex"],
    );
    sim.call("main");
    assert_eq!(sim.log, ["Server: false", "Server: true", "Server: 0"]);
}

#[test]
fn header_placeholders_are_values_when_used_as_python_operands() {
    let mut sim = Sim::load(
        r#"const N = 2
def show(p):
    if entity @a and {N} > 1 and {p} in [5, 6]:
        /say mixed
    as @a if {p} >= 3:
        /say parameter
    if {p} > 1:
        /say plain
def main():
    show(5)
"#,
        &["Alex"],
    );
    sim.call("main");
    assert_eq!(
        sim.log,
        ["Server: mixed", "Alex: parameter", "Server: plain"]
    );
}

#[test]
fn minecraft_while_headers_receive_macro_arguments_on_every_call() {
    let mut sim = Sim::load(
        r#"def drain(tag):
    n = 0
    while entity @a[tag={tag}] and n < 2:
        n += 1
        if n == 2:
            /tag @a remove {tag}
    /say {n}
def main():
    /tag @a add active
    drain("active")
"#,
        &["Alex"],
    );
    sim.call("main");
    assert_eq!(sim.log, ["Server: 2"]);
}

#[test]
fn raw_function_conditions_short_circuit_with_cobble_conditions() {
    let mut sim = Sim::load(
        r#"calls = 0
def effect():
    global calls
    calls += 1
    return 0
def main():
    if True or function sim:effect:
        /say yes
    if entity @a or function sim:effect:
        /say raw
    n = 0
    while n < 2 or function sim:effect:
        n += 1
    /say {n} {calls}
"#,
        &["Alex"],
    );
    sim.call("main");
    assert_eq!(sim.log, ["Server: yes", "Server: raw", "Server: 2 1"]);
}

#[test]
fn logical_keywords_can_be_raw_score_holders_and_objectives() {
    for word in ["and", "or", "not"] {
        let source = format!("def main():\n    n = 0\n    /scoreboard players set {word} temp 5\n    if score {word} temp matches 5:\n        /say holder\n    /scoreboard objectives add {word} dummy\n    /scoreboard players set value {word} 3\n    if score value {word} matches 3 and entity @a:\n        /say objective\n    if score {word} temp > value {word} and True:\n        /say comparison\n");
        let mut sim = Sim::load(&source, &["Alex"]);
        sim.call("main");
        assert_eq!(
            sim.log,
            ["Server: holder", "Server: objective", "Server: comparison"],
            "{word}"
        );
    }
}

#[test]
fn module_compound_declarations_are_visible_to_earlier_functions() {
    for block in ["if True:", "x = 1\nif x == 1:", "for i in [1, 2]:"] {
        let source = format!("def size():\n    return len(total)\n{block}\n    total = [1, 2]\ndef main():\n    n = size()\n    /say {{n}}\n");
        let mut sim = Sim::load(&source, &[]);
        sim.call("main");
        assert_eq!(sim.log, ["Server: 2"], "{source}");
    }
}

#[test]
fn module_compound_declarations_still_reject_cross_file_duplicates() {
    let error = common::mcsim::compile_project(&[
        ("a.cbl", "x = 1\nif x == 1:\n    total = 2\n"),
        ("main.cbl", "import a\ntotal = 9\ndef main():\n    pass\n"),
    ])
    .unwrap_err();
    assert!(error.contains("defined in both"), "{error}");
}

#[test]
fn module_commands_and_execute_headers_cannot_read_later_declarations() {
    for command in ["/say {later}", "as @a if later > 0:\n    /say late"] {
        let source = format!("{command}\nlater = 5\ndef main():\n    pass\n");
        let error = common::mcsim::compile(&source).unwrap_err();
        assert!(error.contains("before its assignment"), "{error}");
    }
}

#[test]
fn single_quoted_text_parameters_preserve_quotes_and_backslashes() {
    let mut sim = Sim::load(
        r#"def show(msg):
    /tellraw @a 'Hello {msg}'
    /tellraw @a {text:'{msg}',color:gold}
    /tellraw @a {text:'a " b',extra:["{msg}"]}
def main():
    show("it's")
    show("back\\slash")
"#,
        &["Alex"],
    );
    sim.call("main");
    assert_eq!(
        sim.log,
        [
            "Hello it's",
            "it's",
            "a \" bit's",
            "Hello back\\slash",
            "back\\slash",
            "a \" bback\\slash"
        ]
    );
}

#[test]
fn mixed_header_text_operands_keep_unicode() {
    let mut sim = Sim::load(
        r#"def check(value):
    if entity @a and {value} == "안녕 😀":
        /say unicode
def main():
    check("안녕 😀")
"#,
        &["Alex"],
    );
    sim.call("main");
    assert_eq!(sim.log, ["Server: unicode"]);
}

#[test]
fn literal_comparisons_keep_values_that_never_enter_a_score() {
    let mut sim = Sim::load(
        r#"const RATE = 0.5
def main():
    if 0.5 > 0:
        /say fraction
    if entity @a and RATE != 0:
        /say constant
    if 9999999999 > 2147483647:
        /say large
    if 0.5 == 0:
        /say wrong
    if -0.5 < 0 and -RATE > -1:
        /say negative
    n = -1
    if n < -0.5:
        /say threshold
"#,
        &["Alex"],
    );
    sim.call("main");
    assert_eq!(
        sim.log,
        [
            "Server: fraction",
            "Server: constant",
            "Server: large",
            "Server: negative",
            "Server: threshold"
        ]
    );
}

#[test]
fn merged_tag_errors_point_to_the_declaration_containing_the_bad_entry() {
    let error = common::mcsim::compile_project(&[
        ("a.cbl", "from stdlib import datapack\ndatapack.function_tag(\"checks\", [\"sim:ok\"])\ndef ok():\n    pass\n"),
        ("main.cbl", "import a\nfrom stdlib import datapack\ndatapack.function_tag(\"checks\", [\"sim:missing\"])\n"),
    ]).unwrap_err();
    assert!(error.contains("main.cbl:3:1"), "{error}");
    assert!(error.contains("sim:missing"), "{error}");
}
