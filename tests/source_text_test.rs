//! How source text becomes commands where comments, quotes, and escapes meet:
//! - a `#name` before a block header's `:` is a value, a `#` after it starts
//!   a comment
//! - an apostrophe inside a word doesn't hide a raw command's comment
//! - a `]` inside a quoted selector value doesn't end the selector
//! - string escapes decode as in Python and stay escaped in the output
//! - columns count characters after wide indentation

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
