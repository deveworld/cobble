//! Regression tests for bugs found while using the project commands the way a
//! pack author does, on a project moved around the disk and with editors that
//! save by replacing files:
//! - `cobble watch` stopped seeing `cobble.toml`, or the single file it was
//!   given, after the first save that replaced the file
//! - moving `build.source` out of a folder that held rebuilt output failed
//!   and left the config unwatched
//! - link state kept paths relative to the folder `cobble link` ran in, so
//!   `watch --link`, `clean --linked`, and `link --status` looked elsewhere
//!   when run from another folder
//! - `cobble build main.cbl` inside `src` ignored the `cobble.toml` above it
//!   and built a pack with the default namespace
//! - a project refused its own output after a namespace change or after the
//!   project folder moved, and `cobble clean` refused it too

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};
use tempfile::TempDir;

fn cobble() -> Command {
    Command::new(env!("CARGO_BIN_EXE_cobble"))
}

fn config(namespace: &str, description: &str) -> String {
    format!(
        r#"[project]
name = "workflow"
description = "{description}"
namespace = "{namespace}"
version = "1.0.0"
pack_format = "121.0"

[build]
source = "src"
output = "output"
entry_points = ["main.cbl"]
"#
    )
}

fn write_project(root: &Path) -> PathBuf {
    let project = root.join("project");
    fs::create_dir_all(project.join("src")).unwrap();
    fs::write(project.join("cobble.toml"), config("workflow", "one")).unwrap();
    fs::write(project.join("src/main.cbl"), "def hello():\n    /say hi\n").unwrap();
    project
}

/// A command tree with `say`, so `--validate` needs no download.
fn write_say_tree(root: &Path) -> PathBuf {
    let path = root.join("commands.json");
    fs::write(
        &path,
        r#"{
  "type": "root",
  "children": {
    "say": {
      "type": "literal",
      "children": {
        "message": {
          "type": "argument",
          "parser": "brigadier:string",
          "properties": { "type": "greedy" },
          "executable": true
        }
      }
    }
  }
}"#,
    )
    .unwrap();
    path
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn succeed(command: &mut Command) -> String {
    let output = command.output().unwrap();
    let text = text(&output);
    assert!(output.status.success(), "{text}");
    text
}

#[test]
fn link_state_names_the_same_folder_from_anywhere() {
    let temp_dir = TempDir::new().unwrap();
    let project = write_project(temp_dir.path());
    let datapacks = temp_dir.path().join("world/datapacks");
    fs::create_dir_all(&datapacks).unwrap();

    succeed(
        cobble()
            .current_dir(&project)
            .args(["link", "--datapacks", "../world/datapacks"]),
    );
    let state: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(project.join(".cobble/link_state.json")).unwrap())
            .unwrap();
    let pack_path = Path::new(state["pack_path"].as_str().unwrap());
    assert!(pack_path.is_absolute(), "{pack_path:?}");
    assert!(pack_path.ends_with("world/datapacks/workflow"));
    assert!(!pack_path.to_string_lossy().contains(".."));

    // Linking from the parent folder gives the same target.
    succeed(cobble().current_dir(temp_dir.path()).args([
        "link",
        "project",
        "--datapacks",
        "../world/datapacks",
    ]));
    assert_eq!(
        state,
        serde_json::from_str::<serde_json::Value>(
            &fs::read_to_string(project.join(".cobble/link_state.json")).unwrap()
        )
        .unwrap()
    );

    // A pack built from the project is found from the parent folder.
    let mut watch = cobble()
        .current_dir(&project)
        .args(["watch", "--link"])
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let marker = datapacks.join("workflow/.cobble/build_manifest.json");
    wait_for(&mut watch, || marker.exists());
    stop(watch);
    let status = succeed(
        cobble()
            .current_dir(temp_dir.path())
            .args(["link", "project", "--status"]),
    );
    assert!(status.contains("Marker: present"), "{status}");
    let clean = succeed(
        cobble()
            .current_dir(temp_dir.path())
            .args(["clean", "project", "--linked", "--yes"]),
    );
    assert!(clean.contains("Removed Cobble output"), "{clean}");
    assert!(!datapacks.join("workflow").exists());
}

#[test]
fn link_state_written_by_older_versions_is_read_from_the_project() {
    let temp_dir = TempDir::new().unwrap();
    let project = write_project(temp_dir.path());
    fs::create_dir_all(project.join(".cobble")).unwrap();
    fs::write(
        project.join(".cobble/link_state.json"),
        r#"{"version":1,"target_kind":"datapacks","target_path":"./../world/datapacks",
            "pack_name":"workflow","pack_path":"./../world/datapacks/workflow"}"#,
    )
    .unwrap();

    let status = succeed(
        cobble()
            .current_dir(temp_dir.path())
            .args(["link", "project", "--status"]),
    );

    let expected = temp_dir.path().join("world/datapacks/workflow");
    assert!(
        status.contains(&format!("Pack path: {}", expected.display())),
        "{status}"
    );
}

#[test]
fn a_file_input_finds_the_config_above_the_current_folder() {
    let temp_dir = TempDir::new().unwrap();
    let project = write_project(temp_dir.path());

    succeed(
        cobble()
            .current_dir(project.join("src"))
            .args(["build", "main.cbl", "-q"]),
    );

    assert!(project
        .join("output/data/workflow/function/hello.mcfunction")
        .exists());
    assert!(!project.join("src/output").exists());
}

#[test]
fn a_namespace_change_replaces_the_projects_own_output() {
    for validate in [false, true] {
        let temp_dir = TempDir::new().unwrap();
        let project = write_project(temp_dir.path());
        let tree = write_say_tree(temp_dir.path());
        let build = |project: &Path| {
            let mut command = cobble();
            command.current_dir(project).args(["build", "-q"]);
            if validate {
                command.arg("--validate").arg("--commands-json").arg(&tree);
            }
            succeed(&mut command);
        };
        build(&project);
        fs::write(project.join("cobble.toml"), config("renamed", "one")).unwrap();

        build(&project);

        let data = project.join("output/data");
        assert!(data.join("renamed/function/hello.mcfunction").exists());
        assert!(!data.join("workflow").exists());
    }
}

#[test]
fn a_moved_project_keeps_its_output() {
    for validate in [false, true] {
        let temp_dir = TempDir::new().unwrap();
        let project = write_project(temp_dir.path());
        succeed(cobble().current_dir(&project).args(["build", "-q"]));
        let moved = temp_dir.path().join("moved");
        fs::rename(&project, &moved).unwrap();

        let doctor = succeed(cobble().current_dir(&moved).args(["doctor", "--json"]));
        let report: serde_json::Value = serde_json::from_str(&doctor).unwrap();
        assert_eq!(
            report["experimental_output"]["marker"]["status"], "ok",
            "{doctor}"
        );

        let mut command = cobble();
        command.current_dir(&moved).args(["build", "-q"]);
        if validate {
            let tree = write_say_tree(temp_dir.path());
            command.arg("--validate").arg("--commands-json").arg(tree);
        }
        succeed(&mut command);

        fs::rename(&moved, &project).unwrap();
        succeed(cobble().current_dir(&project).arg("clean"));
        assert!(!project.join("output").exists());
    }
}

#[test]
fn a_pack_of_a_moved_project_outside_its_folder_stays_refused() {
    let temp_dir = TempDir::new().unwrap();
    let project = write_project(temp_dir.path());
    let output = temp_dir.path().join("shared");
    succeed(
        cobble()
            .current_dir(&project)
            .args(["build", "-q", "-o", "../shared"]),
    );
    let moved = temp_dir.path().join("moved");
    fs::rename(&project, &moved).unwrap();

    let refused = cobble()
        .current_dir(&moved)
        .args(["build", "-q", "-o", "../shared"])
        .output()
        .unwrap();

    assert!(!refused.status.success());
    assert!(text(&refused).contains("owned by another Cobble project"));
    assert!(output.join("data/workflow").exists());
}

fn wait_for(watch: &mut Child, ready: impl Fn() -> bool) {
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(20) {
        if ready() {
            return;
        }
        assert!(watch.try_wait().unwrap().is_none(), "watch exited early");
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = watch.kill();
    let _ = watch.wait();
    panic!("watch did not rebuild in time");
}

fn stop(mut watch: Child) {
    watch.kill().unwrap();
    watch.wait().unwrap();
}

/// Saves `contents` the way many editors do: a new file renamed over the old.
fn replace_file(path: &Path, contents: &str) {
    let temporary = path.with_extension("save.tmp");
    fs::write(&temporary, contents).unwrap();
    fs::rename(&temporary, path).unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn watch_keeps_seeing_files_that_editors_replace() {
    let temp_dir = TempDir::new().unwrap();
    let project = write_project(temp_dir.path());
    let meta = project.join("output/pack.mcmeta");
    let mut watch = cobble()
        .current_dir(&project)
        .arg("watch")
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    wait_for(&mut watch, || meta.exists());
    std::thread::sleep(Duration::from_millis(300));

    for description in ["two", "three", "four"] {
        replace_file(
            &project.join("cobble.toml"),
            &config("workflow", description),
        );
        wait_for(&mut watch, || {
            fs::read_to_string(&meta).is_ok_and(|meta| meta.contains(description))
        });
    }
    stop(watch);

    let function = |name: &str| {
        project
            .join("out/data/workflow/function")
            .join(format!("{name}.mcfunction"))
    };
    let mut watch = cobble()
        .current_dir(&project)
        .args(["watch", "src/main.cbl", "-o", "out"])
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    wait_for(&mut watch, || function("hello").exists());
    std::thread::sleep(Duration::from_millis(300));

    for name in ["first", "second", "third"] {
        let source = format!("def {name}():\n    /say hi\n");
        replace_file(&project.join("src/main.cbl"), &source);
        wait_for(&mut watch, || function(name).exists());
    }
    stop(watch);
}

#[cfg(target_os = "linux")]
#[test]
fn watch_follows_the_source_out_of_a_folder_with_rebuilt_output() {
    let temp_dir = TempDir::new().unwrap();
    let project = write_project(temp_dir.path());
    let with_source = |source: &str, description: &str| {
        config("workflow", description)
            .replace("source = \"src\"", &format!("source = \"{source}\""))
    };
    fs::write(
        project.join("cobble.toml"),
        with_source(".", "one").replace("\"main.cbl\"", "\"src/main.cbl\""),
    )
    .unwrap();
    let meta = project.join("output/pack.mcmeta");
    let mut watch = cobble()
        .current_dir(&project)
        .arg("watch")
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    wait_for(&mut watch, || meta.exists());
    std::thread::sleep(Duration::from_millis(300));

    // The first rebuild replaces folders under the watched source folder.
    fs::write(project.join("src/main.cbl"), "def a():\n    /say hi\n").unwrap();
    let a = project.join("output/data/workflow/function/a.mcfunction");
    wait_for(&mut watch, || a.exists());

    fs::write(project.join("cobble.toml"), with_source("src", "two")).unwrap();
    wait_for(&mut watch, || {
        fs::read_to_string(&meta).is_ok_and(|meta| meta.contains("two"))
    });
    std::thread::sleep(Duration::from_millis(300));

    fs::write(project.join("cobble.toml"), with_source("src", "three")).unwrap();
    wait_for(&mut watch, || {
        fs::read_to_string(&meta).is_ok_and(|meta| meta.contains("three"))
    });
    fs::write(project.join("src/main.cbl"), "def b():\n    /say hi\n").unwrap();
    let b = project.join("output/data/workflow/function/b.mcfunction");
    wait_for(&mut watch, || b.exists());
    stop(watch);
}

#[test]
fn a_continued_command_is_validated_as_one_command() {
    let temp_dir = TempDir::new().unwrap();
    let project = write_project(temp_dir.path());
    let tree = write_say_tree(temp_dir.path());
    let source = project.join("src/main.cbl");
    fs::write(&source, "def hello():\n    /say a \\\n        b\n").unwrap();
    let build = |project: &Path| {
        let mut command = cobble();
        command
            .current_dir(project)
            .args(["build", "-q", "--validate", "--commands-json"])
            .arg(&tree);
        command.output().unwrap()
    };

    let output = build(&project);
    assert!(output.status.success(), "{}", text(&output));
    assert_eq!(
        fs::read_to_string(project.join("output/data/workflow/function/hello.mcfunction")).unwrap(),
        "say a b\n"
    );

    fs::write(&source, "def hello():\n    /nope \\\n        b\n").unwrap();
    let output = build(&project);
    assert!(!output.status.success());
    assert!(text(&output).contains("| nope b\n"), "{}", text(&output));
}

#[test]
fn a_byte_order_mark_at_the_start_of_a_file_is_ignored() {
    let temp_dir = TempDir::new().unwrap();
    let project = write_project(temp_dir.path());
    fs::write(
        project.join("src/main.cbl"),
        "\u{feff}import other\n\ndef hello():\n    other_say()\n",
    )
    .unwrap();
    fs::write(
        project.join("src/other.cbl"),
        "\u{feff}def other_say():\n    /say other\n",
    )
    .unwrap();

    succeed(cobble().current_dir(&project).args(["build", "-q"]));
    succeed(cobble().current_dir(&project).args(["check"]));
    assert_eq!(
        fs::read_to_string(project.join("output/data/workflow/function/other_say.mcfunction"))
            .unwrap(),
        "say other\n"
    );

    // Columns on the first line don't count the mark.
    fs::write(project.join("src/main.cbl"), "\u{feff}x = (\n").unwrap();
    let output = cobble()
        .current_dir(&project)
        .args(["check", "--json"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!text(&output).contains('\u{feff}'), "{}", text(&output));
}
