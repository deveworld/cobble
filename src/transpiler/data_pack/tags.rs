use super::{DataPack, SourceLocation};

impl DataPack {
    /// Returns true when `relative_path` points at a Cobble-owned typed tag
    /// resource (`tags/function/`, `tags/block/`, `tags/item/`, or
    /// `tags/entity_type/`).
    pub(in crate::transpiler::data_pack) fn is_typed_tag_path(relative_path: &str) -> bool {
        relative_path.starts_with("tags/function/")
            || relative_path.starts_with("tags/block/")
            || relative_path.starts_with("tags/item/")
            || relative_path.starts_with("tags/entity_type/")
    }

    /// Merge a typed tag resource. Duplicate declarations for the same tag
    /// ID have their `values` arrays combined, deduplicated, and sorted.
    /// Object entries with `required: true` are normalized to the equivalent
    /// string entry; `required: false` entries stay distinct.
    pub(in crate::transpiler::data_pack) fn merge_tag_resource(
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
        let raw_new_values = new_value
            .get("values")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        // Validate tag schema: values must be an array of string resource IDs
        // or object entries with id/required fields.
        if new_value.get("values").is_some() && !new_value.get("values").unwrap().is_array() {
            return Err(format!(
                "Invalid tag '{}:{}': 'values' must be an array",
                namespace, relative_path
            ));
        }
        let new_values = Self::normalize_tag_values(namespace, relative_path, &raw_new_values)?;
        let new_replace = Self::tag_replace(namespace, relative_path, &new_value)?;

        let merged_json = if let Some(existing_json) = self.json_resources.get(key) {
            let existing_value: serde_json::Value =
                serde_json::from_str(existing_json).map_err(|e| {
                    format!(
                        "Failed to parse existing tag JSON for '{}:{}': {}",
                        namespace, relative_path, e
                    )
                })?;
            let raw_existing_values = existing_value
                .get("values")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let existing_values =
                Self::normalize_tag_values(namespace, relative_path, &raw_existing_values)?;
            let existing_replace = Self::tag_replace(namespace, relative_path, &existing_value)?;

            let mut merged: Vec<serde_json::Value> = existing_values;
            merged.extend(new_values);
            Self::sort_and_dedup_tag_values(&mut merged);

            let mut merged_obj = serde_json::Map::new();
            merged_obj.insert("values".to_string(), serde_json::Value::Array(merged));

            if let Some(replace) = Self::merged_tag_replace(existing_replace, new_replace) {
                merged_obj.insert("replace".to_string(), serde_json::Value::Bool(replace));
            }

            serde_json::to_string_pretty(&serde_json::Value::Object(merged_obj))
                .map_err(|e| format!("Failed to serialize merged tag JSON: {}", e))?
        } else {
            // First declaration: store canonical values in deterministic order.
            let mut sorted_values = new_values;
            Self::sort_and_dedup_tag_values(&mut sorted_values);

            let mut obj = serde_json::Map::new();
            obj.insert(
                "values".to_string(),
                serde_json::Value::Array(sorted_values),
            );
            if let Some(replace) = new_replace {
                obj.insert("replace".to_string(), serde_json::Value::Bool(replace));
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

    fn normalize_tag_values(
        namespace: &str,
        relative_path: &str,
        values: &[serde_json::Value],
    ) -> Result<Vec<serde_json::Value>, String> {
        values
            .iter()
            .enumerate()
            .map(|(idx, entry)| Self::normalize_tag_value(namespace, relative_path, idx, entry))
            .collect()
    }

    /// Best-effort normalization for tag JSON that already exists on disk:
    /// entries that fail validation are kept unchanged instead of aborting
    /// the merge. Shares [`Self::normalize_tag_value`] with the in-memory
    /// typed tag merge so both paths produce identical canonical entries.
    pub(in crate::transpiler::data_pack) fn normalize_tag_values_lossy(
        values: &[serde_json::Value],
    ) -> Vec<serde_json::Value> {
        values
            .iter()
            .enumerate()
            .map(|(idx, entry)| {
                Self::normalize_tag_value("", "", idx, entry).unwrap_or_else(|_| entry.clone())
            })
            .collect()
    }

    fn normalize_tag_value(
        namespace: &str,
        relative_path: &str,
        idx: usize,
        entry: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        if entry.is_string() {
            return Ok(entry.clone());
        }

        let Some(object) = entry.as_object() else {
            return Err(format!(
                "Invalid tag '{}:{}': 'values[{}]' must be a string resource ID or object entry",
                namespace, relative_path, idx
            ));
        };

        for key in object.keys() {
            if key != "id" && key != "required" {
                return Err(format!(
                    "Invalid tag '{}:{}': 'values[{}]' object may only contain 'id' and 'required'",
                    namespace, relative_path, idx
                ));
            }
        }

        let Some(id) = object.get("id").and_then(|value| value.as_str()) else {
            return Err(format!(
                "Invalid tag '{}:{}': 'values[{}]' object must include a string 'id'",
                namespace, relative_path, idx
            ));
        };
        let required = object
            .get("required")
            .map(|value| {
                value.as_bool().ok_or_else(|| {
                    format!(
                        "Invalid tag '{}:{}': 'values[{}].required' must be a boolean",
                        namespace, relative_path, idx
                    )
                })
            })
            .transpose()?
            .unwrap_or(true);

        if required {
            Ok(serde_json::Value::String(id.to_string()))
        } else {
            let mut normalized = serde_json::Map::new();
            normalized.insert("id".to_string(), serde_json::Value::String(id.to_string()));
            normalized.insert("required".to_string(), serde_json::Value::Bool(false));
            Ok(serde_json::Value::Object(normalized))
        }
    }

    pub(in crate::transpiler::data_pack) fn sort_and_dedup_tag_values(
        values: &mut Vec<serde_json::Value>,
    ) {
        values.sort_by(|a, b| {
            let (a_id, a_required_rank) = Self::tag_value_sort_key(a);
            let (b_id, b_required_rank) = Self::tag_value_sort_key(b);
            a_id.cmp(b_id).then(a_required_rank.cmp(&b_required_rank))
        });
        values.dedup();
    }

    fn tag_value_sort_key(value: &serde_json::Value) -> (&str, u8) {
        if let Some(id) = value.as_str() {
            return (id, 0);
        }

        let Some(object) = value.as_object() else {
            return ("", 2);
        };
        let id = object
            .get("id")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let required = object
            .get("required")
            .and_then(|value| value.as_bool())
            .unwrap_or(true);
        (id, if required { 0 } else { 1 })
    }

    fn tag_replace(
        namespace: &str,
        relative_path: &str,
        value: &serde_json::Value,
    ) -> Result<Option<bool>, String> {
        value
            .get("replace")
            .map(|replace| {
                replace.as_bool().ok_or_else(|| {
                    format!(
                        "Invalid tag '{}:{}': 'replace' must be a boolean",
                        namespace, relative_path
                    )
                })
            })
            .transpose()
    }

    fn merged_tag_replace(existing: Option<bool>, new: Option<bool>) -> Option<bool> {
        match (existing, new) {
            (Some(true), _) | (_, Some(true)) => Some(true),
            (Some(false), _) | (_, Some(false)) => Some(false),
            (None, None) => None,
        }
    }
}
