//! Evaluation order and nested arithmetic, executed with the shared Minecraft
//! simulator so storage, integer registers, and calls use the same semantics.
mod common;
use common::mcsim::Sim;

#[test]
fn self_referential_rhs_evaluates_old_value() {
    let sim = Sim::load(
        "x = 5\nx = 2 - x\ny = 5\ny = 10 / y\nz = 5\nz = 3 * z + z\n",
        &[],
    );
    assert_eq!(sim.var("x"), -3);
    assert_eq!(sim.var("y"), 2);
    assert_eq!(sim.var("z"), 20);
}

#[test]
fn self_assign_literal_fast_path_in_function_unchanged() {
    let mut sim = Sim::load(
        "v = 5\nv = v - 2\ndef compute():\n    w = 5\n    w = w - 2\n",
        &[],
    );
    assert!(sim
        .function_text("compute")
        .contains("scoreboard players remove #compute.w sim.vars 2"));
    sim.call("compute");
    assert_eq!(sim.var("v"), 3);
    assert_eq!(sim.var("#compute.w"), 3);
}

#[test]
fn nested_right_operands_use_unique_scratch_scores() {
    let sim = Sim::load("a = 6\nb = 9\nc = 2\nd = 3\nr4 = a * (b * c)\nr5 = a * (b * (c * d))\nr6 = a - (b - (c - d))\n", &[]);
    assert_eq!(sim.var("r4"), 108);
    assert_eq!(sim.var("r5"), 324);
    assert_eq!(sim.var("r6"), -4);
}

#[test]
fn nested_math_calls_copy_results_back_into_math_inputs() {
    let mut sim = Sim::load("def compute():\n    a = math.sqrt(16)\n    b = math.sqrt(math.sqrt(256))\n    c = math.max(math.abs(-5), 3)\n", &[]);
    sim.call("compute");
    assert_eq!(sim.var("#compute.a"), 4);
    assert_eq!(sim.var("#compute.b"), 4);
    assert_eq!(sim.var("#compute.c"), 5);
}

#[test]
fn mixed_math_call_and_nested_subtraction() {
    let sim = Sim::load(
        "a = 6\nb = 9\nc = 2\nd = 3\nr = math.max(a, b) - (c - (d - 1))\n",
        &[],
    );
    assert_eq!(sim.var("r"), 9);
}

#[test]
fn nested_math_call_in_second_argument_does_not_clobber_first() {
    let mut sim = Sim::load("def compute():\n    r = math.max(2, math.abs(-9))\n    s = math.min(math.abs(-4), math.max(1, 6))\n", &[]);
    sim.call("compute");
    assert_eq!(sim.var("#compute.r"), 9);
    assert_eq!(sim.var("#compute.s"), 4);
}
