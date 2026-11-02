//! Python forms the compiler lowers: comparing text, lists, and maps, `in`,
//! conditional expressions, tuple assignment, list methods, `min`, `max`,
//! `abs`, `sum`, slices, joining and repeating, `enumerate`, `match` on text,
//! and `and`/`or` giving an operand.
//!
//! Each `tests/fixtures/python_forms/<name>.cbl` runs in the command simulator
//! and must print `<name>.out`, which is what the same program prints as
//! Python. The random programs in `differential_test.rs` and the in-game
//! self-test of the showcase cover the same forms.

mod common;

use common::mcsim::{compile, compile_project, load_functions, Sim};
use std::fs;
use std::path::Path;

#[test]
fn programs_print_what_python_prints() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/python_forms");
    let mut programs = 0;
    for entry in fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("cbl") {
            continue;
        }
        programs += 1;
        let source = fs::read_to_string(&path).unwrap();
        let expected = fs::read_to_string(path.with_extension("out")).unwrap();
        let mut sim = Sim::load(&source, &["Steve"]);
        sim.call("main");
        let printed: Vec<&str> = sim
            .log
            .iter()
            .map(|line| line.strip_prefix("Server: ").unwrap_or(line))
            .collect();
        assert_eq!(
            printed,
            expected.lines().collect::<Vec<_>>(),
            "{}",
            path.display()
        );
    }
    assert!(programs >= 80, "found only {programs} programs");
}

#[test]
fn tuple_assignment_keeps_a_score_that_a_raw_command_writes() {
    let mut sim = Sim::load(
        r#"
def show(p):
    b = p
    /scoreboard players set b temp 3
    if p > 5:
        b, p = p, b
    /say {b}

def main():
    show(0)
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(sim.log, ["Server: 3"]);
}

#[test]
fn snbt_text_components_show_variables() {
    let mut sim = Sim::load(
        r#"
score = 5
name = "Steve"

def greet(who):
    /tellraw @a [{text:"Hi ",color:yellow},{text:f"{who}",color:aqua}]

def main():
    /tellraw @a {text:f"Score {score}",color:yellow}
    /tellraw @a [{text:"Score ",color:yellow},{text:f"{score}",color:aqua}]
    /tellraw @a {text:"Hi ",extra:[{text:f"{name}",bold:true}]}
    /tellraw @a {text:f"it's {score}",italic:1b}
    greet("Alex")
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(
        sim.log,
        ["Score 5", "Score 5", "Hi Steve", "it's 5", "Hi Alex"]
    );

    let error =
        compile("n = 5\n\ndef main():\n    /tellraw @a {text:\"x\",font:f\"{n}\"}\n").unwrap_err();
    assert!(error.contains("Other fields take no variables"), "{error}");
}

#[test]
fn words_with_one_use_as_keywords_work_as_names() {
    for word in [
        "end", "to", "by", "at", "create", "define", "match", "case", "const", "asat",
    ] {
        let source = format!(
            "{word} = 4\n\ndef {word}_copy({word}):\n    return {word} + 1\n\ndef main():\n    x = {word}_copy({word})\n    /tellraw @a [f\"{{x}}\"]\n"
        );
        let mut sim = Sim::load(&source, &[]);
        sim.call("main");
        assert_eq!(sim.log, ["5"], "{word}");
    }

    let mut sim = Sim::load(
        "def end():\n    /tellraw @a [\"ended\"]\n\ndef main():\n    end()\n",
        &[],
    );
    sim.call("main");
    assert_eq!(sim.log, ["ended"]);
}

#[test]
fn those_words_keep_their_keyword_forms() {
    let (_temp, output_dir) = compile(
        r#"
define @Boss = @e[tag=boss]
create {id:"minecraft:zombie"}
end

def main():
    create @Boss
    for i in range(0, 4) by 2:
        /say {i}
    at @s:
        /say here
    n = 2
    match n:
        case 1 to 3:
            /say low
"#,
    )
    .unwrap();
    let functions = load_functions(&output_dir);
    let main = functions["main"].join("\n");
    assert!(
        main.contains("summon") || main.contains("function"),
        "{main}"
    );
}

#[test]
fn execute_blocks_take_on_facing_and_summon() {
    let (_temp, output_dir) = compile(
        r#"
def main():
    as @e[type=interaction] on target at @s:
        /say a
    facing entity @p eyes:
        /say b
    summon minecraft:marker:
        /tag @s add made
"#,
    )
    .unwrap();
    let main = load_functions(&output_dir)["main"].join("\n");
    assert!(
        main.contains("execute as @e[type=interaction] on target at @s run say a"),
        "{main}"
    );
    assert!(
        main.contains("execute facing entity @p eyes run say b"),
        "{main}"
    );
    assert!(
        main.contains("execute summon minecraft:marker run tag @s add made"),
        "{main}"
    );
}

#[test]
fn helpers_reach_a_pack_only_when_it_uses_them() {
    let (_temp, output_dir) = compile("def main():\n    x = 1\n    /say {x}\n").unwrap();
    assert!(!load_functions(&output_dir)
        .keys()
        .any(|name| name.starts_with("_cobble_list") || name.starts_with("_cobble_text")));

    let (_temp, output_dir) =
        compile("items = [1, 2]\n\ndef main():\n    x = sum(items)\n    /say {x}\n").unwrap();
    let functions = load_functions(&output_dir);
    assert!(functions.contains_key("_cobble_list_sum"));
    assert!(!functions.contains_key("_cobble_list_sort"));
}

#[test]
fn files_of_a_pack_share_one_copy_of_a_helper() {
    let (_temp, output_dir) = compile_project(&[
        (
            "main.cbl",
            "from util import total\n\ndef main():\n    x = sum([1, 2]) + total()\n    /say {x}\n",
        ),
        ("util.cbl", "def total():\n    return sum([3, 4])\n"),
    ])
    .unwrap();
    assert!(load_functions(&output_dir).contains_key("_cobble_list_sum"));
}

#[test]
fn a_module_value_that_runs_commands_is_an_error() {
    let error = compile("items = [1, 2]\nlast = items.pop()\n").unwrap_err();
    assert!(error.contains("can't be set outside a function"), "{error}");
}

#[test]
fn wrong_uses_are_errors() {
    for (source, expected) in [
        (
            "def main():\n    a, b = 2\n",
            "Unsupported assignment target",
        ),
        (
            "def main():\n    x = 1\n    a, b = x\n",
            "can't be unpacked",
        ),
        ("def main():\n    a, b = 1, 2, 3\n", "Parse error"),
        (
            "def main():\n    items = [1]\n    x = items.sort()\n",
            "gives no value",
        ),
        (
            "def main():\n    items = [1]\n    items.remove()\n",
            "takes 1 argument",
        ),
        (
            "def main():\n    for a, b in [1, 2]:\n        /say {a}\n",
            "need `enumerate",
        ),
        (
            "def f(n):\n    return n\n\ndef main():\n    a = 1 < f(2) < 3\n",
            "`<` gives 1 or 0",
        ),
    ] {
        let error = compile(source).unwrap_err();
        assert!(error.contains(expected), "{source}\n{error}");
    }
}

#[test]
fn a_colon_in_a_subscript_separates_bounds_and_elsewhere_it_stays_in_ids() {
    let (_temp, output_dir) = compile(
        "ids = [\"a\", \"b\", \"c\"]\n\ndef main():\n    i = 0\n    j = 2\n    part = ids[i:j]\n    n = len(part)\n    /give @s minecraft:stone 1\n    /say {n}\n",
    )
    .unwrap();
    assert!(load_functions(&output_dir)["main"]
        .iter()
        .any(|line| line.contains("minecraft:stone")));
}
