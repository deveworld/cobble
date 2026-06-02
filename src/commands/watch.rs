use crate::commands::build::{build, BuildOptions};
use crate::config::CobbleConfig;
use notify::{Event, EventKind, RecursiveMode, Watcher};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

#[allow(clippy::too_many_arguments)]
pub fn watch(
    input: Option<PathBuf>,
    output: Option<PathBuf>,
    namespace: Option<String>,
    pack_format: Option<String>,
    description: Option<String>,
    verbose: bool,
    zip: bool,
    validate: bool,
    commands_json: PathBuf,
) -> Result<(), String> {
    // Try to find cobble.toml
    let (config, config_dir, config_path) = if let Some(config_path) = find_config(&input) {
        let config = if pack_format.is_some() {
            CobbleConfig::load_unvalidated(&config_path)?
        } else {
            CobbleConfig::load(&config_path)?
        };
        let config_dir = config_path.parent().unwrap().to_path_buf();
        (Some(config), config_dir, Some(config_path))
    } else {
        (
            None,
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            None,
        )
    };

    // Determine source path to watch
    let watch_path = if let Some(ref input_path) = input {
        input_path.clone()
    } else if let Some(ref cfg) = config {
        config_dir.join(&cfg.build.source)
    } else {
        return Err("No input specified and no cobble.toml found".to_string());
    };

    if !watch_path.exists() {
        return Err(format!("Watch path does not exist: {:?}", watch_path));
    }

    println!("Watching: {:?}", watch_path);
    println!("Press Ctrl+C to stop watching");
    println!();

    let build_input = input.clone();

    // Initial build
    println!("Performing initial build...");
    let build_result = build(BuildOptions {
        input: build_input.clone(),
        output: output.clone(),
        namespace: namespace.clone(),
        pack_format: pack_format.clone(),
        description: description.clone(),
        verbose,
        quiet: false,
        zip,
        validate,
        dry_run: false,
        commands_json: commands_json.clone(),
    });

    match build_result {
        Ok(()) => println!("✓ Initial build succeeded\n"),
        Err(e) => println!("✗ Initial build failed: {}\n", e),
    }

    // Set up file watcher
    let (tx, rx) = mpsc::channel();

    let mut watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
        if let Ok(event) = res {
            let _ = tx.send(event);
        }
    })
    .map_err(|e| format!("Failed to create watcher: {}", e))?;

    // Watch the path recursively
    watcher
        .watch(&watch_path, RecursiveMode::Recursive)
        .map_err(|e| format!("Failed to watch path: {}", e))?;
    let mut watched_paths = vec![watch_path.clone()];
    if let Some(config_path) = &config_path {
        watcher
            .watch(config_path, RecursiveMode::NonRecursive)
            .map_err(|e| format!("Failed to watch config file: {}", e))?;
    }

    // Process events
    loop {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(event) => {
                // Check if the event is relevant
                // Rebuild on: create, modify, delete, or rename of Cobble files or config
                let should_rebuild = match event.kind {
                    EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) => {
                        event.paths.iter().any(|p| {
                            // Rebuild if it's a Cobble file
                            if p.extension()
                                .map(|ext| ext == "cbl" || ext == "cobble")
                                .unwrap_or(false)
                            {
                                return true;
                            }
                            // Rebuild if cobble.toml changed
                            if p.file_name()
                                .map(|name| name == "cobble.toml")
                                .unwrap_or(false)
                            {
                                return true;
                            }
                            false
                        })
                    }
                    _ => false,
                };

                if should_rebuild {
                    let config_changed = event.paths.iter().any(|p| {
                        p.file_name()
                            .map(|name| name == "cobble.toml")
                            .unwrap_or(false)
                    });
                    if config_changed && input.is_none() {
                        if let Some(config_path) = &config_path {
                            let updated_config = if pack_format.is_some() {
                                CobbleConfig::load_unvalidated(config_path)
                            } else {
                                CobbleConfig::load(config_path)
                            };
                            if let Ok(updated_config) = updated_config {
                                let updated_watch_path =
                                    config_dir.join(&updated_config.build.source);
                                if updated_watch_path.exists()
                                    && !watched_paths.contains(&updated_watch_path)
                                {
                                    watcher
                                        .watch(&updated_watch_path, RecursiveMode::Recursive)
                                        .map_err(|e| {
                                            format!(
                                                "Failed to watch updated source path {:?}: {}",
                                                updated_watch_path, e
                                            )
                                        })?;
                                    println!("Watching: {:?}", updated_watch_path);
                                    watched_paths.push(updated_watch_path);
                                }
                            }
                        }
                    }

                    // Get the changed file name for display
                    let changed_file = event
                        .paths
                        .first()
                        .and_then(|p| p.file_name())
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| "unknown".to_string());

                    println!("File changed: {}", changed_file);
                    println!("Rebuilding...");

                    let build_result = build(BuildOptions {
                        input: build_input.clone(),
                        output: output.clone(),
                        namespace: namespace.clone(),
                        pack_format: pack_format.clone(),
                        description: description.clone(),
                        verbose,
                        quiet: false,
                        zip,
                        validate,
                        dry_run: false,
                        commands_json: commands_json.clone(),
                    });

                    match build_result {
                        Ok(()) => println!("✓ Build succeeded\n"),
                        Err(e) => println!("✗ Build failed: {}\n", e),
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // No events, continue watching
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err("Watcher disconnected".to_string());
            }
        }

        // Note: Ctrl+C is handled by signal handlers, no need to check stdin
    }

    // Note: This code is unreachable, but Rust requires a return
    #[allow(unreachable_code)]
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
