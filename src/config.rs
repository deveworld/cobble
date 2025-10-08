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
    pub pack_format: u8,
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

fn default_pack_format() -> u8 {
    18 // Minecraft 1.20.2+ (macro support, maximum compatibility)
}

fn default_source() -> String {
    "src".to_string()
}

fn default_output() -> String {
    "output".to_string()
}

impl CobbleConfig {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let contents =
            fs::read_to_string(path).map_err(|e| format!("Failed to read config file: {}", e))?;
        let config: Self = toml::from_str(&contents)
            .map_err(|e| format!("Failed to parse config file: {}", e))?;

        // Validate pack_format
        if config.project.pack_format < 18 {
            return Err(format!(
                "Invalid pack_format: {}. Must be >= 18 (Minecraft 1.20.2+).\n\
                 \n\
                 Cobble requires Minecraft 1.20.2+ for function macro support.\n\
                 Recommended pack_format values:\n\
                 - 1.20.2: pack_format = 18 (maximum compatibility)\n\
                 - 1.21.7-1.21.8: pack_format = 81\n\
                 - 1.21.9+: pack_format = 88\n\
                 \n\
                 Update your cobble.toml:\n\
                 [project]\n\
                 pack_format = 18",
                config.project.pack_format
            ));
        }

        if config.project.pack_format > 100 {
            eprintln!(
                "Warning: pack_format {} is unusually high. Current latest is 88 (1.21.9).",
                config.project.pack_format
            );
        }

        Ok(config)
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
