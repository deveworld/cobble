use crate::config::CobbleConfig;
use std::fs;
use std::path::PathBuf;

pub struct InitOptions {
    pub name: Option<String>,
    pub description: Option<String>,
    pub pack_format: Option<String>,
    pub template: String,
}

pub fn init(options: InitOptions) -> Result<(), String> {
    let sample_code = sample_code_for_template(&options.template)?;
    let requested_name = options.name.clone();
    let has_name = requested_name.is_some();
    let project_name = requested_name
        .as_ref()
        .and_then(|name| {
            PathBuf::from(name)
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
        })
        .filter(|name| !name.is_empty())
        .or_else(|| {
            std::env::current_dir()
                .ok()
                .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        })
        .unwrap_or_else(|| "my-datapack".to_string());

    println!("Initializing Cobble project: {}", project_name);

    // Create project directory if a name was provided
    let project_dir = if has_name {
        let dir = PathBuf::from(requested_name.as_ref().unwrap());
        fs::create_dir_all(&dir)
            .map_err(|e| format!("Failed to create project directory: {}", e))?;
        dir
    } else {
        std::env::current_dir().map_err(|e| format!("Failed to get current directory: {}", e))?
    };

    // Create cobble.toml
    let mut config = CobbleConfig::default_with_name(project_name);

    // Apply custom description and pack_format if provided
    if let Some(desc) = options.description {
        config.project.description = desc;
    }
    if let Some(format_str) = options.pack_format {
        use crate::pack_format::{
            PackFormat, COBBLE_VERSION, SUPPORTED_MINECRAFT_VERSION, SUPPORTED_PACK_FORMAT,
        };
        let pack_fmt = PackFormat::parse_format(&format_str)?;

        if !pack_fmt.is_supported() {
            return Err(format!(
                "pack_format must be {} (Minecraft Java Edition {}), got {}.\n\
                Cobble v{} exclusively supports Minecraft Java Edition {}.\n\
                See https://minecraft.wiki/w/Pack_format for version compatibility.",
                SUPPORTED_PACK_FORMAT,
                SUPPORTED_MINECRAFT_VERSION,
                pack_fmt,
                COBBLE_VERSION,
                SUPPORTED_MINECRAFT_VERSION
            ));
        }

        config.project.pack_format = format_str;
    }

    let config_path = project_dir.join("cobble.toml");

    if config_path.exists() {
        return Err("cobble.toml already exists".to_string());
    }

    config.save(&config_path)?;

    // Create src directory
    let src_dir = project_dir.join("src");
    fs::create_dir_all(&src_dir).map_err(|e| format!("Failed to create src directory: {}", e))?;

    // Create main.cbl with sample code
    let main_file = src_dir.join("main.cbl");

    fs::write(&main_file, sample_code).map_err(|e| format!("Failed to create main.cbl: {}", e))?;

    // Create .gitignore
    let gitignore = project_dir.join(".gitignore");
    let gitignore_content = r#"# Cobble output
output/
*.zip

# Editor files
.vscode/
.idea/
*.swp
*.swo
*~

# OS files
.DS_Store
Thumbs.db
"#;

    fs::write(&gitignore, gitignore_content)
        .map_err(|e| format!("Failed to create .gitignore: {}", e))?;

    println!("✓ Created cobble.toml");
    println!("✓ Created src/main.cbl");
    println!("✓ Created .gitignore");
    println!();
    println!("Project initialized successfully!");
    println!("Next steps:");
    if has_name {
        println!("  cd {}", project_dir.display());
    }
    println!("  cobble build --dry-run");
    println!("  cobble build --validate");
    println!("  cobble watch");

    Ok(())
}

fn sample_code_for_template(template: &str) -> Result<&'static str, String> {
    match template {
        "minimal" => Ok(r#"def main():
    /say Hello from Cobble
"#),
        "stdlib" => Ok(r#"import stdlib
from stdlib import event

def init():
    """Initialize the data pack"""
    /scoreboard objectives add game_score dummy "Game Score"
    /tellraw @a {"text":"Data pack initialized!", "color":"green"}

def tick():
    """Called every game tick"""
    pass

def hello(player):
    """Greet a player"""
    /tellraw @a {"text":"Hello, World!", "color":"gold"}

# Register event handlers
stdlib.addEventListener(event.LOAD, init)
stdlib.addEventListener(event.TICK, tick)
"#),
        "validation" => Ok(r#"import stdlib
from stdlib import event

def init():
    /tellraw @a {"text":"Cobble validation-ready pack loaded","color":"green"}
    score.objective.add("points", "dummy", "Points")
    score.set("points", 0)
    bossbar.add("progress", "Progress")
    bossbar.set_max("progress", 100)
    bossbar.set_players("progress", "@a")

def tick():
    score.add("points", 1)
    bossbar.set_value("progress", 50)

stdlib.addEventListener(event.LOAD, init)
stdlib.addEventListener(event.TICK, tick)
"#),
        other => Err(format!(
            "Unknown template '{}'. Expected one of: minimal, stdlib, validation",
            other
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_minimal_template_creates_minimal_source() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let project_dir = temp_dir.path().join("minimal_pack");

        init(InitOptions {
            name: Some(project_dir.display().to_string()),
            description: None,
            pack_format: None,
            template: "minimal".to_string(),
        })
        .unwrap();

        let source = fs::read_to_string(project_dir.join("src/main.cbl")).unwrap();
        assert!(source.contains("def main():"));
        assert!(!source.contains("import stdlib"));
    }

    #[test]
    fn init_rejects_unknown_template() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let project_dir = temp_dir.path().join("bad_template");

        let error = init(InitOptions {
            name: Some(project_dir.display().to_string()),
            description: None,
            pack_format: None,
            template: "unknown".to_string(),
        })
        .unwrap_err();

        assert!(error.contains("Unknown template"));
        assert!(!project_dir.exists());
    }

    #[test]
    fn init_uses_basename_for_project_name_when_name_is_a_path() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let project_dir = temp_dir.path().join("nested").join("pack_name");

        init(InitOptions {
            name: Some(project_dir.display().to_string()),
            description: None,
            pack_format: None,
            template: "minimal".to_string(),
        })
        .unwrap();

        let config = fs::read_to_string(project_dir.join("cobble.toml")).unwrap();
        assert!(config.contains(r#"name = "pack_name""#));
        assert!(config.contains(r#"namespace = "pack_name""#));
    }
}
