//! Tests for stdlib v2 per-module opt-in gating.
//!
//! See `docs/stdlib-v2-design.md` for the import and versioning contract.

use std::fs;
use std::path::{Path, PathBuf};

fn compile_source(source: &str) -> Result<(tempfile::TempDir, PathBuf), String> {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input_file = temp_dir.path().join("main.cbl");
    let output_dir = temp_dir.path().join("output");
    fs::write(&input_file, source).unwrap();

    cobble::commands::build::build(cobble::commands::build::BuildOptions {
        input: Some(input_file),
        output: Some(output_dir.clone()),
        namespace: Some("stdlib_v2".to_string()),
        pack_format: None,
        description: None,
        verbose: false,
        quiet: true,
        zip: false,
        experimental_resource_pack: false,
        validate: false,
        dry_run: false,
        commands_json: PathBuf::from("data/commands.json"),
    })?;

    Ok((temp_dir, output_dir))
}

fn compile_with_config(
    source: &str,
    cobble_toml: &str,
) -> Result<(tempfile::TempDir, PathBuf), String> {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let project_dir = temp_dir.path().join("project");
    let source_dir = project_dir.join("src");
    fs::create_dir_all(&source_dir).unwrap();
    fs::write(source_dir.join("main.cbl"), source).unwrap();
    fs::write(project_dir.join("cobble.toml"), cobble_toml).unwrap();

    let output_dir = temp_dir.path().join("output");

    cobble::commands::build::build(cobble::commands::build::BuildOptions {
        input: Some(source_dir),
        output: Some(output_dir.clone()),
        namespace: None,
        pack_format: None,
        description: None,
        verbose: false,
        quiet: true,
        zip: false,
        experimental_resource_pack: false,
        validate: false,
        dry_run: false,
        commands_json: PathBuf::from("data/commands.json"),
    })?;

    Ok((temp_dir, output_dir))
}

fn read_manifest(output_dir: &Path) -> serde_json::Value {
    let manifest_path = output_dir.join(".cobble/build_manifest.json");
    serde_json::from_str(&fs::read_to_string(manifest_path).unwrap()).unwrap()
}

#[test]
fn import_stdlib_activates_all_modules() {
    let (_temp, output_dir) = compile_source(
        r#"
import stdlib

def test():
    text.tellraw('@a', 'hello')
    score.set('points', 1)
    storage.set('state', {'ready': True})
"#,
    )
    .unwrap();

    let manifest = read_manifest(&output_dir);
    assert_eq!(manifest["stdlib_version"], 2);
    let modules = manifest["active_stdlib_modules"].as_array().unwrap();
    assert!(modules.iter().any(|v| v == "text"));
    assert!(modules.iter().any(|v| v == "score"));
    assert!(modules.iter().any(|v| v == "storage"));
    assert!(modules.iter().any(|v| v == "datapack"));
    assert!(modules.iter().any(|v| v == "resource_pack"));
}

#[test]
fn from_stdlib_import_activates_only_listed_modules() {
    let (_temp, output_dir) = compile_source(
        r#"
from stdlib import text, score

def test():
    text.tellraw('@a', 'hello')
    score.set('points', 1)
"#,
    )
    .unwrap();

    let manifest = read_manifest(&output_dir);
    assert_eq!(manifest["stdlib_version"], 2);
    let modules = manifest["active_stdlib_modules"].as_array().unwrap();
    assert!(modules.iter().any(|v| v == "text"));
    assert!(modules.iter().any(|v| v == "score"));
    assert!(!modules.iter().any(|v| v == "storage"));
    assert!(!modules.iter().any(|v| v == "datapack"));
}

#[test]
fn unimported_module_call_is_rejected() {
    let error = compile_source(
        r#"
from stdlib import text

def test():
    text.tellraw('@a', 'hello')
    score.set('points', 1)
"#,
    )
    .unwrap_err();

    assert!(error.contains("module 'score' not imported"));
    assert!(error.contains("from stdlib import score"));
}

#[test]
fn no_stdlib_import_rejects_all_helpers() {
    let error = compile_source(
        r#"
def test():
    text.tellraw('@a', 'hello')
"#,
    )
    .unwrap_err();

    assert!(error.contains("module 'text' not imported"));
}

#[test]
fn event_module_gates_add_event_listener() {
    let (_temp, _output_dir) = compile_source(
        r#"
from stdlib import event

def load():
    /say hi

stdlib.addEventListener(event.LOAD, load)
"#,
    )
    .unwrap();

    let error = compile_source(
        r#"
from stdlib import text

def load():
    /say hi

stdlib.addEventListener(event.LOAD, load)
"#,
    )
    .unwrap_err();
    assert!(error.contains("module 'event' not imported"));
}

#[test]
fn unknown_stdlib_module_in_from_import_is_error() {
    let error = compile_source(
        r#"
from stdlib import nonexistent

def test():
    /say hi
"#,
    )
    .unwrap_err();

    assert!(error.contains("Unknown stdlib module 'nonexistent'"));
    assert!(error.contains("Available modules:"));
}

#[test]
fn stdlib_version_1_activates_all_modules_without_import() {
    let toml = r#"
[project]
name = "v1"
description = "v1 test"
namespace = "stdlib_v2"
pack_format = "101.1"

[build]
source = "src"
output = "output"

[stdlib]
version = 1
"#;

    let (_temp, output_dir) = compile_with_config(
        r#"
def test():
    text.tellraw('@a', 'hello')
    score.set('points', 1)
"#,
        toml,
    )
    .unwrap();

    let manifest = read_manifest(&output_dir);
    assert_eq!(manifest["stdlib_version"], 1);
    let modules = manifest["active_stdlib_modules"].as_array().unwrap();
    assert!(modules.iter().any(|v| v == "text"));
    assert!(modules.iter().any(|v| v == "score"));
}

#[test]
fn stdlib_version_2_is_default_when_config_present() {
    let toml = r#"
[project]
name = "default"
description = "default test"
namespace = "stdlib_v2"
pack_format = "101.1"

[build]
source = "src"
output = "output"
"#;

    let (_temp, output_dir) = compile_with_config(
        r#"
from stdlib import text

def test():
    text.tellraw('@a', 'hello')
"#,
        toml,
    )
    .unwrap();

    let manifest = read_manifest(&output_dir);
    assert_eq!(manifest["stdlib_version"], 2);
}

#[test]
fn datapack_module_gates_datapack_helpers() {
    let error = compile_source(
        r#"
from stdlib import text

datapack.predicate("test", {"condition": "minecraft:random_chance", "chance": 1})

def main():
    /say hi
"#,
    )
    .unwrap_err();

    assert!(error.contains("module 'datapack' not imported"));
}
