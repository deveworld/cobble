use super::{DataPack, SourceLocation};
use std::collections::{BTreeMap, HashMap};

impl DataPack {
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
}
