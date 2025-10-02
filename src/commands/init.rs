use crate::config::CobbleConfig;
use std::fs;
use std::path::PathBuf;

pub struct InitOptions {
    pub name: Option<String>,
    pub description: Option<String>,
    pub pack_format: Option<u32>,
}

pub fn init(options: InitOptions) -> Result<(), String> {
    let has_name = options.name.is_some();
    let project_name = options.name.unwrap_or_else(|| {
        std::env::current_dir()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
            .unwrap_or_else(|| "my-datapack".to_string())
    });

    println!("Initializing Cobble project: {}", project_name);

    // Create project directory if a name was provided
    let project_dir = if has_name {
        let dir = PathBuf::from(&project_name);
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
    if let Some(format) = options.pack_format {
        config.project.pack_format = format as u8;
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
    let sample_code = r#"import stdlib
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
"#;

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
    println!("  • Edit src/main.cbl to add your code");
    println!("  • Run 'cobble build' to generate the data pack");
    println!("  • Run 'cobble watch' for development mode");

    Ok(())
}
