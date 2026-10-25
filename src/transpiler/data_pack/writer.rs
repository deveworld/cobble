use super::{DataPack, GeneratedCommand, GeneratedCommandKind, SourceMap, SourceMapEntry};
use crate::fs_safety::write_file_atomic;
use crate::pack_format::PackFormat;
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

impl DataPack {
    pub fn write(&self) -> std::io::Result<()> {
        self.validate_write_paths()?;
        let generated_namespaces = self.generated_namespaces();
        let generated_asset_namespaces = self.generated_asset_namespaces();
        self.remove_previous_output(&generated_namespaces, &generated_asset_namespaces)?;

        let function_dir = self.namespace_dir().join("function");
        fs::create_dir_all(&function_dir)?;
        self.write_pack_mcmeta()?;

        let stdlib_tags = self.stdlib.generate_tags(&self.namespace);
        let mut source_map_entries = self.write_functions(&function_dir)?;
        self.extend_json_resource_source_map_entries(&mut source_map_entries, &stdlib_tags);
        self.write_metadata(
            source_map_entries,
            &generated_namespaces,
            &generated_asset_namespaces,
        )?;

        let data_dir = self.output_dir.join("data");
        for (tag_name, functions) in &stdlib_tags {
            Self::write_function_tag(&data_dir, &self.namespace, tag_name, functions)?;
        }
        let mut custom_tag_names: Vec<_> = self.tags.keys().collect();
        custom_tag_names.sort();
        for tag_name in custom_tag_names {
            Self::write_function_tag(&data_dir, &self.namespace, tag_name, &self.tags[tag_name])?;
        }

        for (folder, resources) in [
            ("advancement", &self.advancements),
            ("loot_table", &self.loot_tables),
            ("recipe", &self.recipes),
            ("predicate", &self.predicates),
            ("item_modifier", &self.item_modifiers),
        ] {
            if !resources.is_empty() {
                Self::write_json_resource_map(&self.namespace_dir().join(folder), resources)?;
            }
        }
        self.write_declared_json_resources(&data_dir)?;

        self.write_resource_pack_assets(&self.output_dir.join("assets"))
    }

    fn namespace_dir(&self) -> std::path::PathBuf {
        self.output_dir.join("data").join(&self.namespace)
    }

    /// Removes what an earlier build generated, so deleted or renamed
    /// functions, declarations, and namespaces don't stay in the pack.
    fn remove_previous_output(
        &self,
        generated_namespaces: &[String],
        generated_asset_namespaces: &[String],
    ) -> std::io::Result<()> {
        let data_dir = self.output_dir.join("data");
        let assets_dir = self.output_dir.join("assets");
        let metadata_dir = self.output_dir.join(".cobble");

        for namespace in Self::previous_namespaces(&metadata_dir.join("generated_namespaces.json"))
        {
            if !generated_namespaces.contains(&namespace) {
                remove_dir_if_exists(&data_dir.join(namespace))?;
            } else if namespace != self.namespace {
                Self::clean_generated_function_dirs(&data_dir.join(namespace))?;
            }
        }
        if self.clean_previous_resource_pack_outputs {
            for namespace in
                Self::previous_namespaces(&metadata_dir.join("generated_asset_namespaces.json"))
            {
                if !generated_asset_namespaces.contains(&namespace) {
                    remove_dir_if_exists(&assets_dir.join(namespace))?;
                } else {
                    Self::clean_generated_asset_dirs(&assets_dir.join(namespace))?;
                }
            }
        }

        let namespace_dir = self.namespace_dir();
        let minecraft_function_tags = data_dir.join("minecraft").join("tags");
        for dir in [
            namespace_dir.join("function"),
            namespace_dir.join("functions"),
            metadata_dir,
            minecraft_function_tags.join("function"),
            minecraft_function_tags.join("functions"),
        ] {
            remove_dir_if_exists(&dir)?;
        }
        for namespace in generated_namespaces {
            Self::clean_generated_resource_dirs(&data_dir.join(namespace))?;
        }
        if self.clean_previous_resource_pack_outputs {
            for namespace in generated_asset_namespaces {
                Self::clean_generated_asset_dirs(&assets_dir.join(namespace))?;
            }
        }
        Ok(())
    }

    /// Namespaces an earlier build recorded in `path`. Unsafe names are
    /// skipped, so a tampered record can't point outside the pack.
    fn previous_namespaces(path: &Path) -> Vec<String> {
        fs::read_to_string(path)
            .ok()
            .and_then(|content| serde_json::from_str::<Vec<String>>(&content).ok())
            .unwrap_or_default()
            .into_iter()
            .filter(|namespace| Self::is_safe_namespace_path(namespace))
            .collect()
    }

    /// Writes each function in name order and returns its source-map entries.
    fn write_functions(&self, function_dir: &Path) -> std::io::Result<Vec<SourceMapEntry>> {
        let mut source_map_entries = Vec::new();
        let mut function_names: Vec<_> = self.functions.keys().collect();
        function_names.sort();
        for name in function_names {
            let commands = &self.functions[name];
            let file_path = function_dir.join(format!("{}.mcfunction", name));
            let generated_path = format!("data/{}/function/{}.mcfunction", self.namespace, name);
            let mut file_content = String::new();
            for (index, command) in commands.iter().enumerate() {
                file_content.push_str(command);
                file_content.push('\n');
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
            write_file_atomic(&file_path, file_content)?;
        }
        Ok(source_map_entries)
    }

    /// Writes `.cobble/`: the source map, the generated namespace records,
    /// and the build manifest.
    fn write_metadata(
        &self,
        source_map_entries: Vec<SourceMapEntry>,
        generated_namespaces: &[String],
        generated_asset_namespaces: &[String],
    ) -> std::io::Result<()> {
        let metadata_dir = self.output_dir.join(".cobble");
        let source_map_entry_count = source_map_entries.len();
        fs::create_dir_all(&metadata_dir)?;
        if !source_map_entries.is_empty() {
            let source_map = SourceMap {
                version: 1,
                entries: source_map_entries,
            };
            write_file_atomic(
                &metadata_dir.join("source_map.json"),
                serde_json::to_string_pretty(&source_map).unwrap(),
            )?;
        }
        write_file_atomic(
            &metadata_dir.join("generated_namespaces.json"),
            serde_json::to_string_pretty(&generated_namespaces).unwrap(),
        )?;
        if !generated_asset_namespaces.is_empty() {
            write_file_atomic(
                &metadata_dir.join("generated_asset_namespaces.json"),
                serde_json::to_string_pretty(&generated_asset_namespaces).unwrap(),
            )?;
        }
        let build_manifest = self.build_manifest(source_map_entry_count, generated_namespaces);
        write_file_atomic(
            &metadata_dir.join("build_manifest.json"),
            serde_json::to_string_pretty(&build_manifest).unwrap(),
        )
    }

    /// Writes the JSON resources from `datapack.*` declarations, merging
    /// tags with tag files already written in this build.
    fn write_declared_json_resources(&self, data_dir: &Path) -> std::io::Result<()> {
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
            let content = self
                .order_load_tag_json(resource_namespace, relative_path, content)
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
            write_file_atomic(&file_path, content)?;
        }
        Ok(())
    }

    pub(in crate::transpiler::data_pack) fn clean_generated_resource_dirs(
        namespace_dir: &Path,
    ) -> std::io::Result<()> {
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

    pub(in crate::transpiler::data_pack) fn clean_generated_function_dirs(
        namespace_dir: &Path,
    ) -> std::io::Result<()> {
        for function_dir in ["function", "functions"] {
            let path = namespace_dir.join(function_dir);
            if path.exists() {
                fs::remove_dir_all(path)?;
            }
        }
        Ok(())
    }

    pub(in crate::transpiler::data_pack) fn clean_generated_asset_dirs(
        namespace_dir: &Path,
    ) -> std::io::Result<()> {
        for asset_dir in ["models", "items", "lang"] {
            let path = namespace_dir.join(asset_dir);
            if path.exists() {
                fs::remove_dir_all(path)?;
            }
        }
        Ok(())
    }

    pub(in crate::transpiler::data_pack) fn merge_tag_json_if_existing(
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
        Self::merge_tag_json(&existing_content, new_json)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
    }

    /// Merge a tag file written earlier in this build (`existing_json`) with
    /// another declaration of the same tag. Shared by the CLI writer and the
    /// in-memory web compiler so both produce identical tag files. Input that
    /// is not tag-shaped JSON leaves `new_json` unchanged.
    pub fn merge_tag_json(existing_json: &str, new_json: &str) -> Result<String, String> {
        let Ok(mut merged_value) = serde_json::from_str::<serde_json::Value>(existing_json) else {
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

        // Run both value lists through the same normalize + sort + dedup
        // pipeline as the in-memory typed tag merge (tags.rs) so the on-disk
        // merge produces identical, stable output across rebuilds.
        let mut combined = Self::normalize_tag_values_lossy(merged_values);
        combined.extend(Self::normalize_tag_values_lossy(new_values));
        Self::sort_and_dedup_tag_values(&mut combined);
        *merged_values = combined;

        let replace = merged_value
            .get("replace")
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
            || new_value
                .get("replace")
                .and_then(|value| value.as_bool())
                .unwrap_or(false);
        if replace {
            if let Some(object) = merged_value.as_object_mut() {
                object.insert("replace".to_string(), serde_json::Value::Bool(true));
            }
        }

        serde_json::to_string_pretty(&merged_value).map_err(|error| error.to_string())
    }

    pub(in crate::transpiler::data_pack) fn write_function_tag(
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
        write_file_atomic(
            &tag_file,
            format!("{}\n", serde_json::to_string_pretty(&tag_content).unwrap()),
        )?;
        Ok(())
    }

    pub(in crate::transpiler::data_pack) fn write_json_resource_file(
        resource_dir: &Path,
        name: &str,
        json: &str,
    ) -> std::io::Result<()> {
        let file_path = resource_dir.join(format!("{}.json", name));
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent)?;
        }
        write_file_atomic(&file_path, json)
    }

    pub(in crate::transpiler::data_pack) fn write_json_resource_map(
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

    pub(in crate::transpiler::data_pack) fn write_resource_pack_assets(
        &self,
        assets_dir: &Path,
    ) -> std::io::Result<()> {
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
        write_file_atomic(&file_path, json)
    }

    pub(in crate::transpiler::data_pack) fn write_pack_mcmeta(&self) -> std::io::Result<()> {
        let mcmeta_path = self.output_dir.join("pack.mcmeta");
        write_file_atomic(
            &mcmeta_path,
            format!(
                "{}\n",
                serde_json::to_string_pretty(&self.pack_mcmeta_json()).unwrap()
            ),
        )?;
        Ok(())
    }

    /// `pack.mcmeta` contents for this pack's description and pack format.
    /// Shared by the CLI writer and the in-memory web compiler.
    pub fn pack_mcmeta_json(&self) -> serde_json::Value {
        match self.pack_format {
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
        }
    }
}

fn remove_dir_if_exists(path: &Path) -> std::io::Result<()> {
    if path.exists() {
        fs::remove_dir_all(path)?;
    }
    Ok(())
}
