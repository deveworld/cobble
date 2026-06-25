use super::{find_cobble_files, resolve_entry_points};
use crate::config::CobbleConfig;
use crate::diagnostics::{
    parse_source_files, FileSourceDiagnostics, ParsedSourceFile, SourceDiagnostic,
};
use crate::error::report_file_source_diagnostics;
use crate::pack_format::{PackFormat, SUPPORTED_PACK_FORMAT};
use crate::transpiler::Transpiler;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub struct CheckOptions {
    pub input: Option<PathBuf>,
    pub json: bool,
    pub symbols: bool,
    pub experimental_plugins: bool,
    pub experimental_python_compat: bool,
}

#[derive(Serialize)]
struct CheckReport {
    schema_version: u32,
    ok: bool,
    source: String,
    files_checked: usize,
    files: Vec<CheckFileReport>,
    diagnostics: Vec<CheckDiagnosticReport>,
    error_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    experimental_symbols: Option<Vec<CheckSymbolReport>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    experimental_plugins: Option<CheckPluginReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    experimental_python_compat: Option<CheckPythonCompatReport>,
}

#[derive(Serialize)]
struct CheckFileReport {
    file: String,
    imports: usize,
    functions: usize,
    commands: usize,
}

#[derive(Serialize)]
struct CheckDiagnosticReport {
    file: String,
    line: usize,
    column: usize,
    severity: String,
    kind: String,
    message: String,
    help: Option<String>,
    formatted: String,
}

#[derive(Serialize)]
struct CheckSymbolReport {
    file: String,
    name: String,
    kind: String,
    line: usize,
    column: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

#[derive(Serialize)]
struct CheckPluginReport {
    enabled: bool,
    manifests_checked: usize,
    manifests: Vec<CheckPluginManifestReport>,
    diagnostics: Vec<CheckPluginDiagnosticReport>,
}

#[derive(Serialize)]
struct CheckPluginManifestReport {
    name: String,
    plugin_version: u32,
    kind: String,
    capabilities: Vec<String>,
    path: String,
}

#[derive(Serialize)]
struct CheckPluginDiagnosticReport {
    kind: String,
    plugin: String,
    plugin_kind: String,
    severity: String,
    message: String,
}

#[derive(Serialize)]
struct CheckPythonCompatReport {
    enabled: bool,
    mode: String,
    supported_constructs: Vec<String>,
    unsupported_detected: Vec<CheckPythonCompatDiagnosticReport>,
}

#[derive(Serialize)]
struct CheckPythonCompatDiagnosticReport {
    file: String,
    line: usize,
    column: usize,
    kind: String,
    message: String,
    help: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PluginManifestDraft {
    plugin_version: u32,
    name: String,
    kind: String,
    #[serde(default)]
    capabilities: PluginManifestCapabilities,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct PluginManifestCapabilities {
    #[serde(default)]
    read_project_metadata: bool,
    #[serde(default)]
    read_source_text: bool,
    #[serde(default)]
    emit_diagnostics: bool,
}

pub fn check(options: CheckOptions) -> Result<(), String> {
    if options.symbols && !options.json {
        return Err("--symbols requires --json".to_string());
    }

    // Try to find cobble.toml
    let (config, config_dir) = if let Some(config_path) = find_config(&options.input) {
        let config = match CobbleConfig::load(&config_path) {
            Ok(config) => config,
            Err(error) if options.json => {
                print_config_error_json(&config_path, &error, options.symbols)?;
                return Err(format!("Config validation failed: {error}"));
            }
            Err(error) => return Err(error),
        };
        let config_dir = config_path.parent().unwrap().to_path_buf();
        (Some(config), config_dir)
    } else {
        (
            None,
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        )
    };
    let experimental_plugins = options.experimental_plugins
        || config
            .as_ref()
            .map(|cfg| cfg.experimental.plugins)
            .unwrap_or(false);
    let experimental_resource_pack = config
        .as_ref()
        .map(|cfg| cfg.experimental.resource_pack)
        .unwrap_or(false);
    let experimental_python_compat = options.experimental_python_compat
        || config
            .as_ref()
            .map(|cfg| cfg.experimental.python_compat)
            .unwrap_or(false);
    let plugin_report = plugin_report(experimental_plugins, &config_dir);

    // Determine source path
    let source_path = if let Some(ref input_path) = options.input {
        input_path.clone()
    } else if let Some(ref cfg) = config {
        config_dir.join(&cfg.build.source)
    } else {
        return Err("No input specified and no cobble.toml found".to_string());
    };

    let configured_entry_points = if options.input.is_none() {
        config
            .as_ref()
            .map(|cfg| cfg.build.entry_points.clone())
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    // Check if source is a file or directory
    let files_to_check = if source_path.is_file() {
        vec![source_path.clone()]
    } else if source_path.is_dir() {
        if options.input.is_none() && !configured_entry_points.is_empty() {
            resolve_entry_points(&source_path, &configured_entry_points)?
        } else {
            find_cobble_files(&source_path)?
        }
    } else {
        return Err(format!("Source path does not exist: {:?}", source_path));
    };

    if files_to_check.is_empty() {
        let plugin_errors = plugin_error_count(plugin_report.as_ref());
        if options.json {
            print_check_json(&CheckReport {
                schema_version: 1,
                ok: plugin_errors == 0,
                source: path_display(&source_path),
                files_checked: 0,
                files: Vec::new(),
                diagnostics: Vec::new(),
                error_count: plugin_errors,
                experimental_symbols: options.symbols.then(Vec::new),
                experimental_plugins: plugin_report,
                experimental_python_compat: python_compat_report_success(
                    experimental_python_compat,
                ),
            })?;
        } else {
            println!("No Cobble files found to check");
            print_python_compat_report_human(
                python_compat_report_success(experimental_python_compat).as_ref(),
            );
            print_plugin_report_human(plugin_report.as_ref());
        }
        return if plugin_errors == 0 {
            Ok(())
        } else {
            Err(format!(
                "Plugin manifest inspection failed with {plugin_errors} error(s)"
            ))
        };
    }

    if !options.json {
        println!("Checking {} file(s)...", files_to_check.len());
    }

    let parsed_files = match parse_source_files(&files_to_check) {
        Ok(parsed_files) => parsed_files,
        Err(file_diagnostics) => {
            let total_errors = file_diagnostics
                .iter()
                .map(|file| file.diagnostics.len())
                .sum::<usize>();
            let plugin_errors = plugin_error_count(plugin_report.as_ref());
            if options.json {
                print_check_json(&CheckReport {
                    schema_version: 1,
                    ok: false,
                    source: path_display(&source_path),
                    files_checked: files_to_check.len(),
                    files: Vec::new(),
                    diagnostics: diagnostic_reports(&file_diagnostics, &config_dir),
                    error_count: total_errors + plugin_errors,
                    experimental_symbols: symbol_reports_from_diagnostics(
                        &file_diagnostics,
                        &config_dir,
                        options.symbols,
                    ),
                    experimental_plugins: plugin_report,
                    experimental_python_compat: python_compat_report_from_diagnostics(
                        experimental_python_compat,
                        &file_diagnostics,
                        &config_dir,
                    ),
                })?;
            } else {
                for diagnostics in &file_diagnostics {
                    let relative_path = diagnostics
                        .path
                        .strip_prefix(&config_dir)
                        .unwrap_or(&diagnostics.path);
                    println!("  ✗ {:?}:", relative_path);
                }
                report_file_source_diagnostics(&file_diagnostics);

                println!();
                println!(
                    "✗ {} error(s) found in {} file(s)",
                    total_errors,
                    file_diagnostics.len()
                );
                print_python_compat_report_human(
                    python_compat_report_from_diagnostics(
                        experimental_python_compat,
                        &file_diagnostics,
                        &config_dir,
                    )
                    .as_ref(),
                );
                print_plugin_report_human(plugin_report.as_ref());
            }
            return Err(format!(
                "Validation failed with {} error(s)",
                total_errors + plugin_errors
            ));
        }
    };

    let files = check_file_reports(&files_to_check, &parsed_files, &config_dir);
    let semantic_diagnostics = semantic_check_diagnostics(
        &files_to_check,
        &parsed_files,
        config.as_ref(),
        &config_dir,
        experimental_resource_pack,
    );
    if !semantic_diagnostics.is_empty() {
        let semantic_errors = semantic_diagnostics
            .iter()
            .map(|file| file.diagnostics.len())
            .sum::<usize>();
        let plugin_errors = plugin_error_count(plugin_report.as_ref());
        if options.json {
            print_check_json(&CheckReport {
                schema_version: 1,
                ok: false,
                source: path_display(&source_path),
                files_checked: files_to_check.len(),
                files,
                diagnostics: diagnostic_reports(&semantic_diagnostics, &config_dir),
                error_count: semantic_errors + plugin_errors,
                experimental_symbols: symbol_reports_from_diagnostics(
                    &semantic_diagnostics,
                    &config_dir,
                    options.symbols,
                ),
                experimental_plugins: plugin_report,
                experimental_python_compat: python_compat_report_from_diagnostics(
                    experimental_python_compat,
                    &semantic_diagnostics,
                    &config_dir,
                ),
            })?;
        } else {
            report_file_source_diagnostics(&semantic_diagnostics);
            println!();
            println!(
                "✗ {} error(s) found in {} file(s)",
                semantic_errors,
                semantic_diagnostics.len()
            );
            print_python_compat_report_human(
                python_compat_report_from_diagnostics(
                    experimental_python_compat,
                    &semantic_diagnostics,
                    &config_dir,
                )
                .as_ref(),
            );
            print_plugin_report_human(plugin_report.as_ref());
        }
        return Err(format!(
            "Validation failed with {} error(s)",
            semantic_errors + plugin_errors
        ));
    }

    let plugin_errors = plugin_error_count(plugin_report.as_ref());
    if options.json {
        print_check_json(&CheckReport {
            schema_version: 1,
            ok: plugin_errors == 0,
            source: path_display(&source_path),
            files_checked: files_to_check.len(),
            files,
            diagnostics: Vec::new(),
            error_count: plugin_errors,
            experimental_symbols: symbol_reports_from_parsed(
                &parsed_files,
                &config_dir,
                options.symbols,
            ),
            experimental_plugins: plugin_report,
            experimental_python_compat: python_compat_report_success(experimental_python_compat),
        })?;
        return if plugin_errors == 0 {
            Ok(())
        } else {
            Err(format!(
                "Plugin manifest inspection failed with {plugin_errors} error(s)"
            ))
        };
    }

    for file in files {
        println!(
            "  ✓ {:?}: {} imports, {} functions, {} commands",
            file.file, file.imports, file.functions, file.commands
        );
    }

    // Summary
    println!();
    println!("✓ All files passed validation!");
    print_python_compat_report_human(
        python_compat_report_success(experimental_python_compat).as_ref(),
    );
    print_plugin_report_human(plugin_report.as_ref());

    if plugin_errors == 0 {
        Ok(())
    } else {
        Err(format!(
            "Plugin manifest inspection failed with {plugin_errors} error(s)"
        ))
    }
}

fn print_config_error_json(
    config_path: &Path,
    error: &str,
    include_symbols: bool,
) -> Result<(), String> {
    let file = path_display(config_path);
    print_check_json(&CheckReport {
        schema_version: 1,
        ok: false,
        source: file.clone(),
        files_checked: 0,
        files: Vec::new(),
        diagnostics: vec![CheckDiagnosticReport {
            file: file.clone(),
            line: 1,
            column: 1,
            severity: "error".to_string(),
            kind: "config".to_string(),
            message: error.to_string(),
            help: Some("Fix cobble.toml before running check again.".to_string()),
            formatted: format!("{file}:1:1: error[config]: {error}"),
        }],
        error_count: 1,
        experimental_symbols: include_symbols.then(Vec::new),
        experimental_plugins: None,
        experimental_python_compat: None,
    })
}

fn semantic_check_diagnostics(
    files_to_check: &[PathBuf],
    parsed_files: &[ParsedSourceFile],
    config: Option<&CobbleConfig>,
    config_dir: &Path,
    experimental_resource_pack: bool,
) -> Vec<FileSourceDiagnostics> {
    let namespace = config
        .map(|config| config.project.namespace.clone())
        .unwrap_or_else(|| "cobble".to_string());
    let pack_format = config
        .and_then(|config| PackFormat::parse_format(&config.project.pack_format).ok())
        .unwrap_or(SUPPORTED_PACK_FORMAT);
    let stdlib_version = config.map(|config| config.stdlib.version).unwrap_or(2);
    let output_dir = config
        .map(|config| config_dir.join(&config.build.output))
        .unwrap_or_else(|| config_dir.join(".cobble-check-output"));

    let mut transpiler = Transpiler::new(namespace, output_dir);
    transpiler.set_pack_format(pack_format);
    transpiler.set_stdlib_version(stdlib_version);
    transpiler.set_experimental_resource_pack(experimental_resource_pack);
    transpiler.set_source_display_root(config_dir.to_path_buf());

    let mut diagnostics = Vec::new();
    for (path, parsed) in files_to_check.iter().zip(parsed_files.iter()) {
        transpiler.set_current_file_with_source(path, &parsed.source);
        if let Err(error) = transpiler.transpile(&parsed.program) {
            diagnostics.push(FileSourceDiagnostics::new(
                path.clone(),
                parsed.source.clone(),
                vec![semantic_source_diagnostic(&error, &parsed.source)],
            ));
            break;
        }
    }

    diagnostics
}

fn semantic_source_diagnostic(error: &str, source: &str) -> SourceDiagnostic {
    let kind = if let Some((kind, _)) = error.split_once(':') {
        if kind.starts_with("unroll-") {
            kind.to_string()
        } else if error.contains("not imported") {
            "missing-stdlib-module".to_string()
        } else if error.contains("Unknown stdlib module") {
            "unknown-stdlib-module".to_string()
        } else {
            "semantic".to_string()
        }
    } else if error.contains("not imported") {
        "missing-stdlib-module".to_string()
    } else if error.contains("Unknown stdlib module") {
        "unknown-stdlib-module".to_string()
    } else {
        "semantic".to_string()
    };
    let (line, column) = semantic_error_location(error, source).unwrap_or((1, 1));
    SourceDiagnostic::error(kind, line, column, clean_semantic_error_message(error))
}

fn clean_semantic_error_message(error: &str) -> String {
    error
        .lines()
        .filter(|line| {
            let line = line.trim_start();
            !line.starts_with("source-key:") && !line.starts_with("location:")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn semantic_error_location(error: &str, source: &str) -> Option<(usize, usize)> {
    if error.starts_with("unroll-") {
        return unroll_error_location(error)
            .or_else(|| {
                unroll_error_source_key(error).and_then(|source_key| {
                    find_for_loop_location_for_source_key(source, source_key)
                })
            })
            .or_else(|| {
                unroll_error_target(error)
                    .and_then(|target| find_for_loop_location_for_target(source, target))
            })
            .or_else(|| find_for_loop_location(source));
    }

    let module = error
        .split_once("module '")
        .and_then(|(_, rest)| rest.split_once('\''))
        .map(|(module, _)| module)?;
    let needle = format!("{module}.");
    find_source_location(source, &needle).or_else(|| find_source_location(source, module))
}

fn unroll_error_target(error: &str) -> Option<&str> {
    error
        .split_once("\n  loop: for ")
        .and_then(|(_, rest)| rest.split_whitespace().next())
}

fn unroll_error_location(error: &str) -> Option<(usize, usize)> {
    let location = error
        .split_once("\n  location: ")
        .and_then(|(_, rest)| rest.lines().next())?
        .trim();
    let (line, column) = location.split_once(':')?;
    Some((line.parse().ok()?, column.parse().ok()?))
}

fn unroll_error_source_key(error: &str) -> Option<&str> {
    error
        .split_once("\n  source-key: ")
        .and_then(|(_, rest)| rest.lines().next())
        .map(str::trim)
        .filter(|source_key| !source_key.is_empty())
}

fn find_source_location(source: &str, needle: &str) -> Option<(usize, usize)> {
    source.lines().enumerate().find_map(|(line_index, line)| {
        line.find(needle)
            .map(|column_index| (line_index + 1, column_index + 1))
    })
}

fn find_for_loop_location_for_target(source: &str, target: &str) -> Option<(usize, usize)> {
    let needle = format!("for {target} ");
    source.lines().enumerate().find_map(|(line_index, line)| {
        let column_index = line.find(&needle)?;
        line[column_index..]
            .trim_start()
            .starts_with(&needle)
            .then_some((line_index + 1, column_index + 1))
    })
}

fn find_for_loop_location_for_source_key(source: &str, source_key: &str) -> Option<(usize, usize)> {
    source.lines().enumerate().find_map(|(line_index, line)| {
        let statement = line.trim_start();
        if !statement.starts_with("for ") {
            return None;
        }
        if Transpiler::source_line_key(statement) != source_key {
            return None;
        }
        let column = line.len() - statement.len() + 1;
        Some((line_index + 1, column))
    })
}

fn find_for_loop_location(source: &str) -> Option<(usize, usize)> {
    source.lines().enumerate().find_map(|(line_index, line)| {
        let column_index = line.find("for ")?;
        line[column_index..]
            .trim_start()
            .starts_with("for ")
            .then_some((line_index + 1, column_index + 1))
    })
}

fn python_compat_report_success(enabled: bool) -> Option<CheckPythonCompatReport> {
    enabled.then(|| CheckPythonCompatReport {
        enabled: true,
        mode: "diagnostics-only".to_string(),
        supported_constructs: python_compat_supported_constructs(),
        unsupported_detected: Vec::new(),
    })
}

fn python_compat_report_from_diagnostics(
    enabled: bool,
    file_diagnostics: &[FileSourceDiagnostics],
    config_dir: &Path,
) -> Option<CheckPythonCompatReport> {
    if !enabled {
        return None;
    }

    let unsupported_detected = file_diagnostics
        .iter()
        .flat_map(|file| {
            let relative_path = file.path.strip_prefix(config_dir).unwrap_or(&file.path);
            let file_name = path_display(relative_path);
            file.diagnostics
                .iter()
                .filter(|diagnostic| is_python_compat_diagnostic(diagnostic))
                .map(move |diagnostic| CheckPythonCompatDiagnosticReport {
                    file: file_name.clone(),
                    line: diagnostic.line,
                    column: diagnostic.column,
                    kind: diagnostic.kind.clone(),
                    message: diagnostic.message.clone(),
                    help: diagnostic.help.clone(),
                })
        })
        .collect();

    Some(CheckPythonCompatReport {
        enabled: true,
        mode: "diagnostics-only".to_string(),
        supported_constructs: python_compat_supported_constructs(),
        unsupported_detected,
    })
}

fn python_compat_supported_constructs() -> Vec<String> {
    vec!["pass statement as an explicit no-op".to_string()]
}

fn is_python_compat_diagnostic(diagnostic: &SourceDiagnostic) -> bool {
    diagnostic.kind.starts_with("unsupported-")
        || matches!(
            diagnostic.kind.as_str(),
            "parse"
                | "no-op-expression"
                | "missing-import"
                | "missing-import-item"
                | "duplicate-function-parameter"
                | "unclosed-delimiter"
                | "unmatched-delimiter"
                | "unterminated-string"
                | "unexpected-indentation"
                | "inconsistent-indentation"
        )
}

fn plugin_report(enabled: bool, project_root: &Path) -> Option<CheckPluginReport> {
    if !enabled {
        return None;
    }

    let mut report = CheckPluginReport {
        enabled: true,
        manifests_checked: 0,
        manifests: Vec::new(),
        diagnostics: Vec::new(),
    };

    let plugins_dir = project_root.join("plugins");
    match fs::symlink_metadata(&plugins_dir) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            report.diagnostics.push(plugin_host_diagnostic(
                "manifest-safety",
                "error",
                format!(
                    "Refusing to inspect experimental plugin manifests through symlinked directory {}.",
                    plugins_dir.display()
                ),
            ));
        }
        Ok(metadata) if metadata.is_dir() => inspect_plugin_manifest_dir(&plugins_dir, &mut report),
        Ok(_) => report.diagnostics.push(plugin_host_diagnostic(
            "manifest-safety",
            "error",
            format!(
                "Refusing to inspect experimental plugin manifests because {} is not a directory.",
                plugins_dir.display()
            ),
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => report.diagnostics.push(plugin_host_diagnostic(
            "manifest-safety",
            "error",
            format!(
                "Failed to inspect experimental plugin manifest directory {}: {}",
                plugins_dir.display(),
                error
            ),
        )),
    }

    if report.manifests_checked == 0 && report.diagnostics.is_empty() {
        report.diagnostics.push(plugin_host_diagnostic(
            "host-skeleton",
            "warning",
            "Experimental plugin host is enabled, but no plugin manifests were found; no plugins were run.".to_string(),
        ));
    } else if !report.manifests.is_empty() {
        report.diagnostics.push(plugin_host_diagnostic(
            "host-skeleton",
            "warning",
            "Experimental plugin manifests were parsed in read-only mode; execution is not implemented, so no plugins were run.".to_string(),
        ));
    }

    Some(report)
}

fn inspect_plugin_manifest_dir(plugins_dir: &Path, report: &mut CheckPluginReport) {
    let entries = match fs::read_dir(plugins_dir) {
        Ok(entries) => entries,
        Err(error) => {
            report.diagnostics.push(plugin_host_diagnostic(
                "manifest-safety",
                "error",
                format!(
                    "Failed to read experimental plugin manifest directory {}: {}",
                    plugins_dir.display(),
                    error
                ),
            ));
            return;
        }
    };

    let mut manifest_paths = Vec::new();
    for entry in entries {
        match entry {
            Ok(entry) => {
                let path = entry.path();
                if path.extension().and_then(|extension| extension.to_str()) == Some("toml") {
                    manifest_paths.push(path);
                }
            }
            Err(error) => report.diagnostics.push(plugin_host_diagnostic(
                "manifest-safety",
                "error",
                format!(
                    "Failed to inspect an experimental plugin manifest entry in {}: {}",
                    plugins_dir.display(),
                    error
                ),
            )),
        }
    }
    manifest_paths.sort();

    for manifest_path in manifest_paths {
        report.manifests_checked += 1;
        inspect_plugin_manifest(&manifest_path, report);
    }
}

fn inspect_plugin_manifest(manifest_path: &Path, report: &mut CheckPluginReport) {
    match fs::symlink_metadata(manifest_path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            report.diagnostics.push(plugin_manifest_diagnostic(
                manifest_path,
                "manifest-safety",
                "error",
                "Refusing to inspect symlinked experimental plugin manifest.".to_string(),
            ));
            return;
        }
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => {
            report.diagnostics.push(plugin_manifest_diagnostic(
                manifest_path,
                "manifest-safety",
                "error",
                "Refusing to inspect experimental plugin manifest because it is not a file."
                    .to_string(),
            ));
            return;
        }
        Err(error) => {
            report.diagnostics.push(plugin_manifest_diagnostic(
                manifest_path,
                "manifest-safety",
                "error",
                format!("Failed to inspect experimental plugin manifest: {error}"),
            ));
            return;
        }
    }

    let manifest_source = match fs::read_to_string(manifest_path) {
        Ok(source) => source,
        Err(error) => {
            report.diagnostics.push(plugin_manifest_diagnostic(
                manifest_path,
                "manifest-read",
                "error",
                format!("Failed to read experimental plugin manifest: {error}"),
            ));
            return;
        }
    };

    let manifest: PluginManifestDraft = match toml::from_str(&manifest_source) {
        Ok(manifest) => manifest,
        Err(error) => {
            report.diagnostics.push(plugin_manifest_diagnostic(
                manifest_path,
                "manifest-parse",
                "error",
                format!("Failed to parse experimental plugin manifest: {error}"),
            ));
            return;
        }
    };

    if let Err(error) = validate_plugin_manifest(&manifest) {
        report.diagnostics.push(CheckPluginDiagnosticReport {
            kind: "experimental-plugin-diagnostic".to_string(),
            plugin: manifest.name,
            plugin_kind: "manifest-draft".to_string(),
            severity: "error".to_string(),
            message: format!(
                "Rejected experimental plugin manifest {}: {}",
                manifest_path.display(),
                error
            ),
        });
        return;
    }

    report.diagnostics.push(CheckPluginDiagnosticReport {
        kind: "experimental-plugin-diagnostic".to_string(),
        plugin: manifest.name.clone(),
        plugin_kind: "manifest-draft".to_string(),
        severity: "warning".to_string(),
        message: format!(
            "Experimental plugin manifest {} was parsed, but plugin execution is disabled in 0.9; no plugin code was run.",
            manifest_path.display()
        ),
    });
    report.manifests.push(CheckPluginManifestReport {
        name: manifest.name,
        plugin_version: manifest.plugin_version,
        kind: manifest.kind,
        capabilities: enabled_plugin_capabilities(&manifest.capabilities),
        path: path_display(manifest_path),
    });
}

fn validate_plugin_manifest(manifest: &PluginManifestDraft) -> Result<(), String> {
    if manifest.plugin_version != 1 {
        return Err(format!(
            "unsupported plugin_version {}; expected 1",
            manifest.plugin_version
        ));
    }
    if !is_safe_plugin_name(&manifest.name) {
        return Err(
            "plugin name must contain only lowercase letters, digits, '.', '_', or '-'".to_string(),
        );
    }
    if manifest.kind != "diagnostics" {
        return Err(format!(
            "unsupported plugin kind '{}'; expected 'diagnostics'",
            manifest.kind
        ));
    }
    Ok(())
}

fn enabled_plugin_capabilities(capabilities: &PluginManifestCapabilities) -> Vec<String> {
    let mut enabled = Vec::new();
    if capabilities.read_project_metadata {
        enabled.push("read_project_metadata".to_string());
    }
    if capabilities.read_source_text {
        enabled.push("read_source_text".to_string());
    }
    if capabilities.emit_diagnostics {
        enabled.push("emit_diagnostics".to_string());
    }
    enabled
}

fn is_safe_plugin_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().all(|ch| {
            ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '.' | '_' | '-')
        })
}

fn plugin_host_diagnostic(
    plugin_kind: &str,
    severity: &str,
    message: String,
) -> CheckPluginDiagnosticReport {
    CheckPluginDiagnosticReport {
        kind: "experimental-plugin-diagnostic".to_string(),
        plugin: "cobble.plugin_host".to_string(),
        plugin_kind: plugin_kind.to_string(),
        severity: severity.to_string(),
        message,
    }
}

fn plugin_manifest_diagnostic(
    manifest_path: &Path,
    plugin_kind: &str,
    severity: &str,
    message: String,
) -> CheckPluginDiagnosticReport {
    CheckPluginDiagnosticReport {
        kind: "experimental-plugin-diagnostic".to_string(),
        plugin: path_display(manifest_path),
        plugin_kind: plugin_kind.to_string(),
        severity: severity.to_string(),
        message,
    }
}

fn print_plugin_report_human(report: Option<&CheckPluginReport>) {
    let Some(report) = report else {
        return;
    };

    for diagnostic in &report.diagnostics {
        println!(
            "{}: experimental plugin {} reported {}: {}",
            diagnostic.severity, diagnostic.plugin, diagnostic.plugin_kind, diagnostic.message
        );
    }
}

fn plugin_error_count(report: Option<&CheckPluginReport>) -> usize {
    report
        .map(|report| {
            report
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.severity == "error")
                .count()
        })
        .unwrap_or(0)
}

fn print_python_compat_report_human(report: Option<&CheckPythonCompatReport>) {
    let Some(report) = report else {
        return;
    };

    println!(
        "warning: experimental Python compatibility report enabled; mode {}; supported: {}",
        report.mode,
        report.supported_constructs.join(", ")
    );
    if !report.unsupported_detected.is_empty() {
        println!(
            "warning: experimental Python compatibility detected {} unsupported construct(s); these remain errors",
            report.unsupported_detected.len()
        );
    }
}

fn check_file_reports(
    files_to_check: &[PathBuf],
    parsed_files: &[ParsedSourceFile],
    config_dir: &Path,
) -> Vec<CheckFileReport> {
    files_to_check
        .iter()
        .zip(parsed_files.iter())
        .map(|(file_path, parsed)| {
            let relative_path = file_path.strip_prefix(config_dir).unwrap_or(file_path);
            let mut function_count = 0;
            let mut command_count = 0;

            for statement in &parsed.program.statements {
                match statement {
                    crate::ast::Statement::FunctionDef(_) => function_count += 1,
                    crate::ast::Statement::MinecraftCommand(_) => command_count += 1,
                    _ => {}
                }
            }

            CheckFileReport {
                file: path_display(relative_path),
                imports: parsed.program.imports.len(),
                functions: function_count,
                commands: command_count,
            }
        })
        .collect()
}

fn diagnostic_reports(
    file_diagnostics: &[FileSourceDiagnostics],
    config_dir: &Path,
) -> Vec<CheckDiagnosticReport> {
    file_diagnostics
        .iter()
        .flat_map(|file| {
            let relative_path = file.path.strip_prefix(config_dir).unwrap_or(&file.path);
            let file_name = path_display(relative_path);
            file.diagnostics
                .iter()
                .map(move |diagnostic| diagnostic_report(&file_name, diagnostic, &file.source))
        })
        .collect()
}

fn symbol_reports_from_parsed(
    parsed_files: &[ParsedSourceFile],
    config_dir: &Path,
    include_symbols: bool,
) -> Option<Vec<CheckSymbolReport>> {
    include_symbols.then(|| {
        parsed_files
            .iter()
            .flat_map(|file| {
                let relative_path = file.path.strip_prefix(config_dir).unwrap_or(&file.path);
                scan_source_symbols(&path_display(relative_path), &file.source)
            })
            .collect()
    })
}

fn symbol_reports_from_diagnostics(
    file_diagnostics: &[FileSourceDiagnostics],
    config_dir: &Path,
    include_symbols: bool,
) -> Option<Vec<CheckSymbolReport>> {
    include_symbols.then(|| {
        file_diagnostics
            .iter()
            .flat_map(|file| {
                let relative_path = file.path.strip_prefix(config_dir).unwrap_or(&file.path);
                scan_source_symbols(&path_display(relative_path), &file.source)
            })
            .collect()
    })
}

fn scan_source_symbols(file_name: &str, source: &str) -> Vec<CheckSymbolReport> {
    let mut symbols = Vec::new();
    let mut block_string = None;

    for (line_index, raw_line) in source.lines().enumerate() {
        let line = raw_line.trim_start();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if skip_block_string_line(&mut block_string, line) {
            continue;
        }

        let line_number = line_index + 1;
        let column = raw_line.len() - line.len() + 1;

        if let Some(module) = import_module_name(line) {
            symbols.push(symbol_report(
                file_name,
                module,
                "import",
                line_number,
                column,
                None,
            ));
            continue;
        }

        if let Some((module, names)) = from_import_names(line) {
            for name in names {
                symbols.push(symbol_report(
                    file_name,
                    &format!("{module}.{name}"),
                    "import",
                    line_number,
                    column,
                    Some(module.clone()),
                ));
            }
            continue;
        }

        if let Some(name) = named_declaration(line, "def ", '(') {
            symbols.push(symbol_report(
                file_name,
                name,
                "function",
                line_number,
                column,
                None,
            ));
            continue;
        }

        if let Some(name) = named_declaration(line, "const ", '=') {
            symbols.push(symbol_report(
                file_name,
                name,
                "const",
                line_number,
                column,
                None,
            ));
            continue;
        }

        if let Some(name) = selector_alias_name(line) {
            symbols.push(symbol_report(
                file_name,
                &name,
                "selector_alias",
                line_number,
                column,
                None,
            ));
            continue;
        }

        if let Some(name) = entity_template_name(line) {
            symbols.push(symbol_report(
                file_name,
                &name,
                "entity_template",
                line_number,
                column,
                None,
            ));
            continue;
        }

        if let Some((helper, resource_id)) = datapack_resource_name(line) {
            symbols.push(symbol_report(
                file_name,
                &format!("{helper}:{resource_id}"),
                "datapack_resource",
                line_number,
                column,
                Some(helper),
            ));
        }
    }

    symbols
}

fn symbol_report(
    file_name: &str,
    name: &str,
    kind: &str,
    line: usize,
    column: usize,
    detail: Option<String>,
) -> CheckSymbolReport {
    CheckSymbolReport {
        file: file_name.to_string(),
        name: name.to_string(),
        kind: kind.to_string(),
        line,
        column,
        detail,
    }
}

fn skip_block_string_line(block_string: &mut Option<&'static str>, line: &str) -> bool {
    if let Some(delimiter) = block_string {
        if line.contains(*delimiter) {
            *block_string = None;
        }
        return true;
    }

    for delimiter in ["\"\"\"", "'''"] {
        if let Some(rest) = line.strip_prefix(delimiter) {
            if !rest.contains(delimiter) {
                *block_string = Some(delimiter);
            }
            return true;
        }
    }

    false
}

fn import_module_name(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("import ")?;
    first_name(rest)
}

fn from_import_names(line: &str) -> Option<(String, Vec<String>)> {
    let rest = line.strip_prefix("from ")?;
    let (module, imports) = rest.split_once(" import ")?;
    let module = first_name(module)?.to_string();
    let names = imports
        .split(',')
        .filter_map(first_name)
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    Some((module, names))
}

fn named_declaration<'a>(line: &'a str, prefix: &str, delimiter: char) -> Option<&'a str> {
    let rest = line.strip_prefix(prefix)?;
    let name = rest.split(delimiter).next()?.trim();
    is_identifier(name).then_some(name)
}

fn selector_alias_name(line: &str) -> Option<String> {
    let name = line.split_once('=')?.0.trim();
    let identifier = name.strip_prefix('@')?;
    is_identifier(identifier).then(|| name.to_string())
}

fn entity_template_name(line: &str) -> Option<String> {
    let rest = line.strip_prefix("define @")?;
    let name = first_name(rest)?;
    Some(format!("@{name}"))
}

fn datapack_resource_name(line: &str) -> Option<(String, String)> {
    let rest = line.strip_prefix("datapack.")?;
    let (helper, args) = rest.split_once('(')?;
    let helper = helper.trim();
    if !is_identifier(helper) {
        return None;
    }
    let resource_id = first_quoted_string(args)?;
    Some((helper.to_string(), resource_id.to_string()))
}

fn first_name(text: &str) -> Option<&str> {
    let name = text
        .trim()
        .split(|ch: char| ch.is_whitespace() || ch == ',' || ch == ')' || ch == ':')
        .next()?;
    let name = name
        .split_once(" as ")
        .map(|(name, _)| name)
        .unwrap_or(name);
    is_dotted_identifier(name).then_some(name)
}

fn first_quoted_string(text: &str) -> Option<&str> {
    let text = text.trim_start();
    let quote = text.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let rest = &text[quote.len_utf8()..];
    let end = rest.find(quote)?;
    Some(&rest[..end])
}

fn is_dotted_identifier(name: &str) -> bool {
    !name.is_empty() && name.split('.').all(is_identifier)
}

fn is_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn diagnostic_report(
    file_name: &str,
    diagnostic: &SourceDiagnostic,
    source: &str,
) -> CheckDiagnosticReport {
    CheckDiagnosticReport {
        file: file_name.to_string(),
        line: diagnostic.line,
        column: diagnostic.column,
        severity: diagnostic.severity.as_str().to_string(),
        kind: diagnostic.kind.clone(),
        message: diagnostic.message.clone(),
        help: diagnostic.help.clone(),
        formatted: diagnostic.format_with_source(file_name, source),
    }
}

fn print_check_json(report: &CheckReport) -> Result<(), String> {
    let output = serde_json::to_string_pretty(report)
        .map_err(|error| format!("Failed to format check JSON: {error}"))?;
    println!("{output}");
    Ok(())
}

fn path_display(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn find_config(input: &Option<PathBuf>) -> Option<PathBuf> {
    if let Some(path) = input {
        if path.is_file() {
            // If input is a file, look for config in parent directories
            if let Some(parent) = path.parent() {
                return CobbleConfig::find_in_path(parent);
            }
        } else {
            // If input is a directory, look for config in it
            return CobbleConfig::find_in_path(path);
        }
    }
    // Look in current directory
    CobbleConfig::find_in_path(".")
}
