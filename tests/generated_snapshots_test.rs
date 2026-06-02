use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::Value;
use tempfile::TempDir;
use walkdir::WalkDir;

static CWD_LOCK: Mutex<()> = Mutex::new(());

struct CurrentDirGuard {
    previous: PathBuf,
}

impl CurrentDirGuard {
    fn push(path: &Path) -> Self {
        let previous = std::env::current_dir().unwrap();
        std::env::set_current_dir(path).unwrap();
        Self { previous }
    }
}

impl Drop for CurrentDirGuard {
    fn drop(&mut self) {
        std::env::set_current_dir(&self.previous).unwrap();
    }
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn commands_json_fixture(temp_dir: &Path, validate: bool) -> PathBuf {
    let commands_json = temp_dir.join("commands-fixture.json");
    if validate {
        fs::write(
            &commands_json,
            r#"{
                "type": "root",
                "children": {
                    "say": {
                        "type": "literal",
                        "children": {
                            "message": {
                                "type": "argument",
                                "parser": "minecraft:message",
                                "executable": true
                            }
                        }
                    }
                }
            }"#,
        )
        .unwrap();
    }
    commands_json
}

fn build_project_fixture(project: &str, validate: bool) -> (TempDir, PathBuf) {
    let _lock = CWD_LOCK.lock().unwrap();
    let project_dir = repo_root().join(project);
    let temp_dir = TempDir::new().unwrap();
    let output_dir = temp_dir.path().join("output");
    let commands_json = commands_json_fixture(temp_dir.path(), validate);

    let _guard = CurrentDirGuard::push(&project_dir);
    cobble::commands::build::build(cobble::commands::build::BuildOptions {
        input: None,
        output: Some(output_dir.clone()),
        namespace: None,
        pack_format: None,
        description: None,
        verbose: false,
        quiet: true,
        zip: false,
        validate,
        dry_run: false,
        commands_json,
    })
    .unwrap();

    (temp_dir, output_dir)
}

fn build_source_fixture(source: &str, namespace: &str, validate: bool) -> (TempDir, PathBuf) {
    let temp_dir = TempDir::new().unwrap();
    let input_file = temp_dir.path().join("main.cbl");
    let output_dir = temp_dir.path().join("output");
    let commands_json = commands_json_fixture(temp_dir.path(), validate);
    fs::write(&input_file, source).unwrap();

    cobble::commands::build::build(cobble::commands::build::BuildOptions {
        input: Some(input_file),
        output: Some(output_dir.clone()),
        namespace: Some(namespace.to_string()),
        pack_format: None,
        description: Some("Snapshot fixture".to_string()),
        verbose: false,
        quiet: true,
        zip: false,
        validate,
        dry_run: false,
        commands_json,
    })
    .unwrap();

    (temp_dir, output_dir)
}

fn normalized_file_snapshot(output_dir: &Path, relative_path: &str) -> String {
    normalized_file_content(relative_path, &output_dir.join(relative_path))
}

fn datapack_tree_snapshot(output_dir: &Path) -> String {
    let mut files: Vec<_> = WalkDir::new(output_dir)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| entry.into_path())
        .collect();
    files.sort();

    let mut snapshot = String::new();
    for file in files {
        let relative = file
            .strip_prefix(output_dir)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let content = normalized_file_content(&relative, &file);
        snapshot.push_str("===== ");
        snapshot.push_str(&relative);
        snapshot.push_str(" =====\n");
        snapshot.push_str(content.trim_end());
        snapshot.push_str("\n\n");
    }
    snapshot
}

fn normalized_file_content(relative_path: &str, file: &Path) -> String {
    let content = fs::read_to_string(file)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", file.display()));

    if relative_path == ".cobble/build_manifest.json" {
        let mut value: Value = serde_json::from_str(&content).unwrap();
        value["cobble_version"] = Value::String("<cobble-version>".to_string());
        if let Some(validation) = value.get_mut("validation").and_then(Value::as_object_mut) {
            if validation.contains_key("commands_json") {
                validation.insert(
                    "commands_json".to_string(),
                    Value::String("<commands-json>".to_string()),
                );
            }
        }
        return serde_json::to_string_pretty(&value).unwrap();
    }

    content.replace("\r\n", "\n")
}

#[test]
fn snapshot_26_smoke_generated_pack_tree() {
    let (_temp, output_dir) = build_project_fixture("examples/26_smoke", false);
    insta::assert_snapshot!(
        "snapshot_26_smoke_generated_pack_tree",
        datapack_tree_snapshot(&output_dir)
    );
}

#[test]
fn snapshot_26_feature_matrix_generated_pack_tree() {
    let (_temp, output_dir) = build_project_fixture("examples/26_feature_matrix", false);
    insta::assert_snapshot!(
        "snapshot_26_feature_matrix_generated_pack_tree",
        datapack_tree_snapshot(&output_dir)
    );
}

#[test]
fn snapshot_inventory_generated_pack_tree() {
    let (_temp, output_dir) = build_source_fixture(
        &fs::read_to_string(repo_root().join("examples/inventory.cbl")).unwrap(),
        "inventory_snapshot",
        false,
    );
    insta::assert_snapshot!(
        "snapshot_inventory_generated_pack_tree",
        datapack_tree_snapshot(&output_dir)
    );
}

#[test]
fn snapshot_resource_only_generated_pack_tree() {
    let (_temp, output_dir) = build_source_fixture(
        r#"
datapack.block_tag("mineable/test", ["minecraft:stone", "minecraft:deepslate"])
datapack.item_tag("rewards", ["minecraft:diamond"])
datapack.entity_type_tag("targets", ["minecraft:zombie"])
datapack.advancement("root", {"criteria": {"tick": {"trigger": "minecraft:tick"}}})
datapack.loot_table("empty", {"type": "minecraft:empty"})
datapack.recipe("stonecutting/test", {
    "type": "minecraft:stonecutting",
    "ingredient": "minecraft:stone",
    "result": {"id": "minecraft:stone"}
})
datapack.item_modifier("set_name", {"function": "minecraft:set_name", "name": {"text": "Test"}})
datapack.dialog("notice", {"type": "minecraft:notice", "title": {"text": "Notice"}})
"#,
        "resource_only",
        false,
    );
    insta::assert_snapshot!(
        "snapshot_resource_only_generated_pack_tree",
        datapack_tree_snapshot(&output_dir)
    );
}

#[test]
fn snapshot_validated_manifest_metadata() {
    let (_temp, output_dir) = build_source_fixture(
        "def main():\n    /say validated\n",
        "validated_snapshot",
        true,
    );
    insta::assert_snapshot!(
        "snapshot_validated_manifest_metadata",
        normalized_file_snapshot(&output_dir, ".cobble/build_manifest.json")
    );
}

#[test]
fn snapshot_resource_merge_generated_pack_tree() {
    let (_temp, output_dir) = build_source_fixture(
        r#"
import stdlib
from stdlib import event

datapack.function_tag("minecraft:load", ["snapshot_merge:extra_load"])
datapack.predicate("checks/ready", {
    "condition": "minecraft:random_chance",
    "chance": 1
})

def load():
    /say load

def extra_load():
    /say extra

stdlib.addEventListener(event.LOAD, load)
"#,
        "snapshot_merge",
        false,
    );

    insta::assert_snapshot!(
        "snapshot_resource_merge_generated_pack_tree",
        datapack_tree_snapshot(&output_dir)
    );
}
