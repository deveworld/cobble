//! Regression tests for bugs found by compiling `examples/showcase` and
//! running its self-test on a real server:
//! - a comparison of two macro parameters lowered only the left side, so the
//!   right side was compared as an unset score name
//! - multi-line `"""` docstrings were tokenized line by line, so their bodies
//!   compiled as code (and backticks failed to tokenize)
//! - tokenizer errors carried no source location
//! - `#ns:tag` in an execute header was read as a comment, and the
//!   structural check then looped forever on the `#`
//! - statement-level raw conditions (`if score ...:`) were scanned for
//!   undefined variables
//! - an unknown `@Name` in `as`/`at` reached Minecraft verbatim
//! - `storage.read_score()` always passed a scale, so list sizes and string
//!   lengths read as 0

mod common;

use common::mcsim::{compile, load_functions, Sim};
use std::sync::mpsc;
use std::time::Duration;

#[test]
fn comparing_two_parameters_lowers_both_sides() {
    let mut sim = Sim::load(
        r#"
def check(actual, expected):
    if actual == expected:
        /say same
    else:
        /say different

def main():
    check(3, 3)
    check(3, 4)
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(sim.log, ["Server: same", "Server: different"]);
}

#[test]
fn storage_read_score_reads_sizes_and_lengths() {
    let mut sim = Sim::load(
        r#"
import stdlib

def main():
    storage.set("events", [10, 20, 30])
    storage.set("name", "alex")
    storage.set("ratio", 3)
    count = 0
    length = 0
    scaled = 0
    storage.read_score("count", "events")
    storage.read_score("length", "name")
    storage.read_score("scaled", "ratio", 10)
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(sim.var("count"), 3);
    assert_eq!(sim.var("length"), 4);
    assert_eq!(sim.var("scaled"), 30);
    assert!(sim
        .function_text("main")
        .contains("run data get storage sim:global events\n"));
}

#[test]
fn multiline_docstring_bodies_are_not_compiled() {
    let (_temp, output_dir) = compile(
        r#""""Module doc with `backticks`.

/kill @e
"""

def main():
    """Body doc.

    x = undefined_value + 1
    /kill @e
    helper("@a")
    """
    /say real

def helper(player, message):
    pass
"#,
    )
    .unwrap_or_else(|error| panic!("build failed: {error}"));
    let functions = load_functions(&output_dir);
    assert_eq!(functions["main"], ["say real"]);
    assert!(functions
        .values()
        .flatten()
        .all(|command| !command.contains("kill")));
}

#[test]
fn tokenizer_errors_report_their_location() {
    let error = cobble::parser::tokenize_spanned_located("def f():\n    x = 1 + `\n").unwrap_err();
    assert!(error.message.contains("Unexpected character"), "{error}");
    assert_eq!((error.span.line, error.span.column), (2, 13));

    let error = cobble::parser::tokenize_spanned_located("def f():\n    \"\"\"open\n    /say\n")
        .unwrap_err();
    assert!(
        error.message.contains("Unterminated triple-quoted string"),
        "{error}"
    );
    assert_eq!((error.span.line, error.span.column), (2, 5));
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
