use crate::pack_format::{SUPPORTED_MINECRAFT_VERSION, SUPPORTED_PACK_FORMAT};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CobbleConfig {
    pub project: ProjectConfig,
    #[serde(default)]
    pub build: BuildConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
pub struct BuildConfig {
    #[serde(default = "default_source")]
    pub source: String,
    #[serde(default = "default_output")]
    pub output: String,
    #[serde(default)]
    pub entry_points: Vec<String>,
}

fn default_version() -> String {
    "1.0.0".to_string()
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
        toml::from_str(&contents).map_err(|e| format!("Failed to parse config file: {}", e))
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
                env!("CARGO_PKG_VERSION"),
                SUPPORTED_MINECRAFT_VERSION,
                SUPPORTED_PACK_FORMAT
            ));
        }

        Ok(())
    }

    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<(), String> {
        let contents = toml::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize config: {}", e))?;
        fs::write(path, contents).map_err(|e| format!("Failed to write config file: {}", e))
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
