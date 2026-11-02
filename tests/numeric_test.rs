mod common;
use common::mcsim::Sim;
use serde_json::json;

#[test]
fn float_and_double_arithmetic_survive_storage_and_calls() {
    let mut sim = Sim::load(
        r#"a = 0.6
b = double(0.1)
def plus(value):
    return value + value
def main():
    x = a + 0.2
    y = b + double(0.2)
    z = plus(x)
    w = plus(y)
"#,
        &[],
    );
    sim.set_command_budget(20_000_000);
    sim.call("main");
    assert_eq!(
        sim.storage["locals"]["main"]["x"],
        json!(f64::from(0.6_f32 + 0.2_f32))
    );
    assert_eq!(sim.storage["locals"]["main"]["y"], json!(0.1_f64 + 0.2));
    assert_eq!(
        sim.storage["locals"]["main"]["z"],
        json!(f64::from((0.6_f32 + 0.2_f32) * 2.0))
    );
    assert_eq!(
        sim.storage["locals"]["main"]["w"],
        json!((0.1_f64 + 0.2) * 2.0)
    );
}

#[test]
fn comparisons_truth_and_casts_keep_fractional_values() {
    let mut sim = Sim::load(
        r#"def check(value):
    if value and value < 1:
        /say fractional
    if value == double(0.25):
        /say equal
    narrowed = float(value)
    widened = double(narrowed)
    integer = int(-value)
    /tellraw @a f"{narrowed} {widened} {integer}"
def main():
    check(double(0.25))
"#,
        &["Alex"],
    );
    sim.set_command_budget(20_000_000);
    sim.call("main");
    assert_eq!(
        sim.log,
        ["Server: fractional", "Server: equal", "0.25 0.25 0"]
    );
}

#[test]
fn formatting_fractional_parameter_keeps_plain_decimal() {
    let mut sim = Sim::load(
        r#"def check(value):
    if entity @a and f"{value}" == "0.6":
        /say precise
def main():
    check(0.6)
"#,
        &["Alex"],
    );
    sim.set_command_budget(20_000_000);
    sim.call("main");
    assert_eq!(sim.log, ["Server: precise"], "storage: {}", sim.storage);
}

#[test]
fn entity_context_remains_intact_after_numeric_storage_lowering() {
    let mut sim = Sim::load(
        r#"def check(flag):
    if entity @s[tag=missing]:
        /say first
    elif entity @s[tag={flag}]:
        /say tagged
def main():
    /tag @a add chosen
    as @a:
        check("chosen")
        if entity @s[tag=chosen]:
            /say context
        else:
            /say wrong
"#,
        &["Alex"],
    );
    sim.call("main");
    assert_eq!(sim.log, ["Alex: tagged", "Alex: context"]);
}

#[test]
fn float_and_double_fields_lists_and_numeric_builtins_preserve_types() {
    let mut sim = Sim::load(
        r#"def main():
    values = [0.25, double(0.5), 2]
    box = {"x": double(0.1)}
    values[0] += 0.5
    box.x += double(0.2)
    values[1] = values[0] + box.x
    total = sum(values)
    smallest = min(values)
    largest = max(values)
    positive = abs(-box.x)
    picked = min(box.x, double(0.75))
    ordered = sorted(values)
    /tellraw @a f"{smallest} {largest} {positive} {picked}"
"#,
        &["Alex"],
    );
    sim.set_command_budget(40_000_000);
    sim.call("main");
    let local = &sim.storage["locals"]["main"];
    assert_eq!(local["values"][0], json!(0.75));
    assert_eq!(local["box"]["x"], json!(0.1_f64 + 0.2));
    assert_eq!(local["values"][1], json!(0.75 + (0.1_f64 + 0.2)));
    assert_eq!(local["total"], json!(0.75 + (0.75 + (0.1_f64 + 0.2)) + 2.0));
    assert_eq!(local["ordered"], json!([0.75, 0.75 + (0.1_f64 + 0.2), 2]));
    assert_eq!(sim.log, ["0.75 2 0.30000000000000004 0.30000000000000004"]);
}

#[test]
fn division_floor_modulus_and_double_precision_are_distinct() {
    let mut sim = Sim::load(
        r#"def main():
    fraction = 5.0 / 2
    floored = -5.0 // 2
    remainder = -5.5 % 2
    double_fraction = double(5) / 2
    double_floor = double(-5) // 2
    double_mod = double(-5.5) % 2
    large = double(16777216) + double(1)
    float_large = float(16777216) + 1
    tiny = double("4.9406564584124654e-324") * double(1)
"#,
        &[],
    );
    sim.set_command_budget(80_000_000);
    sim.call("main");
    let local = &sim.storage["locals"]["main"];
    assert_eq!(local["fraction"], json!(2.5));
    assert_eq!(local["floored"], json!(-3.0));
    assert_eq!(local["remainder"], json!(0.5));
    assert_eq!(local["double_fraction"], json!(2.5));
    assert_eq!(local["double_floor"], json!(-3.0));
    assert_eq!(local["double_mod"], json!(0.5));
    assert_eq!(local["large"], json!(16777217.0));
    assert_eq!(local["float_large"], json!(16777216.0));
    assert_eq!(local["tiny"].as_f64().unwrap().to_bits(), 1);
}

#[test]
fn square_roots_use_the_operands_precision() {
    let mut sim = Sim::load(
        r#"def main():
    integer = math.sqrt(10)
    single = math.sqrt(float(2))
    wide = math.sqrt(double(2))
    tiny = math.sqrt(double("4.9406564584124654e-324"))
"#,
        &[],
    );
    sim.set_command_budget(80_000_000);
    sim.call("main");
    let local = &sim.storage["locals"]["main"];
    assert_eq!(local["integer"], json!(3));
    assert_eq!(local["single"], json!(f64::from(2.0_f32.sqrt())));
    assert_eq!(local["wide"], json!(2.0_f64.sqrt()));
    assert_eq!(local["tiny"], json!(f64::from_bits(1).sqrt()));
}

#[test]
fn recursive_numeric_calls_and_fallthrough_keep_their_return_values() {
    let mut sim = Sim::load(
        r#"def add_n(value, n):
    if n <= 0:
        return value
    return add_n(value + double(0.1), n - 1)
def optional(flag):
    if flag:
        return double(0.75)
    add_n(double(0.25), 1)
def main():
    recursive = add_n(double(0.1), 3)
    returned = optional(True)
    absent = optional(False)
"#,
        &[],
    );
    sim.set_command_budget(80_000_000);
    sim.call("main");
    let local = &sim.storage["locals"]["main"];
    assert_eq!(local["recursive"], json!(((0.1_f64 + 0.1) + 0.1) + 0.1));
    assert_eq!(local["returned"], json!(0.75));
    assert_eq!(local["absent"], json!(0));
}

#[test]
fn dynamic_list_steps_follow_storage_backed_counters() {
    let mut sim = Sim::load("def main():\n    s = 2\n    a = [1, 2]\n    for v in a by s:\n        /say {v}\n        if v == 1:\n            a.append(3)\n", &[]);
    sim.set_command_budget(5000);
    sim.call("main");
    assert_eq!(sim.log, ["Server: 1", "Server: 3"]);
}

#[test]
fn a_loop_keeps_the_last_value_of_an_existing_storage_variable() {
    let mut sim = Sim::load(
        r#"def last(items):
    value = 0
    for value in items:
        pass
    return value
def main():
    result = last([7, 8])
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(
        sim.var("#main.result"),
        8,
        "locals {} function {:?}",
        sim.storage["locals"]["last"],
        sim.functions.get("last")
    );
}

#[test]
fn mixed_numeric_choices_and_lazy_conditions_preserve_selected_values() {
    let mut sim = Sim::load(
        r#"def choose(flag):
    selected = double(0.1) if flag else 0.25
    return selected + selected
def main():
    wide = choose(True)
    single = choose(False)
    zero = 0.0
    if False and (1.0 / zero > 0):
        /say wrong
    elif wide < double(0.3):
        /say selected
"#,
        &[],
    );
    sim.set_command_budget(20_000_000);
    sim.call("main");
    assert_eq!(sim.storage["locals"]["main"]["wide"], json!(0.2));
    assert_eq!(sim.storage["locals"]["main"]["single"], json!(0.5));
    assert_eq!(sim.log, ["Server: selected"]);
}

#[test]
fn list_indexes_reject_floating_tags_even_when_the_value_is_integral() {
    for index in ["0.0", "0.5", "float(0)", "double(0)"] {
        assert!(
            common::mcsim::compile(&format!(
                "def main():\n    items = [1, 2]\n    value = items[{index}]\n"
            ))
            .is_err(),
            "{index}"
        );
    }
    let mut sim = Sim::load(
        r#"def read(index):
    items = [10, 20]
    return items[index]
def main():
    wrong = read(double(0.5))
    right = read(int(double(0.5)))
"#,
        &["Alex"],
    );
    sim.set_command_budget(20_000_000);
    sim.call("main");
    assert_eq!(sim.var("#main.wrong"), 0);
    assert_eq!(sim.var("#main.right"), 10);
    assert!(sim
        .log
        .iter()
        .any(|line| line.contains("indexes must be integers")));
}

#[test]
fn decimal_float_casts_round_once_and_abs_clears_negative_zero() {
    let mut sim = Sim::load(
        r#"def convert(text):
    return float(text)
def main():
    literal = float("1.00000005960464477539062500000000001")
    runtime = convert("1.00000005960464477539062500000000001")
    single_zero = abs(-0.0)
    wide_zero = abs(double(-0.0))
"#,
        &[],
    );
    sim.set_command_budget(40_000_000);
    sim.call("main");
    let local = &sim.storage["locals"]["main"];
    let expected = f64::from(f32::from_bits(1.0_f32.to_bits() + 1));
    assert_eq!(local["literal"].as_f64(), Some(expected));
    assert_eq!(local["runtime"].as_f64(), Some(expected));
    assert_eq!(local["single_zero"].as_f64().unwrap().to_bits(), 0);
    assert_eq!(local["wide_zero"].as_f64().unwrap().to_bits(), 0);
}

#[test]
fn public_double_arithmetic_matches_binary64_at_extremes() {
    let pairs = [
        (9007199254740992.0_f64, 1.0),
        (1.0, 10.0),
        (-5.5, 2.0),
        (f64::MIN_POSITIVE, 2.0),
        (f64::MAX / 2.0, 2.0),
        (f64::from_bits(1), 0.5),
    ];
    let mut source = "def main():\n".to_string();
    let mut expected = Vec::new();
    for (index, (left, right)) in pairs.into_iter().enumerate() {
        source.push_str(&format!(
            "    a{index} = double(\"{left:e}\")\n    b{index} = double(\"{right:e}\")\n"
        ));
        for (op_index, (op, result)) in [
            ("+", left + right),
            ("-", left - right),
            ("*", left * right),
            ("/", left / right),
        ]
        .into_iter()
        .enumerate()
        {
            let name = format!("r{index}_{op_index}");
            source.push_str(&format!("    {name} = a{index} {op} b{index}\n"));
            expected.push((name, result));
        }
    }
    let mut sim = Sim::load(&source, &[]);
    sim.set_command_budget(200_000_000);
    sim.call("main");
    for (name, expected) in expected {
        let actual = sim.storage["locals"]["main"][&name].as_f64().unwrap();
        assert_eq!(
            actual.to_bits(),
            expected.to_bits(),
            "{name}: {actual:e} vs {expected:e}"
        );
    }
    assert!(sim.log.is_empty(), "{:?}", sim.log);
}

#[test]
fn doubles_loaded_from_storage_work_without_a_double_constructor_in_source() {
    let mut sim = Sim::load(
        r#"box = {"x": 0, "items": []}
def main():
    /data modify storage sim:global vars.box.x set value 0.1d
    /data modify storage sim:global vars.box.items set value [0.1d,0.2d]
    from_field = box.x + 1
    total = sum(box.items)
    box.x += 1
    if total > 0.25:
        /say storage double
"#,
        &[],
    );
    sim.set_command_budget(30_000_000);
    sim.call("main");
    assert_eq!(sim.storage["locals"]["main"]["from_field"], json!(1.1));
    assert_eq!(sim.storage["locals"]["main"]["total"], json!(0.1_f64 + 0.2));
    assert_eq!(sim.storage["vars"]["box"]["x"], json!(1.1));
    assert_eq!(sim.log, ["Server: storage double"]);
}

#[test]
fn native_float_only_arithmetic_does_not_emit_the_software_double_library() {
    let mut sim = Sim::load(
        "def main():\n    x = float(0.6) + 0.2\n    y = math.sqrt(x)\n",
        &[],
    );
    assert!(!sim
        .functions
        .keys()
        .any(|name| name.starts_with("_cobble_soft_") || name.starts_with("_cobble_big_")));
    sim.call("main");
    assert_eq!(
        sim.storage["locals"]["main"]["x"],
        json!(f64::from(0.8_f32))
    );
}

#[test]
fn decimal_literals_round_directly_to_float_without_an_intermediate_double() {
    let decimal = "1.0000000596046447753906250000000000000000000000000000000000001";
    let mut sim = Sim::load(
        &format!("def main():\n    single = {decimal}\n    const VALUE = {decimal}\n    constant = VALUE\n    wide = double({decimal})\n"),
        &[],
    );
    sim.call("main");
    let local = &sim.storage["locals"]["main"];
    assert_eq!(
        local["single"],
        json!(f64::from(f32::from_bits(1.0_f32.to_bits() + 1)))
    );
    assert_eq!(local["constant"], local["single"]);
    assert_eq!(local["wide"], json!(decimal.parse::<f64>().unwrap()));
}

#[test]
fn entity_template_nbt_accepts_integral_and_fractional_float_literals() {
    let sim = Sim::load(
        "define @Target = @e[type=minecraft:marker]\ncreate {\n    \"health\": 100.0,\n    \"scale\": 0.25\n}\nend\ndef main():\n    create @Target\n",
        &[],
    );
    let commands = sim.functions.values().flatten().collect::<Vec<_>>();
    assert!(commands
        .iter()
        .any(|line| line.contains("health:1e2f") || line.contains("\"health\":1e2f")));
}

#[test]
fn numeric_cast_dependencies_are_available_inside_raw_call_conditions() {
    let mut sim = Sim::load(
        r#"def check(value):
    return value
def main():
    text = "0.125"
    as @a if check(double(text)):
        /say converted
"#,
        &["Alex"],
    );
    sim.set_command_budget(20_000_000);
    sim.call("main");
    assert_eq!(sim.log, ["Alex: converted"]);
}
