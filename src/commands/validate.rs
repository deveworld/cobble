use crate::transpiler::SourceMap;
use crate::validator::CommandValidator;
use crate::validator::ValidationReport;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub struct ValidateOptions {
    pub input: PathBuf,
    pub commands_json: PathBuf,
}

pub fn validate(options: ValidateOptions) -> Result<(), String> {
    let report = run_validation(&options.input, &options.commands_json)?;
    print_validation_report(&report, &options.commands_json, &options.input);

    if report.errors.is_empty() && report.source_map_errors.is_empty() {
        println!("All commands valid");
        Ok(())
    } else {
        Err(format!(
            "{} validation error(s) found",
            report.errors.len() + report.source_map_errors.len()
        ))
    }
}

pub fn run_validation(input: &Path, commands_json: &Path) -> Result<ValidationReport, String> {
    if !commands_json.exists() {
        return Err(format!(
            "Command tree not found: {}\n\
            Generate it with: scripts/setup_commands_json.sh 26.1.2\n\
            Default path: data/commands.json",
            commands_json.display()
        ));
    }

    let validator = CommandValidator::from_file(commands_json)?;
    let mut report = validator.validate_datapack(input);
    report.source_map_errors = validate_source_map(input);
    Ok(report)
}

pub fn print_validation_report(
    report: &ValidationReport,
    commands_json: &Path,
    datapack_dir: &Path,
) {
    let source_map = load_source_map(datapack_dir);
    for (file, error) in &report.errors {
        eprintln!(
            "{}:{}: {}",
            file.display(),
            error.line_number,
            error.message
        );
        eprintln!("  | {}", error.command);
        if let Some(entry) = source_map.get(&source_map_key(datapack_dir, file, error.line_number))
        {
            if let Some(source) = &entry.source {
                eprintln!(
                    "  = source: {}:{}:{} ({:?})",
                    source.file.display(),
                    source.line,
                    source.column,
                    entry.kind
                );
            }
        }
    }
    for error in &report.source_map_errors {
        eprintln!("source map: {}", error);
    }

    println!(
        "Checked {} commands in {} files ({} skipped macro lines) using {}",
        report.commands_checked,
        report.files_checked,
        report.commands_skipped,
        commands_json.display()
    );
}

fn validate_source_map(datapack_dir: &Path) -> Vec<String> {
    let path = datapack_dir.join(".cobble").join("source_map.json");
    let Ok(content) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let Ok(source_map) = serde_json::from_str::<SourceMap>(&content) else {
        return vec![format!("failed to parse {}", path.display())];
    };

    let mut errors = Vec::new();
    let mut mapped_lines = HashSet::new();

    for entry in source_map.entries {
        let generated_file = datapack_dir.join(&entry.generated_path);
        mapped_lines.insert((entry.generated_path.clone(), entry.generated_line));
        let Ok(file_content) = std::fs::read_to_string(&generated_file) else {
            errors.push(format!(
                "{}:{} maps to missing file",
                entry.generated_path, entry.generated_line
            ));
            continue;
        };
        let actual = file_content
            .lines()
            .nth(entry.generated_line.saturating_sub(1));
        match actual {
            Some(actual) if actual == entry.command => {}
            Some(actual) => errors.push(format!(
                "{}:{} command mismatch: map='{}' file='{}'",
                entry.generated_path, entry.generated_line, entry.command, actual
            )),
            None => errors.push(format!(
                "{}:{} maps past end of file",
                entry.generated_path, entry.generated_line
            )),
        }
    }

    for entry in WalkDir::new(datapack_dir)
        .follow_links(false)
        .into_iter()
        .filter_map(|entry| entry.ok())
    {
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|ext| ext.to_str()) != Some("mcfunction") {
            continue;
        }
        let relative = path
            .strip_prefix(datapack_dir)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        if let Ok(content) = std::fs::read_to_string(path) {
            for (index, line) in content.lines().enumerate() {
                if line.trim().is_empty() || line.trim_start().starts_with('#') {
                    continue;
                }
                let generated_line = index + 1;
                if !mapped_lines.contains(&(relative.clone(), generated_line)) {
                    errors.push(format!(
                        "{}:{} has no source map entry",
                        relative, generated_line
                    ));
                }
            }
        }
    }

    errors
}

fn load_source_map(
    datapack_dir: &Path,
) -> HashMap<(String, usize), crate::transpiler::SourceMapEntry> {
    let path = datapack_dir.join(".cobble").join("source_map.json");
    let Ok(content) = std::fs::read_to_string(path) else {
        return HashMap::new();
    };
    let Ok(source_map) = serde_json::from_str::<SourceMap>(&content) else {
        return HashMap::new();
    };

    source_map
        .entries
        .into_iter()
        .map(|entry| ((entry.generated_path.clone(), entry.generated_line), entry))
        .collect()
}

fn source_map_key(datapack_dir: &Path, generated_file: &Path, line: usize) -> (String, usize) {
    let relative = generated_file
        .strip_prefix(datapack_dir)
        .unwrap_or(generated_file)
        .to_string_lossy()
        .replace('\\', "/");
    (relative, line)
}
