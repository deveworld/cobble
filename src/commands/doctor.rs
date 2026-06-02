use crate::commands::validate::SUPPORTED_COMMANDS_JSON_SHA1;
use crate::config::CobbleConfig;
use crate::pack_format::{COBBLE_VERSION, SUPPORTED_MINECRAFT_VERSION, SUPPORTED_PACK_FORMAT};
use sha1::{Digest, Sha1};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct DoctorOptions {
    pub path: Option<PathBuf>,
    pub commands_json: PathBuf,
}

pub fn doctor(options: DoctorOptions) -> Result<(), String> {
    let project_root = options.path.unwrap_or_else(|| PathBuf::from("."));
    let config_search_root = if project_root.is_file() {
        project_root.parent().unwrap_or_else(|| Path::new("."))
    } else {
        project_root.as_path()
    };

    println!("Cobble doctor");
    println!("  Cobble version: {COBBLE_VERSION}");
    println!("  Minecraft target: Java Edition {SUPPORTED_MINECRAFT_VERSION}");
    println!("  Pack format: {SUPPORTED_PACK_FORMAT}");

    print_tool_status("Java", "java", &["-version"]);
    print_tool_status("curl", "curl", &["--version"]);
    print_config_status(config_search_root);
    print_commands_json_status(&options.commands_json)?;

    Ok(())
}

fn print_tool_status(label: &str, command: &str, args: &[&str]) {
    let status = Command::new(command).args(args).output();
    match status {
        Ok(output) if output.status.success() => println!("  ✓ {label}: available"),
        Ok(output) => println!(
            "  ! {label}: found but exited with {}",
            output
                .status
                .code()
                .map_or_else(|| "signal".to_string(), |code| code.to_string())
        ),
        Err(error) => println!("  ! {label}: not available ({error})"),
    }
}

fn print_config_status(search_root: &Path) {
    match CobbleConfig::find_in_path(search_root) {
        Some(config_path) => match CobbleConfig::load(&config_path) {
            Ok(config) => {
                println!("  ✓ Config: {}", config_path.display());
                println!("    Project: {}", config.project.name);
                println!("    Namespace: {}", config.project.namespace);
                println!("    Source: {}", config.build.source);
                println!("    Output: {}", config.build.output);
            }
            Err(error) => {
                println!("  ! Config: {} ({error})", config_path.display());
            }
        },
        None => println!(
            "  ! Config: no cobble.toml found from {}",
            search_root.display()
        ),
    }
}

fn print_commands_json_status(commands_json: &Path) -> Result<(), String> {
    match inspect_commands_json(commands_json)? {
        CommandsJsonStatus::Missing => {
            println!("  ! Command tree: missing ({})", commands_json.display());
            if is_default_commands_json_path(commands_json) {
                println!("    Default validation will auto-generate it when needed.");
            }
        }
        CommandsJsonStatus::Present {
            sha1,
            matches_supported,
        } => {
            println!("  ✓ Command tree: {}", commands_json.display());
            println!("    SHA-1: {sha1}");
            if let Some(matches_supported) = matches_supported {
                if matches_supported {
                    println!("    Target match: Minecraft {SUPPORTED_MINECRAFT_VERSION}");
                } else {
                    println!(
                        "    Target mismatch: expected {SUPPORTED_COMMANDS_JSON_SHA1} for Minecraft {SUPPORTED_MINECRAFT_VERSION}"
                    );
                }
            }
        }
    }

    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
enum CommandsJsonStatus {
    Missing,
    Present {
        sha1: String,
        matches_supported: Option<bool>,
    },
}

fn inspect_commands_json(commands_json: &Path) -> Result<CommandsJsonStatus, String> {
    if !commands_json.exists() {
        return Ok(CommandsJsonStatus::Missing);
    }

    let sha1 = sha1_file(commands_json)?;
    let matches_supported = if is_default_commands_json_path(commands_json) {
        Some(sha1.eq_ignore_ascii_case(SUPPORTED_COMMANDS_JSON_SHA1))
    } else {
        None
    };

    Ok(CommandsJsonStatus::Present {
        sha1,
        matches_supported,
    })
}

fn is_default_commands_json_path(commands_json: &Path) -> bool {
    commands_json == Path::new("data/commands.json")
        || commands_json.ends_with(Path::new("data/commands.json"))
}

fn sha1_file(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path)
        .map_err(|error| format!("Failed to open {}: {error}", path.display()))?;
    let mut hasher = Sha1::new();
    let mut buffer = [0; 8192];
    loop {
        let bytes_read = file
            .read(&mut buffer)
            .map_err(|error| format!("Failed to read {}: {error}", path.display()))?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inspect_commands_json_reports_missing_default_tree() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let commands_json = temp_dir.path().join("data/commands.json");

        assert_eq!(
            inspect_commands_json(&commands_json).unwrap(),
            CommandsJsonStatus::Missing
        );
    }

    #[test]
    fn inspect_commands_json_reports_default_tree_fingerprint_mismatch() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let commands_json = temp_dir.path().join("data/commands.json");
        fs::create_dir_all(commands_json.parent().unwrap()).unwrap();
        fs::write(&commands_json, "{}").unwrap();

        let CommandsJsonStatus::Present {
            matches_supported, ..
        } = inspect_commands_json(&commands_json).unwrap()
        else {
            panic!("expected present command tree");
        };

        assert_eq!(matches_supported, Some(false));
    }

    #[test]
    fn doctor_runs_without_config_or_command_tree() {
        let temp_dir = tempfile::TempDir::new().unwrap();

        doctor(DoctorOptions {
            path: Some(temp_dir.path().to_path_buf()),
            commands_json: temp_dir.path().join("missing.json"),
        })
        .unwrap();
    }
}
