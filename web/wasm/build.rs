use std::fs;
use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let root_manifest = manifest_dir.join("../../Cargo.toml");
    println!("cargo:rerun-if-changed={}", root_manifest.display());

    let manifest = fs::read_to_string(&root_manifest).unwrap_or_else(|error| {
        panic!(
            "failed to read root Cargo manifest {}: {error}",
            root_manifest.display()
        )
    });
    let version = root_package_version(&manifest).unwrap_or_else(|| {
        panic!(
            "failed to find root package version in {}",
            root_manifest.display()
        )
    });

    println!("cargo:rustc-env=COBBLE_LANG_VERSION={version}");
}

fn root_package_version(manifest: &str) -> Option<&str> {
    let mut in_package = false;
    for line in manifest.lines() {
        let trimmed = line.trim();
        if trimmed == "[package]" {
            in_package = true;
            continue;
        }
        if in_package && trimmed.starts_with('[') {
            return None;
        }
        if in_package {
            if let Some(version) = trimmed.strip_prefix("version") {
                let version = version.trim_start();
                let version = version.strip_prefix('=')?.trim_start();
                return version.strip_prefix('"')?.split('"').next();
            }
        }
    }
    None
}
