use crate::commands::build::{build, BuildOptions};
use crate::commands::find_config;
use crate::commands::link::linked_output_path;
use crate::commands::output_safety::{
    build_manifest_path, ensure_no_symlink_descendants, ensure_not_symlink,
    project_marker_identity, read_build_manifest, require_manifest_ownership,
};
use crate::config::CobbleConfig;
use notify::{Event, EventKind, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const WATCH_POLL_INTERVAL: Duration = Duration::from_millis(100);
const WATCH_DEBOUNCE_INTERVAL: Duration = Duration::from_millis(250);
/// Upper bound on how long back-to-back change events may keep extending the
/// debounce window before a rebuild is forced.
const WATCH_DEBOUNCE_MAX_WINDOW: Duration = Duration::from_secs(2);

#[derive(clap::Args, Debug)]
pub struct WatchOptions {
    /// Input file or directory to watch
    pub input: Option<PathBuf>,

    /// Output directory
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Data pack namespace
    #[arg(long)]
    pub namespace: Option<String>,

    /// Pack format version (currently requires 121.0)
    #[arg(long)]
    pub pack_format: Option<String>,

    /// Data pack description
    #[arg(long)]
    pub description: Option<String>,

    /// Verbose output
    #[arg(short, long)]
    pub verbose: bool,

    /// Create a zip file
    #[arg(long)]
    pub zip: bool,

    /// Enable experimental resource-pack asset output
    #[arg(long)]
    pub experimental_resource_pack: bool,

    /// Build to the configured linked data pack target
    #[arg(long, conflicts_with = "output")]
    pub link: bool,

    /// Validate generated .mcfunction files after each successful build
    #[arg(long)]
    pub validate: bool,

    /// Path to commands.json (default data/commands.json is auto-generated if missing)
    #[arg(long, default_value = "data/commands.json")]
    pub commands_json: PathBuf,
}

pub fn watch(options: WatchOptions) -> Result<(), String> {
    if options.link && options.output.is_some() {
        return Err("--link cannot be combined with --output".to_string());
    }
    if options.link && options.namespace.is_some() {
        return Err(
            "--link cannot be combined with --namespace; linked outputs must use the project namespace"
                .to_string(),
        );
    }

    let mut session = WatchSession::start(options)?;
    let watch_path = session.watch_path.clone();

    println!("Watching: {:?}", watch_path);
    if let Some(linked_output) = session.linked_output() {
        println!("Linked output: {:?}", linked_output);
    }
    println!("Press Ctrl+C to stop watching");
    println!();

    println!("Performing initial build...");
    match build(session.build_options()) {
        Ok(()) => println!("✓ Initial build succeeded\n"),
        Err(e) => println!("✗ Initial build failed: {}\n", e),
    }

    let (tx, rx) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
        if let Ok(event) = res {
            let _ = tx.send(event);
        }
    })
    .map_err(|e| format!("Failed to create watcher: {}", e))?;
    watcher
        .watch(&watch_path, RecursiveMode::Recursive)
        .map_err(|e| format!("Failed to watch path: {}", e))?;
    let mut watched_source_paths = vec![watch_path];
    if let Some(config_path) = &session.config_path {
        watcher
            .watch(config_path, RecursiveMode::NonRecursive)
            .map_err(|e| format!("Failed to watch config file: {}", e))?;
    }

    loop {
        match rx.recv_timeout(WATCH_POLL_INTERVAL) {
            Ok(first_event) => {
                let events = collect_debounced_events(&rx, first_event);
                let decision = rebuild_decision(
                    &events,
                    session.config_path.as_deref(),
                    &session.ignored_roots,
                );
                if decision.should_rebuild {
                    if decision.config_changed {
                        session.reload_config(&mut watcher, &mut watched_source_paths);
                    }
                    session.rebuild(&decision.changed_paths);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err("Watcher disconnected".to_string());
            }
        }
    }
}

/// The state `cobble watch` keeps between rebuilds.
struct WatchSession {
    options: WatchOptions,
    config: Option<CobbleConfig>,
    config_dir: PathBuf,
    config_path: Option<PathBuf>,
    watch_path: PathBuf,
    /// The `--output` folder, or the link target under `--link`.
    output: Option<PathBuf>,
    expected_link_project_id: Option<String>,
    ignored_roots: Vec<PathBuf>,
}

impl WatchSession {
    fn start(options: WatchOptions) -> Result<Self, String> {
        let (config, config_dir, config_path) =
            if let Some(config_path) = find_config(options.input.as_deref()) {
                let config = if options.pack_format.is_some() {
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

        let watch_path = if let Some(ref input_path) = options.input {
            input_path.clone()
        } else if let Some(ref cfg) = config {
            config_dir.join(&cfg.build.source)
        } else {
            return Err("No input specified and no cobble.toml found".to_string());
        };
        if !watch_path.exists() {
            return Err(format!("Watch path does not exist: {:?}", watch_path));
        }

        let output = if options.link {
            Some(linked_output_path(&config_dir)?)
        } else {
            options.output.clone()
        };
        let expected_link_project_id = options.link.then(|| project_marker_identity(&config_dir).1);
        let ignored_roots = watch_ignored_roots(output.as_ref(), config.as_ref(), &config_dir);
        let session = Self {
            options,
            config,
            config_dir,
            config_path,
            watch_path,
            output,
            expected_link_project_id,
            ignored_roots,
        };
        session.check_linked_output()?;
        Ok(session)
    }

    fn linked_output(&self) -> Option<&PathBuf> {
        self.options.link.then_some(self.output.as_ref()).flatten()
    }

    fn check_linked_output(&self) -> Result<(), String> {
        match self.linked_output() {
            Some(linked_output) => ensure_linked_output_target_safe(
                linked_output,
                expected_link_namespace(self.options.namespace.as_deref(), self.config.as_ref()),
                self.expected_link_project_id.as_deref(),
            ),
            None => Ok(()),
        }
    }

    fn build_options(&self) -> BuildOptions {
        BuildOptions {
            input: self.options.input.clone(),
            output: self.output.clone(),
            namespace: self.options.namespace.clone(),
            pack_format: self.options.pack_format.clone(),
            description: self.options.description.clone(),
            verbose: self.options.verbose,
            quiet: false,
            zip: self.options.zip,
            experimental_resource_pack: self.options.experimental_resource_pack,
            validate: self.options.validate,
            dry_run: false,
            commands_json: self.options.commands_json.clone(),
        }
    }

    /// Reloads `cobble.toml` after it changed. On failure the last good
    /// configuration stays in use.
    fn reload_config<W: Watcher>(
        &mut self,
        watcher: &mut W,
        watched_source_paths: &mut Vec<PathBuf>,
    ) {
        match reload_watch_config(
            watcher,
            watched_source_paths,
            self.config_path.as_deref(),
            &self.config_dir,
            self.options.input.is_none(),
            self.options.pack_format.is_some(),
        ) {
            Ok(Some(updated_config)) => {
                self.config = Some(updated_config);
                if self.options.link {
                    self.resolve_link_target_again();
                }
                self.ignored_roots = watch_ignored_roots(
                    self.output.as_ref(),
                    self.config.as_ref(),
                    &self.config_dir,
                );
            }
            Ok(None) => {}
            Err(error) => {
                println!("✗ Config reload failed: {error}");
                println!("  Rebuilding with the last-good configuration.\n");
            }
        }
    }

    /// Re-resolves the link target and project identity the same way
    /// [`WatchSession::start`] did.
    fn resolve_link_target_again(&mut self) {
        match linked_output_path(&self.config_dir) {
            Ok(updated_target) => {
                if self.output.as_deref() != Some(updated_target.as_path()) {
                    println!("Linked output: {:?}", updated_target);
                }
                self.output = Some(updated_target);
                self.expected_link_project_id = Some(project_marker_identity(&self.config_dir).1);
            }
            Err(error) => {
                println!("! Link target could not be re-resolved: {error}");
                println!("  Keeping the previous linked output target.\n");
            }
        }
    }

    fn rebuild(&self, changed_paths: &[PathBuf]) {
        println!(
            "[{}] {}",
            unix_timestamp(),
            changed_paths_summary(changed_paths)
        );
        println!("Rebuilding...");

        if let Err(error) = self.check_linked_output() {
            println!("✗ Build failed: {error}\n");
            return;
        }
        match build(self.build_options()) {
            Ok(()) => println!("✓ Build succeeded\n"),
            Err(e) => println!("✗ Build failed: {}\n", e),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct RebuildDecision {
    should_rebuild: bool,
    config_changed: bool,
    changed_paths: Vec<PathBuf>,
}

fn collect_debounced_events(rx: &Receiver<Event>, first_event: Event) -> Vec<Event> {
    let started = Instant::now();
    let mut events = vec![first_event];
    // The window is capped so that a busy editor cannot starve rebuilds.
    while let Some(remaining) = WATCH_DEBOUNCE_MAX_WINDOW.checked_sub(started.elapsed()) {
        match rx.recv_timeout(WATCH_DEBOUNCE_INTERVAL.min(remaining)) {
            Ok(event) => events.push(event),
            Err(_) => break,
        }
    }
    events
}

fn rebuild_decision(
    events: &[Event],
    config_path: Option<&Path>,
    ignored_roots: &[PathBuf],
) -> RebuildDecision {
    let mut config_changed = false;
    let mut changed_paths = Vec::new();

    for event in events {
        if !is_rebuild_event_kind(&event.kind) {
            continue;
        }

        for path in &event.paths {
            if is_ignored_watch_path(path, ignored_roots) {
                continue;
            }

            if is_config_path(path, config_path) {
                config_changed = true;
                changed_paths.push(path.clone());
                continue;
            }

            if is_cobble_source_path(path) {
                changed_paths.push(path.clone());
            }
        }
    }

    changed_paths.sort();
    changed_paths.dedup();

    RebuildDecision {
        should_rebuild: !changed_paths.is_empty(),
        config_changed,
        changed_paths,
    }
}

fn is_rebuild_event_kind(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
    )
}

fn is_config_path(path: &Path, config_path: Option<&Path>) -> bool {
    let Some(config_path) = config_path else {
        return path
            .file_name()
            .map(|name| name == "cobble.toml")
            .unwrap_or(false);
    };
    path == config_path
        || canonical_path(path)
            .zip(canonical_path(config_path))
            .map(|(path, config_path)| path == config_path)
            .unwrap_or(false)
}

fn is_cobble_source_path(path: &Path) -> bool {
    path.extension()
        .map(|ext| ext == "cbl" || ext == "cobble")
        .unwrap_or(false)
}

fn is_ignored_watch_path(path: &Path, ignored_roots: &[PathBuf]) -> bool {
    if path
        .components()
        .any(|component| component.as_os_str() == ".cobble")
    {
        return true;
    }

    if path
        .file_name()
        .and_then(|name| name.to_str())
        .map(is_ignored_file_name)
        .unwrap_or(false)
    {
        return true;
    }

    if path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| matches!(ext, "zip" | "tmp" | "swp" | "swo"))
        .unwrap_or(false)
    {
        return true;
    }

    ignored_roots
        .iter()
        .any(|root| path_is_under(path, root.as_path()))
}

fn is_ignored_file_name(name: &str) -> bool {
    name.ends_with('~')
        || name.starts_with(".#")
        || name.starts_with('#')
        || name.contains(".cobble-staging-")
}

fn path_is_under(path: &Path, root: &Path) -> bool {
    if path == root || path.starts_with(root) {
        return true;
    }
    canonical_path(path)
        .zip(canonical_path(root))
        .map(|(path, root)| path == root || path.starts_with(root))
        .unwrap_or(false)
}

fn canonical_path(path: &Path) -> Option<PathBuf> {
    path.canonicalize().ok()
}

fn watch_ignored_roots(
    output: Option<&PathBuf>,
    config: Option<&CobbleConfig>,
    config_dir: &Path,
) -> Vec<PathBuf> {
    let output_dir = if let Some(output) = output {
        output.clone()
    } else if let Some(config) = config {
        config_dir.join(&config.build.output)
    } else {
        PathBuf::from("output")
    };

    vec![output_dir]
}

fn expected_link_namespace<'a>(
    namespace_override: Option<&'a str>,
    config: Option<&'a CobbleConfig>,
) -> Option<&'a str> {
    namespace_override.or_else(|| config.map(|config| config.project.namespace.as_str()))
}

fn ensure_linked_output_target_safe(
    output_dir: &Path,
    expected_namespace: Option<&str>,
    expected_project_id: Option<&str>,
) -> Result<(), String> {
    if !output_dir.exists() {
        return Ok(());
    }

    ensure_not_symlink(output_dir, "build linked output")?;

    let metadata = std::fs::symlink_metadata(output_dir).map_err(|error| {
        format!(
            "Failed to inspect linked output {}: {error}",
            output_dir.display()
        )
    })?;
    if metadata.file_type().is_symlink() {
        return Err(format!(
            "Refusing to build linked output through symlink: {}",
            output_dir.display()
        ));
    }
    if !metadata.is_dir() {
        return Err(format!(
            "Refusing to build linked output over non-directory path: {}",
            output_dir.display()
        ));
    }

    ensure_no_symlink_descendants(output_dir, "build linked output")?;

    let marker_path = build_manifest_path(output_dir);
    read_build_manifest(&marker_path)
        .and_then(|manifest| {
            require_manifest_ownership(&manifest, expected_namespace, expected_project_id)
        })
        .map_err(|error| {
            format!(
                "Refusing to build linked output {}: {error}. Remove or rename the existing directory, or choose a different --pack-name.",
                output_dir.display()
            )
        })?;

    for (relative_path, label) in [
        (Path::new("pack.mcmeta"), "pack.mcmeta"),
        (Path::new("data"), "data directory"),
    ] {
        let path = output_dir.join(relative_path);
        if !path.exists() {
            return Err(format!(
                "Refusing to build linked output {}: missing {}. Remove or rename the existing directory, or choose a different --pack-name.",
                output_dir.display(),
                label
            ));
        }
    }

    Ok(())
}

fn reload_watch_config<W: Watcher>(
    watcher: &mut W,
    watched_source_paths: &mut Vec<PathBuf>,
    config_path: Option<&Path>,
    config_dir: &Path,
    update_source_watch: bool,
    allow_invalid_pack_format: bool,
) -> Result<Option<CobbleConfig>, String> {
    let Some(config_path) = config_path else {
        return Ok(None);
    };
    let updated_config = if allow_invalid_pack_format {
        CobbleConfig::load_unvalidated(config_path)
    } else {
        CobbleConfig::load(config_path)
    }?;

    if update_source_watch {
        let updated_watch_path = config_dir.join(&updated_config.build.source);
        if !updated_watch_path.exists() {
            return Err(format!(
                "Configured source path does not exist: {}",
                updated_watch_path.display()
            ));
        }
        replace_watched_source_paths(watcher, watched_source_paths, updated_watch_path)?;
    }

    Ok(Some(updated_config))
}

fn replace_watched_source_paths<W: Watcher>(
    watcher: &mut W,
    watched_source_paths: &mut Vec<PathBuf>,
    updated_watch_path: PathBuf,
) -> Result<(), String> {
    if watched_source_paths.len() == 1 && watched_source_paths[0] == updated_watch_path {
        return Ok(());
    }

    if !watched_source_paths.contains(&updated_watch_path) {
        watcher
            .watch(&updated_watch_path, RecursiveMode::Recursive)
            .map_err(|error| {
                format!(
                    "Failed to watch updated source path {:?}: {}",
                    updated_watch_path, error
                )
            })?;
        println!("Watching: {:?}", updated_watch_path);
    }

    for old_path in watched_source_paths.iter() {
        if old_path != &updated_watch_path {
            watcher
                .unwatch(old_path)
                .map_err(|error| format!("Failed to stop watching {:?}: {}", old_path, error))?;
            println!("Stopped watching: {:?}", old_path);
        }
    }

    watched_source_paths.clear();
    watched_source_paths.push(updated_watch_path);
    Ok(())
}

fn changed_paths_summary(paths: &[PathBuf]) -> String {
    match paths {
        [] => "No relevant file changes".to_string(),
        [path] => format!("Changed: {}", path.display()),
        paths => {
            let preview = paths
                .iter()
                .take(3)
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ");
            if paths.len() > 3 {
                format!("Changed {} files: {preview}, ...", paths.len())
            } else {
                format!("Changed {} files: {preview}", paths.len())
            }
        }
    }
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{CreateKind, ModifyKind};
    use notify::{Config, EventHandler, Result as NotifyResult, WatcherKind};
    use std::fs;

    #[derive(Default)]
    struct FakeWatcher {
        watched: Vec<PathBuf>,
        unwatched: Vec<PathBuf>,
    }

    impl Watcher for FakeWatcher {
        fn new<F: EventHandler>(_event_handler: F, _config: Config) -> NotifyResult<Self>
        where
            Self: Sized,
        {
            Ok(Self::default())
        }

        fn watch(&mut self, path: &Path, _recursive_mode: RecursiveMode) -> NotifyResult<()> {
            self.watched.push(path.to_path_buf());
            Ok(())
        }

        fn unwatch(&mut self, path: &Path) -> NotifyResult<()> {
            self.unwatched.push(path.to_path_buf());
            Ok(())
        }

        fn kind() -> WatcherKind
        where
            Self: Sized,
        {
            WatcherKind::NullWatcher
        }
    }

    fn modify_event(path: impl Into<PathBuf>) -> Event {
        Event::new(EventKind::Modify(ModifyKind::Any)).add_path(path.into())
    }

    fn create_event(path: impl Into<PathBuf>) -> Event {
        Event::new(EventKind::Create(CreateKind::File)).add_path(path.into())
    }

    #[test]
    fn collect_debounced_events_caps_the_debounce_window() {
        let (tx, rx) = mpsc::channel();
        let sender = std::thread::spawn(move || {
            let started = Instant::now();
            while started.elapsed() < Duration::from_secs(4) {
                if tx.send(modify_event("src/main.cbl")).is_err() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        });

        let started = Instant::now();
        let events = collect_debounced_events(&rx, modify_event("src/main.cbl"));

        assert!(!events.is_empty());
        assert!(
            started.elapsed() < Duration::from_secs(4),
            "a busy event stream must not extend the debounce window forever"
        );
        drop(rx);
        sender.join().unwrap();
    }

    #[test]
    fn rebuild_decision_coalesces_source_and_config_events() {
        let config_path = PathBuf::from("cobble.toml");
        let source_path = PathBuf::from("src/main.cbl");
        let events = vec![
            modify_event(&source_path),
            modify_event(&source_path),
            modify_event(&config_path),
        ];

        let decision = rebuild_decision(&events, Some(&config_path), &[]);

        assert!(decision.should_rebuild);
        assert!(decision.config_changed);
        assert_eq!(
            decision.changed_paths,
            vec![config_path, PathBuf::from("src/main.cbl")]
        );
    }

    #[test]
    fn rebuild_decision_ignores_generated_metadata_zip_and_temp_files() {
        let ignored_roots = vec![PathBuf::from("output")];
        let events = vec![
            modify_event("output/data/example/function/main.cbl"),
            modify_event("src/.cobble/source_map.cbl"),
            create_event("example.zip"),
            modify_event("src/main.cbl.swp"),
            modify_event("src/.#main.cbl"),
            modify_event("src/main.cbl~"),
        ];

        let decision = rebuild_decision(&events, None, &ignored_roots);

        assert!(!decision.should_rebuild);
        assert!(decision.changed_paths.is_empty());
    }

    #[test]
    fn rebuild_decision_ignores_sources_written_into_the_linked_pack() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let linked_pack = temp_dir.path().join("world/datapacks/watch_pack");
        let source = temp_dir.path().join("watch_pack/src/main.cbl");
        // `watch --link` passes the linked pack as the output folder.
        let ignored_roots = watch_ignored_roots(Some(&linked_pack), None, temp_dir.path());

        let noise = vec![create_event(
            linked_pack.join("data/watch_pack/function/generated_noise.cbl"),
        )];
        let decision = rebuild_decision(&noise, None, &ignored_roots);
        assert!(!decision.should_rebuild, "{decision:?}");

        let edit = vec![modify_event(&source)];
        let decision = rebuild_decision(&edit, None, &ignored_roots);
        assert!(decision.should_rebuild);
        assert_eq!(decision.changed_paths, vec![source]);
    }

    #[test]
    fn rebuild_decision_keeps_real_source_changes_after_ignored_events() {
        let ignored_roots = vec![PathBuf::from("output")];
        let events = vec![
            modify_event("output/data/example/function/main.mcfunction"),
            modify_event("src/main.cbl"),
        ];

        let decision = rebuild_decision(&events, None, &ignored_roots);

        assert!(decision.should_rebuild);
        assert!(!decision.config_changed);
        assert_eq!(decision.changed_paths, vec![PathBuf::from("src/main.cbl")]);
    }

    #[test]
    fn replace_watched_source_paths_unwatches_old_source_and_watches_new_source() {
        let mut watcher = FakeWatcher::default();
        let mut watched_paths = vec![PathBuf::from("src")];

        replace_watched_source_paths(&mut watcher, &mut watched_paths, PathBuf::from("modules"))
            .unwrap();

        assert_eq!(watcher.unwatched, vec![PathBuf::from("src")]);
        assert_eq!(watcher.watched, vec![PathBuf::from("modules")]);
        assert_eq!(watched_paths, vec![PathBuf::from("modules")]);
    }

    #[test]
    fn reload_watch_config_rejects_missing_updated_source_without_changing_watches() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let config_path = temp_dir.path().join("cobble.toml");
        fs::write(
            &config_path,
            r#"
[project]
name = "watch_project"
description = "Watch project"
namespace = "watch_project"
version = "1.0.0"
pack_format = "121.0"

[build]
source = "missing_src"
output = "output"
entry_points = []
"#,
        )
        .unwrap();
        let mut watcher = FakeWatcher::default();
        let mut watched_paths = vec![temp_dir.path().join("src")];

        let error = reload_watch_config(
            &mut watcher,
            &mut watched_paths,
            Some(&config_path),
            temp_dir.path(),
            true,
            false,
        )
        .unwrap_err();

        assert!(error.contains("Configured source path does not exist"));
        assert!(watcher.watched.is_empty());
        assert!(watcher.unwatched.is_empty());
        assert_eq!(watched_paths, vec![temp_dir.path().join("src")]);
    }

    #[test]
    fn linked_output_target_allows_missing_directory() {
        let temp_dir = tempfile::TempDir::new().unwrap();

        ensure_linked_output_target_safe(
            &temp_dir.path().join("missing_pack"),
            Some("watch_pack"),
            Some("project-id"),
        )
        .unwrap();
    }

    #[test]
    fn linked_output_target_rejects_unmarked_directory() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let output_dir = temp_dir.path().join("pack");
        fs::create_dir_all(&output_dir).unwrap();
        fs::write(output_dir.join("important.txt"), "keep").unwrap();

        let error =
            ensure_linked_output_target_safe(&output_dir, Some("watch_pack"), Some("project-id"))
                .unwrap_err();

        assert!(error.contains("Refusing to build linked output"));
        assert!(error.contains("missing or unreadable .cobble/build_manifest.json"));
        assert!(output_dir.join("important.txt").exists());
    }

    #[cfg(unix)]
    #[test]
    fn linked_output_target_rejects_symlink_descendant() {
        use std::os::unix::fs::symlink;

        let temp_dir = tempfile::TempDir::new().unwrap();
        let output_dir = temp_dir.path().join("pack");
        let outside_marker_dir = temp_dir.path().join("outside-marker");
        fs::create_dir_all(&output_dir).unwrap();
        fs::create_dir_all(&outside_marker_dir).unwrap();
        fs::create_dir_all(output_dir.join("data/watch_pack/function")).unwrap();
        fs::write(output_dir.join("pack.mcmeta"), "{}").unwrap();
        fs::write(
            outside_marker_dir.join("build_manifest.json"),
            r#"{
  "version": 1,
  "cobble_version": "0.7.3",
  "namespace": "watch_pack",
  "project_id": "project-id"
}"#,
        )
        .unwrap();
        symlink(&outside_marker_dir, output_dir.join(".cobble")).unwrap();

        let error =
            ensure_linked_output_target_safe(&output_dir, Some("watch_pack"), Some("project-id"))
                .unwrap_err();

        assert!(error.contains("Refusing to build linked output through symlink"));
    }

    #[test]
    fn linked_output_target_accepts_marked_pack() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let output_dir = temp_dir.path().join("pack");
        fs::create_dir_all(output_dir.join(".cobble")).unwrap();
        fs::create_dir_all(output_dir.join("data/watch_pack/function")).unwrap();
        fs::write(output_dir.join("pack.mcmeta"), "{}").unwrap();
        fs::write(
            output_dir.join(".cobble/build_manifest.json"),
            r#"{
  "version": 1,
  "cobble_version": "0.7.2",
  "namespace": "watch_pack",
  "project_id": "project-id"
}"#,
        )
        .unwrap();

        ensure_linked_output_target_safe(&output_dir, Some("watch_pack"), Some("project-id"))
            .unwrap();
    }

    #[test]
    fn linked_output_target_rejects_missing_marker_project_id() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let output_dir = temp_dir.path().join("pack");
        fs::create_dir_all(output_dir.join(".cobble")).unwrap();
        fs::create_dir_all(output_dir.join("data/watch_pack/function")).unwrap();
        fs::write(output_dir.join("pack.mcmeta"), "{}").unwrap();
        fs::write(
            output_dir.join(".cobble/build_manifest.json"),
            r#"{
  "version": 1,
  "cobble_version": "0.7.2",
  "namespace": "watch_pack"
}"#,
        )
        .unwrap();

        let error =
            ensure_linked_output_target_safe(&output_dir, Some("watch_pack"), Some("project-id"))
                .unwrap_err();

        assert!(error.contains("missing project_id"));
    }

    #[test]
    fn linked_output_target_rejects_mismatched_marker_namespace() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let output_dir = temp_dir.path().join("pack");
        fs::create_dir_all(output_dir.join(".cobble")).unwrap();
        fs::create_dir_all(output_dir.join("data/other_pack/function")).unwrap();
        fs::write(output_dir.join("pack.mcmeta"), "{}").unwrap();
        fs::write(
            output_dir.join(".cobble/build_manifest.json"),
            r#"{
  "version": 1,
  "cobble_version": "0.7.2",
  "namespace": "other_pack"
}"#,
        )
        .unwrap();

        let error =
            ensure_linked_output_target_safe(&output_dir, Some("watch_pack"), Some("project-id"))
                .unwrap_err();

        assert!(error.contains("marker namespace `other_pack`"));
        assert!(error.contains("project namespace `watch_pack`"));
    }
}
