use super::{DataPack, SourceLocation};
use std::collections::BTreeMap;

impl DataPack {
    pub fn add_resource_pack_model_in_namespace_with_source(
        &mut self,
        namespace: String,
        relative_path: String,
        json: String,
        source: Option<SourceLocation>,
    ) -> Result<(), String> {
        let key = Self::json_resource_key(&namespace, &relative_path);
        if let Some(existing_json) = self.resource_pack_models.get(&key) {
            if existing_json == &json {
                if let Some(source) = source {
                    self.resource_pack_resource_origins
                        .entry(key)
                        .or_default()
                        .push(source);
                }
                return Ok(());
            }

            let mut message = format!(
                "Duplicate resource pack model '{}:{}' (invalid overwrite)",
                namespace, relative_path
            );
            let declarations = [
                (
                    "first",
                    self.resource_pack_resource_origins
                        .get(&key)
                        .and_then(|origins| origins.first()),
                ),
                ("second", source.as_ref()),
            ];
            for (label, location) in declarations {
                if let Some(location) = location {
                    message.push_str(&format!(
                        "\n  {} declaration: {}",
                        label,
                        self.format_source_location(location)
                    ));
                }
            }
            return Err(message);
        }

        self.resource_pack_models.insert(key.clone(), json);
        if let Some(source) = source {
            self.resource_pack_resource_origins
                .insert(key, vec![source]);
        }
        Ok(())
    }

    /// Whether a `resource_pack_models` path is a generated item model
    /// definition (`items/<name>`) rather than a model (`models/...`).
    pub(in crate::transpiler::data_pack) fn is_item_definition_path(relative_path: &str) -> bool {
        relative_path.starts_with("items/")
    }

    pub(in crate::transpiler::data_pack) fn is_item_definition_key(key: &str) -> bool {
        Self::split_json_resource_key(key)
            .is_some_and(|(_, relative_path)| Self::is_item_definition_path(relative_path))
    }

    /// Drops generated item model definitions (`items/<name>`) that a static
    /// project asset replaces. `static_assets` holds paths relative to
    /// `assets/`, such as `mypack/items/custom_sword.json`.
    pub fn remove_item_definitions_replaced_by(
        &mut self,
        static_assets: &std::collections::HashSet<std::path::PathBuf>,
    ) {
        let replaced: Vec<String> = self
            .resource_pack_models
            .keys()
            .filter(|key| {
                Self::split_json_resource_key(key).is_some_and(|(namespace, relative_path)| {
                    Self::is_item_definition_path(relative_path)
                        && static_assets.contains(
                            &std::path::Path::new(namespace).join(format!("{relative_path}.json")),
                        )
                })
            })
            .cloned()
            .collect();
        for key in replaced {
            self.resource_pack_models.remove(&key);
            self.resource_pack_resource_origins.remove(&key);
        }
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
        self.merge_resource_pack_lang(&key, &namespace, &relative_path, &json, source)
    }

    fn merge_resource_pack_lang(
        &mut self,
        key: &str,
        namespace: &str,
        relative_path: &str,
        new_json: &str,
        source: Option<SourceLocation>,
    ) -> Result<(), String> {
        let new_entries = Self::parse_resource_pack_lang_json(namespace, relative_path, new_json)?;
        let merged_entries = if let Some(existing_json) = self.resource_pack_langs.get(key) {
            let mut merged_entries =
                Self::parse_resource_pack_lang_json(namespace, relative_path, existing_json)?;
            for (translation_key, new_value) in new_entries {
                match merged_entries.get(&translation_key) {
                    Some(existing_value) if existing_value == &new_value => {}
                    Some(_) => {
                        let mut message = format!(
                            "Duplicate resource pack language file '{}:{}' \
                             (invalid overwrite for translation key '{}')",
                            namespace, relative_path, translation_key
                        );
                        if let Some(first_source) = self
                            .resource_pack_resource_origins
                            .get(key)
                            .and_then(|v| v.first())
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
                    None => {
                        merged_entries.insert(translation_key, new_value);
                    }
                }
            }
            merged_entries
        } else {
            new_entries
        };

        let merged_json = Self::serialize_resource_pack_lang_json(&merged_entries)?;
        self.resource_pack_langs
            .insert(key.to_string(), merged_json);
        if let Some(source) = source {
            self.resource_pack_resource_origins
                .entry(key.to_string())
                .or_default()
                .push(source);
        }
        Ok(())
    }

    fn parse_resource_pack_lang_json(
        namespace: &str,
        relative_path: &str,
        json: &str,
    ) -> Result<BTreeMap<String, String>, String> {
        let value: serde_json::Value = serde_json::from_str(json).map_err(|e| {
            format!(
                "Failed to parse resource-pack language JSON for '{}:{}': {}",
                namespace, relative_path, e
            )
        })?;
        let Some(object) = value.as_object() else {
            return Err(format!(
                "Invalid resource-pack language file '{}:{}': top-level value must be an object",
                namespace, relative_path
            ));
        };

        let mut entries = BTreeMap::new();
        for (translation_key, value) in object {
            let Some(value) = value.as_str() else {
                return Err(format!(
                    "Invalid resource-pack language file '{}:{}': value for '{}' must be a string",
                    namespace, relative_path, translation_key
                ));
            };
            entries.insert(translation_key.clone(), value.to_string());
        }
        Ok(entries)
    }

    fn serialize_resource_pack_lang_json(
        entries: &BTreeMap<String, String>,
    ) -> Result<String, String> {
        serde_json::to_string_pretty(entries)
            .map_err(|e| format!("Failed to serialize merged resource-pack lang JSON: {}", e))
    }
}
