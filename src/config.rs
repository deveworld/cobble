use crate::fs_safety::write_file_atomic;
use crate::pack_format::{COBBLE_VERSION, SUPPORTED_MINECRAFT_VERSION, SUPPORTED_PACK_FORMAT};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CobbleConfig {
    pub project: ProjectConfig,
    #[serde(default)]
    pub build: BuildConfig,
    #[serde(default)]
    pub stdlib: StdlibConfig,
    #[serde(default)]
    pub experimental: ExperimentalConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectConfig {
    pub name: String,
    pub description: String,
    pub namespace: String,
    #[serde(default = "default_version")]
    pub version: String,
    #[serde(default = "default_pack_format")]
    pub pack_format: String, // Changed from u8 to String to support decimal formats
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct BuildConfig {
    #[serde(default = "default_source")]
    pub source: String,
    #[serde(default = "default_output")]
    pub output: String,
    #[serde(default)]
    pub entry_points: Vec<String>,
}

/// Standard library configuration.
///
/// `version = 2` (default) enables per-module opt-in via
/// `from stdlib import text, score, ...`. `version = 1` keeps the 0.7.x
/// behavior where every module is always active.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StdlibConfig {
    #[serde(default = "default_stdlib_version")]
    pub version: u8,
}

impl Default for StdlibConfig {
    fn default() -> Self {
        Self {
            version: default_stdlib_version(),
        }
    }
}

/// Experimental feature flags. All default off.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ExperimentalConfig {
    #[serde(default)]
    pub resource_pack: bool,
    #[serde(default)]
    pub plugins: bool,
    #[serde(default)]
    pub python_compat: bool,
}

fn default_version() -> String {
    "1.0.0".to_string()
}

fn default_stdlib_version() -> u8 {
    2
}

fn default_pack_format() -> String {
    SUPPORTED_PACK_FORMAT.to_string()
}

fn default_source() -> String {
    "src".to_string()
}

fn default_output() -> String {
    "output".to_string()
}

impl CobbleConfig {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let config = Self::load_unvalidated(path)?;
        config.validate_pack_format()?;
        Ok(config)
    }

    pub fn load_unvalidated<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let contents =
            fs::read_to_string(path).map_err(|e| format!("Failed to read config file: {}", e))?;
        let config: Self =
            toml::from_str(&contents).map_err(|e| format!("Failed to parse config file: {}", e))?;
        config.validate_stdlib_version()?;
        Ok(config)
    }

    pub fn validate_pack_format(&self) -> Result<(), String> {
        // Validate pack_format
        let pack_format = crate::pack_format::PackFormat::parse_format(&self.project.pack_format)
            .map_err(|e| {
            format!("Invalid pack_format '{}': {}", self.project.pack_format, e)
        })?;

        if !pack_format.is_supported() {
            return Err(format!(
                "Invalid pack_format: {}. Must be {} (Minecraft Java Edition {}).\n\
                 \n\
                 Cobble v{} exclusively supports Minecraft Java Edition {}.\n\
                 See https://minecraft.wiki/w/Pack_format for version compatibility.\n\
                 \n\
                 Update your cobble.toml:\n\
                 [project]\n\
                 pack_format = \"{}\"",
                self.project.pack_format,
                SUPPORTED_PACK_FORMAT,
                SUPPORTED_MINECRAFT_VERSION,
                COBBLE_VERSION,
                SUPPORTED_MINECRAFT_VERSION,
                SUPPORTED_PACK_FORMAT
            ));
        }

        Ok(())
    }

    pub fn validate_stdlib_version(&self) -> Result<(), String> {
        if matches!(self.stdlib.version, 1 | 2) {
            return Ok(());
        }

        Err(format!(
            "Invalid stdlib.version: {}. Supported values are 1 or 2.",
            self.stdlib.version
        ))
    }

    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<(), String> {
        let contents = toml::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize config: {}", e))?;
        write_file_atomic(path.as_ref(), contents)
            .map_err(|e| format!("Failed to write config file: {}", e))
    }

    pub fn default_with_name(name: String) -> Self {
        Self {
            project: ProjectConfig {
                namespace: name.to_lowercase().replace(" ", "_").replace("-", "_"),
                description: format!("{} Data Pack", name),
                name,
                version: default_version(),
                pack_format: default_pack_format(),
            },
            build: BuildConfig {
                source: default_source(),
                output: default_output(),
                entry_points: vec![],
            },
            stdlib: StdlibConfig::default(),
            experimental: ExperimentalConfig::default(),
        }
    }

    pub fn find_in_path<P: AsRef<Path>>(path: P) -> Option<PathBuf> {
        const MAX_DEPTH: usize = 100;
        Self::find_in_path_with_depth(path, 0, MAX_DEPTH)
    }

    fn find_in_path_with_depth<P: AsRef<Path>>(
        path: P,
        depth: usize,
        max_depth: usize,
    ) -> Option<PathBuf> {
        if depth > max_depth {
            return None;
        }

        let path = path.as_ref();
        let config_file = path.join("cobble.toml");

        if config_file.exists() {
            return Some(config_file);
        }

        // Search in parent directories
        if let Some(parent) = path.parent() {
            if parent != path {
                return Self::find_in_path_with_depth(parent, depth + 1, max_depth);
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::CobbleConfig;
    use std::fs;

    #[test]
    fn load_rejects_unsupported_stdlib_versions() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let config_path = temp_dir.path().join("cobble.toml");
        fs::write(
            &config_path,
            r#"
[project]
name = "bad_stdlib"
description = "Bad stdlib"
namespace = "bad_stdlib"
pack_format = "101.1"

[stdlib]
version = 3
"#,
        )
        .unwrap();

        let error = CobbleConfig::load_unvalidated(&config_path).unwrap_err();
        assert!(error.contains("Invalid stdlib.version: 3"));
    }
}
