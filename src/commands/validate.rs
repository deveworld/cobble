use crate::commands::output_safety::ensure_no_symlink_components;
use crate::pack_format::SUPPORTED_MINECRAFT_VERSION;
use crate::transpiler::SourceMap;
use crate::validator::CommandValidator;
use crate::validator::ValidationReport;
use sha1::{Digest, Sha1};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use walkdir::WalkDir;

const VERSION_MANIFEST_URLS: &[&str] = &[
    "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json",
    "https://launchermeta.mojang.com/mc/game/version_manifest_v2.json",
    "https://piston-meta.mojang.com/mc/game/version_manifest.json",
    "https://launchermeta.mojang.com/mc/game/version_manifest.json",
];

const SUPPORTED_SERVER_JAR_URL: &str =
    "https://piston-data.mojang.com/v1/objects/97ccd4c0ed3f81bbb7bfacddd1090b0c56f9bc51/server.jar";
const SUPPORTED_SERVER_JAR_SHA1: &str = "97ccd4c0ed3f81bbb7bfacddd1090b0c56f9bc51";
pub const SUPPORTED_COMMANDS_JSON_SHA1: &str = "18bb0eb6768838b2237821418aa5832d1c837d45";

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
    ensure_commands_json(commands_json)?;

    let validator = CommandValidator::from_file(commands_json)?;
    let mut report = validator.validate_datapack(input);
    report.source_map_errors = validate_source_map(input);
    Ok(report)
}

fn ensure_commands_json(commands_json: &Path) -> Result<(), String> {
    ensure_commands_json_path_safe(commands_json)?;

    if commands_json.exists() {
        return verify_default_commands_json_fingerprint(commands_json);
    }

    if !is_auto_download_commands_json_path(commands_json) {
        return Err(missing_command_tree_error(commands_json));
    }

    if std::env::var("COBBLE_COMMANDS_JSON_AUTO_DOWNLOAD").as_deref() == Ok("0") {
        return Err(format!(
            "{}\nAutomatic download is disabled by COBBLE_COMMANDS_JSON_AUTO_DOWNLOAD=0.",
            missing_command_tree_error(commands_json)
        ));
    }

    generate_commands_json(commands_json)?;
    verify_default_commands_json_fingerprint(commands_json)
}

fn ensure_commands_json_path_safe(commands_json: &Path) -> Result<(), String> {
    if is_auto_download_commands_json_path(commands_json) {
        ensure_no_symlink_components(commands_json, "generate commands.json")?;
    }
    Ok(())
}

fn verify_default_commands_json_fingerprint(commands_json: &Path) -> Result<(), String> {
    if !is_auto_download_commands_json_path(commands_json) {
        return Ok(());
    }

    verify_commands_json_sha1(commands_json, SUPPORTED_COMMANDS_JSON_SHA1).map_err(|error| {
        format!(
            "{}\n\
            The default command tree must match Minecraft Java Edition {}. Remove {} and rerun validation to regenerate it, or pass --commands-json with a deliberate custom tree.",
            error,
            SUPPORTED_MINECRAFT_VERSION,
            commands_json.display()
        )
    })
}

fn verify_commands_json_sha1(commands_json: &Path, expected_sha1: &str) -> Result<(), String> {
    let actual_sha1 = sha1_file(commands_json)?;
    if actual_sha1.eq_ignore_ascii_case(expected_sha1) {
        return Ok(());
    }

    Err(format!(
        "Command tree SHA-1 mismatch for {}: expected {}, got {}",
        commands_json.display(),
        expected_sha1,
        actual_sha1
    ))
}

fn missing_command_tree_error(commands_json: &Path) -> String {
    format!(
        "Command tree not found: {}\n\
        Cobble can auto-generate the default data/commands.json path during validation.\n\
        For custom paths, generate data/commands.json with scripts/setup_commands_json.sh {} and copy it to the requested path.\n\
        Default path: data/commands.json",
        commands_json.display(),
        SUPPORTED_MINECRAFT_VERSION
    )
}

fn is_auto_download_commands_json_path(commands_json: &Path) -> bool {
    commands_json == Path::new("data/commands.json")
}

fn generate_commands_json(commands_json: &Path) -> Result<(), String> {
    let output_dir = commands_json.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(output_dir).map_err(|e| {
        format!(
            "Failed to create commands.json directory {}: {}",
            output_dir.display(),
            e
        )
    })?;

    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| {
            format!(
                "System clock error while creating command tree cache: {}",
                e
            )
        })?
        .as_nanos();
    let work_dir = output_dir.join(format!(".commands-json-{}-{}", std::process::id(), stamp));
    fs::create_dir_all(&work_dir).map_err(|e| {
        format!(
            "Failed to create temporary command tree directory {}: {}",
            work_dir.display(),
            e
        )
    })?;

    println!(
        "Command tree not found at {}; generating Minecraft {} commands.json...",
        commands_json.display(),
        SUPPORTED_MINECRAFT_VERSION
    );

    let result = generate_commands_json_in(&work_dir, commands_json);
    let cleanup_result = fs::remove_dir_all(&work_dir);
    if let Err(error) = cleanup_result {
        eprintln!(
            "Warning: failed to remove temporary command tree directory {}: {}",
            work_dir.display(),
            error
        );
    }
    result
}

fn generate_commands_json_in(work_dir: &Path, commands_json: &Path) -> Result<(), String> {
    if let Ok(commands_url) = std::env::var("COBBLE_COMMANDS_JSON_URL") {
        download_ready_made_commands_json(&commands_url, commands_json)?;
        return Ok(());
    }

    let server_jar = work_dir.join("server.jar");
    let expected_sha1 = prepare_server_jar(&server_jar)?;
    verify_sha1(&server_jar, &expected_sha1)?;
    let server_jar = server_jar.canonicalize().map_err(|e| {
        format!(
            "Failed to resolve Minecraft server jar path {}: {}",
            server_jar.display(),
            e
        )
    })?;

    run_command(
        Command::new("java")
            .arg("-DbundlerMainClass=net.minecraft.data.Main")
            .arg("-jar")
            .arg(&server_jar)
            .arg("--reports")
            .current_dir(work_dir),
        "generate Minecraft command reports",
    )?;

    let generated_commands = find_generated_commands_json(work_dir).ok_or_else(|| {
        format!(
            "Minecraft server reports did not generate commands.json under {}",
            work_dir.display()
        )
    })?;

    let partial = commands_json.with_file_name("commands.json.part");
    let _ = fs::remove_file(&partial);
    fs::copy(&generated_commands, &partial).map_err(|e| {
        format!(
            "Failed to copy generated commands.json from {} to {}: {}",
            generated_commands.display(),
            partial.display(),
            e
        )
    })?;
    fs::rename(&partial, commands_json).map_err(|e| {
        format!(
            "Failed to move generated commands.json to {}: {}",
            commands_json.display(),
            e
        )
    })?;

    println!("Generated commands.json at {}", commands_json.display());
    Ok(())
}

fn download_ready_made_commands_json(url: &str, commands_json: &Path) -> Result<(), String> {
    println!("Downloading commands.json from COBBLE_COMMANDS_JSON_URL...");
    let partial = commands_json.with_file_name("commands.json.part");
    let _ = fs::remove_file(&partial);
    run_command(
        Command::new("curl")
            .args(["-fsSL", "--retry", "3", "-o"])
            .arg(&partial)
            .arg(url),
        "download commands.json",
    )?;
    fs::rename(&partial, commands_json).map_err(|e| {
        format!(
            "Failed to move downloaded commands.json to {}: {}",
            commands_json.display(),
            e
        )
    })?;
    println!("Downloaded commands.json at {}", commands_json.display());
    Ok(())
}

fn prepare_server_jar(server_jar: &Path) -> Result<String, String> {
    let override_sha1 = std::env::var("COBBLE_MINECRAFT_SERVER_SHA1").ok();

    if let Ok(local_jar) = std::env::var("COBBLE_MINECRAFT_SERVER_JAR") {
        let local_jar = PathBuf::from(local_jar);
        if !local_jar.exists() {
            return Err(format!(
                "COBBLE_MINECRAFT_SERVER_JAR points to a missing file: {}",
                local_jar.display()
            ));
        }
        fs::copy(&local_jar, server_jar).map_err(|e| {
            format!(
                "Failed to copy local Minecraft server jar from {} to {}: {}",
                local_jar.display(),
                server_jar.display(),
                e
            )
        })?;
        return Ok(override_sha1.unwrap_or_else(|| SUPPORTED_SERVER_JAR_SHA1.to_string()));
    }

    let (server_url, expected_sha1) = if let Ok(url) = std::env::var("COBBLE_MINECRAFT_SERVER_URL")
    {
        (
            url,
            override_sha1.unwrap_or_else(|| SUPPORTED_SERVER_JAR_SHA1.to_string()),
        )
    } else {
        resolve_server_jar_url()?
    };

    run_command(
        Command::new("curl")
            .args(["-fsSL", "--retry", "3", "-o"])
            .arg(server_jar)
            .arg(&server_url),
        "download Minecraft server.jar",
    )?;
    Ok(expected_sha1)
}

fn resolve_server_jar_url() -> Result<(String, String), String> {
    let mut errors = Vec::new();
    for manifest_url in VERSION_MANIFEST_URLS {
        let manifest = match curl_json(manifest_url) {
            Ok(manifest) => manifest,
            Err(error) => {
                errors.push(format!("{}: {}", manifest_url, error));
                continue;
            }
        };

        let Some(version_url) = manifest
            .get("versions")
            .and_then(|value| value.as_array())
            .and_then(|versions| {
                versions.iter().find_map(|version| {
                    if version.get("id").and_then(|value| value.as_str())
                        == Some(SUPPORTED_MINECRAFT_VERSION)
                    {
                        version.get("url").and_then(|value| value.as_str())
                    } else {
                        None
                    }
                })
            })
        else {
            errors.push(format!(
                "{}: Minecraft version {} was not found",
                manifest_url, SUPPORTED_MINECRAFT_VERSION
            ));
            continue;
        };

        let version_info = match curl_json(version_url) {
            Ok(version_info) => version_info,
            Err(error) => {
                errors.push(format!("{}: {}", version_url, error));
                continue;
            }
        };

        let server_url = version_info
            .pointer("/downloads/server/url")
            .and_then(|value| value.as_str());
        let server_sha1 = version_info
            .pointer("/downloads/server/sha1")
            .and_then(|value| value.as_str());

        if let (Some(server_url), Some(server_sha1)) = (server_url, server_sha1) {
            return Ok((server_url.to_string(), server_sha1.to_string()));
        }

        errors.push(format!(
            "{}: metadata does not include a server download URL and SHA-1",
            version_url
        ));
    }

    eprintln!(
        "Warning: failed to resolve Minecraft server jar from manifests; using pinned {} direct server.jar URL.",
        SUPPORTED_MINECRAFT_VERSION
    );
    for error in &errors {
        eprintln!("  - {}", error);
    }
    Ok((
        SUPPORTED_SERVER_JAR_URL.to_string(),
        SUPPORTED_SERVER_JAR_SHA1.to_string(),
    ))
}

fn verify_sha1(path: &Path, expected_sha1: &str) -> Result<(), String> {
    let actual_sha1 = sha1_file(path)?;
    if actual_sha1.eq_ignore_ascii_case(expected_sha1) {
        return Ok(());
    }

    let _ = fs::remove_file(path);
    Err(format!(
        "Downloaded Minecraft server jar failed SHA-1 verification: expected {}, got {}",
        expected_sha1, actual_sha1
    ))
}

fn sha1_file(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path).map_err(|e| {
        format!(
            "Failed to open {} for SHA-1 verification: {}",
            path.display(),
            e
        )
    })?;
    let mut hasher = Sha1::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let read = file.read(&mut buffer).map_err(|e| {
            format!(
                "Failed to read {} for SHA-1 verification: {}",
                path.display(),
                e
            )
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn curl_json(url: &str) -> Result<serde_json::Value, String> {
    let output = Command::new("curl")
        .args(["-fsSL", "--retry", "3", url])
        .output()
        .map_err(|e| format!("Failed to execute curl while fetching {}: {}", url, e))?;
    if !output.status.success() {
        return Err(format!(
            "curl failed while fetching {}: {}",
            url,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("Failed to parse JSON response from {}: {}", url, e))
}

fn run_command(command: &mut Command, description: &str) -> Result<(), String> {
    let output = command
        .output()
        .map_err(|e| format!("Failed to execute command to {}: {}", description, e))?;
    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    Err(format!(
        "Failed to {}.\nstdout:\n{}\nstderr:\n{}",
        description, stdout, stderr
    ))
}

fn find_generated_commands_json(work_dir: &Path) -> Option<PathBuf> {
    WalkDir::new(work_dir)
        .follow_links(false)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.into_path())
        .find(|path| {
            path.is_file()
                && path.file_name().and_then(|name| name.to_str()) == Some("commands.json")
                && path
                    .components()
                    .any(|component| component.as_os_str() == "reports")
        })
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
        if error.position <= error.command.chars().count() {
            eprintln!("  | {}^", " ".repeat(error.position));
        }
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
        "Checked {} commands in {} files ({} macro commands checked, {} skipped) using {}",
        report.commands_checked,
        report.files_checked,
        report.macro_commands_checked,
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
        let Some(generated_path) = clean_source_map_generated_path(&entry.generated_path) else {
            errors.push(format!(
                "{}:{} has invalid generated_path outside the data pack",
                entry.generated_path, entry.generated_line
            ));
            continue;
        };
        let generated_path_key = generated_path.to_string_lossy().replace('\\', "/");
        let generated_file = datapack_dir.join(&generated_path);
        mapped_lines.insert((generated_path_key.clone(), entry.generated_line));
        if !is_regular_file_no_symlink(&generated_file) {
            errors.push(format!(
                "{}:{} maps to missing or non-regular file",
                generated_path_key, entry.generated_line
            ));
            continue;
        }
        let Ok(file_content) = std::fs::read_to_string(&generated_file) else {
            errors.push(format!(
                "{}:{} maps to missing file",
                generated_path_key, entry.generated_line
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
                generated_path_key, entry.generated_line, entry.command, actual
            )),
            None => errors.push(format!(
                "{}:{} maps past end of file",
                generated_path_key, entry.generated_line
            )),
        }
    }

    for entry in WalkDir::new(datapack_dir)
        .follow_links(false)
        .into_iter()
        .filter_map(|entry| entry.ok())
    {
        let path = entry.path();
        if !entry.file_type().is_file()
            || path.extension().and_then(|ext| ext.to_str()) != Some("mcfunction")
        {
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

fn is_regular_file_no_symlink(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_file())
        .unwrap_or(false)
}

fn clean_source_map_generated_path(generated_path: &str) -> Option<PathBuf> {
    let path = Path::new(generated_path);
    if path.is_absolute() {
        return None;
    }

    let mut clean = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => clean.push(part),
            _ => return None,
        }
    }

    if clean.as_os_str().is_empty() {
        None
    } else {
        Some(clean)
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn cwd_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    struct CurrentDirGuard {
        previous: PathBuf,
    }

    impl CurrentDirGuard {
        fn push(path: &Path) -> Self {
            let previous = std::env::current_dir().unwrap();
            std::env::set_current_dir(path).unwrap();
            Self { previous }
        }
    }

    impl Drop for CurrentDirGuard {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.previous);
        }
    }

    #[test]
    fn auto_download_only_applies_to_default_commands_json_paths() {
        assert!(is_auto_download_commands_json_path(Path::new(
            "data/commands.json"
        )));
        assert!(!is_auto_download_commands_json_path(Path::new(
            "../../data/commands.json"
        )));
        assert!(!is_auto_download_commands_json_path(Path::new(
            "/tmp/project/data/commands.json"
        )));

        assert!(!is_auto_download_commands_json_path(Path::new(
            "commands.json"
        )));
        assert!(!is_auto_download_commands_json_path(Path::new(
            "data/missing_commands.json"
        )));
        assert!(!is_auto_download_commands_json_path(Path::new(
            "custom/commands.json"
        )));
    }

    #[test]
    fn missing_custom_commands_json_path_reports_manual_setup() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let input_dir = temp_dir.path().join("pack");
        let commands_json = temp_dir.path().join("missing_commands.json");
        fs::create_dir_all(&input_dir).unwrap();

        let error = run_validation(&input_dir, &commands_json).unwrap_err();

        assert!(error.contains("Command tree not found"));
        assert!(error.contains("scripts/setup_commands_json.sh"));
        assert!(!commands_json.exists());
    }

    #[cfg(unix)]
    #[test]
    fn default_commands_json_rejects_symlink_parent_before_generation() {
        use std::os::unix::fs::symlink;

        let _guard = cwd_lock().lock().unwrap();
        let temp_dir = tempfile::TempDir::new().unwrap();
        let outside_dir = temp_dir.path().join("outside-data");
        fs::create_dir_all(&outside_dir).unwrap();
        symlink(&outside_dir, temp_dir.path().join("data")).unwrap();
        let _cwd = CurrentDirGuard::push(temp_dir.path());

        let error = ensure_commands_json(Path::new("data/commands.json")).unwrap_err();

        assert!(error.contains("Refusing to generate commands.json through symlink"));
        assert!(!outside_dir.join("commands.json").exists());
        assert!(!outside_dir.join("commands.json.part").exists());
    }

    #[test]
    fn command_tree_fingerprint_rejects_wrong_sha1() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let commands_json = temp_dir.path().join("commands.json");
        fs::write(&commands_json, "{}").unwrap();

        let error =
            verify_commands_json_sha1(&commands_json, SUPPORTED_COMMANDS_JSON_SHA1).unwrap_err();

        assert!(error.contains("Command tree SHA-1 mismatch"));
    }

    #[test]
    fn run_validation_accepts_custom_command_tree_without_supported_fingerprint() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let input_dir = temp_dir.path().join("pack");
        let commands_json = temp_dir.path().join("custom_commands.json");
        fs::create_dir_all(&input_dir).unwrap();
        fs::write(&commands_json, r#"{"type":"root","children":{}}"#).unwrap();

        let report = run_validation(&input_dir, &commands_json).unwrap();

        assert_eq!(report.files_checked, 0);
        assert!(report.errors.is_empty());
        assert!(report.source_map_errors.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn download_ready_made_commands_json_accepts_local_fixture_url() {
        if Command::new("curl").arg("--version").output().is_err() {
            eprintln!("Skipping commands.json download fixture test: curl not available");
            return;
        }

        let temp_dir = tempfile::TempDir::new().unwrap();
        let fixture = temp_dir.path().join("fixture_commands.json");
        let commands_json = temp_dir.path().join("commands.json");
        let fixture_content = r#"{"type":"root","children":{}}"#;
        fs::write(&fixture, fixture_content).unwrap();

        download_ready_made_commands_json(&format!("file://{}", fixture.display()), &commands_json)
            .unwrap();

        assert_eq!(fs::read_to_string(commands_json).unwrap(), fixture_content);
        assert!(!temp_dir.path().join("commands.json.part").exists());
    }
}
