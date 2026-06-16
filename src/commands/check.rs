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
    pub symbols: bool,
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

pub fn check(options: CheckOptions) -> Result<(), String> {
    if options.symbols && !options.json {
        return Err("--symbols requires --json".to_string());
    }

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
                schema_version: 1,
                ok: true,
                source: path_display(&source_path),
                files_checked: 0,
                files: Vec::new(),
                diagnostics: Vec::new(),
                error_count: 0,
                experimental_symbols: options.symbols.then(Vec::new),
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
                    schema_version: 1,
                    ok: false,
                    source: path_display(&source_path),
                    files_checked: files_to_check.len(),
                    files: Vec::new(),
                    diagnostics: diagnostic_reports(&file_diagnostics, &config_dir),
                    error_count: total_errors,
                    experimental_symbols: symbol_reports_from_diagnostics(
                        &file_diagnostics,
                        &config_dir,
                        options.symbols,
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
            }
            return Err(format!("Validation failed with {} error(s)", total_errors));
        }
    };

    let files = check_file_reports(&files_to_check, &parsed_files, &config_dir);
    if options.json {
        print_check_json(&CheckReport {
            schema_version: 1,
            ok: true,
            source: path_display(&source_path),
            files_checked: files_to_check.len(),
            files,
            diagnostics: Vec::new(),
            error_count: 0,
            experimental_symbols: symbol_reports_from_parsed(
                &parsed_files,
                &config_dir,
                options.symbols,
            ),
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
