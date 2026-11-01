//! Fixes from the independent review after the backlog round: Minecraft
//! conditions joined to calls, whole-text placeholders holding quotes,
//! runtime loop variables after the loop, imported maps copied to locals,
//! `in` beside a Minecraft condition, late module reads at scale, module
//! loops in two files, joins in stdlib helper arguments, and per-entity
//! condition flags.

mod common;

use common::mcsim::{compile, compile_project, Sim};

fn compile_error(source: &str) -> String {
    match compile(source) {
        Ok(_) => panic!("expected a compile error for:\n{source}"),
        Err(error) => error,
    }
}

fn tag(sim: &mut Sim, player: &str, tag: &str) {
    sim.tags
        .entry(player.to_string())
        .or_default()
        .insert(tag.to_string());
}

/// Chat lines with the `Server: ` prefix of a line said by no one removed.
fn chat(sim: &Sim) -> Vec<String> {
    sim.log
        .iter()
        .map(|line| line.strip_prefix("Server: ").unwrap_or(line).to_string())
        .collect()
}

// --- A Minecraft condition joined to a call by `and` / `or` ---

const MIXED_CALLS: &str = r#"def f():
    /say f-called
    return 1
def g():
    /say g-called
    return 0
def main():
    if entity @e[tag=t] and f():
        /say and-ran
    if entity @e[tag=t] or g():
        /say or-ran
def left_call():
    if f() and entity @e[tag=t]:
        /say left-ran
def negated():
    if not entity @e[tag=t] and f():
        /say not-ran
def grouped():
    x = 1
    if x > 0 and (entity @e[tag=t] or g()):
        /say grouped-ran
def each():
    as @a if entity @s[tag=t] or f():
        /say each-ran
"#;

#[test]
fn a_minecraft_condition_and_a_call_short_circuit_like_python() {
    // Nobody is tagged: `and` skips f, `or` calls g, neither body runs.
    let mut sim = Sim::load(MIXED_CALLS, &["Alex", "Bo"]);
    sim.call("main");
    assert_eq!(chat(&sim), ["g-called"]);

    // Alex is tagged: `and` calls f and runs, `or` skips g and runs.
    let mut sim = Sim::load(MIXED_CALLS, &["Alex", "Bo"]);
    tag(&mut sim, "Alex", "t");
    sim.call("main");
    assert_eq!(chat(&sim), ["f-called", "and-ran", "or-ran"]);
}

#[test]
fn a_minecraft_condition_on_either_side_of_a_call_is_tested() {
    let mut sim = Sim::load(MIXED_CALLS, &["Alex"]);
    sim.call("left_call");
    sim.call("negated");
    sim.call("grouped");
    assert_eq!(chat(&sim), ["f-called", "f-called", "not-ran", "g-called"]);

    let mut sim = Sim::load(MIXED_CALLS, &["Alex"]);
    tag(&mut sim, "Alex", "t");
    sim.call("left_call");
    sim.call("negated");
    sim.call("grouped");
    assert_eq!(chat(&sim), ["f-called", "left-ran", "grouped-ran"]);
}

#[test]
fn a_per_entity_minecraft_condition_or_a_call_runs_the_call_only_when_needed() {
    let mut sim = Sim::load(MIXED_CALLS, &["Alex", "Bo"]);
    tag(&mut sim, "Alex", "t");
    sim.call("each");
    assert_eq!(
        chat(&sim),
        ["Bo: f-called", "Alex: each-ran", "Bo: each-ran"]
    );
}

// --- A parameter that fills a tellraw or title text ---

#[test]
fn a_whole_text_parameter_is_macro_text_in_a_wrapper() {
    let mut sim = Sim::load(
        r#"def show(msg):
    /tellraw @a {msg}
    /tellraw @a note: {msg}
def quoted(msg):
    /tellraw @a {"text":"{msg}"}
def bar(msg):
    /title @a actionbar {msg}
def raw(msg):
    /tellraw @a $(msg)
def main():
    show("plain sentence")
    show(7)
    quoted("say \"hi\" a\\b")
    raw("{\"text\":\"ready\"}")
"#,
        &["Alex"],
    );
    // Plain text is macro text, so a direct call with inline arguments
    // shows it too.
    assert_eq!(
        sim.function_text("show"),
        "$tellraw @a {\"text\":\"$(msg)\"}\n$tellraw @a {\"text\":\"note: $(msg)\"}"
    );
    assert_eq!(
        sim.function_text("bar"),
        "$title @a actionbar {\"text\":\"$(msg)\"}"
    );
    // A component's text reads the value from `args`, so a `"` or `\` works.
    assert!(
        sim.function_text("quoted").contains(r#""nbt":"_text.msg""#),
        "{}",
        sim.function_text("quoted")
    );
    // An author's `$(msg)` stays the raw macro text.
    assert_eq!(sim.function_text("raw"), "$tellraw @a $(msg)");
    sim.call("main");
    assert_eq!(
        sim.log,
        [
            "plain sentence",
            "note: plain sentence",
            "7",
            "note: 7",
            r#"say "hi" a\b"#,
            "ready",
        ]
    );
    sim.log.clear();
    sim.call_with("show", serde_json::json!({"msg": "hello world"}));
    assert_eq!(sim.log, ["hello world", "note: hello world"]);
}

// --- A runtime loop's variable after the loop ---

#[test]
fn a_runtime_loop_variable_holds_its_last_value_after_the_loop() {
    // Python prints `b 6 1`, `x=b`, then `3 0` and `7`.
    let mut sim = Sim::load(
        r#"names = ["a", "b"]
nums = [5, 6]
def main():
    for n in names:
        pass
    for v in nums:
        pass
    for i in range(len(nums)):
        pass
    /say {n} {v} {i}
    x = n
    /say x={x}
    y = v + i
    k = len(nums) + 2
    for d in range(k, 0, -1):
        pass
    for e in range(k - 4, k, 2):
        /say e={e}
    /say {d} {e}
    w = 7
    for w in range(k - 4):
        pass
    /say {w}
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(chat(&sim), ["b 6 1", "x=b", "e=0", "e=2", "1 2", "7"]);
    assert_eq!(sim.var("#main.y"), 7);
}

#[test]
fn a_loop_variable_of_another_type_before_the_loop_is_a_compile_error() {
    let error = compile_error(
        r#"names = ["a", "b"]
def main():
    n = 3
    for n in names:
        pass
    /say {n}
"#,
    );
    assert!(
        error.contains("`n` holds a number before this loop and text in it"),
        "{error}"
    );
    assert!(error.contains("main.cbl:4"), "{error}");
}

// --- A local copied from an imported map ---

#[test]
fn a_local_copy_of_an_imported_map_reads_its_fields() {
    let mut sim = Sim::load_project(
        &[
            ("state.cbl", "inv = {\"gold\": 3}\nnames = [\"a\", \"b\"]\n"),
            (
                "main.cbl",
                r#"import state
from state import inv, names
def main():
    local = inv
    g = local.gold
    h = local["gold"]
    people = names
    first = people[0]
    /say {g} {h} {first}
"#,
            ),
        ],
        &[],
    );
    sim.call("main");
    assert_eq!(chat(&sim), ["3 3 a"]);
}

// --- `in` beside a Minecraft condition ---

const MIXED_IN: &str = r#"names = ["Steve", "Alex"]
def main():
    x = 5
    if entity @a and x in [1, 5]:
        /say A
    if entity @a and "Steve" in names:
        /say B
    if entity @a and "Bo" not in names:
        /say C
    if entity @a and x not in [1, 5]:
        /say WRONG-D
    unless entity @a or "Bo" in names:
        /say E
    if entity @a or x in [2]:
        /say F
"#;

#[test]
fn in_beside_a_minecraft_condition_tests_membership() {
    let mut sim = Sim::load(MIXED_IN, &["Alex"]);
    sim.call("main");
    assert_eq!(chat(&sim), ["A", "B", "C", "F"]);

    // With no one online, only the `unless` passes.
    let mut sim = Sim::load(MIXED_IN, &[]);
    sim.call("main");
    assert_eq!(chat(&sim), ["E"]);
}

#[test]
fn in_beside_a_per_entity_minecraft_condition_is_a_compile_error() {
    let error = compile_error(
        "names = [\"Steve\"]\ndef main():\n    as @a if entity @s[tag=x] and \"Steve\" in names:\n        /say hi\n",
    );
    assert!(error.contains("main.cbl:3"), "{error}");
    assert!(
        error.contains("Test it with an `if` inside the block"),
        "{error}"
    );
}

#[test]
fn an_undefined_name_after_in_beside_a_minecraft_condition_is_reported() {
    let error =
        compile_error("def main():\n    if entity @a and \"Steve\" in nobody:\n        /say hi\n");
    assert!(error.contains("Undefined variable `nobody`"), "{error}");
    assert!(error.contains("main.cbl:2"), "{error}");
}

#[test]
fn a_dimension_after_a_mixed_condition_stays_an_execute_modifier() {
    let (_temp, output) = compile(
        "def main():\n    ready = 1\n    if entity @a and ready in minecraft:the_nether:\n        /say hi\n",
    )
    .unwrap();
    let main = common::mcsim::load_functions(&output)["main"].join("\n");
    assert!(
        main.contains(" in minecraft:the_nether run say hi"),
        "{main}"
    );
}

// --- Many functions and many module lists ---

/// Builds `source` without writing the pack and returns the seconds taken.
fn dry_build_seconds(source: &str) -> f64 {
    let temp = tempfile::TempDir::new().unwrap();
    let input = temp.path().join("main.cbl");
    std::fs::write(&input, source).unwrap();
    let start = std::time::Instant::now();
    cobble::commands::build::build(cobble::commands::build::BuildOptions {
        input: Some(input),
        output: Some(temp.path().join("output")),
        namespace: Some("sim".to_string()),
        pack_format: None,
        description: None,
        verbose: false,
        quiet: true,
        zip: false,
        experimental_resource_pack: false,
        validate: false,
        dry_run: true,
        commands_json: std::path::PathBuf::from("data/commands.json"),
    })
    .unwrap();
    start.elapsed().as_secs_f64()
}

#[test]
fn many_functions_beside_many_module_lists_build_in_linear_time() {
    // Each function used to copy every module table, so this took about
    // a minute in a debug build; it takes a few seconds now.
    let count = 4000;
    let defs: String = (0..count)
        .map(|i| format!("def f{i}():\n    x = {i}\n    /say {{x}}\n"))
        .collect();
    let lists: String = (0..count)
        .map(|i| format!("l{i} = [{i}, {}]\n", i + 1))
        .collect();
    for source in [format!("{defs}{lists}"), format!("{lists}{defs}")] {
        let seconds = dry_build_seconds(&source);
        assert!(seconds < 40.0, "took {seconds:.1} s");
    }
}

// --- Module-level loops in two files ---

#[test]
fn an_expanded_module_loop_in_two_files_defines_no_variable() {
    let mut sim = Sim::load_project(
        &[
            (
                "main.cbl",
                "from b import count\ntotal = 0\nfor i in range(3):\n    total += i\ndef main():\n    return total + count\n",
            ),
            ("b.cbl", "count = 0\nfor i in range(3):\n    count += 1\n"),
        ],
        &[],
    );
    assert_eq!(sim.call("main"), Some(6));
}

#[test]
fn a_module_loop_variable_read_after_the_loop_holds_its_last_value() {
    // Python: `2 2 3`.
    let mut sim = Sim::load(
        r#"def early():
    /say {i}
total = 0
for i in range(3):
    total += i
x = i
def main():
    early()
    /say {i} {x} {total}
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(chat(&sim), ["2", "2 2 3"]);
}

#[test]
fn a_real_module_loop_variable_in_two_files_is_still_an_error() {
    let Err(error) = compile_project(&[
        (
            "main.cbl",
            "import b\nfor i in range(3):\n    pass\nlast = i\ndef main():\n    pass\n",
        ),
        ("b.cbl", "for i in range(2):\n    pass\nseen = i\n"),
    ]) else {
        panic!("expected a duplicate module variable");
    };
    assert!(
        error.contains("Module variable `i` is defined in both"),
        "{error}"
    );
}

// --- Joins known at compile time in stdlib helper arguments ---

#[test]
fn a_join_known_at_compile_time_reaches_a_stdlib_helper_as_text() {
    let (_temp, output) = compile(
        r#"from stdlib import entity
const N = 4
def main():
    for i in range(2):
        entity.tag_add("@s", "t" + str(i))
        entity.tag_add("@s", "u" + str(i * 2) + "{i}")
    entity.tag_add("@s", "n" + str(N) + str(True))
"#,
    )
    .unwrap();
    let main = common::mcsim::load_functions(&output)["main"].join("\n");
    assert_eq!(
        main,
        "tag @s add t0\ntag @s add u00\ntag @s add t1\ntag @s add u21\ntag @s add n4True"
    );
}

#[test]
fn a_runtime_helper_argument_in_an_expanded_loop_is_reported_at_the_call() {
    for (source, at) in [
        (
            "from stdlib import entity\ndef main():\n    n = 1\n    for i in range(2):\n        if n > 0:\n            entity.tag_add(\"@s\", \"t\" + str(n))\n",
            "main.cbl:6:13",
        ),
        (
            "from stdlib import entity\ndef main():\n    n = 1\n    for j in range(2):\n        for i in range(2):\n            as @a:\n                pass\n            as @a:\n                entity.tag_add(\"@s\", \"t\" + str(n))\n",
            "main.cbl:9:17",
        ),
    ] {
        let error = compile_error(source);
        assert!(error.contains(at), "{at}: {error}");
        assert!(
            error.contains("must be a literal, a const, or a parameter"),
            "{error}"
        );
    }
}

// --- Per-entity condition flags ---

#[test]
fn a_per_entity_condition_flag_leaves_a_module_variable_of_the_same_name_alone() {
    let source = r#"x = 5
y = 3
def main():
    as @a unless x > 3 and y > 2:
        /say no
    as @a if x > 3 or y > 9:
        /say yes
    /say {x} {y}
"#;
    // Players named like the module variables.
    let mut sim = Sim::load(source, &["x", "y"]);
    sim.call("main");
    assert_eq!(chat(&sim), ["x: yes", "y: yes", "5 3"]);
    assert_eq!((sim.var("x"), sim.var("y")), (5, 3));
    let main = sim.function_text("main");
    assert!(!main.contains("@s sim.vars"), "{main}");
}

#[test]
fn a_long_chain_of_calls_builds_in_linear_time() {
    // Note 15: finding the recursive functions searched the calls from
    // each function, so a chain of 20,000 took half a minute in a release
    // build. Mutual recursion inside the chain is still found.
    let count = 10_000;
    let mut source = String::from("def f0():\n    /say 0\n");
    for i in 1..count {
        source.push_str(&format!("def f{i}():\n    f{}()\n", i - 1));
    }
    let seconds = dry_build_seconds(&source);
    assert!(seconds < 40.0, "took {seconds:.1} s");
    let mut sim = Sim::load(
        "def even(n):\n    if n == 0:\n        return 1\n    return odd(n - 1)\n\ndef odd(n):\n    if n == 0:\n        return 0\n    return even(n - 1)\n\ndef main():\n    return even(6) * 10 + odd(3)\n",
        &[],
    );
    assert_eq!(sim.call("main"), Some(11));
}
