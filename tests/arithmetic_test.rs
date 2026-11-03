//! Arithmetic contracts checked by executing generated commands. Scratch
//! register names and the choice of lowering helpers are implementation details.
mod common;

use common::mcsim::Sim;

#[test]
fn assignments_read_the_old_value_before_writing_the_result() {
    let mut sim = Sim::load(
        "x = 5\nx = 2 - x\ny = 5\ny = 10 / y\nz = 5\nz = 3 * z + z\n\
         def compute():\n    w = 5\n    w = w - 2\n",
        &[],
    );
    sim.call("compute");
    for (name, expected) in [("x", -3), ("z", 20), ("#compute.w", 3)] {
        assert_eq!(sim.var(name), expected, "{name}");
    }
    assert_eq!(sim.storage["vars"]["y"], 2.0);
}

#[test]
fn nested_operators_and_math_calls_preserve_each_operand() {
    let mut sim = Sim::load(
        r#"from stdlib import math
def compute():
    a = 6
    b = 9
    c = 2
    d = 3
    product = a * (b * (c * d))
    difference = a - (b - (c - d))
    root = math.sqrt(math.sqrt(256))
    maximum = math.max(2, math.abs(-9))
    minimum = math.min(math.abs(-4), math.max(1, 6))
    mixed = math.max(a, b) - (c - (d - 1))
    argument = math.sqrt(1 + math.abs(-8))
"#,
        &[],
    );
    sim.call("compute");
    for (name, expected) in [
        ("product", 324),
        ("difference", -4),
        ("maximum", 9),
        ("minimum", 4),
        ("mixed", 9),
    ] {
        assert_eq!(sim.var(&format!("#compute.{name}")), expected, "{name}");
    }
    assert_eq!(sim.storage["locals"]["compute"]["root"], 4.0);
    assert_eq!(sim.storage["locals"]["compute"]["argument"], 3.0);
}

#[test]
fn integer_powers_handle_zero_and_compound_assignment() {
    let mut sim = Sim::load(
        r#"def main():
    base = 2
    positive = base ** 3
    limit = base ** 100
    zero = base ** 0
    zero_base = 0
    zero_to_zero = zero_base ** 0
    base **= 0
    negative = (-2) ** 3
"#,
        &[],
    );
    sim.call("main");
    for (name, expected) in [
        ("positive", 8),
        ("limit", 0),
        ("zero", 1),
        ("zero_to_zero", 1),
        ("base", 1),
        ("negative", -8),
    ] {
        assert_eq!(sim.var(&format!("#main.{name}")), expected, "{name}");
    }
}

#[test]
fn integer_comparisons_at_the_limits_do_not_overflow() {
    for (value, expected) in [
        (i32::MAX, vec!["Server: maximum"]),
        (i32::MIN, vec!["Server: minimum"]),
        (10, vec!["Server: middle"]),
    ] {
        let mut sim = Sim::load(
            &format!(
                r#"def main():
    value = {value}
    if value > 2147483647:
        /say wrong
    if value < -2147483648:
        /say wrong
    if value >= 2147483647:
        /say maximum
    if value <= -2147483648:
        /say minimum
    if value > 5 and value < 20:
        /say middle
"#
            ),
            &[],
        );
        sim.call("main");
        assert_eq!(sim.log, expected, "value={value}");
    }
}
