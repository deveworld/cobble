use super::{DataPack, GeneratedCommandKind, SourceLocation, SourceMapEntry};
use serde_json::json;
use std::collections::HashMap;

impl DataPack {
    pub(in crate::transpiler::data_pack) fn source_map_entry_count_snapshot(&self) -> usize {
        let mut json_entries = Vec::new();
        let stdlib_tags = self.stdlib.generate_tags(&self.namespace);
        self.extend_json_resource_source_map_entries(&mut json_entries, &stdlib_tags);
        self.functions.values().map(Vec::len).sum::<usize>() + json_entries.len()
    }

    pub(in crate::transpiler::data_pack) fn extend_json_resource_source_map_entries(
        &self,
        entries: &mut Vec<SourceMapEntry>,
        stdlib_tags: &HashMap<String, Vec<String>>,
    ) {
        let mut stdlib_tag_names: Vec<_> = stdlib_tags.keys().collect();
        stdlib_tag_names.sort();
        for tag_name in stdlib_tag_names {
            let Some((path, json)) =
                self.function_tag_source_map_data(tag_name, &stdlib_tags[tag_name])
            else {
                continue;
            };
            entries.push(self.json_source_map_entry(path, &json, None));
        }

        let mut custom_tag_names: Vec<_> = self.tags.keys().collect();
        custom_tag_names.sort();
        for tag_name in custom_tag_names {
            let Some((path, json)) =
                self.function_tag_source_map_data(tag_name, &self.tags[tag_name])
            else {
                continue;
            };
            entries.push(self.json_source_map_entry(path, &json, None));
        }

        self.extend_json_resource_map_source_entries(
            entries,
            "advancement",
            &self.advancements,
            &HashMap::new(),
            true,
        );
        self.extend_json_resource_map_source_entries(
            entries,
            "loot_table",
            &self.loot_tables,
            &HashMap::new(),
            true,
        );
        self.extend_json_resource_map_source_entries(
            entries,
            "recipe",
            &self.recipes,
            &HashMap::new(),
            true,
        );
        self.extend_json_resource_map_source_entries(
            entries,
            "predicate",
            &self.predicates,
            &HashMap::new(),
            true,
        );
        self.extend_json_resource_map_source_entries(
            entries,
            "item_modifier",
            &self.item_modifiers,
            &HashMap::new(),
            true,
        );

        self.extend_json_resource_map_source_entries(
            entries,
            "",
            &self.json_resources,
            &self.json_resource_origins,
            false,
        );
        self.extend_asset_resource_map_source_entries(
            entries,
            &self.resource_pack_models,
            &self.resource_pack_resource_origins,
        );
        self.extend_asset_resource_map_source_entries(
            entries,
            &self.resource_pack_langs,
            &self.resource_pack_resource_origins,
        );
    }

    fn function_tag_source_map_data(
        &self,
        tag_name: &str,
        functions: &[String],
    ) -> Option<(String, String)> {
        let (namespace, relative_tag_name) = tag_name
            .split_once(':')
            .unwrap_or((&self.namespace, tag_name));
        if Self::validate_namespace_for_write(namespace, "function tag").is_err()
            || Self::validate_resource_path_for_write(relative_tag_name, "function tag").is_err()
        {
            return None;
        }
        let json = serde_json::to_string_pretty(&json!({ "values": functions })).ok()?;
        Some((
            format!("data/{namespace}/tags/function/{relative_tag_name}.json"),
            json,
        ))
    }

    fn extend_json_resource_map_source_entries(
        &self,
        entries: &mut Vec<SourceMapEntry>,
        resource_dir: &str,
        resources: &HashMap<String, String>,
        origins: &HashMap<String, Vec<SourceLocation>>,
        default_namespace: bool,
    ) {
        let mut keys: Vec<_> = resources.keys().collect();
        keys.sort();
        for key in keys {
            let (namespace, relative_path) = if default_namespace {
                (self.namespace.as_str(), key.as_str())
            } else {
                let Some(parts) = Self::split_json_resource_key(key) else {
                    continue;
                };
                parts
            };
            let generated_path = if resource_dir.is_empty() {
                format!("data/{namespace}/{relative_path}.json")
            } else {
                format!("data/{namespace}/{resource_dir}/{relative_path}.json")
            };
            let source = origins
                .get(key)
                .and_then(|sources| sources.first())
                .cloned();
            entries.push(self.json_source_map_entry(
                generated_path,
                resources[key].as_str(),
                source,
            ));
        }
    }

    fn extend_asset_resource_map_source_entries(
        &self,
        entries: &mut Vec<SourceMapEntry>,
        resources: &HashMap<String, String>,
        origins: &HashMap<String, Vec<SourceLocation>>,
    ) {
        let mut keys: Vec<_> = resources.keys().collect();
        keys.sort();
        for key in keys {
            let Some((namespace, relative_path)) = Self::split_json_resource_key(key) else {
                continue;
            };
            let source = origins
                .get(key)
                .and_then(|sources| sources.first())
                .cloned();
            entries.push(self.json_source_map_entry(
                format!("assets/{namespace}/{relative_path}.json"),
                resources[key].as_str(),
                source,
            ));
        }
    }

    fn json_source_map_entry(
        &self,
        generated_path: String,
        json: &str,
        source: Option<SourceLocation>,
    ) -> SourceMapEntry {
        SourceMapEntry {
            generated_path,
            generated_line: 1,
            command: json.lines().next().unwrap_or("").to_string(),
            source: source.map(|source| self.normalize_source_location(source)),
            kind: GeneratedCommandKind::JsonGenerated,
        }
    }
}
