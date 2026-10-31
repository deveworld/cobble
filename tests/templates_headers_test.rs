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
