mod common;
use common::mcsim::Sim;

// Internal arithmetic helpers aren't public source-language builtins. Compile
// their test callers through the parser/transpiler rather than diagnostics.
fn internal_runtime(source: &str) -> Sim {
    let directory = tempfile::tempdir().unwrap();
    let program = cobble::parser::parse(source).unwrap();
    let mut compiler = cobble::transpiler::Transpiler::new("sim".into(), directory.path().into());
    compiler.transpile(&program).unwrap();
    compiler.data_pack.write().unwrap();
    Sim::from_output(directory.path(), &[])
}

#[test]
fn bigint_arithmetic_keeps_bits_beyond_scoreboard_and_float_precision() {
    let mut sim = internal_runtime(
        r#"def main():
    a = _cobble_big_from_decimal("9007199254740993")
    b = _cobble_big_from_decimal("4294967296")
    total = _cobble_big_add(a, b)
    product = _cobble_big_mul(a, b)
    division = _cobble_big_divmod(a, b)
    shown_total = _cobble_big_to_decimal(total)
    shown_product = _cobble_big_to_decimal(product)
    shown_q = _cobble_big_to_decimal(division.q)
    shown_r = _cobble_big_to_decimal(division.r)
    /say {shown_total} {shown_product} {shown_q} {shown_r}
"#,
    );
    sim.call("main");
    assert_eq!(
        sim.log,
        ["Server: 9007203549708289 38685626227668137885564928 2097152 1"]
    );
}

#[test]
fn bigint_shifts_and_signed_i32_magnitudes_are_exact() {
    let mut sim = internal_runtime(
        r#"def main():
    minimum = _cobble_big_from_int(-2147483648)
    shifted = _cobble_big_shl(minimum, 53)
    restored = _cobble_big_shr(shifted, 53)
    text = _cobble_big_to_decimal(restored)
    bits = _cobble_big_bits(shifted)
    /say {text} {bits}
"#,
    );
    sim.call("main");
    assert_eq!(sim.log, ["Server: 2147483648 85"]);
}

#[test]
fn software_binary64_rounds_after_the_full_precision_operation() {
    let mut sim = internal_runtime(
        r#"def main():
    a = _cobble_soft_parse("9007199254740992", 53)
    b = _cobble_soft_parse("1", 53)
    sum = _cobble_soft_add(a, b)
    product = _cobble_soft_mul(a, b)
    divided = _cobble_soft_div(b, _cobble_soft_parse("10", 53))
    total_text = _cobble_soft_decimal(sum)
    product_text = _cobble_soft_decimal(product)
    fraction_text = _cobble_soft_decimal(divided)
    /say {total_text}
    /say {product_text}
    /say {fraction_text}
"#,
    );
    sim.call("main");
    let values: Vec<f64> = sim
        .log
        .iter()
        .map(|line| line.strip_prefix("Server: ").unwrap().parse().unwrap())
        .collect();
    assert_eq!(values, [9007199254740992.0, 9007199254740992.0, 0.1]);
}

#[test]
fn software_binary64_parse_and_decimal_output_round_trip_extremes() {
    for value in [
        0.0_f64,
        -0.0,
        f64::from_bits(1),
        f64::MIN_POSITIVE,
        f64::MAX,
        -1.0e100,
    ] {
        let text = format!("{value:e}");
        let source = format!("def main():\n    bits = _cobble_soft_parse(\"{text}\", 53)\n    text = _cobble_soft_decimal(bits)\n    /say {{text}}\n");
        let mut sim = internal_runtime(&source);
        sim.set_command_budget(50_000_000);
        sim.call("main");
        let result: f64 = sim.log[0]
            .strip_prefix("Server: ")
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(result.to_bits(), value.to_bits(), "{text}: {:?}", sim.log);
    }
}
