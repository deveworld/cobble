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
//! - nested `math.*` calls, the smallest 32-bit constant, lists in
//!   arithmetic, and nested map fields compute what they say
//! - a comparison reads its left side before a call on its right runs
//! - a call to a function without parameters keeps the caller's arguments
//! - an empty `range` or array runs no passes
//! - `s[i]` is a one-character string, counted from the end when negative
//! - a function takes list and map literals and string characters as
//!   arguments, and a list parameter can be indexed, iterated, and assigned
//!
//! `tests/differential_test.rs` checks the same rules on random programs.

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
    assert_eq!(sim.var("#small.r"), 10);
    assert!(sim.log.is_empty(), "{:?}", sim.log);

    sim.call("large");
    assert_eq!(sim.var("#large.r"), 140);
    assert_eq!(sim.log, ["Server: big 70"]);
    assert_eq!(sim.var("amount"), 100);
}

#[test]
fn assigning_a_parameter_updates_later_reads() {
    let mut sim = Sim::load(
        r#"steps = 0

def countdown(n):
    global steps
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
    assert_eq!(sim.var("#main.c"), 0);
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
    global result
    result = x * x
    return result

def plain():
    global result
    result = 1 + square(3)

def compound():
    global result
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
    assert_eq!(sim.var("#main.a"), 18);
    assert_eq!(sim.var("#main.b"), 18);
}

#[test]
fn while_condition_runs_once_per_iteration() {
    let mut sim = Sim::load(
        r#"calls = 0
ticks = 0

def more():
    global calls
    calls = calls + 1
    if calls < 3:
        return 1
    return 0

def main():
    global ticks
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
    assert_eq!(sim.var("#main.found"), 1);
    assert_eq!(sim.log, ["Alex: picked"]);
}

#[test]
fn and_or_skip_the_right_side_when_the_left_decides() {
    let mut sim = Sim::load(
        r#"hits = 0

def no():
    global hits
    hits = hits + 1
    return 0

def yes():
    global hits
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
    global hits
    hits = hits + 1
    return 0

def yes():
    global hits
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
fn numbers_and_negations_work_as_conditions() {
    let mut sim = Sim::load(
        r#"def main():
    x = 5
    if 3:
        /say three
    if 0:
        /say zero
    if -x:
        /say minus x
    if x and -2:
        /say both
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(
        sim.log,
        ["Server: three", "Server: minus x", "Server: both"]
    );
}

#[test]
fn comparison_reads_its_left_side_before_the_call_on_its_right() {
    let mut sim = Sim::load(
        r#"level = 0
hits = 0

def raise_level():
    global level
    level = 10
    return 5

def main():
    global hits
    if level > raise_level():
        hits += 1
    if level > raise_level():
        hits += 10
"#,
        &[],
    );
    sim.call("main");
    // The first test compares 0 > 5, the second 10 > 5.
    assert_eq!(sim.var("hits"), 10);
}

#[test]
fn call_without_parameters_keeps_the_callers_arguments() {
    let mut sim = Sim::load(
        r#"result = 0

def inner(p):
    return p + 1

def middle():
    inner(5)

def middle_value():
    return inner(6)

def outer(n):
    global result
    middle()
    result = n * 10
    result += middle_value()
    result += n

def main():
    outer(7)
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(sim.var("result"), 7 * 10 + 7 + 7);
}

#[test]
fn empty_ranges_and_arrays_run_no_passes() {
    let mut sim = Sim::load(
        r#"total = 0

def main():
    global total
    for i in range(4):
        for j in range(i, 2):
            total += 1
    for k in range(3, 1):
        total += 100
    for k in range(-2):
        total += 100
    for k in range(0, 3, -1):
        total += 100
    for k in []:
        total += 100
    items = []
    for k in items:
        total += 100
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(sim.var("total"), 3);
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
    assert_eq!(sim.var("#main.computed"), 14100);
    assert_eq!(sim.var("#main.folded"), sim.var("#main.computed"));
    assert_eq!(sim.var("#main.computed_pow"), i32::MIN);
    assert_eq!(sim.var("#main.folded_pow"), sim.var("#main.computed_pow"));
    assert_eq!(sim.var("#main.folded_div"), -4);
    assert_eq!(sim.var("#main.folded_mod"), 2);
}

#[test]
fn nested_math_calls_keep_their_arguments() {
    let mut sim = Sim::load(
        r#"from stdlib import math

def main():
    a = -8
    root = math.sqrt(1 + math.abs(a))
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(sim.var("#main.root"), 3);
}

#[test]
fn smallest_int_constant_wraps_like_the_scoreboard() {
    let mut sim = Sim::load(
        r#"const LOW = -2147483648

def main():
    a = 5
    up = (a + 1) + LOW
    down = (a + 1) - LOW
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(sim.var("#main.up"), 6i32.wrapping_add(i32::MIN));
    assert_eq!(sim.var("#main.down"), 6i32.wrapping_sub(i32::MIN));
}

#[test]
fn list_on_the_right_of_an_operator_reads_its_size() {
    let mut sim = Sim::load(
        r#"items = [4, 5, 6]

def main():
    a = 1
    left = items + (a + 1)
    right = (a + 1) + items
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(sim.var("#main.left"), 5);
    assert_eq!(sim.var("#main.right"), 5);
}

#[test]
fn nested_map_fields_read_through_the_chain() {
    let mut sim = Sim::load(
        r#"cfg = {"inner": {"c": 5}}
inner = 3

def main():
    d = cfg.inner.c
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(sim.var("#main.d"), 5);
}

#[test]
fn recursive_functions_keep_each_call_separate() {
    let mut sim = Sim::load(
        r#"def fact(n):
    if n <= 1:
        return 1
    return n * fact(n - 1)

def fib(n):
    if n < 2:
        return n
    a = fib(n - 1)
    b = fib(n - 2)
    return a + b

def main():
    f = fact(6)
    g = fib(10)
    h = 1 + fib(7) * 2
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(sim.var("#main.f"), 720);
    assert_eq!(sim.var("#main.g"), 55);
    assert_eq!(sim.var("#main.h"), 27);
}

#[test]
fn mutual_recursion_and_recursive_calls_with_arguments() {
    let mut sim = Sim::load(
        r#"def is_even(n):
    if n == 0:
        return 1
    return is_odd(n - 1)

def is_odd(n):
    if n == 0:
        return 0
    return is_even(n - 1)

def fact(n):
    if n <= 1:
        return 1
    return n * fact(n - 1)

def wrapper(k):
    return fact(k) + k

def main():
    e = is_even(10)
    o = is_even(7)
    w = wrapper(4)
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(sim.var("#main.e"), 1);
    assert_eq!(sim.var("#main.o"), 0);
    assert_eq!(sim.var("#main.w"), 28);
}

#[test]
fn recursive_calls_keep_storage_locals_and_parameters() {
    let mut sim = Sim::load(
        r#"def label(n):
    name = "leaf"
    if n > 0:
        name = "node"
        label(n - 1)
    /say {name} {n}

def main():
    label(2)
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(
        sim.log,
        ["Server: leaf 0", "Server: node 1", "Server: node 2"]
    );
}

#[test]
fn imported_functions_keep_their_locals_in_a_project_build() {
    // The project build reaches `util.cbl` twice: through the import and as
    // a file of the source folder. It compiles the file once.
    let mut sim = Sim::load_project(
        &[
            (
                "main.cbl",
                "import util\n\nresult = 0\n\ndef main():\n    global result\n    result = util.count_to(3)\n",
            ),
            (
                "util.cbl",
                "def count_to(limit):\n    slot = 0\n    while slot < limit:\n        slot += 1\n    return slot\n",
            ),
        ],
        &[],
    );
    sim.call("main");
    assert_eq!(sim.var("result"), 3);
    assert!(!sim.function_text("count_to").contains(" slot "));
    let loop_helpers = sim
        .functions
        .keys()
        .filter(|name| name.contains("while"))
        .count();
    assert_eq!(loop_helpers, 2, "{:?}", sim.functions.keys());
}

#[test]
fn reading_a_local_before_assigning_it_is_an_error() {
    for (source, location) in [
        (
            "count = 0\n\ndef bump():\n    count = count + 1\n",
            "main.cbl:4:5",
        ),
        (
            "count = 0\n\ndef bump():\n    while count < 3:\n        count += 1\n",
            "main.cbl:4:5",
        ),
        (
            "count = 0\n\ndef bump():\n    /say hi\n    count += 5\n",
            "main.cbl:5:5",
        ),
    ] {
        let error = compile(source).expect_err("an unbound local must not compile");
        assert!(
            error.contains("`count` is read before it is assigned in `bump`"),
            "{error}"
        );
        assert!(error.contains("global count"), "{error}");
        assert!(error.contains(location), "{error}");
    }
    // A later iteration may read what an earlier one assigned.
    compile("def later():\n    for i in range(3):\n        if i > 0:\n            y = x + 1\n        x = i\n")
        .expect("a read guarded to later iterations compiles");
}

#[test]
fn raw_return_in_a_recursive_function_is_an_error() {
    let error = compile(
        r#"def down(n):
    if n > 0:
        down(n - 1)
    /return 1
"#,
    )
    .expect_err("a raw return would skip the frame restore");
    assert!(error.contains("recursive function `down`"), "{error}");
    assert!(error.contains("main.cbl:4:5"), "{error}");
}

fn storage(sim: &Sim, path: &[&str]) -> serde_json::Value {
    path.iter()
        .fold(&sim.storage, |value, key| &value[*key])
        .clone()
}

#[test]
fn string_indexes_give_characters() {
    let mut sim = Sim::load(
        r#"word = "abc"
first = word[0]

def main():
    global word
    s = "hello"
    i = 1
    j = -2
    a = s[0]
    b = s[i]
    c = s[-1]
    d = s[j]
    e = a[0]
    word = word[2]
    /say v {a}{b}{c}{d}{e} {word} {first}
    return len(c)
"#,
        &[],
    );
    assert_eq!(sim.call("main"), Some(1));
    assert_eq!(sim.log, vec!["Server: v heolh c a"]);
}

#[test]
fn copying_a_string_keeps_it_a_string() {
    let mut sim = Sim::load(
        r#"name = "steve"
copy = name

def rename(new_name):
    global name
    name = name
    name = new_name

def main():
    rename("alex")
    /say v {name} {copy}
    return len(name)
"#,
        &[],
    );
    assert_eq!(sim.call("main"), Some(4));
    assert_eq!(sim.log, vec!["Server: v alex steve"]);
}

#[test]
fn functions_take_lists_and_string_characters() {
    let mut sim = Sim::load(
        r#"total = 0

def sum_two(values):
    global total
    first = values[0]
    for value in values:
        total += value
    values = [first, 100]
    return values[1] + first

def show(letter):
    /say v {letter}

def main():
    word = "xyz"
    show(word[1])
    pairs = [3, 4]
    return sum_two([1, 2]) + sum_two(pairs) * 1000
"#,
        &[],
    );
    assert_eq!(sim.call("main"), Some(103 * 1000 + 101));
    assert_eq!(sim.var("total"), 10);
    assert_eq!(sim.log, vec!["Server: v y"]);
    assert_eq!(
        storage(&sim, &["locals", "main", "pairs"]),
        serde_json::json!([3, 4])
    );
}

#[test]
fn a_comma_inside_an_index_is_not_tuple_unpacking() {
    let mut sim = Sim::load(
        r#"items = [1, 2, 3]

def pick(a, b):
    return a + b

def main():
    items[pick(0, 1)] = 9
    items[pick(1, 1)] += 5
    return items[1] * 100 + items[2]
"#,
        &[],
    );
    assert_eq!(sim.call("main"), Some(908));
}

#[test]
fn list_loop_targets_copy_into_numbers() {
    let mut sim = Sim::load(
        r#"last = 0
values = [4, 5, 6]

def main():
    global last
    for value in values:
        last = value
    start = 1
    total = 0
    for value in [start, start + 1]:
        copy = value
        copy -= 1
        total += copy
    return total
"#,
        &[],
    );
    assert_eq!(sim.call("main"), Some(1));
    assert_eq!(sim.var("last"), 6);
}

#[test]
fn items_and_fields_keep_their_strings() {
    let mut sim = Sim::load(
        r#"names = ["Steve", "Alex"]
config = {"title": "Boss", "hp": 5}
first = names[0]
title = config.title

def show(text):
    /say v {text}
    return len(text)

def main():
    name = names[1]
    field = config["title"]
    letters = [name[0], first[-1], "z"]
    letters[2] = field[1]
    /say v {name} {field} {first} {title}
    return show(names[1]) * 10 + show(config.title) + config.hp
"#,
        &[],
    );
    assert_eq!(sim.call("main"), Some(4 * 10 + 4 + 5));
    assert_eq!(
        sim.log,
        vec![
            "Server: v Alex Boss Steve Boss",
            "Server: v Alex",
            "Server: v Boss",
        ]
    );
    assert_eq!(
        storage(&sim, &["locals", "main", "letters"]),
        serde_json::json!(["A", "e", "o"])
    );
}

#[test]
fn parameters_keep_strings_and_index_by_their_value() {
    let mut sim = Sim::load(
        r#"def first_and_last(value):
    copy = value
    head = copy[0]
    tail = value[-1]
    /say v {head} {tail}
    return len(copy)

def main():
    words = 0
    for word in ["ab", "xyz"]:
        words += first_and_last(word)
        letter = word[len(word) - 1]
        /say v {letter}
    return words * 100 + first_and_last([7, 8, 9])
"#,
        &[],
    );
    assert_eq!(sim.call("main"), Some(5 * 100 + 3));
    assert_eq!(
        sim.log,
        vec![
            "Server: v a b",
            "Server: v b",
            "Server: v x z",
            "Server: v z",
            "Server: v 7 9",
        ]
    );
}

#[test]
fn a_call_reads_its_arguments_from_left_to_right() {
    let mut sim = Sim::load(
        r#"level = 0

def set_level(value, other):
    global level
    level = value
    return 0

def main():
    global level
    level = 1
    set_level(level, set_level(5, 0))
    return level
"#,
        &[],
    );
    // `level` is read before the inner call sets it to 5.
    assert_eq!(sim.call("main"), Some(1));
}

#[test]
fn a_raw_command_that_sets_a_copy_of_a_parameter_sets_its_value() {
    let mut sim = Sim::load(
        r#"def bump(start):
    value = start
    total = 0
    for i in range(3):
        /say v {value}
        /scoreboard players add value temp 10
    return value

def main():
    return bump(1)
"#,
        &[],
    );
    assert_eq!(sim.call("main"), Some(31));
    assert_eq!(sim.log, vec!["Server: v 1", "Server: v 11", "Server: v 21"]);
}

#[test]
fn an_item_written_with_another_type_reads_the_written_value() {
    // `rename` runs first and is compiled after `main`.
    let mut sim = Sim::load(
        r#"names = ["Steve", "Alex"]
config = {"title": "Boss", "hp": 5}
items = [1, 2]

def main():
    rename()
    y = names[1]
    t = config.title
    x = items[0]
    hp = config.hp
    /say v {y} {t} {x} {hp}
    return (y + 1) * 100 + (t + 1) * 10 + hp

def rename():
    names[1] = 7
    config.title = 2
    items[0] = "abc"
"#,
        &[],
    );
    assert_eq!(sim.call("main"), Some(8 * 100 + 3 * 10 + 5));
    assert_eq!(sim.log, vec!["Server: v 7 2 abc 5"]);
}

#[test]
fn an_item_written_in_another_file_reads_the_written_value() {
    let mut sim = Sim::load_project(
        &[
            (
                "a.cbl",
                r#"names = ["Steve", "Alex"]

def read_a():
    y = names[1]
    /say v {y}
    return y + 1
"#,
            ),
            (
                "b.cbl",
                r#"def write_b():
    names[1] = 7
"#,
            ),
        ],
        &[],
    );
    sim.call("write_b");
    assert_eq!(sim.call("read_a"), Some(8));
    assert_eq!(sim.log, vec!["Server: v 7"]);
}
