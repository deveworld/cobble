use super::{find_cobble_files, find_config, resolve_entry_points};
use crate::commands::output_safety::{
    build_manifest_path, ensure_no_symlink_descendants, ensure_no_symlinks_from,
    ensure_not_symlink, project_marker_identity, read_build_manifest, require_output_ownership,
};
use crate::commands::validate::{print_validation_report, resolve_commands_json, run_validation};
use crate::config::CobbleConfig;
use crate::diagnostics::{
    format_file_diagnostics, parse_source_files, warning_diagnostics, ParsedSourceFile,
};
use crate::fs_safety::{
    copy_file_atomic, create_temp_output_file, replace_with_temp_file, write_file_atomic,
};
use crate::pack_format::{
    PackFormat, COBBLE_VERSION, SUPPORTED_MINECRAFT_VERSION, SUPPORTED_PACK_FORMAT,
};
use crate::transpiler::{
    BuildManifestInput, BuildManifestValidation, TranspileWarning, Transpiler, TranspilerOptions,
};
use crate::validator::ValidationReport;
use std::collections::HashSet;
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Component, Path, PathBuf};
use walkdir::WalkDir;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

pub struct BuildOptions {
    pub input: Option<PathBuf>,
    pub output: Option<PathBuf>,
    pub namespace: Option<String>,
    pub pack_format: Option<String>,
    pub description: Option<String>,
    pub verbose: bool,
    pub quiet: bool,
    pub zip: bool,
    pub experimental_resource_pack: bool,
    pub validate: bool,
    pub dry_run: bool,
    pub commands_json: PathBuf,
}

pub fn build(options: BuildOptions) -> Result<(), String> {
    ensure_compatible_flags(&options)?;
    let project = resolve_project(&options)?;
    check_output_location(&options, &project)?;
    let asset_passthrough_files = if project.experimental_resource_pack {
        collect_resource_pack_asset_passthrough_files(&project.project_root)?
    } else {
        Vec::new()
    };
    print_build_start(&options, &project);

    let output = OutputDirs::new(&project.output_dir, options.validate)?;
    let mut transpiler = compile_project(&options, &project, output.build_dir())?;
    let assets = prepare_resource_pack_assets(&mut transpiler, &project, asset_passthrough_files)?;
    if !options.dry_run || options.validate {
        output.discard_staging_on_error(
            transpiler
                .write_data_pack()
                .map_err(|e| format!("Failed to write data pack: {}", e)),
        )?;
    }
    let validation_summary = if options.validate {
        Some(validate_build_output(&options, &mut transpiler, &output)?)
    } else {
        None
    };

    if options.dry_run {
        finish_dry_run(&options, &transpiler, &output, validation_summary.as_ref());
        return Ok(());
    }

    let copied_assets = output.discard_staging_on_error(copy_resource_pack_assets(
        &project,
        &assets,
        output.build_dir(),
    ))?;
    publish_output(&options, &project, &output)?;
    if copied_assets > 0 && !options.quiet {
        println!("✓ Copied {} resource-pack asset(s)", copied_assets);
    }

    // Create zip if requested
    let zip_path = if options.zip {
        Some(write_zip(&options, &project, &assets)?)
    } else {
        None
    };

    if !options.quiet {
        print_build_summary(
            &transpiler,
            transpiler.compiled_file_count(),
            validation_summary.as_ref(),
            zip_path.as_deref(),
            false,
        );
    }

    Ok(())
}

fn ensure_compatible_flags(options: &BuildOptions) -> Result<(), String> {
    if options.quiet && options.verbose {
        return Err("--quiet cannot be combined with --verbose".to_string());
    }
    if options.dry_run && options.zip {
        return Err(
            "--dry-run cannot be combined with --zip because no final output is written"
                .to_string(),
        );
    }
    Ok(())
}

struct BuildProject {
    source_path: PathBuf,
    output_dir: PathBuf,
    namespace: String,
    description: String,
    pack_format: PackFormat,
    experimental_resource_pack: bool,
    stdlib_version: u8,
    configured_entry_points: Vec<String>,
    files_to_compile: Vec<PathBuf>,
    source_display_root: PathBuf,
    project_root: PathBuf,
    project_root_marker: String,
    project_id: String,
}

fn resolve_project(options: &BuildOptions) -> Result<BuildProject, String> {
    let (config, config_dir) = load_project_config(options)?;

    // Determine source and output paths
    let source_path = if let Some(ref input) = options.input {
        input.clone()
    } else if let Some(ref cfg) = config {
        config_dir.join(&cfg.build.source)
    } else {
        return Err("No input specified and no cobble.toml found".to_string());
    };

    let output_dir = if let Some(ref output) = options.output {
        output.clone()
    } else if let Some(ref cfg) = config {
        config_dir.join(&cfg.build.output)
    } else {
        PathBuf::from("output")
    };

    if options.zip && is_in_datapacks_folder(&output_dir) {
        return Err(format!(
            "--zip cannot write into a world's datapacks folder ({}).\n\
            The ZIP is written next to the pack folder, and Minecraft would load both as \
            separate packs, running every function twice. Build without --zip there, or build \
            the ZIP elsewhere and copy only the .zip file into datapacks.",
            output_dir.display()
        ));
    }

    // Get namespace from options, config, or use default
    let namespace = options
        .namespace
        .clone()
        .or_else(|| config.as_ref().map(|c| c.project.namespace.clone()))
        .unwrap_or_else(|| "cobble".to_string());

    // Security: Validate namespace before it is used in data pack paths or zip names.
    validate_namespace(&namespace)?;

    let experimental_resource_pack = options.experimental_resource_pack
        || config
            .as_ref()
            .map(|cfg| cfg.experimental.resource_pack)
            .unwrap_or(false);

    let description = options
        .description
        .clone()
        .or_else(|| config.as_ref().map(|c| c.project.description.clone()))
        .unwrap_or_else(|| "Generated by Cobble".to_string());

    let pack_format = resolve_pack_format(options, config.as_ref())?;

    let configured_entry_points = if options.input.is_none() {
        config
            .as_ref()
            .map(|cfg| cfg.build.entry_points.clone())
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    let files_to_compile =
        collect_files_to_compile(options, &source_path, &configured_entry_points)?;

    let source_display_root = source_display_root(&source_path, &config_dir);
    let project_root = config
        .as_ref()
        .map(|_| config_dir.clone())
        .unwrap_or_else(|| source_display_root.clone());
    let (project_root_marker, project_id) = project_marker_identity(&project_root);

    Ok(BuildProject {
        source_path,
        output_dir,
        namespace,
        description,
        pack_format,
        experimental_resource_pack,
        stdlib_version: config.as_ref().map(|cfg| cfg.stdlib.version).unwrap_or(2),
        configured_entry_points,
        files_to_compile,
        source_display_root,
        project_root,
        project_root_marker,
        project_id,
    })
}

fn load_project_config(options: &BuildOptions) -> Result<(Option<CobbleConfig>, PathBuf), String> {
    // Try to find cobble.toml
    if let Some(config_path) = find_config(options.input.as_deref()) {
        let config = if options.pack_format.is_some() {
            CobbleConfig::load_unvalidated(&config_path)?
        } else {
            CobbleConfig::load(&config_path)?
        };
        let config_dir = config_path.parent().unwrap().to_path_buf();
        Ok((Some(config), config_dir))
    } else {
        Ok((
            None,
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        ))
    }
}

fn resolve_pack_format(
    options: &BuildOptions,
    config: Option<&CobbleConfig>,
) -> Result<PackFormat, String> {
    let pack_format = if let Some(ref pack_fmt_str) = options.pack_format {
        PackFormat::parse_format(pack_fmt_str)?
    } else if let Some(cfg) = config {
        PackFormat::parse_format(&cfg.project.pack_format)?
    } else {
        SUPPORTED_PACK_FORMAT
    };

    // Validate pack_format against this release's single supported Minecraft target.
    if !pack_format.is_supported() {
        return Err(format!(
            "pack_format must be {} (Minecraft Java Edition {}), got {}.\n\
            Cobble v{} exclusively supports Minecraft Java Edition {}.\n\
            See https://minecraft.wiki/w/Pack_format for version compatibility.",
            SUPPORTED_PACK_FORMAT,
            SUPPORTED_MINECRAFT_VERSION,
            pack_format,
            COBBLE_VERSION,
            SUPPORTED_MINECRAFT_VERSION
        ));
    }
    // Write the canonical spelling (`121` and `121.0` are the same format).
    Ok(SUPPORTED_PACK_FORMAT)
}

fn collect_files_to_compile(
    options: &BuildOptions,
    source_path: &Path,
    configured_entry_points: &[String],
) -> Result<Vec<PathBuf>, String> {
    // Check if source is a file or directory
    let files_to_compile = if source_path.is_file() {
        vec![source_path.to_path_buf()]
    } else if source_path.is_dir() {
        if options.input.is_none() {
            if !configured_entry_points.is_empty() {
                resolve_entry_points(source_path, configured_entry_points)?
            } else {
                find_cobble_files(source_path)?
            }
        } else {
            find_cobble_files(source_path)?
        }
    } else {
        return Err(format!("Source path does not exist: {:?}", source_path));
    };

    if files_to_compile.is_empty() {
        return Err("No Cobble files found to compile".to_string());
    }
    Ok(files_to_compile)
}

fn check_output_location(options: &BuildOptions, project: &BuildProject) -> Result<(), String> {
    if !options.dry_run || options.validate {
        ensure_build_output_path_safe(&project.output_dir)?;
    }
    if !options.dry_run && !options.validate {
        ensure_plain_build_output_dir_owned(
            &project.output_dir,
            &project.namespace,
            &project.project_id,
            &project.project_root,
        )?;
    }
    Ok(())
}

fn print_build_start(options: &BuildOptions, project: &BuildProject) {
    if options.verbose {
        println!("Building {} file(s)...", project.files_to_compile.len());
        println!("Namespace: {}", project.namespace);
        println!("Pack format: {}", project.pack_format);
        println!("Description: {}", project.description);
    } else if !options.quiet {
        println!("Building {} file(s)...", project.files_to_compile.len());
    }
    if options.dry_run && !options.quiet {
        println!("Dry run: final output will not be written");
    }
}

/// `--validate` builds into a sibling staging directory and publishes it over
/// the final output only after validation passes.
struct OutputDirs {
    final_dir: PathBuf,
    staging_dir: Option<PathBuf>,
}

impl OutputDirs {
    fn new(final_dir: &Path, staged: bool) -> Result<Self, String> {
        let staging_dir = if staged {
            Some(staging_output_dir(final_dir)?)
        } else {
            None
        };
        Ok(Self {
            final_dir: final_dir.to_path_buf(),
            staging_dir,
        })
    }

    fn build_dir(&self) -> &Path {
        self.staging_dir.as_deref().unwrap_or(&self.final_dir)
    }

    fn discard_staging(&self) {
        if let Some(staging_dir) = &self.staging_dir {
            let _ = fs::remove_dir_all(staging_dir);
        }
    }

    fn discard_staging_on_error<T>(&self, result: Result<T, String>) -> Result<T, String> {
        if result.is_err() {
            self.discard_staging();
        }
        result
    }
}

fn compile_project(
    options: &BuildOptions,
    project: &BuildProject,
    build_output_dir: &Path,
) -> Result<Transpiler, String> {
    let source_display_root = &project.source_display_root;

    let mut transpiler = Transpiler::with_options(TranspilerOptions {
        description: Some(project.description.clone()),
        pack_format: project.pack_format,
        stdlib_version: Some(project.stdlib_version),
        experimental_resource_pack: project.experimental_resource_pack,
        source_display_root: Some(source_display_root.clone()),
        project_identity: Some((
            project.project_root_marker.clone(),
            project.project_id.clone(),
        )),
        build_input: Some(BuildManifestInput {
            source: path_display_relative(&project.source_path, source_display_root),
            entry_points: project.configured_entry_points.clone(),
            compiled_files: project
                .files_to_compile
                .iter()
                .map(|path| path_display_relative(path, source_display_root))
                .collect(),
        }),
        ..TranspilerOptions::new(project.namespace.clone(), build_output_dir.to_path_buf())
    });

    let parsed_files = parse_source_files(&project.files_to_compile).map_err(|diagnostics| {
        format!(
            "Language diagnostics failed:\n{}",
            format_file_diagnostics(&diagnostics)
        )
    })?;

    // A file can write the lists and maps of a file compiled before it.
    for parsed in &parsed_files {
        transpiler.record_container_writes(&parsed.program);
        transpiler.record_module_variables(&parsed.path, &parsed.program);
    }

    // Compile all files
    for (file_path, parsed) in project.files_to_compile.iter().zip(parsed_files.iter()) {
        // A file that an earlier file imports was compiled with it.
        if transpiler.has_compiled(file_path) {
            continue;
        }
        if !options.quiet {
            println!(
                "  • Compiling: {:?}",
                file_path.file_name().unwrap_or_default()
            );
        }

        // Set current file for import resolution and source tracking
        transpiler.set_current_file_with_source_spans(file_path, &parsed.source, &parsed.spans);

        let result = transpiler.transpile(&parsed.program);
        print_warnings(
            &transpiler.take_warnings(),
            file_path,
            parsed,
            project,
            &parsed_files,
        );
        result.map_err(|e| match &e.location {
            Some(location) => format!(
                "Transpilation failed at {}:{}:{}: {}",
                path_display_relative(&location.file, source_display_root),
                location.line,
                location.column,
                e
            ),
            None => format!("Transpilation failed for {:?}: {}", file_path, e),
        })?;
    }

    // The resources a declaration names may be declared in a later file.
    transpiler
        .check_resource_references()
        .map_err(|e| match &e.location {
            Some(location) => format!(
                "Transpilation failed at {}:{}:{}: {}",
                path_display_relative(&location.file, source_display_root),
                location.line,
                location.column,
                e
            ),
            None => format!("Transpilation failed: {}", e),
        })?;

    Ok(transpiler)
}

/// Prints the warnings that compiling `file_path` found. A warning can be
/// about a file that `file_path` imports.
fn print_warnings(
    warnings: &[TranspileWarning],
    file_path: &Path,
    parsed: &ParsedSourceFile,
    project: &BuildProject,
    parsed_files: &[ParsedSourceFile],
) {
    if warnings.is_empty() {
        return;
    }
    let root = &project.source_display_root;
    let report_file = |file: &Path| {
        let source = project
            .files_to_compile
            .iter()
            .zip(parsed_files)
            .find(|(path, _)| path.canonicalize().ok().as_deref() == Some(file))
            .map(|(_, parsed)| parsed.source.clone())
            .or_else(|| crate::diagnostics::read_source_file(file).ok())?;
        Some((PathBuf::from(path_display_relative(file, root)), source))
    };
    let current = path_display_relative(file_path, root);
    let diagnostics =
        warning_diagnostics(warnings, Path::new(&current), &parsed.source, report_file);
    eprintln!("{}", format_file_diagnostics(&diagnostics));
}

struct ResourcePackAssets {
    asset_passthrough_files: Vec<AssetPassthroughFile>,
    current_static_asset_paths: HashSet<PathBuf>,
    previous_static_asset_paths: HashSet<PathBuf>,
    generated_asset_paths: HashSet<PathBuf>,
    clean_static_assets: bool,
}

fn prepare_resource_pack_assets(
    transpiler: &mut Transpiler,
    project: &BuildProject,
    asset_passthrough_files: Vec<AssetPassthroughFile>,
) -> Result<ResourcePackAssets, String> {
    let current_static_asset_paths = resource_pack_static_asset_paths(&asset_passthrough_files);
    transpiler
        .data_pack
        .remove_item_definitions_replaced_by(&current_static_asset_paths);
    let generated_asset_paths = generated_resource_pack_asset_paths(transpiler);
    let previous_static_asset_paths =
        read_resource_pack_static_asset_manifest(&project.output_dir)?;
    let output_assets_overlap_project_assets =
        output_assets_tree_overlaps_project_assets(&project.output_dir, &project.project_root)?;
    let clean_static_assets =
        project.experimental_resource_pack || !output_assets_overlap_project_assets;
    transpiler
        .data_pack
        .set_clean_previous_resource_pack_outputs(clean_static_assets);
    if project.experimental_resource_pack {
        ensure_no_resource_pack_asset_passthrough_conflicts(
            &asset_passthrough_files,
            &generated_asset_paths,
        )?;
        ensure_no_resource_pack_asset_passthrough_output_cleaning_overlap(
            &asset_passthrough_files,
            &project.output_dir,
            &generated_asset_paths,
        )?;
        transpiler
            .data_pack
            .set_resource_pack_static_assets(resource_pack_asset_keys(&current_static_asset_paths));
    }

    Ok(ResourcePackAssets {
        asset_passthrough_files,
        current_static_asset_paths,
        previous_static_asset_paths,
        generated_asset_paths,
        clean_static_assets,
    })
}

fn validate_build_output(
    options: &BuildOptions,
    transpiler: &mut Transpiler,
    output: &OutputDirs,
) -> Result<BuildManifestValidation, String> {
    if !options.quiet {
        println!("Validating generated commands...");
    }
    let commands_json = resolve_commands_json(&options.commands_json);
    let report =
        output.discard_staging_on_error(run_validation(output.build_dir(), &commands_json))?;
    let has_validation_errors = !report.errors.is_empty() || !report.source_map_errors.is_empty();
    if !options.quiet || has_validation_errors {
        print_validation_report(
            &report,
            &commands_json,
            output.build_dir(),
            &output.final_dir,
        );
    }
    if has_validation_errors {
        output.discard_staging();
        return Err(format!(
            "{} validation error(s) found",
            report.errors.len() + report.source_map_errors.len()
        ));
    }

    let validation_summary = validation_summary_from_report(&commands_json, &report);
    transpiler
        .data_pack
        .set_validation_summary(Some(validation_summary.clone()));
    if !options.dry_run {
        output.discard_staging_on_error(
            transpiler
                .write_data_pack()
                .map_err(|e| format!("Failed to write validation metadata: {}", e)),
        )?;
    }
    Ok(validation_summary)
}

fn finish_dry_run(
    options: &BuildOptions,
    transpiler: &Transpiler,
    output: &OutputDirs,
    validation_summary: Option<&BuildManifestValidation>,
) {
    output.discard_staging();
    if options.quiet {
        return;
    }
    println!("✓ Dry run completed; no output written");
    if options.validate {
        println!("✓ All commands valid");
    }
    print_build_summary(
        transpiler,
        transpiler.compiled_file_count(),
        validation_summary,
        None,
        true,
    );
}

fn copy_resource_pack_assets(
    project: &BuildProject,
    assets: &ResourcePackAssets,
    asset_output_dir: &Path,
) -> Result<usize, String> {
    if assets.clean_static_assets {
        clean_stale_resource_pack_asset_passthrough(
            &assets.previous_static_asset_paths,
            &assets.current_static_asset_paths,
            &assets.generated_asset_paths,
            asset_output_dir,
        )?;
    }
    if !project.experimental_resource_pack {
        return Ok(0);
    }

    let copied = copy_resource_pack_asset_passthrough(
        &assets.asset_passthrough_files,
        asset_output_dir,
        &assets.generated_asset_paths,
    )?;
    if !assets.current_static_asset_paths.is_empty() {
        write_resource_pack_static_asset_manifest(
            asset_output_dir,
            &assets.current_static_asset_paths,
        )?;
    }
    Ok(copied)
}

fn publish_output(
    options: &BuildOptions,
    project: &BuildProject,
    output: &OutputDirs,
) -> Result<(), String> {
    if let Some(staging_dir) = &output.staging_dir {
        output.discard_staging_on_error(publish_validated_output(
            staging_dir,
            &output.final_dir,
            &project.namespace,
            &project.project_id,
            &project.project_root,
        ))?;
    }
    if !options.quiet {
        println!("✓ Data pack generated at {:?}", output.final_dir);
        if options.validate {
            println!("✓ All commands valid");
        }
    }
    Ok(())
}

fn write_zip(
    options: &BuildOptions,
    project: &BuildProject,
    assets: &ResourcePackAssets,
) -> Result<PathBuf, String> {
    let zip_asset_paths = if project.experimental_resource_pack {
        Some(
            assets
                .generated_asset_paths
                .union(&assets.current_static_asset_paths)
                .cloned()
                .collect::<HashSet<_>>(),
        )
    } else {
        None
    };
    let zip_path = create_zip(
        &project.output_dir,
        &project.namespace,
        zip_asset_paths.as_ref(),
    )?;
    if !options.quiet {
        println!("✓ Created {}", zip_path.display());
    }
    Ok(zip_path)
}

fn path_display(path: &Path) -> String {
    path.display().to_string()
}

fn path_display_relative(path: &Path, root: &Path) -> String {
    let canonical_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());

    if let Ok(relative_path) = canonical_path.strip_prefix(&canonical_root) {
        if relative_path.as_os_str().is_empty() {
            return ".".to_string();
        }
        return path_display(relative_path);
    }

    if path.is_relative() {
        return path_display(path);
    }

    path_display(&canonical_path)
}

fn source_display_root(source_path: &Path, config_dir: &Path) -> PathBuf {
    let source_path = source_path
        .canonicalize()
        .unwrap_or_else(|_| source_path.to_path_buf());
    let config_dir = config_dir
        .canonicalize()
        .unwrap_or_else(|_| config_dir.to_path_buf());

    if source_path.strip_prefix(&config_dir).is_ok() {
        return config_dir;
    }

    if source_path.is_file() {
        return source_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
    }

    source_path
}

fn validation_summary_from_report(
    commands_json: &Path,
    report: &ValidationReport,
) -> BuildManifestValidation {
    BuildManifestValidation {
        enabled: true,
        commands_json: path_display(commands_json),
        files_checked: report.files_checked,
        commands_checked: report.commands_checked,
        macro_commands_checked: report.macro_commands_checked,
        commands_skipped: report.commands_skipped,
        errors: report.errors.len(),
        source_map_errors: report.source_map_errors.len(),
    }
}

fn print_build_summary(
    transpiler: &Transpiler,
    source_files: usize,
    validation: Option<&BuildManifestValidation>,
    zip_path: Option<&Path>,
    dry_run: bool,
) {
    let generated = transpiler.data_pack.generated_counts();
    println!("Build summary:");
    println!("  Source files: {}", source_files);
    println!("  Functions: {}", generated.functions);
    println!("  Commands: {}", generated.commands);
    println!("  Function tags: {}", generated.function_tags);
    println!("  JSON resources: {}", generated.total_json_resources);
    if generated.resource_pack_models > 0 {
        println!("  Resource-pack models: {}", generated.resource_pack_models);
    }
    if generated.resource_pack_item_definitions > 0 {
        println!(
            "  Resource-pack item definitions: {}",
            generated.resource_pack_item_definitions
        );
    }
    if generated.resource_pack_langs > 0 {
        println!(
            "  Resource-pack lang files: {}",
            generated.resource_pack_langs
        );
    }
    if let Some(validation) = validation {
        println!(
            "  Validation: {} commands in {} files ({} macro checked, {} skipped)",
            validation.commands_checked,
            validation.files_checked,
            validation.macro_commands_checked,
            validation.commands_skipped
        );
    }
    if let Some(zip_path) = zip_path {
        println!("  ZIP: {}", zip_path.display());
    }
    if dry_run {
        println!("  Output: not written (--dry-run)");
    }
}

/// Validate that namespace is a Minecraft resource namespace and a single safe
/// filesystem path segment.
fn validate_namespace(namespace: &str) -> Result<(), String> {
    if namespace.is_empty() {
        return Err("Namespace cannot be empty".to_string());
    }
    if namespace == "." || namespace == ".." {
        return Err(format!(
            "Invalid namespace '{}': Namespace cannot be '.' or '..'",
            namespace
        ));
    }
    if namespace.len() > 64 {
        return Err(format!(
            "Namespace too long: {} chars (max 64)",
            namespace.len()
        ));
    }
    if !namespace
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-' || c == '.')
    {
        return Err(format!(
            "Invalid namespace '{}': Can only contain lowercase letters, digits, underscores, hyphens, and dots.\n\
            Example: 'my_datapack', 'cool-pack.v2'",
            namespace
        ));
    }
    Ok(())
}

fn ensure_build_output_path_safe(output_dir: &Path) -> Result<(), String> {
    ensure_not_symlink(output_dir, "build")?;
    if output_dir.exists() {
        let metadata = fs::symlink_metadata(output_dir)
            .map_err(|e| format!("Failed to inspect build output {:?}: {}", output_dir, e))?;
        if !metadata.is_dir() {
            return Err(format!(
                "Refusing to build data pack over non-directory output path: {}",
                output_dir.display()
            ));
        }
    }
    ensure_no_symlink_descendants(output_dir, "build")
}

/// Plain (non-staged) builds write and clean stale entries directly inside the
/// final output directory, so require that directory to be new, truly empty,
/// or already owned by this project before anything inside it is replaced.
fn ensure_plain_build_output_dir_owned(
    output_dir: &Path,
    namespace: &str,
    project_id: &str,
    project_root: &Path,
) -> Result<(), String> {
    let mut entries = match fs::read_dir(output_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(format!(
                "Failed to inspect build output {:?}: {}",
                output_dir, error
            ));
        }
    };
    if entries.next().is_none() {
        return Ok(());
    }

    let manifest_path = build_manifest_path(output_dir);
    let manifest = read_build_manifest(&manifest_path).map_err(|error| {
        format!(
            "Refusing to build into existing non-empty output directory {} that is not Cobble-owned: {}.\n\
            Pick a new or empty output directory, or use `cobble build --validate` for staged publishing into a Cobble-owned directory.",
            output_dir.display(),
            error
        )
    })?;
    require_output_ownership(
        &manifest,
        output_dir,
        Some(project_root),
        Some(namespace),
        Some(project_id),
    )
    .map_err(|error| {
        format!(
            "Refusing to build into output directory owned by another Cobble project at {}: {}",
            output_dir.display(),
            error
        )
    })
}

fn staging_output_dir(output_dir: &Path) -> Result<PathBuf, String> {
    let parent = output_dir.parent().unwrap_or_else(|| Path::new("."));
    let name = output_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("output");
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| format!("System clock error while creating staging output: {}", e))?
        .as_nanos();
    let staging = parent.join(format!(
        ".{}.cobble-staging-{}-{}",
        name,
        std::process::id(),
        stamp
    ));
    if staging.exists() {
        fs::remove_dir_all(&staging)
            .map_err(|e| format!("Failed to clean staging output {:?}: {}", staging, e))?;
    }
    Ok(staging)
}

fn publish_validated_output(
    staging_dir: &Path,
    output_dir: &Path,
    namespace: &str,
    project_id: &str,
    project_root: &Path,
) -> Result<(), String> {
    if output_dir.exists() {
        ensure_validated_output_has_replaceable_name(output_dir)?;
        ensure_output_dir_does_not_contain_project_root(output_dir, project_root)?;
        ensure_replace_output_dir_safe(output_dir, namespace, project_id, project_root)?;
        ensure_no_symlink_descendants(output_dir, "publish validated output")?;

        let publish_dir = replacement_publish_dir(output_dir)?;
        if let Err(error) = prepare_validated_publish_dir(output_dir, staging_dir, &publish_dir) {
            let _ = fs::remove_dir_all(&publish_dir);
            return Err(error);
        }

        let backup_dir = replacement_backup_dir(output_dir)?;
        fs::rename(output_dir, &backup_dir).map_err(|e| {
            format!(
                "Failed to move existing output {:?} to backup {:?}: {}",
                output_dir, backup_dir, e
            )
        })?;

        if let Err(error) = fs::rename(&publish_dir, output_dir) {
            let _ = fs::rename(&backup_dir, output_dir);
            let _ = fs::remove_dir_all(&publish_dir);
            return Err(format!(
                "Failed to move validated data pack from {:?} to {:?}: {}",
                publish_dir, output_dir, error
            ));
        }

        // The validated output has already been swapped into place, so cleanup
        // failures must not turn the successful publish into a build error.
        if let Err(error) = fs::remove_dir_all(&backup_dir) {
            eprintln!(
                "Warning: build succeeded, but the previous output backup {:?} could not be removed: {}. Remove it manually.",
                backup_dir, error
            );
        }
        if let Err(error) = fs::remove_dir_all(staging_dir) {
            eprintln!(
                "Warning: build succeeded, but the staging output {:?} could not be removed: {}. Remove it manually.",
                staging_dir, error
            );
        }
        return Ok(());
    }

    fs::rename(staging_dir, output_dir).map_err(|e| {
        format!(
            "Failed to move validated data pack from {:?} to {:?}: {}",
            staging_dir, output_dir, e
        )
    })
}

fn ensure_validated_output_has_replaceable_name(output_dir: &Path) -> Result<(), String> {
    if output_dir.file_name().is_some() {
        return Ok(());
    }

    Err(format!(
        "Refusing to replace validated output directory {} because it is not a named child path; use a separate build output directory",
        output_dir.display()
    ))
}

fn prepare_validated_publish_dir(
    output_dir: &Path,
    staging_dir: &Path,
    publish_dir: &Path,
) -> Result<(), String> {
    copy_dir_contents(output_dir, publish_dir)?;
    clean_publish_generated_paths(publish_dir, staging_dir)?;
    copy_dir_contents(staging_dir, publish_dir)
}

fn copy_dir_contents(source_dir: &Path, target_dir: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(source_dir).map_err(|error| {
        format!(
            "Failed to inspect directory {} while preparing validated output: {}",
            source_dir.display(),
            error
        )
    })?;
    if !metadata.is_dir() {
        return Err(format!(
            "Expected directory while preparing validated output: {}",
            source_dir.display()
        ));
    }

    for entry in WalkDir::new(source_dir).follow_links(false).min_depth(1) {
        let entry = entry.map_err(|error| {
            format!(
                "Failed to inspect {} while preparing validated output: {}",
                source_dir.display(),
                error
            )
        })?;
        let path = entry.path();
        if entry.file_type().is_symlink() {
            return Err(format!(
                "Refusing to publish validated output through symlink: {}",
                path.display()
            ));
        }
        let relative_path = path.strip_prefix(source_dir).map_err(|error| {
            format!(
                "Failed to calculate validated output path for {}: {}",
                path.display(),
                error
            )
        })?;
        let target_path = target_dir.join(relative_path);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target_path).map_err(|error| {
                format!(
                    "Failed to create validated output directory {}: {}",
                    target_path.display(),
                    error
                )
            })?;
        } else if entry.file_type().is_file() {
            if let Some(parent) = target_path.parent() {
                fs::create_dir_all(parent).map_err(|error| {
                    format!(
                        "Failed to create validated output directory {}: {}",
                        parent.display(),
                        error
                    )
                })?;
            }
            if target_path.is_dir() {
                fs::remove_dir_all(&target_path).map_err(|error| {
                    format!(
                        "Failed to replace validated output directory {}: {}",
                        target_path.display(),
                        error
                    )
                })?;
            }
            copy_file_atomic(path, &target_path).map_err(|error| {
                format!(
                    "Failed to copy validated output {} to {}: {}",
                    path.display(),
                    target_path.display(),
                    error
                )
            })?;
        }
    }
    Ok(())
}

fn clean_publish_generated_paths(publish_dir: &Path, staging_dir: &Path) -> Result<(), String> {
    let previous_namespaces = read_generated_namespaces_from_output(publish_dir)?;
    let current_namespaces = read_generated_namespaces_from_output(staging_dir)?;
    let previous_asset_namespaces = read_generated_asset_namespaces_from_output(publish_dir)?;
    let current_asset_namespaces = read_generated_asset_namespaces_from_output(staging_dir)?;
    let previous_static_assets = read_resource_pack_static_asset_manifest(publish_dir)?;
    let current_static_assets = read_resource_pack_static_asset_manifest(staging_dir)?;

    remove_path_if_exists(&publish_dir.join("pack.mcmeta"))?;
    remove_path_if_exists(&publish_dir.join(".cobble"))?;

    let data_dir = publish_dir.join("data");
    for namespace in previous_namespaces.union(&current_namespaces) {
        if !is_safe_namespace_path(namespace) {
            continue;
        }
        let namespace_dir = data_dir.join(namespace);
        if current_namespaces.contains(namespace) {
            clean_generated_data_dirs(&namespace_dir)?;
        } else {
            remove_path_if_exists(&namespace_dir)?;
        }
    }

    let assets_dir = publish_dir.join("assets");
    for namespace in previous_asset_namespaces.union(&current_asset_namespaces) {
        if !is_safe_namespace_path(namespace) {
            continue;
        }
        clean_generated_asset_dirs(&assets_dir.join(namespace))?;
    }

    clean_stale_resource_pack_asset_passthrough(
        &previous_static_assets,
        &current_static_assets,
        &HashSet::new(),
        publish_dir,
    )?;

    Ok(())
}

fn clean_generated_data_dirs(namespace_dir: &Path) -> Result<(), String> {
    for resource_dir in [
        "function",
        "functions",
        "tags",
        "advancement",
        "advancements",
        "loot_table",
        "loot_tables",
        "recipe",
        "recipes",
        "predicate",
        "predicates",
        "item_modifier",
        "item_modifiers",
        "dialog",
    ] {
        remove_path_if_exists(&namespace_dir.join(resource_dir))?;
    }
    Ok(())
}

fn clean_generated_asset_dirs(namespace_dir: &Path) -> Result<(), String> {
    for asset_dir in ["models", "items", "lang"] {
        remove_path_if_exists(&namespace_dir.join(asset_dir))?;
    }
    Ok(())
}

fn read_generated_namespaces_from_output(output_dir: &Path) -> Result<HashSet<String>, String> {
    read_string_set_json(output_dir.join(".cobble/generated_namespaces.json"))
}

fn read_generated_asset_namespaces_from_output(
    output_dir: &Path,
) -> Result<HashSet<String>, String> {
    read_string_set_json(output_dir.join(".cobble/generated_asset_namespaces.json"))
}

fn read_string_set_json(path: PathBuf) -> Result<HashSet<String>, String> {
    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(HashSet::new()),
        Err(error) => {
            return Err(format!("Failed to read {}: {}", path.display(), error));
        }
    };
    let values = serde_json::from_str::<Vec<String>>(&content)
        .map_err(|error| format!("Failed to parse {}: {}", path.display(), error))?;
    Ok(values.into_iter().collect())
}

fn is_safe_namespace_path(namespace: &str) -> bool {
    validate_namespace(namespace).is_ok()
}

fn remove_path_if_exists(path: &Path) -> Result<(), String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(format!("Failed to inspect {}: {}", path.display(), error));
        }
    };
    if metadata.file_type().is_symlink() {
        return Err(format!(
            "Refusing to remove generated output through symlink: {}",
            path.display()
        ));
    }
    if metadata.is_dir() {
        fs::remove_dir_all(path)
            .map_err(|error| format!("Failed to remove {}: {}", path.display(), error))
    } else {
        fs::remove_file(path)
            .map_err(|error| format!("Failed to remove {}: {}", path.display(), error))
    }
}

fn replacement_backup_dir(output_dir: &Path) -> Result<PathBuf, String> {
    let parent = output_dir.parent().unwrap_or_else(|| Path::new("."));
    let name = output_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("output");
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| format!("System clock error while creating output backup: {}", e))?
        .as_nanos();
    let backup_dir = parent.join(format!(
        ".{}.cobble-backup-{}-{}",
        name,
        std::process::id(),
        stamp
    ));
    if backup_dir.exists() {
        fs::remove_dir_all(&backup_dir)
            .map_err(|e| format!("Failed to clean output backup {:?}: {}", backup_dir, e))?;
    }
    Ok(backup_dir)
}

fn replacement_publish_dir(output_dir: &Path) -> Result<PathBuf, String> {
    let parent = output_dir.parent().unwrap_or_else(|| Path::new("."));
    let name = output_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("output");
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| format!("System clock error while creating publish output: {}", e))?
        .as_nanos();
    let publish_dir = parent.join(format!(
        ".{}.cobble-publish-{}-{}",
        name,
        std::process::id(),
        stamp
    ));
    if publish_dir.exists() {
        fs::remove_dir_all(&publish_dir)
            .map_err(|e| format!("Failed to clean publish output {:?}: {}", publish_dir, e))?;
    }
    Ok(publish_dir)
}

fn ensure_output_dir_does_not_contain_project_root(
    output_dir: &Path,
    project_root: &Path,
) -> Result<(), String> {
    let output = output_dir.canonicalize().map_err(|error| {
        format!(
            "Failed to resolve existing output directory {}: {}",
            output_dir.display(),
            error
        )
    })?;
    let project = project_root.canonicalize().map_err(|error| {
        format!(
            "Failed to resolve project root {}: {}",
            project_root.display(),
            error
        )
    })?;
    if project.starts_with(&output) {
        return Err(format!(
            "Refusing to replace validated output directory {} because it contains the project root {}; use a separate build output directory",
            output_dir.display(),
            project_root.display()
        ));
    }
    Ok(())
}

fn ensure_replace_output_dir_safe(
    output_dir: &Path,
    namespace: &str,
    project_id: &str,
    project_root: &Path,
) -> Result<(), String> {
    let metadata = fs::symlink_metadata(output_dir)
        .map_err(|e| format!("Failed to inspect existing output {:?}: {}", output_dir, e))?;
    if !metadata.is_dir() {
        return Err(format!(
            "Refusing to replace non-directory output path: {}",
            output_dir.display()
        ));
    }

    let manifest_path = build_manifest_path(output_dir);
    let manifest = read_build_manifest(&manifest_path).map_err(|error| {
        format!(
            "Refusing to replace existing output without Cobble ownership marker at {}: {}",
            manifest_path.display(),
            error
        )
    })?;
    require_output_ownership(
        &manifest,
        output_dir,
        Some(project_root),
        Some(namespace),
        Some(project_id),
    )
    .map_err(|error| {
        format!(
            "Refusing to replace existing output owned by another Cobble project at {}: {}",
            output_dir.display(),
            error
        )
    })
}

#[derive(Debug, Clone)]
struct AssetPassthroughFile {
    source_path: PathBuf,
    relative_path: PathBuf,
}

fn collect_resource_pack_asset_passthrough_files(
    project_root: &Path,
) -> Result<Vec<AssetPassthroughFile>, String> {
    let assets_dir = project_root.join("assets");
    let metadata = match fs::symlink_metadata(&assets_dir) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(format!(
                "Failed to inspect resource-pack assets directory {}: {}",
                assets_dir.display(),
                error
            ));
        }
    };
    ensure_not_symlink(&assets_dir, "copy resource-pack assets")?;
    if !metadata.is_dir() {
        return Err(format!(
            "Refusing to copy resource-pack assets from non-directory path: {}",
            assets_dir.display()
        ));
    }

    let canonical_project_root = project_root.canonicalize().map_err(|error| {
        format!(
            "Failed to resolve project root while copying resource-pack assets {}: {}",
            project_root.display(),
            error
        )
    })?;
    let canonical_assets_dir = assets_dir.canonicalize().map_err(|error| {
        format!(
            "Failed to resolve resource-pack assets directory {}: {}",
            assets_dir.display(),
            error
        )
    })?;
    if !canonical_assets_dir.starts_with(&canonical_project_root) {
        return Err(format!(
            "Refusing to copy resource-pack assets outside the project: {}",
            assets_dir.display()
        ));
    }

    let mut files = Vec::new();
    for entry in WalkDir::new(&assets_dir)
        .follow_links(false)
        .sort_by_file_name()
    {
        let entry = entry.map_err(|error| {
            format!(
                "Failed to inspect resource-pack assets under {}: {}",
                assets_dir.display(),
                error
            )
        })?;
        let path = entry.path();
        if entry.file_type().is_symlink() {
            return Err(format!(
                "Refusing to copy resource-pack assets through symlink: {}",
                path.display()
            ));
        }
        if !entry.file_type().is_file() {
            continue;
        }

        let relative_path = path.strip_prefix(&assets_dir).map_err(|error| {
            format!(
                "Failed to calculate resource-pack asset path for {}: {}",
                path.display(),
                error
            )
        })?;
        validate_asset_passthrough_relative_path(relative_path)?;

        let canonical_file = path.canonicalize().map_err(|error| {
            format!(
                "Failed to resolve resource-pack asset {}: {}",
                path.display(),
                error
            )
        })?;
        if !canonical_file.starts_with(&canonical_assets_dir)
            || !canonical_file.starts_with(&canonical_project_root)
        {
            return Err(format!(
                "Refusing to copy resource-pack asset outside the project: {}",
                path.display()
            ));
        }

        files.push(AssetPassthroughFile {
            source_path: path.to_path_buf(),
            relative_path: relative_path.to_path_buf(),
        });
    }

    Ok(files)
}

fn ensure_no_resource_pack_asset_passthrough_conflicts(
    files: &[AssetPassthroughFile],
    generated_assets: &HashSet<PathBuf>,
) -> Result<(), String> {
    for file in files {
        if generated_assets.contains(&file.relative_path) {
            return Err(resource_pack_asset_conflict_message(&file.relative_path));
        }
    }
    Ok(())
}

fn ensure_no_resource_pack_asset_passthrough_output_cleaning_overlap(
    files: &[AssetPassthroughFile],
    output_dir: &Path,
    generated_assets: &HashSet<PathBuf>,
) -> Result<(), String> {
    if files.is_empty() {
        return Ok(());
    }

    let output_assets_dir = output_dir.join("assets");
    let Ok(canonical_output_assets_dir) = output_assets_dir.canonicalize() else {
        return Ok(());
    };
    let generated_namespaces = generated_resource_pack_asset_namespaces(generated_assets);
    let previous_generated_namespaces = read_generated_resource_pack_asset_namespaces(output_dir)?;

    for file in files {
        let Some((namespace, top_dir)) = asset_namespace_and_top_dir(&file.relative_path) else {
            continue;
        };

        let canonical_source = file.source_path.canonicalize().map_err(|error| {
            format!(
                "Failed to resolve resource-pack asset {}: {}",
                file.source_path.display(),
                error
            )
        })?;
        if !canonical_source.starts_with(&canonical_output_assets_dir) {
            continue;
        }

        if previous_generated_namespaces.contains(&namespace) {
            return Err(format!(
                "Refusing to copy resource-pack assets from output assets tree: {}. Previous generated resource-pack output may clean assets/{}; use a separate build output directory.",
                file.source_path.display(),
                namespace
            ));
        }

        if generated_namespaces.contains(&namespace)
            && matches!(top_dir.as_str(), "models" | "items" | "lang")
        {
            return Err(format!(
                "Refusing to copy resource-pack assets from output assets tree: {}. Generated resource-pack output cleans assets/{}/{}; use a separate build output directory.",
                file.source_path.display(),
                namespace,
                top_dir
            ));
        }
    }

    Ok(())
}

fn resource_pack_static_asset_paths(files: &[AssetPassthroughFile]) -> HashSet<PathBuf> {
    files
        .iter()
        .map(|file| file.relative_path.clone())
        .collect()
}

fn output_assets_tree_overlaps_project_assets(
    output_dir: &Path,
    project_root: &Path,
) -> Result<bool, String> {
    let output_assets_dir = output_dir.join("assets");
    let project_assets_dir = project_root.join("assets");
    let canonical_project_assets = match project_assets_dir.canonicalize() {
        Ok(path) => path,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(format!(
                "Failed to resolve project resource-pack assets directory {}: {}",
                project_assets_dir.display(),
                error
            ));
        }
    };
    let canonical_output_assets = match output_assets_dir.canonicalize() {
        Ok(path) => path,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(format!(
                "Failed to resolve output resource-pack assets directory {}: {}",
                output_assets_dir.display(),
                error
            ));
        }
    };

    Ok(
        canonical_output_assets.starts_with(&canonical_project_assets)
            || canonical_project_assets.starts_with(&canonical_output_assets),
    )
}

fn read_generated_resource_pack_asset_namespaces(
    output_dir: &Path,
) -> Result<HashSet<String>, String> {
    let path = output_dir
        .join(".cobble")
        .join("generated_asset_namespaces.json");
    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(HashSet::new()),
        Err(error) => {
            return Err(format!(
                "Failed to read previous generated resource-pack namespaces {}: {}",
                path.display(),
                error
            ));
        }
    };
    let namespaces = serde_json::from_str::<Vec<String>>(&content).map_err(|error| {
        format!(
            "Failed to parse previous generated resource-pack namespaces {}: {}",
            path.display(),
            error
        )
    })?;
    Ok(namespaces
        .into_iter()
        .filter(|namespace| validate_namespace(namespace).is_ok())
        .collect())
}

fn read_resource_pack_static_asset_manifest(output_dir: &Path) -> Result<HashSet<PathBuf>, String> {
    let path = resource_pack_static_asset_manifest_path(output_dir);
    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(HashSet::new()),
        Err(error) => {
            return Err(format!(
                "Failed to read previous static resource-pack asset manifest {}: {}",
                path.display(),
                error
            ));
        }
    };
    let entries = serde_json::from_str::<Vec<String>>(&content).map_err(|error| {
        format!(
            "Failed to parse previous static resource-pack asset manifest {}: {}",
            path.display(),
            error
        )
    })?;
    let mut paths = HashSet::new();
    for entry in entries {
        let relative_path = PathBuf::from(&entry);
        validate_asset_passthrough_relative_path(&relative_path).map_err(|error| {
            format!(
                "Invalid previous static resource-pack asset manifest entry `{}` in {}: {}",
                entry,
                path.display(),
                error
            )
        })?;
        paths.insert(relative_path);
    }
    Ok(paths)
}

fn write_resource_pack_static_asset_manifest(
    output_dir: &Path,
    current_assets: &HashSet<PathBuf>,
) -> Result<(), String> {
    let path = resource_pack_static_asset_manifest_path(output_dir);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "Failed to create static resource-pack asset manifest directory {}: {}",
                parent.display(),
                error
            )
        })?;
    }
    let entries = resource_pack_asset_keys(current_assets);
    write_file_atomic(
        &path,
        serde_json::to_string_pretty(&entries).map_err(|error| {
            format!(
                "Failed to serialize static resource-pack asset manifest {}: {}",
                path.display(),
                error
            )
        })?,
    )
    .map_err(|error| {
        format!(
            "Failed to write static resource-pack asset manifest {}: {}",
            path.display(),
            error
        )
    })
}

fn resource_pack_static_asset_manifest_path(output_dir: &Path) -> PathBuf {
    output_dir
        .join(".cobble")
        .join("static_asset_passthrough.json")
}

fn clean_stale_resource_pack_asset_passthrough(
    previous_assets: &HashSet<PathBuf>,
    current_assets: &HashSet<PathBuf>,
    generated_assets: &HashSet<PathBuf>,
    output_dir: &Path,
) -> Result<usize, String> {
    let mut removed = 0;
    let output_assets_dir = output_dir.join("assets");

    for relative_path in previous_assets {
        if current_assets.contains(relative_path) || generated_assets.contains(relative_path) {
            continue;
        }

        let target_path = output_assets_dir.join(relative_path);
        ensure_no_symlinks_from(output_dir, &target_path, "clean stale resource-pack assets")?;
        let metadata = match fs::symlink_metadata(&target_path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(format!(
                    "Failed to inspect stale resource-pack asset {}: {}",
                    target_path.display(),
                    error
                ));
            }
        };
        if !metadata.is_file() {
            return Err(format!(
                "Refusing to clean stale resource-pack asset at non-file path: {}",
                target_path.display()
            ));
        }
        fs::remove_file(&target_path).map_err(|error| {
            format!(
                "Failed to remove stale resource-pack asset {}: {}",
                target_path.display(),
                error
            )
        })?;
        removed += 1;
        clean_empty_asset_parent_dirs(&output_assets_dir, &target_path)?;
    }

    Ok(removed)
}

fn clean_empty_asset_parent_dirs(
    output_assets_dir: &Path,
    removed_file: &Path,
) -> Result<(), String> {
    let mut current = removed_file.parent();
    while let Some(dir) = current {
        if dir == output_assets_dir || !dir.starts_with(output_assets_dir) {
            break;
        }
        ensure_no_symlinks_from(output_assets_dir, dir, "clean stale resource-pack assets")?;
        match fs::remove_dir(dir) {
            Ok(()) => current = dir.parent(),
            Err(error) if error.kind() == ErrorKind::NotFound => break,
            Err(error) if error.kind() == ErrorKind::DirectoryNotEmpty => break,
            Err(error) => {
                return Err(format!(
                    "Failed to remove empty stale resource-pack asset directory {}: {}",
                    dir.display(),
                    error
                ));
            }
        }
    }
    Ok(())
}

fn copy_resource_pack_asset_passthrough(
    files: &[AssetPassthroughFile],
    output_dir: &Path,
    generated_assets: &HashSet<PathBuf>,
) -> Result<usize, String> {
    if files.is_empty() {
        return Ok(0);
    }

    let output_assets_dir = output_dir.join("assets");
    let mut copied = 0;
    for file in files {
        if generated_assets.contains(&file.relative_path) {
            return Err(resource_pack_asset_conflict_message(&file.relative_path));
        }

        let target_path = output_assets_dir.join(&file.relative_path);
        ensure_no_symlinks_from(output_dir, &target_path, "copy resource-pack assets")?;
        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                format!(
                    "Failed to create resource-pack asset directory {}: {}",
                    parent.display(),
                    error
                )
            })?;
            ensure_no_symlinks_from(output_dir, parent, "copy resource-pack assets")?;
        }

        if target_path.exists() {
            let source = file.source_path.canonicalize().map_err(|error| {
                format!(
                    "Failed to resolve resource-pack asset {}: {}",
                    file.source_path.display(),
                    error
                )
            })?;
            let target = target_path.canonicalize().map_err(|error| {
                format!(
                    "Failed to resolve resource-pack asset target {}: {}",
                    target_path.display(),
                    error
                )
            })?;
            if source == target {
                continue;
            }
        }

        copy_file_atomic(&file.source_path, &target_path).map_err(|error| {
            format!(
                "Failed to copy resource-pack asset {} to {}: {}",
                file.source_path.display(),
                target_path.display(),
                error
            )
        })?;
        copied += 1;
    }

    Ok(copied)
}

fn resource_pack_asset_conflict_message(relative_path: &Path) -> String {
    format!(
        "Static resource-pack asset assets/{} conflicts with generated resource-pack output",
        path_display(relative_path)
    )
}

fn generated_resource_pack_asset_paths(transpiler: &Transpiler) -> HashSet<PathBuf> {
    transpiler
        .data_pack
        .resource_pack_models
        .keys()
        .chain(transpiler.data_pack.resource_pack_langs.keys())
        .filter_map(|key| {
            let (namespace, relative_path) = key.split_once('/')?;
            Some(PathBuf::from(namespace).join(format!("{}.json", relative_path)))
        })
        .collect()
}

fn resource_pack_asset_keys(paths: &HashSet<PathBuf>) -> Vec<String> {
    let mut keys: Vec<_> = paths
        .iter()
        .map(|path| {
            path.components()
                .filter_map(component_to_string)
                .collect::<Vec<_>>()
                .join("/")
        })
        .collect();
    keys.sort();
    keys
}

fn generated_resource_pack_asset_namespaces(
    generated_assets: &HashSet<PathBuf>,
) -> HashSet<String> {
    generated_assets
        .iter()
        .filter_map(|path| asset_namespace_and_top_dir(path).map(|(namespace, _)| namespace))
        .collect()
}

fn asset_namespace_and_top_dir(path: &Path) -> Option<(String, String)> {
    let mut components = path.components();
    let namespace = component_to_string(components.next()?)?;
    let top_dir = component_to_string(components.next()?)?;
    Some((namespace, top_dir))
}

fn component_to_string(component: Component<'_>) -> Option<String> {
    let Component::Normal(value) = component else {
        return None;
    };
    value.to_str().map(ToString::to_string)
}

fn validate_asset_passthrough_relative_path(path: &Path) -> Result<(), String> {
    if path.is_absolute() {
        return Err(format!(
            "Invalid resource-pack asset path `{}`: assets must use relative paths",
            path.display()
        ));
    }

    let mut segments = Vec::new();
    for component in path.components() {
        let Component::Normal(segment) = component else {
            return Err(format!(
                "Invalid resource-pack asset path `{}`: no '.', '..', root, or prefix components are allowed",
                path.display()
            ));
        };
        let Some(segment) = segment.to_str() else {
            return Err(format!(
                "Invalid resource-pack asset path `{}`: path must be valid UTF-8",
                path.display()
            ));
        };
        if segment.is_empty()
            || segment == "."
            || segment == ".."
            || segment.contains('\\')
            || segment.contains(':')
            || !segment.chars().all(|c| {
                c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-' || c == '.'
            })
        {
            return Err(format!(
                "Invalid resource-pack asset path `{}`: use lowercase asset paths with '/', '_', '-', or '.', and no empty, '.', or '..' segments",
                path.display()
            ));
        }
        segments.push(segment);
    }

    if segments.len() < 2 {
        return Err(format!(
            "Invalid resource-pack asset path `{}`: expected assets/<namespace>/<path>",
            path.display()
        ));
    }
    validate_namespace(segments[0])?;

    Ok(())
}

/// Whether `output_dir` sits directly in a world's `datapacks` folder: a
/// folder named `datapacks` next to the world's `level.dat`.
fn is_in_datapacks_folder(output_dir: &Path) -> bool {
    let Ok(output_dir) = std::path::absolute(output_dir) else {
        return false;
    };
    let Some(datapacks) = output_dir.parent() else {
        return false;
    };
    datapacks
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case("datapacks"))
        && datapacks
            .parent()
            .is_some_and(|world| world.join("level.dat").is_file())
}

fn create_zip(
    output_dir: &Path,
    namespace: &str,
    asset_paths: Option<&HashSet<PathBuf>>,
) -> Result<PathBuf, String> {
    let zip_path = output_dir.with_file_name(format!("{}.zip", namespace));
    ensure_not_symlink(&zip_path, "create zip file")?;
    let (file, temp_zip_path) = create_temp_output_file(&zip_path)
        .map_err(|e| format!("Failed to create temporary zip file: {}", e))?;

    let mut zip = ZipWriter::new(file);
    // Every entry gets the same time, 1980-01-01 00:00, so the same pack
    // gives a byte-identical ZIP.
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .last_modified_time(zip::DateTime::default());

    // Add all files from output directory to zip, in a stable order so the
    // same pack gives the same ZIP.
    let zip_result = (|| {
        for entry in WalkDir::new(output_dir)
            .sort_by_file_name()
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if path.is_file() {
                let relative_path = path
                    .strip_prefix(output_dir)
                    .map_err(|e| format!("Failed to get relative path: {}", e))?;

                // Convert path to use forward slashes for ZIP (required by Minecraft)
                let include_asset = asset_paths
                    .and_then(|paths| {
                        relative_path
                            .strip_prefix("assets")
                            .ok()
                            .map(|asset_path| paths.contains(asset_path))
                    })
                    .unwrap_or(false);
                if relative_path != Path::new("pack.mcmeta")
                    && !relative_path.starts_with("data")
                    && !include_asset
                {
                    continue;
                }

                let zip_path = relative_path.to_string_lossy().replace('\\', "/");
                let file_data =
                    fs::read(path).map_err(|e| format!("Failed to read file for zip: {}", e))?;

                zip.start_file(zip_path, options)
                    .map_err(|e| format!("Failed to add file to zip: {}", e))?;

                zip.write_all(&file_data)
                    .map_err(|e| format!("Failed to write file to zip: {}", e))?;
            }
        }

        zip.finish()
            .map_err(|e| format!("Failed to finalize zip: {}", e))?;
        Ok(())
    })();
    if let Err(error) = zip_result {
        let _ = fs::remove_file(&temp_zip_path);
        return Err(error);
    }
    replace_with_temp_file(&temp_zip_path, &zip_path).map_err(|e| {
        let _ = fs::remove_file(&temp_zip_path);
        format!("Failed to move temporary zip into place: {}", e)
    })?;

    Ok(zip_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::cwd_lock;

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
            std::env::set_current_dir(&self.previous).unwrap();
        }
    }

    fn write_say_commands_json(path: &Path) {
        fs::write(
            path,
            r#"{
                "type": "root",
                "children": {
                    "gamerule": {
                        "type": "literal",
                        "children": {
                            "rest": {
                                "type": "argument",
                                "parser": "minecraft:message",
                                "executable": true
                            }
                        }
                    },
                    "say": {
                        "type": "literal",
                        "children": {
                            "message": {
                                "type": "argument",
                                "parser": "minecraft:message",
                                "executable": true
                            }
                        }
                    },
                    "scoreboard": {
                        "type": "literal",
                        "children": {
                            "rest": {
                                "type": "argument",
                                "parser": "minecraft:message",
                                "executable": true
                            }
                        }
                    }
                }
            }"#,
        )
        .unwrap();
    }

    fn without_return_prologue(content: &str) -> String {
        content
            .lines()
            .filter(|line| !line.starts_with("scoreboard players set #return_flag_"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn resolves_entry_points_relative_to_source_dir() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let source_dir = temp_dir.path().join("src");
        fs::create_dir_all(&source_dir).unwrap();
        fs::write(source_dir.join("main.cbl"), "def main():\n    pass\n").unwrap();
        fs::write(source_dir.join("utils.cbl"), "def helper():\n    pass\n").unwrap();

        let files = resolve_entry_points(&source_dir, &["main.cbl".to_string()]).unwrap();

        assert_eq!(files, vec![source_dir.join("main.cbl")]);
    }

    #[test]
    fn build_validate_reports_missing_command_tree() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let input_file = temp_dir.path().join("test.cbl");
        let output_dir = temp_dir.path().join("output");
        let commands_json = temp_dir.path().join("missing_commands.json");
        fs::write(&input_file, "def test():\n    /say hello\n").unwrap();

        let error = build(BuildOptions {
            input: Some(input_file),
            output: Some(output_dir.clone()),
            namespace: None,
            pack_format: None,
            description: None,
            verbose: false,
            quiet: false,
            zip: false,
            experimental_resource_pack: false,
            validate: true,
            dry_run: false,
            commands_json,
        })
        .unwrap_err();

        assert!(error.contains("Command tree not found"));
        assert!(error.contains("scripts/setup_commands_json.sh 26.3"));
        assert!(!temp_dir
            .path()
            .read_dir()
            .unwrap()
            .filter_map(|entry| entry.ok())
            .any(|entry| entry
                .file_name()
                .to_string_lossy()
                .contains(".output.cobble-staging-")));
    }

    #[test]
    fn build_validate_fails_on_invalid_generated_command() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let input_file = temp_dir.path().join("test.cbl");
        let output_dir = temp_dir.path().join("output");
        let commands_json = temp_dir.path().join("commands.json");
        fs::write(&input_file, "def test():\n    /say hello\n").unwrap();
        fs::write(&commands_json, r#"{"type":"root","children":{}}"#).unwrap();

        let error = build(BuildOptions {
            input: Some(input_file),
            output: Some(output_dir.clone()),
            namespace: None,
            pack_format: None,
            description: None,
            verbose: false,
            quiet: false,
            zip: false,
            experimental_resource_pack: false,
            validate: true,
            dry_run: false,
            commands_json,
        })
        .unwrap_err();

        assert!(error.contains("validation error(s) found"));
        assert!(!output_dir.exists());
    }

    #[test]
    fn build_validate_preserves_previous_output_on_validation_failure() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let input_file = temp_dir.path().join("test.cbl");
        let output_dir = temp_dir.path().join("output");
        let valid_commands_json = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("data")
            .join("commands.json");
        if !valid_commands_json.exists() {
            return;
        }

        fs::write(&input_file, "def test():\n    /say valid\n").unwrap();
        build(BuildOptions {
            input: Some(input_file.clone()),
            output: Some(output_dir.clone()),
            namespace: None,
            pack_format: None,
            description: None,
            verbose: false,
            quiet: false,
            zip: false,
            experimental_resource_pack: false,
            validate: true,
            dry_run: false,
            commands_json: valid_commands_json.clone(),
        })
        .unwrap();

        fs::write(&input_file, "def test():\n    /titel @a actionbar bad\n").unwrap();
        let error = build(BuildOptions {
            input: Some(input_file),
            output: Some(output_dir.clone()),
            namespace: None,
            pack_format: None,
            description: None,
            verbose: false,
            quiet: false,
            zip: false,
            experimental_resource_pack: false,
            validate: true,
            dry_run: false,
            commands_json: valid_commands_json,
        })
        .unwrap_err();

        assert!(error.contains("validation error(s) found"));
        let content =
            fs::read_to_string(output_dir.join("data/cobble/function/test.mcfunction")).unwrap();
        assert_eq!(without_return_prologue(&content).trim(), "say valid");
        assert!(output_dir.join(".cobble/build_manifest.json").is_file());
    }

    #[test]
    fn validated_publish_restores_previous_output_when_staging_move_fails() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let project_dir = temp_dir.path().join("project");
        let output_dir = temp_dir.path().join("output");
        let missing_staging = temp_dir.path().join("missing-staging");
        fs::create_dir_all(&project_dir).unwrap();
        fs::create_dir_all(output_dir.join(".cobble")).unwrap();
        fs::write(output_dir.join("sentinel.txt"), "keep\n").unwrap();

        let namespace = "restore";
        let (_, project_id) = project_marker_identity(&project_dir);
        fs::write(
            output_dir.join(".cobble/build_manifest.json"),
            format!(
                r#"{{"version":1,"cobble_version":"test","namespace":"{namespace}","project_id":"{project_id}"}}"#
            ),
        )
        .unwrap();

        let error = publish_validated_output(
            &missing_staging,
            &output_dir,
            namespace,
            &project_id,
            &project_dir,
        )
        .unwrap_err();

        assert!(error.contains("preparing validated output"));
        assert_eq!(
            fs::read_to_string(output_dir.join("sentinel.txt")).unwrap(),
            "keep\n"
        );
        assert!(!temp_dir
            .path()
            .read_dir()
            .unwrap()
            .filter_map(|entry| entry.ok())
            .any(|entry| entry
                .file_name()
                .to_string_lossy()
                .contains(".output.cobble-backup-")));
    }

    #[test]
    fn validated_publish_refuses_to_replace_project_root() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let project_dir = temp_dir.path().join("project");
        let staging_dir = temp_dir.path().join("staging");
        fs::create_dir_all(project_dir.join(".cobble")).unwrap();
        fs::create_dir_all(&staging_dir).unwrap();
        fs::write(project_dir.join("main.cbl"), "def main():\n    /say keep\n").unwrap();

        let namespace = "project_root";
        let (_, project_id) = project_marker_identity(&project_dir);
        fs::write(
            project_dir.join(".cobble/build_manifest.json"),
            format!(
                r#"{{"version":1,"cobble_version":"test","namespace":"{namespace}","project_id":"{project_id}"}}"#
            ),
        )
        .unwrap();

        let error = publish_validated_output(
            &staging_dir,
            &project_dir,
            namespace,
            &project_id,
            &project_dir,
        )
        .unwrap_err();

        assert!(error.contains("Refusing to replace validated output directory"));
        assert!(project_dir.join("main.cbl").exists());
        assert!(staging_dir.exists());
    }

    #[test]
    fn validated_publish_refuses_current_directory_without_publish_artifact() {
        let _lock = cwd_lock().lock().unwrap();
        let temp_dir = tempfile::TempDir::new().unwrap();
        let output_dir = temp_dir.path().join("output");
        let project_dir = temp_dir.path().join("project");
        let staging_dir = temp_dir.path().join("staging");
        fs::create_dir_all(output_dir.join(".cobble")).unwrap();
        fs::create_dir_all(&project_dir).unwrap();
        fs::create_dir_all(&staging_dir).unwrap();

        let namespace = "current_dir";
        let (_, project_id) = project_marker_identity(&project_dir);
        fs::write(
            output_dir.join(".cobble/build_manifest.json"),
            format!(
                r#"{{"version":1,"cobble_version":"test","namespace":"{namespace}","project_id":"{project_id}"}}"#
            ),
        )
        .unwrap();

        let _guard = CurrentDirGuard::push(&output_dir);
        let error = publish_validated_output(
            Path::new("../staging"),
            Path::new("."),
            namespace,
            &project_id,
            &project_dir,
        )
        .unwrap_err();

        assert!(error.contains("Refusing to replace validated output directory ."));
        assert!(staging_dir.exists());
        assert!(!output_dir
            .read_dir()
            .unwrap()
            .filter_map(|entry| entry.ok())
            .any(|entry| entry
                .file_name()
                .to_string_lossy()
                .contains(".output.cobble-publish-")));
    }

    #[test]
    fn build_rejects_existing_non_directory_output_path() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let input_file = temp_dir.path().join("test.cbl");
        let output_path = temp_dir.path().join("output-file");
        fs::write(&input_file, "def test():\n    /say safe\n").unwrap();
        fs::write(&output_path, "important\n").unwrap();

        let error = build(BuildOptions {
            input: Some(input_file),
            output: Some(output_path.clone()),
            namespace: Some("file_output".to_string()),
            pack_format: None,
            description: None,
            verbose: false,
            quiet: false,
            zip: false,
            experimental_resource_pack: false,
            validate: true,
            dry_run: false,
            commands_json: PathBuf::from("data/commands.json"),
        })
        .unwrap_err();

        assert!(error.contains("Refusing to build data pack over non-directory output path"));
        assert_eq!(fs::read_to_string(&output_path).unwrap(), "important\n");
    }

    #[test]
    fn build_rejects_namespace_paths_before_write_or_zip() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let project_dir = temp_dir.path().join("project");
        let input_file = project_dir.join("test.cbl");
        let output_dir = project_dir.join("out");
        let abs_escape = temp_dir.path().join("abs_escape");
        fs::create_dir_all(&project_dir).unwrap();
        fs::write(&input_file, "def test():\n    /say safe\n").unwrap();

        let cases = [
            (".", false, output_dir.clone()),
            ("..", false, output_dir.clone()),
            ("../../rel_escape", false, project_dir.join("rel_escape")),
            (
                "nested/escaped",
                false,
                output_dir.join("data/nested/escaped"),
            ),
            ("bad\\escaped", false, output_dir.join("data/bad\\escaped")),
            (
                abs_escape.to_str().expect("temp path should be UTF-8"),
                false,
                abs_escape.clone(),
            ),
            (
                "../zip_escape",
                true,
                temp_dir.path().join("zip_escape.zip"),
            ),
        ];

        for (namespace, zip, escaped_path) in cases {
            let error = build(BuildOptions {
                input: Some(input_file.clone()),
                output: Some(output_dir.clone()),
                namespace: Some(namespace.to_string()),
                pack_format: None,
                description: None,
                verbose: false,
                quiet: true,
                zip,
                experimental_resource_pack: false,
                validate: false,
                dry_run: false,
                commands_json: PathBuf::from("data/commands.json"),
            })
            .unwrap_err();

            assert!(
                error.contains("Invalid namespace") || error.contains("Namespace too long"),
                "namespace {namespace:?} produced unexpected error: {error}"
            );
            assert!(
                !escaped_path.exists(),
                "namespace {namespace:?} unexpectedly created {}",
                escaped_path.display()
            );
        }

        assert!(!output_dir.exists());
    }

    #[test]
    fn dry_run_does_not_write_output() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let input_file = temp_dir.path().join("test.cbl");
        let output_dir = temp_dir.path().join("output");
        fs::write(&input_file, "def test():\n    /say dry run\n").unwrap();

        build(BuildOptions {
            input: Some(input_file),
            output: Some(output_dir.clone()),
            namespace: Some("dry_run".to_string()),
            pack_format: None,
            description: None,
            verbose: false,
            quiet: false,
            zip: false,
            experimental_resource_pack: false,
            validate: false,
            dry_run: true,
            commands_json: PathBuf::from("data/commands.json"),
        })
        .unwrap();

        assert!(!output_dir.exists());
    }

    #[cfg(unix)]
    #[test]
    fn build_allows_symlink_output_parent_component() {
        use std::os::unix::fs::symlink;

        let temp_dir = tempfile::TempDir::new().unwrap();
        let input_file = temp_dir.path().join("test.cbl");
        let real_parent = temp_dir.path().join("real-parent");
        let symlink_parent = temp_dir.path().join("symlink-parent");
        let output_dir = symlink_parent.join("output");
        fs::write(&input_file, "def test():\n    /say safe\n").unwrap();
        fs::create_dir_all(&real_parent).unwrap();
        symlink(&real_parent, &symlink_parent).unwrap();

        build(BuildOptions {
            input: Some(input_file),
            output: Some(output_dir),
            namespace: Some("symlink_build".to_string()),
            pack_format: None,
            description: None,
            verbose: false,
            quiet: false,
            zip: false,
            experimental_resource_pack: false,
            validate: false,
            dry_run: false,
            commands_json: PathBuf::from("data/commands.json"),
        })
        .unwrap();

        // Links above the output folder are where the user chose to build.
        assert!(real_parent.join("output/pack.mcmeta").exists());
    }

    #[cfg(unix)]
    #[test]
    fn build_directory_input_allows_symlink_output_parent_component() {
        use std::os::unix::fs::symlink;

        let temp_dir = tempfile::TempDir::new().unwrap();
        let source_dir = temp_dir.path().join("src");
        let real_parent = temp_dir.path().join("real-parent");
        let symlink_parent = temp_dir.path().join("symlink-parent");
        let output_dir = symlink_parent.join("output");
        fs::create_dir_all(&source_dir).unwrap();
        fs::write(source_dir.join("main.cbl"), "def main():\n    /say safe\n").unwrap();
        fs::create_dir_all(&real_parent).unwrap();
        symlink(&real_parent, &symlink_parent).unwrap();

        build(BuildOptions {
            input: Some(source_dir),
            output: Some(output_dir),
            namespace: Some("symlink_build".to_string()),
            pack_format: None,
            description: None,
            verbose: false,
            quiet: false,
            zip: false,
            experimental_resource_pack: false,
            validate: false,
            dry_run: false,
            commands_json: PathBuf::from("data/commands.json"),
        })
        .unwrap();

        // Links above the output folder are where the user chose to build.
        assert!(real_parent.join("output/pack.mcmeta").exists());
    }

    #[cfg(unix)]
    #[test]
    fn build_rejects_existing_output_tree_with_symlink_descendant() {
        use std::os::unix::fs::symlink;

        let temp_dir = tempfile::TempDir::new().unwrap();
        let input_file = temp_dir.path().join("test.cbl");
        let output_dir = temp_dir.path().join("output");
        let outside_dir = temp_dir.path().join("outside");
        fs::write(&input_file, "def test():\n    /say safe\n").unwrap();
        fs::create_dir_all(&output_dir).unwrap();
        fs::create_dir_all(&outside_dir).unwrap();
        fs::write(outside_dir.join("important.txt"), "keep\n").unwrap();
        symlink(&outside_dir, output_dir.join("data")).unwrap();

        let error = build(BuildOptions {
            input: Some(input_file),
            output: Some(output_dir),
            namespace: Some("symlink_build".to_string()),
            pack_format: None,
            description: None,
            verbose: false,
            quiet: false,
            zip: false,
            experimental_resource_pack: false,
            validate: false,
            dry_run: false,
            commands_json: PathBuf::from("data/commands.json"),
        })
        .unwrap_err();

        assert!(error.contains("Refusing to build through symlink"));
        assert_eq!(
            fs::read_to_string(outside_dir.join("important.txt")).unwrap(),
            "keep\n"
        );
        assert!(!outside_dir.join("symlink_build").exists());
    }

    #[test]
    fn build_rejects_traversal_namespace_from_config_without_deleting_functions() {
        let _guard = cwd_lock().lock().unwrap();
        let temp_dir = tempfile::TempDir::new().unwrap();
        let project_dir = temp_dir.path().join("project");
        let source_dir = project_dir.join("src");
        let victim_function_dir = temp_dir.path().join("victim").join("function");

        fs::create_dir_all(&source_dir).unwrap();
        fs::create_dir_all(project_dir.join("data")).unwrap();
        fs::create_dir_all(&victim_function_dir).unwrap();
        fs::write(source_dir.join("main.cbl"), "def main():\n    /say safe\n").unwrap();
        fs::write(
            victim_function_dir.join("important.mcfunction"),
            "say keep\n",
        )
        .unwrap();
        fs::write(
            project_dir.join("cobble.toml"),
            r#"
[project]
name = "malicious"
description = "malicious"
namespace = "../../victim"
version = "1.0.0"
pack_format = "121.0"

[build]
source = "src"
output = "."
"#,
        )
        .unwrap();

        let _cwd = CurrentDirGuard::push(&project_dir);
        let error = build(BuildOptions {
            input: None,
            output: None,
            namespace: None,
            pack_format: None,
            description: None,
            verbose: false,
            quiet: true,
            zip: false,
            experimental_resource_pack: false,
            validate: false,
            dry_run: false,
            commands_json: PathBuf::from("commands.json"),
        })
        .unwrap_err();

        assert!(error.contains("Invalid namespace"));
        assert_eq!(
            fs::read_to_string(victim_function_dir.join("important.mcfunction")).unwrap(),
            "say keep\n"
        );
    }

    #[test]
    fn build_validate_refuses_to_replace_unowned_existing_output() {
        let _guard = cwd_lock().lock().unwrap();
        let temp_dir = tempfile::TempDir::new().unwrap();
        let project_dir = temp_dir.path().join("project");
        let source_dir = project_dir.join("src");
        let victim_dir = temp_dir.path().join("victim").join("keep");
        let commands_json = project_dir.join("commands.json");

        fs::create_dir_all(&source_dir).unwrap();
        fs::create_dir_all(victim_dir.join("sub")).unwrap();
        fs::write(source_dir.join("main.cbl"), "def main():\n    /say safe\n").unwrap();
        fs::write(victim_dir.join("sub").join("important.txt"), "keep\n").unwrap();
        write_say_commands_json(&commands_json);
        fs::write(
            project_dir.join("cobble.toml"),
            r#"
[project]
name = "malicious"
description = "malicious"
namespace = "malicious"
version = "1.0.0"
pack_format = "121.0"

[build]
source = "src"
output = "../victim/keep"
"#,
        )
        .unwrap();

        let _cwd = CurrentDirGuard::push(&project_dir);
        let error = build(BuildOptions {
            input: None,
            output: None,
            namespace: None,
            pack_format: None,
            description: None,
            verbose: false,
            quiet: true,
            zip: false,
            experimental_resource_pack: false,
            validate: true,
            dry_run: false,
            commands_json,
        })
        .unwrap_err();

        assert!(
            error.contains("Refusing to replace existing output without Cobble ownership marker"),
            "{error}"
        );
        assert_eq!(
            fs::read_to_string(victim_dir.join("sub").join("important.txt")).unwrap(),
            "keep\n"
        );
        assert!(!victim_dir.join("pack.mcmeta").exists());
    }

    #[test]
    fn build_validate_empty_source_does_not_touch_existing_output() {
        let _guard = cwd_lock().lock().unwrap();
        let temp_dir = tempfile::TempDir::new().unwrap();
        let project_dir = temp_dir.path().join("project");
        let source_dir = project_dir.join("src");
        let victim_dir = temp_dir.path().join("victim").join("keep");
        let commands_json = project_dir.join("commands.json");

        fs::create_dir_all(&source_dir).unwrap();
        fs::create_dir_all(victim_dir.join("data/empty/function")).unwrap();
        fs::write(
            victim_dir.join("data/empty/function/manual.mcfunction"),
            "say keep\n",
        )
        .unwrap();
        write_say_commands_json(&commands_json);
        fs::write(
            project_dir.join("cobble.toml"),
            r#"
[project]
name = "empty"
description = "empty"
namespace = "empty"
version = "1.0.0"
pack_format = "121.0"

[build]
source = "src"
output = "../victim/keep"
"#,
        )
        .unwrap();

        let _cwd = CurrentDirGuard::push(&project_dir);
        let error = build(BuildOptions {
            input: None,
            output: None,
            namespace: None,
            pack_format: None,
            description: None,
            verbose: false,
            quiet: true,
            zip: false,
            experimental_resource_pack: false,
            validate: true,
            dry_run: false,
            commands_json,
        })
        .unwrap_err();

        assert!(error.contains("No Cobble files found"), "{error}");
        assert_eq!(
            fs::read_to_string(victim_dir.join("data/empty/function/manual.mcfunction")).unwrap(),
            "say keep\n"
        );
        assert!(!victim_dir.join("pack.mcmeta").exists());
        assert!(!victim_dir.join(".cobble/build_manifest.json").exists());
    }

    #[test]
    fn build_validate_preserves_unrelated_files_after_prior_nonvalidated_build() {
        let _guard = cwd_lock().lock().unwrap();
        let temp_dir = tempfile::TempDir::new().unwrap();
        let project_dir = temp_dir.path().join("project");
        let source_dir = project_dir.join("src");
        let victim_dir = temp_dir.path().join("victim").join("keep");
        let commands_json = project_dir.join("commands.json");

        fs::create_dir_all(&source_dir).unwrap();
        fs::write(source_dir.join("main.cbl"), "def main():\n    /say safe\n").unwrap();
        write_say_commands_json(&commands_json);
        fs::write(
            project_dir.join("cobble.toml"),
            r#"
[project]
name = "malicious"
description = "malicious"
namespace = "malicious"
version = "1.0.0"
pack_format = "121.0"

[build]
source = "src"
output = "../victim/keep"
"#,
        )
        .unwrap();

        let _cwd = CurrentDirGuard::push(&project_dir);
        build(BuildOptions {
            input: None,
            output: None,
            namespace: None,
            pack_format: None,
            description: None,
            verbose: false,
            quiet: true,
            zip: false,
            experimental_resource_pack: false,
            validate: false,
            dry_run: false,
            commands_json: commands_json.clone(),
        })
        .unwrap();
        // Unrelated files that appear after the plain build must survive the
        // staged --validate replacement.
        fs::create_dir_all(victim_dir.join("sub")).unwrap();
        fs::write(victim_dir.join("sub").join("important.txt"), "keep\n").unwrap();
        build(BuildOptions {
            input: None,
            output: None,
            namespace: None,
            pack_format: None,
            description: None,
            verbose: false,
            quiet: true,
            zip: false,
            experimental_resource_pack: false,
            validate: true,
            dry_run: false,
            commands_json,
        })
        .unwrap();

        assert_eq!(
            fs::read_to_string(victim_dir.join("sub").join("important.txt")).unwrap(),
            "keep\n"
        );
        assert!(victim_dir
            .join("data/malicious/function/main.mcfunction")
            .exists());
    }

    #[test]
    fn build_validate_replaces_owned_existing_output() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let input_file = temp_dir.path().join("main.cbl");
        let output_dir = temp_dir.path().join("output");
        let commands_json = temp_dir.path().join("commands.json");
        write_say_commands_json(&commands_json);

        let build_once = || {
            build(BuildOptions {
                input: Some(input_file.clone()),
                output: Some(output_dir.clone()),
                namespace: Some("owned".to_string()),
                pack_format: None,
                description: None,
                verbose: false,
                quiet: true,
                zip: false,
                experimental_resource_pack: false,
                validate: true,
                dry_run: false,
                commands_json: commands_json.clone(),
            })
        };

        fs::write(&input_file, "def main():\n    /say first\n").unwrap();
        build_once().unwrap();
        fs::write(output_dir.join("notes.txt"), "keep\n").unwrap();
        fs::write(&input_file, "def main():\n    /say second\n").unwrap();
        build_once().unwrap();

        let content =
            fs::read_to_string(output_dir.join("data/owned/function/main.mcfunction")).unwrap();
        assert_eq!(without_return_prologue(&content).trim(), "say second");
        assert_eq!(
            fs::read_to_string(output_dir.join("notes.txt")).unwrap(),
            "keep\n"
        );
    }

    #[test]
    fn quiet_build_succeeds_and_verbose_quiet_is_rejected() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let input_file = temp_dir.path().join("test.cbl");
        let output_dir = temp_dir.path().join("output");
        fs::write(&input_file, "def test():\n    /say quiet\n").unwrap();

        build(BuildOptions {
            input: Some(input_file.clone()),
            output: Some(output_dir.clone()),
            namespace: Some("quiet".to_string()),
            pack_format: None,
            description: None,
            verbose: false,
            quiet: true,
            zip: false,
            experimental_resource_pack: false,
            validate: false,
            dry_run: false,
            commands_json: PathBuf::from("data/commands.json"),
        })
        .unwrap();
        assert!(output_dir
            .join("data/quiet/function/test.mcfunction")
            .exists());

        let error = build(BuildOptions {
            input: Some(input_file),
            output: Some(output_dir),
            namespace: Some("quiet".to_string()),
            pack_format: None,
            description: None,
            verbose: true,
            quiet: true,
            zip: false,
            experimental_resource_pack: false,
            validate: false,
            dry_run: false,
            commands_json: PathBuf::from("data/commands.json"),
        })
        .unwrap_err();
        assert!(error.contains("--quiet cannot be combined with --verbose"));
    }

    #[test]
    fn dry_run_validate_preserves_existing_output_and_cleans_staging() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let input_file = temp_dir.path().join("test.cbl");
        let output_dir = temp_dir.path().join("output");
        let valid_commands_json = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("data")
            .join("commands.json");
        if !valid_commands_json.exists() {
            return;
        }

        fs::write(&input_file, "def test():\n    /say valid\n").unwrap();
        fs::create_dir_all(&output_dir).unwrap();
        fs::write(output_dir.join("marker.txt"), "keep\n").unwrap();

        build(BuildOptions {
            input: Some(input_file),
            output: Some(output_dir.clone()),
            namespace: Some("dry_run".to_string()),
            pack_format: None,
            description: None,
            verbose: false,
            quiet: false,
            zip: false,
            experimental_resource_pack: false,
            validate: true,
            dry_run: true,
            commands_json: valid_commands_json,
        })
        .unwrap();

        assert_eq!(
            fs::read_to_string(output_dir.join("marker.txt")).unwrap(),
            "keep\n"
        );
        assert!(!temp_dir
            .path()
            .read_dir()
            .unwrap()
            .filter_map(|entry| entry.ok())
            .any(|entry| entry
                .file_name()
                .to_string_lossy()
                .contains(".output.cobble-staging-")));
    }

    #[test]
    fn build_validate_writes_validation_summary_to_manifest() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let input_file = temp_dir.path().join("test.cbl");
        let output_dir = temp_dir.path().join("output");
        let valid_commands_json = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("data")
            .join("commands.json");
        if !valid_commands_json.exists() {
            return;
        }

        fs::write(&input_file, "def test():\n    /say valid\n").unwrap();
        build(BuildOptions {
            input: Some(input_file),
            output: Some(output_dir.clone()),
            namespace: Some("manifest_validate".to_string()),
            pack_format: None,
            description: None,
            verbose: false,
            quiet: false,
            zip: false,
            experimental_resource_pack: false,
            validate: true,
            dry_run: false,
            commands_json: valid_commands_json,
        })
        .unwrap();

        let manifest_path = output_dir.join(".cobble/build_manifest.json");
        let manifest: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(manifest_path).unwrap()).unwrap();
        assert_eq!(manifest["validation"]["enabled"], true);
        assert_eq!(manifest["validation"]["commands_checked"], 1);
        assert_eq!(manifest["validation"]["errors"], 0);
    }

    #[test]
    fn build_fails_on_missing_import_with_importing_file() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let input_file = temp_dir.path().join("main.cbl");
        let output_dir = temp_dir.path().join("output");
        fs::write(&input_file, "import missing\n\ndef test():\n    pass\n").unwrap();

        let error = build(BuildOptions {
            input: Some(input_file.clone()),
            output: Some(output_dir),
            namespace: None,
            pack_format: None,
            description: None,
            verbose: false,
            quiet: false,
            zip: false,
            experimental_resource_pack: false,
            validate: false,
            dry_run: false,
            commands_json: PathBuf::from("data/commands.json"),
        })
        .unwrap_err();

        assert!(error.contains("Cannot import 'missing'"));
        assert!(error.contains(&input_file.display().to_string()));
    }

    #[test]
    fn build_fails_on_import_cycle_with_chain() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let main_file = temp_dir.path().join("main.cbl");
        let helper_file = temp_dir.path().join("helper.cbl");
        let output_dir = temp_dir.path().join("output");
        fs::write(&main_file, "import helper\n\ndef main():\n    /say main\n").unwrap();
        fs::write(
            &helper_file,
            "import main\n\ndef helper():\n    /say helper\n",
        )
        .unwrap();

        let error = build(BuildOptions {
            input: Some(main_file.clone()),
            output: Some(output_dir),
            namespace: None,
            pack_format: None,
            description: None,
            verbose: false,
            quiet: false,
            zip: false,
            experimental_resource_pack: false,
            validate: false,
            dry_run: false,
            commands_json: PathBuf::from("data/commands.json"),
        })
        .unwrap_err();

        assert!(error.contains("Circular import detected"));
        assert!(error.contains(&main_file.display().to_string()));
        assert!(error.contains(&helper_file.display().to_string()));
    }

    #[test]
    fn build_fails_on_cross_file_duplicate_function_without_writing_output() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let main_file = temp_dir.path().join("main.cbl");
        let helper_file = temp_dir.path().join("helper.cbl");
        let output_dir = temp_dir.path().join("output");
        fs::write(
            &main_file,
            "import helper\n\ndef greet():\n    /say from main\n",
        )
        .unwrap();
        fs::write(&helper_file, "def greet():\n    /say from helper\n").unwrap();

        let error = build(BuildOptions {
            input: Some(main_file),
            output: Some(output_dir.clone()),
            namespace: None,
            pack_format: None,
            description: None,
            verbose: false,
            quiet: false,
            zip: false,
            experimental_resource_pack: false,
            validate: false,
            dry_run: false,
            commands_json: PathBuf::from("data/commands.json"),
        })
        .unwrap_err();

        assert!(error.contains("Language diagnostics failed"));
        assert!(error.contains("duplicate-function"));
        assert!(error.contains("Duplicate function definition `greet` across imported files"));
        assert!(!output_dir.exists());
    }

    #[test]
    fn build_fails_on_language_diagnostic_without_writing_output() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let input_file = temp_dir.path().join("main.cbl");
        let output_dir = temp_dir.path().join("output");
        fs::write(&input_file, "def main():\n    a, b = 2\n").unwrap();

        let error = build(BuildOptions {
            input: Some(input_file),
            output: Some(output_dir.clone()),
            namespace: None,
            pack_format: None,
            description: None,
            verbose: false,
            quiet: false,
            zip: false,
            experimental_resource_pack: false,
            validate: false,
            dry_run: false,
            commands_json: PathBuf::from("data/commands.json"),
        })
        .unwrap_err();

        assert!(error.contains("Language diagnostics failed"));
        assert!(error.contains("unsupported-assignment-target"));
        assert!(error.contains("Unsupported assignment target"));
        assert!(!output_dir.exists());
    }

    #[test]
    fn cli_pack_format_overrides_invalid_config_value() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let source_dir = temp_dir.path().join("src");
        let output_dir = temp_dir.path().join("output");
        fs::create_dir_all(&source_dir).unwrap();
        fs::write(source_dir.join("main.cbl"), "def main():\n    /say hi\n").unwrap();
        fs::write(
            temp_dir.path().join("cobble.toml"),
            r#"
[project]
name = "Override"
description = "Override"
namespace = "override"
pack_format = "18"

[build]
source = "src"
output = "output"
"#,
        )
        .unwrap();

        build(BuildOptions {
            input: Some(source_dir),
            output: Some(output_dir.clone()),
            namespace: None,
            pack_format: Some("121.0".to_string()),
            description: None,
            verbose: false,
            quiet: false,
            zip: false,
            experimental_resource_pack: false,
            validate: false,
            dry_run: false,
            commands_json: PathBuf::from("data/commands.json"),
        })
        .unwrap();

        assert!(output_dir
            .join("data/override/function/main.mcfunction")
            .exists());
    }

    #[test]
    fn empty_source_directory_does_not_clean_previous_output() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let source_dir = temp_dir.path().join("src");
        let output_dir = temp_dir.path().join("output");
        let input_file = source_dir.join("main.cbl");
        fs::create_dir_all(&source_dir).unwrap();
        fs::write(&input_file, "def main():\n    /say hi\n").unwrap();

        let build_once = || {
            build(BuildOptions {
                input: Some(source_dir.clone()),
                output: Some(output_dir.clone()),
                namespace: Some("stale".to_string()),
                pack_format: None,
                description: None,
                verbose: false,
                quiet: false,
                zip: false,
                experimental_resource_pack: false,
                validate: false,
                dry_run: false,
                commands_json: PathBuf::from("data/commands.json"),
            })
        };

        build_once().unwrap();
        assert!(output_dir
            .join("data/stale/function/main.mcfunction")
            .exists());

        fs::remove_file(input_file).unwrap();
        let error = build_once().unwrap_err();

        assert!(error.contains("No Cobble files found"));
        assert!(output_dir
            .join("data/stale/function/main.mcfunction")
            .exists());
    }

    #[test]
    fn zip_contains_only_datapack_files_when_output_is_current_dir() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let _lock = cwd_lock().lock().unwrap();
        let _guard = CurrentDirGuard::push(temp_dir.path());
        fs::write("main.cbl", "def main():\n    /say hi\n").unwrap();
        // The current directory is non-empty (it holds the source), so a
        // plain build into it requires this project's ownership marker.
        let (_, project_id) = project_marker_identity(temp_dir.path());
        fs::create_dir_all(".cobble").unwrap();
        fs::write(
            ".cobble/build_manifest.json",
            format!(
                r#"{{"version":1,"cobble_version":"test","namespace":"zipped","project_id":"{project_id}"}}"#
            ),
        )
        .unwrap();

        build(BuildOptions {
            input: Some(PathBuf::from("main.cbl")),
            output: Some(PathBuf::from(".")),
            namespace: Some("zipped".to_string()),
            pack_format: None,
            description: None,
            verbose: false,
            quiet: false,
            zip: true,
            experimental_resource_pack: false,
            validate: false,
            dry_run: false,
            commands_json: PathBuf::from("data/commands.json"),
        })
        .unwrap();

        let zip_file = fs::File::open(temp_dir.path().join("zipped.zip")).unwrap();
        let mut archive = zip::ZipArchive::new(zip_file).unwrap();
        let names: Vec<String> = (0..archive.len())
            .map(|index| archive.by_index(index).unwrap().name().to_string())
            .collect();

        assert!(names.iter().any(|name| name == "pack.mcmeta"));
        assert!(names
            .iter()
            .any(|name| name == "data/zipped/function/main.mcfunction"));
        assert!(!names.iter().any(|name| name == "main.cbl"));
        assert!(!names.iter().any(|name| name == "zipped.zip"));
        assert!(!names.iter().any(|name| name.starts_with(".cobble/")));
    }

    #[cfg(unix)]
    #[test]
    fn build_zip_refuses_to_overwrite_symlink_target() {
        use std::os::unix::fs::symlink;

        let temp_dir = tempfile::TempDir::new().unwrap();
        let input_file = temp_dir.path().join("main.cbl");
        let output_dir = temp_dir.path().join("output");
        let victim_file = temp_dir.path().join("important.txt");
        let zip_path = temp_dir.path().join("zipped.zip");
        fs::write(&input_file, "def main():\n    /say hi\n").unwrap();
        fs::write(&victim_file, "keep\n").unwrap();
        symlink(&victim_file, &zip_path).unwrap();

        let error = build(BuildOptions {
            input: Some(input_file),
            output: Some(output_dir),
            namespace: Some("zipped".to_string()),
            pack_format: None,
            description: None,
            verbose: false,
            quiet: true,
            zip: true,
            experimental_resource_pack: false,
            validate: false,
            dry_run: false,
            commands_json: PathBuf::from("data/commands.json"),
        })
        .unwrap_err();

        assert!(error.contains("Refusing to create zip file through symlink"));
        assert_eq!(fs::read_to_string(victim_file).unwrap(), "keep\n");
    }

    #[cfg(unix)]
    #[test]
    fn build_zip_replaces_hardlink_without_overwriting_target_inode() {
        use std::fs::hard_link;

        let temp_dir = tempfile::TempDir::new().unwrap();
        let project_dir = temp_dir.path().join("project");
        let source_dir = project_dir.join("src");
        let output_dir = project_dir.join("output");
        let outside_dir = temp_dir.path().join("outside");
        let victim_file = outside_dir.join("important.txt");
        let zip_path = project_dir.join("resources.zip");
        fs::create_dir_all(&source_dir).unwrap();
        fs::create_dir_all(&outside_dir).unwrap();
        fs::write(
            source_dir.join("main.cbl"),
            "def main():\n    /say hardlink\n",
        )
        .unwrap();
        fs::write(&victim_file, "keep\n").unwrap();
        hard_link(&victim_file, &zip_path).unwrap();

        build(BuildOptions {
            input: Some(source_dir),
            output: Some(output_dir),
            namespace: Some("resources".to_string()),
            pack_format: None,
            description: None,
            verbose: false,
            quiet: true,
            zip: true,
            experimental_resource_pack: false,
            validate: false,
            dry_run: false,
            commands_json: PathBuf::from("data/commands.json"),
        })
        .unwrap();

        assert_eq!(fs::read_to_string(&victim_file).unwrap(), "keep\n");
        assert!(fs::read(&zip_path).unwrap().starts_with(b"PK"));
    }

    #[cfg(unix)]
    #[test]
    fn build_pack_mcmeta_replaces_hardlink_without_overwriting_target_inode() {
        use std::fs::hard_link;

        let temp_dir = tempfile::TempDir::new().unwrap();
        let input_file = temp_dir.path().join("main.cbl");
        let output_dir = temp_dir.path().join("output");
        let outside_dir = temp_dir.path().join("outside");
        let victim_file = outside_dir.join("pack_target.txt");
        fs::create_dir_all(&output_dir).unwrap();
        fs::create_dir_all(&outside_dir).unwrap();
        fs::write(&input_file, "def main():\n    /say hardlink\n").unwrap();
        fs::write(&victim_file, "keep\n").unwrap();
        hard_link(&victim_file, output_dir.join("pack.mcmeta")).unwrap();
        // Plain builds only re-enter non-empty directories owned by this
        // project, so mark the pre-existing output accordingly.
        let (_, project_id) = project_marker_identity(temp_dir.path());
        fs::create_dir_all(output_dir.join(".cobble")).unwrap();
        fs::write(
            output_dir.join(".cobble/build_manifest.json"),
            format!(
                r#"{{"version":1,"cobble_version":"test","namespace":"hardpack","project_id":"{project_id}"}}"#
            ),
        )
        .unwrap();

        build(BuildOptions {
            input: Some(input_file),
            output: Some(output_dir.clone()),
            namespace: Some("hardpack".to_string()),
            pack_format: None,
            description: None,
            verbose: false,
            quiet: true,
            zip: false,
            experimental_resource_pack: false,
            validate: false,
            dry_run: false,
            commands_json: PathBuf::from("data/commands.json"),
        })
        .unwrap();

        assert_eq!(fs::read_to_string(&victim_file).unwrap(), "keep\n");
        assert!(fs::read_to_string(output_dir.join("pack.mcmeta"))
            .unwrap()
            .contains("Generated by Cobble"));
    }

    #[cfg(unix)]
    #[test]
    fn resource_pack_asset_passthrough_replaces_hardlink_without_overwriting_target_inode() {
        use std::fs::hard_link;

        let _guard = cwd_lock().lock().unwrap();
        let temp_dir = tempfile::TempDir::new().unwrap();
        let project_dir = temp_dir.path().join("project");
        let source_dir = project_dir.join("src");
        let source_asset = project_dir.join("assets/resources/textures/item/icon.png");
        let output_asset = project_dir.join("output/assets/resources/textures/item/icon.png");
        let outside_dir = temp_dir.path().join("outside");
        let victim_file = outside_dir.join("asset_target.txt");
        fs::create_dir_all(&source_dir).unwrap();
        fs::create_dir_all(source_asset.parent().unwrap()).unwrap();
        fs::create_dir_all(output_asset.parent().unwrap()).unwrap();
        fs::create_dir_all(&outside_dir).unwrap();
        fs::write(source_dir.join("main.cbl"), "def main():\n    /say asset\n").unwrap();
        fs::write(&source_asset, "source-asset\n").unwrap();
        fs::write(&victim_file, "keep\n").unwrap();
        hard_link(&victim_file, &output_asset).unwrap();
        fs::write(
            project_dir.join("cobble.toml"),
            r#"
[project]
name = "resources"
description = "Resources"
namespace = "resources"
version = "1.0.0"
pack_format = "121.0"

[build]
source = "src"
output = "output"

[experimental]
resource_pack = true
"#,
        )
        .unwrap();
        // Plain builds only re-enter non-empty directories owned by this
        // project, so mark the pre-seeded output accordingly.
        let (_, project_id) = project_marker_identity(&project_dir);
        fs::create_dir_all(project_dir.join("output/.cobble")).unwrap();
        fs::write(
            project_dir.join("output/.cobble/build_manifest.json"),
            format!(
                r#"{{"version":1,"cobble_version":"test","namespace":"resources","project_id":"{project_id}"}}"#
            ),
        )
        .unwrap();

        let _cwd = CurrentDirGuard::push(&project_dir);
        build(BuildOptions {
            input: None,
            output: None,
            namespace: None,
            pack_format: None,
            description: None,
            verbose: false,
            quiet: true,
            zip: false,
            experimental_resource_pack: false,
            validate: false,
            dry_run: false,
            commands_json: PathBuf::from("data/commands.json"),
        })
        .unwrap();

        assert_eq!(fs::read_to_string(&victim_file).unwrap(), "keep\n");
        assert_eq!(fs::read_to_string(&output_asset).unwrap(), "source-asset\n");
    }

    #[test]
    fn asset_passthrough_relative_path_validation_rejects_traversal() {
        for path in [
            "../outside.png",
            "resources/../outside.png",
            "/resources/textures/item/outside.png",
            "resources",
            "BadNamespace/textures/item/test.png",
        ] {
            assert!(
                validate_asset_passthrough_relative_path(Path::new(path)).is_err(),
                "{path} should be rejected"
            );
        }

        validate_asset_passthrough_relative_path(Path::new(
            "resources/textures/item/custom_sword.png",
        ))
        .unwrap();
    }
}
