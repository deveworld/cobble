use super::output_safety::ensure_no_symlink_components;
use crate::config::CobbleConfig;
use crate::fs_safety::write_file_atomic;
use std::fs;
use std::path::PathBuf;

pub struct InitOptions {
    pub name: Option<String>,
    pub description: Option<String>,
    pub pack_format: Option<String>,
    pub template: String,
    pub list_templates: bool,
}

pub fn init(options: InitOptions) -> Result<(), String> {
    if options.list_templates {
        print_templates();
        return Ok(());
    }

    let template = template_for_name(&options.template)?;
    let sample_code = template.source;
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
        ensure_no_symlink_components(&dir, "initialize project")?;
        fs::create_dir_all(&dir)
            .map_err(|e| format!("Failed to create project directory: {}", e))?;
        dir
    } else {
        std::env::current_dir().map_err(|e| format!("Failed to get current directory: {}", e))?
    };

    // Create cobble.toml
    let mut config = CobbleConfig::default_with_name(project_name);
    if template.experimental_plugins {
        config.experimental.plugins = true;
    }

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
    let src_dir = project_dir.join("src");
    let main_file = src_dir.join("main.cbl");
    let gitignore = project_dir.join(".gitignore");

    for path in [&config_path, &src_dir, &main_file, &gitignore] {
        ensure_no_symlink_components(path, "initialize project")?;
    }
    for extra_file in template.extra_files {
        let path = project_dir.join(extra_file.path);
        ensure_no_symlink_components(&path, "initialize project")?;
    }

    if config_path.exists() {
        return Err("cobble.toml already exists".to_string());
    }

    config.save(&config_path)?;

    // Create src directory
    fs::create_dir_all(&src_dir).map_err(|e| format!("Failed to create src directory: {}", e))?;

    // Create main.cbl with sample code
    write_file_atomic(&main_file, sample_code)
        .map_err(|e| format!("Failed to create main.cbl: {}", e))?;

    // Create .gitignore
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

    write_file_atomic(&gitignore, gitignore_content)
        .map_err(|e| format!("Failed to create .gitignore: {}", e))?;

    for extra_file in template.extra_files {
        let path = project_dir.join(extra_file.path);
        if let Some(parent) = path.parent() {
            ensure_no_symlink_components(parent, "initialize project")?;
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create {}: {}", parent.display(), e))?;
        }
        write_file_atomic(&path, extra_file.content)
            .map_err(|e| format!("Failed to create {}: {}", path.display(), e))?;
        println!("✓ Created {}", extra_file.path);
    }

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

fn template_for_name(template: &str) -> Result<InitTemplate, String> {
    templates()
        .iter()
        .find(|candidate| candidate.name == template)
        .copied()
        .ok_or_else(|| {
            format!(
                "Unknown template '{}'. Expected one of: {}",
                template,
                templates()
                    .iter()
                    .map(|template| template.name)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
}

fn print_templates() {
    println!("Available templates:");
    for template in templates() {
        let suffix = if template.default { " (default)" } else { "" };
        println!("  {:<15} {}{}", template.name, template.description, suffix);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct InitTemplate {
    name: &'static str,
    description: &'static str,
    default: bool,
    source: &'static str,
    experimental_plugins: bool,
    extra_files: &'static [InitExtraFile],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct InitExtraFile {
    path: &'static str,
    content: &'static str,
}

fn templates() -> &'static [InitTemplate] {
    &[
        InitTemplate {
            name: "minimal",
            description: "Small single-function pack with no imports",
            default: false,
            experimental_plugins: false,
            extra_files: &[],
            source: r#"def main():
    /say Hello from Cobble
"#,
        },
        InitTemplate {
            name: "stdlib",
            description: "Event-ready starter using stdlib load and tick hooks",
            default: true,
            experimental_plugins: false,
            extra_files: &[],
            source: r#"import stdlib
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
"#,
        },
        InitTemplate {
            name: "validation",
            description: "Validation-ready pack that exercises common helpers",
            default: false,
            experimental_plugins: false,
            extra_files: &[],
            source: r#"import stdlib
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
"#,
        },
        InitTemplate {
            name: "resource-heavy",
            description: "Starter with tags, predicates, recipes, loot, and dialogs",
            default: false,
            experimental_plugins: false,
            extra_files: &[],
            source: r#"import stdlib
from stdlib import event

datapack.item_tag("reward_items", ["minecraft:diamond", "minecraft:emerald"])
datapack.block_tag("building_blocks", ["minecraft:stone", "minecraft:deepslate"])
datapack.entity_type_tag("hostile_targets", ["minecraft:zombie", "minecraft:skeleton"])
datapack.predicate("always", {
    "condition": "minecraft:random_chance",
    "chance": 1
})
datapack.advancement("root", {"criteria": {"tick": {"trigger": "minecraft:tick"}}})
datapack.loot_table("empty_reward", {"type": "minecraft:empty"})
datapack.recipe("stonecutting/polished_granite", {
    "type": "minecraft:stonecutting",
    "ingredient": "minecraft:granite",
    "result": {"id": "minecraft:polished_granite"}
})
datapack.item_modifier("reward_name", {
    "function": "minecraft:set_name",
    "name": {"text": "Cobble Reward"}
})
datapack.dialog("notice", {
    "type": "minecraft:notice",
    "title": {"text": "Resource Pack Ready"}
})

def init():
    /tellraw @a {"text":"Resource-heavy Cobble pack loaded","color":"green"}

stdlib.addEventListener(event.LOAD, init)
"#,
        },
        InitTemplate {
            name: "game-mechanic",
            description: "Small score loop with selectors, events, and actionbar feedback",
            default: false,
            experimental_plugins: false,
            extra_files: &[],
            source: r#"import stdlib
from stdlib import event

@Players = @a[gamemode=!spectator]

def init():
    /scoreboard objectives add cobble_points dummy "Cobble Points"
    /tellraw @a {"text":"Game mechanic starter loaded","color":"green"}

def tick():
    /execute as @Players run scoreboard players add @s cobble_points 1
    /execute as @a[scores={cobble_points=100..}] run title @s actionbar {"text":"Goal reached","color":"gold"}

stdlib.addEventListener(event.LOAD, init)
stdlib.addEventListener(event.TICK, tick)
"#,
        },
        InitTemplate {
            name: "web-demo",
            description: "Compact starter matching the browser /try default sample",
            default: false,
            experimental_plugins: false,
            extra_files: &[],
            source: r#"def on_load():
    /tellraw @a {"text":"Cobble demo loaded","color":"green"}
    /scoreboard objectives add demo dummy
    reward_player()

def reward_player():
    /give @p minecraft:diamond 1
    /title @p title {"text":"Reward unlocked","color":"aqua"}
"#,
        },
        InitTemplate {
            name: "plugin-diagnostics",
            description: "Experimental diagnostics-only plugin manifest starter",
            default: false,
            experimental_plugins: true,
            extra_files: &[InitExtraFile {
                path: "plugins/example_lints.toml",
                content: r#"plugin_version = 1
name = "example_lints"
kind = "diagnostics"
description = "Example diagnostics-only lint manifest"
minimum_cobble_version = "0.9.0"
diagnostic_rules = [
    "example_lints.no_tellraw",
    "example_lints.no_raw_op",
    "example_lints.no_gamemode_creative",
    "example_lints.max_raw_command_length",
]

[capabilities]
read_project_metadata = true
read_source_text = true
emit_diagnostics = true
"#,
            }],
            source: r#"def main():
    /say plugin diagnostics manifest loaded
"#,
        },
    ]
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
            list_templates: false,
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
            list_templates: false,
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
            list_templates: false,
        })
        .unwrap();

        let config = fs::read_to_string(project_dir.join("cobble.toml")).unwrap();
        assert!(config.contains(r#"name = "pack_name""#));
        assert!(config.contains(r#"namespace = "pack_name""#));
    }

    #[test]
    fn init_plugin_diagnostics_template_creates_manifest_and_enables_config() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let project_dir = temp_dir.path().join("plugin_pack");

        init(InitOptions {
            name: Some(project_dir.display().to_string()),
            description: None,
            pack_format: None,
            template: "plugin-diagnostics".to_string(),
            list_templates: false,
        })
        .unwrap();

        let config = fs::read_to_string(project_dir.join("cobble.toml")).unwrap();
        assert!(config.contains("plugins = true"));

        let manifest = fs::read_to_string(project_dir.join("plugins/example_lints.toml")).unwrap();
        assert!(manifest.contains("plugin_version = 1"));
        assert!(manifest.contains(r#""example_lints.no_tellraw""#));
        assert!(manifest.contains(r#""example_lints.no_raw_op""#));
        assert!(manifest.contains(r#""example_lints.no_gamemode_creative""#));
        assert!(manifest.contains(r#""example_lints.max_raw_command_length""#));
    }

    #[test]
    fn templates_have_unique_names_and_one_default() {
        let templates = templates();
        let default_count = templates.iter().filter(|template| template.default).count();

        assert_eq!(default_count, 1);
        for (index, template) in templates.iter().enumerate() {
            assert!(!template.name.is_empty());
            assert!(!template.description.is_empty());
            assert!(!template.source.is_empty());
            assert!(
                templates[index + 1..]
                    .iter()
                    .all(|other| other.name != template.name),
                "duplicate template name: {}",
                template.name
            );
        }
    }

    #[test]
    fn every_template_initializes_and_builds() {
        for template in templates() {
            let temp_dir = tempfile::TempDir::new().unwrap();
            let project_dir = temp_dir.path().join(template.name);

            init(InitOptions {
                name: Some(project_dir.display().to_string()),
                description: None,
                pack_format: None,
                template: template.name.to_string(),
                list_templates: false,
            })
            .unwrap();

            crate::commands::check::check(crate::commands::check::CheckOptions {
                input: Some(project_dir.join("src")),
                json: false,
                symbols: false,
                experimental_plugins: false,
                experimental_python_compat: false,
            })
            .unwrap();

            crate::commands::build::build(crate::commands::build::BuildOptions {
                input: Some(project_dir.join("src")),
                output: Some(temp_dir.path().join("output")),
                namespace: None,
                pack_format: None,
                description: None,
                verbose: false,
                quiet: true,
                zip: false,
                experimental_resource_pack: false,
                validate: false,
                dry_run: false,
                commands_json: PathBuf::from("data/commands.json"),
            })
            .unwrap();
        }
    }
}
