use super::{find_cobble_files, resolve_entry_points};
use crate::commands::output_safety::ensure_no_symlinks_from;
use crate::config::CobbleConfig;
use crate::diagnostics::{
    parse_source_files, python_compat_observed_constructs, python_compat_suggestion_for_kind,
    python_compat_supported_constructs, warning_diagnostics, FileSourceDiagnostics,
    ParsedSourceFile, SourceDiagnostic,
};
use crate::error::report_file_source_diagnostics;
use crate::pack_format::{PackFormat, SUPPORTED_PACK_FORMAT};
use crate::transpiler::{TranspileError, Transpiler, TranspilerOptions};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub struct CheckOptions {
    pub input: Option<PathBuf>,
    pub json: bool,
    pub symbols: bool,
    pub experimental_plugins: bool,
    pub experimental_python_compat: bool,
    /// The pack's namespace, as `cobble build --namespace` takes it: the
    /// project's, or `cobble` outside a project, when `None`.
    pub namespace: Option<String>,
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
    warning_count: usize,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    minimum_cobble_version: Option<String>,
    diagnostic_rules: Vec<String>,
    capabilities: Vec<String>,
    #[serde(skip)]
    can_read_source_text: bool,
    #[serde(skip)]
    can_emit_diagnostics: bool,
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
    observed_constructs: Vec<String>,
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
    suggested_cobble_alternative: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PluginManifestDraft {
    plugin_version: u32,
    name: String,
    kind: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    minimum_cobble_version: Option<String>,
    #[serde(default)]
    diagnostic_rules: Vec<String>,
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

const MAX_PLUGIN_MANIFEST_BYTES: u64 = 64 * 1024;

pub fn check(options: CheckOptions) -> Result<(), String> {
    if options.symbols && !options.json {
        return Err("--symbols requires --json".to_string());
    }

    let (config, config_dir) = load_check_config(&options)?;
    let experimental_plugins =
        options.experimental_plugins || config.as_ref().is_some_and(|cfg| cfg.experimental.plugins);
    let experimental_resource_pack = config
        .as_ref()
        .is_some_and(|cfg| cfg.experimental.resource_pack);
    let experimental_python_compat = options.experimental_python_compat
        || config
            .as_ref()
            .is_some_and(|cfg| cfg.experimental.python_compat);
    let mut plugin_report = plugin_report(experimental_plugins, &config_dir);

    let source_path = if let Some(ref input_path) = options.input {
        input_path.clone()
    } else if let Some(ref cfg) = config {
        config_dir.join(&cfg.build.source)
    } else {
        return Err("No input specified and no cobble.toml found".to_string());
    };
    let files_to_check = files_to_check(&options, config.as_ref(), &source_path)?;
    let run = CheckRun {
        options: &options,
        config_dir: &config_dir,
        source_path: &source_path,
        files_checked: files_to_check.len(),
        experimental_python_compat,
    };

    if files_to_check.is_empty() {
        return run.finish_without_files(plugin_report);
    }

    if !options.json {
        println!("Checking {} file(s)...", files_to_check.len());
    }

    let parsed_files = match parse_source_files(&files_to_check) {
        Ok(parsed_files) => parsed_files,
        Err(file_diagnostics) => {
            apply_declarative_plugin_rules(
                plugin_report.as_mut(),
                file_diagnostics
                    .iter()
                    .map(|file| (file.path.as_path(), file.source.as_str())),
                &config_dir,
            );
            let observed = observed_constructs_from_diagnostics(&file_diagnostics);
            return run.fail(
                plugin_report,
                Vec::new(),
                &file_diagnostics,
                &[],
                observed,
                true,
            );
        }
    };

    let files = check_file_reports(&files_to_check, &parsed_files, &config_dir);
    apply_declarative_plugin_rules(
        plugin_report.as_mut(),
        parsed_files
            .iter()
            .map(|file| (file.path.as_path(), file.source.as_str())),
        &config_dir,
    );
    let SemanticCheck { errors, warnings } = semantic_check(
        &files_to_check,
        &parsed_files,
        config.as_ref(),
        &config_dir,
        experimental_resource_pack,
        options.namespace.as_deref(),
    );
    let observed = observed_constructs_from_parsed(&parsed_files);
    if !errors.is_empty() {
        return run.fail(plugin_report, files, &errors, &warnings, observed, false);
    }
    run.pass(plugin_report, files, &parsed_files, &warnings, observed)
}

/// Loads `cobble.toml` when there is one. Under `--json`, a config error is
/// also printed as a JSON report.
fn load_check_config(options: &CheckOptions) -> Result<(Option<CobbleConfig>, PathBuf), String> {
    let Some(config_path) = super::find_config(options.input.as_deref()) else {
        return Ok((
            None,
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        ));
    };
    let config = match CobbleConfig::load(&config_path) {
        Ok(config) => config,
        Err(error) if options.json => {
            print_config_error_json(&config_path, &error, options.symbols)?;
            return Err(format!("Config validation failed: {error}"));
        }
        Err(error) => return Err(error),
    };
    let config_dir = config_path.parent().unwrap().to_path_buf();
    Ok((Some(config), config_dir))
}

fn files_to_check(
    options: &CheckOptions,
    config: Option<&CobbleConfig>,
    source_path: &Path,
) -> Result<Vec<PathBuf>, String> {
    if source_path.is_file() {
        return Ok(vec![source_path.to_path_buf()]);
    }
    if !source_path.is_dir() {
        return Err(format!("Source path does not exist: {:?}", source_path));
    }
    let configured_entry_points = match (&options.input, config) {
        (None, Some(cfg)) => cfg.build.entry_points.clone(),
        _ => Vec::new(),
    };
    if configured_entry_points.is_empty() {
        find_cobble_files(source_path)
    } else {
        resolve_entry_points(source_path, &configured_entry_points)
    }
}

/// One `cobble check` run, and how it reports its outcome as JSON or text.
struct CheckRun<'a> {
    options: &'a CheckOptions,
    config_dir: &'a Path,
    source_path: &'a Path,
    files_checked: usize,
    experimental_python_compat: bool,
}

impl CheckRun<'_> {
    fn finish_without_files(&self, plugin_report: Option<CheckPluginReport>) -> Result<(), String> {
        let plugin_errors = plugin_error_count(plugin_report.as_ref());
        let python_compat =
            python_compat_report_success(self.experimental_python_compat, Vec::new());
        if self.options.json {
            print_check_json(&CheckReport {
                schema_version: 1,
                ok: plugin_errors == 0,
                source: path_display(self.source_path),
                files_checked: 0,
                files: Vec::new(),
                diagnostics: Vec::new(),
                error_count: plugin_errors,
                warning_count: 0,
                experimental_symbols: self.options.symbols.then(Vec::new),
                experimental_plugins: plugin_report,
                experimental_python_compat: python_compat,
            })?;
        } else {
            println!("No Cobble files found to check");
            print_python_compat_report_human(python_compat.as_ref());
            print_plugin_report_human(plugin_report.as_ref());
        }
        plugin_result(plugin_errors)
    }

    /// Reports parse or semantic errors, and the warnings found before
    /// them. `list_files` prints each failing file first, as parse failures
    /// do.
    fn fail(
        &self,
        plugin_report: Option<CheckPluginReport>,
        files: Vec<CheckFileReport>,
        diagnostics: &[FileSourceDiagnostics],
        warnings: &[FileSourceDiagnostics],
        observed_constructs: Vec<String>,
        list_files: bool,
    ) -> Result<(), String> {
        let errors = diagnostic_count(diagnostics);
        let plugin_errors = plugin_error_count(plugin_report.as_ref());
        let python_compat = python_compat_report_from_diagnostics(
            self.experimental_python_compat,
            diagnostics,
            observed_constructs,
            self.config_dir,
        );
        if self.options.json {
            print_check_json(&CheckReport {
                schema_version: 1,
                ok: false,
                source: path_display(self.source_path),
                files_checked: self.files_checked,
                files,
                diagnostics: diagnostic_reports(
                    diagnostics.iter().chain(warnings),
                    self.config_dir,
                ),
                error_count: errors + plugin_errors,
                warning_count: diagnostic_count(warnings),
                experimental_symbols: symbol_reports_from_diagnostics(
                    diagnostics,
                    self.config_dir,
                    self.options.symbols,
                ),
                experimental_plugins: plugin_report,
                experimental_python_compat: python_compat,
            })?;
        } else {
            if list_files {
                for file in diagnostics {
                    let relative_path = file
                        .path
                        .strip_prefix(self.config_dir)
                        .unwrap_or(&file.path);
                    println!("  ✗ {:?}:", relative_path);
                }
            }
            report_file_source_diagnostics(diagnostics);
            report_file_source_diagnostics(warnings);
            println!();
            println!(
                "✗ {} error(s) found in {} file(s)",
                errors,
                diagnostics.len()
            );
            print_warning_count_human(warnings);
            print_python_compat_report_human(python_compat.as_ref());
            print_plugin_report_human(plugin_report.as_ref());
        }
        Err(format!(
            "Validation failed with {} error(s)",
            errors + plugin_errors
        ))
    }

    fn pass(
        &self,
        plugin_report: Option<CheckPluginReport>,
        files: Vec<CheckFileReport>,
        parsed_files: &[ParsedSourceFile],
        warnings: &[FileSourceDiagnostics],
        observed_constructs: Vec<String>,
    ) -> Result<(), String> {
        let plugin_errors = plugin_error_count(plugin_report.as_ref());
        let python_compat =
            python_compat_report_success(self.experimental_python_compat, observed_constructs);
        if self.options.json {
            print_check_json(&CheckReport {
                schema_version: 1,
                ok: plugin_errors == 0,
                source: path_display(self.source_path),
                files_checked: self.files_checked,
                files,
                diagnostics: diagnostic_reports(warnings, self.config_dir),
                error_count: plugin_errors,
                warning_count: diagnostic_count(warnings),
                experimental_symbols: symbol_reports_from_parsed(
                    parsed_files,
                    self.config_dir,
                    self.options.symbols,
                ),
                experimental_plugins: plugin_report,
                experimental_python_compat: python_compat,
            })?;
        } else {
            for file in files {
                println!(
                    "  ✓ {:?}: {} imports, {} functions, {} commands",
                    file.file, file.imports, file.functions, file.commands
                );
            }
            report_file_source_diagnostics(warnings);
            println!();
            println!("✓ All files passed validation!");
            print_warning_count_human(warnings);
            print_python_compat_report_human(python_compat.as_ref());
            print_plugin_report_human(plugin_report.as_ref());
        }
        plugin_result(plugin_errors)
    }
}

fn diagnostic_count(files: &[FileSourceDiagnostics]) -> usize {
    files.iter().map(|file| file.diagnostics.len()).sum()
}

fn print_warning_count_human(warnings: &[FileSourceDiagnostics]) {
    let count = diagnostic_count(warnings);
    if count > 0 {
        println!("{count} warning(s)");
    }
}

fn plugin_result(plugin_errors: usize) -> Result<(), String> {
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
        warning_count: 0,
        experimental_symbols: include_symbols.then(Vec::new),
        experimental_plugins: None,
        experimental_python_compat: None,
    })
}

/// The errors and warnings that compiling the checked files reports.
struct SemanticCheck {
    errors: Vec<FileSourceDiagnostics>,
    warnings: Vec<FileSourceDiagnostics>,
}

fn semantic_check(
    files_to_check: &[PathBuf],
    parsed_files: &[ParsedSourceFile],
    config: Option<&CobbleConfig>,
    config_dir: &Path,
    experimental_resource_pack: bool,
    namespace: Option<&str>,
) -> SemanticCheck {
    // The same namespace `cobble build` uses, so references to the pack's
    // own resources are checked alike.
    let namespace = namespace
        .map(str::to_string)
        .or_else(|| config.map(|config| config.project.namespace.clone()))
        .unwrap_or_else(|| "cobble".to_string());
    let pack_format = config
        .and_then(|config| PackFormat::parse_format(&config.project.pack_format).ok())
        .unwrap_or(SUPPORTED_PACK_FORMAT);
    let stdlib_version = config.map(|config| config.stdlib.version).unwrap_or(2);
    let output_dir = config
        .map(|config| config_dir.join(&config.build.output))
        .unwrap_or_else(|| config_dir.join(".cobble-check-output"));

    let mut transpiler = Transpiler::with_options(TranspilerOptions {
        pack_format,
        stdlib_version: Some(stdlib_version),
        command_limit: config.and_then(|config| config.build.command_limit),
        experimental_resource_pack,
        source_display_root: Some(config_dir.to_path_buf()),
        ..TranspilerOptions::new(namespace, output_dir)
    });

    // The path to report for a file a location names, and its source.
    let report_file = |file: &Path| {
        files_to_check
            .iter()
            .zip(parsed_files)
            .find(|(checked, _)| canonical_or_original(checked) == file)
            .map(|(checked, parsed)| (checked.clone(), parsed.source.clone()))
            .or_else(|| imported_file_source(file, config_dir))
    };

    // Keep checking the remaining files after a failure so one run reveals
    // every file's first semantic error. A file that an earlier file imports
    // was checked with it.
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    // A file can write the lists and maps of a file checked before it.
    for parsed in parsed_files {
        transpiler.record_container_writes(&parsed.program);
        transpiler.record_module_variables(&parsed.path, &parsed.program);
    }
    for (path, parsed) in files_to_check.iter().zip(parsed_files.iter()) {
        if transpiler.has_compiled(path) {
            continue;
        }
        transpiler.set_current_file_with_source_spans(path, &parsed.source, &parsed.spans);
        let result = transpiler.transpile(&parsed.program);
        warnings.extend(warning_diagnostics(
            &transpiler.take_warnings(),
            path,
            &parsed.source,
            report_file,
        ));
        if let Err(error) = result {
            errors.push(transpile_error_report(error, path, parsed, report_file));
        }
    }
    // The resources a declaration names may be declared in a later file.
    if errors.is_empty() {
        if let Err(error) = transpiler.check_resource_references() {
            let declared_in = error.location.as_ref().and_then(|location| {
                files_to_check
                    .iter()
                    .zip(parsed_files.iter())
                    .find(|(path, _)| canonical_or_original(path) == location.file)
            });
            if let Some((path, parsed)) =
                declared_in.or_else(|| files_to_check.iter().zip(parsed_files.iter()).next())
            {
                errors.push(transpile_error_report(error, path, parsed, report_file));
            }
        }
    }

    SemanticCheck { errors, warnings }
}

/// Reports a failed transpile of `path`. An error raised in a file that
/// `path` imports is reported in that file.
fn transpile_error_report(
    mut error: TranspileError,
    path: &Path,
    parsed: &ParsedSourceFile,
    report_file: impl Fn(&Path) -> Option<(PathBuf, String)>,
) -> FileSourceDiagnostics {
    let error_file: Option<PathBuf> = error
        .location
        .as_ref()
        .map(|location| location.file.clone())
        .filter(|file| *file != canonical_or_original(path));
    let (report_path, source) = match error_file.map(|file| report_file(&file)) {
        None => (path.to_path_buf(), parsed.source.clone()),
        Some(Some(report)) => report,
        Some(None) => {
            // The location would point into a file that can't be shown.
            error.location = None;
            (path.to_path_buf(), parsed.source.clone())
        }
    };
    let diagnostic = SourceDiagnostic::from_transpile_error(&error, &source);
    FileSourceDiagnostics::new(report_path, source, vec![diagnostic])
}

/// The path to report and the source of an imported file outside the
/// checked files. The path is under `config_dir` when the file is.
fn imported_file_source(file: &Path, config_dir: &Path) -> Option<(PathBuf, String)> {
    let source = fs::read_to_string(file).ok()?;
    let path = file
        .strip_prefix(canonical_or_original(config_dir))
        .map(|relative| config_dir.join(relative))
        .unwrap_or_else(|_| file.to_path_buf());
    Some((path, source))
}

fn canonical_or_original(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn python_compat_report_success(
    enabled: bool,
    observed_constructs: Vec<String>,
) -> Option<CheckPythonCompatReport> {
    enabled.then(|| CheckPythonCompatReport {
        enabled: true,
        mode: "diagnostics-only".to_string(),
        supported_constructs: python_compat_supported_constructs(),
        observed_constructs,
        unsupported_detected: Vec::new(),
    })
}

fn python_compat_report_from_diagnostics(
    enabled: bool,
    file_diagnostics: &[FileSourceDiagnostics],
    observed_constructs: Vec<String>,
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
                    suggested_cobble_alternative: python_compat_suggestion_for_kind(
                        &diagnostic.kind,
                        &diagnostic.message,
                        diagnostic.help.as_deref(),
                    ),
                })
        })
        .collect();

    Some(CheckPythonCompatReport {
        enabled: true,
        mode: "diagnostics-only".to_string(),
        supported_constructs: python_compat_supported_constructs(),
        observed_constructs,
        unsupported_detected,
    })
}

fn observed_constructs_from_diagnostics(file_diagnostics: &[FileSourceDiagnostics]) -> Vec<String> {
    observed_constructs(file_diagnostics.iter().map(|file| file.source.as_str()))
}

fn observed_constructs_from_parsed(parsed_files: &[ParsedSourceFile]) -> Vec<String> {
    observed_constructs(parsed_files.iter().map(|file| file.source.as_str()))
}

fn observed_constructs<'a>(sources: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut observed = std::collections::HashSet::new();
    for source in sources {
        observed.extend(python_compat_observed_constructs(source));
    }

    python_compat_supported_constructs()
        .into_iter()
        .filter(|construct| observed.contains(construct))
        .collect()
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
    if let Err(error) =
        ensure_no_symlinks_from(project_root, &plugins_dir, "inspect plugin manifests")
    {
        report
            .diagnostics
            .push(plugin_host_diagnostic("manifest-safety", "error", error));
        return Some(report);
    }
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
            "Experimental plugin manifests were parsed in read-only mode; plugin code execution is not implemented.".to_string(),
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
    if let Err(error) = ensure_no_symlinks_from(
        manifest_path.parent().unwrap_or(manifest_path),
        manifest_path,
        "inspect plugin manifest",
    ) {
        report.diagnostics.push(plugin_manifest_diagnostic(
            manifest_path,
            "manifest-safety",
            "error",
            error,
        ));
        return;
    }
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

    match fs::metadata(manifest_path) {
        Ok(metadata) if metadata.len() > MAX_PLUGIN_MANIFEST_BYTES => {
            report.diagnostics.push(plugin_manifest_diagnostic(
                manifest_path,
                "manifest-read",
                "error",
                format!(
                    "Refusing to read experimental plugin manifest larger than {} bytes.",
                    MAX_PLUGIN_MANIFEST_BYTES
                ),
            ));
            return;
        }
        Ok(_) => {}
        Err(error) => {
            report.diagnostics.push(plugin_manifest_diagnostic(
                manifest_path,
                "manifest-read",
                "error",
                format!("Failed to inspect experimental plugin manifest size: {error}"),
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
            plugin: rejected_manifest_plugin_id(manifest_path, &manifest.name),
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
            "Experimental plugin manifest {} was parsed, but plugin execution is disabled in this release; no plugin code was run.",
            manifest_path.display()
        ),
    });
    report.manifests.push(CheckPluginManifestReport {
        name: manifest.name,
        plugin_version: manifest.plugin_version,
        kind: manifest.kind,
        description: manifest.description,
        minimum_cobble_version: manifest.minimum_cobble_version,
        diagnostic_rules: manifest.diagnostic_rules,
        capabilities: enabled_plugin_capabilities(&manifest.capabilities),
        can_read_source_text: manifest.capabilities.read_source_text,
        can_emit_diagnostics: manifest.capabilities.emit_diagnostics,
        path: path_display(manifest_path),
    });
}

fn apply_declarative_plugin_rules<'a>(
    report: Option<&mut CheckPluginReport>,
    sources: impl IntoIterator<Item = (&'a Path, &'a str)>,
    config_dir: &Path,
) {
    let Some(report) = report else {
        return;
    };

    let manifest_rules = report
        .manifests
        .iter()
        .flat_map(|manifest| {
            manifest.diagnostic_rules.iter().map(|rule| {
                (
                    manifest.name.clone(),
                    manifest.can_read_source_text,
                    manifest.can_emit_diagnostics,
                    rule.clone(),
                )
            })
        })
        .collect::<Vec<_>>();
    if manifest_rules.is_empty() {
        return;
    }

    let sources = sources.into_iter().collect::<Vec<_>>();
    for (plugin, can_read_source_text, can_emit_diagnostics, rule) in manifest_rules {
        if !can_read_source_text || !can_emit_diagnostics {
            report.diagnostics.push(CheckPluginDiagnosticReport {
                kind: "experimental-plugin-diagnostic".to_string(),
                plugin,
                plugin_kind: "declarative-rule".to_string(),
                severity: "warning".to_string(),
                message: format!(
                    "Experimental declarative rule {rule} requires read_source_text and emit_diagnostics capabilities; skipped."
                ),
            });
            continue;
        }
        match rule.as_str() {
            "example_lints.no_tellraw" => {
                apply_line_match_rule(
                    report,
                    &plugin,
                    &rule,
                    &sources,
                    config_dir,
                    line_uses_tellraw,
                    "avoid tellraw in this diagnostics profile",
                );
            }
            "example_lints.no_raw_op" => {
                apply_line_match_rule(
                    report,
                    &plugin,
                    &rule,
                    &sources,
                    config_dir,
                    |line| line_uses_command_word(line, "op"),
                    "avoid raw operator-permission commands in checked source",
                );
            }
            "example_lints.no_gamemode_creative" => {
                apply_line_match_rule(
                    report,
                    &plugin,
                    &rule,
                    &sources,
                    config_dir,
                    line_uses_gamemode_creative,
                    "avoid commands that switch players into creative mode",
                );
            }
            "example_lints.max_raw_command_length" => {
                apply_line_match_rule(
                    report,
                    &plugin,
                    &rule,
                    &sources,
                    config_dir,
                    line_has_long_raw_command,
                    "split long raw commands or move structured JSON/resources into helpers",
                );
            }
            _ => report.diagnostics.push(CheckPluginDiagnosticReport {
                kind: "experimental-plugin-diagnostic".to_string(),
                plugin,
                plugin_kind: "declarative-rule".to_string(),
                severity: "warning".to_string(),
                message: format!(
                    "Experimental declarative rule {rule} is not implemented in this Cobble build; skipped."
                ),
            }),
        }
    }
}

fn apply_line_match_rule(
    report: &mut CheckPluginReport,
    plugin: &str,
    rule: &str,
    sources: &[(&Path, &str)],
    config_dir: &Path,
    predicate: impl Fn(&str) -> bool,
    guidance: &str,
) {
    for (path, source) in sources {
        let relative_path = path.strip_prefix(config_dir).unwrap_or(path);
        let file = path_display(relative_path);
        for (line_index, line) in source.lines().enumerate() {
            if !predicate(line) {
                continue;
            }
            report.diagnostics.push(CheckPluginDiagnosticReport {
                kind: "experimental-plugin-diagnostic".to_string(),
                plugin: plugin.to_string(),
                plugin_kind: "declarative-rule".to_string(),
                severity: "warning".to_string(),
                message: format!("Rule {rule} matched {file}:{}; {guidance}.", line_index + 1,),
            });
        }
    }
}

fn line_uses_tellraw(line: &str) -> bool {
    line_uses_command_word(line, "tellraw")
}

fn line_uses_command_word(line: &str, command: &str) -> bool {
    let line = line.trim_start();
    line.starts_with(&format!("/{command} "))
        || line == format!("/{command}")
        || line.contains(&format!(" run {command} "))
        || line.ends_with(&format!(" run {command}"))
}

fn line_uses_gamemode_creative(line: &str) -> bool {
    let line = line.trim_start();
    line.starts_with("/gamemode creative ")
        || line == "/gamemode creative"
        || line.contains(" run gamemode creative ")
        || line.ends_with(" run gamemode creative")
}

fn line_has_long_raw_command(line: &str) -> bool {
    let line = line.trim_start();
    line.starts_with('/') && line.chars().count() > 120
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
            escape_diagnostic_field(&manifest.kind)
        ));
    }
    if let Some(description) = &manifest.description {
        if description.len() > 280 {
            return Err("plugin description must be at most 280 bytes".to_string());
        }
        if description.chars().any(char::is_control) {
            return Err("plugin description cannot contain control characters".to_string());
        }
    }
    if let Some(version) = &manifest.minimum_cobble_version {
        if !is_safe_plugin_version_requirement(version) {
            return Err(
                "minimum_cobble_version must be an ASCII version requirement up to 64 bytes"
                    .to_string(),
            );
        }
    }
    if manifest.diagnostic_rules.len() > 64 {
        return Err("diagnostic_rules can list at most 64 rule identifiers".to_string());
    }
    for rule in &manifest.diagnostic_rules {
        if !is_safe_plugin_rule_id(rule) {
            return Err(
                "diagnostic rule identifiers must contain only lowercase letters, digits, '.', '_', or '-'"
                    .to_string(),
            );
        }
    }
    Ok(())
}

fn rejected_manifest_plugin_id(manifest_path: &Path, manifest_name: &str) -> String {
    if is_safe_plugin_name(manifest_name) {
        manifest_name.to_string()
    } else {
        path_display(manifest_path)
    }
}

fn escape_diagnostic_field(value: &str) -> String {
    value.chars().flat_map(char::escape_default).collect()
}

fn escape_human_diagnostic_field(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        if character.is_control() {
            escaped.extend(character.escape_default());
        } else {
            escaped.push(character);
        }
    }
    escaped
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

fn is_safe_plugin_rule_id(rule: &str) -> bool {
    !rule.is_empty()
        && rule.len() <= 64
        && rule.chars().all(|ch| {
            ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '.' | '_' | '-')
        })
}

fn is_safe_plugin_version_requirement(version: &str) -> bool {
    !version.is_empty()
        && version.len() <= 64
        && version.chars().all(|ch| {
            ch.is_ascii_alphanumeric()
                || matches!(
                    ch,
                    '.' | '-' | '_' | '+' | '^' | '~' | '<' | '>' | '=' | ' ' | ','
                )
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
            escape_human_diagnostic_field(&diagnostic.severity),
            escape_human_diagnostic_field(&diagnostic.plugin),
            escape_human_diagnostic_field(&diagnostic.plugin_kind),
            escape_human_diagnostic_field(&diagnostic.message)
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
    if !report.observed_constructs.is_empty() {
        println!(
            "warning: experimental Python compatibility observed supported constructs: {}",
            report.observed_constructs.join(", ")
        );
    }
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

fn diagnostic_reports<'a>(
    file_diagnostics: impl IntoIterator<Item = &'a FileSourceDiagnostics>,
    config_dir: &Path,
) -> Vec<CheckDiagnosticReport> {
    file_diagnostics
        .into_iter()
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
