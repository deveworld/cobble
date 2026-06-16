use crate::commands::link::linked_output_path;
use crate::commands::output_safety::{
    build_manifest_path, ensure_no_symlink_components, ensure_no_symlink_descendants,
    project_marker_identity, read_build_manifest, require_manifest_ownership,
};
use crate::config::CobbleConfig;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub struct CleanOptions {
    pub path: Option<PathBuf>,
    pub output: Option<PathBuf>,
    pub dry_run: bool,
    pub linked: bool,
    pub yes: bool,
}

pub fn clean(options: CleanOptions) -> Result<(), String> {
    let (config, config_dir) = if let Some(config_path) = find_config(&options.path) {
        let config = CobbleConfig::load(&config_path)?;
        let config_dir = config_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        (Some(config), Some(config_dir))
    } else {
        (None, None)
    };

    if options.linked && options.output.is_some() {
        return Err("--linked cannot be combined with --output".to_string());
    }
    if options.linked && !options.dry_run && !options.yes {
        return Err(
            "`cobble clean --linked` requires --yes. Run `cobble clean --linked --dry-run` first."
                .to_string(),
        );
    }

    let expected_namespace = if options.linked {
        config
            .as_ref()
            .map(|config| config.project.namespace.as_str())
    } else {
        None
    };
    let expected_project_id = config_dir
        .as_deref()
        .filter(|_| options.linked || options.output.is_none())
        .map(|config_dir| project_marker_identity(config_dir).1);

    let output_dir = if options.linked {
        let config_dir = config_dir
            .as_deref()
            .ok_or_else(|| "No cobble.toml found for linked cleanup".to_string())?;
        linked_output_path(config_dir)?
    } else if let Some(output) = options.output {
        output
    } else if let (Some(config), Some(config_dir)) = (&config, &config_dir) {
        config_dir.join(&config.build.output)
    } else {
        return Err("No output specified and no cobble.toml found".to_string());
    };

    let plan = inspect_clean_target(
        &output_dir,
        config_dir.as_deref(),
        expected_namespace,
        expected_project_id.as_deref(),
    )?;
    if !plan.exists {
        println!("Nothing to clean: {}", plan.output_dir.display());
        return Ok(());
    }

    if options.dry_run {
        println!(
            "Would remove Cobble output: {} ({} entr{})",
            plan.output_dir.display(),
            plan.entry_count,
            if plan.entry_count == 1 { "y" } else { "ies" }
        );
        println!("Safety checks:");
        println!("  Marker: {}", plan.marker_path.display());
        if let Some(namespace) = &plan.marker_namespace {
            println!("  Namespace: {namespace}");
        }
        if let Some(project_id) = &plan.marker_project_id {
            println!("  Project id: {project_id}");
        }
        println!("  Required files: pack.mcmeta, data/");
        println!("  Symlinks: none found in output path or descendants");
        if !plan.generated_namespaces.is_empty() {
            println!(
                "Generated namespaces: {}",
                plan.generated_namespaces.join(", ")
            );
        }
        if options.linked {
            println!("Next step: run `cobble clean --linked --yes` to remove this linked output.");
        } else {
            println!("Next step: rerun without --dry-run to remove this output.");
        }
        return Ok(());
    }

    fs::remove_dir_all(&plan.output_dir)
        .map_err(|error| format!("Failed to remove {}: {error}", plan.output_dir.display()))?;
    println!("Removed Cobble output: {}", plan.output_dir.display());
    Ok(())
}

#[derive(Debug)]
struct CleanPlan {
    output_dir: PathBuf,
    exists: bool,
    entry_count: usize,
    marker_path: PathBuf,
    marker_namespace: Option<String>,
    marker_project_id: Option<String>,
    generated_namespaces: Vec<String>,
}

fn inspect_clean_target(
    output_dir: &Path,
    config_dir: Option<&Path>,
    expected_namespace: Option<&str>,
    expected_project_id: Option<&str>,
) -> Result<CleanPlan, String> {
    if !output_dir.exists() {
        return Ok(CleanPlan {
            output_dir: output_dir.to_path_buf(),
            exists: false,
            entry_count: 0,
            marker_path: build_manifest_path(output_dir),
            marker_namespace: None,
            marker_project_id: None,
            generated_namespaces: Vec::new(),
        });
    }

    ensure_no_symlink_components(output_dir, "clean")?;
    let metadata = fs::symlink_metadata(output_dir)
        .map_err(|error| format!("Failed to inspect {}: {error}", output_dir.display()))?;
    if metadata.file_type().is_symlink() {
        return Err(format!(
            "Refusing to clean symlink output path: {}",
            output_dir.display()
        ));
    }
    if !metadata.is_dir() {
        return Err(format!(
            "Refusing to clean non-directory output path: {}",
            output_dir.display()
        ));
    }
    ensure_no_symlink_descendants(output_dir, "clean")?;

    if let Some(config_dir) = config_dir {
        let output_canonical = output_dir
            .canonicalize()
            .map_err(|error| format!("Failed to resolve {}: {error}", output_dir.display()))?;
        let config_canonical = config_dir
            .canonicalize()
            .map_err(|error| format!("Failed to resolve {}: {error}", config_dir.display()))?;
        if output_canonical == config_canonical {
            return Err(format!(
                "Refusing to clean project root as output: {}",
                output_dir.display()
            ));
        }
    }

    let manifest_path = build_manifest_path(output_dir);
    let manifest = read_build_manifest(&manifest_path)
        .and_then(|manifest| {
            require_manifest_ownership(&manifest, expected_namespace, expected_project_id)?;
            Ok(manifest)
        })
        .map_err(|error| format!("Refusing to clean {}: {}", output_dir.display(), error))?;
    require_path(output_dir.join("pack.mcmeta"), "pack.mcmeta", output_dir)?;
    require_path(output_dir.join("data"), "data directory", output_dir)?;

    let entry_count = WalkDir::new(output_dir)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .count();
    let generated_namespaces = manifest
        .get("generated_namespaces")
        .and_then(Value::as_array)
        .map(|namespaces| {
            namespaces
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let marker_namespace = manifest_string(&manifest, "namespace");
    let marker_project_id = manifest_string(&manifest, "project_id");

    Ok(CleanPlan {
        output_dir: output_dir.to_path_buf(),
        exists: true,
        entry_count,
        marker_path: manifest_path,
        marker_namespace,
        marker_project_id,
        generated_namespaces,
    })
}

fn manifest_string(manifest: &Value, field: &str) -> Option<String> {
    manifest
        .get(field)
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn require_path(path: PathBuf, label: &str, output_dir: &Path) -> Result<(), String> {
    if path.exists() {
        Ok(())
    } else {
        Err(format!(
            "Refusing to clean {}: missing {}",
            output_dir.display(),
            label
        ))
    }
}

fn find_config(path: &Option<PathBuf>) -> Option<PathBuf> {
    if let Some(path) = path {
        if path.is_file() {
            if let Some(parent) = path.parent() {
                return CobbleConfig::find_in_path(parent);
            }
        } else {
            return CobbleConfig::find_in_path(path);
        }
    }
    CobbleConfig::find_in_path(".")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_marked_output(output_dir: &Path) {
        fs::create_dir_all(output_dir.join(".cobble")).unwrap();
        fs::create_dir_all(output_dir.join("data/example/function")).unwrap();
        fs::write(output_dir.join("pack.mcmeta"), "{}").unwrap();
        fs::write(
            output_dir.join(".cobble/build_manifest.json"),
            r#"{
  "version": 1,
  "cobble_version": "0.7.0",
  "namespace": "example",
  "generated_namespaces": ["example"]
}"#,
        )
        .unwrap();
    }

    #[test]
    fn inspect_clean_target_accepts_marked_output() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let output_dir = temp_dir.path().join("output");
        write_marked_output(&output_dir);

        let plan = inspect_clean_target(&output_dir, None, None, None).unwrap();

        assert!(plan.exists);
        assert_eq!(plan.generated_namespaces, vec!["example"]);
        assert!(plan.entry_count > 0);
    }

    #[test]
    fn inspect_clean_target_rejects_unmarked_output() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let output_dir = temp_dir.path().join("output");
        fs::create_dir_all(&output_dir).unwrap();

        let error = inspect_clean_target(&output_dir, None, None, None).unwrap_err();

        assert!(error.contains("Refusing to clean"));
        assert!(output_dir.exists());
    }

    #[test]
    fn inspect_clean_target_rejects_mismatched_expected_namespace() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let output_dir = temp_dir.path().join("output");
        write_marked_output(&output_dir);

        let error =
            inspect_clean_target(&output_dir, None, Some("other_namespace"), None).unwrap_err();

        assert!(error.contains("marker namespace `example`"));
        assert!(error.contains("project namespace `other_namespace`"));
        assert!(output_dir.exists());
    }

    #[test]
    fn inspect_clean_target_rejects_mismatched_project_id() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let output_dir = temp_dir.path().join("output");
        write_marked_output(&output_dir);
        fs::write(
            output_dir.join(".cobble/build_manifest.json"),
            r#"{
  "version": 1,
  "cobble_version": "0.7.1",
  "namespace": "example",
  "project_id": "other-project",
  "generated_namespaces": ["example"]
}"#,
        )
        .unwrap();

        let error = inspect_clean_target(&output_dir, None, Some("example"), Some("project-id"))
            .unwrap_err();

        assert!(error.contains("marker project_id `other-project`"));
        assert!(error.contains("this project `project-id`"));
        assert!(output_dir.exists());
    }

    #[cfg(unix)]
    #[test]
    fn inspect_clean_target_rejects_symlink_parent_component() {
        use std::os::unix::fs::symlink;

        let temp_dir = tempfile::TempDir::new().unwrap();
        let real_parent = temp_dir.path().join("real-parent");
        let symlink_parent = temp_dir.path().join("symlink-parent");
        let output_dir = real_parent.join("output");
        write_marked_output(&output_dir);
        symlink(&real_parent, &symlink_parent).unwrap();

        let error =
            inspect_clean_target(&symlink_parent.join("output"), None, None, None).unwrap_err();

        assert!(error.contains("Refusing to clean through symlink"));
        assert!(output_dir.exists());
    }

    #[cfg(unix)]
    #[test]
    fn inspect_clean_target_rejects_symlink_descendant() {
        use std::os::unix::fs::symlink;

        let temp_dir = tempfile::TempDir::new().unwrap();
        let output_dir = temp_dir.path().join("output");
        let outside_dir = temp_dir.path().join("outside");
        write_marked_output(&output_dir);
        fs::create_dir_all(&outside_dir).unwrap();
        fs::write(outside_dir.join("important.txt"), "keep\n").unwrap();
        symlink(&outside_dir, output_dir.join("data/example/function/leak")).unwrap();

        let error = inspect_clean_target(&output_dir, None, None, None).unwrap_err();

        assert!(error.contains("Refusing to clean through symlink"));
        assert!(output_dir.exists());
        assert_eq!(
            fs::read_to_string(outside_dir.join("important.txt")).unwrap(),
            "keep\n"
        );
    }
}
