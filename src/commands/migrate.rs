use crate::config::CobbleConfig;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub struct MigrateOptions {
    pub path: Option<PathBuf>,
    pub from: String,
    pub to: String,
    pub json: bool,
    pub apply: bool,
}

#[derive(Serialize)]
struct MigrationReport {
    schema_version: u32,
    ok: bool,
    changed: bool,
    from: String,
    to: String,
    apply: bool,
    project_path: String,
    config: MigrationConfigReport,
    source: MigrationSourceReport,
    diagnostics: Vec<MigrationDiagnostic>,
    actions: Vec<MigrationAction>,
}

#[derive(Serialize)]
struct MigrationDiagnostic {
    severity: &'static str,
    code: &'static str,
    message: String,
}

#[derive(Serialize)]
struct MigrationAction {
    id: &'static str,
    status: &'static str,
    message: String,
}

#[derive(Serialize)]
struct MigrationConfigReport {
    status: &'static str,
    path: Option<String>,
    source: Option<String>,
    stdlib_version: Option<u8>,
    experimental_resource_pack: Option<bool>,
    experimental_python_compat: Option<bool>,
    message: String,
}

#[derive(Serialize)]
struct MigrationSourceReport {
    status: &'static str,
    path: Option<String>,
    files_scanned: usize,
    files: Vec<String>,
    resource_pack_references: usize,
    legacy_stdlib_import_files: usize,
    stdlib_module_import_files: usize,
    language_support_notes: Vec<String>,
    message: String,
}

struct ConfigSettings {
    source: String,
    stdlib_version: u8,
    experimental_resource_pack: bool,
    experimental_python_compat: bool,
}

struct SourceScan {
    files: Vec<PathBuf>,
    resource_pack_references: usize,
    legacy_stdlib_import_files: usize,
    stdlib_module_import_files: usize,
    read_errors: Vec<String>,
}

struct SourceSignals {
    resource_pack_references: usize,
    legacy_stdlib_import: bool,
    stdlib_module_import: bool,
}

pub fn migrate(options: MigrateOptions) -> Result<(), String> {
    let json = options.json;
    let report = build_migration_report(options);

    if json {
        print_json_report(&report)?;
    } else {
        print_human_report(&report);
    }

    if report.ok {
        Ok(())
    } else if report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "unsupported_migration_route")
    {
        Err(format!(
            "Unsupported experimental migration route: {} -> {}",
            report.from, report.to
        ))
    } else {
        Err(format!(
            "Migration inspection failed for experimental route: {} -> {}",
            report.from, report.to
        ))
    }
}

fn build_migration_report(options: MigrateOptions) -> MigrationReport {
    let project_path = options.path.unwrap_or_else(|| PathBuf::from("."));
    let supported_route = is_supported_route(&options.from, &options.to);
    let mut diagnostics = Vec::new();
    let mut actions = Vec::new();
    let mut config = skipped_config_report();
    let mut source = skipped_source_report();

    if supported_route {
        let inspection = inspect_project(&project_path, &mut diagnostics, &mut actions);
        config = inspection.0;
        source = inspection.1;

        if options.apply {
            diagnostics.push(MigrationDiagnostic {
                severity: "warning",
                code: "experimental_migration_no_rewrites",
                message:
                    "Apply was supplied, but this experimental skeleton has no automatic rewrites yet; no files were changed."
                        .to_string(),
            });
            actions.push(MigrationAction {
                id: "apply_rewrites",
                status: "skipped",
                message:
                    "No automatic 0.8 -> 0.9 rewrites are implemented in this experimental skeleton."
                        .to_string(),
            });
        } else {
            diagnostics.push(MigrationDiagnostic {
                severity: "info",
                code: "experimental_migration_dry_run",
                message:
                    "Dry-run/report mode is the default; no files were changed. File modifications require --apply."
                        .to_string(),
            });
            actions.push(MigrationAction {
                id: "apply_rewrites",
                status: "skipped",
                message:
                    "Run with --apply only after reviewing the report and once rewrite support exists."
                        .to_string(),
            });
        }
    } else {
        diagnostics.push(MigrationDiagnostic {
            severity: "error",
            code: "unsupported_migration_route",
            message: "This experimental skeleton only reports planned support for 0.8 -> 0.9."
                .to_string(),
        });
        actions.push(MigrationAction {
            id: "migration_route",
            status: "unsupported",
            message: "No migration actions are available for this route.".to_string(),
        });
    }

    let ok = supported_route
        && !diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == "error");

    MigrationReport {
        schema_version: 1,
        ok,
        changed: false,
        from: options.from,
        to: options.to,
        apply: options.apply,
        project_path: path_display(&project_path),
        config,
        source,
        diagnostics,
        actions,
    }
}

fn inspect_project(
    project_path: &Path,
    diagnostics: &mut Vec<MigrationDiagnostic>,
    actions: &mut Vec<MigrationAction>,
) -> (MigrationConfigReport, MigrationSourceReport) {
    let config_path = find_config(project_path);
    let config_dir = config_path
        .as_ref()
        .and_then(|path| path.parent())
        .map(Path::to_path_buf);
    let project_root = config_dir
        .clone()
        .unwrap_or_else(|| project_root_for_path(project_path));

    let (config, settings) = match config_path {
        Some(path) => inspect_config_file(&path, diagnostics),
        None => {
            let settings = default_config_settings();
            let message = format!(
                "No cobble.toml found from {}; using src and default planning settings.",
                path_display(project_path)
            );
            diagnostics.push(MigrationDiagnostic {
                severity: "info",
                code: "config_missing",
                message: message.clone(),
            });
            (
                MigrationConfigReport {
                    status: "missing",
                    path: None,
                    source: Some(settings.source.clone()),
                    stdlib_version: Some(settings.stdlib_version),
                    experimental_resource_pack: Some(settings.experimental_resource_pack),
                    experimental_python_compat: Some(settings.experimental_python_compat),
                    message,
                },
                settings,
            )
        }
    };

    actions.push(MigrationAction {
        id: "inspect_config",
        status: config.status,
        message: config.message.clone(),
    });

    diagnostics.push(MigrationDiagnostic {
        severity: "info",
        code: "stdlib_version",
        message: format!("Planning with stdlib version {}.", settings.stdlib_version),
    });

    if settings.experimental_resource_pack {
        diagnostics.push(MigrationDiagnostic {
            severity: "info",
            code: "experimental_resource_pack_configured",
            message: "[experimental] resource_pack is enabled in cobble.toml.".to_string(),
        });
    }
    if settings.experimental_python_compat {
        diagnostics.push(MigrationDiagnostic {
            severity: "info",
            code: "experimental_python_compat_configured",
            message: "[experimental] python_compat is enabled in cobble.toml.".to_string(),
        });
    }

    let source_path = source_path_for(project_path, &project_root, &settings.source);
    let source = inspect_sources(
        &source_path,
        settings.experimental_resource_pack,
        diagnostics,
    );

    actions.push(MigrationAction {
        id: "scan_sources",
        status: source.status,
        message: source.message.clone(),
    });

    actions.push(MigrationAction {
        id: "report_stdlib",
        status: "noted",
        message: format!(
            "Report stdlib version {} and any import-style notes.",
            settings.stdlib_version
        ),
    });

    let resource_pack_status = if source.resource_pack_references > 0 {
        if settings.experimental_resource_pack {
            "configured"
        } else {
            "candidate"
        }
    } else {
        "not_detected"
    };
    actions.push(MigrationAction {
        id: "candidate_resource_pack_config",
        status: resource_pack_status,
        message: resource_pack_action_message(&source, settings.experimental_resource_pack),
    });

    actions.push(MigrationAction {
        id: "report_language_support",
        status: if source.language_support_notes.is_empty() {
            "not_detected"
        } else {
            "noted"
        },
        message: if source.language_support_notes.is_empty() {
            "No stdlib or resource-pack language-support notes were detected.".to_string()
        } else {
            format!(
                "Report {} language-support note(s) from scanned sources.",
                source.language_support_notes.len()
            )
        },
    });

    actions.push(MigrationAction {
        id: "report_manual_steps",
        status: "planned",
        message: "Report source locations, skipped changes, and manual edits.".to_string(),
    });

    (config, source)
}

fn inspect_config_file(
    config_path: &Path,
    diagnostics: &mut Vec<MigrationDiagnostic>,
) -> (MigrationConfigReport, ConfigSettings) {
    match read_config_settings(config_path) {
        Ok(settings) => {
            let message = format!(
                "Found cobble.toml at {}; using build.source = {}.",
                path_display(config_path),
                settings.source
            );
            diagnostics.push(MigrationDiagnostic {
                severity: "info",
                code: "config_found",
                message: message.clone(),
            });
            (
                MigrationConfigReport {
                    status: "found",
                    path: Some(path_display(config_path)),
                    source: Some(settings.source.clone()),
                    stdlib_version: Some(settings.stdlib_version),
                    experimental_resource_pack: Some(settings.experimental_resource_pack),
                    experimental_python_compat: Some(settings.experimental_python_compat),
                    message,
                },
                settings,
            )
        }
        Err(error) => {
            let settings = default_config_settings();
            let message = format!(
                "Could not inspect cobble.toml at {}; using src and default planning settings: {}",
                path_display(config_path),
                error
            );
            diagnostics.push(MigrationDiagnostic {
                severity: "error",
                code: "config_parse_failed",
                message: message.clone(),
            });
            (
                MigrationConfigReport {
                    status: "error",
                    path: Some(path_display(config_path)),
                    source: Some(settings.source.clone()),
                    stdlib_version: Some(settings.stdlib_version),
                    experimental_resource_pack: Some(settings.experimental_resource_pack),
                    experimental_python_compat: Some(settings.experimental_python_compat),
                    message,
                },
                settings,
            )
        }
    }
}

fn read_config_settings(config_path: &Path) -> Result<ConfigSettings, String> {
    let config = CobbleConfig::load(config_path)?;

    Ok(ConfigSettings {
        source: config.build.source,
        stdlib_version: config.stdlib.version,
        experimental_resource_pack: config.experimental.resource_pack,
        experimental_python_compat: config.experimental.python_compat,
    })
}

fn inspect_sources(
    source_path: &Path,
    experimental_resource_pack: bool,
    diagnostics: &mut Vec<MigrationDiagnostic>,
) -> MigrationSourceReport {
    if !source_path.exists() {
        let message = format!("Source path does not exist: {}", path_display(source_path));
        diagnostics.push(MigrationDiagnostic {
            severity: "warning",
            code: "source_missing",
            message: message.clone(),
        });
        return MigrationSourceReport {
            status: "missing",
            path: Some(path_display(source_path)),
            files_scanned: 0,
            files: Vec::new(),
            resource_pack_references: 0,
            legacy_stdlib_import_files: 0,
            stdlib_module_import_files: 0,
            language_support_notes: Vec::new(),
            message,
        };
    }

    match scan_source_files(source_path) {
        Ok(scan) => {
            source_report_from_scan(source_path, scan, experimental_resource_pack, diagnostics)
        }
        Err(error) => {
            let message = format!(
                "Failed to scan source path {}: {}",
                path_display(source_path),
                error
            );
            diagnostics.push(MigrationDiagnostic {
                severity: "error",
                code: "source_scan_failed",
                message: message.clone(),
            });
            MigrationSourceReport {
                status: "error",
                path: Some(path_display(source_path)),
                files_scanned: 0,
                files: Vec::new(),
                resource_pack_references: 0,
                legacy_stdlib_import_files: 0,
                stdlib_module_import_files: 0,
                language_support_notes: Vec::new(),
                message,
            }
        }
    }
}

fn source_report_from_scan(
    source_path: &Path,
    scan: SourceScan,
    experimental_resource_pack: bool,
    diagnostics: &mut Vec<MigrationDiagnostic>,
) -> MigrationSourceReport {
    let files: Vec<String> = scan
        .files
        .iter()
        .map(|path| path_display_relative(path, source_path))
        .collect();
    let mut notes = Vec::new();

    if scan.legacy_stdlib_import_files > 0 {
        notes.push(format!(
            "{} file(s) use legacy `import stdlib`; review per-module stdlib imports for 0.9.",
            scan.legacy_stdlib_import_files
        ));
        diagnostics.push(MigrationDiagnostic {
            severity: "warning",
            code: "legacy_stdlib_imports",
            message: notes.last().cloned().unwrap_or_default(),
        });
    }

    if scan.stdlib_module_import_files > 0 {
        notes.push(format!(
            "{} file(s) already use `from stdlib import ...`.",
            scan.stdlib_module_import_files
        ));
        diagnostics.push(MigrationDiagnostic {
            severity: "info",
            code: "stdlib_module_imports",
            message: notes.last().cloned().unwrap_or_default(),
        });
    }

    if scan.resource_pack_references > 0 && !experimental_resource_pack {
        notes.push(format!(
            "{} resource_pack.* reference(s) may need explicit 0.9 resource-pack review.",
            scan.resource_pack_references
        ));
        diagnostics.push(MigrationDiagnostic {
            severity: "warning",
            code: "resource_pack_experimental_candidate",
            message: notes.last().cloned().unwrap_or_default(),
        });
    }

    for error in &scan.read_errors {
        diagnostics.push(MigrationDiagnostic {
            severity: "error",
            code: "source_read_failed",
            message: error.clone(),
        });
    }

    let status = if scan.read_errors.is_empty() {
        "scanned"
    } else {
        "error"
    };
    let message = if scan.files.len() == 1 {
        format!(
            "Scanned 1 Cobble source file under {}.",
            path_display(source_path)
        )
    } else {
        format!(
            "Scanned {} Cobble source files under {}.",
            scan.files.len(),
            path_display(source_path)
        )
    };
    diagnostics.push(MigrationDiagnostic {
        severity: "info",
        code: "source_scan_completed",
        message: message.clone(),
    });

    MigrationSourceReport {
        status,
        path: Some(path_display(source_path)),
        files_scanned: scan.files.len(),
        files,
        resource_pack_references: scan.resource_pack_references,
        legacy_stdlib_import_files: scan.legacy_stdlib_import_files,
        stdlib_module_import_files: scan.stdlib_module_import_files,
        language_support_notes: notes,
        message,
    }
}

fn scan_source_files(source_path: &Path) -> Result<SourceScan, String> {
    let mut files = collect_cobble_files(source_path)?;
    files.sort_by_key(|path| path_display(path));

    let mut scan = SourceScan {
        files,
        resource_pack_references: 0,
        legacy_stdlib_import_files: 0,
        stdlib_module_import_files: 0,
        read_errors: Vec::new(),
    };

    for file in &scan.files {
        match fs::read_to_string(file) {
            Ok(contents) => {
                let signals = analyze_source(&contents);
                scan.resource_pack_references += signals.resource_pack_references;
                if signals.legacy_stdlib_import {
                    scan.legacy_stdlib_import_files += 1;
                }
                if signals.stdlib_module_import {
                    scan.stdlib_module_import_files += 1;
                }
            }
            Err(error) => scan.read_errors.push(format!(
                "Failed to read source file {}: {}",
                path_display(file),
                error
            )),
        }
    }

    Ok(scan)
}

fn collect_cobble_files(source_path: &Path) -> Result<Vec<PathBuf>, String> {
    if source_path.is_file() {
        return Ok(is_cobble_file(source_path)
            .then(|| source_path.to_path_buf())
            .into_iter()
            .collect());
    }

    let mut files = Vec::new();
    for entry in WalkDir::new(source_path).follow_links(false) {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        if entry.file_type().is_symlink() {
            continue;
        }
        if entry.file_type().is_file() && is_cobble_file(path) {
            files.push(path.to_path_buf());
        }
    }

    Ok(files)
}

fn analyze_source(source: &str) -> SourceSignals {
    let mut legacy_stdlib_import = false;
    let mut stdlib_module_import = false;

    for line in source.lines() {
        let line = line.trim();
        if line == "import stdlib" || line.starts_with("import stdlib ") {
            legacy_stdlib_import = true;
        }
        if line == "from stdlib import" || line.starts_with("from stdlib import ") {
            stdlib_module_import = true;
        }
    }

    SourceSignals {
        resource_pack_references: source.matches("resource_pack.").count(),
        legacy_stdlib_import,
        stdlib_module_import,
    }
}

fn resource_pack_action_message(
    source: &MigrationSourceReport,
    experimental_resource_pack: bool,
) -> String {
    match (source.resource_pack_references, experimental_resource_pack) {
        (0, _) => "No resource_pack.* source references were detected.".to_string(),
        (count, true) => format!(
            "Detected {count} resource_pack.* reference(s); [experimental] resource_pack is configured."
        ),
        (count, false) => format!(
            "Detected {count} resource_pack.* reference(s); report candidate experimental config review."
        ),
    }
}

fn source_path_for(project_path: &Path, project_root: &Path, configured_source: &str) -> PathBuf {
    if project_path.is_file() {
        return project_path.to_path_buf();
    }

    let source = Path::new(configured_source);
    if source.is_absolute() {
        source.to_path_buf()
    } else {
        project_root.join(source)
    }
}

fn default_config_settings() -> ConfigSettings {
    ConfigSettings {
        source: "src".to_string(),
        stdlib_version: 2,
        experimental_resource_pack: false,
        experimental_python_compat: false,
    }
}

fn skipped_config_report() -> MigrationConfigReport {
    MigrationConfigReport {
        status: "skipped",
        path: None,
        source: None,
        stdlib_version: None,
        experimental_resource_pack: None,
        experimental_python_compat: None,
        message: "Config inspection was skipped for this migration route.".to_string(),
    }
}

fn skipped_source_report() -> MigrationSourceReport {
    MigrationSourceReport {
        status: "skipped",
        path: None,
        files_scanned: 0,
        files: Vec::new(),
        resource_pack_references: 0,
        legacy_stdlib_import_files: 0,
        stdlib_module_import_files: 0,
        language_support_notes: Vec::new(),
        message: "Source scanning was skipped for this migration route.".to_string(),
    }
}

fn project_root_for_path(path: &Path) -> PathBuf {
    if path.is_file() {
        path.parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
    } else {
        path.to_path_buf()
    }
}

fn find_config(path: &Path) -> Option<PathBuf> {
    if path.is_file() {
        return path.parent().and_then(CobbleConfig::find_in_path);
    }

    CobbleConfig::find_in_path(path)
}

fn print_json_report(report: &MigrationReport) -> Result<(), String> {
    let output = serde_json::to_string_pretty(report)
        .map_err(|error| format!("Failed to format migration JSON: {error}"))?;
    println!("{output}");
    Ok(())
}

fn print_human_report(report: &MigrationReport) {
    println!("Cobble migrate (experimental)");
    println!("  From: {}", report.from);
    println!("  To: {}", report.to);
    println!(
        "  Mode: {}",
        if report.apply {
            "apply requested"
        } else {
            "dry-run/report"
        }
    );
    println!("  Changed: {}", report.changed);
    println!("  Project path: {}", report.project_path);

    println!("Config:");
    println!("  {}: {}", report.config.status, report.config.message);
    if let Some(path) = &report.config.path {
        println!("  Path: {path}");
    }
    if let Some(source) = &report.config.source {
        println!("  Source setting: {source}");
    }
    if let Some(version) = report.config.stdlib_version {
        println!("  Stdlib version: {version}");
    }
    if let Some(enabled) = report.config.experimental_resource_pack {
        println!("  Experimental resource pack: {enabled}");
    }
    if let Some(enabled) = report.config.experimental_python_compat {
        println!("  Experimental Python compatibility: {enabled}");
    }

    println!("Sources:");
    println!("  {}: {}", report.source.status, report.source.message);
    if let Some(path) = &report.source.path {
        println!("  Path: {path}");
    }
    println!("  Source files scanned: {}", report.source.files_scanned);
    println!(
        "  Resource-pack references: {}",
        report.source.resource_pack_references
    );
    if !report.source.language_support_notes.is_empty() {
        println!("Language support notes:");
        for note in &report.source.language_support_notes {
            println!("  - {note}");
        }
    }

    println!("Diagnostics:");
    for diagnostic in &report.diagnostics {
        println!(
            "  {} [{}]: {}",
            diagnostic.severity, diagnostic.code, diagnostic.message
        );
    }

    println!("Actions:");
    for action in &report.actions {
        println!("  {} [{}]: {}", action.id, action.status, action.message);
    }

    println!("No files were changed.");
}

fn is_supported_route(from: &str, to: &str) -> bool {
    matches!(from, "0.8" | "0.8.0") && matches!(to, "0.9" | "0.9.0")
}

fn is_cobble_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("cbl" | "cobble")
    )
}

fn path_display(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn path_display_relative(path: &Path, root: &Path) -> String {
    let root = if root.is_file() {
        root.parent().unwrap_or(root)
    } else {
        root
    };

    path.strip_prefix(root)
        .map(path_display)
        .unwrap_or_else(|_| path_display(path))
}
