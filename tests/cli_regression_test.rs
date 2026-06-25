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

fn write_say_commands_json(temp_dir: &Path) -> PathBuf {
    let commands_json = temp_dir.join("commands-fixture.json");
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
    commands_json
}

fn output_text(output: &Output) -> (String, String) {
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn cli_doctor_reports_project_and_command_tree_status() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let commands_json = write_say_commands_json(temp_dir.path());
    fs::create_dir_all(temp_dir.path().join("src")).unwrap();
    fs::write(
        temp_dir.path().join("cobble.toml"),
        r#"
[project]
name = "doctor_project"
description = "Doctor regression project"
namespace = "doctor_project"
version = "1.0.0"
pack_format = "101.1"

[build]
source = "src"
output = "output"
entry_points = []
"#,
    )
    .unwrap();

    let output = cobble()
        .arg("doctor")
        .arg(temp_dir.path())
        .arg("--commands-json")
        .arg(&commands_json)
        .output()
        .unwrap();

    let (stdout, stderr) = output_text(&output);
    assert!(
        output.status.success(),
        "doctor failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(stdout.contains("Cobble doctor"));
    assert!(stdout.contains("Project: doctor_project"));
    assert!(stdout.contains("Namespace: doctor_project"));
    assert!(stdout.contains("Command tree:"));
    assert!(stdout.contains(commands_json.to_string_lossy().as_ref()));
    assert!(stdout.contains("SHA-1:"));
}

#[test]
fn cli_doctor_reports_missing_command_tree_without_download() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let missing_commands = temp_dir.path().join("missing-commands.json");

    let output = cobble()
        .arg("doctor")
        .arg(temp_dir.path())
        .arg("--commands-json")
        .arg(&missing_commands)
        .output()
        .unwrap();

    let (stdout, stderr) = output_text(&output);
    assert!(
        output.status.success(),
        "doctor failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(stdout.contains("Command tree: missing"));
    assert!(stdout.contains(missing_commands.to_string_lossy().as_ref()));
}

#[test]
fn cli_doctor_json_reports_stable_core_shape() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let commands_json = write_say_commands_json(temp_dir.path());
    fs::create_dir_all(temp_dir.path().join("src")).unwrap();
    fs::write(
        temp_dir.path().join("cobble.toml"),
        r#"
[project]
name = "doctor_json_project"
description = "Doctor JSON regression project"
namespace = "doctor_json_project"
version = "1.0.0"
pack_format = "101.1"

[build]
source = "src"
output = "output"
entry_points = []
"#,
    )
    .unwrap();

    let output = cobble()
        .arg("doctor")
        .arg("--json")
        .arg(temp_dir.path())
        .arg("--commands-json")
        .arg(&commands_json)
        .output()
        .unwrap();

    let (stdout, stderr) = output_text(&output);
    assert!(
        output.status.success(),
        "doctor --json failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stderr.is_empty(),
        "doctor --json should keep stderr quiet on success: {stderr}"
    );

    let value: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["schema_version"], 1);
    assert!(matches!(
        value["status"].as_str(),
        Some("ok" | "warning" | "error")
    ));
    assert_eq!(value["cobble"]["pack_format"], "101.1");
    assert_eq!(value["config"]["id"], "config");
    assert_eq!(value["config"]["status"], "ok");
    assert_eq!(
        value["config"]["project"]["namespace"],
        "doctor_json_project"
    );
    assert_eq!(value["commands_json"]["id"], "commands_json");
    assert_eq!(value["commands_json"]["status"], "ok");
    assert_eq!(
        value["commands_json"]["path"],
        commands_json.to_string_lossy().as_ref()
    );
    assert_eq!(value["experimental_output"]["id"], "output");
    assert_eq!(value["experimental_output"]["status"], "not_present");
    assert_eq!(value["experimental_output"]["configured"], true);
    assert_eq!(value["experimental_output"]["exists"], false);
    assert_eq!(value["experimental_link"]["id"], "link");
    assert_eq!(value["experimental_link"]["status"], "not_configured");
    assert_eq!(value["experimental_link"]["configured"], false);
    assert!(value["commands_json"]["sha1"].as_str().is_some());
    assert!(value["tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|tool| { tool["id"] == "tool.java" && tool["status"].as_str().is_some() }));
}

#[test]
fn cli_doctor_json_reports_configured_output_marker_status() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let project_dir = temp_dir.path().join("doctor_output_project");
    let commands_json = write_say_commands_json(temp_dir.path());

    let init = cobble()
        .arg("init")
        .arg("--name")
        .arg(&project_dir)
        .arg("--template")
        .arg("minimal")
        .output()
        .unwrap();
    let (init_stdout, init_stderr) = output_text(&init);
    assert!(
        init.status.success(),
        "init failed\nstdout:\n{init_stdout}\nstderr:\n{init_stderr}"
    );

    let build = cobble().arg("build").arg(&project_dir).output().unwrap();
    let (build_stdout, build_stderr) = output_text(&build);
    assert!(
        build.status.success(),
        "build failed\nstdout:\n{build_stdout}\nstderr:\n{build_stderr}"
    );

    let doctor = cobble()
        .arg("doctor")
        .arg("--json")
        .arg(&project_dir)
        .arg("--commands-json")
        .arg(&commands_json)
        .output()
        .unwrap();
    let (doctor_stdout, doctor_stderr) = output_text(&doctor);
    assert!(
        doctor.status.success(),
        "doctor --json failed\nstdout:\n{doctor_stdout}\nstderr:\n{doctor_stderr}"
    );

    let value: serde_json::Value = serde_json::from_str(&doctor_stdout).unwrap();
    assert_eq!(value["experimental_output"]["id"], "output");
    assert_eq!(value["experimental_output"]["status"], "ok");
    assert_eq!(value["experimental_output"]["configured"], true);
    assert_eq!(value["experimental_output"]["exists"], true);
    assert_eq!(value["experimental_output"]["marker"]["present"], true);
    assert_eq!(
        value["experimental_output"]["marker"]["namespace"],
        "doctor_output_project"
    );
    assert_eq!(
        value["experimental_output"]["path"],
        project_dir.join("output").to_string_lossy().as_ref()
    );
}

#[test]
fn cli_migrate_defaults_to_experimental_dry_run_report() {
    let output = cobble().arg("migrate").output().unwrap();

    let (stdout, stderr) = output_text(&output);
    assert!(
        output.status.success(),
        "migrate failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stderr.is_empty(),
        "migrate should keep stderr quiet on success: {stderr}"
    );
    assert!(stdout.contains("Cobble migrate (experimental)"));
    assert!(stdout.contains("From: 0.8"));
    assert!(stdout.contains("To: 0.9"));
    assert!(stdout.contains("Mode: dry-run/report"));
    assert!(stdout.contains("Changed: false"));
    assert!(stdout.contains("Source files scanned:"));
    assert!(stdout.contains("File modifications require --apply"));
    assert!(stdout.contains("No files were changed."));
}

#[test]
fn cli_migrate_json_reports_project_scan_for_configured_source() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let project_dir = temp_dir.path().join("migrate_project");
    let source_dir = project_dir.join("cobble_src");
    fs::create_dir_all(source_dir.join("nested")).unwrap();
    fs::write(
        project_dir.join("cobble.toml"),
        r#"
[project]
name = "migrate_project"
description = "Migration fixture"
namespace = "migrate_project"
version = "1.0.0"
pack_format = "101.1"

[build]
source = "cobble_src"
output = "output"
entry_points = []

[stdlib]
version = 1

[experimental]
resource_pack = true
python_compat = true
"#,
    )
    .unwrap();
    fs::write(source_dir.join("z.cbl"), "import stdlib\n").unwrap();
    fs::write(
        source_dir.join("nested").join("a.cobble"),
        "from stdlib import resource_pack\nresource_pack.lang(\"en_us\", {\"item.test\": \"Test\"})\n",
    )
    .unwrap();

    let output = cobble()
        .arg("migrate")
        .arg(&project_dir)
        .arg("--from")
        .arg("0.8")
        .arg("--to")
        .arg("0.9")
        .arg("--json")
        .output()
        .unwrap();

    let (stdout, stderr) = output_text(&output);
    assert!(
        output.status.success(),
        "migrate --json failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stderr.is_empty(),
        "migrate --json should keep stderr quiet on success: {stderr}"
    );

    let value: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["ok"], true);
    assert_eq!(value["changed"], false);
    assert_eq!(value["from"], "0.8");
    assert_eq!(value["to"], "0.9");
    assert_eq!(value["apply"], false);
    assert_eq!(
        value["project_path"],
        project_dir.to_string_lossy().as_ref()
    );
    assert_eq!(value["config"]["status"], "found");
    assert_eq!(
        value["config"]["path"],
        project_dir.join("cobble.toml").to_string_lossy().as_ref()
    );
    assert_eq!(value["config"]["source"], "cobble_src");
    assert_eq!(value["config"]["stdlib_version"], 1);
    assert_eq!(value["config"]["experimental_resource_pack"], true);
    assert_eq!(value["config"]["experimental_python_compat"], true);
    assert_eq!(value["source"]["status"], "scanned");
    assert_eq!(
        value["source"]["path"],
        source_dir.to_string_lossy().as_ref()
    );
    assert_eq!(value["source"]["files_scanned"], 2);
    assert_eq!(
        value["source"]["files"],
        serde_json::json!(["nested/a.cobble", "z.cbl"])
    );
    assert_eq!(value["source"]["resource_pack_references"], 1);
    assert_eq!(value["source"]["legacy_stdlib_import_files"], 1);
    assert_eq!(value["source"]["stdlib_module_import_files"], 1);
    assert!(value["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|diagnostic| diagnostic["code"] == "experimental_migration_dry_run"));
    assert!(value["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|diagnostic| diagnostic["code"] == "config_found"));
    assert!(value["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|diagnostic| diagnostic["code"] == "source_scan_completed"));
    assert!(value["actions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|action| action["id"] == "scan_sources" && action["status"] == "scanned"));
    assert!(value["actions"].as_array().unwrap().iter().any(|action| {
        action["id"] == "candidate_resource_pack_config" && action["status"] == "configured"
    }));
}

#[test]
fn cli_migrate_json_scans_src_when_config_is_missing() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let source_dir = temp_dir.path().join("src");
    fs::create_dir_all(&source_dir).unwrap();
    fs::write(source_dir.join("main.cbl"), "def main():\n    /say ok\n").unwrap();

    let output = cobble()
        .arg("migrate")
        .arg("--json")
        .arg(temp_dir.path())
        .output()
        .unwrap();

    let (stdout, stderr) = output_text(&output);
    assert!(
        output.status.success(),
        "migrate --json failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stderr.is_empty(),
        "migrate --json should keep stderr quiet on success: {stderr}"
    );

    let value: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["ok"], true);
    assert_eq!(value["changed"], false);
    assert_eq!(value["config"]["status"], "missing");
    assert_eq!(value["config"]["source"], "src");
    assert_eq!(value["config"]["stdlib_version"], 2);
    assert_eq!(value["config"]["experimental_resource_pack"], false);
    assert_eq!(value["config"]["experimental_python_compat"], false);
    assert_eq!(value["source"]["status"], "scanned");
    assert_eq!(value["source"]["files_scanned"], 1);
    assert_eq!(value["source"]["files"], serde_json::json!(["main.cbl"]));
}

#[test]
fn cli_migrate_json_reports_malformed_config_values_as_errors() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let project_dir = temp_dir.path().join("migrate_bad_config");
    fs::create_dir_all(&project_dir).unwrap();
    fs::write(
        project_dir.join("cobble.toml"),
        r#"
[project]
name = "bad"
description = "Bad"
namespace = "bad"
version = "1.0.0"
pack_format = "101.1"

[build]
source = 123
"#,
    )
    .unwrap();

    let output = cobble()
        .arg("migrate")
        .arg(&project_dir)
        .arg("--json")
        .output()
        .unwrap();

    let (stdout, stderr) = output_text(&output);
    assert!(
        !output.status.success(),
        "migrate should fail on malformed config values\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(stderr.contains("Migration inspection failed"));

    let value: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["ok"], false);
    assert_eq!(value["changed"], false);
    assert_eq!(value["config"]["status"], "error");
    assert!(value["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|diagnostic| {
            diagnostic["code"] == "config_parse_failed"
                && diagnostic["message"]
                    .as_str()
                    .unwrap()
                    .contains("invalid type")
        }));
}

#[test]
fn cli_migrate_json_reports_unknown_config_keys_as_errors() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let project_dir = temp_dir.path().join("migrate_unknown_config");
    fs::create_dir_all(project_dir.join("src")).unwrap();
    fs::write(
        project_dir.join("src/main.cbl"),
        "def main():\n    /say ok\n",
    )
    .unwrap();
    fs::write(
        project_dir.join("cobble.toml"),
        r#"
[project]
name = "bad"
description = "Bad"
namespace = "bad"
version = "1.0.0"
pack_format = "101.1"

[build]
source = "src"
output = "output"

[experimental]
resource_packs = true
"#,
    )
    .unwrap();

    let output = cobble()
        .arg("migrate")
        .arg(&project_dir)
        .arg("--json")
        .output()
        .unwrap();

    let (stdout, stderr) = output_text(&output);
    assert!(
        !output.status.success(),
        "migrate should fail on unknown config keys\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(stderr.contains("Migration inspection failed"));

    let value: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["ok"], false);
    assert_eq!(value["config"]["status"], "error");
    assert!(value["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|diagnostic| {
            diagnostic["code"] == "config_parse_failed"
                && diagnostic["message"]
                    .as_str()
                    .unwrap()
                    .contains("unknown field `resource_packs`")
        }));
}

#[test]
fn cli_migrate_apply_skeleton_does_not_modify_files() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let sentinel = temp_dir.path().join("sentinel.txt");
    fs::write(&sentinel, "keep me").unwrap();

    let output = cobble()
        .arg("migrate")
        .arg("--apply")
        .current_dir(temp_dir.path())
        .output()
        .unwrap();

    let (stdout, stderr) = output_text(&output);
    assert!(
        output.status.success(),
        "migrate --apply failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stderr.is_empty(),
        "migrate --apply should keep stderr quiet on success: {stderr}"
    );
    assert!(stdout.contains("Mode: apply requested"));
    assert!(stdout.contains("no automatic rewrites yet"));
    assert!(stdout.contains("No files were changed."));
    assert_eq!(fs::read_to_string(&sentinel).unwrap(), "keep me");
}

#[test]
fn cli_migrate_json_reports_unsupported_routes_without_changes() {
    let output = cobble()
        .arg("migrate")
        .arg("--from")
        .arg("0.7")
        .arg("--to")
        .arg("0.9")
        .arg("--json")
        .output()
        .unwrap();

    let (stdout, stderr) = output_text(&output);
    assert!(
        !output.status.success(),
        "unsupported migrate route should fail\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(stderr.contains("Unsupported experimental migration route"));

    let value: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["ok"], false);
    assert_eq!(value["changed"], false);
    assert_eq!(value["from"], "0.7");
    assert_eq!(value["to"], "0.9");
    assert_eq!(value["config"]["status"], "skipped");
    assert_eq!(value["source"]["status"], "skipped");
    assert!(value["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|diagnostic| diagnostic["code"] == "unsupported_migration_route"));
}

#[test]
fn cli_doctor_json_reports_link_status_and_marker() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let project_dir = temp_dir.path().join("linked_pack");
    let datapacks_dir = temp_dir.path().join("world").join("datapacks");
    let pack_dir = datapacks_dir.join("linked_pack");
    let commands_json = write_say_commands_json(temp_dir.path());

    let init = cobble()
        .arg("init")
        .arg("--name")
        .arg(&project_dir)
        .arg("--template")
        .arg("minimal")
        .output()
        .unwrap();
    let (init_stdout, init_stderr) = output_text(&init);
    assert!(
        init.status.success(),
        "init failed\nstdout:\n{init_stdout}\nstderr:\n{init_stderr}"
    );

    let link = cobble()
        .arg("link")
        .arg(&project_dir)
        .arg("--datapacks")
        .arg(&datapacks_dir)
        .output()
        .unwrap();
    let (link_stdout, link_stderr) = output_text(&link);
    assert!(
        link.status.success(),
        "link failed\nstdout:\n{link_stdout}\nstderr:\n{link_stderr}"
    );

    let missing_marker = cobble()
        .arg("doctor")
        .arg("--json")
        .arg(&project_dir)
        .arg("--commands-json")
        .arg(&commands_json)
        .output()
        .unwrap();
    let (missing_stdout, missing_stderr) = output_text(&missing_marker);
    assert!(
        missing_marker.status.success(),
        "doctor --json failed\nstdout:\n{missing_stdout}\nstderr:\n{missing_stderr}"
    );
    let missing: serde_json::Value = serde_json::from_str(&missing_stdout).unwrap();
    assert_eq!(missing["experimental_link"]["id"], "link");
    assert_eq!(missing["experimental_link"]["status"], "warning");
    assert_eq!(missing["experimental_link"]["configured"], true);
    assert_eq!(missing["experimental_link"]["target_kind"], "datapacks");
    assert_eq!(missing["experimental_link"]["marker"]["present"], false);

    let build = cobble()
        .arg("build")
        .arg(project_dir.join("src"))
        .arg("--output")
        .arg(&pack_dir)
        .output()
        .unwrap();
    let (build_stdout, build_stderr) = output_text(&build);
    assert!(
        build.status.success(),
        "build failed\nstdout:\n{build_stdout}\nstderr:\n{build_stderr}"
    );

    let present_marker = cobble()
        .arg("doctor")
        .arg("--json")
        .arg(&project_dir)
        .arg("--commands-json")
        .arg(&commands_json)
        .output()
        .unwrap();
    let (present_stdout, present_stderr) = output_text(&present_marker);
    assert!(
        present_marker.status.success(),
        "doctor --json failed\nstdout:\n{present_stdout}\nstderr:\n{present_stderr}"
    );
    let present: serde_json::Value = serde_json::from_str(&present_stdout).unwrap();
    assert_eq!(present["experimental_link"]["status"], "ok");
    assert_eq!(present["experimental_link"]["marker"]["present"], true);
    assert_eq!(
        present["experimental_link"]["marker"]["path"],
        pack_dir
            .join(".cobble/build_manifest.json")
            .to_string_lossy()
            .as_ref()
    );
}

#[cfg(unix)]
#[test]
fn cli_doctor_json_rejects_symlinked_link_marker_parent() {
    use std::os::unix::fs::symlink;

    let temp_dir = tempfile::TempDir::new().unwrap();
    let project_dir = temp_dir.path().join("linked_pack");
    let datapacks_dir = temp_dir.path().join("world").join("datapacks");
    let pack_dir = datapacks_dir.join("linked_pack");
    let outside_marker_dir = temp_dir.path().join("outside-marker");
    let commands_json = write_say_commands_json(temp_dir.path());

    let init = cobble()
        .arg("init")
        .arg("--name")
        .arg(&project_dir)
        .arg("--template")
        .arg("minimal")
        .output()
        .unwrap();
    let (init_stdout, init_stderr) = output_text(&init);
    assert!(
        init.status.success(),
        "init failed\nstdout:\n{init_stdout}\nstderr:\n{init_stderr}"
    );

    let link = cobble()
        .arg("link")
        .arg(&project_dir)
        .arg("--datapacks")
        .arg(&datapacks_dir)
        .output()
        .unwrap();
    let (link_stdout, link_stderr) = output_text(&link);
    assert!(
        link.status.success(),
        "link failed\nstdout:\n{link_stdout}\nstderr:\n{link_stderr}"
    );

    fs::create_dir_all(pack_dir.join("data/linked_pack/function")).unwrap();
    fs::create_dir_all(&outside_marker_dir).unwrap();
    fs::write(pack_dir.join("pack.mcmeta"), "{}").unwrap();
    fs::write(
        outside_marker_dir.join("build_manifest.json"),
        r#"{
  "version": 1,
  "cobble_version": "0.7.3",
  "namespace": "linked_pack",
  "project_id": "not-this-project"
}"#,
    )
    .unwrap();
    symlink(&outside_marker_dir, pack_dir.join(".cobble")).unwrap();

    let output = cobble()
        .arg("doctor")
        .arg("--json")
        .arg(&project_dir)
        .arg("--commands-json")
        .arg(&commands_json)
        .output()
        .unwrap();

    let (stdout, stderr) = output_text(&output);
    assert!(
        output.status.success(),
        "doctor --json failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    let value: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["status"], "error");
    assert_eq!(value["experimental_link"]["marker"]["status"], "error");
    assert!(value["experimental_link"]["marker"]["message"]
        .as_str()
        .unwrap()
        .contains("Refusing to inspect linked output marker through symlink"));
}

#[test]
fn cli_init_lists_templates_without_creating_project_files() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let project_dir = temp_dir.path().join("listed_pack");

    let output = cobble()
        .arg("init")
        .arg("--list-templates")
        .arg("--name")
        .arg(&project_dir)
        .output()
        .unwrap();

    let (stdout, stderr) = output_text(&output);
    assert!(
        output.status.success(),
        "init --list-templates failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(stdout.contains("Available templates"));
    assert!(stdout.contains("minimal"));
    assert!(stdout.contains("stdlib"));
    assert!(stdout.contains("(default)"));
    assert!(stdout.contains("validation"));
    assert!(stdout.contains("resource-heavy"));
    assert!(stdout.contains("game-mechanic"));
    assert!(stdout.contains("web-demo"));
    assert!(!project_dir.exists());
}

#[cfg(unix)]
#[test]
fn cli_init_refuses_symlinked_source_directory_without_writing_outside() {
    use std::os::unix::fs::symlink;

    let temp_dir = tempfile::TempDir::new().unwrap();
    let outside_dir = temp_dir.path().join("outside-src");
    fs::create_dir_all(&outside_dir).unwrap();
    symlink(&outside_dir, temp_dir.path().join("src")).unwrap();

    let output = cobble()
        .arg("init")
        .current_dir(temp_dir.path())
        .output()
        .unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("Refusing to initialize project through symlink"));
    assert!(!outside_dir.join("main.cbl").exists());
    assert!(!temp_dir.path().join("cobble.toml").exists());
}

#[test]
fn cli_clean_dry_run_reports_marked_output_without_deleting() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(temp_dir.path(), "def main():\n    /say clean dry run\n");
    let output_dir = temp_dir.path().join("output");

    let build = cobble()
        .arg("build")
        .arg(&input)
        .arg("--namespace")
        .arg("clean_dry_run")
        .arg("--output")
        .arg(&output_dir)
        .output()
        .unwrap();
    let (build_stdout, build_stderr) = output_text(&build);
    assert!(
        build.status.success(),
        "build failed\nstdout:\n{build_stdout}\nstderr:\n{build_stderr}"
    );

    let output = cobble()
        .arg("clean")
        .arg("--dry-run")
        .arg("--output")
        .arg(&output_dir)
        .output()
        .unwrap();

    let (stdout, stderr) = output_text(&output);
    assert!(
        output.status.success(),
        "clean --dry-run failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(stdout.contains("Would remove Cobble output"));
    assert!(stdout.contains("Safety checks:"));
    assert!(stdout.contains(".cobble/build_manifest.json"));
    assert!(stdout.contains("Namespace: clean_dry_run"));
    assert!(stdout.contains("Required files: pack.mcmeta, data/"));
    assert!(stdout.contains("Symlinks: none found"));
    assert!(stdout.contains("Next step: rerun without --dry-run"));
    assert!(stdout.contains("clean_dry_run"));
    assert!(output_dir.exists());
}

#[test]
fn cli_clean_removes_marked_output() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(temp_dir.path(), "def main():\n    /say clean\n");
    let output_dir = temp_dir.path().join("output");

    let build = cobble()
        .arg("build")
        .arg(&input)
        .arg("--namespace")
        .arg("clean_remove")
        .arg("--output")
        .arg(&output_dir)
        .output()
        .unwrap();
    let (build_stdout, build_stderr) = output_text(&build);
    assert!(
        build.status.success(),
        "build failed\nstdout:\n{build_stdout}\nstderr:\n{build_stderr}"
    );

    let output = cobble()
        .arg("clean")
        .arg("--output")
        .arg(&output_dir)
        .output()
        .unwrap();

    let (stdout, stderr) = output_text(&output);
    assert!(
        output.status.success(),
        "clean failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(stdout.contains("Removed Cobble output"));
    assert!(!output_dir.exists());
}

#[test]
fn cli_clean_refuses_unmarked_output() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let output_dir = temp_dir.path().join("output");
    fs::create_dir_all(&output_dir).unwrap();
    fs::write(output_dir.join("important.txt"), "keep me").unwrap();

    let output = cobble()
        .arg("clean")
        .arg("--output")
        .arg(&output_dir)
        .output()
        .unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("Refusing to clean"));
    assert!(output_dir.join("important.txt").exists());
}

#[test]
fn cli_clean_linked_requires_confirmation_and_removes_marked_linked_output() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let project_dir = temp_dir.path().join("linked_pack");
    let datapacks_dir = temp_dir.path().join("world").join("datapacks");
    let pack_dir = datapacks_dir.join("linked_pack");

    let init = cobble()
        .arg("init")
        .arg("--name")
        .arg(&project_dir)
        .arg("--template")
        .arg("minimal")
        .output()
        .unwrap();
    let (init_stdout, init_stderr) = output_text(&init);
    assert!(
        init.status.success(),
        "init failed\nstdout:\n{init_stdout}\nstderr:\n{init_stderr}"
    );

    let link = cobble()
        .arg("link")
        .arg(&project_dir)
        .arg("--datapacks")
        .arg(&datapacks_dir)
        .output()
        .unwrap();
    let (link_stdout, link_stderr) = output_text(&link);
    assert!(
        link.status.success(),
        "link failed\nstdout:\n{link_stdout}\nstderr:\n{link_stderr}"
    );

    let build = cobble()
        .arg("build")
        .arg(project_dir.join("src"))
        .arg("--output")
        .arg(&pack_dir)
        .output()
        .unwrap();
    let (build_stdout, build_stderr) = output_text(&build);
    assert!(
        build.status.success(),
        "build failed\nstdout:\n{build_stdout}\nstderr:\n{build_stderr}"
    );
    assert!(pack_dir.join(".cobble/build_manifest.json").exists());

    let dry_run = cobble()
        .arg("clean")
        .arg(&project_dir)
        .arg("--linked")
        .arg("--dry-run")
        .output()
        .unwrap();
    let (dry_stdout, dry_stderr) = output_text(&dry_run);
    assert!(
        dry_run.status.success(),
        "clean --linked --dry-run failed\nstdout:\n{dry_stdout}\nstderr:\n{dry_stderr}"
    );
    assert!(dry_stdout.contains("Would remove Cobble output"));
    assert!(dry_stdout.contains("Safety checks:"));
    assert!(dry_stdout.contains("Namespace: linked_pack"));
    assert!(dry_stdout.contains("Project id:"));
    assert!(dry_stdout.contains("Next step: run `cobble clean --linked --yes`"));
    assert!(pack_dir.exists());

    let unconfirmed = cobble()
        .arg("clean")
        .arg(&project_dir)
        .arg("--linked")
        .output()
        .unwrap();
    let (_stdout, stderr) = output_text(&unconfirmed);
    assert!(!unconfirmed.status.success());
    assert!(stderr.contains("requires --yes"));
    assert!(pack_dir.exists());

    let confirmed = cobble()
        .arg("clean")
        .arg(&project_dir)
        .arg("--linked")
        .arg("--yes")
        .output()
        .unwrap();
    let (confirmed_stdout, confirmed_stderr) = output_text(&confirmed);
    assert!(
        confirmed.status.success(),
        "clean --linked --yes failed\nstdout:\n{confirmed_stdout}\nstderr:\n{confirmed_stderr}"
    );
    assert!(confirmed_stdout.contains("Removed Cobble output"));
    assert!(!pack_dir.exists());
}

#[test]
fn cli_link_dry_run_does_not_write_link_state() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let project_dir = temp_dir.path().join("linked_pack");
    let datapacks_dir = temp_dir.path().join("world").join("datapacks");

    let init = cobble()
        .arg("init")
        .arg("--name")
        .arg(&project_dir)
        .arg("--template")
        .arg("minimal")
        .output()
        .unwrap();
    let (init_stdout, init_stderr) = output_text(&init);
    assert!(
        init.status.success(),
        "init failed\nstdout:\n{init_stdout}\nstderr:\n{init_stderr}"
    );

    let output = cobble()
        .arg("link")
        .arg(&project_dir)
        .arg("--datapacks")
        .arg(&datapacks_dir)
        .arg("--dry-run")
        .output()
        .unwrap();

    let (stdout, stderr) = output_text(&output);
    assert!(
        output.status.success(),
        "link --dry-run failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(stdout.contains("Would configure Cobble link"));
    assert!(stdout.contains(datapacks_dir.to_string_lossy().as_ref()));
    assert!(!project_dir.join(".cobble/link_state.json").exists());
    assert!(!datapacks_dir.exists());
}

#[test]
fn cli_link_configures_status_and_clear() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let project_dir = temp_dir.path().join("linked_pack");
    let datapacks_dir = temp_dir.path().join("world").join("datapacks");

    let init = cobble()
        .arg("init")
        .arg("--name")
        .arg(&project_dir)
        .arg("--template")
        .arg("minimal")
        .output()
        .unwrap();
    let (init_stdout, init_stderr) = output_text(&init);
    assert!(
        init.status.success(),
        "init failed\nstdout:\n{init_stdout}\nstderr:\n{init_stderr}"
    );

    let link = cobble()
        .arg("link")
        .arg(&project_dir)
        .arg("--datapacks")
        .arg(&datapacks_dir)
        .output()
        .unwrap();
    let (link_stdout, link_stderr) = output_text(&link);
    assert!(
        link.status.success(),
        "link failed\nstdout:\n{link_stdout}\nstderr:\n{link_stderr}"
    );
    assert!(datapacks_dir.exists());
    assert!(project_dir.join(".cobble/link_state.json").exists());

    let status = cobble()
        .arg("link")
        .arg(&project_dir)
        .arg("--status")
        .output()
        .unwrap();
    let (status_stdout, status_stderr) = output_text(&status);
    assert!(
        status.status.success(),
        "link --status failed\nstdout:\n{status_stdout}\nstderr:\n{status_stderr}"
    );
    assert!(status_stdout.contains("Cobble link configured"));
    assert!(status_stdout.contains(datapacks_dir.to_string_lossy().as_ref()));
    assert!(status_stdout.contains("Marker: missing"));
    assert!(status_stdout.contains("Recovery: run `cobble watch --link`"));

    let clear = cobble()
        .arg("link")
        .arg(&project_dir)
        .arg("--clear")
        .output()
        .unwrap();
    let (clear_stdout, clear_stderr) = output_text(&clear);
    assert!(
        clear.status.success(),
        "link --clear failed\nstdout:\n{clear_stdout}\nstderr:\n{clear_stderr}"
    );
    assert!(clear_stdout.contains("Cleared Cobble link state"));
    assert!(!project_dir.join(".cobble/link_state.json").exists());

    let status_after_clear = cobble()
        .arg("link")
        .arg(&project_dir)
        .arg("--status")
        .output()
        .unwrap();
    let (status_after_clear_stdout, status_after_clear_stderr) = output_text(&status_after_clear);
    assert!(
        status_after_clear.status.success(),
        "link --status after clear failed\nstdout:\n{status_after_clear_stdout}\nstderr:\n{status_after_clear_stderr}"
    );
    assert!(status_after_clear_stdout.contains("No Cobble link configured"));
    assert!(status_after_clear_stdout.contains("Recovery: run `cobble link --datapacks <DIR>`"));
}

#[test]
fn cli_link_world_writes_resolved_datapacks_state() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let project_dir = temp_dir.path().join("linked_pack");
    let world_dir = temp_dir.path().join("world");
    let datapacks_dir = world_dir.join("datapacks");
    let pack_dir = datapacks_dir.join("world_pack");

    let init = cobble()
        .arg("init")
        .arg("--name")
        .arg(&project_dir)
        .arg("--template")
        .arg("minimal")
        .output()
        .unwrap();
    let (init_stdout, init_stderr) = output_text(&init);
    assert!(
        init.status.success(),
        "init failed\nstdout:\n{init_stdout}\nstderr:\n{init_stderr}"
    );

    let link = cobble()
        .arg("link")
        .arg(&project_dir)
        .arg("--world")
        .arg(&world_dir)
        .arg("--pack-name")
        .arg("world_pack")
        .output()
        .unwrap();
    let (link_stdout, link_stderr) = output_text(&link);
    assert!(
        link.status.success(),
        "link --world failed\nstdout:\n{link_stdout}\nstderr:\n{link_stderr}"
    );
    assert!(datapacks_dir.exists());

    let state: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(project_dir.join(".cobble/link_state.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(state["target_kind"], "world");
    assert_eq!(
        state["target_path"],
        datapacks_dir.to_string_lossy().as_ref()
    );
    assert_eq!(state["pack_name"], "world_pack");
    assert_eq!(state["pack_path"], pack_dir.to_string_lossy().as_ref());
}

#[test]
fn cli_link_minecraft_dry_run_resolves_save_datapacks_path() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let project_dir = temp_dir.path().join("linked_pack");
    let minecraft_dir = temp_dir.path().join(".minecraft");
    let expected_datapacks = minecraft_dir
        .join("saves")
        .join("dev_pack")
        .join("datapacks");
    let expected_pack = expected_datapacks.join("dev_pack");

    let init = cobble()
        .arg("init")
        .arg("--name")
        .arg(&project_dir)
        .arg("--template")
        .arg("minimal")
        .output()
        .unwrap();
    let (init_stdout, init_stderr) = output_text(&init);
    assert!(
        init.status.success(),
        "init failed\nstdout:\n{init_stdout}\nstderr:\n{init_stderr}"
    );

    let output = cobble()
        .arg("link")
        .arg(&project_dir)
        .arg("--minecraft")
        .arg(&minecraft_dir)
        .arg("--pack-name")
        .arg("dev_pack")
        .arg("--dry-run")
        .output()
        .unwrap();
    let (stdout, stderr) = output_text(&output);
    assert!(
        output.status.success(),
        "link --minecraft --dry-run failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(stdout.contains("Target kind: minecraft"));
    assert!(stdout.contains(expected_datapacks.to_string_lossy().as_ref()));
    assert!(stdout.contains(expected_pack.to_string_lossy().as_ref()));
    assert!(!project_dir.join(".cobble/link_state.json").exists());
    assert!(!expected_datapacks.exists());
}

#[test]
fn cli_link_status_doctor_and_clean_reject_tampered_pack_path() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let project_dir = temp_dir.path().join("linked_pack");
    let datapacks_dir = temp_dir.path().join("world").join("datapacks");
    let outside_dir = temp_dir.path().join("outside").join("linked_pack");

    let init = cobble()
        .arg("init")
        .arg("--name")
        .arg(&project_dir)
        .arg("--template")
        .arg("minimal")
        .output()
        .unwrap();
    let (init_stdout, init_stderr) = output_text(&init);
    assert!(
        init.status.success(),
        "init failed\nstdout:\n{init_stdout}\nstderr:\n{init_stderr}"
    );

    let link = cobble()
        .arg("link")
        .arg(&project_dir)
        .arg("--datapacks")
        .arg(&datapacks_dir)
        .output()
        .unwrap();
    let (link_stdout, link_stderr) = output_text(&link);
    assert!(
        link.status.success(),
        "link failed\nstdout:\n{link_stdout}\nstderr:\n{link_stderr}"
    );

    fs::create_dir_all(&outside_dir).unwrap();
    fs::write(outside_dir.join("important.txt"), "keep me").unwrap();
    let state_path = project_dir.join(".cobble/link_state.json");
    let mut state: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    state["pack_path"] = serde_json::Value::String(outside_dir.display().to_string());
    fs::write(
        &state_path,
        serde_json::to_string_pretty(&state).unwrap() + "\n",
    )
    .unwrap();

    let status = cobble()
        .arg("link")
        .arg(&project_dir)
        .arg("--status")
        .output()
        .unwrap();
    let (status_stdout, status_stderr) = output_text(&status);
    assert!(
        status.status.success(),
        "link --status failed\nstdout:\n{status_stdout}\nstderr:\n{status_stderr}"
    );
    assert!(status_stdout.contains("Link state: invalid"));
    assert!(status_stdout.contains("outside target datapacks directory"));
    assert!(status_stdout.contains("Marker: not checked"));
    assert!(status_stdout.contains("Recovery: run `cobble link --clear`"));

    let doctor = cobble()
        .arg("doctor")
        .arg("--json")
        .arg(&project_dir)
        .arg("--commands-json")
        .arg(write_say_commands_json(temp_dir.path()))
        .output()
        .unwrap();
    let (doctor_stdout, doctor_stderr) = output_text(&doctor);
    assert!(
        doctor.status.success(),
        "doctor --json failed\nstdout:\n{doctor_stdout}\nstderr:\n{doctor_stderr}"
    );
    let doctor_json: serde_json::Value = serde_json::from_str(&doctor_stdout).unwrap();
    assert_eq!(doctor_json["status"], "error");
    assert_eq!(doctor_json["experimental_link"]["status"], "error");
    assert!(doctor_json["experimental_link"]["message"]
        .as_str()
        .unwrap()
        .contains("outside target datapacks directory"));

    let clean = cobble()
        .arg("clean")
        .arg(&project_dir)
        .arg("--linked")
        .arg("--yes")
        .output()
        .unwrap();
    let (_clean_stdout, clean_stderr) = output_text(&clean);
    assert!(!clean.status.success());
    assert!(clean_stderr.contains("outside target datapacks directory"));
    assert!(outside_dir.join("important.txt").exists());
}

#[test]
fn cli_watch_link_requires_configured_link_state() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let project_dir = temp_dir.path().join("linked_pack");

    let init = cobble()
        .arg("init")
        .arg("--name")
        .arg(&project_dir)
        .arg("--template")
        .arg("minimal")
        .output()
        .unwrap();
    let (init_stdout, init_stderr) = output_text(&init);
    assert!(
        init.status.success(),
        "init failed\nstdout:\n{init_stdout}\nstderr:\n{init_stderr}"
    );

    let output = cobble()
        .arg("watch")
        .arg(project_dir.join("src"))
        .arg("--link")
        .output()
        .unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("No Cobble link configured"));
}

#[test]
fn cli_watch_link_rejects_output_override() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let project_dir = temp_dir.path().join("linked_pack");

    let init = cobble()
        .arg("init")
        .arg("--name")
        .arg(&project_dir)
        .arg("--template")
        .arg("minimal")
        .output()
        .unwrap();
    let (init_stdout, init_stderr) = output_text(&init);
    assert!(
        init.status.success(),
        "init failed\nstdout:\n{init_stdout}\nstderr:\n{init_stderr}"
    );

    let output = cobble()
        .arg("watch")
        .arg(project_dir.join("src"))
        .arg("--link")
        .arg("--output")
        .arg(temp_dir.path().join("output"))
        .output()
        .unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("--link cannot be combined with --output"));
}

#[test]
fn cli_watch_link_rejects_namespace_override() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let project_dir = temp_dir.path().join("linked_pack");

    let init = cobble()
        .arg("init")
        .arg("--name")
        .arg(&project_dir)
        .arg("--template")
        .arg("minimal")
        .output()
        .unwrap();
    let (init_stdout, init_stderr) = output_text(&init);
    assert!(
        init.status.success(),
        "init failed\nstdout:\n{init_stdout}\nstderr:\n{init_stderr}"
    );

    let output = cobble()
        .arg("watch")
        .arg(project_dir.join("src"))
        .arg("--link")
        .arg("--namespace")
        .arg("other_pack")
        .output()
        .unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("--link cannot be combined with --namespace"));
}

#[test]
fn cli_watch_link_refuses_unmarked_existing_pack() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let project_dir = temp_dir.path().join("linked_pack");
    let datapacks_dir = temp_dir.path().join("world").join("datapacks");
    let pack_dir = datapacks_dir.join("linked_pack");

    let init = cobble()
        .arg("init")
        .arg("--name")
        .arg(&project_dir)
        .arg("--template")
        .arg("minimal")
        .output()
        .unwrap();
    let (init_stdout, init_stderr) = output_text(&init);
    assert!(
        init.status.success(),
        "init failed\nstdout:\n{init_stdout}\nstderr:\n{init_stderr}"
    );

    let link = cobble()
        .arg("link")
        .arg(&project_dir)
        .arg("--datapacks")
        .arg(&datapacks_dir)
        .output()
        .unwrap();
    let (link_stdout, link_stderr) = output_text(&link);
    assert!(
        link.status.success(),
        "link failed\nstdout:\n{link_stdout}\nstderr:\n{link_stderr}"
    );

    fs::create_dir_all(&pack_dir).unwrap();
    fs::write(pack_dir.join("important.txt"), "keep me").unwrap();

    let output = cobble()
        .arg("watch")
        .arg(project_dir.join("src"))
        .arg("--link")
        .output()
        .unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("Refusing to build linked output"));
    assert!(pack_dir.join("important.txt").exists());
}

#[test]
fn cli_watch_link_refuses_mismatched_marker_namespace() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let project_dir = temp_dir.path().join("linked_pack");
    let datapacks_dir = temp_dir.path().join("world").join("datapacks");
    let pack_dir = datapacks_dir.join("linked_pack");

    let init = cobble()
        .arg("init")
        .arg("--name")
        .arg(&project_dir)
        .arg("--template")
        .arg("minimal")
        .output()
        .unwrap();
    let (init_stdout, init_stderr) = output_text(&init);
    assert!(
        init.status.success(),
        "init failed\nstdout:\n{init_stdout}\nstderr:\n{init_stderr}"
    );

    let link = cobble()
        .arg("link")
        .arg(&project_dir)
        .arg("--datapacks")
        .arg(&datapacks_dir)
        .output()
        .unwrap();
    let (link_stdout, link_stderr) = output_text(&link);
    assert!(
        link.status.success(),
        "link failed\nstdout:\n{link_stdout}\nstderr:\n{link_stderr}"
    );

    fs::create_dir_all(pack_dir.join(".cobble")).unwrap();
    fs::create_dir_all(pack_dir.join("data/other_pack/function")).unwrap();
    fs::write(pack_dir.join("pack.mcmeta"), "{}").unwrap();
    fs::write(
        pack_dir.join(".cobble/build_manifest.json"),
        r#"{
  "version": 1,
  "cobble_version": "0.7.2",
  "namespace": "other_pack"
}"#,
    )
    .unwrap();

    let status = cobble()
        .arg("link")
        .arg(&project_dir)
        .arg("--status")
        .output()
        .unwrap();
    let (status_stdout, status_stderr) = output_text(&status);
    assert!(
        status.status.success(),
        "link --status failed\nstdout:\n{status_stdout}\nstderr:\n{status_stderr}"
    );
    assert!(status_stdout.contains("Marker: invalid"));
    assert!(status_stdout.contains("marker namespace `other_pack`"));
    assert!(status_stdout.contains("Recovery: move the existing pack aside"));

    let output = cobble()
        .arg("watch")
        .arg(project_dir.join("src"))
        .arg("--link")
        .output()
        .unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("marker namespace `other_pack`"));
    assert!(stderr.contains("project namespace `linked_pack`"));
}

#[test]
fn cli_link_watch_and_clean_reject_forged_same_namespace_marker() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let project_dir = temp_dir.path().join("linked_pack");
    let datapacks_dir = temp_dir.path().join("world").join("datapacks");
    let pack_dir = datapacks_dir.join("linked_pack");

    let init = cobble()
        .arg("init")
        .arg("--name")
        .arg(&project_dir)
        .arg("--template")
        .arg("minimal")
        .output()
        .unwrap();
    let (init_stdout, init_stderr) = output_text(&init);
    assert!(
        init.status.success(),
        "init failed\nstdout:\n{init_stdout}\nstderr:\n{init_stderr}"
    );

    let link = cobble()
        .arg("link")
        .arg(&project_dir)
        .arg("--datapacks")
        .arg(&datapacks_dir)
        .output()
        .unwrap();
    let (link_stdout, link_stderr) = output_text(&link);
    assert!(
        link.status.success(),
        "link failed\nstdout:\n{link_stdout}\nstderr:\n{link_stderr}"
    );

    fs::create_dir_all(pack_dir.join(".cobble")).unwrap();
    fs::create_dir_all(pack_dir.join("data/linked_pack/function")).unwrap();
    fs::write(pack_dir.join("pack.mcmeta"), "{}").unwrap();
    fs::write(pack_dir.join("SENTINEL_DO_NOT_DELETE.txt"), "keep me").unwrap();
    fs::write(
        pack_dir.join(".cobble/build_manifest.json"),
        r#"{
  "version": 1,
  "cobble_version": "0.7.2",
  "namespace": "linked_pack",
  "generated_namespaces": ["linked_pack"]
}"#,
    )
    .unwrap();

    let status = cobble()
        .arg("link")
        .arg(&project_dir)
        .arg("--status")
        .output()
        .unwrap();
    let (status_stdout, status_stderr) = output_text(&status);
    assert!(
        status.status.success(),
        "link --status failed\nstdout:\n{status_stdout}\nstderr:\n{status_stderr}"
    );
    assert!(status_stdout.contains("Marker: invalid"));
    assert!(status_stdout.contains("missing project_id"));
    assert!(status_stdout.contains("Recovery: move the existing pack aside"));

    let watch = cobble()
        .arg("watch")
        .arg(project_dir.join("src"))
        .arg("--link")
        .output()
        .unwrap();
    let (_watch_stdout, watch_stderr) = output_text(&watch);
    assert!(!watch.status.success());
    assert!(watch_stderr.contains("missing project_id"));

    let clean = cobble()
        .arg("clean")
        .arg(&project_dir)
        .arg("--linked")
        .arg("--yes")
        .output()
        .unwrap();
    let (_clean_stdout, clean_stderr) = output_text(&clean);
    assert!(!clean.status.success());
    assert!(clean_stderr.contains("missing project_id"));
    assert!(pack_dir.join("SENTINEL_DO_NOT_DELETE.txt").exists());
}

#[test]
fn cli_build_dry_run_does_not_write_final_output() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(temp_dir.path(), "def main():\n    /say dry run\n");
    let output_dir = temp_dir.path().join("output");

    let output = cobble()
        .arg("build")
        .arg(&input)
        .arg("--namespace")
        .arg("cli_regression")
        .arg("--output")
        .arg(&output_dir)
        .arg("--dry-run")
        .output()
        .unwrap();

    let (stdout, stderr) = output_text(&output);
    assert!(
        output.status.success(),
        "dry-run failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(stdout.contains("Dry run: final output will not be written"));
    assert!(stdout.contains("Build summary:"));
    assert!(stdout.contains("Output: not written (--dry-run)"));
    assert!(
        !output_dir.exists(),
        "dry-run should not create the final output directory"
    );
}

#[test]
fn cli_build_dry_run_validate_reports_validation_summary() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let commands_json = write_say_commands_json(temp_dir.path());
    let input = write_source(temp_dir.path(), "def main():\n    /say valid\n");
    let output_dir = temp_dir.path().join("output");

    let output = cobble()
        .arg("build")
        .arg(&input)
        .arg("--namespace")
        .arg("cli_regression")
        .arg("--output")
        .arg(&output_dir)
        .arg("--dry-run")
        .arg("--validate")
        .arg("--commands-json")
        .arg(&commands_json)
        .output()
        .unwrap();

    let (stdout, stderr) = output_text(&output);
    assert!(
        output.status.success(),
        "dry-run validation failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(stdout.contains("Validating generated commands"));
    assert!(stdout.contains("All commands valid"));
    assert!(stdout.contains("Validation:"));
    assert!(stdout.contains("Output: not written (--dry-run)"));
    assert!(!output_dir.exists());
}

#[test]
fn cli_build_rejects_dry_run_with_zip() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(temp_dir.path(), "def main():\n    /say invalid options\n");

    let output = cobble()
        .arg("build")
        .arg(&input)
        .arg("--dry-run")
        .arg("--zip")
        .output()
        .unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("--dry-run cannot be combined with --zip"));
}

#[test]
fn cli_build_validate_refuses_existing_file_output_without_deleting_it() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(temp_dir.path(), "def main():\n    /say preserve file\n");
    let output_path = temp_dir.path().join("output-file");
    fs::write(&output_path, "important\n").unwrap();

    let output = cobble()
        .arg("build")
        .arg(&input)
        .arg("--namespace")
        .arg("cli_regression")
        .arg("--output")
        .arg(&output_path)
        .arg("--validate")
        .arg("--commands-json")
        .arg(write_say_commands_json(temp_dir.path()))
        .output()
        .unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("Refusing to build data pack over non-directory output path"));
    assert!(output_path.is_file());
    assert_eq!(fs::read_to_string(&output_path).unwrap(), "important\n");
}

#[cfg(unix)]
#[test]
fn cli_build_refuses_symlink_output_parent_component() {
    use std::os::unix::fs::symlink;

    let temp_dir = tempfile::TempDir::new().unwrap();
    let source_dir = temp_dir.path().join("src");
    let real_parent = temp_dir.path().join("real-datapacks");
    let symlink_parent = temp_dir.path().join("symlink-datapacks");
    fs::create_dir_all(&source_dir).unwrap();
    fs::write(source_dir.join("main.cbl"), "def main():\n    /say safe\n").unwrap();
    fs::create_dir_all(&real_parent).unwrap();
    symlink(&real_parent, &symlink_parent).unwrap();

    let output = cobble()
        .arg("build")
        .arg(&source_dir)
        .arg("-o")
        .arg(symlink_parent.join("build_pack"))
        .output()
        .unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("Refusing to build through symlink"));
    assert!(!real_parent.join("build_pack").exists());
}

#[cfg(unix)]
#[test]
fn cli_build_refuses_symlink_descendant_in_existing_output() {
    use std::os::unix::fs::symlink;

    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(temp_dir.path(), "def main():\n    /say safe\n");
    let output_dir = temp_dir.path().join("output");
    let outside_dir = temp_dir.path().join("outside");
    fs::create_dir_all(&output_dir).unwrap();
    fs::create_dir_all(&outside_dir).unwrap();
    fs::write(outside_dir.join("important.txt"), "keep\n").unwrap();
    symlink(&outside_dir, output_dir.join("data")).unwrap();

    let output = cobble()
        .arg("build")
        .arg(&input)
        .arg("-o")
        .arg(&output_dir)
        .output()
        .unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("Refusing to build through symlink"));
    assert_eq!(
        fs::read_to_string(outside_dir.join("important.txt")).unwrap(),
        "keep\n"
    );
    assert!(!outside_dir.join("cobble").exists());
}

#[test]
fn cli_check_reports_language_diagnostic_location() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
def reward(player, amount=1):
    /say reward
"#,
    );

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stdout.contains("Checking 1 file"));
    assert!(stderr.contains("unsupported-function-parameter"));
    assert!(stderr.contains("Default parameter values are not supported"));
    assert!(stderr.contains("Use explicit arguments at each call site"));
    assert!(stderr.contains("main.cbl:2:"));
}

#[test]
fn cli_check_reports_structural_syntax_diagnostic_location() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
def main():
    value = (1 + 2
"#,
    );

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("unclosed-delimiter"));
    assert!(stderr.contains("Opening delimiter `(` is not closed"));
    assert!(stderr.contains("main.cbl:3:13"));
}

#[test]
fn cli_check_reports_duplicate_function_parameters() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
def greet(player, player):
    /say duplicate
"#,
    );

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("duplicate-function-parameter"));
    assert!(stderr.contains("Duplicate function parameter `player`"));
    assert!(stderr.contains("main.cbl:2:19"));
}

#[test]
fn cli_build_rejects_language_diagnostic_before_output() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
def main():
    score += 1
"#,
    );
    let output_dir = temp_dir.path().join("output");

    let output = cobble()
        .arg("build")
        .arg(&input)
        .arg("--output")
        .arg(&output_dir)
        .output()
        .unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("Language diagnostics failed"));
    assert!(stderr.contains("unsupported-assignment"));
    assert!(stderr.contains("Compound assignment `+=` is not supported"));
    assert!(stderr.contains("3 |     score += 1"));
    assert!(stderr.contains("^"));
    assert!(!output_dir.exists());
}

#[test]
fn cli_check_rejects_for_else_without_rejecting_if_else() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
def main():
    if True:
        pass
    else:
        pass
    for i in range(3):
        pass
    else:
        pass
"#,
    );

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert_eq!(stderr.matches("unsupported-control-flow").count(), 1);
    assert!(stderr.contains("`for ... else` blocks are not supported"));
}

#[test]
fn cli_check_reports_semantic_preflight_diagnostics() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
def helper():
    pass

def helper():
    return

def main():
    result = helper()
"#,
    );

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("duplicate-function"));
    assert!(stderr.contains("unsupported-return"));
    assert!(stderr.contains("unsupported-function-call-expression"));
    assert!(stderr.contains("Minecraft functions cannot return early"));
}

#[test]
fn cli_check_reports_undefined_variable_diagnostics() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
def main():
    total = missing_score + 1
"#,
    );

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("undefined-variable"));
    assert!(stderr.contains("Undefined variable `missing_score`"));
    assert!(stderr.contains("Define or import `missing_score`"));
}

#[test]
fn cli_check_json_reports_success_summary() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(temp_dir.path(), "def main():\n    /say ok\n");

    let output = cobble()
        .arg("check")
        .arg("--json")
        .arg(&input)
        .output()
        .unwrap();

    let (stdout, stderr) = output_text(&output);
    assert!(
        output.status.success(),
        "check --json failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    let value: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["ok"], true);
    assert_eq!(value["files_checked"], 1);
    assert_eq!(value["error_count"], 0);
    assert_eq!(value["diagnostics"].as_array().unwrap().len(), 0);
    assert_eq!(value["files"][0]["functions"], 1);
}

#[test]
fn cli_check_json_symbols_reports_experimental_editor_symbols() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
import stdlib

const INDEX = 0
@Players = @a[type=player]
define @Marker = @e[type=marker]
create {"Tags": ["marker"]}
end

datapack.predicate("always", {"condition": "minecraft:random_chance", "chance": 1})

def setup():
    /say ok
"#,
    );

    let output = cobble()
        .arg("check")
        .arg("--json")
        .arg("--symbols")
        .arg(&input)
        .output()
        .unwrap();

    let (stdout, stderr) = output_text(&output);
    assert!(
        output.status.success(),
        "check --json --symbols failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    let value: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["schema_version"], 1);
    let symbols = value["experimental_symbols"].as_array().unwrap();
    assert!(symbols.iter().any(|symbol| {
        symbol["kind"] == "import" && symbol["name"] == "stdlib" && symbol["line"] == 2
    }));
    assert!(symbols
        .iter()
        .any(|symbol| symbol["kind"] == "const" && symbol["name"] == "INDEX"));
    assert!(symbols
        .iter()
        .any(|symbol| symbol["kind"] == "selector_alias" && symbol["name"] == "@Players"));
    assert!(symbols
        .iter()
        .any(|symbol| symbol["kind"] == "entity_template" && symbol["name"] == "@Marker"));
    assert!(symbols.iter().any(|symbol| {
        symbol["kind"] == "datapack_resource" && symbol["name"] == "predicate:always"
    }));
    assert!(symbols
        .iter()
        .any(|symbol| symbol["kind"] == "function" && symbol["name"] == "setup"));
}

#[test]
fn cli_check_json_omits_experimental_plugin_host_by_default() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(temp_dir.path(), "def main():\n    /say ok\n");

    let output = cobble()
        .arg("check")
        .arg("--json")
        .arg(&input)
        .output()
        .unwrap();

    let (stdout, stderr) = output_text(&output);
    assert!(
        output.status.success(),
        "check --json failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    let value: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(value.get("experimental_plugins").is_none());
}

#[test]
fn cli_check_json_omits_experimental_python_compat_by_default() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(temp_dir.path(), "def main():\n    pass\n");

    let output = cobble()
        .arg("check")
        .arg("--json")
        .arg(&input)
        .output()
        .unwrap();

    let (stdout, stderr) = output_text(&output);
    assert!(
        output.status.success(),
        "check --json failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    let value: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(value.get("experimental_python_compat").is_none());
}

#[test]
fn cli_check_json_reports_experimental_python_compat_when_enabled() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(temp_dir.path(), "def main():\n    pass\n");

    let output = cobble()
        .arg("check")
        .arg("--json")
        .arg("--experimental-python-compat")
        .arg(&input)
        .output()
        .unwrap();

    let (stdout, stderr) = output_text(&output);
    assert!(
        output.status.success(),
        "check --json --experimental-python-compat failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    let value: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let compat = &value["experimental_python_compat"];
    assert_eq!(compat["enabled"], true);
    assert_eq!(compat["mode"], "diagnostics-only");
    assert!(compat["supported_constructs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|construct| construct == "pass statement as an explicit no-op"));
    assert!(compat["unsupported_detected"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn cli_check_json_reports_experimental_python_compat_diagnostics() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(temp_dir.path(), "class Game:\n    pass\n");

    let output = cobble()
        .arg("check")
        .arg("--json")
        .arg("--experimental-python-compat")
        .arg(&input)
        .output()
        .unwrap();

    let (stdout, stderr) = output_text(&output);
    assert!(
        !output.status.success(),
        "check --json --experimental-python-compat unexpectedly passed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    let value: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let detected = value["experimental_python_compat"]["unsupported_detected"]
        .as_array()
        .unwrap();
    assert!(detected
        .iter()
        .any(|diagnostic| diagnostic["kind"] == "unsupported-python-syntax"));
}

#[test]
fn cli_check_json_reports_experimental_python_compat_when_enabled_by_config() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    fs::create_dir_all(temp_dir.path().join("src")).unwrap();
    fs::write(
        temp_dir.path().join("cobble.toml"),
        r#"
[project]
name = "python_compat_config"
description = "Python compatibility config"
namespace = "python_compat_config"
version = "1.0.0"
pack_format = "101.1"

[build]
source = "src"
output = "output"
entry_points = []

[experimental]
python_compat = true
"#,
    )
    .unwrap();
    fs::write(
        temp_dir.path().join("src/main.cbl"),
        "def main():\n    pass\n",
    )
    .unwrap();

    let output = cobble()
        .current_dir(temp_dir.path())
        .arg("check")
        .arg("--json")
        .output()
        .unwrap();

    let (stdout, stderr) = output_text(&output);
    assert!(
        output.status.success(),
        "check --json with python compat config failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    let value: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["experimental_python_compat"]["enabled"], true);
    assert_eq!(
        value["experimental_python_compat"]["mode"],
        "diagnostics-only"
    );
}

#[test]
fn cli_check_json_reports_experimental_plugin_host_when_enabled() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(temp_dir.path(), "def main():\n    /say ok\n");

    let output = cobble()
        .arg("check")
        .arg("--json")
        .arg("--experimental-plugins")
        .arg(&input)
        .output()
        .unwrap();

    let (stdout, stderr) = output_text(&output);
    assert!(
        output.status.success(),
        "check --json --experimental-plugins failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    let value: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let plugins = &value["experimental_plugins"];
    assert_eq!(plugins["enabled"], true);
    assert_eq!(plugins["manifests_checked"], 0);
    assert!(plugins["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|diagnostic| {
            diagnostic["kind"] == "experimental-plugin-diagnostic"
                && diagnostic["plugin"] == "cobble.plugin_host"
                && diagnostic["plugin_kind"] == "host-skeleton"
        }));
}

#[test]
fn cli_check_json_reports_experimental_plugin_host_when_enabled_by_config() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    fs::create_dir_all(temp_dir.path().join("src")).unwrap();
    fs::write(
        temp_dir.path().join("cobble.toml"),
        r#"
[project]
name = "plugin_config"
description = "Plugin config"
namespace = "plugin_config"
version = "1.0.0"
pack_format = "101.1"

[build]
source = "src"
output = "output"
entry_points = []

[experimental]
plugins = true
"#,
    )
    .unwrap();
    fs::write(
        temp_dir.path().join("src/main.cbl"),
        "def main():\n    /say ok\n",
    )
    .unwrap();

    let output = cobble()
        .current_dir(temp_dir.path())
        .arg("check")
        .arg("--json")
        .output()
        .unwrap();

    let (stdout, stderr) = output_text(&output);
    assert!(
        output.status.success(),
        "check --json with config plugins failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    let value: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["experimental_plugins"]["enabled"], true);
    assert_eq!(value["experimental_plugins"]["manifests_checked"], 0);
}

#[test]
fn cli_check_json_parses_experimental_plugin_manifests_without_running_code() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    fs::create_dir_all(temp_dir.path().join("src")).unwrap();
    fs::create_dir_all(temp_dir.path().join("plugins")).unwrap();
    fs::write(
        temp_dir.path().join("cobble.toml"),
        r#"
[project]
name = "plugin_manifest"
description = "Plugin manifest"
namespace = "plugin_manifest"
version = "1.0.0"
pack_format = "101.1"

[build]
source = "src"
output = "output"
entry_points = []
"#,
    )
    .unwrap();
    fs::write(
        temp_dir.path().join("src/main.cbl"),
        "def main():\n    /say ok\n",
    )
    .unwrap();
    fs::write(
        temp_dir.path().join("plugins/example_lints.toml"),
        r#"
plugin_version = 1
name = "example_lints"
kind = "diagnostics"

[capabilities]
read_project_metadata = true
read_source_text = true
emit_diagnostics = true
"#,
    )
    .unwrap();

    let output = cobble()
        .current_dir(temp_dir.path())
        .arg("check")
        .arg("--json")
        .arg("--experimental-plugins")
        .output()
        .unwrap();

    let (stdout, stderr) = output_text(&output);
    assert!(
        output.status.success(),
        "check --json --experimental-plugins failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    let value: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let plugins = &value["experimental_plugins"];
    assert_eq!(plugins["enabled"], true);
    assert_eq!(plugins["manifests_checked"], 1);
    assert_eq!(plugins["manifests"][0]["name"], "example_lints");
    assert_eq!(plugins["manifests"][0]["plugin_version"], 1);
    assert_eq!(plugins["manifests"][0]["kind"], "diagnostics");
    assert!(plugins["manifests"][0]["capabilities"]
        .as_array()
        .unwrap()
        .iter()
        .any(|capability| capability == "emit_diagnostics"));
    assert!(plugins["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|diagnostic| {
            diagnostic["plugin"] == "example_lints"
                && diagnostic["plugin_kind"] == "manifest-draft"
                && diagnostic["message"]
                    .as_str()
                    .unwrap()
                    .contains("no plugin code was run")
        }));
}

#[test]
fn cli_check_json_rejects_unknown_experimental_plugin_capabilities() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    fs::create_dir_all(temp_dir.path().join("src")).unwrap();
    fs::create_dir_all(temp_dir.path().join("plugins")).unwrap();
    fs::write(
        temp_dir.path().join("cobble.toml"),
        r#"
[project]
name = "plugin_manifest"
description = "Plugin manifest"
namespace = "plugin_manifest"
version = "1.0.0"
pack_format = "101.1"

[build]
source = "src"
output = "output"
entry_points = []
"#,
    )
    .unwrap();
    fs::write(
        temp_dir.path().join("src/main.cbl"),
        "def main():\n    /say ok\n",
    )
    .unwrap();
    fs::write(
        temp_dir.path().join("plugins/unsafe.toml"),
        r#"
plugin_version = 1
name = "unsafe"
kind = "diagnostics"

[capabilities]
read_project_metadata = true
open_network = true
"#,
    )
    .unwrap();

    let output = cobble()
        .current_dir(temp_dir.path())
        .arg("check")
        .arg("--json")
        .arg("--experimental-plugins")
        .output()
        .unwrap();

    let (stdout, stderr) = output_text(&output);
    assert!(
        !output.status.success(),
        "check --json --experimental-plugins unexpectedly passed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    let value: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["ok"], false);
    assert_eq!(value["error_count"], 1);
    let plugins = &value["experimental_plugins"];
    assert_eq!(plugins["manifests_checked"], 1);
    assert!(plugins["manifests"].as_array().unwrap().is_empty());
    assert!(plugins["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|diagnostic| {
            diagnostic["severity"] == "error"
                && diagnostic["plugin_kind"] == "manifest-parse"
                && diagnostic["message"]
                    .as_str()
                    .unwrap()
                    .contains("unknown field")
        }));
}

#[test]
fn cli_check_rejects_unknown_and_invalid_config_schema() {
    let cases = [
        (
            "unknown_project_key",
            r#"
[project]
name = "bad_config"
description = "Bad config"
namespace = "bad_config"
version = "1.0.0"
pack_format = "101.1"
typo = true

[build]
source = "src"
output = "output"
entry_points = []
"#,
            "unknown field `typo`",
        ),
        (
            "unknown_experimental_flag",
            r#"
[project]
name = "bad_config"
description = "Bad config"
namespace = "bad_config"
version = "1.0.0"
pack_format = "101.1"

[build]
source = "src"
output = "output"
entry_points = []

[experimental]
resource_packs = true
"#,
            "unknown field `resource_packs`",
        ),
        (
            "invalid_experimental_type",
            r#"
[project]
name = "bad_config"
description = "Bad config"
namespace = "bad_config"
version = "1.0.0"
pack_format = "101.1"

[build]
source = "src"
output = "output"
entry_points = []

[experimental]
resource_pack = "yes"
"#,
            "invalid type",
        ),
    ];

    for (name, config, expected) in cases {
        let temp_dir = tempfile::TempDir::new().unwrap();
        fs::create_dir_all(temp_dir.path().join("src")).unwrap();
        fs::write(
            temp_dir.path().join("src/main.cbl"),
            "def main():\n    /say ok\n",
        )
        .unwrap();
        fs::write(temp_dir.path().join("cobble.toml"), config).unwrap();

        let output = cobble()
            .current_dir(temp_dir.path())
            .arg("check")
            .output()
            .unwrap();

        let (stdout, stderr) = output_text(&output);
        assert!(
            !output.status.success(),
            "{name} unexpectedly succeeded\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
        assert!(
            stderr.contains(expected),
            "{name} stderr did not contain {expected:?}\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
    }
}

#[test]
fn cli_check_json_reports_config_schema_errors_as_json() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    fs::create_dir_all(temp_dir.path().join("src")).unwrap();
    fs::write(
        temp_dir.path().join("src/main.cbl"),
        "def main():\n    /say ok\n",
    )
    .unwrap();
    fs::write(
        temp_dir.path().join("cobble.toml"),
        r#"
[project]
name = "bad_config"
description = "Bad config"
namespace = "bad_config"
version = "1.0.0"
pack_format = "101.1"

[build]
source = "src"
output = "output"

[experimental]
resource_packs = true
"#,
    )
    .unwrap();

    let output = cobble()
        .current_dir(temp_dir.path())
        .arg("check")
        .arg("--json")
        .output()
        .unwrap();

    let (stdout, stderr) = output_text(&output);
    assert!(
        !output.status.success(),
        "check --json unexpectedly passed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(stderr.contains("Config validation failed"));

    let value: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["ok"], false);
    assert_eq!(value["error_count"], 1);
    assert_eq!(value["diagnostics"][0]["kind"], "config");
    assert!(value["diagnostics"][0]["message"]
        .as_str()
        .unwrap()
        .contains("unknown field `resource_packs`"));
}

#[test]
fn cli_check_human_reports_experimental_plugin_host_when_enabled() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(temp_dir.path(), "def main():\n    /say ok\n");

    let output = cobble()
        .arg("check")
        .arg("--experimental-plugins")
        .arg(&input)
        .output()
        .unwrap();

    let (stdout, stderr) = output_text(&output);
    assert!(
        output.status.success(),
        "check --experimental-plugins failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(stderr.is_empty());
    assert!(
        stdout.contains("warning: experimental plugin cobble.plugin_host reported host-skeleton")
    );
    assert!(stdout.contains("no plugins were run"));
}

#[test]
fn cli_check_symbols_requires_json() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(temp_dir.path(), "def main():\n    /say ok\n");

    let output = cobble()
        .arg("check")
        .arg("--symbols")
        .arg(&input)
        .output()
        .unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("--symbols requires --json"));
}

#[test]
fn cli_check_json_reports_structured_diagnostics() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
def main():
    total = missing_score + 1
"#,
    );

    let output = cobble()
        .arg("check")
        .arg("--json")
        .arg(&input)
        .output()
        .unwrap();

    let (stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("Validation failed with 1 error"));
    let value: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["ok"], false);
    assert_eq!(value["files_checked"], 1);
    assert_eq!(value["error_count"], 1);
    assert_eq!(value["diagnostics"][0]["kind"], "undefined-variable");
    assert_eq!(value["diagnostics"][0]["line"], 3);
    assert_eq!(value["diagnostics"][0]["column"], 13);
    assert!(value["diagnostics"][0]["formatted"]
        .as_str()
        .unwrap()
        .contains("missing_score"));
}

#[test]
fn cli_check_json_reports_unknown_math_helper_diagnostics() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
def main():
    value = math.nope(1)
"#,
    );

    let output = cobble()
        .arg("check")
        .arg("--json")
        .arg(&input)
        .output()
        .unwrap();

    let (stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("Validation failed with 1 error"));
    let value: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["ok"], false);
    assert_eq!(value["error_count"], 1);
    assert_eq!(value["diagnostics"][0]["kind"], "undefined-function");
    assert_eq!(value["diagnostics"][0]["line"], 3);
    assert!(value["diagnostics"][0]["formatted"]
        .as_str()
        .unwrap()
        .contains("Unknown math function `math.nope`"));
}

#[test]
fn cli_check_json_reports_unroll_budget_diagnostics() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
def main():
    for i in range(64):
        for j in range(64):
            for k in range(64):
                /say nested {i} {j} {k}
"#,
    );

    let output = cobble()
        .arg("check")
        .arg("--json")
        .arg(&input)
        .output()
        .unwrap();

    let (stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("Validation failed with 1 error"));
    let value: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["ok"], false);
    assert_eq!(value["error_count"], 1);
    assert_eq!(value["diagnostics"][0]["kind"], "unroll-limit-exceeded");
    assert_eq!(value["diagnostics"][0]["line"], 5);
    assert_eq!(value["diagnostics"][0]["column"], 13);
    assert!(value["diagnostics"][0]["message"]
        .as_str()
        .unwrap()
        .contains("nested unrolling"));
}

#[test]
fn cli_check_json_reports_unroll_diagnostic_on_failing_loop() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
def main():
    for i in range(1):
        /say ok {i}
    for i in range(1025):
        /say bad {i}
"#,
    );

    let output = cobble()
        .arg("check")
        .arg("--json")
        .arg(&input)
        .output()
        .unwrap();

    let (stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("Validation failed with 1 error"));
    let value: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["diagnostics"][0]["kind"], "unroll-limit-exceeded");
    assert_eq!(value["diagnostics"][0]["line"], 5);
    assert_eq!(value["diagnostics"][0]["column"], 5);
}

#[test]
fn cli_check_json_reports_semantic_stdlib_module_errors() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
from stdlib import datapack, item_component

datapack.item_modifier("named", {
    "function": "minecraft:set_components",
    "components": item_component.custom_name(text.plain("Name"))
})

def main():
    /say ok
"#,
    );

    let output = cobble()
        .arg("check")
        .arg("--json")
        .arg(&input)
        .output()
        .unwrap();

    let (stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("Validation failed with 1 error"));
    let value: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["ok"], false);
    assert_eq!(value["error_count"], 1);
    assert_eq!(value["diagnostics"][0]["kind"], "missing-stdlib-module");
    assert!(value["diagnostics"][0]["message"]
        .as_str()
        .unwrap()
        .contains("module 'text' not imported"));
}

#[test]
fn cli_check_json_python_compat_reports_duplicate_function_parameters() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
def main(player, player):
    pass
"#,
    );

    let output = cobble()
        .arg("check")
        .arg("--json")
        .arg("--experimental-python-compat")
        .arg(&input)
        .output()
        .unwrap();

    let (stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("Validation failed with 1 error"));
    let value: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(value["experimental_python_compat"]["unsupported_detected"]
        .as_array()
        .unwrap()
        .iter()
        .any(|diagnostic| diagnostic["kind"] == "duplicate-function-parameter"));
}

#[test]
fn cli_fmt_check_reports_unformatted_file_without_writing() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(temp_dir.path(), "def main():  \r\n  /say check  \r\n");

    let output = cobble()
        .arg("fmt")
        .arg("--check")
        .arg(&input)
        .output()
        .unwrap();

    let (stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stdout.contains("Would reformat"));
    assert!(stderr.contains("file(s) need formatting"));
    assert_eq!(
        fs::read_to_string(&input).unwrap(),
        "def main():  \r\n  /say check  \r\n"
    );
}

#[test]
fn cli_fmt_diff_reports_changes_without_writing() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let original = "def main():  \r\n  /say diff  \r\n";
    let input = write_source(temp_dir.path(), original);

    let output = cobble()
        .arg("fmt")
        .arg("--diff")
        .arg(&input)
        .output()
        .unwrap();

    let (stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stdout.contains("--- "));
    assert!(stdout.contains("+++ "));
    assert!(stdout.contains("-def main():  "));
    assert!(stdout.contains("+def main():"));
    assert!(stdout.contains("-  /say diff  "));
    assert!(stdout.contains("+    /say diff"));
    assert!(stderr.contains("file(s) differ from formatter output"));
    assert_eq!(fs::read_to_string(&input).unwrap(), original);
}

#[test]
fn cli_fmt_formats_file_conservatively() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        "def main():  \n  # setup  \n  /tellraw @a {\"text\":\"Hi\",\"color\":\"green\"}  \n",
    );

    let output = cobble().arg("fmt").arg(&input).output().unwrap();

    let (stdout, stderr) = output_text(&output);
    assert!(
        output.status.success(),
        "fmt failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(stdout.contains("Formatted 1 file"));
    assert_eq!(
        fs::read_to_string(&input).unwrap(),
        "def main():\n    # setup\n    /tellraw @a {\"text\":\"Hi\",\"color\":\"green\"}\n"
    );
}

#[cfg(unix)]
#[test]
fn cli_fmt_refuses_symlink_input_without_writing_target() {
    use std::os::unix::fs::symlink;

    let temp_dir = tempfile::TempDir::new().unwrap();
    let outside = temp_dir.path().join("outside.cbl");
    let input = temp_dir.path().join("linked.cbl");
    let original = "def main():  \n  /say outside  \n";
    fs::write(&outside, original).unwrap();
    symlink(&outside, &input).unwrap();

    let output = cobble().arg("fmt").arg(&input).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("Refusing to format source through symlink"));
    assert_eq!(fs::read_to_string(&outside).unwrap(), original);
}

#[test]
fn cli_fmt_rejects_invalid_source_without_writing() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let original = "def main():\n  value = (1 + 2\n";
    let input = write_source(temp_dir.path(), original);

    let output = cobble().arg("fmt").arg(&input).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("Formatting aborted"));
    assert!(stderr.contains("unclosed-delimiter"));
    assert_eq!(fs::read_to_string(&input).unwrap(), original);
}

#[test]
fn cli_fmt_directory_failure_writes_no_files() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let source_dir = temp_dir.path().join("src");
    fs::create_dir_all(&source_dir).unwrap();
    let good = source_dir.join("good.cbl");
    let bad = source_dir.join("bad.cbl");
    let good_original = "def main():  \r\n  /say keep original until all files pass  \r\n";
    let bad_original = "def broken():\n  value = (1 + 2\n";
    fs::write(&good, good_original).unwrap();
    fs::write(&bad, bad_original).unwrap();

    let output = cobble().arg("fmt").arg(&source_dir).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("Formatting aborted"));
    assert!(stderr.contains("bad.cbl"));
    assert_eq!(fs::read_to_string(&good).unwrap(), good_original);
    assert_eq!(fs::read_to_string(&bad).unwrap(), bad_original);
}

#[test]
fn cli_check_reports_undefined_standalone_call_arguments() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
def main():
    score.set("points", missing_score)
"#,
    );

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("undefined-variable"));
    assert!(stderr.contains("Undefined variable `missing_score`"));
}

#[test]
fn cli_check_reports_unsupported_none_usage() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
def main():
    storage.set("state", {"note": None})
"#,
    );

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("unsupported-none"));
    assert!(stderr.contains("None/null is only supported in data pack JSON resource helper values"));
}

#[test]
fn cli_check_rejects_lowercase_null_in_json_resources() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
datapack.predicate("maybe", {"condition": "minecraft:random_chance", "chance": null})
"#,
    );

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("unsupported-none"));
    assert!(stderr.contains("None/null is only supported in data pack JSON resource helper values"));
}

#[test]
fn cli_check_reports_storage_access_and_type_mismatch() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
def main():
    items = [1, 2, 3]
    first = items[i]
    value = 1
    value = "one"
"#,
    );

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("unsupported-storage-access"));
    assert!(stderr.contains("type-mismatch"));
}

#[test]
fn cli_check_reports_unsupported_storage_access() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
def main():
    items = [1, 2, 3]
    first = items[i]
"#,
    );

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("unsupported-storage-access"));
    assert!(stderr.contains("Dynamic storage-backed subscript indexes are not supported"));
}

#[test]
fn cli_check_allows_numeric_const_storage_subscript() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
const INDEX = 0

def main():
    items = [1, 2, 3]
    first = items[INDEX]
"#,
    );

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (stdout, stderr) = output_text(&output);
    assert!(
        output.status.success(),
        "check failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(stdout.contains("All files passed validation"));
}

#[test]
fn cli_check_reports_noop_expression_diagnostics() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
def main():
    score + 1
"#,
    );

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("no-op-expression"));
    assert!(stderr.contains("Standalone expression does not generate Minecraft commands"));
    assert!(stderr.contains("main.cbl:3:5"));
}

#[test]
fn cli_check_reports_type_mismatch_diagnostics() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
def main():
    items = ["sword"]
    items = 3
"#,
    );

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("type-mismatch"));
    assert!(stderr.contains("Type mismatch for variable 'items'"));
    assert!(stderr.contains("previously defined as type: list"));
    assert!(stderr.contains("Cannot reassign to type: integer"));
}

#[test]
fn cli_check_reports_datapack_resource_id_diagnostics() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
datapack.function_tag("minecraft/load", ["cli_regression:setup"])
datapack.item_tag("rewards", ["minecraft/diamond"])
"#,
    );

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("datapack-resource-id"));
    assert!(stderr.contains("Use 'minecraft:load' instead"));
    assert!(stderr.contains("Invalid tag value"));
    assert!(stderr.contains("minecraft:diamond"));
}

#[test]
fn cli_check_reports_multiline_datapack_tag_diagnostics() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
datapack.item_tag(
    "rewards",
    ["minecraft/diamond", 1],
)
"#,
    );

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("datapack-resource-id"));
    assert!(stderr.contains("minecraft:diamond"));
    assert!(stderr.contains("Tag values must be string resource IDs"));
}

#[test]
fn cli_check_reports_user_function_argument_count() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
def main():
    greet("@a")

def greet(player, message):
    /tellraw {player} {"text":"{message}"}
"#,
    );

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("function-argument-count"));
    assert!(stderr.contains("Function `greet` expects 2 argument(s), but 1 provided"));
    assert!(stderr.contains("Expected parameters: (player, message)"));
}

#[test]
fn cli_check_reports_undefined_user_function_calls() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
def main():
    missing("x")
"#,
    );

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("undefined-function"));
    assert!(stderr.contains("Undefined function `missing`"));
}

#[test]
fn cli_check_reports_unknown_dotted_helper_calls() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
def main():
    helper.do()
"#,
    );

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("undefined-function"));
    assert!(stderr.contains("Unknown helper function `helper.do`"));
}

#[test]
fn cli_check_reports_nested_function_call_arguments() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
def main():
    greet(make_name())

def make_name():
    pass

def greet(name):
    /say {name}
"#,
    );

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("unsupported-function-call-argument"));
    assert!(stderr.contains("Function `greet` arguments cannot contain function call expressions"));
}

#[test]
fn cli_check_reports_undefined_command_placeholders() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
def main(player):
    /tellraw {player} {"text":"{message}"}
"#,
    );

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("undefined-placeholder"));
    assert!(stderr.contains("Undefined command placeholder `message`"));
    assert!(stderr.contains("main.cbl:3:"));
}

#[test]
fn cli_check_reports_forward_command_placeholders() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
def main():
    /say {message}
    message = "hi"
"#,
    );

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("undefined-placeholder"));
    assert!(stderr.contains("Undefined command placeholder `message`"));
}

#[test]
fn cli_check_reports_invalid_command_placeholders() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
def main():
    /say {bad-name}
"#,
    );

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("invalid-placeholder"));
    assert!(stderr.contains("Invalid command placeholder `bad-name`"));
}

#[test]
fn cli_check_allows_selector_score_maps_in_raw_commands() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
def tick():
    /execute as @a[scores={demo=1..}] run title @s actionbar {"text":"Running","color":"yellow"}
"#,
    );

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (stdout, stderr) = output_text(&output);
    assert!(
        output.status.success(),
        "check failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

#[test]
fn cli_check_rejects_imported_function_command_placeholders() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
from helper import greet

def main():
    /say {greet}
"#,
    );
    fs::write(
        temp_dir.path().join("helper.cbl"),
        "def greet():\n    /say hi\n",
    )
    .unwrap();

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("unsupported-placeholder-symbol"));
    assert!(stderr.contains("Imported function `greet` cannot be used as a command placeholder"));
}

#[test]
fn cli_check_allows_imported_command_placeholders() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        r#"
from helper import imported_score

def main():
    /say {imported_score}
"#,
    );
    fs::write(
        temp_dir.path().join("helper.cbl"),
        "imported_score = 1\n\ndef helper():\n    pass\n",
    )
    .unwrap();

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (stdout, stderr) = output_text(&output);
    assert!(
        output.status.success(),
        "check failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(!stderr.contains("undefined-placeholder"));
}

#[test]
fn cli_check_reports_missing_import_with_importing_file() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(temp_dir.path(), "import missing\n\ndef main():\n    pass\n");

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("missing-import"));
    assert!(stderr.contains("Cannot import 'missing'"));
    assert!(stderr.contains(input.to_string_lossy().as_ref()));
}

#[test]
fn cli_check_reports_missing_from_import_item() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        "from helper import greet, missing\n\ndef main():\n    pass\n",
    );
    fs::write(
        temp_dir.path().join("helper.cbl"),
        "def greet():\n    /say hi\n",
    )
    .unwrap();

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("missing-import-item"));
    assert!(stderr.contains("Cannot import `missing` from `helper`"));
    assert!(stderr.contains("Available symbols: greet"));
    assert!(stderr.contains("main.cbl:1:27"));
}

#[test]
fn cli_check_reports_cross_file_duplicate_functions() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        "import helper\n\ndef greet():\n    /say from main\n",
    );
    fs::write(
        temp_dir.path().join("helper.cbl"),
        "def greet():\n    /say from helper\n",
    )
    .unwrap();

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("duplicate-function"));
    assert!(stderr.contains("Duplicate function definition `greet` across imported files"));
    assert!(stderr.contains("helper.cbl"));
}

#[test]
fn cli_check_reports_directory_duplicate_functions() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    fs::write(
        temp_dir.path().join("first.cbl"),
        "def same():\n    /say first\n",
    )
    .unwrap();
    fs::write(
        temp_dir.path().join("second.cbl"),
        "def same():\n    /say second\n",
    )
    .unwrap();

    let output = cobble().arg("check").arg(temp_dir.path()).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("duplicate-function"));
    assert!(stderr.contains("Duplicate function definition `same` across imported files"));
    assert!(stderr.contains("first.cbl"));
    assert!(stderr.contains("second.cbl"));
}

#[test]
fn cli_check_honors_configured_entry_points_without_explicit_input() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let src_dir = temp_dir.path().join("src");
    fs::create_dir_all(&src_dir).unwrap();
    fs::write(
        temp_dir.path().join("cobble.toml"),
        r#"
[project]
name = "entry_points"
description = "entry point check parity"
namespace = "entry_points"
version = "1.0.0"
pack_format = "101.1"

[build]
source = "src"
output = "output"
entry_points = ["main.cbl"]
"#,
    )
    .unwrap();
    fs::write(src_dir.join("main.cbl"), "def main():\n    /say selected\n").unwrap();
    fs::write(
        src_dir.join("unused.cbl"),
        "def unused(value=1):\n    /say should not be checked\n",
    )
    .unwrap();

    let output = cobble()
        .current_dir(temp_dir.path())
        .arg("check")
        .output()
        .unwrap();

    let (stdout, stderr) = output_text(&output);
    assert!(
        output.status.success(),
        "check failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(stdout.contains("Checking 1 file"));
    assert!(!stderr.contains("unsupported-function-parameter"));
}

#[test]
fn cli_build_reports_directory_duplicate_functions_before_output() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    fs::write(
        temp_dir.path().join("first.cbl"),
        "def same():\n    /say first\n",
    )
    .unwrap();
    fs::write(
        temp_dir.path().join("second.cbl"),
        "def same():\n    /say second\n",
    )
    .unwrap();
    let output_dir = temp_dir.path().join("output");

    let output = cobble()
        .arg("build")
        .arg(temp_dir.path())
        .arg("--namespace")
        .arg("cli_regression")
        .arg("--output")
        .arg(&output_dir)
        .arg("--dry-run")
        .output()
        .unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("Language diagnostics failed"));
    assert!(stderr.contains("duplicate-function"));
    assert!(stderr.contains("Duplicate function definition `same` across imported files"));
    assert!(!output_dir.exists());
}

#[test]
fn cli_check_reports_cross_file_duplicate_selector_aliases() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        "import helper\n\n@Players = @a\n\ndef main():\n    pass\n",
    );
    fs::write(
        temp_dir.path().join("helper.cbl"),
        "@Players = @p\n\ndef helper():\n    pass\n",
    )
    .unwrap();

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("duplicate-symbol"));
    assert!(stderr.contains("Duplicate selector alias `@Players` across imported files"));
    assert!(stderr.contains("helper.cbl"));
}

#[test]
fn cli_check_reports_imported_function_argument_count() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(
        temp_dir.path(),
        "import helper\n\ndef main():\n    greet(\"@a\")\n",
    );
    fs::write(
        temp_dir.path().join("helper.cbl"),
        "def greet(player, message):\n    /tellraw {player} {\"text\":\"{message}\"}\n",
    )
    .unwrap();

    let output = cobble().arg("check").arg(&input).output().unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("function-argument-count"));
    assert!(stderr.contains("Function `greet` expects 2 argument(s), but 1 provided"));
    assert!(stderr.contains("helper.cbl"));
}

#[test]
fn cli_build_validate_prints_source_mapped_diagnostics() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let commands_json = write_say_commands_json(temp_dir.path());
    let input = write_source(temp_dir.path(), "def main():\n    /not_a_command\n");
    let output_dir = temp_dir.path().join("output");

    let output = cobble()
        .arg("build")
        .arg(&input)
        .arg("--namespace")
        .arg("cli_regression")
        .arg("--output")
        .arg(&output_dir)
        .arg("--validate")
        .arg("--commands-json")
        .arg(&commands_json)
        .output()
        .unwrap();

    let (stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stdout.contains("Validating generated commands"));
    assert!(stderr.contains("not_a_command"));
    assert!(stderr.contains("^"));
    assert!(stderr.contains("source: main.cbl:2:5"));
    assert!(stderr.contains("validation error(s) found"));
}

#[test]
fn cli_inspect_json_reports_manifest_summary() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(temp_dir.path(), "def main():\n    /say inspect\n");
    let output_dir = temp_dir.path().join("output");

    let build = cobble()
        .arg("build")
        .arg(&input)
        .arg("--namespace")
        .arg("cli_regression")
        .arg("--output")
        .arg(&output_dir)
        .arg("--quiet")
        .output()
        .unwrap();
    let (build_stdout, build_stderr) = output_text(&build);
    assert!(
        build.status.success(),
        "build failed\nstdout:\n{build_stdout}\nstderr:\n{build_stderr}"
    );

    let inspect = cobble()
        .arg("inspect")
        .arg(&output_dir)
        .arg("--json")
        .output()
        .unwrap();
    let (stdout, stderr) = output_text(&inspect);
    assert!(
        inspect.status.success(),
        "inspect failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    let value: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["ok"], true);
    assert_eq!(value["status"], "ok");
    assert_eq!(value["manifest"]["namespace"], "cli_regression");
    assert_eq!(value["manifest"]["generated"]["functions"], 1);
    assert_eq!(value["manifest"]["generated"]["commands"], 1);
    assert_eq!(value["source_map_entries"], 1);
}

#[test]
fn cli_inspect_human_output_reports_manifest_summary() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let input = write_source(temp_dir.path(), "def main():\n    /say inspect\n");
    let output_dir = temp_dir.path().join("output");

    let build = cobble()
        .arg("build")
        .arg(&input)
        .arg("--namespace")
        .arg("cli_regression")
        .arg("--output")
        .arg(&output_dir)
        .arg("--quiet")
        .output()
        .unwrap();
    let (build_stdout, build_stderr) = output_text(&build);
    assert!(
        build.status.success(),
        "build failed\nstdout:\n{build_stdout}\nstderr:\n{build_stderr}"
    );

    let inspect = cobble().arg("inspect").arg(&output_dir).output().unwrap();
    let (stdout, stderr) = output_text(&inspect);
    assert!(
        inspect.status.success(),
        "inspect failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(stdout.contains("Cobble inspect:"));
    assert!(stdout.contains("Namespace: cli_regression"));
    assert!(stdout.contains("Functions: 1"));
    assert!(stdout.contains("Commands: 1"));
    assert!(stdout.contains("Validation: not recorded"));
}

#[test]
fn cli_inspect_reports_resource_pack_static_asset_counts() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let project_dir = temp_dir.path().join("project");
    let source_dir = project_dir.join("src");
    let asset_path = project_dir.join("assets/inspect_static/textures/item/icon.png");
    fs::create_dir_all(&source_dir).unwrap();
    fs::create_dir_all(asset_path.parent().unwrap()).unwrap();
    fs::write(
        source_dir.join("main.cbl"),
        "def main():\n    /say assets\n",
    )
    .unwrap();
    fs::write(&asset_path, b"fake png bytes").unwrap();
    fs::write(
        project_dir.join("cobble.toml"),
        r#"
[project]
name = "inspect_static"
description = "Inspect static assets"
namespace = "inspect_static"
version = "1.0.0"
pack_format = "101.1"

[build]
source = "src"
output = "output"

[experimental]
resource_pack = true
"#,
    )
    .unwrap();

    let build = cobble()
        .arg("build")
        .arg(&project_dir)
        .arg("--quiet")
        .output()
        .unwrap();
    let (build_stdout, build_stderr) = output_text(&build);
    assert!(
        build.status.success(),
        "build failed\nstdout:\n{build_stdout}\nstderr:\n{build_stderr}"
    );

    let output_dir = project_dir.join("output");
    let inspect = cobble().arg("inspect").arg(&output_dir).output().unwrap();
    let (stdout, stderr) = output_text(&inspect);
    assert!(
        inspect.status.success(),
        "inspect failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(stdout.contains("Resource-pack static assets: 1"));
    assert!(stdout.contains("resource_pack_static_asset: inspect_static:textures/item/icon.png"));

    let inspect_json = cobble()
        .arg("inspect")
        .arg(&output_dir)
        .arg("--json")
        .output()
        .unwrap();
    let (stdout, stderr) = output_text(&inspect_json);
    assert!(
        inspect_json.status.success(),
        "inspect --json failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    let value: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(
        value["manifest"]["generated"]["resource_pack_static_assets"],
        1
    );
    assert!(value["manifest"]["resources"]
        .as_array()
        .unwrap()
        .iter()
        .any(|resource| {
            resource["kind"] == "resource_pack_static_asset"
                && resource["namespace"] == "inspect_static"
                && resource["path"] == "textures/item/icon.png"
        }));
}

#[test]
fn cli_inspect_missing_manifest_fails_with_actionable_error() {
    let temp_dir = tempfile::TempDir::new().unwrap();

    let output = cobble()
        .arg("inspect")
        .arg(temp_dir.path())
        .output()
        .unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("No Cobble build manifest found"));
    assert!(stderr.contains("Run `cobble build`"));
}

#[test]
fn cli_inspect_malformed_manifest_reports_parse_error() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let cobble_dir = temp_dir.path().join(".cobble");
    fs::create_dir_all(&cobble_dir).unwrap();
    fs::write(cobble_dir.join("build_manifest.json"), "{not json").unwrap();

    let output = cobble()
        .arg("inspect")
        .arg(temp_dir.path())
        .output()
        .unwrap();

    let (_stdout, stderr) = output_text(&output);
    assert!(!output.status.success());
    assert!(stderr.contains("Failed to parse"));
    assert!(stderr.contains("build_manifest.json"));
}
