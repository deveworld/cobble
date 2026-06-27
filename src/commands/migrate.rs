use super::output_safety::ensure_no_symlink_components;
use crate::config::CobbleConfig;
use crate::diagnostics::{parse_source, python_compat_suggestion_for_kind};
use crate::fs_safety::write_file_atomic_with_permissions;
use crate::pack_format::SUPPORTED_PACK_FORMAT;
use serde::Serialize;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use toml_edit::{value, DocumentMut};
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
    #[serde(skip_serializing_if = "Option::is_none")]
    before: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    after: Option<String>,
}

#[derive(Serialize)]
struct MigrationConfigReport {
    status: &'static str,
    path: Option<String>,
    backup_path: Option<String>,
    source: Option<String>,
    pack_format: Option<String>,
    stdlib_version: Option<u8>,
    experimental_resource_pack: Option<bool>,
    experimental_python_compat: Option<bool>,
    changes: Vec<MigrationConfigChange>,
    message: String,
}

#[derive(Serialize)]
struct MigrationConfigChange {
    field: &'static str,
    before: String,
    after: String,
    status: &'static str,
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
    unsupported_python_constructs: usize,
    file_details: Vec<MigrationSourceFileReport>,
    language_support_notes: Vec<String>,
    message: String,
}

#[derive(Serialize)]
struct MigrationSourceFileReport {
    file: String,
    resource_pack_references: usize,
    legacy_stdlib_import: bool,
    stdlib_module_import: bool,
    unsupported_python_constructs: usize,
    locations: Vec<MigrationSourceLocation>,
}

#[derive(Serialize, Clone)]
struct MigrationSourceLocation {
    file: String,
    line: usize,
    column: usize,
    kind: String,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    suggested_cobble_alternative: Option<String>,
}

#[derive(Clone)]
struct ConfigSettings {
    source: String,
    pack_format: String,
    stdlib_version: u8,
    experimental_resource_pack: bool,
    experimental_python_compat: bool,
}

struct ProjectInspection {
    config: MigrationConfigReport,
    source: MigrationSourceReport,
    config_path: Option<PathBuf>,
    settings: ConfigSettings,
}

struct SourceScan {
    files: Vec<PathBuf>,
    file_reports: Vec<MigrationSourceFileReport>,
    resource_pack_references: usize,
    legacy_stdlib_import_files: usize,
    stdlib_module_import_files: usize,
    unsupported_python_constructs: usize,
    read_errors: Vec<String>,
}

struct SourceSignals {
    resource_pack_references: usize,
    legacy_stdlib_import: bool,
    stdlib_module_import: bool,
    unsupported_python_constructs: usize,
    locations: Vec<MigrationSourceLocation>,
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
    let mut changed = false;

    if supported_route {
        let inspection = inspect_project(&project_path, &mut diagnostics, &mut actions);
        config = inspection.config;
        source = inspection.source;

        if options.apply {
            changed = apply_supported_migration(
                inspection.config_path.as_deref(),
                &inspection.settings,
                &mut config,
                &mut diagnostics,
                &mut actions,
            );
        } else {
            diagnostics.push(MigrationDiagnostic {
                severity: "info",
                code: "experimental_migration_dry_run",
                message:
                    "Dry-run/report mode is the default; no files were changed. File modifications require --apply."
                        .to_string(),
            });
            actions.push(MigrationAction {
                id: "apply_config",
                status: "skipped",
                message:
                    "Run with --apply after reviewing the report to apply supported config-only migrations."
                        .to_string(),
                before: None,
                after: None,
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
            before: None,
            after: None,
        });
    }

    let ok = supported_route
        && !diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == "error");

    MigrationReport {
        schema_version: 1,
        ok,
        changed,
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
) -> ProjectInspection {
    let config_path = find_config(project_path);
    let config_dir = config_path
        .as_ref()
        .and_then(|path| path.parent())
        .map(Path::to_path_buf);
    let project_root = config_dir
        .clone()
        .unwrap_or_else(|| project_root_for_path(project_path));

    let (config, settings) = match config_path.as_ref() {
        Some(path) => inspect_config_file(path, diagnostics),
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
                    backup_path: None,
                    source: Some(settings.source.clone()),
                    pack_format: Some(settings.pack_format.clone()),
                    stdlib_version: Some(settings.stdlib_version),
                    experimental_resource_pack: Some(settings.experimental_resource_pack),
                    experimental_python_compat: Some(settings.experimental_python_compat),
                    changes: Vec::new(),
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
        before: None,
        after: None,
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
        before: None,
        after: None,
    });

    actions.push(MigrationAction {
        id: "report_stdlib",
        status: "noted",
        message: format!(
            "Report stdlib version {} and any import-style notes.",
            settings.stdlib_version
        ),
        before: None,
        after: None,
    });

    actions.push(MigrationAction {
        id: "update_pack_format",
        status: pack_format_action_status(&config, &settings),
        message: pack_format_action_message(&config, &settings),
        before: Some(settings.pack_format.clone()),
        after: Some(SUPPORTED_PACK_FORMAT.to_string()),
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
        before: None,
        after: None,
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
        before: None,
        after: None,
    });

    actions.push(MigrationAction {
        id: "report_manual_steps",
        status: if source
            .file_details
            .iter()
            .any(|file| !file.locations.is_empty())
        {
            "noted"
        } else {
            "not_detected"
        },
        message: if source
            .file_details
            .iter()
            .any(|file| !file.locations.is_empty())
        {
            "Reported source locations and manual review hints for migration-sensitive constructs."
                .to_string()
        } else {
            "No source-location manual review hints were detected.".to_string()
        },
        before: None,
        after: None,
    });

    ProjectInspection {
        config,
        source,
        config_path,
        settings,
    }
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
                    backup_path: None,
                    source: Some(settings.source.clone()),
                    pack_format: Some(settings.pack_format.clone()),
                    stdlib_version: Some(settings.stdlib_version),
                    experimental_resource_pack: Some(settings.experimental_resource_pack),
                    experimental_python_compat: Some(settings.experimental_python_compat),
                    changes: config_changes_for(&settings),
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
                    backup_path: None,
                    source: Some(settings.source.clone()),
                    pack_format: Some(settings.pack_format.clone()),
                    stdlib_version: Some(settings.stdlib_version),
                    experimental_resource_pack: Some(settings.experimental_resource_pack),
                    experimental_python_compat: Some(settings.experimental_python_compat),
                    changes: Vec::new(),
                    message,
                },
                settings,
            )
        }
    }
}

fn read_config_settings(config_path: &Path) -> Result<ConfigSettings, String> {
    let config = CobbleConfig::load_unvalidated(config_path)?;

    Ok(ConfigSettings {
        source: config.build.source,
        pack_format: config.project.pack_format,
        stdlib_version: config.stdlib.version,
        experimental_resource_pack: config.experimental.resource_pack,
        experimental_python_compat: config.experimental.python_compat,
    })
}

fn config_changes_for(settings: &ConfigSettings) -> Vec<MigrationConfigChange> {
    if pack_format_needs_update(settings) {
        vec![MigrationConfigChange {
            field: "project.pack_format",
            before: settings.pack_format.clone(),
            after: SUPPORTED_PACK_FORMAT.to_string(),
            status: "candidate",
        }]
    } else {
        vec![MigrationConfigChange {
            field: "project.pack_format",
            before: settings.pack_format.clone(),
            after: SUPPORTED_PACK_FORMAT.to_string(),
            status: "not_needed",
        }]
    }
}

fn mark_config_change_applied(config_report: &mut MigrationConfigReport, field: &str) {
    for change in &mut config_report.changes {
        if change.field == field {
            change.status = "applied";
        }
    }
}

fn apply_supported_migration(
    config_path: Option<&Path>,
    settings: &ConfigSettings,
    config_report: &mut MigrationConfigReport,
    diagnostics: &mut Vec<MigrationDiagnostic>,
    actions: &mut Vec<MigrationAction>,
) -> bool {
    let Some(config_path) = config_path else {
        diagnostics.push(MigrationDiagnostic {
            severity: "warning",
            code: "migration_apply_no_config",
            message: "Apply was supplied, but no cobble.toml was found; no files were changed."
                .to_string(),
        });
        actions.push(MigrationAction {
            id: "apply_config",
            status: "skipped",
            message: "No cobble.toml was available for config-only migration.".to_string(),
            before: None,
            after: None,
        });
        return false;
    };

    if config_report.status != "found" {
        diagnostics.push(MigrationDiagnostic {
            severity: "error",
            code: "migration_apply_config_unavailable",
            message:
                "Apply was supplied, but cobble.toml could not be parsed safely; no files were changed."
                    .to_string(),
        });
        actions.push(MigrationAction {
            id: "apply_config",
            status: "error",
            message: "Config-only migration requires a valid cobble.toml.".to_string(),
            before: None,
            after: None,
        });
        return false;
    }

    if !pack_format_needs_update(settings) {
        diagnostics.push(MigrationDiagnostic {
            severity: "info",
            code: "migration_apply_no_changes",
            message: "Apply was supplied, but no supported config-only migrations were needed."
                .to_string(),
        });
        actions.push(MigrationAction {
            id: "apply_config",
            status: "skipped",
            message: "No config changes were needed for the supported 0.8 -> 0.9 route."
                .to_string(),
            before: None,
            after: None,
        });
        return false;
    }

    match apply_config_migration(config_path) {
        Ok(backup_path) => {
            config_report.pack_format = Some(SUPPORTED_PACK_FORMAT.to_string());
            config_report.backup_path = Some(path_display(&backup_path));
            mark_config_change_applied(config_report, "project.pack_format");
            diagnostics.push(MigrationDiagnostic {
                severity: "info",
                code: "migration_apply_config_updated",
                message: format!(
                    "Updated cobble.toml project.pack_format from {} to {}.",
                    settings.pack_format, SUPPORTED_PACK_FORMAT
                ),
            });
            actions.push(MigrationAction {
                id: "apply_config",
                status: "applied",
                message: format!(
                    "Updated project.pack_format to {} in {}; backup written to {}.",
                    SUPPORTED_PACK_FORMAT,
                    path_display(config_path),
                    path_display(&backup_path)
                ),
                before: Some(settings.pack_format.clone()),
                after: Some(SUPPORTED_PACK_FORMAT.to_string()),
            });
            true
        }
        Err(error) => {
            diagnostics.push(MigrationDiagnostic {
                severity: "error",
                code: "migration_apply_config_failed",
                message: error,
            });
            actions.push(MigrationAction {
                id: "apply_config",
                status: "error",
                message: "Failed to apply config-only migration.".to_string(),
                before: Some(settings.pack_format.clone()),
                after: Some(SUPPORTED_PACK_FORMAT.to_string()),
            });
            false
        }
    }
}

fn apply_config_migration(config_path: &Path) -> Result<PathBuf, String> {
    ensure_no_symlink_components(config_path, "apply migration config")?;
    CobbleConfig::load_unvalidated(config_path)?;
    let original_permissions = fs::metadata(config_path)
        .map_err(|error| {
            format!(
                "Failed to inspect config permissions for migration {}: {error}",
                path_display(config_path)
            )
        })?
        .permissions();
    let contents = fs::read_to_string(config_path).map_err(|error| {
        format!(
            "Failed to read config for migration {}: {error}",
            path_display(config_path)
        )
    })?;
    let mut document: DocumentMut = contents.parse().map_err(|error| {
        format!(
            "Failed to parse config for targeted migration {}: {error}",
            path_display(config_path)
        )
    })?;
    let project = document
        .get_mut("project")
        .and_then(|item| item.as_table_mut())
        .ok_or_else(|| "Failed to find [project] table for pack_format migration".to_string())?;
    project["pack_format"] = value(SUPPORTED_PACK_FORMAT.to_string());

    let backup_path = migration_backup_path(config_path)?;
    copy_config_backup(config_path, &backup_path)?;
    write_file_atomic_with_permissions(config_path, document.to_string(), original_permissions)
        .map_err(|error| {
            format!(
                "Failed to write migrated config {}: {error}",
                path_display(config_path)
            )
        })?;
    Ok(backup_path)
}

fn copy_config_backup(source: &Path, backup_path: &Path) -> Result<(), String> {
    ensure_no_symlink_components(backup_path, "write migration backup")?;
    let source_permissions = fs::metadata(source)
        .map_err(|error| {
            format!(
                "Failed to inspect config permissions for backup {}: {error}",
                path_display(source)
            )
        })?
        .permissions();
    let mut source_file = File::open(source).map_err(|error| {
        format!(
            "Failed to open config for backup {}: {error}",
            path_display(source)
        )
    })?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(source_permissions.mode() & 0o777);
    let mut backup_file = options.open(backup_path).map_err(|error| {
        format!(
            "Failed to create migration backup {}: {error}",
            path_display(backup_path)
        )
    })?;

    let result = (|| {
        io::copy(&mut source_file, &mut backup_file)?;
        backup_file.flush()
    })();
    if let Err(error) = result {
        let _ = fs::remove_file(backup_path);
        return Err(format!(
            "Failed to write migration backup {}: {error}",
            path_display(backup_path)
        ));
    }
    fs::set_permissions(backup_path, source_permissions).map_err(|error| {
        let _ = fs::remove_file(backup_path);
        format!(
            "Failed to set migration backup permissions {}: {error}",
            path_display(backup_path)
        )
    })?;

    Ok(())
}

fn migration_backup_path(config_path: &Path) -> Result<PathBuf, String> {
    let parent = config_path.parent().unwrap_or_else(|| Path::new("."));
    let name = config_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("cobble.toml");
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| format!("System clock error while creating migration backup: {error}"))?
        .as_nanos();

    for attempt in 0..32 {
        let candidate = parent.join(format!(
            ".{name}.cobble-migrate-backup-{}-{stamp}-{attempt}.bak",
            std::process::id()
        ));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }

    Err(format!(
        "Could not allocate migration backup path next to {}",
        path_display(config_path)
    ))
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
            unsupported_python_constructs: 0,
            file_details: Vec::new(),
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
                unsupported_python_constructs: 0,
                file_details: Vec::new(),
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

    if scan.unsupported_python_constructs > 0 {
        notes.push(format!(
            "{} Python-like unsupported construct(s) should be reviewed with `cobble check --experimental-python-compat`.",
            scan.unsupported_python_constructs
        ));
        diagnostics.push(MigrationDiagnostic {
            severity: "warning",
            code: "unsupported_python_constructs",
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
        unsupported_python_constructs: scan.unsupported_python_constructs,
        file_details: scan.file_reports,
        language_support_notes: notes,
        message,
    }
}

fn scan_source_files(source_path: &Path) -> Result<SourceScan, String> {
    let mut files = collect_cobble_files(source_path)?;
    files.sort_by_key(|path| path_display(path));

    let mut scan = SourceScan {
        files,
        file_reports: Vec::new(),
        resource_pack_references: 0,
        legacy_stdlib_import_files: 0,
        stdlib_module_import_files: 0,
        unsupported_python_constructs: 0,
        read_errors: Vec::new(),
    };

    for file in scan.files.clone() {
        match fs::read_to_string(&file) {
            Ok(contents) => {
                let relative_file = path_display_relative(&file, source_path);
                let signals = analyze_source(&contents, &relative_file);
                scan.resource_pack_references += signals.resource_pack_references;
                if signals.legacy_stdlib_import {
                    scan.legacy_stdlib_import_files += 1;
                }
                if signals.stdlib_module_import {
                    scan.stdlib_module_import_files += 1;
                }
                scan.unsupported_python_constructs += signals.unsupported_python_constructs;
                scan.file_reports.push(MigrationSourceFileReport {
                    file: relative_file,
                    resource_pack_references: signals.resource_pack_references,
                    legacy_stdlib_import: signals.legacy_stdlib_import,
                    stdlib_module_import: signals.stdlib_module_import,
                    unsupported_python_constructs: signals.unsupported_python_constructs,
                    locations: signals.locations,
                });
            }
            Err(error) => scan.read_errors.push(format!(
                "Failed to read source file {}: {}",
                path_display(&file),
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

fn analyze_source(source: &str, file: &str) -> SourceSignals {
    let mut legacy_stdlib_import = false;
    let mut stdlib_module_import = false;
    let mut resource_pack_references = 0;
    let mut locations = Vec::new();

    for (line_index, raw_line) in source.lines().enumerate() {
        let line_number = line_index + 1;
        let line = raw_line.trim();
        if line == "import stdlib" || line.starts_with("import stdlib ") {
            legacy_stdlib_import = true;
            let column = raw_line
                .find("import stdlib")
                .map(|index| index + 1)
                .unwrap_or(1);
            locations.push(MigrationSourceLocation {
                file: file.to_string(),
                line: line_number,
                column,
                kind: "legacy-stdlib-import".to_string(),
                message:
                    "Legacy `import stdlib` should be reviewed for 0.9 per-module stdlib imports."
                        .to_string(),
                suggested_cobble_alternative: Some(
                    "Use explicit imports such as `from stdlib import text, entity, schedule`."
                        .to_string(),
                ),
            });
        }
        if line == "from stdlib import" || line.starts_with("from stdlib import ") {
            stdlib_module_import = true;
        }
        for column in match_columns(raw_line, "resource_pack.") {
            resource_pack_references += 1;
            locations.push(MigrationSourceLocation {
                file: file.to_string(),
                line: line_number,
                column,
                kind: "resource-pack-reference".to_string(),
                message: "resource_pack.* usage requires the 0.9 resource-pack opt-in."
                    .to_string(),
                suggested_cobble_alternative: Some(
                    "Keep `[experimental] resource_pack = true` or pass `--experimental-resource-pack`."
                        .to_string(),
                ),
            });
        }
    }

    if let Err(diagnostics) = parse_source(source) {
        for diagnostic in diagnostics {
            if !migration_python_diagnostic_kind(&diagnostic.kind) {
                continue;
            }
            locations.push(MigrationSourceLocation {
                file: file.to_string(),
                line: diagnostic.line,
                column: diagnostic.column,
                kind: diagnostic.kind.clone(),
                message: diagnostic.message.clone(),
                suggested_cobble_alternative: python_compat_suggestion_for_kind(
                    &diagnostic.kind,
                    &diagnostic.message,
                    diagnostic.help.as_deref(),
                ),
            });
        }
    }
    let unsupported_python_constructs = locations
        .iter()
        .filter(|location| migration_python_diagnostic_kind(&location.kind))
        .count();

    SourceSignals {
        resource_pack_references,
        legacy_stdlib_import,
        stdlib_module_import,
        unsupported_python_constructs,
        locations,
    }
}

fn match_columns(line: &str, needle: &str) -> Vec<usize> {
    line.match_indices(needle)
        .map(|(index, _)| index + 1)
        .collect()
}

fn migration_python_diagnostic_kind(kind: &str) -> bool {
    kind.starts_with("unsupported-")
        || matches!(
            kind,
            "duplicate-function-parameter"
                | "no-op-expression"
                | "missing-import"
                | "missing-import-item"
        )
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

fn pack_format_action_status(
    config: &MigrationConfigReport,
    settings: &ConfigSettings,
) -> &'static str {
    match config.status {
        "found" if pack_format_needs_update(settings) => "candidate",
        "found" => "not_needed",
        "missing" => "unavailable",
        "error" => "error",
        _ => "skipped",
    }
}

fn pack_format_action_message(config: &MigrationConfigReport, settings: &ConfigSettings) -> String {
    match config.status {
        "found" if pack_format_needs_update(settings) => format!(
            "project.pack_format is {}; --apply can update it to {}.",
            settings.pack_format, SUPPORTED_PACK_FORMAT
        ),
        "found" => format!("project.pack_format is already {}.", SUPPORTED_PACK_FORMAT),
        "missing" => "No cobble.toml was found, so no pack_format update is available.".to_string(),
        "error" => {
            "cobble.toml could not be parsed, so no pack_format update is available.".to_string()
        }
        _ => "Config inspection was skipped for this migration route.".to_string(),
    }
}

fn pack_format_needs_update(settings: &ConfigSettings) -> bool {
    settings.pack_format != SUPPORTED_PACK_FORMAT.to_string()
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
        pack_format: SUPPORTED_PACK_FORMAT.to_string(),
        stdlib_version: 2,
        experimental_resource_pack: false,
        experimental_python_compat: false,
    }
}

fn skipped_config_report() -> MigrationConfigReport {
    MigrationConfigReport {
        status: "skipped",
        path: None,
        backup_path: None,
        source: None,
        pack_format: None,
        stdlib_version: None,
        experimental_resource_pack: None,
        experimental_python_compat: None,
        changes: Vec::new(),
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
        unsupported_python_constructs: 0,
        file_details: Vec::new(),
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
    if let Some(path) = &report.config.backup_path {
        println!("  Backup: {path}");
    }
    if let Some(source) = &report.config.source {
        println!("  Source setting: {source}");
    }
    if let Some(pack_format) = &report.config.pack_format {
        println!("  Pack format: {pack_format}");
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
    println!(
        "  Unsupported Python-like constructs: {}",
        report.source.unsupported_python_constructs
    );
    if !report.source.language_support_notes.is_empty() {
        println!("Language support notes:");
        for note in &report.source.language_support_notes {
            println!("  - {note}");
        }
    }
    if report
        .source
        .file_details
        .iter()
        .any(|file| !file.locations.is_empty())
    {
        println!("Manual review locations:");
        for file in &report.source.file_details {
            for location in &file.locations {
                println!(
                    "  {}:{}:{} [{}]: {}",
                    location.file, location.line, location.column, location.kind, location.message
                );
                if let Some(suggestion) = &location.suggested_cobble_alternative {
                    println!("    suggestion: {suggestion}");
                }
            }
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

    if report.changed {
        println!("Files were changed by supported config-only migration actions.");
    } else {
        println!("No files were changed.");
    }
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
