use super::{find_cobble_files, resolve_entry_points};
use crate::config::CobbleConfig;
use crate::diagnostics::{
    parse_source_files, FileSourceDiagnostics, ParsedSourceFile, SourceDiagnostic,
};
use crate::error::report_file_source_diagnostics;
use serde::Serialize;
use std::path::{Path, PathBuf};

pub struct CheckOptions {
    pub input: Option<PathBuf>,
    pub json: bool,
}

#[derive(Serialize)]
struct CheckReport {
    ok: bool,
    source: String,
    files_checked: usize,
    files: Vec<CheckFileReport>,
    diagnostics: Vec<CheckDiagnosticReport>,
    error_count: usize,
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

pub fn check(options: CheckOptions) -> Result<(), String> {
    // Try to find cobble.toml
    let (config, config_dir) = if let Some(config_path) = find_config(&options.input) {
        let config = CobbleConfig::load(&config_path)?;
        let config_dir = config_path.parent().unwrap().to_path_buf();
        (Some(config), config_dir)
    } else {
        (
            None,
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        )
    };

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
        if options.json {
            print_check_json(&CheckReport {
                ok: true,
                source: path_display(&source_path),
                files_checked: 0,
                files: Vec::new(),
                diagnostics: Vec::new(),
                error_count: 0,
            })?;
        } else {
            println!("No Cobble files found to check");
        }
        return Ok(());
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
            if options.json {
                print_check_json(&CheckReport {
                    ok: false,
                    source: path_display(&source_path),
                    files_checked: files_to_check.len(),
                    files: Vec::new(),
                    diagnostics: diagnostic_reports(&file_diagnostics, &config_dir),
                    error_count: total_errors,
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
            }
            return Err(format!("Validation failed with {} error(s)", total_errors));
        }
    };

    let files = check_file_reports(&files_to_check, &parsed_files, &config_dir);
    if options.json {
        print_check_json(&CheckReport {
            ok: true,
            source: path_display(&source_path),
            files_checked: files_to_check.len(),
            files,
            diagnostics: Vec::new(),
            error_count: 0,
        })?;
        return Ok(());
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

    Ok(())
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
