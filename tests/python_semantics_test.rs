//! Generated packs follow Python's evaluation rules where a program can tell
//! the difference:
//! - a parameter shadows a module variable with the same name
//! - assigning to a parameter updates every later read of it
//! - an assignment runs after the whole right side
//! - a `while` condition is evaluated once per iteration
//! - `return` inside `as @a:` stops the remaining entities
//! - `and`, `or`, and `elif` skip conditions that can't change the result
//! - any value other than 0 is true, negative numbers included
//! - constant folding uses the same 32-bit arithmetic as the scoreboard

mod common;

use common::mcsim::{compile, Sim};

#[test]
fn parameter_shadows_module_variable_with_the_same_name() {
    let mut sim = Sim::load(
        r#"amount = 100

def reward(amount):
    if amount > 50:
        /say big {amount}
    return amount * 2

def small():
    r = reward(5)

def large():
    r = reward(70)
"#,
        &[],
    );
    sim.call("small");
    assert_eq!(sim.var("r"), 10);
    assert!(sim.log.is_empty(), "{:?}", sim.log);

    sim.call("large");
    assert_eq!(sim.var("r"), 140);
    assert_eq!(sim.log, ["Server: big 70"]);
    assert_eq!(sim.var("amount"), 100);
}

#[test]
fn assigning_a_parameter_updates_later_reads() {
    let mut sim = Sim::load(
        r#"steps = 0

def countdown(n):
    while n > 0:
        n = n - 1
        steps += 1
    return n

def main():
    c = countdown(3)
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(sim.var("c"), 0);
    assert_eq!(sim.var("steps"), 3);
}

#[test]
fn command_text_reads_the_reassigned_parameter() {
    let mut sim = Sim::load(
        r#"def give(count, name):
    /say before {count}
    count = count * 2
    name = "Steve"
    /say after {count} for {name}

def main():
    give(5, "Alex")
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(sim.log, ["Server: before 5", "Server: after 10 for Steve"]);
}

#[test]
fn global_declaration_of_a_parameter_is_an_error() {
    let error = compile(
        r#"score = 0

def bump(score):
    global score
    score = score + 1
"#,
    )
    .expect_err("a parameter declared global must not compile");
    assert!(error.contains("is a parameter"), "{error}");
}

#[test]
fn assignment_runs_after_the_calls_on_its_right_side() {
    let mut sim = Sim::load(
        r#"result = 0

def square(x):
    result = x * x
    return result

def plain():
    result = 1 + square(3)

def compound():
    result = 1
    result += square(3)
"#,
        &[],
    );
    sim.call("plain");
    assert_eq!(sim.var("result"), 10);
    sim.call("compound");
    assert_eq!(sim.var("result"), 10);
}

#[test]
fn parameter_operands_survive_a_call_in_the_same_expression() {
    let mut sim = Sim::load(
        r#"def sq(k):
    return k * k

def left(n):
    return n * sq(3)

def right(n):
    return sq(3) * n

def main():
    a = left(2)
    b = right(2)
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(sim.var("a"), 18);
    assert_eq!(sim.var("b"), 18);
}

#[test]
fn while_condition_runs_once_per_iteration() {
    let mut sim = Sim::load(
        r#"calls = 0
ticks = 0

def more():
    calls = calls + 1
    if calls < 3:
        return 1
    return 0

def main():
    while more():
        ticks += 1
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(sim.var("ticks"), 2);
    assert_eq!(sim.var("calls"), 3);
}

#[test]
fn return_inside_as_block_stops_the_other_entities() {
    let mut sim = Sim::load(
        r#"def first():
    as @a:
        /say picked
        return 1
    return 0

def main():
    found = first()
"#,
        &["Alex", "Steve"],
    );
    sim.call("main");
    assert_eq!(sim.var("found"), 1);
    assert_eq!(sim.log, ["Alex: picked"]);
}

#[test]
fn and_or_skip_the_right_side_when_the_left_decides() {
    let mut sim = Sim::load(
        r#"hits = 0

def no():
    hits = hits + 1
    return 0

def yes():
    hits = hits + 10
    return 1

def main():
    if no() and yes():
        /say both
    if yes() or no():
        /say either
    if yes() and no():
        /say never
    if no() or yes():
        /say second
"#,
        &[],
    );
    sim.call("main");
    // no, yes, yes+no, no+yes
    assert_eq!(sim.var("hits"), 1 + 10 + 11 + 11);
    assert_eq!(sim.log, ["Server: either", "Server: second"]);
}

#[test]
fn elif_condition_is_skipped_once_a_branch_ran() {
    let mut sim = Sim::load(
        r#"hits = 0

def no():
    hits = hits + 1
    return 0

def yes():
    hits = hits + 10
    return 1

def main():
    if yes():
        /say first
    elif no():
        /say second
    elif yes():
        /say third
    if no():
        /say a
    elif yes():
        /say b
    else:
        /say c
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(sim.var("hits"), 10 + 1 + 10);
    assert_eq!(sim.log, ["Server: first", "Server: b"]);
}

#[test]
fn negative_numbers_are_true() {
    let mut sim = Sim::load(
        r#"def main():
    x = -5
    if x:
        /say x is true
    if not x:
        /say x is false
    zero = 0
    if zero:
        /say zero is true
    if not zero:
        /say zero is false
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(sim.log, ["Server: x is true", "Server: zero is false"]);
}

#[test]
fn constant_folding_matches_runtime_overflow() {
    let mut sim = Sim::load(
        r#"def main():
    folded = 100000 * 100000 / 100000
    v = 100000
    computed = v * v / v
    folded_pow = 2 ^ 31
    two = 2
    computed_pow = two ^ 31
    folded_div = -7 / 2
    folded_mod = -7 % 3
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(sim.var("computed"), 14100);
    assert_eq!(sim.var("folded"), sim.var("computed"));
    assert_eq!(sim.var("computed_pow"), i32::MIN);
    assert_eq!(sim.var("folded_pow"), sim.var("computed_pow"));
    assert_eq!(sim.var("folded_div"), -4);
    assert_eq!(sim.var("folded_mod"), 2);
}
