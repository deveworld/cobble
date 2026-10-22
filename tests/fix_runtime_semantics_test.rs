//! Runtime-semantics regressions, checked by executing the generated pack in
//! the command simulator (`tests/common/mcsim.rs`) instead of matching
//! generated text.

mod common;

use common::mcsim::Sim;

// ---------------------------------------------------------------------------
// execute blocks: the body runs once per execution context
// ---------------------------------------------------------------------------

#[test]
fn execute_body_is_not_cut_off_by_its_own_condition() {
    let mut sim = Sim::load(
        r#"
def promote():
    as @a if score @s lvl matches 0:
        /scoreboard players set @s lvl 1
        /say promoted
"#,
        &["alex", "steve"],
    );
    sim.set_score("alex", "lvl", 0);
    sim.set_score("steve", "lvl", 5);
    sim.call("promote");
    assert_eq!(sim.score("alex", "lvl"), 1);
    assert_eq!(sim.score("steve", "lvl"), 5);
    assert_eq!(sim.log, vec!["alex: promoted"]);
}

#[test]
fn execute_body_runs_whole_body_per_entity() {
    let mut sim = Sim::load(
        r#"
def greet():
    as @a:
        /say first
        /say second
"#,
        &["alex", "steve"],
    );
    sim.call("greet");
    assert_eq!(
        sim.log,
        vec![
            "alex: first",
            "alex: second",
            "steve: first",
            "steve: second"
        ]
    );
}

#[test]
fn while_loop_inside_execute_runs_for_every_entity() {
    let mut sim = Sim::load(
        r#"
def spin():
    as @a:
        k = 0
        while k < 3:
            /say tick
            k += 1
"#,
        &["alex", "steve"],
    );
    sim.call("spin");
    assert_eq!(sim.log.len(), 6, "log: {:?}", sim.log);
    assert_eq!(sim.log.iter().filter(|l| l.starts_with("steve")).count(), 3);
}

#[test]
fn runtime_for_inside_execute_runs_for_every_entity() {
    let mut sim = Sim::load(
        r#"
def spin():
    n = 2
    as @a:
        for i in range(n):
            /say tick
"#,
        &["alex", "steve"],
    );
    sim.call("spin");
    assert_eq!(sim.log.len(), 4, "log: {:?}", sim.log);
}

#[test]
fn if_else_inside_execute_is_decided_per_entity() {
    let mut sim = Sim::load(
        r#"
def check():
    as @a:
        x = 0
        /execute store result score x temp run scoreboard players get @s lvl
        if x > 2:
            /say high
        else:
            /say low
"#,
        &["alex", "steve"],
    );
    sim.set_score("alex", "lvl", 5);
    sim.set_score("steve", "lvl", 1);
    sim.call("check");
    assert_eq!(sim.log, vec!["alex: high", "steve: low"]);
}

#[test]
fn multi_command_assignment_inside_execute_runs_once_per_entity() {
    let mut sim = Sim::load(
        r#"
def bump():
    total = 0
    as @a:
        total = total * 2 + 1
"#,
        &["alex", "steve", "kai"],
    );
    sim.call("bump");
    // 0 -> 1 -> 3 -> 7: each entity applies the whole assignment once.
    assert_eq!(sim.var("total"), 7);
}

#[test]
fn return_inside_execute_body_propagates() {
    let mut sim = Sim::load(
        r#"
def first_high():
    as @a if score @s lvl matches 3..:
        /say found
        return 1
    return 0

def probe():
    result = first_high()
"#,
        &["alex"],
    );
    sim.set_score("alex", "lvl", 4);
    sim.call("probe");
    assert_eq!(sim.var("result"), 1);
}

// ---------------------------------------------------------------------------
// condition negation follows De Morgan's laws
// ---------------------------------------------------------------------------

fn branch_taken(source_condition: &str, x: i32, y: i32) -> bool {
    let mut sim = Sim::load(
        &format!(
            r#"
def probe(a, b):
    x = a
    y = b
    hit = 0
    if {source_condition}:
        hit = 1
    return hit

def run_case():
    result = probe({x}, {y})
"#
        ),
        &[],
    );
    sim.call("run_case");
    sim.var("result") == 1
}

#[test]
fn not_of_and_chain_is_or_of_negations() {
    // not (x > 1 and y > 1) is true unless both hold.
    assert!(branch_taken("not (x > 1 and y > 1)", 5, 0));
    assert!(branch_taken("not (x > 1 and y > 1)", 0, 5));
    assert!(!branch_taken("not (x > 1 and y > 1)", 5, 5));
}

#[test]
fn not_of_and_chain_with_unless_clause() {
    // `x != 5` renders as an `unless` clause inside the AND chain.
    assert!(branch_taken("not (x != 5 and y > 1)", 5, 9));
    assert!(branch_taken("not (x != 5 and y > 1)", 4, 0));
    assert!(!branch_taken("not (x != 5 and y > 1)", 4, 9));
}

#[test]
fn not_of_or_is_and_of_negations() {
    assert!(branch_taken("not (x > 1 or y > 1)", 0, 0));
    assert!(!branch_taken("not (x > 1 or y > 1)", 5, 0));
    assert!(!branch_taken("not (x > 1 or y > 1)", 0, 5));
}

#[test]
fn double_negation_of_and_chain_is_identity() {
    assert!(branch_taken("not not (x > 1 and y > 1)", 5, 5));
    assert!(!branch_taken("not not (x > 1 and y > 1)", 5, 0));
}

#[test]
fn negated_and_combined_with_more_conditions() {
    let condition = "not (x > 1 and y > 1) and x == 5";
    assert!(branch_taken(condition, 5, 0));
    assert!(!branch_taken(condition, 5, 5));
    assert!(!branch_taken(condition, 4, 0));
}

#[test]
fn negated_and_in_while_condition() {
    let mut sim = Sim::load(
        r#"
def count():
    x = 0
    y = 10
    while not (x > 2 and y > 2):
        x += 1
        y += 1
    result = x
"#,
        &[],
    );
    sim.call("count");
    assert_eq!(sim.var("result"), 3);
}

#[test]
fn negated_and_in_flag_assignment() {
    let mut sim = Sim::load(
        r#"
def compute():
    x = 5
    y = 0
    flag = not (x > 1 and y > 1)
"#,
        &[],
    );
    sim.call("compute");
    assert_eq!(sim.var("flag"), 1);
}

#[test]
fn negated_and_in_execute_condition() {
    let mut sim = Sim::load(
        r#"
def check():
    x = 5
    y = 0
    as @a if not (x > 1 and y > 1):
        /say passes
    as @a unless x > 1 and y > 1:
        /say unless passes
"#,
        &["alex"],
    );
    sim.call("check");
    assert_eq!(sim.log, vec!["alex: passes", "alex: unless passes"]);
}

#[test]
fn unless_or_with_and_alternative_in_execute_condition() {
    // unless ((x > 1 and y > 1) or y == 7) passes when neither alternative holds.
    let source = |x: i32, y: i32| {
        format!(
            r#"
def check():
    x = {x}
    y = {y}
    as @a unless (x > 1 and y > 1) or y == 7:
        /say passes
"#
        )
    };
    let mut sim = Sim::load(&source(5, 0), &["alex"]);
    sim.call("check");
    assert_eq!(sim.log, vec!["alex: passes"]);

    let mut sim = Sim::load(&source(0, 5), &["alex"]);
    sim.call("check");
    assert_eq!(sim.log, vec!["alex: passes"]);

    let mut sim = Sim::load(&source(5, 5), &["alex"]);
    sim.call("check");
    assert!(sim.log.is_empty(), "log: {:?}", sim.log);
}

// ---------------------------------------------------------------------------
// self-referential flag assignment
// ---------------------------------------------------------------------------

#[test]
fn flag_assignment_reading_its_own_target() {
    let mut sim = Sim::load(
        r#"
def toggle():
    flag = True
    flag = not flag
    a = flag
    flag = not flag
    b = flag
    done = True
    c = 0
    done = done and c > 0
"#,
        &[],
    );
    sim.call("toggle");
    assert_eq!(sim.var("a"), 0);
    assert_eq!(sim.var("b"), 1);
    assert_eq!(sim.var("done"), 0);
}

// ---------------------------------------------------------------------------
// compile-time constants and literal comparisons in conditions
// ---------------------------------------------------------------------------

#[test]
fn constant_conditions_fold_at_compile_time() {
    let mut sim = Sim::load(
        r#"
const DEBUG = 1
const QUIET = 0

def check():
    if DEBUG:
        /say debug
    if QUIET:
        /say quiet
    if not QUIET:
        /say not quiet
    if DEBUG > 0:
        /say debug positive
    if 1 == 1:
        /say literal true
    if 2 < 1:
        /say literal false
"#,
        &[],
    );
    sim.call("check");
    assert_eq!(
        sim.log,
        vec![
            "Server: debug",
            "Server: not quiet",
            "Server: debug positive",
            "Server: literal true"
        ]
    );
}

#[test]
fn match_on_constant_uses_its_value() {
    let mut sim = Sim::load(
        r#"
const MODE = 2

def check():
    match MODE:
        case 1:
            /say one
        case 2:
            /say two
        case _:
            /say other
"#,
        &[],
    );
    sim.call("check");
    assert_eq!(sim.log, vec!["Server: two"]);
}

// ---------------------------------------------------------------------------
// self-referential assignments inside functions
// ---------------------------------------------------------------------------

#[test]
fn self_referential_right_operand_inside_function() {
    let mut sim = Sim::load(
        r#"
const C = 4

def compute():
    x = 3
    x = 10 - x
    a = x
    y = 20
    x = y - x
    b = x
    x = C + x
    c = x
    x = 12 / x
    d = x
    x = 7
    x = 50 % x
    e = x
    x = 5
    x = x + x
    f = x
"#,
        &[],
    );
    sim.call("compute");
    assert_eq!(sim.var("a"), 7, "x = 10 - x with x = 3");
    assert_eq!(sim.var("b"), 13, "x = y - x with x = 7, y = 20");
    assert_eq!(sim.var("c"), 17, "x = C + x with x = 13");
    assert_eq!(sim.var("d"), 0, "x = 12 / x with x = 17");
    assert_eq!(sim.var("e"), 1, "x = 50 % x with x = 7");
    assert_eq!(sim.var("f"), 10, "x = x + x with x = 5");
}

// ---------------------------------------------------------------------------
// constant folding matches runtime scoreboard arithmetic
// ---------------------------------------------------------------------------

/// Compile `expr` once with literal operands (folded at compile time) and once
/// through variables (computed by scoreboard operations), and return both.
fn folded_and_runtime(expr_with_literals: &str, expr_with_vars: &str, vars: &str) -> (i32, i32) {
    let mut sim = Sim::load(
        &format!(
            r#"
folded_module = {expr_with_literals}

def compute():
    folded = {expr_with_literals}
{vars}
    runtime = {expr_with_vars}
"#
        ),
        &[],
    );
    sim.call("compute");
    assert_eq!(
        sim.var("folded"),
        sim.var("folded_module"),
        "module-level and function folding disagree for {expr_with_literals}"
    );
    (sim.var("folded"), sim.var("runtime"))
}

#[test]
fn constant_division_uses_floor_division() {
    let vars = "    a = 0 - 7\n    b = 2";
    assert_eq!(folded_and_runtime("-7 / 2", "a / b", vars), (-4, -4));
    let vars = "    a = 7\n    b = 0 - 2";
    assert_eq!(folded_and_runtime("7 / -2", "a / b", vars), (-4, -4));
    let vars = "    a = 7\n    b = 2";
    assert_eq!(folded_and_runtime("7 / 2", "a / b", vars), (3, 3));
}

#[test]
fn constant_modulo_uses_floor_modulus() {
    let vars = "    a = 0 - 7\n    b = 3";
    assert_eq!(folded_and_runtime("-7 % 3", "a % b", vars), (2, 2));
    let vars = "    a = 7\n    b = 0 - 3";
    assert_eq!(folded_and_runtime("7 % -3", "a % b", vars), (-2, -2));
}

#[test]
fn constant_division_truncates_each_step() {
    let vars = "    a = 1\n    b = 3";
    assert_eq!(folded_and_runtime("1 / 3 * 3", "a / b * b", vars), (0, 0));
}

#[test]
fn const_declarations_use_floor_semantics() {
    let mut sim = Sim::load(
        r#"
const N = -7
const D = 2
const M = N % 3

def compute():
    q = N / D
    r = M
    total = 0
    for i in range(N / D, 0):
        total += 1
"#,
        &[],
    );
    sim.call("compute");
    assert_eq!(sim.var("q"), -4);
    assert_eq!(sim.var("r"), 2);
    assert_eq!(sim.var("total"), 4, "range(-4, 0) unrolls four times");
}

#[test]
fn fractional_constants_keep_real_arithmetic() {
    let mut sim = Sim::load(
        r#"
const PI = 3.14159
const RADIUS = 10

def compute():
    area = PI * RADIUS * RADIUS
    half = 7 % 0.5
"#,
        &[],
    );
    sim.call("compute");
    assert_eq!(sim.var("area"), 314);
    assert_eq!(sim.var("half"), 0);
}

// ---------------------------------------------------------------------------
// `/` after an operand divides; it only starts a raw command at statement start
// ---------------------------------------------------------------------------

#[test]
fn unspaced_slash_after_operand_is_division() {
    let mut sim = Sim::load(
        r#"
def compute():
    a = 12
    b = 3
    x = a/b
    y = (a+b)/b
    z = a/b/2
    /say done
"#,
        &[],
    );
    sim.call("compute");
    assert_eq!(sim.var("x"), 4);
    assert_eq!(sim.var("y"), 5);
    assert_eq!(sim.var("z"), 2);
    assert_eq!(sim.log, vec!["Server: done"]);
}

#[test]
fn raw_execute_conditions_keep_source_glued_tokens() {
    let (_temp, output_dir) = common::mcsim::compile(
        r#"
def f():
    as @a positioned ^1 ^ ^0.5:
        /say local
    as @a if block ~ ~-1 ~ minecraft:stone[snowy=false]:
        /say block
    as @a if function sim:util/check:
        /say fn
    as @a if data storage sim:global vars.items[0]:
        /say path
    x = 2
    as @a if x>1:
        /say python

def util():
    /say u
"#,
    )
    .unwrap();
    let f = common::mcsim::load_functions(&output_dir)
        .remove("f")
        .unwrap()
        .join("\n");
    for expected in [
        "execute as @a positioned ^1 ^ ^0.5 run say local",
        "execute as @a if block ~ ~-1 ~ minecraft:stone[snowy=false] run say block",
        "execute as @a if function sim:util/check run say fn",
        "execute as @a if data storage sim:global vars.items[0] run say path",
        "execute as @a if score x sim.vars matches 2.. run say python",
    ] {
        assert!(f.contains(expected), "missing `{expected}` in:\n{f}");
    }
}

// ---------------------------------------------------------------------------
// statement calls pass argument values
// ---------------------------------------------------------------------------

#[test]
fn statement_call_passes_storage_string_value() {
    let mut sim = Sim::load(
        r#"
def shout(m):
    /say {m}

def main():
    msg = "hello"
    shout(msg)
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(sim.log, vec!["Server: hello"]);
}

#[test]
fn statement_calls_with_differently_typed_expressions() {
    let mut sim = Sim::load(
        r#"
def show(v):
    /say {v}

def main():
    a = 1
    b = 2
    show(a + 1)
    show(a > b)
    show(b > a)
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(sim.log, vec!["Server: 2", "Server: 0", "Server: 1"]);
}

#[test]
fn single_macro_command_under_if_keeps_macro_marker_first() {
    let mut sim = Sim::load(
        r#"
def notify(p, n):
    if n > 0:
        /say {p} has {n}
    else:
        /say {p} has none

def main():
    notify("alex", 3)
    notify("kai", 0)
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(sim.log, vec!["Server: alex has 3", "Server: kai has none"]);
    assert!(
        !sim.function_text("notify").contains("run $"),
        "{}",
        sim.function_text("notify")
    );
}

#[test]
fn calling_parameterless_function_with_arguments_is_an_error() {
    let error = common::mcsim::compile(
        r#"
def ping():
    /say ping

def main():
    ping(1)
"#,
    )
    .expect_err("extra arguments must be rejected");
    assert!(error.contains("ping"), "{error}");
}

// ---------------------------------------------------------------------------
// execute helper edge cases (review follow-ups)
// ---------------------------------------------------------------------------

#[test]
fn raw_return_in_execute_body_still_returns_from_the_function() {
    let mut sim = Sim::load(
        r#"
def f():
    as @a:
        /say hi
        /return 1
    /say after
"#,
        &["alex", "steve"],
    );
    sim.call("f");
    assert!(
        !sim.log.iter().any(|line| line.ends_with("after")),
        "raw /return must leave f: {:?}",
        sim.log
    );
    assert!(sim.log.contains(&"alex: hi".to_string()), "{:?}", sim.log);
}

#[test]
fn macro_params_inside_execute_helper() {
    let mut sim = Sim::load(
        r#"
def greet(name, target):
    as {target}:
        /say hello {name}
        /say bye {name}

def main():
    greet("kai", "@a")
"#,
        &["alex", "steve"],
    );
    sim.call("main");
    assert_eq!(
        sim.log,
        vec![
            "alex: hello kai",
            "alex: bye kai",
            "steve: hello kai",
            "steve: bye kai"
        ]
    );
}

#[test]
fn per_command_execute_body_never_marks_plain_lines_as_macros() {
    // The raw `/return` keeps the per-command form; the plain `say` line must
    // not inherit the `$` of the macro line before it (Minecraft rejects
    // macro lines without variables; the simulator panics on them).
    let mut sim = Sim::load(
        r#"
def f(p):
    as @a:
        /say {p}
        /say plain
        /return 1

def main():
    f("x")
"#,
        &["alex"],
    );
    sim.call("main");
    assert_eq!(sim.log, vec!["alex: x", "alex: plain"]);
}

#[test]
fn store_blocks_keep_one_store_per_command() {
    let mut sim = Sim::load(
        r#"
def f():
    as @a store result score @s hp:
        /scoreboard players get @s base
        /scoreboard players get @s bonus
"#,
        &["alex", "steve"],
    );
    sim.set_score("alex", "base", 10);
    sim.set_score("alex", "bonus", 3);
    sim.set_score("steve", "base", 20);
    sim.set_score("steve", "bonus", 7);
    sim.objectives.insert("hp".into());
    sim.call("f");
    assert!(
        !sim.functions
            .keys()
            .any(|name| name.starts_with("execute_temp_")),
        "store blocks must not route through a helper"
    );
    assert_eq!(sim.score("alex", "hp"), 3);
    assert_eq!(sim.score("steve", "hp"), 7);
}

#[test]
fn asat_block_runs_whole_body_per_entity() {
    let mut sim = Sim::load(
        r#"
def f():
    asat @a:
        /say one
        /say two
"#,
        &["alex", "steve"],
    );
    sim.call("f");
    assert_eq!(
        sim.log,
        vec!["alex: one", "alex: two", "steve: one", "steve: two"]
    );
}

#[test]
fn structured_execute_condition_is_decided_per_entity() {
    // `not (A and B) and C` lowers to AND around an OR flag; under `as @a`
    // the flag must be a per-entity score, not one fake player shared by
    // every entity.
    let mut sim = Sim::load(
        r#"
def f():
    x = 1
    as @a if not (@s > 1 and @s < 5) and x > 0:
        /say outside
"#,
        &["alex", "steve"],
    );
    sim.set_score("alex", "sim.vars", 3);
    sim.set_score("steve", "sim.vars", 9);
    sim.call("f");
    assert_eq!(sim.log, vec!["steve: outside"]);
}

#[test]
fn fractional_const_division_is_kept_for_commands() {
    let mut sim = Sim::load(
        r#"
const HALF = 1 / 2

def f():
    /say {HALF}
    whole = HALF
"#,
        &[],
    );
    sim.call("f");
    assert_eq!(sim.log, vec!["Server: 0.5"]);
    assert_eq!(sim.var("whole"), 0);
}
