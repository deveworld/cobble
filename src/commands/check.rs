use crate::config::CobbleConfig;
use crate::error::report_parse_errors;
use crate::parser::parse;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub fn check(input: Option<PathBuf>) -> Result<(), String> {
    // Try to find cobble.toml
    let (config, config_dir) = if let Some(config_path) = find_config(&input) {
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
    let source_path = if let Some(ref input_path) = input {
        input_path.clone()
    } else if let Some(ref cfg) = config {
        config_dir.join(&cfg.build.source)
    } else {
        return Err("No input specified and no cobble.toml found".to_string());
    };

    // Check if source is a file or directory
    let files_to_check = if source_path.is_file() {
        vec![source_path.clone()]
    } else if source_path.is_dir() {
        find_cobble_files(&source_path)?
    } else {
        return Err(format!("Source path does not exist: {:?}", source_path));
    };

    if files_to_check.is_empty() {
        println!("No Cobble files found to check");
        return Ok(());
    }

    println!("Checking {} file(s)...", files_to_check.len());

    let mut total_errors = 0;
    let total_warnings = 0;
    let mut failed_files = Vec::new();

    for file_path in &files_to_check {
        let relative_path = file_path.strip_prefix(&config_dir).unwrap_or(file_path);

        let src = match fs::read_to_string(file_path) {
            Ok(content) => content,
            Err(e) => {
                println!("  ✗ {:?}: Failed to read - {}", relative_path, e);
                failed_files.push(file_path.clone());
                total_errors += 1;
                continue;
            }
        };

        match parse(&src) {
            Ok(program) => {
                // File parsed successfully
                let import_count = program.imports.len();

                // Count functions and other elements
                let mut func_count = 0;
                let mut cmd_count = 0;

                for stmt in &program.statements {
                    match stmt {
                        crate::ast::Statement::FunctionDef(_) => func_count += 1,
                        crate::ast::Statement::MinecraftCommand(_) => cmd_count += 1,
                        _ => {}
                    }
                }

                println!(
                    "  ✓ {:?}: {} imports, {} functions, {} commands",
                    relative_path, import_count, func_count, cmd_count
                );
            }
            Err(errors) => {
                // Parse failed - use ariadne for beautiful error reporting
                println!("  ✗ {:?}:", relative_path);
                let filename = file_path.to_string_lossy();
                report_parse_errors(&filename, &src, &errors);
                failed_files.push(file_path.clone());
                total_errors += 1;
            }
        }
    }

    // Summary
    println!();
    if total_errors == 0 && total_warnings == 0 {
        println!("✓ All files passed validation!");
    } else {
        if total_errors > 0 {
            println!(
                "✗ {} error(s) found in {} file(s)",
                total_errors,
                failed_files.len()
            );
        }
        if total_warnings > 0 {
            println!("⚠ {} warning(s) found", total_warnings);
        }
        return Err(format!("Validation failed with {} error(s)", total_errors));
    }

    Ok(())
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

fn find_cobble_files(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();

    for entry in WalkDir::new(dir)
        .follow_links(false) // Security: Don't follow symlinks to prevent attacks
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.is_symlink() {
            eprintln!("⚠️  Warning: Skipping symlink: {:?}", path);
            continue;
        }
        if path.is_file() {
            if let Some(ext) = path.extension() {
                if ext == "cbl" || ext == "cobble" {
                    files.push(path.to_path_buf());
                }
            }
        }
    }

    files.sort();
    Ok(files)
}
