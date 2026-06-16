use serde_json::Value;
use sha1::{Digest, Sha1};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub(crate) fn build_manifest_path(output_dir: &Path) -> PathBuf {
    output_dir.join(".cobble").join("build_manifest.json")
}

pub(crate) fn read_build_manifest(path: &Path) -> Result<Value, String> {
    let content = fs::read_to_string(path)
        .map_err(|error| format!("missing or unreadable .cobble/build_manifest.json ({error})"))?;
    let manifest = serde_json::from_str::<Value>(&content)
        .map_err(|error| format!("invalid .cobble/build_manifest.json ({error})"))?;
    if manifest.get("version").and_then(Value::as_u64) != Some(1) {
        return Err("unsupported .cobble/build_manifest.json version".to_string());
    }
    if manifest
        .get("cobble_version")
        .and_then(Value::as_str)
        .is_none()
    {
        return Err("missing cobble_version in .cobble/build_manifest.json".to_string());
    }
    if manifest.get("namespace").and_then(Value::as_str).is_none() {
        return Err("missing namespace in .cobble/build_manifest.json".to_string());
    }
    Ok(manifest)
}

pub(crate) fn require_manifest_namespace(
    manifest: &Value,
    expected_namespace: Option<&str>,
) -> Result<(), String> {
    let Some(expected_namespace) = expected_namespace else {
        return Ok(());
    };
    let actual_namespace = manifest
        .get("namespace")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing namespace in .cobble/build_manifest.json".to_string())?;
    if actual_namespace != expected_namespace {
        return Err(format!(
            "marker namespace `{actual_namespace}` does not match project namespace `{expected_namespace}`"
        ));
    }
    Ok(())
}

pub(crate) fn project_marker_identity(project_root: &Path) -> (String, String) {
    let canonical_root = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf());
    let marker_root = canonical_root.to_string_lossy().replace('\\', "/");
    let mut hasher = Sha1::new();
    hasher.update(marker_root.as_bytes());
    let project_id = format!("{:x}", hasher.finalize());
    (marker_root, project_id)
}

pub(crate) fn require_manifest_ownership(
    manifest: &Value,
    expected_namespace: Option<&str>,
    expected_project_id: Option<&str>,
) -> Result<(), String> {
    require_manifest_namespace(manifest, expected_namespace)?;
    let Some(expected_project_id) = expected_project_id else {
        return Ok(());
    };
    let actual_project_id = manifest
        .get("project_id")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing project_id in .cobble/build_manifest.json".to_string())?;
    if actual_project_id != expected_project_id {
        return Err(format!(
            "marker project_id `{actual_project_id}` does not match this project `{expected_project_id}`"
        ));
    }
    Ok(())
}

pub(crate) fn ensure_no_symlink_components(path: &Path, context: &str) -> Result<(), String> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(format!(
                    "Refusing to {context} through symlink: {}",
                    current.display()
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(format!(
                    "Failed to inspect {} while checking {context}: {error}",
                    current.display()
                ));
            }
        }
    }
    Ok(())
}

pub(crate) fn ensure_no_symlink_descendants(root: &Path, context: &str) -> Result<(), String> {
    if !root.exists() {
        return Ok(());
    }

    for entry in WalkDir::new(root).follow_links(false) {
        let entry = entry.map_err(|error| {
            format!(
                "Failed to inspect {} while checking {context}: {error}",
                root.display()
            )
        })?;
        if entry.file_type().is_symlink() {
            return Err(format!(
                "Refusing to {context} through symlink: {}",
                entry.path().display()
            ));
        }
    }

    Ok(())
}
