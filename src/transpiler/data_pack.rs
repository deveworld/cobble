use crate::pack_format::{PackFormat, SUPPORTED_PACK_FORMAT};
use crate::stdlib::StdLib;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

mod functions;
mod json_resources;
mod manifest;
mod paths;
mod resource_pack;
mod resources;
mod runtime;
mod source_locations;
mod source_map;
mod tags;
mod writer;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceLocation {
    pub file: PathBuf,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum GeneratedCommandKind {
    UserCommand,
    StdLib,
    RuntimeSetup,
    ControlFlow,
    Unrolled,
    JsonGenerated,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GeneratedCommand {
    pub text: String,
    pub source: Option<SourceLocation>,
    pub kind: GeneratedCommandKind,
}

impl GeneratedCommand {
    pub fn new(text: String, source: Option<SourceLocation>, kind: GeneratedCommandKind) -> Self {
        Self { text, source, kind }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceMapEntry {
    pub generated_path: String,
    pub generated_line: usize,
    pub command: String,
    pub source: Option<SourceLocation>,
    pub kind: GeneratedCommandKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceMap {
    pub version: u8,
    pub entries: Vec<SourceMapEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BuildManifest {
    pub version: u8,
    pub cobble_version: String,
    pub minecraft_version: String,
    pub pack_format: PackFormat,
    pub pack_format_text: String,
    pub namespace: String,
    pub description: String,
    pub project_root: String,
    pub project_id: String,
    pub generated_at_unix_epoch_ms: u64,
    pub input: Option<BuildManifestInput>,
    pub generated_namespaces: Vec<String>,
    pub generated: BuildManifestGenerated,
    pub resources: Vec<BuildManifestResourceEntry>,
    pub validation: Option<BuildManifestValidation>,
    /// Stdlib version declared in `cobble.toml`. Omitted when no stdlib
    /// helpers are used.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stdlib_version: Option<u8>,
    /// Stdlib modules activated by imports. Omitted when empty (e.g. when
    /// `import stdlib` activates all modules).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub active_stdlib_modules: Vec<String>,
    /// Number of `for` loops expanded at compile time. Omitted when zero.
    #[serde(skip_serializing_if = "is_zero_usize", default)]
    pub unrolled_loops: usize,
    /// Experimental features enabled for this build.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub experimental_features: Vec<String>,
}

fn is_zero_usize(value: &usize) -> bool {
    *value == 0
}

#[derive(Debug, Clone, Serialize)]
pub struct BuildManifestInput {
    pub source: String,
    pub entry_points: Vec<String>,
    pub compiled_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BuildManifestGenerated {
    pub functions: usize,
    pub commands: usize,
    pub source_map_entries: usize,
    pub function_tags: usize,
    pub stdlib_function_tags: usize,
    pub custom_function_tags: usize,
    pub json_function_tags: usize,
    pub advancements: usize,
    pub loot_tables: usize,
    pub recipes: usize,
    pub predicates: usize,
    pub item_modifiers: usize,
    pub json_resources: usize,
    pub total_json_resources: usize,
    #[serde(skip_serializing_if = "is_zero_usize", default)]
    pub resource_pack_models: usize,
    #[serde(skip_serializing_if = "is_zero_usize", default)]
    pub resource_pack_langs: usize,
    #[serde(skip_serializing_if = "is_zero_usize", default)]
    pub resource_pack_static_assets: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, Hash)]
pub struct BuildManifestResourceEntry {
    pub kind: String,
    pub namespace: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct BuildManifestValidation {
    pub enabled: bool,
    pub commands_json: String,
    pub files_checked: usize,
    pub commands_checked: usize,
    pub macro_commands_checked: usize,
    pub commands_skipped: usize,
    pub errors: usize,
    pub source_map_errors: usize,
}

pub struct DataPack {
    pub namespace: String,
    pub description: String,
    pub output_dir: PathBuf,
    pub functions: HashMap<String, Vec<String>>,
    pub command_metadata: HashMap<String, HashMap<usize, GeneratedCommand>>,
    pub tags: HashMap<String, Vec<String>>,
    pub advancements: HashMap<String, String>,
    pub loot_tables: HashMap<String, String>,
    pub recipes: HashMap<String, String>,
    pub predicates: HashMap<String, String>,
    pub item_modifiers: HashMap<String, String>,
    pub json_resources: HashMap<String, String>,
    pub json_resource_origins: HashMap<String, Vec<SourceLocation>>,
    pub resource_pack_models: HashMap<String, String>,
    pub resource_pack_langs: HashMap<String, String>,
    pub resource_pack_static_assets: HashSet<String>,
    pub resource_pack_resource_origins: HashMap<String, Vec<SourceLocation>>,
    pub pack_format: PackFormat,
    pub stdlib: StdLib,
    pub used_objectives: HashSet<String>,
    pub source_display_root: Option<PathBuf>,
    pub build_input: Option<BuildManifestInput>,
    pub project_root: Option<String>,
    pub project_id: Option<String>,
    pub validation_summary: Option<BuildManifestValidation>,
    /// Stdlib version from `cobble.toml`. Set by `Transpiler`.
    pub stdlib_version: Option<u8>,
    /// Active stdlib module names. Set by `Transpiler`.
    pub active_stdlib_modules: Vec<String>,
    /// Number of `for` loops unrolled at compile time.
    pub unrolled_loops: usize,
    /// Experimental features enabled for this build.
    pub experimental_features: Vec<String>,
    /// Whether previous generated resource-pack output may be cleaned.
    pub clean_previous_resource_pack_outputs: bool,
}

impl DataPack {
    pub fn new(namespace: String, output_dir: PathBuf) -> Self {
        Self {
            namespace,
            description: "Generated by Cobble".to_string(),
            output_dir,
            functions: HashMap::new(),
            command_metadata: HashMap::new(),
            tags: HashMap::new(),
            advancements: HashMap::new(),
            loot_tables: HashMap::new(),
            recipes: HashMap::new(),
            predicates: HashMap::new(),
            item_modifiers: HashMap::new(),
            json_resources: HashMap::new(),
            json_resource_origins: HashMap::new(),
            resource_pack_models: HashMap::new(),
            resource_pack_langs: HashMap::new(),
            resource_pack_static_assets: HashSet::new(),
            resource_pack_resource_origins: HashMap::new(),
            pack_format: SUPPORTED_PACK_FORMAT,
            stdlib: StdLib::new(),
            used_objectives: HashSet::new(),
            source_display_root: None,
            build_input: None,
            project_root: None,
            project_id: None,
            validation_summary: None,
            stdlib_version: None,
            active_stdlib_modules: Vec::new(),
            unrolled_loops: 0,
            experimental_features: Vec::new(),
            clean_previous_resource_pack_outputs: true,
        }
    }

    pub fn set_description(&mut self, desc: String) {
        self.description = desc;
    }

    pub fn set_pack_format(&mut self, format: PackFormat) {
        self.pack_format = format;
    }

    pub fn set_build_input(&mut self, input: BuildManifestInput) {
        self.build_input = Some(input);
    }

    pub fn set_project_identity(&mut self, project_root: String, project_id: String) {
        self.project_root = Some(project_root);
        self.project_id = Some(project_id);
    }

    pub fn set_source_display_root(&mut self, root: PathBuf) {
        self.source_display_root = Some(root);
    }

    pub fn set_validation_summary(&mut self, validation: Option<BuildManifestValidation>) {
        self.validation_summary = validation;
    }

    pub fn generated_counts(&self) -> BuildManifestGenerated {
        let resources = self.generated_resource_entries();
        let source_map_entry_count = self.source_map_entry_count_snapshot();
        self.generated_counts_with_source_map(source_map_entry_count, &resources)
    }

    pub fn build_manifest_snapshot(&self) -> BuildManifest {
        let generated_namespaces = self.generated_namespaces();
        let source_map_entry_count = self.source_map_entry_count_snapshot();
        self.build_manifest(source_map_entry_count, &generated_namespaces)
    }

    pub fn set_resource_pack_static_assets<I>(&mut self, assets: I)
    where
        I: IntoIterator<Item = String>,
    {
        self.resource_pack_static_assets = assets.into_iter().collect();
    }

    pub fn set_clean_previous_resource_pack_outputs(&mut self, enabled: bool) {
        self.clean_previous_resource_pack_outputs = enabled;
    }
}
