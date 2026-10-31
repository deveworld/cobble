//! Placeholders, stdlib helper arguments, execute headers, and macro text:
//! - a modifier word inside an NBT path, a block state, or SNBT stays part
//!   of the condition, and a variable named like a modifier is a variable
//! - a number written with a leading dot keeps the space before it
//! - `on`, `anchored`, `align`, `summon`, and `asat` take placeholders

mod common;

use common::mcsim::{compile, load_functions, Sim};

fn functions(source: &str) -> std::collections::HashMap<String, Vec<String>> {
    let (_temp, output_dir) = compile(source).unwrap_or_else(|e| panic!("build failed: {e}"));
    load_functions(&output_dir)
}

fn function(source: &str, name: &str) -> String {
    functions(source)
        .remove(name)
        .unwrap_or_else(|| panic!("function `{name}` not generated"))
        .join("\n")
}

#[test]
fn modifier_words_inside_a_condition_stay_in_the_condition() {
    let mut sim = Sim::load(
        r#"
def main():
    /data modify storage sim:a x set value {in:{y:1},on:1b}
    if data storage sim:a x.in.y:
        /say path
    if data storage sim:a x.on:
        /say on
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(sim.log, ["Server: path", "Server: on"]);
    let main = sim.function_text("main");
    assert!(
        main.contains("execute if data storage sim:a x.in.y run say path"),
        "{main}"
    );

    let check = function(
        r#"
def check(facing):
    if block ~ ~ ~ minecraft:furnace[facing={facing}]:
        /say found

def main():
    check("north")
"#,
        "check",
    );
    assert_eq!(
        check,
        "$execute if block ~ ~ ~ minecraft:furnace[facing=$(facing)] run say found"
    );

    let main = function(
        r#"
def main():
    if data storage sim:a flag{on:1b}:
        /say a
    if data storage sim:a x{if:1b}:
        /say b
    store result storage sim:a x.on int 1:
        /say c
"#,
        "main",
    );
    assert_eq!(
        main,
        "execute if data storage sim:a flag{on:1b} run say a\n\
         execute if data storage sim:a x{if:1b} run say b\n\
         execute store result storage sim:a x.on int 1 run say c"
    );
}

#[test]
fn a_variable_named_like_a_modifier_is_a_variable() {
    let mut sim = Sim::load(
        r#"
on = 1

def main():
    as @a if on == 1:
        /say found
    if score on temp matches 1:
        /say score
"#,
        &["Alex"],
    );
    sim.call("main");
    assert_eq!(sim.log, ["Alex: found", "Server: score"]);
}

#[test]
fn a_number_with_a_leading_dot_keeps_its_space() {
    let main = function(
        r#"
def main():
    positioned 1.5 -60 .5:
        /say a
    positioned .5 .5 .5:
        /say b
    rotated 90 .5:
        /say c
    store result storage sim:a v float .5:
        /say d
"#,
        "main",
    );
    assert_eq!(
        main,
        "execute positioned 1.5 -60 .5 run say a\n\
         execute positioned .5 .5 .5 run say b\n\
         execute rotated 90 .5 run say c\n\
         execute store result storage sim:a v float .5 run say d"
    );
}

#[test]
fn every_modifier_takes_a_placeholder() {
    let functions = functions(
        r#"
def on_rel(rel):
    on {rel}:
        /say a

def align_axes(axes):
    align {axes}:
        /say b

def anchored_at(a):
    anchored {a}:
        /say c

def summon_kind(kind):
    summon {kind}:
        /say d

def asat_who(who):
    asat {who}:
        /say e

def main():
    on_rel("vehicle")
    align_axes("xz")
    anchored_at("eyes")
    summon_kind("pig")
    asat_who("@a")
"#,
    );
    let text = |name: &str| functions[name].join("\n");
    assert_eq!(text("on_rel"), "$execute on $(rel) run say a");
    assert_eq!(text("align_axes"), "$execute align $(axes) run say b");
    assert_eq!(text("anchored_at"), "$execute anchored $(a) run say c");
    assert_eq!(text("summon_kind"), "$execute summon $(kind) run say d");
    assert_eq!(text("asat_who"), "$execute as $(who) at @s run say e");
}

#[test]
fn brace_escapes_follow_python_format_strings() {
    // Each expected line is Python's `str.format` of the same text, with
    // `x="$(x)"`, `n="$(n)"`, and `k="$(k)"`.
    let functions = functions(
        r#"
def tags(n):
    /summon pig ~ ~ ~ {{Tags:["{n}"]}}

def wrap(x):
    /say {{ {x} }}
    /say {{{x}}}
    /say {{y={x}}}
    /kill @e[scores={{obj={x}}}]

def key(k):
    /data merge storage sim:a {{{k}:1}}

def main():
    /data merge storage sim:a {{a:{{b:1}},c:2}}
    /say {{{{a}}}}
    /say Type {{name}} to see a placeholder
    /tellraw @a {"text":"a","hover_event":{"action":"show_text","value":"b"}}
    tags("t")
    wrap("w")
    key("k")
"#,
    );
    let text = |name: &str| functions[name].join("\n");
    assert_eq!(text("tags"), r#"$summon pig ~ ~ ~ {Tags:["$(n)"]}"#);
    assert_eq!(
        text("wrap"),
        "$say { $(x) }\n$say {$(x)}\n$say {y=$(x)}\n$kill @e[scores={obj=$(x)}]"
    );
    assert_eq!(text("key"), "$data merge storage sim:a {$(k):1}");
    let main = text("main");
    for line in [
        "data merge storage sim:a {a:{b:1},c:2}",
        "say {{a}}",
        "say Type {name} to see a placeholder",
        r#"tellraw @a {"text":"a","hover_event":{"action":"show_text","value":"b"}}"#,
    ] {
        assert!(
            main.lines().any(|command| command == line),
            "{line}\n{main}"
        );
    }
}

#[test]
fn an_escaped_placeholder_in_a_loop_is_filled_in() {
    let mut sim = Sim::load(
        r#"
def main():
    for i in [1, 2]:
        /say {{n={i}}} {{i}}
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(sim.log, ["Server: {n=1} {i}", "Server: {n=2} {i}"]);
}

#[test]
fn a_placeholder_between_escapes_is_checked() {
    let error = compile(
        r#"
def main():
    /say {{y={nope}}}
"#,
    )
    .expect_err("an unknown placeholder inside an escape is an error");
    assert!(
        error.contains("Undefined command placeholder `nope`"),
        "{error}"
    );
}

#[test]
fn a_header_placeholder_must_be_a_parameter_or_a_const() {
    for (source, message) in [
        (
            "def main():\n    as @a[tag=t{nope}]:\n        /say hi\n",
            "Undefined placeholder `{nope}` in an execute header",
        ),
        (
            "def main():\n    if entity @a[tag=t{nope}]:\n        /say hi\n",
            "Undefined placeholder `{nope}` in an execute header",
        ),
        (
            "w = 1\ndef main():\n    as @a[tag=t{w}]:\n        /say hi\n",
            "Cannot use the variable `w` as a placeholder in an execute header",
        ),
        (
            "w = 1\ndef main():\n    if score @s {w} matches 1:\n        /say hi\n",
            "Cannot use the variable `w` as a placeholder in an execute header",
        ),
        (
            "w = 1\ndef main():\n    positioned ~ ~{w} ~:\n        /say hi\n",
            "Cannot use the variable `w` as a placeholder in an execute header",
        ),
    ] {
        let error = compile(source).expect_err(source);
        assert!(error.contains(message), "{source}\n{error}");
    }

    let main = function(
        r#"
const UP = 2

def lift(tag):
    as @a[tag={tag}] positioned ~ ~{UP} ~ if data storage sim:a {{x:{{y:1}}}}:
        /say up

def main():
    lift("t")
"#,
        "lift",
    );
    assert_eq!(
        main,
        "$execute as @a[tag=$(tag)] positioned ~ ~2 ~ if data storage sim:a {x:{y:1}} run say up"
    );
}

#[test]
fn a_selector_alias_takes_a_const() {
    let main = function(
        r#"
const R = 8
@Near = @e[distance=..{R}]

def main():
    /tellraw @a {"selector":"@Near"}
    /kill @Near
    as @Near:
        /say x
"#,
        "main",
    );
    assert_eq!(
        main,
        "tellraw @a {\"selector\":\"@e[distance=..8]\"}\n\
         kill @e[distance=..8]\n\
         execute as @e[distance=..8] run say x"
    );
    let error = compile("@Near = @e[distance=..{R}]\ndef main():\n    /kill @Near\n")
        .expect_err("an alias needs a const");
    assert!(
        error.contains("Selector alias `@Near` uses `{R}`"),
        "{error}"
    );
}

#[test]
fn macro_text_commands_keep_their_arguments() {
    let functions = functions(
        r#"
score = 4

def times(a, b, c):
    /title @a times {a} {b} {c}
    /title @a times $(a) 20 10

def raw_macro():
    /title @s actionbar $(prompt)
    /tellraw @a $(msg)
    /tellraw @a[tag=x, limit=1] {"text":"$(m)"}
    /title @a[tag=x, limit=1] title {"text":"$(m)"}
    /tellraw @a[name="a b"] {"text":"$(m)"}

def whole(msg):
    /tellraw @a {msg}
    /title @a[tag=x, limit=1] actionbar {msg}

def shown():
    /tellraw @a[name="a b"] {"text":"score {score}"}

def main():
    times(10, 20, 30)
    whole("hi there")
"#,
    );
    let text = |name: &str| functions[name].join("\n");
    assert_eq!(
        text("times"),
        "$title @a times $(a) $(b) $(c)\n$title @a times $(a) 20 10"
    );
    assert_eq!(
        text("raw_macro"),
        "$title @s actionbar $(prompt)\n\
         $tellraw @a $(msg)\n\
         $tellraw @a[tag=x, limit=1] {\"text\":\"$(m)\"}\n\
         $title @a[tag=x, limit=1] title {\"text\":\"$(m)\"}\n\
         $tellraw @a[name=\"a b\"] {\"text\":\"$(m)\"}"
    );
    // A placeholder that fills the whole text shows it as plain text.
    assert_eq!(
        text("whole"),
        "$tellraw @a {\"text\":\"$(msg)\"}\n$title @a[tag=x, limit=1] actionbar {\"text\":\"$(msg)\"}"
    );
    assert!(
        text("shown").starts_with("tellraw @a[name=\"a b\"] {"),
        "{}",
        text("shown")
    );
}

#[test]
fn an_escaped_name_is_text_next_to_a_variable() {
    let mut sim = Sim::load(
        r#"
coins = 3

def main():
    /tellraw @a {"text":"{{coins}} is {coins}"}
    /say {{coins}} is {coins}
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(sim.log, ["{coins} is 3", "Server: {coins} is 3"]);
}
