use std::path::{Path, PathBuf};
use std::sync::Mutex;

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

fn build_fixture(project: &str) {
    let _lock = CWD_LOCK.lock().unwrap();
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let project_dir = repo_root.join(project);
    let temp_dir = tempfile::TempDir::new().unwrap();
    let output_dir = temp_dir.path().join("output");
    let commands_json = repo_root.join("data/commands.json");

    let _guard = CurrentDirGuard::push(&project_dir);
    cobble::commands::build::build(cobble::commands::build::BuildOptions {
        input: None,
        output: Some(output_dir.clone()),
        namespace: None,
        pack_format: None,
        description: None,
        verbose: false,
        quiet: false,
        zip: false,
        experimental_resource_pack: false,
        validate: commands_json.exists(),
        dry_run: false,
        commands_json,
    })
    .unwrap();

    let inspect = std::process::Command::new(env!("CARGO_BIN_EXE_cobble"))
        .arg("inspect")
        .arg(&output_dir)
        .arg("--json")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&inspect.stdout);
    assert!(
        inspect.status.success(),
        "inspect failed for {project}: {}",
        String::from_utf8_lossy(&inspect.stderr)
    );
    let value: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["ok"], true, "{project}: {stdout}");
}

#[test]
fn fixture_26_smoke_builds_and_validates_when_command_tree_exists() {
    build_fixture("tests/fixtures/projects/26_smoke");
}

#[test]
fn fixture_26_feature_matrix_builds_and_validates_when_command_tree_exists() {
    build_fixture("tests/fixtures/projects/26_feature_matrix");
}

#[test]
fn fixture_resource_authoring_builds_and_validates_when_command_tree_exists() {
    build_fixture("tests/fixtures/projects/resource_authoring");
}

#[test]
fn fixture_resource_pack_builds_and_validates_when_command_tree_exists() {
    build_fixture("tests/fixtures/projects/resource_pack");
}

#[test]
fn fixture_stdlib_v2_builds_and_validates_when_command_tree_exists() {
    build_fixture("tests/fixtures/projects/stdlib_v2");
}

#[test]
fn fixture_stdlib_v3_builds_and_validates_when_command_tree_exists() {
    build_fixture("tests/fixtures/projects/stdlib_v3");
}

#[test]
fn fixture_unrolling_builds_and_validates_when_command_tree_exists() {
    build_fixture("tests/fixtures/projects/unrolling");
}

#[test]
fn fixture_showcase_builds_and_validates_when_command_tree_exists() {
    build_fixture("tests/fixtures/projects/showcase");
}

#[test]
fn example_lobby_builds_and_validates_when_command_tree_exists() {
    build_fixture("examples/lobby");
}

#[test]
fn example_parkour_builds_and_validates_when_command_tree_exists() {
    build_fixture("examples/parkour");
}

#[test]
fn example_magic_items_builds_and_validates_when_command_tree_exists() {
    build_fixture("examples/magic_items");
}
