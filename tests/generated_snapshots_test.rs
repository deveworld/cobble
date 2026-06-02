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

fn build_project_fixture(project: &str) -> (TempDir, PathBuf) {
    let _lock = CWD_LOCK.lock().unwrap();
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let project_dir = repo_root.join(project);
    let temp_dir = TempDir::new().unwrap();
    let output_dir = temp_dir.path().join("output");

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
        validate: false,
        dry_run: false,
        commands_json: repo_root.join("data/commands.json"),
    })
    .unwrap();

    (temp_dir, output_dir)
}

fn build_source_fixture(source: &str, namespace: &str) -> (TempDir, PathBuf) {
    let temp_dir = TempDir::new().unwrap();
    let input_file = temp_dir.path().join("main.cbl");
    let output_dir = temp_dir.path().join("output");
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
        validate: false,
        dry_run: false,
        commands_json: PathBuf::from("data/commands.json"),
    })
    .unwrap();

    (temp_dir, output_dir)
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
        return serde_json::to_string_pretty(&value).unwrap();
    }

    content.replace("\r\n", "\n")
}

#[test]
fn snapshot_26_smoke_generated_pack_tree() {
    let (_temp, output_dir) = build_project_fixture("examples/26_smoke");
    insta::assert_snapshot!(
        "snapshot_26_smoke_generated_pack_tree",
        datapack_tree_snapshot(&output_dir)
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
    );

    insta::assert_snapshot!(
        "snapshot_resource_merge_generated_pack_tree",
        datapack_tree_snapshot(&output_dir)
    );
}
