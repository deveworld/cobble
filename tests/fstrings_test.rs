mod common;
use common::mcsim::{compile, Sim};

#[test]
fn command_strings_interpolate_only_with_an_f_prefix() {
    let mut sim = Sim::load(
        r#"def main():
    wave = 3
    /title @a title f"Wave {wave}"
    /tellraw @a {"text":f"Wave {wave}","color":"gold"}
    /tellraw @a {"text":"Wave {wave}"}
    /tellraw @a 'literal {missing} {{braces}}'
    /tellraw @a F'Wave {wave}'
"#,
        &["Alex"],
    );
    sim.call("main");
    assert_eq!(
        sim.log,
        [
            "Wave 3",
            "Wave 3",
            "Wave {wave}",
            "literal {missing} {{braces}}",
            "Wave 3"
        ]
    );
}

#[test]
fn formatted_command_parameters_keep_quotes_and_backslashes() {
    let mut sim = Sim::load(
        r#"def show(message):
    /tellraw @a f"Hello {message}"
    /tellraw @a {text:f'{message}',extra:[" {message}"]}
def main():
    show("it's \"quoted\" back\\slash")
"#,
        &["Alex"],
    );
    sim.call("main");
    assert_eq!(
        sim.log,
        [
            "Hello it's \"quoted\" back\\slash",
            "it's \"quoted\" back\\slash {message}"
        ]
    );
}

#[test]
fn strings_stay_literal_when_a_loop_unrolls() {
    let mut sim = Sim::load(
        r#"def main():
    for i in [1, 2]:
        /tellraw @a ["{i}",f" {i}"," {{i}}"]
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(sim.log, ["{i} 1 {{i}}", "{i} 2 {{i}}"]);
}

#[test]
fn literal_fields_do_not_read_later_variables_or_unknown_names() {
    compile(
        r#"/tellraw @a "{later} {missing}"
later = 2
def main():
    /tellraw @a "{local}"
    local = 1
"#,
    )
    .unwrap();
    let error = compile("def main():\n    /tellraw @a f\"{missing}\"\n").unwrap_err();
    assert!(error.contains("missing"), "{error}");
}

#[test]
fn raw_arguments_and_f_string_escapes_keep_their_meanings() {
    let mut sim = Sim::load(
        r#"def show(p):
    /say raw {p}
    /tellraw @a f"{{{p}}}"
    /tellraw @a "{{{p}}}"
    /say it's {p}
def main():
    show(7)
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(
        sim.log,
        ["Server: raw 7", "{7}", "{{{p}}}", "Server: it's 7"]
    );
}

#[test]
fn formatted_values_work_as_expressions_and_preserve_runtime_text() {
    let mut sim = Sim::load(
        r#"def show(value):
    message = f"[{value}]"
    /tellraw @a f"{message}"
def main():
    wave = 3
    message = f"Wave {wave}"
    /tellraw @a f"{message}"
    show("say \"hi\" back\\slash")
"#,
        &["Alex"],
    );
    sim.call("main");
    assert_eq!(sim.log, ["Wave 3", "[say \"hi\" back\\slash]"]);
}

#[test]
fn formatting_does_not_call_a_user_function_named_str() {
    let mut sim = Sim::load(
        r#"def str(value):
    return 99
def main():
    value = 3
    message = f"{value}"
    /tellraw @a f"{message}"
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(sim.log, ["3"]);
}

#[test]
fn text_helpers_distinguish_literal_and_formatted_strings() {
    let mut sim = Sim::load(
        r#"from stdlib import text
def show(value):
    text.tellraw("@a", "literal {value}")
    text.tellraw("@a", f"formatted {value}")
    text.tellraw("@a", text.colored(f"styled {value}", "gold"))
def main():
    show("say \"hi\" back\\slash")
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(
        sim.log,
        [
            "literal {value}",
            "formatted say \"hi\" back\\slash",
            "styled say \"hi\" back\\slash"
        ]
    );
}

#[test]
fn unsupported_formatting_is_reported_instead_of_emitted_as_literal_text() {
    for source in [
        "def main():\n    /tellraw @a f\"{value + 1}\"\n",
        "value = f\"{name!r}\"\n",
        "value = f\"{name:02}\"\n",
        "value = f\"{name\"\n",
        "def main():\n    /tellraw @a f\"{name\"\n",
        "value = f\"unterminated\n",
        "value = f\"\"\"triple\"\"\"\n",
        "def main():\n    /tellraw @a f\"\"\"triple\"\"\"\n",
    ] {
        assert!(compile(source).is_err(), "{source}");
    }
}

#[test]
fn stored_formatting_refuses_containers_clearly() {
    let error = compile("def main():\n    items = [1, 2]\n    value = f\"{items}\"\n").unwrap_err();
    assert!(error.contains("text and numbers"), "{error}");
    let mut sim = Sim::load(
        r#"def show(value):
    message = f"{value}"
    /tellraw @a f"{message}"
def main():
    show(["a", "b"])
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(sim.log, ["An f-string value must be text or a number.", ""]);
}

#[test]
fn formatted_fields_are_checked_and_literal_escape_braces_stay_literal() {
    let diagnostics =
        cobble::diagnostics::analyze_source("def main():\n    value = f\"hello {missing}\"\n");
    assert!(
        diagnostics
            .iter()
            .any(|d| d.kind == "undefined-variable" && d.message.contains("missing")),
        "{diagnostics:?}"
    );
    let mut sim = Sim::load(
        r#"def main():
    wave = 3
    label = f"\u007bwave\u007d / {wave}"
    other = "["+f"{wave}"+"]"
    slash = f"\{wave} / \\{wave}"
    /tellraw @a f"{label} {other}"
    /tellraw @a f"{slash}"
    # f"{not_a_read}"
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(sim.log, ["{wave} / 3 [3]", "\\3 / \\3"]);
}

#[test]
fn literal_resource_strings_do_not_expand_parameter_names() {
    let (_temp, out) = compile(
        r#"from stdlib import datapack
def define(value):
    datapack.dialog("notice", {"type":"minecraft:notice","title":"{value}"})
"#,
    )
    .unwrap();
    let json: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(out.join("data/sim/dialog/notice.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(json["title"], "{value}");
}

#[test]
fn module_initializers_can_build_formatted_values() {
    let mut sim = Sim::load(
        r#"wave = 3
label = f"Wave {wave}"
def main():
    /tellraw @a f"{label}"
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(sim.log, ["Wave 3"]);
}

#[test]
fn fractional_arguments_used_only_in_f_strings_keep_their_precision() {
    let mut sim = Sim::load(
        r#"from stdlib import text
def show(value):
    label = f"stored {value}"
    /tellraw @a f"{label}"
    text.tellraw("@a", f"helper {value}")
def forward(value):
    show(value)
def main():
    forward(0.6)
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(sim.log, ["stored 0.6", "helper 0.6"]);
}

#[test]
fn native_macros_keep_literal_braces_and_can_share_a_formatted_message() {
    let mut sim = Sim::load(
        r#"def show(value):
    /tellraw @a {"text":"literal {missing} $(value)"}
    /tellraw @a {"text":f"{value} $(value)"}
def main():
    show(3)
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(sim.log, ["literal {missing} 3", "3 3"]);
}

#[test]
fn execute_headers_obey_the_same_string_boundary() {
    compile(
        r#"def main():
    as @a[name="{later}"]:
        /say literal
    if entity @a[name="{later}"]:
        /say literal
    later = "Alex"
"#,
    )
    .unwrap();
    assert!(compile(
        r#"def main():
    as @a[name=f"{later}"]:
        /say formatted
    later = "Alex"
"#
    )
    .is_err());
}

#[test]
fn mixed_minecraft_conditions_keep_formatted_fractional_arguments() {
    let mut sim = Sim::load(
        r#"def check(value):
    if entity @a and f"{value}" == "0.6":
        /say precise
def main():
    check(0.6)
"#,
        &["Alex"],
    );
    sim.call("main");
    assert_eq!(sim.log, ["Server: precise"]);
}
