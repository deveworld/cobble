use super::{DataPack, SourceLocation};

impl DataPack {
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
}
