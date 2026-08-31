//! Regression tests for expression evaluator codegen fixes:
//! - self-referential right-hand sides (`x = 2 - x`)
//! - unique scratch scores for nested right operands (`a * (b * (c * d))`)
//! - math.* result copy honoring the target objective (nested math calls)
//!
//! The generated pack is executed with a small scoreboard interpreter that
//! mirrors Minecraft's command semantics for the commands the pack emits.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// Compile a Cobble source and return the tempdir plus the output directory.
fn compile_source(source: &str) -> Result<(TempDir, PathBuf), String> {
    let temp_dir = TempDir::new().unwrap();
    let input_file = temp_dir.path().join("test.cbl");
    let output_dir = temp_dir.path().join("output");

    let full_source = format!("import stdlib\n{source}");
    fs::write(&input_file, &full_source).unwrap();

    cobble::commands::build::build(cobble::commands::build::BuildOptions {
        input: Some(input_file),
        output: Some(output_dir.clone()),
        namespace: None,
        pack_format: None,
        description: None,
        verbose: false,
        quiet: false,
        zip: false,
        experimental_resource_pack: false,
        validate: false,
        dry_run: false,
        commands_json: PathBuf::from("data/commands.json"),
    })?;

    Ok((temp_dir, output_dir))
}

fn read_function(output_dir: &Path, function_name: &str) -> String {
    let function_path = output_dir
        .join("data/cobble/function")
        .join(format!("{}.mcfunction", function_name));
    fs::read_to_string(&function_path)
        .unwrap_or_else(|e| panic!("cannot read {}: {}", function_path.display(), e))
}

/// Load every generated mcfunction of the `cobble` namespace by name.
fn load_pack_functions(output_dir: &Path) -> HashMap<String, Vec<String>> {
    let function_dir = output_dir.join("data/cobble/function");
    let mut functions = HashMap::new();
    for entry in fs::read_dir(&function_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|ext| ext == "mcfunction") {
            let name = path.file_stem().unwrap().to_string_lossy().to_string();
            let commands = fs::read_to_string(&path)
                .unwrap()
                .lines()
                .map(str::to_string)
                .collect();
            functions.insert(name, commands);
        }
    }
    functions
}

/// Scoreboard state: (player, objective) -> value, mirroring i32 scores.
type Scores = HashMap<(String, String), i64>;

fn get_score(scores: &Scores, player: &str, objective: &str) -> i64 {
    scores
        .get(&(player.to_string(), objective.to_string()))
        .copied()
        .unwrap_or(0)
}

fn set_score(scores: &mut Scores, player: &str, objective: &str, value: i64) {
    scores.insert(
        (player.to_string(), objective.to_string()),
        value as i32 as i64,
    );
}

/// Minecraft `/=` rounds toward negative infinity; `%=` is a floor modulus.
fn floor_div(a: i64, b: i64) -> i64 {
    let q = a / b;
    if a % b != 0 && ((a < 0) != (b < 0)) {
        q - 1
    } else {
        q
    }
}

fn floor_mod(a: i64, b: i64) -> i64 {
    a - floor_div(a, b) * b
}

fn matches_range(value: i64, range: &str) -> bool {
    if let Some(idx) = range.find("..") {
        let (low, high) = (&range[..idx], &range[idx + 2..]);
        let low_ok = low.is_empty() || value >= low.parse::<i64>().unwrap();
        let high_ok = high.is_empty() || value <= high.parse::<i64>().unwrap();
        low_ok && high_ok
    } else {
        value == range.parse::<i64>().unwrap()
    }
}

fn run_pack_function(functions: &HashMap<String, Vec<String>>, name: &str, scores: &mut Scores) {
    let commands = functions
        .get(name)
        .unwrap_or_else(|| panic!("function `{}` not found in generated pack", name));
    for command in commands {
        exec_command(functions, command, scores);
    }
}

fn exec_command(functions: &HashMap<String, Vec<String>>, command: &str, scores: &mut Scores) {
    let command = command.trim();
    if command.is_empty()
        || command.starts_with('#')
        || command.starts_with("gamerule")
        || command.starts_with("scoreboard objectives")
        || command.starts_with("tellraw")
        || command.starts_with("data ")
    {
        return;
    }

    if let Some(rest) = command.strip_prefix("function ") {
        let name = rest
            .split(':')
            .nth(1)
            .unwrap_or_else(|| panic!("unqualified function call: {}", command));
        run_pack_function(functions, name, scores);
        return;
    }

    if command.starts_with("execute ") {
        let tokens: Vec<&str> = command.split_whitespace().collect();
        let mut i = 1;
        while tokens[i] == "if" || tokens[i] == "unless" {
            let negate = tokens[i] == "unless";
            assert_eq!(
                tokens[i + 1],
                "score",
                "unsupported execute clause: {command}"
            );
            let value = get_score(scores, tokens[i + 2], tokens[i + 3]);
            let passed = if tokens[i + 4] == "matches" {
                let passed = matches_range(value, tokens[i + 5]);
                i += 6;
                passed
            } else {
                let other = get_score(scores, tokens[i + 5], tokens[i + 6]);
                let passed = match tokens[i + 4] {
                    "<" => value < other,
                    "<=" => value <= other,
                    "=" => value == other,
                    ">=" => value >= other,
                    ">" => value > other,
                    op => panic!("unsupported comparison `{op}` in: {command}"),
                };
                i += 7;
                passed
            };
            if passed == negate {
                return;
            }
        }
        assert_eq!(tokens[i], "run", "unsupported execute chain: {command}");
        let rest = tokens[i + 1..].join(" ");
        exec_command(functions, &rest, scores);
        return;
    }

    if let Some(rest) = command.strip_prefix("scoreboard players ") {
        let tokens: Vec<&str> = rest.split_whitespace().collect();
        match tokens[0] {
            "set" => set_score(scores, tokens[1], tokens[2], tokens[3].parse().unwrap()),
            "add" => {
                let value = get_score(scores, tokens[1], tokens[2]);
                let delta: i64 = tokens[3].parse().unwrap();
                set_score(scores, tokens[1], tokens[2], value + delta);
            }
            "remove" => {
                let value = get_score(scores, tokens[1], tokens[2]);
                let delta: i64 = tokens[3].parse().unwrap();
                set_score(scores, tokens[1], tokens[2], value - delta);
            }
            "operation" => {
                let lhs = get_score(scores, tokens[1], tokens[2]);
                let rhs = get_score(scores, tokens[4], tokens[5]);
                let result = match tokens[3] {
                    "=" => rhs,
                    "+=" => lhs + rhs,
                    "-=" => lhs - rhs,
                    "*=" => lhs * rhs,
                    "/=" => floor_div(lhs, rhs),
                    "%=" => floor_mod(lhs, rhs),
                    "<" => lhs.min(rhs),
                    ">" => lhs.max(rhs),
                    op => panic!("unsupported operation `{op}` in: {command}"),
                };
                set_score(scores, tokens[1], tokens[2], result);
            }
            op => panic!("unsupported scoreboard subcommand `{op}` in: {command}"),
        }
        return;
    }

    panic!("test interpreter cannot execute: {command}");
}

/// Build, run `_cobble_init`, and return the scores.
fn run_init(source: &str) -> (Scores, String) {
    let (_temp, output_dir) = compile_source(source).expect("build failed");
    let functions = load_pack_functions(&output_dir);
    let init = read_function(&output_dir, "_cobble_init");
    let mut scores = Scores::new();
    run_pack_function(&functions, "_cobble_init", &mut scores);
    (scores, init)
}

#[test]
fn self_referential_rhs_evaluates_old_value() {
    let source = r#"
x = 5
x = 2 - x
y = 5
y = 10 / y
z = 5
z = 3 * z + z
"#;
    let (scores, init) = run_init(source);
    assert_eq!(get_score(&scores, "x", "temp"), -3, "x = 2 - x with x=5");
    assert_eq!(get_score(&scores, "y", "temp"), 2, "y = 10 / y with y=5");
    assert_eq!(
        get_score(&scores, "z", "temp"),
        20,
        "z = 3 * z + z with z=5"
    );
    // The self-referential RHS must be routed through a scratch score that is
    // copied into the target at the end.
    assert!(
        init.contains("scoreboard players operation x temp = #expr_result_"),
        "expected scratch copy for x in init:\n{init}"
    );
}

#[test]
fn self_assign_literal_fast_path_in_function_unchanged() {
    let source = r#"
v = 5
v = v - 2

def compute():
    w = 5
    w = w - 2
"#;
    let (_temp, output_dir) = compile_source(source).expect("build failed");
    let functions = load_pack_functions(&output_dir);

    // Inside a function, `w = w - 2` keeps the optimized single-command form.
    let body = read_function(&output_dir, "compute");
    assert!(
        body.contains("scoreboard players remove w temp 2"),
        "fast path lost in function body:\n{body}"
    );
    assert!(
        !body.contains("#expr_result_"),
        "fast path should not use a scratch score:\n{body}"
    );

    let mut scores = Scores::new();
    run_pack_function(&functions, "_cobble_init", &mut scores);
    assert_eq!(get_score(&scores, "v", "temp"), 3, "v = v - 2 with v=5");
    run_pack_function(&functions, "compute", &mut scores);
    assert_eq!(get_score(&scores, "w", "temp"), 3, "w = w - 2 with w=5");
}

#[test]
fn nested_right_operands_use_unique_scratch_scores() {
    let source = r#"
a = 6
b = 9
c = 2
d = 3
r4 = a * (b * c)
r5 = a * (b * (c * d))
r6 = a - (b - (c - d))
"#;
    let (scores, init) = run_init(source);
    assert_eq!(get_score(&scores, "r4", "temp"), 108, "a * (b * c)");
    assert_eq!(get_score(&scores, "r5", "temp"), 324, "a * (b * (c * d))");
    assert_eq!(get_score(&scores, "r6", "temp"), -4, "a - (b - (c - d))");
    // Every right-operand scratch is suffixed; the shared bare name is gone.
    assert!(
        !init.contains("#expr_temp "),
        "bare #expr_temp scratch should no longer be emitted:\n{init}"
    );
}

#[test]
fn nested_math_calls_use_math_objective_for_intermediate_targets() {
    let source = r#"
def compute():
    a = math.sqrt(16)
    b = math.sqrt(math.sqrt(256))
    c = math.max(math.abs(-5), 3)
"#;
    let (_temp, output_dir) = compile_source(source).expect("build failed");
    let functions = load_pack_functions(&output_dir);
    let body = read_function(&output_dir, "compute");
    // The nested call result must be copied back into `#math_input math`,
    // not into the hardcoded `temp` objective.
    assert!(
        body.contains("scoreboard players operation #math_input math = #math_result math"),
        "nested math result copy should target the math objective:\n{body}"
    );

    let mut scores = Scores::new();
    run_pack_function(&functions, "compute", &mut scores);
    assert_eq!(get_score(&scores, "a", "temp"), 4, "math.sqrt(16)");
    assert_eq!(
        get_score(&scores, "b", "temp"),
        4,
        "math.sqrt(math.sqrt(256))"
    );
    assert_eq!(
        get_score(&scores, "c", "temp"),
        5,
        "math.max(math.abs(-5), 3)"
    );
}

#[test]
fn mixed_math_call_and_nested_subtraction() {
    let source = r#"
a = 6
b = 9
c = 2
d = 3
r = math.max(a, b) - (c - (d - 1))
"#;
    let (scores, _init) = run_init(source);
    // max(6, 9) - (2 - (3 - 1)) = 9 - 0 = 9
    assert_eq!(
        get_score(&scores, "r", "temp"),
        9,
        "math.max(a, b) - (c - (d - 1))"
    );
}

#[test]
fn nested_math_call_in_second_argument_does_not_clobber_first() {
    let source = r#"
def compute():
    r = math.max(2, math.abs(-9))
    s = math.min(math.abs(-4), math.max(1, 6))
"#;
    let (_temp, output_dir) = compile_source(source).expect("build failed");
    let functions = load_pack_functions(&output_dir);
    let body = read_function(&output_dir, "compute");

    // Arguments are staged into unique scratches and copied into the shared
    // helper inputs only after both arguments are fully evaluated, so a
    // nested math call in the second argument cannot clobber the first.
    let lines: Vec<&str> = body.lines().collect();
    let abs_call = lines
        .iter()
        .position(|line| line.ends_with(":_cobble_math_abs"))
        .expect("abs helper call present");
    let input1_copy = lines
        .iter()
        .position(|line| {
            line.starts_with("scoreboard players operation #math_input math = #math_arg_")
        })
        .expect("staged copy into #math_input present");
    let max_call = lines
        .iter()
        .position(|line| line.ends_with(":_cobble_math_max"))
        .expect("max helper call present");
    assert!(
        abs_call < input1_copy && input1_copy < max_call,
        "nested abs must run before the shared inputs are staged:\n{body}"
    );

    let mut scores = Scores::new();
    run_pack_function(&functions, "compute", &mut scores);
    assert_eq!(
        get_score(&scores, "r", "temp"),
        9,
        "math.max(2, math.abs(-9))"
    );
    assert_eq!(
        get_score(&scores, "s", "temp"),
        4,
        "math.min(math.abs(-4), math.max(1, 6))"
    );
}
