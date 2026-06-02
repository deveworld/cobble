use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

pub struct InspectOptions {
    pub input: PathBuf,
    pub json: bool,
}

pub fn inspect(options: InspectOptions) -> Result<(), String> {
    let summary = read_inspection(&options.input)?;

    if options.json {
        let output = serde_json::json!({
            "manifest": summary.manifest,
            "source_map_entries": summary.source_map_entries,
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&output)
                .map_err(|error| format!("Failed to format inspection JSON: {error}"))?
        );
        return Ok(());
    }

    print_inspection(&options.input, &summary);
    Ok(())
}

#[derive(Debug)]
struct InspectionSummary {
    manifest: Value,
    source_map_entries: Option<usize>,
}

fn read_inspection(input: &Path) -> Result<InspectionSummary, String> {
    let manifest_path = input.join(".cobble").join("build_manifest.json");
    let manifest_content = fs::read_to_string(&manifest_path).map_err(|error| {
        format!(
            "No Cobble build manifest found at {}: {error}\n\
            Run `cobble build` on a data pack directory first. ZIP archives do not include .cobble metadata.",
            manifest_path.display()
        )
    })?;
    let manifest = serde_json::from_str::<Value>(&manifest_content)
        .map_err(|error| format!("Failed to parse {}: {error}", manifest_path.display()))?;

    Ok(InspectionSummary {
        source_map_entries: read_source_map_entry_count(input)?,
        manifest,
    })
}

fn read_source_map_entry_count(input: &Path) -> Result<Option<usize>, String> {
    let source_map_path = input.join(".cobble").join("source_map.json");
    if !source_map_path.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(&source_map_path)
        .map_err(|error| format!("Failed to read {}: {error}", source_map_path.display()))?;
    let source_map = serde_json::from_str::<Value>(&content)
        .map_err(|error| format!("Failed to parse {}: {error}", source_map_path.display()))?;
    Ok(source_map
        .get("entries")
        .and_then(Value::as_array)
        .map(Vec::len))
}

fn print_inspection(input: &Path, summary: &InspectionSummary) {
    let manifest = &summary.manifest;
    println!("Cobble inspect: {}", input.display());
    println!(
        "  Cobble version: {}",
        string_value(manifest, &["cobble_version"]).unwrap_or("unknown")
    );
    println!(
        "  Minecraft target: Java Edition {}",
        string_value(manifest, &["minecraft_version"]).unwrap_or("unknown")
    );
    println!(
        "  Pack format: {}",
        string_value(manifest, &["pack_format_text"]).unwrap_or("unknown")
    );
    println!(
        "  Namespace: {}",
        string_value(manifest, &["namespace"]).unwrap_or("unknown")
    );

    if let Some(input) = manifest.get("input").and_then(Value::as_object) {
        if let Some(source) = input.get("source").and_then(Value::as_str) {
            println!("  Source: {source}");
        }
        if let Some(compiled_files) = input.get("compiled_files").and_then(Value::as_array) {
            println!("  Source files: {}", compiled_files.len());
        }
    }

    println!("Generated:");
    print_usize(manifest, &["generated", "functions"], "  Functions");
    print_usize(manifest, &["generated", "commands"], "  Commands");
    print_usize(
        manifest,
        &["generated", "total_json_resources"],
        "  JSON resources",
    );
    print_usize(manifest, &["generated", "function_tags"], "  Function tags");

    match summary.source_map_entries {
        Some(entries) => println!("  Source map entries: {entries}"),
        None => println!("  Source map entries: none"),
    }

    if let Some(validation) = manifest.get("validation").filter(|value| !value.is_null()) {
        println!("Validation:");
        print_usize(validation, &["commands_checked"], "  Commands checked");
        print_usize(validation, &["errors"], "  Command errors");
        print_usize(validation, &["source_map_errors"], "  Source map errors");
    } else {
        println!("Validation: not recorded");
    }

    if let Some(resources) = manifest.get("resources").and_then(Value::as_array) {
        println!("Resources: {}", resources.len());
        for resource in resources.iter().take(12) {
            let kind = resource
                .get("kind")
                .and_then(Value::as_str)
                .unwrap_or("resource");
            let namespace = resource
                .get("namespace")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let path = resource.get("path").and_then(Value::as_str).unwrap_or("");
            println!("  {kind}: {namespace}:{path}");
        }
        if resources.len() > 12 {
            println!("  ... {} more", resources.len() - 12);
        }
    }
}

fn string_value<'a>(value: &'a Value, path: &[&str]) -> Option<&'a str> {
    let mut current = value;
    for segment in path {
        current = current.get(*segment)?;
    }
    current.as_str()
}

fn print_usize(value: &Value, path: &[&str], label: &str) {
    let mut current = value;
    for segment in path {
        let Some(next) = current.get(*segment) else {
            return;
        };
        current = next;
    }
    if let Some(number) = current.as_u64() {
        println!("{label}: {number}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_inspection_loads_manifest_and_source_map_count() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let cobble_dir = temp_dir.path().join(".cobble");
        fs::create_dir_all(&cobble_dir).unwrap();
        fs::write(
            cobble_dir.join("build_manifest.json"),
            r#"{
                "version": 1,
                "cobble_version": "0.0.0-test",
                "minecraft_version": "26.1.2",
                "pack_format_text": "101.1",
                "namespace": "inspect",
                "generated": {"functions": 1, "commands": 1, "total_json_resources": 0, "function_tags": 0},
                "resources": []
            }"#,
        )
        .unwrap();
        fs::write(
            cobble_dir.join("source_map.json"),
            r#"{"version":1,"entries":[{"generated_path":"data/inspect/function/main.mcfunction","generated_line":1,"command":"say hi","source":null,"kind":"UserCommand"}]}"#,
        )
        .unwrap();

        let summary = read_inspection(temp_dir.path()).unwrap();

        assert_eq!(summary.manifest["namespace"], "inspect");
        assert_eq!(summary.source_map_entries, Some(1));
    }

    #[test]
    fn read_inspection_reports_missing_manifest() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let error = read_inspection(temp_dir.path()).unwrap_err();
        assert!(error.contains("No Cobble build manifest found"));
    }
}
