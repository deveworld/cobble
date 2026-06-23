use super::output_safety::{
    build_manifest_path, ensure_no_symlink_components, project_marker_identity,
    read_build_manifest, require_manifest_ownership,
};
use crate::config::CobbleConfig;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Component, Path, PathBuf};

pub struct LinkOptions {
    pub project_path: Option<PathBuf>,
    pub datapacks: Option<PathBuf>,
    pub world: Option<PathBuf>,
    pub minecraft: Option<PathBuf>,
    pub pack_name: Option<String>,
    pub dry_run: bool,
    pub clear: bool,
    pub status: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct LinkState {
    pub version: u8,
    pub target_kind: String,
    pub target_path: String,
    pub pack_name: String,
    pub pack_path: String,
}

pub fn link(options: LinkOptions) -> Result<(), String> {
    let (config, config_dir) = load_project(&options.project_path)?;

    if options.status {
        print_link_status(&config, &config_dir)?;
        return Ok(());
    }

    if options.clear {
        clear_link_state(&config_dir)?;
        return Ok(());
    }

    let state = build_link_state(&config, &config_dir, &options)?;
    if options.dry_run {
        println!("Would configure Cobble link:");
        println!("  Target kind: {}", state.target_kind);
        println!("  Target path: {}", state.target_path);
        println!("  Pack name: {}", state.pack_name);
        println!("  Pack path: {}", state.pack_path);
        return Ok(());
    }

    let target_path = PathBuf::from(&state.target_path);
    ensure_safe_directory_target(&target_path)?;
    fs::create_dir_all(&target_path)
        .map_err(|error| format!("Failed to create {}: {error}", target_path.display()))?;

    write_link_state(&config_dir, &state)?;
    println!("Configured Cobble link:");
    println!("  Target kind: {}", state.target_kind);
    println!("  Target path: {}", state.target_path);
    println!("  Pack path: {}", state.pack_path);
    println!("Use `cobble watch --link` to build into the linked target.");
    Ok(())
}

pub(crate) fn linked_output_path(config_dir: &Path) -> Result<PathBuf, String> {
    let state = read_link_state(config_dir)?.ok_or_else(|| {
        "No Cobble link configured. Run `cobble link --datapacks <DIR>` first.".to_string()
    })?;
    validate_link_state_paths(&state)?;
    Ok(PathBuf::from(&state.pack_path))
}

pub(crate) fn validate_link_state_paths(state: &LinkState) -> Result<(), String> {
    validate_pack_name(&state.pack_name)?;
    let target_path = PathBuf::from(&state.target_path);
    let pack_path = PathBuf::from(&state.pack_path);
    ensure_safe_directory_target(&target_path)?;
    ensure_pack_path_is_under_target(state, &target_path, &pack_path)?;
    Ok(())
}

fn load_project(project_path: &Option<PathBuf>) -> Result<(CobbleConfig, PathBuf), String> {
    let config_path = find_config(project_path)
        .ok_or_else(|| "No cobble.toml found for link command".to_string())?;
    let config = CobbleConfig::load(&config_path)?;
    let config_dir = config_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    Ok((config, config_dir))
}

fn build_link_state(
    config: &CobbleConfig,
    config_dir: &Path,
    options: &LinkOptions,
) -> Result<LinkState, String> {
    let explicit_targets = [
        options.datapacks.is_some(),
        options.world.is_some(),
        options.minecraft.is_some(),
    ]
    .into_iter()
    .filter(|present| *present)
    .count();
    if explicit_targets != 1 {
        return Err(
            "Specify exactly one link target: --datapacks <DIR>, --world <DIR>, or --minecraft <DIR>"
                .to_string(),
        );
    }

    let pack_name = options
        .pack_name
        .clone()
        .unwrap_or_else(|| config.project.namespace.clone());
    validate_pack_name(&pack_name)?;

    let (target_kind, target_path) = if let Some(datapacks) = &options.datapacks {
        ("datapacks", resolve_path(config_dir, datapacks))
    } else if let Some(world) = &options.world {
        ("world", resolve_path(config_dir, world).join("datapacks"))
    } else if let Some(minecraft) = &options.minecraft {
        (
            "minecraft",
            resolve_path(config_dir, minecraft)
                .join("saves")
                .join(&pack_name)
                .join("datapacks"),
        )
    } else {
        unreachable!("explicit target count checked above");
    };

    let pack_path = target_path.join(&pack_name);
    Ok(LinkState {
        version: 1,
        target_kind: target_kind.to_string(),
        target_path: path_display(&target_path),
        pack_name,
        pack_path: path_display(&pack_path),
    })
}

fn print_link_status(config: &CobbleConfig, config_dir: &Path) -> Result<(), String> {
    match read_link_state(config_dir)? {
        Some(state) => {
            println!("Cobble link configured");
            println!("  Target kind: {}", state.target_kind);
            println!("  Target path: {}", state.target_path);
            println!("  Pack name: {}", state.pack_name);
            println!("  Pack path: {}", state.pack_path);
            if let Err(error) = validate_link_state_paths(&state) {
                println!("  Link state: invalid; {error}");
                println!("  Marker: not checked");
                println!("  Recovery: run `cobble link --clear` and configure the link again.");
                return Ok(());
            }
            let pack_path = PathBuf::from(&state.pack_path);
            let marker_path = build_manifest_path(&pack_path);
            let (_, project_id) = project_marker_identity(config_dir);
            match read_build_manifest(&marker_path).and_then(|manifest| {
                require_manifest_ownership(
                    &manifest,
                    Some(&config.project.namespace),
                    Some(&project_id),
                )
            }) {
                Ok(()) => println!("  Marker: present"),
                Err(error) if !marker_path.exists() => {
                    println!(
                        "  Marker: missing; run `cobble watch --link` or build to the linked path"
                    );
                    println!("  Marker detail: {error}");
                    println!("  Recovery: run `cobble watch --link` to create the linked pack.");
                }
                Err(error) => {
                    println!("  Marker: invalid; {error}");
                    println!(
                        "  Recovery: move the existing pack aside or rebuild it from the owning Cobble project."
                    );
                }
            }
        }
        None => {
            println!("No Cobble link configured");
            println!("  Recovery: run `cobble link --datapacks <DIR>` to configure one.");
        }
    }
    Ok(())
}

fn clear_link_state(config_dir: &Path) -> Result<(), String> {
    let path = link_state_path(config_dir);
    ensure_no_symlink_components(&path, "clear link state")?;
    if path.exists() {
        fs::remove_file(&path)
            .map_err(|error| format!("Failed to remove {}: {error}", path.display()))?;
        println!("Cleared Cobble link state");
    } else {
        println!("No Cobble link configured");
    }
    Ok(())
}

fn write_link_state(config_dir: &Path, state: &LinkState) -> Result<(), String> {
    let path = link_state_path(config_dir);
    ensure_no_symlink_components(&path, "write link state")?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Failed to create {}: {error}", parent.display()))?;
    }
    let content = serde_json::to_string_pretty(state)
        .map_err(|error| format!("Failed to serialize link state: {error}"))?;
    fs::write(&path, content)
        .map_err(|error| format!("Failed to write {}: {error}", path.display()))
}

pub(crate) fn read_link_state(config_dir: &Path) -> Result<Option<LinkState>, String> {
    let path = link_state_path(config_dir);
    ensure_no_symlink_components(&path, "read link state")?;
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(&path)
        .map_err(|error| format!("Failed to read {}: {error}", path.display()))?;
    let state = serde_json::from_str::<LinkState>(&content)
        .map_err(|error| format!("Failed to parse {}: {error}", path.display()))?;
    if state.version != 1 {
        return Err(format!(
            "Unsupported link state version {} in {}",
            state.version,
            path.display()
        ));
    }
    Ok(Some(state))
}

pub(crate) fn link_state_path(config_dir: &Path) -> PathBuf {
    config_dir.join(".cobble").join("link_state.json")
}

fn ensure_safe_directory_target(path: &Path) -> Result<(), String> {
    ensure_no_symlink_components(path, "link")?;
    if path.exists() {
        let metadata = fs::symlink_metadata(path)
            .map_err(|error| format!("Failed to inspect {}: {error}", path.display()))?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "Refusing to link through symlink: {}",
                path.display()
            ));
        }
        if !metadata.is_dir() {
            return Err(format!(
                "Link target must be a directory: {}",
                path.display()
            ));
        }
    }
    Ok(())
}

fn validate_pack_name(pack_name: &str) -> Result<(), String> {
    if pack_name.is_empty() {
        return Err("Linked pack name cannot be empty".to_string());
    }
    if pack_name.contains('/') || pack_name.contains('\\') || pack_name == "." || pack_name == ".."
    {
        return Err(format!("Invalid linked pack name: {pack_name}"));
    }
    Ok(())
}

fn ensure_pack_path_is_under_target(
    state: &LinkState,
    target_path: &Path,
    pack_path: &Path,
) -> Result<(), String> {
    let normalized_target = normalize_path(target_path);
    let normalized_pack = normalize_path(pack_path);
    if !normalized_pack.starts_with(&normalized_target) {
        return Err(format!(
            "Linked pack path {} is outside target datapacks directory {}",
            pack_path.display(),
            target_path.display()
        ));
    }
    if normalized_pack.file_name().and_then(|name| name.to_str()) != Some(&state.pack_name) {
        return Err(format!(
            "Linked pack path {} does not end with pack name {}",
            pack_path.display(),
            state.pack_name
        ));
    }
    Ok(())
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    normalized.push(component.as_os_str());
                }
            }
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str())
            }
        }
    }
    normalized
}

fn resolve_path(config_dir: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        config_dir.join(path)
    }
}

fn path_display(path: &Path) -> String {
    path.to_string_lossy().into_owned()
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

    fn config() -> CobbleConfig {
        CobbleConfig::default_with_name("linked_pack".to_string())
    }

    #[test]
    fn build_link_state_targets_datapacks_directory() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let state = build_link_state(
            &config(),
            temp_dir.path(),
            &LinkOptions {
                project_path: None,
                datapacks: Some(PathBuf::from("world/datapacks")),
                world: None,
                minecraft: None,
                pack_name: None,
                dry_run: false,
                clear: false,
                status: false,
            },
        )
        .unwrap();

        assert_eq!(state.target_kind, "datapacks");
        assert_eq!(state.pack_name, "linked_pack");
        assert!(state.target_path.ends_with("world/datapacks"));
        assert!(state.pack_path.ends_with("world/datapacks/linked_pack"));
    }

    #[test]
    fn build_link_state_targets_world_datapacks_directory() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let state = build_link_state(
            &config(),
            temp_dir.path(),
            &LinkOptions {
                project_path: None,
                datapacks: None,
                world: Some(PathBuf::from("saves/test_world")),
                minecraft: None,
                pack_name: Some("world_pack".to_string()),
                dry_run: false,
                clear: false,
                status: false,
            },
        )
        .unwrap();

        assert_eq!(state.target_kind, "world");
        assert_eq!(state.pack_name, "world_pack");
        assert!(state.target_path.ends_with("saves/test_world/datapacks"));
        assert!(state
            .pack_path
            .ends_with("saves/test_world/datapacks/world_pack"));
    }

    #[test]
    fn build_link_state_targets_minecraft_save_datapacks_directory() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let state = build_link_state(
            &config(),
            temp_dir.path(),
            &LinkOptions {
                project_path: None,
                datapacks: None,
                world: None,
                minecraft: Some(PathBuf::from(".minecraft")),
                pack_name: Some("dev_pack".to_string()),
                dry_run: false,
                clear: false,
                status: false,
            },
        )
        .unwrap();

        assert_eq!(state.target_kind, "minecraft");
        assert_eq!(state.pack_name, "dev_pack");
        assert!(state
            .target_path
            .ends_with(".minecraft/saves/dev_pack/datapacks"));
        assert!(state
            .pack_path
            .ends_with(".minecraft/saves/dev_pack/datapacks/dev_pack"));
    }

    #[test]
    fn build_link_state_rejects_multiple_targets() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let error = build_link_state(
            &config(),
            temp_dir.path(),
            &LinkOptions {
                project_path: None,
                datapacks: Some(PathBuf::from("datapacks")),
                world: Some(PathBuf::from("world")),
                minecraft: None,
                pack_name: None,
                dry_run: false,
                clear: false,
                status: false,
            },
        )
        .unwrap_err();

        assert!(error.contains("Specify exactly one link target"));
    }

    #[test]
    fn write_and_read_link_state_round_trip() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let state = LinkState {
            version: 1,
            target_kind: "datapacks".to_string(),
            target_path: "world/datapacks".to_string(),
            pack_name: "linked_pack".to_string(),
            pack_path: "world/datapacks/linked_pack".to_string(),
        };

        write_link_state(temp_dir.path(), &state).unwrap();

        assert_eq!(read_link_state(temp_dir.path()).unwrap(), Some(state));
    }

    #[cfg(unix)]
    #[test]
    fn link_state_operations_reject_symlink_state_directory() {
        use std::os::unix::fs::symlink;

        let temp_dir = tempfile::TempDir::new().unwrap();
        let outside_dir = temp_dir.path().join("outside");
        let state_path = outside_dir.join("link_state.json");
        fs::create_dir_all(&outside_dir).unwrap();
        symlink(&outside_dir, temp_dir.path().join(".cobble")).unwrap();

        let state = LinkState {
            version: 1,
            target_kind: "datapacks".to_string(),
            target_path: "world/datapacks".to_string(),
            pack_name: "linked_pack".to_string(),
            pack_path: "world/datapacks/linked_pack".to_string(),
        };

        let write_error = write_link_state(temp_dir.path(), &state).unwrap_err();
        assert!(write_error.contains("Refusing to write link state through symlink"));
        assert!(!state_path.exists());

        fs::write(&state_path, serde_json::to_string(&state).unwrap()).unwrap();
        let read_error = read_link_state(temp_dir.path()).unwrap_err();
        assert!(read_error.contains("Refusing to read link state through symlink"));

        let clear_error = clear_link_state(temp_dir.path()).unwrap_err();
        assert!(clear_error.contains("Refusing to clear link state through symlink"));
        assert!(state_path.exists());
    }

    #[test]
    fn linked_output_path_rejects_tampered_pack_path_outside_target() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let state = LinkState {
            version: 1,
            target_kind: "datapacks".to_string(),
            target_path: temp_dir
                .path()
                .join("world/datapacks")
                .display()
                .to_string(),
            pack_name: "linked_pack".to_string(),
            pack_path: temp_dir
                .path()
                .join("outside/linked_pack")
                .display()
                .to_string(),
        };
        write_link_state(temp_dir.path(), &state).unwrap();

        let error = linked_output_path(temp_dir.path()).unwrap_err();

        assert!(error.contains("outside target datapacks directory"));
    }

    #[test]
    fn linked_output_path_rejects_tampered_pack_path_name() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let target_path = temp_dir.path().join("world/datapacks");
        let state = LinkState {
            version: 1,
            target_kind: "datapacks".to_string(),
            target_path: target_path.display().to_string(),
            pack_name: "linked_pack".to_string(),
            pack_path: target_path.join("other_pack").display().to_string(),
        };
        write_link_state(temp_dir.path(), &state).unwrap();

        let error = linked_output_path(temp_dir.path()).unwrap_err();

        assert!(error.contains("does not end with pack name"));
    }

    #[cfg(unix)]
    #[test]
    fn linked_output_path_rejects_symlink_target_component() {
        use std::os::unix::fs::symlink;

        let temp_dir = tempfile::TempDir::new().unwrap();
        let real_target = temp_dir.path().join("real-datapacks");
        let symlink_target = temp_dir.path().join("symlink-datapacks");
        fs::create_dir_all(&real_target).unwrap();
        symlink(&real_target, &symlink_target).unwrap();
        let state = LinkState {
            version: 1,
            target_kind: "datapacks".to_string(),
            target_path: symlink_target.display().to_string(),
            pack_name: "linked_pack".to_string(),
            pack_path: symlink_target.join("linked_pack").display().to_string(),
        };
        write_link_state(temp_dir.path(), &state).unwrap();

        let error = linked_output_path(temp_dir.path()).unwrap_err();

        assert!(error.contains("Refusing to link through symlink"));
    }
}
