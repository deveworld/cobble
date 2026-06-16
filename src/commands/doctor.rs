use crate::commands::link::{link_state_path, read_link_state, validate_link_state_paths};
use crate::commands::output_safety::{
    build_manifest_path, ensure_no_symlink_components, ensure_no_symlink_descendants,
    project_marker_identity, read_build_manifest, require_manifest_ownership,
};
use crate::commands::validate::SUPPORTED_COMMANDS_JSON_SHA1;
use crate::config::CobbleConfig;
use crate::pack_format::{COBBLE_VERSION, SUPPORTED_MINECRAFT_VERSION, SUPPORTED_PACK_FORMAT};
use serde::Serialize;
use sha1::{Digest, Sha1};
use std::fs;
use std::io::ErrorKind;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct DoctorOptions {
    pub path: Option<PathBuf>,
    pub commands_json: PathBuf,
    pub json: bool,
}

pub fn doctor(options: DoctorOptions) -> Result<(), String> {
    let project_root = options.path.unwrap_or_else(|| PathBuf::from("."));
    let config_search_root = if project_root.is_file() {
        project_root.parent().unwrap_or_else(|| Path::new("."))
    } else {
        project_root.as_path()
    };

    let report = build_doctor_report(&project_root, config_search_root, &options.commands_json);
    if options.json {
        print_json_report(&report)
    } else {
        print_human_report(&report);
        Ok(())
    }
}

#[derive(Serialize)]
struct DoctorReport {
    schema_version: u32,
    status: String,
    cobble: CobbleReport,
    project_path: String,
    config: ConfigReport,
    experimental_output: OutputReport,
    commands_json: CommandsJsonReport,
    experimental_link: LinkReport,
    tools: Vec<ToolReport>,
}

#[derive(Serialize)]
struct CobbleReport {
    version: &'static str,
    minecraft_target: String,
    pack_format: String,
}

#[derive(Serialize)]
struct ConfigReport {
    id: &'static str,
    status: String,
    search_root: String,
    path: Option<String>,
    project: Option<ProjectReport>,
    build: Option<BuildReport>,
    message: Option<String>,
}

#[derive(Serialize)]
struct ProjectReport {
    name: String,
    namespace: String,
    description: String,
    version: String,
    pack_format: String,
}

#[derive(Serialize)]
struct BuildReport {
    source: String,
    output: String,
    entry_points: Vec<String>,
}

#[derive(Serialize)]
struct CommandsJsonReport {
    id: &'static str,
    status: String,
    path: String,
    is_default_path: bool,
    sha1: Option<String>,
    expected_sha1: Option<&'static str>,
    matches_supported: Option<bool>,
    minecraft_target: &'static str,
    message: String,
}

#[derive(Serialize)]
struct OutputReport {
    id: &'static str,
    status: String,
    configured: bool,
    path: Option<String>,
    exists: bool,
    marker: OutputMarkerReport,
    message: String,
}

#[derive(Serialize)]
struct OutputMarkerReport {
    status: String,
    path: Option<String>,
    present: bool,
    namespace: Option<String>,
    project_id: Option<String>,
    message: String,
}

#[derive(Serialize)]
struct LinkReport {
    id: &'static str,
    status: String,
    configured: bool,
    state_path: Option<String>,
    target_kind: Option<String>,
    target_path: Option<String>,
    pack_name: Option<String>,
    pack_path: Option<String>,
    marker: LinkMarkerReport,
    message: String,
}

#[derive(Serialize)]
struct LinkMarkerReport {
    status: String,
    path: Option<String>,
    present: bool,
    project_id: Option<String>,
    message: String,
}

#[derive(Serialize)]
struct ToolReport {
    id: String,
    label: String,
    command: String,
    status: String,
    exit_code: Option<i32>,
    message: String,
}

fn build_doctor_report(
    project_root: &Path,
    config_search_root: &Path,
    commands_json: &Path,
) -> DoctorReport {
    let tools = vec![
        inspect_tool("tool.java", "Java", "java", &["-version"]),
        inspect_tool("tool.curl", "curl", "curl", &["--version"]),
    ];
    let config = inspect_config(config_search_root);
    let experimental_output = inspect_output_report(&config);
    let commands_json = inspect_commands_json_report(commands_json);
    let experimental_link = inspect_link_report(&config);
    let status = overall_status(
        &config,
        &experimental_output,
        &commands_json,
        &experimental_link,
        &tools,
    )
    .to_string();

    DoctorReport {
        schema_version: 1,
        status,
        cobble: CobbleReport {
            version: COBBLE_VERSION,
            minecraft_target: format!("Java Edition {SUPPORTED_MINECRAFT_VERSION}"),
            pack_format: SUPPORTED_PACK_FORMAT.to_string(),
        },
        project_path: path_display(project_root),
        config,
        experimental_output,
        commands_json,
        experimental_link,
        tools,
    }
}

fn print_human_report(report: &DoctorReport) {
    println!("Cobble doctor");
    println!("  Cobble version: {}", report.cobble.version);
    println!("  Minecraft target: {}", report.cobble.minecraft_target);
    println!("  Pack format: {}", report.cobble.pack_format);

    for tool in &report.tools {
        print_tool_report(tool);
    }
    print_config_report(&report.config);
    print_output_report(&report.experimental_output);
    print_link_report(&report.experimental_link);
    print_commands_json_report(&report.commands_json);
    println!("  Status: {}", report.status);
}

fn print_json_report(report: &DoctorReport) -> Result<(), String> {
    let output = serde_json::to_string_pretty(report)
        .map_err(|error| format!("Failed to format doctor JSON: {error}"))?;
    println!("{output}");
    Ok(())
}

fn inspect_tool(id: &str, label: &str, command: &str, args: &[&str]) -> ToolReport {
    let status = Command::new(command).args(args).output();
    match status {
        Ok(output) if output.status.success() => ToolReport {
            id: id.to_string(),
            label: label.to_string(),
            command: command.to_string(),
            status: "ok".to_string(),
            exit_code: output.status.code(),
            message: "available".to_string(),
        },
        Ok(output) => {
            let exit_code = output.status.code();
            ToolReport {
                id: id.to_string(),
                label: label.to_string(),
                command: command.to_string(),
                status: "warning".to_string(),
                exit_code,
                message: format!(
                    "found but exited with {}",
                    exit_code.map_or_else(|| "signal".to_string(), |code| code.to_string())
                ),
            }
        }
        Err(error) => ToolReport {
            id: id.to_string(),
            label: label.to_string(),
            command: command.to_string(),
            status: "warning".to_string(),
            exit_code: None,
            message: format!("not available ({error})"),
        },
    }
}

fn inspect_config(search_root: &Path) -> ConfigReport {
    match CobbleConfig::find_in_path(search_root) {
        Some(config_path) => match CobbleConfig::load(&config_path) {
            Ok(config) => ConfigReport {
                id: "config",
                status: "ok".to_string(),
                search_root: path_display(search_root),
                path: Some(path_display(&config_path)),
                project: Some(ProjectReport {
                    name: config.project.name,
                    namespace: config.project.namespace,
                    description: config.project.description,
                    version: config.project.version,
                    pack_format: config.project.pack_format,
                }),
                build: Some(BuildReport {
                    source: config.build.source,
                    output: config.build.output,
                    entry_points: config.build.entry_points,
                }),
                message: None,
            },
            Err(error) => ConfigReport {
                id: "config",
                status: "error".to_string(),
                search_root: path_display(search_root),
                path: Some(path_display(&config_path)),
                project: None,
                build: None,
                message: Some(error),
            },
        },
        None => ConfigReport {
            id: "config",
            status: "warning".to_string(),
            search_root: path_display(search_root),
            path: None,
            project: None,
            build: None,
            message: Some(format!(
                "no cobble.toml found from {}",
                search_root.display()
            )),
        },
    }
}

fn inspect_output_report(config: &ConfigReport) -> OutputReport {
    let Some(config_path) = config.path.as_deref() else {
        return output_not_configured("no cobble.toml; output path not available".to_string());
    };
    let Some(build) = &config.build else {
        return output_not_configured("invalid config; output path not available".to_string());
    };
    let config_dir = Path::new(config_path)
        .parent()
        .unwrap_or_else(|| Path::new("."));
    let configured_output = Path::new(&build.output);
    let output_path = if configured_output.is_absolute() {
        configured_output.to_path_buf()
    } else {
        config_dir.join(configured_output)
    };
    let output_path_display = Some(path_display(&output_path));

    if let Err(error) = ensure_no_symlink_components(&output_path, "inspect output") {
        return OutputReport {
            id: "output",
            status: "error".to_string(),
            configured: true,
            path: output_path_display,
            exists: output_path.exists(),
            marker: output_marker_not_applicable(),
            message: error,
        };
    }

    if !output_path.exists() {
        return OutputReport {
            id: "output",
            status: "not_present".to_string(),
            configured: true,
            path: output_path_display,
            exists: false,
            marker: output_marker_not_applicable(),
            message: "configured output does not exist yet".to_string(),
        };
    }

    let metadata = match fs::symlink_metadata(&output_path) {
        Ok(metadata) => metadata,
        Err(error) => {
            return OutputReport {
                id: "output",
                status: "error".to_string(),
                configured: true,
                path: output_path_display,
                exists: true,
                marker: output_marker_not_applicable(),
                message: format!("failed to inspect output path: {error}"),
            }
        }
    };
    if metadata.file_type().is_symlink() {
        return OutputReport {
            id: "output",
            status: "error".to_string(),
            configured: true,
            path: output_path_display,
            exists: true,
            marker: output_marker_not_applicable(),
            message: "configured output path is a symlink".to_string(),
        };
    }
    if !metadata.is_dir() {
        return OutputReport {
            id: "output",
            status: "error".to_string(),
            configured: true,
            path: output_path_display,
            exists: true,
            marker: output_marker_not_applicable(),
            message: "configured output path is not a directory".to_string(),
        };
    }
    if let Err(error) = ensure_no_symlink_descendants(&output_path, "inspect output") {
        return OutputReport {
            id: "output",
            status: "error".to_string(),
            configured: true,
            path: output_path_display,
            exists: true,
            marker: output_marker_not_applicable(),
            message: error,
        };
    }

    let (_, expected_project_id) = project_marker_identity(config_dir);
    let marker = inspect_output_marker(
        &output_path,
        config
            .project
            .as_ref()
            .map(|project| project.namespace.as_str()),
        Some(&expected_project_id),
    );
    let status = marker.status.clone();
    let message = match status.as_str() {
        "ok" => "configured output is Cobble-generated for this project".to_string(),
        "warning" => marker.message.clone(),
        _ => marker.message.clone(),
    };

    OutputReport {
        id: "output",
        status,
        configured: true,
        path: output_path_display,
        exists: true,
        marker,
        message,
    }
}

fn inspect_output_marker(
    output_path: &Path,
    expected_namespace: Option<&str>,
    expected_project_id: Option<&str>,
) -> OutputMarkerReport {
    let marker_path = build_manifest_path(output_path);
    match fs::symlink_metadata(&marker_path) {
        Ok(metadata) if metadata.file_type().is_symlink() => OutputMarkerReport {
            status: "error".to_string(),
            path: Some(path_display(&marker_path)),
            present: false,
            namespace: None,
            project_id: None,
            message: "marker path is a symlink".to_string(),
        },
        Ok(metadata) if metadata.is_file() => match read_build_manifest(&marker_path) {
            Ok(manifest) => {
                let namespace = manifest
                    .get("namespace")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string);
                let project_id = manifest
                    .get("project_id")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string);
                if let Err(error) =
                    require_manifest_ownership(&manifest, expected_namespace, expected_project_id)
                {
                    OutputMarkerReport {
                        status: "warning".to_string(),
                        path: Some(path_display(&marker_path)),
                        present: true,
                        namespace,
                        project_id,
                        message: error,
                    }
                } else {
                    OutputMarkerReport {
                        status: "ok".to_string(),
                        path: Some(path_display(&marker_path)),
                        present: true,
                        namespace,
                        project_id,
                        message: "present".to_string(),
                    }
                }
            }
            Err(error) => OutputMarkerReport {
                status: "warning".to_string(),
                path: Some(path_display(&marker_path)),
                present: false,
                namespace: None,
                project_id: None,
                message: error,
            },
        },
        Ok(_) => OutputMarkerReport {
            status: "warning".to_string(),
            path: Some(path_display(&marker_path)),
            present: false,
            namespace: None,
            project_id: None,
            message: "marker path exists but is not a file".to_string(),
        },
        Err(error) if error.kind() == ErrorKind::NotFound => OutputMarkerReport {
            status: "warning".to_string(),
            path: Some(path_display(&marker_path)),
            present: false,
            namespace: None,
            project_id: None,
            message:
                "output exists but has no Cobble build marker; clean will refuse this directory"
                    .to_string(),
        },
        Err(error) => OutputMarkerReport {
            status: "error".to_string(),
            path: Some(path_display(&marker_path)),
            present: false,
            namespace: None,
            project_id: None,
            message: format!("failed to inspect marker: {error}"),
        },
    }
}

fn output_not_configured(message: String) -> OutputReport {
    OutputReport {
        id: "output",
        status: "not_configured".to_string(),
        configured: false,
        path: None,
        exists: false,
        marker: output_marker_not_applicable(),
        message,
    }
}

fn output_marker_not_applicable() -> OutputMarkerReport {
    OutputMarkerReport {
        status: "not_applicable".to_string(),
        path: None,
        present: false,
        namespace: None,
        project_id: None,
        message: "no output path".to_string(),
    }
}

fn inspect_link_report(config: &ConfigReport) -> LinkReport {
    let Some(config_path) = config.path.as_deref() else {
        return link_not_configured(None, "no cobble.toml; link state not available".to_string());
    };
    let config_dir = Path::new(config_path)
        .parent()
        .unwrap_or_else(|| Path::new("."));
    let state_path = Some(path_display(&link_state_path(config_dir)));

    match read_link_state(config_dir) {
        Ok(Some(state)) => {
            if let Err(error) = validate_link_state_paths(&state) {
                return LinkReport {
                    id: "link",
                    status: "error".to_string(),
                    configured: true,
                    state_path,
                    target_kind: Some(state.target_kind),
                    target_path: Some(state.target_path),
                    pack_name: Some(state.pack_name),
                    pack_path: Some(state.pack_path),
                    marker: link_marker_not_applicable(),
                    message: error,
                };
            }
            let (_, expected_project_id) = project_marker_identity(config_dir);
            let marker = inspect_link_marker(
                Path::new(&state.pack_path),
                config
                    .project
                    .as_ref()
                    .map(|project| project.namespace.as_str()),
                Some(&expected_project_id),
            );
            let status = marker.status.clone();
            let message = if marker.present {
                "configured; linked pack marker is present".to_string()
            } else {
                marker.message.clone()
            };
            LinkReport {
                id: "link",
                status,
                configured: true,
                state_path,
                target_kind: Some(state.target_kind),
                target_path: Some(state.target_path),
                pack_name: Some(state.pack_name),
                pack_path: Some(state.pack_path),
                marker,
                message,
            }
        }
        Ok(None) => link_not_configured(state_path, "no link state configured".to_string()),
        Err(error) => LinkReport {
            id: "link",
            status: "error".to_string(),
            configured: false,
            state_path,
            target_kind: None,
            target_path: None,
            pack_name: None,
            pack_path: None,
            marker: link_marker_not_applicable(),
            message: error,
        },
    }
}

fn inspect_link_marker(
    pack_path: &Path,
    expected_namespace: Option<&str>,
    expected_project_id: Option<&str>,
) -> LinkMarkerReport {
    let marker_path = build_manifest_path(pack_path);
    match fs::symlink_metadata(&marker_path) {
        Ok(metadata) if metadata.file_type().is_symlink() => LinkMarkerReport {
            status: "error".to_string(),
            path: Some(path_display(&marker_path)),
            present: false,
            project_id: None,
            message: "marker path is a symlink".to_string(),
        },
        Ok(metadata) if metadata.is_file() => {
            match read_build_manifest(&marker_path).and_then(|manifest| {
                require_manifest_ownership(&manifest, expected_namespace, expected_project_id)?;
                Ok(manifest)
            }) {
                Ok(manifest) => {
                    let project_id = manifest
                        .get("project_id")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string);
                    LinkMarkerReport {
                        status: "ok".to_string(),
                        path: Some(path_display(&marker_path)),
                        present: true,
                        project_id,
                        message: "present".to_string(),
                    }
                }
                Err(error) => {
                    let project_id = read_build_manifest(&marker_path).ok().and_then(|manifest| {
                        manifest
                            .get("project_id")
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_string)
                    });
                    LinkMarkerReport {
                        status: "error".to_string(),
                        path: Some(path_display(&marker_path)),
                        present: false,
                        project_id,
                        message: error,
                    }
                }
            }
        }
        Ok(_) => LinkMarkerReport {
            status: "error".to_string(),
            path: Some(path_display(&marker_path)),
            present: false,
            project_id: None,
            message: "marker path exists but is not a file".to_string(),
        },
        Err(error) if error.kind() == ErrorKind::NotFound => LinkMarkerReport {
            status: "warning".to_string(),
            path: Some(path_display(&marker_path)),
            present: false,
            project_id: None,
            message: "marker missing; run `cobble watch --link` or build to the linked pack path"
                .to_string(),
        },
        Err(error) => LinkMarkerReport {
            status: "error".to_string(),
            path: Some(path_display(&marker_path)),
            present: false,
            project_id: None,
            message: format!("failed to inspect marker: {error}"),
        },
    }
}

fn link_not_configured(state_path: Option<String>, message: String) -> LinkReport {
    LinkReport {
        id: "link",
        status: "not_configured".to_string(),
        configured: false,
        state_path,
        target_kind: None,
        target_path: None,
        pack_name: None,
        pack_path: None,
        marker: link_marker_not_applicable(),
        message,
    }
}

fn link_marker_not_applicable() -> LinkMarkerReport {
    LinkMarkerReport {
        status: "not_applicable".to_string(),
        path: None,
        present: false,
        project_id: None,
        message: "no linked pack path".to_string(),
    }
}

fn inspect_commands_json_report(commands_json: &Path) -> CommandsJsonReport {
    let is_default_path = is_default_commands_json_path(commands_json);
    match inspect_commands_json(commands_json) {
        CommandsJsonStatus::Missing => {
            let message = if is_default_path {
                "missing; default validation will auto-generate it when needed".to_string()
            } else {
                "missing".to_string()
            };
            CommandsJsonReport {
                id: "commands_json",
                status: "warning".to_string(),
                path: path_display(commands_json),
                is_default_path,
                sha1: None,
                expected_sha1: is_default_path.then_some(SUPPORTED_COMMANDS_JSON_SHA1),
                matches_supported: None,
                minecraft_target: SUPPORTED_MINECRAFT_VERSION,
                message,
            }
        }
        CommandsJsonStatus::Present {
            sha1,
            matches_supported,
        } => {
            let status = if matches_supported == Some(false) {
                "warning"
            } else {
                "ok"
            };
            let message = match matches_supported {
                Some(true) => format!("matches Minecraft {SUPPORTED_MINECRAFT_VERSION}"),
                Some(false) => format!(
                    "target mismatch; expected {SUPPORTED_COMMANDS_JSON_SHA1} for Minecraft {SUPPORTED_MINECRAFT_VERSION}"
                ),
                None => "present; custom path fingerprint recorded".to_string(),
            };
            CommandsJsonReport {
                id: "commands_json",
                status: status.to_string(),
                path: path_display(commands_json),
                is_default_path,
                sha1: Some(sha1),
                expected_sha1: is_default_path.then_some(SUPPORTED_COMMANDS_JSON_SHA1),
                matches_supported,
                minecraft_target: SUPPORTED_MINECRAFT_VERSION,
                message,
            }
        }
        CommandsJsonStatus::Error(error) => CommandsJsonReport {
            id: "commands_json",
            status: "error".to_string(),
            path: path_display(commands_json),
            is_default_path,
            sha1: None,
            expected_sha1: is_default_path.then_some(SUPPORTED_COMMANDS_JSON_SHA1),
            matches_supported: None,
            minecraft_target: SUPPORTED_MINECRAFT_VERSION,
            message: error,
        },
    }
}

fn print_tool_report(tool: &ToolReport) {
    if tool.status == "ok" {
        println!("  ✓ {}: {}", tool.label, tool.message);
    } else {
        println!("  ! {}: {}", tool.label, tool.message);
    }
}

fn print_config_report(config: &ConfigReport) {
    match config.status.as_str() {
        "ok" => {
            println!(
                "  ✓ Config: {}",
                config.path.as_deref().unwrap_or("unknown")
            );
            if let Some(project) = &config.project {
                println!("    Project: {}", project.name);
                println!("    Namespace: {}", project.namespace);
            }
            if let Some(build) = &config.build {
                println!("    Source: {}", build.source);
                println!("    Output: {}", build.output);
            }
        }
        _ => {
            let path = config.path.as_deref().unwrap_or("not found");
            let message = config.message.as_deref().unwrap_or("unknown issue");
            println!("  ! Config: {path} ({message})");
        }
    }
}

fn print_output_report(output: &OutputReport) {
    match output.status.as_str() {
        "ok" => println!(
            "  ✓ Output: {}",
            output.path.as_deref().unwrap_or("configured")
        ),
        "not_present" => println!(
            "  - Output: not built yet ({})",
            output.path.as_deref().unwrap_or("unknown")
        ),
        "warning" => {
            println!("  ! Output: {}", output.message);
            if let Some(path) = &output.path {
                println!("    Path: {path}");
            }
        }
        "error" => println!("  ! Output: {}", output.message),
        _ => println!("  - Output: not configured"),
    }
}

fn print_link_report(link: &LinkReport) {
    match link.status.as_str() {
        "ok" => {
            println!(
                "  ✓ Link: {}",
                link.pack_path.as_deref().unwrap_or("configured")
            );
            println!("    Marker: present");
        }
        "warning" => {
            println!("  ! Link: {}", link.message);
            if let Some(pack_path) = &link.pack_path {
                println!("    Pack path: {pack_path}");
            }
        }
        "error" => {
            println!("  ! Link: {}", link.message);
        }
        _ => {
            println!("  - Link: not configured");
        }
    }
}

fn print_commands_json_report(commands_json: &CommandsJsonReport) {
    if commands_json.status == "ok" {
        println!("  ✓ Command tree: {}", commands_json.path);
    } else {
        println!(
            "  ! Command tree: {} ({})",
            commands_json.message, commands_json.path
        );
    }
    if let Some(sha1) = &commands_json.sha1 {
        println!("    SHA-1: {sha1}");
    }
    if let Some(matches_supported) = commands_json.matches_supported {
        if matches_supported {
            println!("    Target match: Minecraft {SUPPORTED_MINECRAFT_VERSION}");
        } else {
            println!(
                "    Target mismatch: expected {SUPPORTED_COMMANDS_JSON_SHA1} for Minecraft {SUPPORTED_MINECRAFT_VERSION}"
            );
        }
    } else if commands_json.is_default_path && commands_json.sha1.is_none() {
        println!("    Default validation will auto-generate it when needed.");
    }
}

fn overall_status(
    config: &ConfigReport,
    output: &OutputReport,
    commands_json: &CommandsJsonReport,
    link: &LinkReport,
    tools: &[ToolReport],
) -> &'static str {
    if config.status == "error"
        || output.status == "error"
        || commands_json.status == "error"
        || link.status == "error"
        || tools.iter().any(|tool| tool.status == "error")
    {
        "error"
    } else if config.status == "warning"
        || output.status == "warning"
        || commands_json.status == "warning"
        || link.status == "warning"
        || tools.iter().any(|tool| tool.status == "warning")
    {
        "warning"
    } else {
        "ok"
    }
}

#[derive(Debug, PartialEq, Eq)]
enum CommandsJsonStatus {
    Missing,
    Present {
        sha1: String,
        matches_supported: Option<bool>,
    },
    Error(String),
}

fn inspect_commands_json(commands_json: &Path) -> CommandsJsonStatus {
    if !commands_json.exists() {
        return CommandsJsonStatus::Missing;
    }

    let sha1 = match sha1_file(commands_json) {
        Ok(sha1) => sha1,
        Err(error) => return CommandsJsonStatus::Error(error),
    };
    let matches_supported = if is_default_commands_json_path(commands_json) {
        Some(sha1.eq_ignore_ascii_case(SUPPORTED_COMMANDS_JSON_SHA1))
    } else {
        None
    };

    CommandsJsonStatus::Present {
        sha1,
        matches_supported,
    }
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

fn path_display(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inspect_commands_json_reports_missing_default_tree() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let commands_json = temp_dir.path().join("data/commands.json");

        assert_eq!(
            inspect_commands_json(&commands_json),
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
        } = inspect_commands_json(&commands_json)
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
            json: false,
        })
        .unwrap();
    }
}
