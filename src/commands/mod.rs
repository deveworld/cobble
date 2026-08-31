pub mod build;
pub mod check;
pub mod clean;
pub mod doctor;
pub mod fmt;
pub mod init;
pub mod inspect;
pub mod link;
pub mod migrate;
pub(crate) mod output_safety;
pub mod validate;
pub mod watch;

pub use build::build;
pub use check::{check, CheckOptions};
pub use clean::{clean, CleanOptions};
pub use doctor::{doctor, DoctorOptions};
pub use fmt::{format as format_sources, FmtOptions};
pub use init::init;
pub use inspect::{inspect, InspectOptions};
pub use link::{link, LinkOptions};
pub use migrate::{migrate, MigrateOptions};
pub use validate::validate;
pub use watch::{watch, WatchOptions};

use crate::config::CobbleConfig;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Locate the project's `cobble.toml`: for a file input, search its parent
/// directories; for a directory input, search the directory itself; with no
/// input, search the current directory.
pub(crate) fn find_config(input: Option<&Path>) -> Option<PathBuf> {
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

pub(crate) fn resolve_entry_points(
    source_dir: &Path,
    entry_points: &[String],
) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();

    for entry_point in entry_points {
        let entry_path = Path::new(entry_point);
        let path = if entry_path.is_absolute() {
            entry_path.to_path_buf()
        } else {
            source_dir.join(entry_path)
        };

        if path.is_file() {
            files.push(path);
        } else if path.is_dir() {
            files.extend(find_cobble_files(&path)?);
        } else {
            return Err(format!("Entry point does not exist: {}", path.display()));
        }
    }

    // Entry points may overlap (for example a directory plus a file inside
    // it); compile each file once, keeping the first occurrence's order.
    let mut seen = HashSet::new();
    files.retain(|path| seen.insert(path.canonicalize().unwrap_or_else(|_| path.clone())));

    Ok(files)
}

pub(crate) fn find_cobble_files(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();

    for entry in WalkDir::new(dir)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.is_symlink() {
            eprintln!("Warning: skipping symlink: {:?}", path);
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
