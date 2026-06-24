use crate::pack_format::{
    PackFormat, COBBLE_VERSION, SUPPORTED_MINECRAFT_VERSION, SUPPORTED_PACK_FORMAT,
};
use crate::stdlib::{EventType, StdLib};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

fn stable_relative_path(path: &Path, root: &Path) -> PathBuf {
    let canonical_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());

    if let Ok(relative_path) = canonical_path.strip_prefix(&canonical_root) {
        if !relative_path.as_os_str().is_empty() {
            return relative_path.to_path_buf();
        }
    }

    if path.is_relative() {
        return path.to_path_buf();
    }

    canonical_path
}

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
        let source_map_entry_count = self.functions.values().map(Vec::len).sum();
        self.generated_counts_with_source_map(source_map_entry_count, &resources)
    }

    pub fn build_manifest_snapshot(&self) -> BuildManifest {
        let generated_namespaces = self.generated_namespaces();
        let source_map_entry_count = self.functions.values().map(Vec::len).sum();
        self.build_manifest(source_map_entry_count, &generated_namespaces)
    }

    fn metadata_for_commands(
        commands: &[String],
        kind: GeneratedCommandKind,
    ) -> HashMap<usize, GeneratedCommand> {
        commands
            .iter()
            .enumerate()
            .map(|(index, command)| {
                (
                    index,
                    GeneratedCommand::new(command.clone(), None, kind.clone()),
                )
            })
            .collect()
    }

    fn complete_metadata(
        commands: &[String],
        mut metadata: HashMap<usize, GeneratedCommand>,
        default_kind: GeneratedCommandKind,
    ) -> HashMap<usize, GeneratedCommand> {
        for (index, command) in commands.iter().enumerate() {
            metadata.entry(index).or_insert_with(|| {
                GeneratedCommand::new(command.clone(), None, default_kind.clone())
            });
        }
        metadata
    }

    pub fn add_function(&mut self, name: String, commands: Vec<String>) {
        self.add_function_with_kind(name, commands, GeneratedCommandKind::ControlFlow);
    }

    pub fn add_function_with_kind(
        &mut self,
        name: String,
        commands: Vec<String>,
        kind: GeneratedCommandKind,
    ) {
        let metadata = Self::metadata_for_commands(&commands, kind);
        self.command_metadata.insert(name.clone(), metadata);
        self.functions.insert(name, commands);
    }

    pub fn add_function_with_metadata(
        &mut self,
        name: String,
        commands: Vec<String>,
        metadata: HashMap<usize, GeneratedCommand>,
    ) {
        let metadata =
            Self::complete_metadata(&commands, metadata, GeneratedCommandKind::ControlFlow);
        self.command_metadata.insert(name.clone(), metadata);
        self.functions.insert(name, commands);
    }

    pub fn insert_function_commands_with_kind(
        &mut self,
        name: &str,
        index: usize,
        new_commands: &[String],
        kind: GeneratedCommandKind,
    ) {
        if new_commands.is_empty() {
            return;
        }

        let Some(commands) = self.functions.get_mut(name) else {
            return;
        };
        let insert_index = index.min(commands.len());
        for (offset, command) in new_commands.iter().enumerate() {
            commands.insert(insert_index + offset, command.clone());
        }

        let existing_metadata = self.command_metadata.remove(name).unwrap_or_default();
        let mut updated_metadata = HashMap::new();
        for (old_index, metadata) in existing_metadata {
            let new_index = if old_index >= insert_index {
                old_index + new_commands.len()
            } else {
                old_index
            };
            updated_metadata.insert(new_index, metadata);
        }
        for (offset, command) in new_commands.iter().enumerate() {
            updated_metadata.insert(
                insert_index + offset,
                GeneratedCommand::new(command.clone(), None, kind.clone()),
            );
        }

        let completed = Self::complete_metadata(
            commands,
            updated_metadata,
            GeneratedCommandKind::ControlFlow,
        );
        self.command_metadata.insert(name.to_string(), completed);
    }

    pub fn insert_function_commands_with_metadata(
        &mut self,
        name: &str,
        index: usize,
        new_commands: &[String],
        new_metadata: HashMap<usize, GeneratedCommand>,
    ) {
        if new_commands.is_empty() {
            return;
        }

        let Some(commands) = self.functions.get_mut(name) else {
            return;
        };
        let insert_index = index.min(commands.len());
        for (offset, command) in new_commands.iter().enumerate() {
            commands.insert(insert_index + offset, command.clone());
        }

        let existing_metadata = self.command_metadata.remove(name).unwrap_or_default();
        let mut updated_metadata = HashMap::new();
        for (old_index, metadata) in existing_metadata {
            let new_index = if old_index >= insert_index {
                old_index + new_commands.len()
            } else {
                old_index
            };
            updated_metadata.insert(new_index, metadata);
        }

        for (offset, command) in new_commands.iter().enumerate() {
            let generated = new_metadata.get(&offset).cloned().unwrap_or_else(|| {
                GeneratedCommand::new(command.clone(), None, GeneratedCommandKind::ControlFlow)
            });
            updated_metadata.insert(insert_index + offset, generated);
        }

        let completed = Self::complete_metadata(
            commands,
            updated_metadata,
            GeneratedCommandKind::ControlFlow,
        );
        self.command_metadata.insert(name.to_string(), completed);
    }

    pub fn track_objective(&mut self, objective: &str) {
        self.used_objectives.insert(objective.to_string());
    }

    pub fn ensure_init_function(&mut self) {
        // Get or create the init function from load event handlers
        let load_handlers = self.stdlib.get_event_handlers(&EventType::Load);

        if let Some(init_func_name) = load_handlers.first().cloned() {
            // Add objectives to the beginning of the load function
            if let Some(commands) = self.functions.get_mut(&init_func_name) {
                let mut setup_commands = Vec::new();

                // Add gamerule first
                let gamerule_cmd = "gamerule max_command_sequence_length 1000000000".to_string();
                if !commands.contains(&gamerule_cmd) {
                    setup_commands.push(gamerule_cmd);
                }

                // Then add objectives
                let mut objectives: Vec<_> = self.used_objectives.iter().collect();
                objectives.sort();
                for objective in objectives {
                    let obj_cmd = format!("scoreboard objectives add {} dummy", objective);
                    // Only add if not already present
                    if !commands.contains(&obj_cmd) {
                        setup_commands.push(obj_cmd);
                    }
                }

                // Initialize Boolean literal constants if __internal__ objective is used
                if self.used_objectives.contains("__internal__") {
                    let internal_init_1 =
                        "scoreboard players set #true_const __internal__ 1".to_string();
                    let internal_init_2 =
                        "scoreboard players set #false_const __internal__ 0".to_string();
                    if !commands.contains(&internal_init_1) {
                        setup_commands.push(internal_init_1);
                    }
                    if !commands.contains(&internal_init_2) {
                        setup_commands.push(internal_init_2);
                    }
                }

                // Prepend setup commands to existing commands
                if !setup_commands.is_empty() {
                    let inserted_count = setup_commands.len();
                    let mut updated_commands = setup_commands;
                    updated_commands.extend(commands.clone());

                    let mut updated_metadata = Self::metadata_for_commands(
                        &updated_commands[..inserted_count],
                        GeneratedCommandKind::RuntimeSetup,
                    );
                    if let Some(existing_metadata) = self.command_metadata.remove(&init_func_name) {
                        for (index, command) in existing_metadata {
                            updated_metadata.insert(index + inserted_count, command);
                        }
                    }
                    updated_metadata = Self::complete_metadata(
                        &updated_commands,
                        updated_metadata,
                        GeneratedCommandKind::ControlFlow,
                    );

                    self.command_metadata
                        .insert(init_func_name.clone(), updated_metadata);
                    *commands = updated_commands;
                }
            }
        } else if !self.used_objectives.is_empty() {
            // No load handler exists, create a default init function
            let mut commands = Vec::new();

            // Add gamerule first
            commands.push("gamerule max_command_sequence_length 1000000000".to_string());

            // Then add objectives
            let mut objectives: Vec<_> = self.used_objectives.iter().collect();
            objectives.sort();
            for objective in objectives {
                commands.push(format!("scoreboard objectives add {} dummy", objective));
            }

            // Initialize Boolean literal constants if __internal__ objective is used
            if self.used_objectives.contains("__internal__") {
                commands.push("scoreboard players set #true_const __internal__ 1".to_string());
                commands.push("scoreboard players set #false_const __internal__ 0".to_string());
            }

            self.add_function_with_kind(
                "_cobble_init".to_string(),
                commands,
                GeneratedCommandKind::RuntimeSetup,
            );
            self.stdlib
                .add_event_listener(EventType::Load, "_cobble_init".to_string());
        }
    }

    pub fn add_tag(&mut self, tag_name: String, functions: Vec<String>) {
        self.tags.insert(tag_name, functions);
    }

    pub fn add_advancement(&mut self, name: String, json: String) {
        self.advancements.insert(name, json);
    }

    pub fn add_loot_table(&mut self, name: String, json: String) {
        self.loot_tables.insert(name, json);
    }

    pub fn add_recipe(&mut self, name: String, json: String) {
        self.recipes.insert(name, json);
    }

    pub fn add_predicate(&mut self, name: String, json: String) {
        self.predicates.insert(name, json);
    }

    pub fn add_item_modifier(&mut self, name: String, json: String) {
        self.item_modifiers.insert(name, json);
    }

    pub fn add_json_resource(&mut self, relative_path: String, json: String) -> Result<(), String> {
        self.add_json_resource_in_namespace(self.namespace.clone(), relative_path, json)
    }

    pub fn add_json_resource_in_namespace(
        &mut self,
        namespace: String,
        relative_path: String,
        json: String,
    ) -> Result<(), String> {
        self.add_json_resource_in_namespace_with_source(namespace, relative_path, json, None)
    }

    pub fn add_json_resource_in_namespace_with_source(
        &mut self,
        namespace: String,
        relative_path: String,
        json: String,
        source: Option<SourceLocation>,
    ) -> Result<(), String> {
        let key = Self::json_resource_key(&namespace, &relative_path);

        // Tags (tags/function/, tags/block/, tags/item/, tags/entity_type/)
        // are Cobble-owned typed resources. Duplicate declarations for the
        // same tag ID are merged deterministically instead of rejected.
        if Self::is_typed_tag_path(&relative_path) {
            return self.merge_tag_resource(&key, &namespace, &relative_path, &json, source);
        }

        // Pass-through JSON resources: exact duplicates are accepted, but
        // differing JSON for the same key is an error.
        if let Some(existing_json) = self.json_resources.get(&key) {
            if existing_json == &json {
                if let Some(source) = source {
                    self.json_resource_origins
                        .entry(key)
                        .or_default()
                        .push(source);
                }
                return Ok(());
            }

            let duplicate_kind =
                Self::json_resource_duplicate_kind(&relative_path, existing_json, &json);
            let mut message = format!(
                "Duplicate data pack resource '{}:{}' ({})",
                namespace, relative_path, duplicate_kind
            );
            if let Some(first_source) = self.json_resource_origins.get(&key).and_then(|v| v.first())
            {
                message.push_str(&format!(
                    "\n  first declaration: {}",
                    self.format_source_location(first_source)
                ));
            }
            if let Some(second_source) = source.as_ref() {
                message.push_str(&format!(
                    "\n  second declaration: {}",
                    self.format_source_location(second_source)
                ));
            }
            return Err(message);
        }
        self.json_resources.insert(key.clone(), json);
        if let Some(source) = source {
            self.json_resource_origins.insert(key, vec![source]);
        }
        Ok(())
    }

    pub fn add_resource_pack_model_in_namespace_with_source(
        &mut self,
        namespace: String,
        relative_path: String,
        json: String,
        source: Option<SourceLocation>,
    ) -> Result<(), String> {
        let key = Self::json_resource_key(&namespace, &relative_path);
        let source_display_root = self.source_display_root.clone();
        Self::insert_pass_through_resource(
            &mut self.resource_pack_models,
            &mut self.resource_pack_resource_origins,
            key,
            namespace,
            relative_path,
            json,
            source,
            |namespace, relative_path| {
                format!(
                    "Duplicate resource pack model '{}:{}'",
                    namespace, relative_path
                )
            },
            |source| Self::format_source_location_with_root(&source_display_root, source),
        )
    }

    pub fn add_resource_pack_lang_in_namespace_with_source(
        &mut self,
        namespace: String,
        locale: String,
        json: String,
        source: Option<SourceLocation>,
    ) -> Result<(), String> {
        let relative_path = format!("lang/{}", locale);
        let key = Self::json_resource_key(&namespace, &relative_path);
        let source_display_root = self.source_display_root.clone();
        Self::insert_pass_through_resource(
            &mut self.resource_pack_langs,
            &mut self.resource_pack_resource_origins,
            key,
            namespace,
            relative_path,
            json,
            source,
            |namespace, relative_path| {
                format!(
                    "Duplicate resource pack language file '{}:{}'",
                    namespace, relative_path
                )
            },
            |source| Self::format_source_location_with_root(&source_display_root, source),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn insert_pass_through_resource<F, G>(
        resources: &mut HashMap<String, String>,
        origins: &mut HashMap<String, Vec<SourceLocation>>,
        key: String,
        namespace: String,
        relative_path: String,
        json: String,
        source: Option<SourceLocation>,
        duplicate_message: F,
        format_source_location: G,
    ) -> Result<(), String>
    where
        F: Fn(&str, &str) -> String,
        G: Fn(&SourceLocation) -> String,
    {
        if let Some(existing_json) = resources.get(&key) {
            if existing_json == &json {
                if let Some(source) = source {
                    origins.entry(key).or_default().push(source);
                }
                return Ok(());
            }

            let mut message = format!(
                "{} (invalid overwrite)",
                duplicate_message(&namespace, &relative_path)
            );
            if let Some(first_source) = origins.get(&key).and_then(|v| v.first()) {
                message.push_str(&format!(
                    "\n  first declaration: {}",
                    format_source_location(first_source)
                ));
            }
            if let Some(second_source) = source.as_ref() {
                message.push_str(&format!(
                    "\n  second declaration: {}",
                    format_source_location(second_source)
                ));
            }
            return Err(message);
        }

        resources.insert(key.clone(), json);
        if let Some(source) = source {
            origins.insert(key, vec![source]);
        }
        Ok(())
    }

    /// Returns true when `relative_path` points at a Cobble-owned typed tag
    /// resource (`tags/function/`, `tags/block/`, `tags/item/`, or
    /// `tags/entity_type/`).
    fn is_typed_tag_path(relative_path: &str) -> bool {
        relative_path.starts_with("tags/function/")
            || relative_path.starts_with("tags/block/")
            || relative_path.starts_with("tags/item/")
            || relative_path.starts_with("tags/entity_type/")
    }

    /// Merge a typed tag resource. Duplicate declarations for the same tag
    /// ID have their `values` arrays combined, deduplicated, and sorted.
    fn merge_tag_resource(
        &mut self,
        key: &str,
        namespace: &str,
        relative_path: &str,
        new_json: &str,
        source: Option<SourceLocation>,
    ) -> Result<(), String> {
        let new_value: serde_json::Value = serde_json::from_str(new_json).map_err(|e| {
            format!(
                "Failed to parse tag JSON for '{}:{}': {}",
                namespace, relative_path, e
            )
        })?;
        let new_values = new_value
            .get("values")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        // Validate tag schema: values must be an array of string resource IDs.
        if new_value.get("values").is_some() && !new_value.get("values").unwrap().is_array() {
            return Err(format!(
                "Invalid tag '{}:{}': 'values' must be an array",
                namespace, relative_path
            ));
        }
        for (idx, entry) in new_values.iter().enumerate() {
            if !entry.is_string() {
                return Err(format!(
                    "Invalid tag '{}:{}': 'values[{}]' must be a string resource ID",
                    namespace, relative_path, idx
                ));
            }
        }

        let merged_json = if let Some(existing_json) = self.json_resources.get(key) {
            let existing_value: serde_json::Value =
                serde_json::from_str(existing_json).map_err(|e| {
                    format!(
                        "Failed to parse existing tag JSON for '{}:{}': {}",
                        namespace, relative_path, e
                    )
                })?;
            let existing_values = existing_value
                .get("values")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();

            let mut merged: Vec<serde_json::Value> = existing_values.clone();
            for entry in &new_values {
                if !merged.contains(entry) {
                    merged.push(entry.clone());
                }
            }
            merged.sort_by(|a, b| a.as_str().unwrap_or("").cmp(b.as_str().unwrap_or("")));

            let mut merged_obj = serde_json::Map::new();
            merged_obj.insert("values".to_string(), serde_json::Value::Array(merged));

            // Preserve replace if present in either declaration.
            if let Some(replace) = existing_value.get("replace") {
                merged_obj.insert("replace".to_string(), replace.clone());
            }
            if let Some(replace) = new_value.get("replace") {
                merged_obj.insert("replace".to_string(), replace.clone());
            }

            // Warn about replace if any declaration used true.
            if existing_value
                .get("replace")
                .or_else(|| new_value.get("replace"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                eprintln!(
                    "warning: tag '{}:{}' declared with replace=true.\n\
                     replace semantics are deferred in 0.8.0; the value is serialized as given\n\
                     but does not change merge behavior.",
                    namespace, relative_path
                );
            }

            serde_json::to_string_pretty(&serde_json::Value::Object(merged_obj))
                .map_err(|e| format!("Failed to serialize merged tag JSON: {}", e))?
        } else {
            // First declaration: validate and store as-is, sorting values.
            let mut sorted_values = new_values.clone();
            sorted_values.sort_by(|a, b| a.as_str().unwrap_or("").cmp(b.as_str().unwrap_or("")));

            let mut obj = serde_json::Map::new();
            obj.insert(
                "values".to_string(),
                serde_json::Value::Array(sorted_values),
            );
            if let Some(replace) = new_value.get("replace") {
                obj.insert("replace".to_string(), replace.clone());
                if replace.as_bool().unwrap_or(false) {
                    eprintln!(
                        "warning: tag '{}:{}' declared with replace=true.\n\
                         replace semantics are deferred in 0.8.0; the value is serialized as given\n\
                         but does not change merge behavior.",
                        namespace, relative_path
                    );
                }
            }
            serde_json::to_string_pretty(&serde_json::Value::Object(obj))
                .map_err(|e| format!("Failed to serialize tag JSON: {}", e))?
        };

        self.json_resources.insert(key.to_string(), merged_json);
        if let Some(source) = source {
            self.json_resource_origins
                .entry(key.to_string())
                .or_default()
                .push(source);
        }
        Ok(())
    }

    pub fn write(&self) -> std::io::Result<()> {
        self.validate_write_paths()?;

        let data_dir = self.output_dir.join("data");
        let assets_dir = self.output_dir.join("assets");
        let namespace_dir = data_dir.join(&self.namespace);
        let function_dir = namespace_dir.join("function");
        let legacy_function_dir = namespace_dir.join("functions");
        let minecraft_tags_dir = self
            .output_dir
            .join("data")
            .join("minecraft")
            .join("tags")
            .join("function");
        let legacy_minecraft_tags_dir = self
            .output_dir
            .join("data")
            .join("minecraft")
            .join("tags")
            .join("functions");
        let source_map_dir = self.output_dir.join(".cobble");
        let generated_namespaces_path = source_map_dir.join("generated_namespaces.json");
        let generated_asset_namespaces_path =
            source_map_dir.join("generated_asset_namespaces.json");
        let generated_namespaces = self.generated_namespaces();
        let generated_asset_namespaces = self.generated_asset_namespaces();

        if let Ok(content) = fs::read_to_string(&generated_namespaces_path) {
            if let Ok(previous_namespaces) = serde_json::from_str::<Vec<String>>(&content) {
                for namespace in previous_namespaces {
                    if !Self::is_safe_namespace_path(&namespace) {
                        continue;
                    }

                    if !generated_namespaces.contains(&namespace) {
                        let old_namespace_dir = data_dir.join(namespace);
                        if old_namespace_dir.exists() {
                            fs::remove_dir_all(old_namespace_dir)?;
                        }
                    } else if namespace != self.namespace {
                        Self::clean_generated_function_dirs(&data_dir.join(namespace))?;
                    }
                }
            }
        }
        if let Ok(content) = fs::read_to_string(&generated_asset_namespaces_path) {
            if let Ok(previous_namespaces) = serde_json::from_str::<Vec<String>>(&content) {
                for namespace in previous_namespaces {
                    if !Self::is_safe_namespace_path(&namespace) {
                        continue;
                    }

                    if !generated_asset_namespaces.contains(&namespace) {
                        let old_namespace_dir = assets_dir.join(namespace);
                        if old_namespace_dir.exists() {
                            fs::remove_dir_all(old_namespace_dir)?;
                        }
                    } else {
                        Self::clean_generated_asset_dirs(&assets_dir.join(namespace))?;
                    }
                }
            }
        }

        // Clean functions directory to remove stale .mcfunction files
        // This prevents deleted or renamed functions from persisting in the datapack
        if function_dir.exists() {
            fs::remove_dir_all(&function_dir)?;
        }
        if legacy_function_dir.exists() {
            fs::remove_dir_all(&legacy_function_dir)?;
        }

        // Clean generated metadata and JSON resource directories so rebuilds do
        // not leave removed declarations behind in the output pack.
        if source_map_dir.exists() {
            fs::remove_dir_all(&source_map_dir)?;
        }
        if minecraft_tags_dir.exists() {
            fs::remove_dir_all(&minecraft_tags_dir)?;
        }
        if legacy_minecraft_tags_dir.exists() {
            fs::remove_dir_all(&legacy_minecraft_tags_dir)?;
        }
        for namespace in &generated_namespaces {
            Self::clean_generated_resource_dirs(&data_dir.join(namespace))?;
        }
        for namespace in &generated_asset_namespaces {
            Self::clean_generated_asset_dirs(&assets_dir.join(namespace))?;
        }

        fs::create_dir_all(&function_dir)?;

        // Write pack.mcmeta
        self.write_pack_mcmeta()?;

        // Write all functions
        let mut source_map_entries = Vec::new();
        let mut function_names: Vec<_> = self.functions.keys().collect();
        function_names.sort();
        for name in function_names {
            let commands = &self.functions[name];
            let file_path = function_dir.join(format!("{}.mcfunction", name));
            let mut file = fs::File::create(file_path)?;
            let generated_path = format!("data/{}/function/{}.mcfunction", self.namespace, name);
            for (index, command) in commands.iter().enumerate() {
                writeln!(file, "{}", command)?;
                let metadata = self
                    .command_metadata
                    .get(name)
                    .and_then(|commands| commands.get(&index))
                    .cloned()
                    .unwrap_or_else(|| {
                        GeneratedCommand::new(
                            command.clone(),
                            None,
                            GeneratedCommandKind::ControlFlow,
                        )
                    });
                source_map_entries.push(SourceMapEntry {
                    generated_path: generated_path.clone(),
                    generated_line: index + 1,
                    command: metadata.text,
                    source: metadata
                        .source
                        .map(|source| self.normalize_source_location(source)),
                    kind: metadata.kind,
                });
            }
        }

        let source_map_entry_count = source_map_entries.len();
        if !source_map_entries.is_empty() {
            let source_map = SourceMap {
                version: 1,
                entries: source_map_entries,
            };
            fs::create_dir_all(&source_map_dir)?;
            fs::write(
                source_map_dir.join("source_map.json"),
                serde_json::to_string_pretty(&source_map).unwrap(),
            )?;
        }
        fs::create_dir_all(&source_map_dir)?;
        fs::write(
            source_map_dir.join("generated_namespaces.json"),
            serde_json::to_string_pretty(&generated_namespaces).unwrap(),
        )?;
        if !generated_asset_namespaces.is_empty() {
            fs::write(
                source_map_dir.join("generated_asset_namespaces.json"),
                serde_json::to_string_pretty(&generated_asset_namespaces).unwrap(),
            )?;
        }

        let stdlib_tags = self.stdlib.generate_tags(&self.namespace);
        let build_manifest = self.build_manifest(source_map_entry_count, &generated_namespaces);
        fs::write(
            source_map_dir.join("build_manifest.json"),
            serde_json::to_string_pretty(&build_manifest).unwrap(),
        )?;

        // Write tags from stdlib
        for (tag_name, functions) in stdlib_tags {
            Self::write_function_tag(&data_dir, &self.namespace, &tag_name, &functions)?;
        }
        let mut custom_tag_names: Vec<_> = self.tags.keys().collect();
        custom_tag_names.sort();
        for tag_name in custom_tag_names {
            Self::write_function_tag(&data_dir, &self.namespace, tag_name, &self.tags[tag_name])?;
        }

        // Write advancements
        if !self.advancements.is_empty() {
            let advancement_dir = namespace_dir.join("advancement");
            Self::write_json_resource_map(&advancement_dir, &self.advancements)?;
        }

        // Write loot tables
        if !self.loot_tables.is_empty() {
            let loot_table_dir = namespace_dir.join("loot_table");
            Self::write_json_resource_map(&loot_table_dir, &self.loot_tables)?;
        }

        // Write recipes
        if !self.recipes.is_empty() {
            let recipe_dir = namespace_dir.join("recipe");
            Self::write_json_resource_map(&recipe_dir, &self.recipes)?;
        }

        // Write predicates
        if !self.predicates.is_empty() {
            let predicate_dir = namespace_dir.join("predicate");
            Self::write_json_resource_map(&predicate_dir, &self.predicates)?;
        }

        // Write item modifiers
        if !self.item_modifiers.is_empty() {
            let item_modifier_dir = namespace_dir.join("item_modifier");
            Self::write_json_resource_map(&item_modifier_dir, &self.item_modifiers)?;
        }

        // Write generic JSON resources generated by datapack.* declarations
        let mut json_resource_paths: Vec<_> = self.json_resources.keys().collect();
        json_resource_paths.sort();
        for key in json_resource_paths {
            let json = &self.json_resources[key];
            let Some((resource_namespace, relative_path)) = Self::split_json_resource_key(key)
            else {
                continue;
            };
            let file_path = data_dir
                .join(resource_namespace)
                .join(format!("{}.json", relative_path));
            if let Some(parent) = file_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let content = Self::merge_tag_json_if_existing(&file_path, relative_path, json)?;
            fs::write(file_path, content)?;
        }

        self.write_resource_pack_assets(&assets_dir)?;

        Ok(())
    }

    fn build_manifest(
        &self,
        source_map_entry_count: usize,
        generated_namespaces: &[String],
    ) -> BuildManifest {
        let resources = self.generated_resource_entries();
        let generated = self.generated_counts_with_source_map(source_map_entry_count, &resources);

        BuildManifest {
            version: 1,
            cobble_version: COBBLE_VERSION.to_string(),
            minecraft_version: SUPPORTED_MINECRAFT_VERSION.to_string(),
            pack_format: self.pack_format,
            pack_format_text: self.pack_format.to_string(),
            namespace: self.namespace.clone(),
            description: self.description.clone(),
            project_root: self.project_root.clone().unwrap_or_default(),
            project_id: self.project_id.clone().unwrap_or_default(),
            generated_at_unix_epoch_ms: Self::generated_at_unix_epoch_ms(),
            input: self.build_input.clone(),
            generated_namespaces: generated_namespaces.to_vec(),
            generated,
            resources,
            validation: self.validation_summary.clone(),
            stdlib_version: self.stdlib_version,
            active_stdlib_modules: self.active_stdlib_modules.clone(),
            unrolled_loops: self.unrolled_loops,
            experimental_features: self.experimental_features.clone(),
        }
    }

    fn generated_at_unix_epoch_ms() -> u64 {
        #[cfg(target_arch = "wasm32")]
        {
            0
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
                .unwrap_or(0)
        }
    }

    fn normalize_source_location(&self, mut source: SourceLocation) -> SourceLocation {
        if let Some(root) = &self.source_display_root {
            source.file = stable_relative_path(&source.file, root);
        }
        source
    }

    fn format_source_location(&self, source: &SourceLocation) -> String {
        Self::format_source_location_with_root(&self.source_display_root, source)
    }

    fn format_source_location_with_root(root: &Option<PathBuf>, source: &SourceLocation) -> String {
        let mut source = source.clone();
        if let Some(root) = root {
            source.file = stable_relative_path(&source.file, root);
        }
        format!(
            "{}:{}:{}",
            source.file.display(),
            source.line,
            source.column
        )
    }

    fn json_resource_duplicate_kind(
        relative_path: &str,
        existing_json: &str,
        new_json: &str,
    ) -> &'static str {
        if existing_json == new_json {
            "exact duplicate"
        } else if Self::is_typed_tag_path(relative_path) {
            // This should not be reached for typed tags because they go
            // through merge_tag_resource, but keep the classification for
            // defensive callers.
            "invalid duplicate tag declaration"
        } else {
            "invalid overwrite"
        }
    }

    fn generated_resource_entries(&self) -> Vec<BuildManifestResourceEntry> {
        let mut entries = Vec::new();

        entries.extend(self.stdlib_function_tag_entries());
        entries.extend(self.custom_function_tag_entries());

        entries.extend(
            self.advancements
                .keys()
                .map(|name| self.resource_entry("advancement", name)),
        );
        entries.extend(
            self.loot_tables
                .keys()
                .map(|name| self.resource_entry("loot_table", name)),
        );
        entries.extend(
            self.recipes
                .keys()
                .map(|name| self.resource_entry("recipe", name)),
        );
        entries.extend(
            self.predicates
                .keys()
                .map(|name| self.resource_entry("predicate", name)),
        );
        entries.extend(
            self.item_modifiers
                .keys()
                .map(|name| self.resource_entry("item_modifier", name)),
        );

        for key in self.json_resources.keys() {
            let Some((namespace, path)) = Self::split_json_resource_key(key) else {
                continue;
            };
            entries.push(Self::resource_entry_from_json_path(namespace, path));
        }
        for key in self.resource_pack_models.keys() {
            let Some((namespace, path)) = Self::split_json_resource_key(key) else {
                continue;
            };
            entries.push(Self::resource_entry_from_asset_path(
                "resource_pack_model",
                namespace,
                path,
            ));
        }
        for key in self.resource_pack_langs.keys() {
            let Some((namespace, path)) = Self::split_json_resource_key(key) else {
                continue;
            };
            entries.push(Self::resource_entry_from_asset_path(
                "resource_pack_lang",
                namespace,
                path,
            ));
        }

        Self::sort_and_dedup_resource_entries(&mut entries);
        entries
    }

    fn stdlib_function_tag_entries(&self) -> Vec<BuildManifestResourceEntry> {
        let mut entries: Vec<_> = self
            .stdlib
            .generate_tags(&self.namespace)
            .keys()
            .map(|tag_name| {
                Self::resource_entry_from_tag_name("function_tag", &self.namespace, tag_name)
            })
            .collect();
        Self::sort_and_dedup_resource_entries(&mut entries);
        entries
    }

    fn custom_function_tag_entries(&self) -> Vec<BuildManifestResourceEntry> {
        let mut entries = Vec::new();

        for tag_name in self.tags.keys() {
            entries.push(Self::resource_entry_from_tag_name(
                "function_tag",
                &self.namespace,
                tag_name,
            ));
        }

        for key in self.json_resources.keys() {
            let Some((namespace, path)) = Self::split_json_resource_key(key) else {
                continue;
            };
            if let Some(path) = path.strip_prefix("tags/function/") {
                entries.push(BuildManifestResourceEntry {
                    kind: "function_tag".to_string(),
                    namespace: namespace.to_string(),
                    path: path.to_string(),
                });
            }
        }

        Self::sort_and_dedup_resource_entries(&mut entries);
        entries
    }

    fn sort_and_dedup_resource_entries(entries: &mut Vec<BuildManifestResourceEntry>) {
        entries.sort_by(|left, right| {
            (
                left.kind.as_str(),
                left.namespace.as_str(),
                left.path.as_str(),
            )
                .cmp(&(
                    right.kind.as_str(),
                    right.namespace.as_str(),
                    right.path.as_str(),
                ))
        });
        entries.dedup();
    }

    fn resource_entry(&self, kind: &str, path: &str) -> BuildManifestResourceEntry {
        BuildManifestResourceEntry {
            kind: kind.to_string(),
            namespace: self.namespace.clone(),
            path: path.to_string(),
        }
    }

    fn resource_entry_from_tag_name(
        kind: &str,
        default_namespace: &str,
        tag_name: &str,
    ) -> BuildManifestResourceEntry {
        let (namespace, path) = tag_name
            .split_once(':')
            .unwrap_or((default_namespace, tag_name));
        BuildManifestResourceEntry {
            kind: kind.to_string(),
            namespace: namespace.to_string(),
            path: path.to_string(),
        }
    }

    fn resource_entry_from_json_path(namespace: &str, path: &str) -> BuildManifestResourceEntry {
        let (kind, path) = if let Some(path) = path.strip_prefix("tags/function/") {
            ("function_tag", path)
        } else if let Some(path) = path.strip_prefix("tags/block/") {
            ("block_tag", path)
        } else if let Some(path) = path.strip_prefix("tags/item/") {
            ("item_tag", path)
        } else if let Some(path) = path.strip_prefix("tags/entity_type/") {
            ("entity_type_tag", path)
        } else if let Some(path) = path.strip_prefix("advancement/") {
            ("advancement", path)
        } else if let Some(path) = path.strip_prefix("loot_table/") {
            ("loot_table", path)
        } else if let Some(path) = path.strip_prefix("recipe/") {
            ("recipe", path)
        } else if let Some(path) = path.strip_prefix("predicate/") {
            ("predicate", path)
        } else if let Some(path) = path.strip_prefix("item_modifier/") {
            ("item_modifier", path)
        } else if let Some(path) = path.strip_prefix("dialog/") {
            ("dialog", path)
        } else {
            ("json_resource", path)
        };

        BuildManifestResourceEntry {
            kind: kind.to_string(),
            namespace: namespace.to_string(),
            path: path.to_string(),
        }
    }

    fn resource_entry_from_asset_path(
        kind: &str,
        namespace: &str,
        path: &str,
    ) -> BuildManifestResourceEntry {
        let path = path
            .strip_prefix("models/")
            .or_else(|| path.strip_prefix("lang/"))
            .unwrap_or(path);
        BuildManifestResourceEntry {
            kind: kind.to_string(),
            namespace: namespace.to_string(),
            path: path.to_string(),
        }
    }

    fn generated_counts_with_source_map(
        &self,
        source_map_entry_count: usize,
        resources: &[BuildManifestResourceEntry],
    ) -> BuildManifestGenerated {
        let mut json_advancement_count = 0;
        let mut json_loot_table_count = 0;
        let mut json_recipe_count = 0;
        let mut json_predicate_count = 0;
        let mut json_item_modifier_count = 0;
        let mut json_function_tag_count = 0;

        for key in self.json_resources.keys() {
            let Some((_, path)) = Self::split_json_resource_key(key) else {
                continue;
            };
            if path.starts_with("advancement/") {
                json_advancement_count += 1;
            } else if path.starts_with("loot_table/") {
                json_loot_table_count += 1;
            } else if path.starts_with("recipe/") {
                json_recipe_count += 1;
            } else if path.starts_with("predicate/") {
                json_predicate_count += 1;
            } else if path.starts_with("item_modifier/") {
                json_item_modifier_count += 1;
            } else if path.starts_with("tags/function/") {
                json_function_tag_count += 1;
            }
        }

        let advancement_count = self.advancements.len() + json_advancement_count;
        let loot_table_count = self.loot_tables.len() + json_loot_table_count;
        let recipe_count = self.recipes.len() + json_recipe_count;
        let predicate_count = self.predicates.len() + json_predicate_count;
        let item_modifier_count = self.item_modifiers.len() + json_item_modifier_count;
        let legacy_typed_json_resources = self.advancements.len()
            + self.loot_tables.len()
            + self.recipes.len()
            + self.predicates.len()
            + self.item_modifiers.len();

        let function_tag_count = resources
            .iter()
            .filter(|resource| resource.kind == "function_tag")
            .count();
        let stdlib_function_tags = self.stdlib_function_tag_entries();
        let stdlib_function_tag_set: HashSet<_> = stdlib_function_tags.iter().cloned().collect();
        let custom_function_tag_count = self
            .custom_function_tag_entries()
            .into_iter()
            .filter(|entry| !stdlib_function_tag_set.contains(entry))
            .count();

        BuildManifestGenerated {
            functions: self.functions.len(),
            commands: self.functions.values().map(Vec::len).sum(),
            source_map_entries: source_map_entry_count,
            function_tags: function_tag_count,
            stdlib_function_tags: stdlib_function_tags.len(),
            custom_function_tags: custom_function_tag_count,
            json_function_tags: json_function_tag_count,
            advancements: advancement_count,
            loot_tables: loot_table_count,
            recipes: recipe_count,
            predicates: predicate_count,
            item_modifiers: item_modifier_count,
            json_resources: self.json_resources.len(),
            total_json_resources: legacy_typed_json_resources + self.json_resources.len(),
            resource_pack_models: self.resource_pack_models.len(),
            resource_pack_langs: self.resource_pack_langs.len(),
        }
    }

    fn is_safe_namespace_path(namespace: &str) -> bool {
        !namespace.is_empty()
            && namespace != "."
            && namespace != ".."
            && namespace.chars().all(|c| {
                c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-' || c == '.'
            })
    }

    fn is_safe_resource_path(path: &str) -> bool {
        !path.is_empty()
            && !path.contains('\\')
            && !path.contains(':')
            && path.split('/').all(|segment| {
                !segment.is_empty()
                    && segment != "."
                    && segment != ".."
                    && segment.chars().all(|c| {
                        c.is_ascii_lowercase()
                            || c.is_ascii_digit()
                            || c == '_'
                            || c == '-'
                            || c == '.'
                    })
            })
    }

    fn invalid_path_error(message: impl Into<String>) -> std::io::Error {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, message.into())
    }

    fn validate_namespace_for_write(namespace: &str, label: &str) -> std::io::Result<()> {
        if Self::is_safe_namespace_path(namespace) {
            Ok(())
        } else {
            Err(Self::invalid_path_error(format!(
                "Invalid {label} namespace `{namespace}`: use lowercase letters, digits, underscores, hyphens, and dots only"
            )))
        }
    }

    fn validate_resource_path_for_write(path: &str, label: &str) -> std::io::Result<()> {
        if Self::is_safe_resource_path(path) {
            Ok(())
        } else {
            Err(Self::invalid_path_error(format!(
                "Invalid {label} path `{path}`: use lowercase resource paths with '/', '_', '-', or '.', and no empty, '.', or '..' segments"
            )))
        }
    }

    fn validate_write_paths(&self) -> std::io::Result<()> {
        Self::validate_namespace_for_write(&self.namespace, "data pack")?;

        for function_name in self.functions.keys() {
            Self::validate_resource_path_for_write(function_name, "function")?;
        }

        for tag_name in self.tags.keys() {
            let (namespace, path) = tag_name
                .split_once(':')
                .unwrap_or((&self.namespace, tag_name.as_str()));
            Self::validate_namespace_for_write(namespace, "function tag")?;
            Self::validate_resource_path_for_write(path, "function tag")?;
        }

        for (label, resources) in [
            ("advancement", &self.advancements),
            ("loot table", &self.loot_tables),
            ("recipe", &self.recipes),
            ("predicate", &self.predicates),
            ("item modifier", &self.item_modifiers),
        ] {
            for name in resources.keys() {
                Self::validate_resource_path_for_write(name, label)?;
            }
        }

        for key in self.json_resources.keys() {
            let Some((namespace, relative_path)) = Self::split_json_resource_key(key) else {
                return Err(Self::invalid_path_error(format!(
                    "Invalid JSON resource key `{key}`"
                )));
            };
            Self::validate_namespace_for_write(namespace, "JSON resource")?;
            Self::validate_resource_path_for_write(relative_path, "JSON resource")?;
        }

        for key in self.resource_pack_models.keys() {
            let Some((namespace, relative_path)) = Self::split_json_resource_key(key) else {
                return Err(Self::invalid_path_error(format!(
                    "Invalid resource-pack model key `{key}`"
                )));
            };
            Self::validate_namespace_for_write(namespace, "resource-pack model")?;
            Self::validate_resource_path_for_write(relative_path, "resource-pack model")?;
        }

        for key in self.resource_pack_langs.keys() {
            let Some((namespace, relative_path)) = Self::split_json_resource_key(key) else {
                return Err(Self::invalid_path_error(format!(
                    "Invalid resource-pack language key `{key}`"
                )));
            };
            Self::validate_namespace_for_write(namespace, "resource-pack language")?;
            Self::validate_resource_path_for_write(relative_path, "resource-pack language")?;
        }

        for namespace in self.generated_namespaces() {
            Self::validate_namespace_for_write(&namespace, "generated")?;
        }
        for namespace in self.generated_asset_namespaces() {
            Self::validate_namespace_for_write(&namespace, "generated asset")?;
        }

        Ok(())
    }

    fn json_resource_key(namespace: &str, relative_path: &str) -> String {
        format!("{}/{}", namespace, relative_path)
    }

    fn split_json_resource_key(key: &str) -> Option<(&str, &str)> {
        key.split_once('/')
    }

    fn generated_namespaces(&self) -> Vec<String> {
        let mut namespaces = HashSet::new();
        namespaces.insert(self.namespace.clone());
        namespaces.insert("minecraft".to_string());
        for key in self.json_resources.keys() {
            if let Some((namespace, _)) = Self::split_json_resource_key(key) {
                namespaces.insert(namespace.to_string());
            }
        }
        for tag_name in self.tags.keys() {
            if let Some((namespace, _)) = tag_name.split_once(':') {
                namespaces.insert(namespace.to_string());
            }
        }
        let mut namespaces = namespaces.into_iter().collect::<Vec<_>>();
        namespaces.sort();
        namespaces
    }

    fn generated_asset_namespaces(&self) -> Vec<String> {
        let mut namespaces = HashSet::new();
        for key in self
            .resource_pack_models
            .keys()
            .chain(self.resource_pack_langs.keys())
        {
            if let Some((namespace, _)) = Self::split_json_resource_key(key) {
                namespaces.insert(namespace.to_string());
            }
        }
        let mut namespaces = namespaces.into_iter().collect::<Vec<_>>();
        namespaces.sort();
        namespaces
    }

    fn clean_generated_resource_dirs(namespace_dir: &Path) -> std::io::Result<()> {
        let tags_dir = namespace_dir.join("tags");
        if tags_dir.exists() {
            fs::remove_dir_all(tags_dir)?;
        }

        for resource_dir in [
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
            let path = namespace_dir.join(resource_dir);
            if path.exists() {
                fs::remove_dir_all(path)?;
            }
        }
        Ok(())
    }

    fn clean_generated_function_dirs(namespace_dir: &Path) -> std::io::Result<()> {
        for function_dir in ["function", "functions"] {
            let path = namespace_dir.join(function_dir);
            if path.exists() {
                fs::remove_dir_all(path)?;
            }
        }
        Ok(())
    }

    fn clean_generated_asset_dirs(namespace_dir: &Path) -> std::io::Result<()> {
        for asset_dir in ["models", "lang"] {
            let path = namespace_dir.join(asset_dir);
            if path.exists() {
                fs::remove_dir_all(path)?;
            }
        }
        Ok(())
    }

    fn merge_tag_json_if_existing(
        file_path: &Path,
        relative_path: &str,
        new_json: &str,
    ) -> std::io::Result<String> {
        if !relative_path.starts_with("tags/") || !file_path.exists() {
            return Ok(new_json.to_string());
        }

        let Ok(existing_content) = fs::read_to_string(file_path) else {
            return Ok(new_json.to_string());
        };
        let Ok(mut merged_value) = serde_json::from_str::<serde_json::Value>(&existing_content)
        else {
            return Ok(new_json.to_string());
        };
        let Ok(new_value) = serde_json::from_str::<serde_json::Value>(new_json) else {
            return Ok(new_json.to_string());
        };

        let Some(merged_values) = merged_value
            .get_mut("values")
            .and_then(|value| value.as_array_mut())
        else {
            return Ok(new_json.to_string());
        };
        let Some(new_values) = new_value.get("values").and_then(|value| value.as_array()) else {
            return Ok(new_json.to_string());
        };

        for new_value in new_values {
            if !merged_values.contains(new_value) {
                merged_values.push(new_value.clone());
            }
        }

        serde_json::to_string_pretty(&merged_value)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
    }

    fn write_function_tag(
        data_dir: &Path,
        default_namespace: &str,
        tag_name: &str,
        functions: &[String],
    ) -> std::io::Result<()> {
        let (namespace, relative_tag_name) =
            if let Some((namespace, path)) = tag_name.split_once(':') {
                (namespace, path)
            } else {
                (default_namespace, tag_name)
            };
        Self::validate_namespace_for_write(namespace, "function tag")?;
        Self::validate_resource_path_for_write(relative_tag_name, "function tag")?;
        let tag_file = data_dir
            .join(namespace)
            .join("tags")
            .join("function")
            .join(format!("{}.json", relative_tag_name));

        let tag_content = json!({
            "values": functions
        });

        if let Some(parent) = tag_file.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = fs::File::create(tag_file)?;
        writeln!(
            file,
            "{}",
            serde_json::to_string_pretty(&tag_content).unwrap()
        )?;
        Ok(())
    }

    fn write_json_resource_file(
        resource_dir: &Path,
        name: &str,
        json: &str,
    ) -> std::io::Result<()> {
        let file_path = resource_dir.join(format!("{}.json", name));
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(file_path, json)
    }

    fn write_json_resource_map(
        resource_dir: &Path,
        resources: &HashMap<String, String>,
    ) -> std::io::Result<()> {
        let mut names: Vec<_> = resources.keys().collect();
        names.sort();
        for name in names {
            Self::write_json_resource_file(resource_dir, name, &resources[name])?;
        }
        Ok(())
    }

    fn write_resource_pack_assets(&self, assets_dir: &Path) -> std::io::Result<()> {
        let mut model_paths: Vec<_> = self.resource_pack_models.keys().collect();
        model_paths.sort();
        for key in model_paths {
            let Some((namespace, relative_path)) = Self::split_json_resource_key(key) else {
                continue;
            };
            Self::write_asset_json_file(
                assets_dir,
                namespace,
                relative_path,
                &self.resource_pack_models[key],
            )?;
        }

        let mut lang_paths: Vec<_> = self.resource_pack_langs.keys().collect();
        lang_paths.sort();
        for key in lang_paths {
            let Some((namespace, relative_path)) = Self::split_json_resource_key(key) else {
                continue;
            };
            Self::write_asset_json_file(
                assets_dir,
                namespace,
                relative_path,
                &self.resource_pack_langs[key],
            )?;
        }

        Ok(())
    }

    fn write_asset_json_file(
        assets_dir: &Path,
        namespace: &str,
        relative_path: &str,
        json: &str,
    ) -> std::io::Result<()> {
        let file_path = assets_dir
            .join(namespace)
            .join(format!("{}.json", relative_path));
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(file_path, json)
    }

    fn write_pack_mcmeta(&self) -> std::io::Result<()> {
        let mcmeta_path = self.output_dir.join("pack.mcmeta");

        let mcmeta_content = match self.pack_format {
            PackFormat::Decimal(major, minor) => {
                json!({
                    "pack": {
                        "description": self.description,
                        "min_format": [major, minor],
                        "max_format": [major, minor]
                    }
                })
            }
            PackFormat::Integer(v) => {
                json!({
                    "pack": {
                        "pack_format": v,
                        "description": self.description
                    }
                })
            }
        };

        let mut file = fs::File::create(mcmeta_path)?;
        writeln!(
            file,
            "{}",
            serde_json::to_string_pretty(&mcmeta_content).unwrap()
        )?;
        Ok(())
    }

    pub fn ensure_math_helper(&mut self, op: &str) {
        let func_name = format!("_cobble_math_{}", op);
        if self.functions.contains_key(&func_name) {
            return;
        }

        let mut commands = Vec::new();
        match op {
            "abs" => {
                commands.push(
                    "scoreboard players operation #math_result math = #math_input math".to_string(),
                );
                commands.push("scoreboard players set #math_temp math -1".to_string());
                commands.push("execute if score #math_result math matches ..-1 run scoreboard players operation #math_result math *= #math_temp math".to_string());
            }
            "min" => {
                commands.push(
                    "scoreboard players operation #math_result math = #math_input math".to_string(),
                );
                commands.push("execute if score #math_input2 math < #math_result math run scoreboard players operation #math_result math = #math_input2 math".to_string());
            }
            "max" => {
                commands.push(
                    "scoreboard players operation #math_result math = #math_input math".to_string(),
                );
                commands.push("execute if score #math_input2 math > #math_result math run scoreboard players operation #math_result math = #math_input2 math".to_string());
            }
            "sqrt" => {
                commands.push("scoreboard players set #math_result math 0".to_string());
                commands.push(
                    "scoreboard players operation #sqrt_high math = #math_input math".to_string(),
                );
                commands.push("scoreboard players set #sqrt_low math 0".to_string());
                commands.push("scoreboard players set #sqrt_limit math 46340".to_string());
                commands.push("scoreboard players set #sqrt_two math 2".to_string());
                commands.push(
                    "execute if score #sqrt_high math matches ..-1 run scoreboard players set #sqrt_high math 0"
                        .to_string(),
                );
                commands.push(
                    "execute if score #sqrt_high math > #sqrt_limit math run scoreboard players operation #sqrt_high math = #sqrt_limit math"
                        .to_string(),
                );

                for _ in 0..16 {
                    commands.push(
                        "scoreboard players operation #sqrt_mid math = #sqrt_low math".to_string(),
                    );
                    commands.push(
                        "scoreboard players operation #sqrt_mid math += #sqrt_high math"
                            .to_string(),
                    );
                    commands.push(
                        "scoreboard players operation #sqrt_mid math /= #sqrt_two math".to_string(),
                    );
                    commands.push(
                        "scoreboard players operation #sqrt_square math = #sqrt_mid math"
                            .to_string(),
                    );
                    commands.push(
                        "scoreboard players operation #sqrt_square math *= #sqrt_mid math"
                            .to_string(),
                    );
                    commands.push(
                        "execute if score #sqrt_square math <= #math_input math run scoreboard players operation #math_result math = #sqrt_mid math"
                            .to_string(),
                    );
                    commands.push(
                        "execute if score #sqrt_square math <= #math_input math run scoreboard players operation #sqrt_low math = #sqrt_mid math"
                            .to_string(),
                    );
                    commands.push(
                        "execute if score #sqrt_square math <= #math_input math run scoreboard players add #sqrt_low math 1"
                            .to_string(),
                    );
                    commands.push(
                        "execute if score #sqrt_square math > #math_input math run scoreboard players operation #sqrt_high math = #sqrt_mid math"
                            .to_string(),
                    );
                    commands.push(
                        "execute if score #sqrt_square math > #math_input math run scoreboard players remove #sqrt_high math 1"
                            .to_string(),
                    );
                }
            }
            _ => {}
        }

        self.add_function_with_kind(func_name, commands, GeneratedCommandKind::StdLib);
    }
}
