//! How source text becomes commands where comments, quotes, and escapes meet:
//! - a `#name` before a block header's `:` is a value, a `#` after it starts
//!   a comment
//! - an apostrophe inside a word doesn't hide a raw command's comment
//! - a `]` inside a quoted selector value doesn't end the selector
//! - string escapes decode as in Python and stay escaped in the output
//! - columns count characters after wide indentation
//! - deeply nested code with an error fails quickly
//! - `positioned as` and `rotated as` keep their selector
//! - two statements on one line are an error
//! - an `execute` keyword before block modifiers is optional
//! - a raw command goes on past a trailing `\\`, the way Minecraft reads a
//!   function file

mod common;

use common::mcsim::{compile, load_functions};
use std::fs;
use std::process::Command;

fn main_function(source: &str) -> String {
    let (_temp, output_dir) = compile(source).unwrap_or_else(|e| panic!("build failed: {e}"));
    load_functions(&output_dir)
        .remove("main")
        .expect("main function")
        .join("\n")
}

#[test]
fn hash_names_in_block_headers_are_values() {
    let main = main_function(
        r#"def main():
    if score #count obj matches 1:
        /say counted
    as @a[tag=#marked] if score #b obj matches 2: # a comment
        /say marked
    x = 1 # a trailing comment
    if x > 0:  # another comment
        /say positive
"#,
    );
    assert!(
        main.contains("execute if score #count obj matches 1 run say counted"),
        "{main}"
    );
    assert!(
        main.contains("execute as @a[tag=#marked] if score #b obj matches 2 run say marked"),
        "{main}"
    );
    assert!(main.contains("run say positive"), "{main}");
    assert!(!main.contains("comment"), "{main}");
}

#[test]
fn apostrophe_in_a_raw_command_keeps_the_comment_out() {
    let main = main_function(
        r#"def main():
    /say it's done # note
"#,
    );
    assert_eq!(main, "say it's done");
}

#[test]
fn bracket_inside_a_quoted_selector_value_stays_in_the_selector() {
    let main = main_function(
        r#"def main():
    as @e[name="a]b",tag=x]:
        /say hi
"#,
    );
    assert_eq!(main, r#"execute as @e[name="a]b",tag=x] run say hi"#);
}

#[test]
fn string_escapes_decode_like_python_and_stay_escaped_in_commands() {
    let main = main_function(
        r#"def main():
    msg = "one\ntwo\tthree §a \d \"q\""
    data = {"k": "a\\b"}
"#,
    );
    assert!(
        main.contains(r#"set value "one\ntwo\tthree §a \\d \"q\"""#),
        "{main}"
    );
    assert!(main.contains(r#"set value {k:"a\\b"}"#), "{main}");
    assert_eq!(main.lines().count(), 2, "a string split a command: {main}");
}

#[test]
fn line_break_in_a_plain_argument_is_an_error() {
    let error = compile(
        r#"from stdlib import timer

def main():
    timer.set("a\nb", 20)
"#,
    )
    .expect_err("a line break must not reach a command");
    assert!(error.contains("can't contain a line break"), "{error}");
}

#[test]
fn columns_count_characters_after_wide_indentation() {
    let temp = tempfile::TempDir::new().unwrap();
    let source = temp.path().join("main.cbl");
    fs::write(&source, "def main():\n\u{3000}\u{3000}x = = 1\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_cobble"))
        .arg("build")
        .arg(&source)
        .arg("-o")
        .arg(temp.path().join("out"))
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(stderr.contains("main.cbl:2:7:"), "{stderr}");
}

#[test]
fn errors_in_deeply_nested_code_fail_quickly() {
    let parens = format!(
        "def main():\n    x = {}1 +{}\n",
        "(".repeat(40),
        ")".repeat(40)
    );
    let mut nested = String::from("x = 1\ndef main():\n");
    for depth in 0..30 {
        nested.push_str(&format!("{}if x > {}:\n", "    ".repeat(depth + 1), depth));
    }
    nested.push_str(&format!("{}y = = 1\n", "    ".repeat(31)));
    // The CLI runs on the main thread's stack; test threads have less.
    let temp = tempfile::TempDir::new().unwrap();
    let file = temp.path().join("main.cbl");
    for source in [parens, nested] {
        fs::write(&file, source).unwrap();
        let started = std::time::Instant::now();
        let output = Command::new(env!("CARGO_BIN_EXE_cobble"))
            .arg("build")
            .arg(&file)
            .arg("-o")
            .arg(temp.path().join("out"))
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("error[parse]"));
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "took {:?}",
            started.elapsed()
        );
    }
}

#[test]
fn positioned_and_rotated_as_keep_their_selector() {
    let main = main_function(
        r#"def main():
    positioned as @s:
        /say here
    as @a rotated as @p:
        /say turned
"#,
    );
    assert!(
        main.contains("execute positioned as @s run say here"),
        "{main}"
    );
    assert!(
        main.contains("execute as @a rotated as @p run say turned"),
        "{main}"
    );
}

#[test]
fn two_statements_on_one_line_are_an_error() {
    for source in [
        "def main():\n    x = 1 y = 2\n",
        "def main():\n    return 1 2\n",
        "def main():\n    pass pass\n",
    ] {
        assert!(compile(source).is_err(), "accepted: {source}");
    }
}

#[test]
fn execute_keyword_before_modifiers_is_optional() {
    let main = main_function(
        r#"def main():
    execute as @a at @s:
        /say hi
    as @a at @s:
        /say hi
"#,
    );
    assert_eq!(
        main,
        "execute as @a at @s run say hi\nexecute as @a at @s run say hi"
    );
}

#[test]
fn a_raw_command_continues_after_a_backslash() {
    let main = main_function(
        r#"def main():
    /execute as @a \
            at @s \
        run say done # note
    /tellraw @a [ \
        {"text":"a"}, \
        {"text":"b"} \
    ]
    if score #a obj matches 1:
        /say in \
    a block
    /say last
"#,
    );
    assert_eq!(
        main.lines().collect::<Vec<_>>(),
        [
            "execute as @a at @s run say done",
            r#"tellraw @a [ {"text":"a"}, {"text":"b"} ]"#,
            "execute if score #a obj matches 1 run say in a block",
            "say last",
        ]
    );
}

#[test]
fn a_continued_raw_command_takes_placeholders() {
    let (_temp, output_dir) = compile(
        r#"def give(item, count):
    /give @a \
        {item} \
        {count}

def main():
    give("minecraft:stone", 2)
"#,
    )
    .unwrap_or_else(|e| panic!("build failed: {e}"));
    assert_eq!(
        load_functions(&output_dir).remove("give").expect("give"),
        ["$give @a $(item) $(count)"]
    );
}

#[test]
fn a_continued_raw_command_reads_crlf_and_a_missing_final_newline() {
    let main = main_function("def main():\r\n    /say a \\\r\n      b\r\n    /say c \\\n      d");
    assert_eq!(main, "say a b\nsay c d");
}

#[test]
fn a_backslash_in_a_comment_continues_nothing() {
    let main = main_function("def main():\n    /say a # see C:\\\n    /say b\n");
    assert_eq!(main, "say a\nsay b");
}

#[test]
fn a_string_keeps_lines_that_look_like_a_continued_command() {
    let main = main_function(
        r#"def main():
    x = """
/say a \
b
"""
"#,
    );
    assert_eq!(
        main,
        r#"data modify storage sim:global locals.main.x set value "\n/say a \\\nb\n""#
    );
}
