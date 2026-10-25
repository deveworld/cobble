use serde_json::Value;
use sha1::{Digest, Sha1};
use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};
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

/// Refuses a symlink at `root` or at any component of `path` below it.
///
/// Symlinks above `root` are allowed. They are part of where the user put the
/// project or the output, such as `/tmp` on macOS, a linked home folder, or a
/// `.minecraft` folder moved to another drive, and `build.output` can already
/// name any folder. A symlink at or inside `root` could send a write or a
/// delete somewhere else, so those are refused. When `path` is not under
/// `root`, every component of `path` is checked.
pub(crate) fn ensure_no_symlinks_from(
    root: &Path,
    path: &Path,
    context: &str,
) -> Result<(), String> {
    // `out/` and `out/.` resolve the link when inspected as written, so look
    // at the normalized paths (`components` drops a trailing `/` and `.`). A
    // root with `..` names a folder only after resolving the parts before it,
    // so check every component of the path instead.
    let root: PathBuf = root.components().collect();
    let path: PathBuf = path.components().collect();
    if root
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return ensure_no_symlink_components(&path, context);
    }
    let Ok(below_root) = path.strip_prefix(&root) else {
        return ensure_no_symlink_components(&path, context);
    };
    let mut current = root.clone();
    if !inspect_component(&current, context)? {
        return Ok(());
    }
    for component in below_root.components() {
        current.push(component.as_os_str());
        if !inspect_component(&current, context)? {
            return Ok(());
        }
    }
    Ok(())
}

/// Refuses `path` when it is a symlink itself. Symlinks above it are allowed;
/// see [`ensure_no_symlinks_from`].
pub(crate) fn ensure_not_symlink(path: &Path, context: &str) -> Result<(), String> {
    ensure_no_symlinks_from(path, path, context)
}

fn ensure_no_symlink_components(path: &Path, context: &str) -> Result<(), String> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        if !inspect_component(&current, context)? {
            return Ok(());
        }
    }
    Ok(())
}

/// Returns whether `path` exists, or an error when it is a symlink.
fn inspect_component(path: &Path, context: &str) -> Result<bool, String> {
    if path.as_os_str().is_empty() {
        return Ok(true);
    }
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(format!(
            "Refusing to {context} through symlink: {}",
            path.display()
        )),
        Ok(_) => Ok(true),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!(
            "Failed to inspect {} while checking {context}: {error}",
            path.display()
        )),
    }
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

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn symlinks_above_the_root_are_allowed_and_at_or_below_it_refused() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let real = temp_dir.path().join("real");
        fs::create_dir_all(real.join("pack/data")).unwrap();
        let linked_parent = temp_dir.path().join("linked");
        symlink(&real, &linked_parent).unwrap();

        let root = linked_parent.join("pack");
        ensure_no_symlinks_from(&root, &root.join("data/file.json"), "build").unwrap();
        ensure_not_symlink(&root, "build").unwrap();

        let linked_root = temp_dir.path().join("linked-pack");
        symlink(real.join("pack"), &linked_root).unwrap();
        let error = ensure_not_symlink(&linked_root, "build").unwrap_err();
        assert!(
            error.contains("Refusing to build through symlink"),
            "{error}"
        );

        symlink(&real, real.join("pack/data/escape")).unwrap();
        let error = ensure_no_symlinks_from(&root, &root.join("data/escape/file.json"), "build")
            .unwrap_err();
        assert!(error.contains("data/escape"), "{error}");
    }

    #[test]
    fn trailing_separators_and_dots_do_not_hide_a_symlinked_root() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let real = temp_dir.path().join("real");
        fs::create_dir_all(real.join("pack")).unwrap();
        let linked = temp_dir.path().join("linked");
        symlink(real.join("pack"), &linked).unwrap();

        let linked_text = linked.display().to_string();
        for spelled in [
            format!("{linked_text}/"),
            format!("{linked_text}/."),
            format!("{linked_text}/.."),
            format!("{linked_text}/./"),
        ] {
            let error = ensure_not_symlink(Path::new(&spelled), "clean")
                .expect_err(&format!("{spelled} must be refused"));
            assert!(
                error.contains("Refusing to clean through symlink"),
                "{error}"
            );
        }
        // A plain folder spelled with a trailing separator is still fine.
        ensure_not_symlink(Path::new(&format!("{}/", real.display())), "clean").unwrap();
    }

    #[test]
    fn paths_outside_the_root_check_every_component() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let real = temp_dir.path().join("real");
        fs::create_dir_all(&real).unwrap();
        let linked = temp_dir.path().join("linked");
        symlink(&real, &linked).unwrap();

        let error = ensure_no_symlinks_from(
            &temp_dir.path().join("elsewhere"),
            &linked.join("file.json"),
            "write",
        )
        .unwrap_err();
        assert!(
            error.contains("Refusing to write through symlink"),
            "{error}"
        );
    }
}
