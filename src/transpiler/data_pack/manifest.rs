use super::{BuildManifest, BuildManifestGenerated, BuildManifestResourceEntry, DataPack};
use crate::pack_format::{COBBLE_VERSION, SUPPORTED_MINECRAFT_VERSION};
use std::collections::HashSet;

impl DataPack {
    pub(in crate::transpiler::data_pack) fn build_manifest(
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

    pub(in crate::transpiler::data_pack) fn generated_resource_entries(
        &self,
    ) -> Vec<BuildManifestResourceEntry> {
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
        for key in &self.resource_pack_static_assets {
            let Some((namespace, path)) = Self::split_json_resource_key(key) else {
                continue;
            };
            entries.push(BuildManifestResourceEntry {
                kind: "resource_pack_static_asset".to_string(),
                namespace: namespace.to_string(),
                path: path.to_string(),
            });
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
        } else if let Some(path) = path.strip_prefix("tags/dialog/") {
            ("dialog_tag", path)
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

    pub(in crate::transpiler::data_pack) fn generated_counts_with_source_map(
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
            resource_pack_static_assets: self.resource_pack_static_assets.len(),
        }
    }
}
