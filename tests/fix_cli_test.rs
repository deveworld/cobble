//! CLI regression tests for output-safety and CLI-surface fixes:
//! plain-build ownership gating, `clean --output` ownership, link target
//! validation, init ordering, entry-point dedup, migrate pack_format
//! comparison, and `check --json` runtime-for diagnostic kinds.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn cobble() -> Command {
    Command::new(env!("CARGO_BIN_EXE_cobble"))
}

fn write_source(temp_dir: &Path, source: &str) -> PathBuf {
    let input = temp_dir.join("main.cbl");
    fs::write(&input, source).unwrap();
    input
}

fn output_text(output: &Output) -> (String, String) {
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn init_project(project_dir: &Path) {
    let output = cobble()
        .arg("init")
        .arg("--name")
        .arg(project_dir)
        .arg("--template")
        .arg("minimal")
        .output()
        .unwrap();
    let (stdout, stderr) = output_text(&output);
    assert!(
        output.status.success(),
        "init failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

fn build_project(project_dir: &Path) {
    let output = cobble().arg("build").arg(project_dir).output().unwrap();
    let (stdout, stderr) = output_text(&output);
    assert!(
        output.status.success(),
        "build failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

#[test]
fn cli_build_refuses_foreign_non_empty_output_dir() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(temp_dir.path(), "def main():\n    /say hi\n");
    let output_dir = temp_dir.path().join("out");
    fs::create_dir_all(output_dir.join("data/otherpack/function")).unwrap();
    fs::write(
        output_dir.join("data/otherpack/function/x.mcfunction"),
        "say keep\n",
    )
    .unwrap();
    fs::write(output_dir.join("README.txt"), "keep\n").unwrap();
    // A stale namespace list without an ownership marker used to direct the
    // writer's deletions inside a directory Cobble never owned.
    fs::create_dir_all(output_dir.join(".cobble")).unwrap();
    fs::write(
        output_dir.join(".cobble/generated_namespaces.json"),
        r#"["otherpack"]"#,
    )
    .unwrap();

    let output = cobble()
        .arg("build")
        .arg(&input)
        .arg("--namespace")
        .arg("fix_cli")
        .arg("--output")
        .arg(&output_dir)
        .output()
        .unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(
        stderr.contains("Refusing to build into existing non-empty output directory"),
        "stderr: {stderr}"
    );
    assert!(output_dir
        .join("data/otherpack/function/x.mcfunction")
        .exists());
    assert!(output_dir.join("README.txt").exists());
}

#[test]
fn cli_build_allows_new_empty_and_owned_output_dirs() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(temp_dir.path(), "def main():\n    /say rebuild\n");

    // A new (not yet existing) directory is created.
    let new_dir = temp_dir.path().join("new_out");
    for _ in 0..2 {
        // The second build re-enters the now marked, non-empty directory.
        let output = cobble()
            .arg("build")
            .arg(&input)
            .arg("--namespace")
            .arg("fix_cli")
            .arg("--output")
            .arg(&new_dir)
            .output()
            .unwrap();
        let (stdout, stderr) = output_text(&output);
        assert!(
            output.status.success(),
            "build into own output failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
    }
    assert!(new_dir.join(".cobble/build_manifest.json").exists());

    // A pre-existing but truly empty directory is also accepted.
    let empty_dir = temp_dir.path().join("empty_out");
    fs::create_dir_all(&empty_dir).unwrap();
    let output = cobble()
        .arg("build")
        .arg(&input)
        .arg("--namespace")
        .arg("fix_cli")
        .arg("--output")
        .arg(&empty_dir)
        .output()
        .unwrap();
    let (stdout, stderr) = output_text(&output);
    assert!(
        output.status.success(),
        "build into empty output failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(empty_dir.join("pack.mcmeta").exists());
}

#[test]
fn cli_build_refuses_output_owned_by_another_project() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let project_a = temp_dir.path().join("a");
    let project_b = temp_dir.path().join("b");
    fs::create_dir_all(&project_a).unwrap();
    fs::create_dir_all(&project_b).unwrap();
    let input_a = write_source(&project_a, "def main():\n    /say a\n");
    let input_b = write_source(&project_b, "def main():\n    /say b\n");
    let output_dir = temp_dir.path().join("shared_out");

    let build_a = cobble()
        .arg("build")
        .arg(&input_a)
        .arg("--namespace")
        .arg("fix_cli")
        .arg("--output")
        .arg(&output_dir)
        .output()
        .unwrap();
    assert!(build_a.status.success());

    let build_b = cobble()
        .arg("build")
        .arg(&input_b)
        .arg("--namespace")
        .arg("fix_cli")
        .arg("--output")
        .arg(&output_dir)
        .output()
        .unwrap();

    let (_stdout, stderr) = output_text(&build_b);
    assert!(!build_b.status.success());
    assert!(
        stderr.contains("Refusing to build into output directory owned by another Cobble project"),
        "stderr: {stderr}"
    );
    // Project A's generated function is untouched.
    assert!(output_dir
        .join("data/fix_cli/function/main.mcfunction")
        .exists());
}

#[test]
fn cli_clean_output_refuses_other_projects_output() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let project_a = temp_dir.path().join("proj_a");
    let project_b = temp_dir.path().join("proj_b");
    init_project(&project_a);
    init_project(&project_b);
    build_project(&project_a);
    let a_output = project_a.join("output");
    assert!(a_output.join(".cobble/build_manifest.json").exists());

    // From inside project B, `clean --output` must refuse project A's output.
    let refused = cobble()
        .current_dir(&project_b)
        .arg("clean")
        .arg("--output")
        .arg(&a_output)
        .output()
        .unwrap();
    let (_stdout, stderr) = output_text(&refused);
    assert!(!refused.status.success());
    assert!(
        stderr.contains("Refusing to clean") && stderr.contains("does not match this project"),
        "stderr: {stderr}"
    );
    assert!(a_output.exists());

    // From inside project A the same invocation still works.
    let allowed = cobble()
        .current_dir(&project_a)
        .arg("clean")
        .arg("--output")
        .arg(&a_output)
        .output()
        .unwrap();
    let (stdout, stderr) = output_text(&allowed);
    assert!(
        allowed.status.success(),
        "clean --output in own project failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(!a_output.exists());
}

#[test]
fn cli_link_world_requires_existing_world_directory() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let project_dir = temp_dir.path().join("proj");
    init_project(&project_dir);
    let missing_world = temp_dir.path().join("nonexistent_world");

    let refused = cobble()
        .current_dir(&project_dir)
        .arg("link")
        .arg("--world")
        .arg(&missing_world)
        .output()
        .unwrap();
    let (_stdout, stderr) = output_text(&refused);
    assert!(!refused.status.success());
    assert!(
        stderr.contains("World directory does not exist"),
        "stderr: {stderr}"
    );
    assert!(
        !missing_world.exists(),
        "a typo'd --world path must not be created"
    );

    // Once the world directory exists, the datapacks/ subdirectory is created.
    let world_dir = temp_dir.path().join("real_world");
    fs::create_dir_all(&world_dir).unwrap();
    let linked = cobble()
        .current_dir(&project_dir)
        .arg("link")
        .arg("--world")
        .arg(&world_dir)
        .output()
        .unwrap();
    let (stdout, stderr) = output_text(&linked);
    assert!(
        linked.status.success(),
        "link --world failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(world_dir.join("datapacks").is_dir());
}

#[test]
fn cli_init_invalid_pack_format_leaves_no_directory() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let project_dir = temp_dir.path().join("badfmt");

    let output = cobble()
        .arg("init")
        .arg("--name")
        .arg(&project_dir)
        .arg("--pack-format")
        .arg("82")
        .output()
        .unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("pack_format must be"), "stderr: {stderr}");
    assert!(
        !project_dir.exists(),
        "failed init must not leave an empty project directory behind"
    );
}

#[test]
fn cli_build_compiles_overlapping_entry_points_once() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let project_dir = temp_dir.path().join("dedup");
    fs::create_dir_all(project_dir.join("src")).unwrap();
    fs::write(
        project_dir.join("cobble.toml"),
        r#"
[project]
name = "fix_dedup"
description = "Entry dedup fixture"
namespace = "fix_dedup"
version = "1.0.0"
pack_format = "101.1"

[build]
source = "src"
output = "output"
entry_points = ["main.cbl", "."]
"#,
    )
    .unwrap();
    fs::write(
        project_dir.join("src/main.cbl"),
        "def main():\n    /say dedup once\n",
    )
    .unwrap();

    let output = cobble()
        .current_dir(&project_dir)
        .arg("build")
        .output()
        .unwrap();

    let (stdout, stderr) = output_text(&output);
    assert!(
        output.status.success(),
        "build failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("Building 1 file(s)"),
        "overlapping entry points must be compiled once\nstdout:\n{stdout}"
    );
    let main_function =
        fs::read_to_string(project_dir.join("output/data/fix_dedup/function/main.mcfunction"))
            .unwrap();
    assert_eq!(
        main_function.matches("say dedup once").count(),
        1,
        "duplicate entry points must not duplicate commands:\n{main_function}"
    );
}

#[test]
fn cli_migrate_treats_equivalent_pack_format_as_not_needed() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let project_dir = temp_dir.path().join("migrate_fmt");
    fs::create_dir_all(project_dir.join("src")).unwrap();
    fs::write(
        project_dir.join("cobble.toml"),
        r#"
[project]
name = "migrate_fmt"
description = "Migration formatting fixture"
namespace = "migrate_fmt"
version = "1.0.0"
pack_format = "101.01"

[build]
source = "src"
output = "output"
entry_points = []
"#,
    )
    .unwrap();
    fs::write(project_dir.join("src/main.cbl"), "def main():\n    pass\n").unwrap();

    let output = cobble()
        .arg("migrate")
        .arg(&project_dir)
        .arg("--json")
        .output()
        .unwrap();

    let (stdout, stderr) = output_text(&output);
    assert!(
        output.status.success(),
        "migrate --json failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    let value: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["changed"], false);
    // "101.01" parses to the same pack format as "101.1", so no update is
    // proposed even though the raw strings differ.
    assert!(
        value["config"]["changes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|change| change["field"] == "project.pack_format"
                && change["status"] == "not_needed"),
        "stdout:\n{stdout}"
    );
}

#[test]
fn cli_check_json_reports_runtime_for_kind() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        "def main():\n    x = \"abc\"\n    for v in x:\n        /say hi\n",
    );

    let output = cobble()
        .arg("check")
        .arg("--json")
        .arg(&input)
        .output()
        .unwrap();

    let (stdout, _stderr) = output_text(&output);
    assert!(!output.status.success());
    let value: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let diagnostics = value["diagnostics"].as_array().unwrap();
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic["kind"] == "runtime-for-unsupported"),
        "runtime-for errors must keep their documented kind\nstdout:\n{stdout}"
    );
}
