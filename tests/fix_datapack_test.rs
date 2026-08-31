use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

/// Build helper mirroring tests/integration_test.rs. Unlike `compile_source`
/// there, this keeps the input and output paths so a project can be rebuilt
/// into the same output directory to observe cross-rebuild behavior.
fn build_options(input: PathBuf, output: PathBuf) -> cobble::commands::build::BuildOptions {
    cobble::commands::build::BuildOptions {
        input: Some(input),
        output: Some(output),
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
    }
}

fn write_project(source: &str) -> (TempDir, PathBuf, PathBuf) {
    let temp_dir = TempDir::new().unwrap();
    let input_file = temp_dir.path().join("test.cbl");
    let output_dir = temp_dir.path().join("output");
    fs::write(&input_file, source).unwrap();
    (temp_dir, input_file, output_dir)
}

/// Regression test for the divergent tag-merge algorithms: the on-disk merge
/// in writer.rs used to append stdlib entries unsorted and without
/// normalization, while the in-memory merge in tags.rs sorted, deduplicated,
/// and normalized `required: true` object entries to strings. Both paths must
/// now produce identical normalized output, stable across rebuilds.
#[test]
fn merged_function_tag_is_sorted_normalized_and_stable_across_rebuilds() {
    let source = r#"import stdlib
from stdlib import event

datapack.function_tag("minecraft:load", ["zzz:setup", {"id": "aaa:setup", "required": True}, {"id": "bbb:optional", "required": False}])

def on_load():
    /say hi

stdlib.addEventListener(event.LOAD, on_load)
"#;
    let (_temp, input_file, output_dir) = write_project(source);

    cobble::commands::build::build(build_options(input_file.clone(), output_dir.clone())).unwrap();
    let load_path = output_dir.join("data/minecraft/tags/function/load.json");
    let first = fs::read_to_string(&load_path).unwrap();

    cobble::commands::build::build(build_options(input_file, output_dir.clone())).unwrap();
    let second = fs::read_to_string(&load_path).unwrap();

    assert_eq!(
        first, second,
        "minecraft/tags/function/load.json must be byte-identical across rebuilds"
    );

    let tag: serde_json::Value = serde_json::from_str(&first).unwrap();
    let values = tag["values"].as_array().unwrap();
    // Sorted by id; `required: true` object normalized to a plain string;
    // `required: false` object preserved; stdlib load handler merged in at
    // its sorted position instead of being prepended; no duplicates.
    assert_eq!(
        values,
        &vec![
            serde_json::json!("aaa:setup"),
            serde_json::json!({"id": "bbb:optional", "required": false}),
            serde_json::json!("cobble:on_load"),
            serde_json::json!("zzz:setup"),
        ],
        "merged tag values must be normalized and sorted"
    );
}

/// The merged tag must not grow or reorder when the same project is rebuilt
/// repeatedly into the same output directory.
#[test]
fn merged_function_tag_does_not_grow_across_many_rebuilds() {
    let source = r#"import stdlib
from stdlib import event

datapack.function_tag("minecraft:load", ["other:setup"])

def on_load():
    /say hi

stdlib.addEventListener(event.LOAD, on_load)
"#;
    let (_temp, input_file, output_dir) = write_project(source);
    let load_path = output_dir.join("data/minecraft/tags/function/load.json");

    let mut contents = Vec::new();
    for _ in 0..3 {
        cobble::commands::build::build(build_options(input_file.clone(), output_dir.clone()))
            .unwrap();
        contents.push(fs::read_to_string(&load_path).unwrap());
    }
    assert_eq!(contents[0], contents[1]);
    assert_eq!(contents[1], contents[2]);

    let tag: serde_json::Value = serde_json::from_str(&contents[2]).unwrap();
    assert_eq!(
        tag["values"],
        serde_json::json!(["cobble:on_load", "other:setup"]),
        "tag values must stay sorted with no duplicates after repeated rebuilds"
    );
}

/// Regression test for the canonicalization cache in source_locations.rs:
/// source_map.json paths are a v1 contract and must stay relative to the
/// source display root and byte-identical across rebuilds.
#[test]
fn source_map_paths_are_relative_and_stable_across_rebuilds() {
    let source = r#"import stdlib

def main():
    /say one
    /say two
    /say three
"#;
    let (temp, input_file, output_dir) = write_project(source);

    cobble::commands::build::build(build_options(input_file.clone(), output_dir.clone())).unwrap();
    let source_map_path = output_dir.join(".cobble/source_map.json");
    let first = fs::read_to_string(&source_map_path).unwrap();

    cobble::commands::build::build(build_options(input_file, output_dir.clone())).unwrap();
    let second = fs::read_to_string(&source_map_path).unwrap();

    assert_eq!(
        first, second,
        ".cobble/source_map.json must be byte-identical across rebuilds"
    );

    let source_map: serde_json::Value = serde_json::from_str(&first).unwrap();
    assert_eq!(source_map["version"], 1);
    let entries = source_map["entries"].as_array().unwrap();
    assert!(!entries.is_empty());

    let temp_root = temp.path().canonicalize().unwrap();
    let temp_root_display = temp_root.to_string_lossy().to_string();
    let mut saw_relative_source = false;
    for entry in entries {
        if let Some(file) = entry["source"]["file"].as_str() {
            assert!(
                !file.contains(&temp_root_display),
                "source path must be relative to the display root, got {file}"
            );
            if file == "test.cbl" {
                saw_relative_source = true;
            }
        }
    }
    assert!(
        saw_relative_source,
        "expected at least one entry mapped to the relative path test.cbl"
    );
}
