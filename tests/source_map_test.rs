use std::fs;
use std::path::PathBuf;

use cobble::transpiler::{GeneratedCommandKind, SourceMap};

#[test]
fn source_map_tracks_user_raw_command_locations() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input_file = temp_dir.path().join("main.cbl");
    let output_dir = temp_dir.path().join("output");
    fs::write(
        &input_file,
        "def test():\n    x = 1\n    score.set(\"points\", 1)\n    /say hello\n",
    )
    .unwrap();

    cobble::commands::build::build(cobble::commands::build::BuildOptions {
        input: Some(input_file.clone()),
        output: Some(output_dir.clone()),
        namespace: Some("source_map".to_string()),
        pack_format: None,
        description: None,
        verbose: false,
        zip: false,
        validate: false,
        commands_json: PathBuf::from("data/commands.json"),
    })
    .unwrap();

    let source_map_path = output_dir.join(".cobble/source_map.json");
    let source_map: SourceMap =
        serde_json::from_str(&fs::read_to_string(source_map_path).unwrap()).unwrap();

    assert_eq!(source_map.version, 1);
    assert!(source_map.entries.len() > 1);

    assert!(source_map
        .entries
        .iter()
        .any(|entry| entry.kind == GeneratedCommandKind::RuntimeSetup));
    assert!(source_map
        .entries
        .iter()
        .any(|entry| entry.kind == GeneratedCommandKind::StdLib));
    assert!(source_map
        .entries
        .iter()
        .all(|entry| !entry.command.is_empty()));

    let entry = source_map
        .entries
        .iter()
        .find(|entry| entry.command == "say hello")
        .expect("raw command source map entry missing");
    assert_eq!(
        entry.generated_path,
        "data/source_map/function/test.mcfunction"
    );
    assert_eq!(entry.command, "say hello");
    assert_eq!(entry.kind, GeneratedCommandKind::UserCommand);

    let source = entry.source.as_ref().expect("source location missing");
    assert_eq!(source.file, input_file.canonicalize().unwrap());
    assert_eq!(source.line, 4);
    assert_eq!(source.column, 5);
}

#[test]
fn source_map_stays_aligned_after_load_setup_insertions() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input_file = temp_dir.path().join("main.cbl");
    let output_dir = temp_dir.path().join("output");
    fs::write(
        &input_file,
        r#"
import stdlib
from stdlib import event

counter = 1

def load():
    /say hello

stdlib.addEventListener(event.LOAD, load)
"#,
    )
    .unwrap();

    cobble::commands::build::build(cobble::commands::build::BuildOptions {
        input: Some(input_file.clone()),
        output: Some(output_dir.clone()),
        namespace: Some("source_map".to_string()),
        pack_format: None,
        description: None,
        verbose: false,
        zip: false,
        validate: false,
        commands_json: PathBuf::from("data/commands.json"),
    })
    .unwrap();

    let load_path = output_dir.join("data/source_map/function/load.mcfunction");
    let load_lines: Vec<String> = fs::read_to_string(&load_path)
        .unwrap()
        .lines()
        .map(ToString::to_string)
        .collect();
    let generated_line = load_lines
        .iter()
        .position(|line| line == "say hello")
        .map(|index| index + 1)
        .expect("generated say command missing");

    let source_map_path = output_dir.join(".cobble/source_map.json");
    let source_map: SourceMap =
        serde_json::from_str(&fs::read_to_string(source_map_path).unwrap()).unwrap();
    let entry = source_map
        .entries
        .iter()
        .find(|entry| entry.command == "say hello")
        .expect("source map entry for say command missing");

    assert_eq!(entry.generated_line, generated_line);
    assert_eq!(load_lines[entry.generated_line - 1], entry.command);
    assert_eq!(entry.kind, GeneratedCommandKind::UserCommand);

    let source = entry.source.as_ref().expect("source location missing");
    assert_eq!(source.file, input_file.canonicalize().unwrap());
    assert_eq!(source.line, 8);
    assert_eq!(source.column, 5);
}

#[test]
fn source_map_tracks_user_commands_through_control_flow_rewrites() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input_file = temp_dir.path().join("main.cbl");
    let output_dir = temp_dir.path().join("output");
    fs::write(
        &input_file,
        "def test():\n    x = 1\n    if x > 0:\n        /say first\n        /say second\n    while x > 0:\n        /say loop\n        x = x - 1\n    for i in range(2):\n        /say step {i}\n    match x:\n        case 0:\n            /say zero\n            /say done\n        case _:\n            /say default\n    as @a:\n        /say execute body\n",
    )
    .unwrap();

    cobble::commands::build::build(cobble::commands::build::BuildOptions {
        input: Some(input_file.clone()),
        output: Some(output_dir.clone()),
        namespace: Some("source_map".to_string()),
        pack_format: None,
        description: None,
        verbose: false,
        zip: false,
        validate: false,
        commands_json: PathBuf::from("data/commands.json"),
    })
    .unwrap();

    let source_map_path = output_dir.join(".cobble/source_map.json");
    let source_map: SourceMap =
        serde_json::from_str(&fs::read_to_string(source_map_path).unwrap()).unwrap();
    let canonical_input = input_file.canonicalize().unwrap();

    let assert_user_entry = |needle: &str, path_fragment: &str, line: usize, column: usize| {
        let entry = source_map
            .entries
            .iter()
            .find(|entry| entry.command.contains(needle))
            .unwrap_or_else(|| panic!("source map entry containing '{needle}' missing"));
        assert!(
            entry.generated_path.contains(path_fragment),
            "expected generated path containing {path_fragment}, got {}",
            entry.generated_path
        );
        assert_eq!(entry.kind, GeneratedCommandKind::UserCommand);
        let source = entry.source.as_ref().expect("source location missing");
        assert_eq!(source.file, canonical_input);
        assert_eq!(source.line, line);
        assert_eq!(source.column, column);
    };

    assert_user_entry("say first", "if_temp_", 4, 9);
    assert_user_entry("say loop", "while_body_", 7, 9);
    assert_user_entry("say step", "loop_body_", 10, 9);
    assert_user_entry("say zero", "match_case_", 13, 13);
    assert_user_entry("say default", "test.mcfunction", 16, 13);
    assert_user_entry(
        "execute as @a run say execute body",
        "test.mcfunction",
        18,
        9,
    );
}

#[test]
fn source_map_tracks_generated_commands_back_to_source_statements() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input_file = temp_dir.path().join("main.cbl");
    let output_dir = temp_dir.path().join("output");
    fs::write(
        &input_file,
        r#"def helper(target):
    /say {target}

def test():
    text.tellraw('@a', 'Ready')
    score.set("points", 1)
    random.int("roll", 1, 6)
    timer.done("cooldown")
    storage.set("state", {"ready": True})
    value = math.sqrt(64)
    helper('ok')
"#,
    )
    .unwrap();

    cobble::commands::build::build(cobble::commands::build::BuildOptions {
        input: Some(input_file.clone()),
        output: Some(output_dir.clone()),
        namespace: Some("source_map".to_string()),
        pack_format: None,
        description: None,
        verbose: false,
        zip: false,
        validate: false,
        commands_json: PathBuf::from("data/commands.json"),
    })
    .unwrap();

    let source_map_path = output_dir.join(".cobble/source_map.json");
    let source_map: SourceMap =
        serde_json::from_str(&fs::read_to_string(source_map_path).unwrap()).unwrap();
    let canonical_input = input_file.canonicalize().unwrap();

    let assert_entry = |needle: &str, kind: GeneratedCommandKind, line: usize, column: usize| {
        let entry = source_map
            .entries
            .iter()
            .find(|entry| entry.command.contains(needle))
            .unwrap_or_else(|| panic!("source map entry containing '{needle}' missing"));
        assert_eq!(entry.kind, kind);
        let source = entry.source.as_ref().expect("source location missing");
        assert_eq!(source.file, canonical_input);
        assert_eq!(source.line, line);
        assert_eq!(source.column, column);
    };

    assert_entry(
        r#"tellraw @a {"text":"Ready"}"#,
        GeneratedCommandKind::StdLib,
        5,
        5,
    );
    assert_entry(
        "scoreboard players set points temp 1",
        GeneratedCommandKind::StdLib,
        6,
        5,
    );
    assert_entry(
        "execute store result score roll temp run random value 1..6",
        GeneratedCommandKind::StdLib,
        7,
        5,
    );
    assert_entry(
        "scoreboard players set cooldown_done temp 0",
        GeneratedCommandKind::StdLib,
        8,
        5,
    );
    assert_entry(
        "data modify storage source_map:global state set value {ready:1b}",
        GeneratedCommandKind::StdLib,
        9,
        5,
    );
    assert_entry(
        "function source_map:_cobble_math_sqrt",
        GeneratedCommandKind::ControlFlow,
        10,
        5,
    );
    assert_entry(
        "data modify storage source_map:global args.target set value \"ok\"",
        GeneratedCommandKind::ControlFlow,
        11,
        5,
    );
    assert_entry(
        "function source_map:helper with storage source_map:global args",
        GeneratedCommandKind::ControlFlow,
        11,
        5,
    );
}

#[test]
fn source_map_keeps_duplicate_match_commands_on_original_case_lines() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input_file = temp_dir.path().join("main.cbl");
    let output_dir = temp_dir.path().join("output");
    fs::write(
        &input_file,
        "def test():\n    x = 1\n    match x:\n        case 10:\n            /say duplicate\n        case 1:\n            /say duplicate\n",
    )
    .unwrap();

    cobble::commands::build::build(cobble::commands::build::BuildOptions {
        input: Some(input_file.clone()),
        output: Some(output_dir.clone()),
        namespace: Some("source_map".to_string()),
        pack_format: None,
        description: None,
        verbose: false,
        zip: false,
        validate: false,
        commands_json: PathBuf::from("data/commands.json"),
    })
    .unwrap();

    let source_map_path = output_dir.join(".cobble/source_map.json");
    let source_map: SourceMap =
        serde_json::from_str(&fs::read_to_string(source_map_path).unwrap()).unwrap();
    let canonical_input = input_file.canonicalize().unwrap();

    let source_line_for_match = |range: &str| {
        let entry = source_map
            .entries
            .iter()
            .find(|entry| {
                entry.command.contains("say duplicate")
                    && entry.command.contains(&format!("matches {} run", range))
            })
            .unwrap_or_else(|| panic!("source map entry for match range {range} missing"));
        assert_eq!(entry.kind, GeneratedCommandKind::UserCommand);
        let source = entry.source.as_ref().expect("source location missing");
        assert_eq!(source.file, canonical_input);
        source.line
    };

    assert_eq!(source_line_for_match("10"), 5);
    assert_eq!(source_line_for_match("1"), 7);
}

#[test]
fn source_map_tracks_module_init_escaped_strings_and_multiline_calls() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input_file = temp_dir.path().join("main.cbl");
    let output_dir = temp_dir.path().join("output");
    fs::write(
        &input_file,
        r#"counter = 1

def test():
    ok = 1
    if ok > 0:
        text.tellraw(
            '@a',
            'He said \"go\"'
        )
"#,
    )
    .unwrap();

    cobble::commands::build::build(cobble::commands::build::BuildOptions {
        input: Some(input_file.clone()),
        output: Some(output_dir.clone()),
        namespace: Some("source_map".to_string()),
        pack_format: None,
        description: None,
        verbose: false,
        zip: false,
        validate: false,
        commands_json: PathBuf::from("data/commands.json"),
    })
    .unwrap();

    let source_map_path = output_dir.join(".cobble/source_map.json");
    let source_map: SourceMap =
        serde_json::from_str(&fs::read_to_string(source_map_path).unwrap()).unwrap();
    let canonical_input = input_file.canonicalize().unwrap();

    let counter_entry = source_map
        .entries
        .iter()
        .find(|entry| entry.command == "scoreboard players set counter temp 1")
        .expect("module-level counter init source map entry missing");
    assert_eq!(counter_entry.kind, GeneratedCommandKind::ControlFlow);
    let counter_source = counter_entry.source.as_ref().expect("source missing");
    assert_eq!(counter_source.file, canonical_input);
    assert_eq!(counter_source.line, 1);
    assert_eq!(counter_source.column, 1);

    let tellraw_entry = source_map
        .entries
        .iter()
        .find(|entry| entry.command.contains(r#"He said \"go\""#))
        .expect("multiline tellraw source map entry missing");
    assert_eq!(tellraw_entry.kind, GeneratedCommandKind::StdLib);
    let tellraw_source = tellraw_entry.source.as_ref().expect("source missing");
    assert_eq!(tellraw_source.file, input_file.canonicalize().unwrap());
    assert_eq!(tellraw_source.line, 6);
    assert_eq!(tellraw_source.column, 9);
}

#[test]
fn source_map_tracks_while_condition_helper_metadata() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input_file = temp_dir.path().join("main.cbl");
    let output_dir = temp_dir.path().join("output");
    fs::write(
        &input_file,
        "def test():\n    x = 1\n    y = 0\n    while x > 0 or y > 0:\n        x = x - 1\n",
    )
    .unwrap();

    cobble::commands::build::build(cobble::commands::build::BuildOptions {
        input: Some(input_file.clone()),
        output: Some(output_dir.clone()),
        namespace: Some("source_map".to_string()),
        pack_format: None,
        description: None,
        verbose: false,
        zip: false,
        validate: false,
        commands_json: PathBuf::from("data/commands.json"),
    })
    .unwrap();

    let source_map_path = output_dir.join(".cobble/source_map.json");
    let source_map: SourceMap =
        serde_json::from_str(&fs::read_to_string(source_map_path).unwrap()).unwrap();
    let canonical_input = input_file.canonicalize().unwrap();

    let entry = source_map
        .entries
        .iter()
        .find(|entry| {
            entry.generated_path.contains("while_temp_")
                && entry.command.starts_with("scoreboard players set or_temp_")
        })
        .expect("while OR helper source map entry missing");
    let source = entry.source.as_ref().expect("source missing");
    assert_eq!(source.file, canonical_input);
    assert_eq!(source.line, 4);
    assert_eq!(source.column, 5);
}

#[test]
fn validate_reports_stale_source_map_entries() {
    let commands_json = PathBuf::from("data/commands.json");
    if !commands_json.exists() {
        return;
    }

    let temp_dir = tempfile::TempDir::new().unwrap();
    let input_file = temp_dir.path().join("main.cbl");
    let output_dir = temp_dir.path().join("output");
    fs::write(&input_file, "def test():\n    /say valid\n").unwrap();

    cobble::commands::build::build(cobble::commands::build::BuildOptions {
        input: Some(input_file),
        output: Some(output_dir.clone()),
        namespace: Some("source_map".to_string()),
        pack_format: None,
        description: None,
        verbose: false,
        zip: false,
        validate: false,
        commands_json: commands_json.clone(),
    })
    .unwrap();

    let source_map_path = output_dir.join(".cobble/source_map.json");
    let mut source_map: SourceMap =
        serde_json::from_str(&fs::read_to_string(&source_map_path).unwrap()).unwrap();
    source_map.entries[0].command = "stale bogus".to_string();
    fs::write(
        &source_map_path,
        serde_json::to_string_pretty(&source_map).unwrap(),
    )
    .unwrap();

    let report = cobble::commands::validate::run_validation(&output_dir, &commands_json).unwrap();
    assert!(
        report
            .source_map_errors
            .iter()
            .any(|error| error.contains("command mismatch")),
        "expected source map mismatch, got {:?}",
        report.source_map_errors
    );
}
