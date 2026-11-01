use super::INIT_FUNCTION;
use super::{DataPack, SourceLocation};
use crate::stdlib::EventType;
use std::collections::HashSet;

/// A typed tag as its declarations are merged: its values without repeats,
/// in declaration order, and its `replace`. [`DataPack::flush_tags`] writes
/// it to `json_resources`, so each merge costs only its own values.
#[derive(Clone, Debug, Default)]
pub(crate) struct MergedTag {
    values: Vec<serde_json::Value>,
    seen: HashSet<String>,
    replace: Option<bool>,
    function_tag: bool,
    /// Changed since it was last written to `json_resources`.
    dirty: bool,
}

impl MergedTag {
    fn extend(&mut self, values: Vec<serde_json::Value>) {
        for value in values {
            if self.seen.insert(value.to_string()) {
                self.values.push(value);
            }
        }
    }
}

impl DataPack {
    /// Returns true when `relative_path` points at a Cobble-owned typed tag
    /// resource (`tags/function/`, `tags/block/`, `tags/item/`,
    /// `tags/entity_type/`, or `tags/dialog/`).
    pub(in crate::transpiler::data_pack) fn is_typed_tag_path(relative_path: &str) -> bool {
        relative_path.starts_with("tags/function/")
            || relative_path.starts_with("tags/block/")
            || relative_path.starts_with("tags/item/")
            || relative_path.starts_with("tags/entity_type/")
            || relative_path.starts_with("tags/dialog/")
    }

    /// Whether `path` (`tags/function/x` or `data/ns/tags/function/x.json`)
    /// is a function tag. Minecraft runs a function tag in the order of its
    /// values, so its values keep their order instead of being sorted.
    pub fn is_function_tag_path(path: &str) -> bool {
        path.starts_with("tags/function/") || path.contains("/tags/function/")
    }

    /// Merge a typed tag resource. Duplicate declarations for the same tag
    /// ID have their `values` arrays combined and deduplicated. Function
    /// tags keep the declared order, with later declarations appended and
    /// repeats after the first dropped; other tags are sorted.
    /// Object entries with `required: true` are normalized to the equivalent
    /// string entry; `required: false` entries stay distinct.
    /// The merged tag reaches `json_resources` at [`Self::flush_tags`].
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

        if !self.merged_tags.contains_key(key) {
            // A tag written before, or one `json_resources` already holds.
            let mut merged = MergedTag {
                function_tag: Self::is_function_tag_path(relative_path),
                ..MergedTag::default()
            };
            if let Some(existing_json) = self.json_resources.get(key) {
                let existing_value: serde_json::Value = serde_json::from_str(existing_json)
                    .map_err(|e| {
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
                merged.extend(Self::normalize_tag_values(
                    namespace,
                    relative_path,
                    &raw_existing_values,
                )?);
                merged.replace = Self::tag_replace(namespace, relative_path, &existing_value)?;
            }
            self.merged_tags.insert(key.to_string(), merged);
        }
        let merged = self
            .merged_tags
            .get_mut(key)
            .expect("the merged tag was just added");
        merged.extend(new_values);
        merged.replace = Self::merged_tag_replace(merged.replace, new_replace);
        merged.dirty = true;
        if let Some(source) = source {
            self.json_resource_origins
                .entry(key.to_string())
                .or_default()
                .push(source);
        }
        Ok(())
    }

    /// Writes the tags merged since the last call to `json_resources`.
    pub fn flush_tags(&mut self) -> Result<(), String> {
        for (key, merged) in &mut self.merged_tags {
            if !merged.dirty {
                continue;
            }
            let mut values = merged.values.clone();
            if !merged.function_tag {
                Self::sort_and_dedup_tag_values(&mut values);
            }
            let mut object = serde_json::Map::new();
            object.insert("values".to_string(), serde_json::Value::Array(values));
            if let Some(replace) = merged.replace {
                object.insert("replace".to_string(), serde_json::Value::Bool(replace));
            }
            let json = serde_json::to_string_pretty(&serde_json::Value::Object(object))
                .map_err(|e| format!("Failed to serialize tag JSON: {}", e))?;
            self.json_resources.insert(key.clone(), json);
            merged.dirty = false;
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

    /// Function tags keep their order and drop repeats after the first;
    /// other tags are sorted and deduplicated.
    pub(in crate::transpiler::data_pack) fn order_tag_values(
        path: &str,
        values: &mut Vec<serde_json::Value>,
    ) {
        if Self::is_function_tag_path(path) {
            let mut seen = std::collections::HashSet::new();
            values.retain(|value| seen.insert(value.to_string()));
        } else {
            Self::sort_and_dedup_tag_values(values);
        }
    }

    fn sort_and_dedup_tag_values(values: &mut Vec<serde_json::Value>) {
        values.sort_by(|a, b| {
            let (a_id, a_required_rank) = Self::tag_value_sort_key(a);
            let (b_id, b_required_rank) = Self::tag_value_sort_key(b);
            a_id.cmp(b_id).then(a_required_rank.cmp(&b_required_rank))
        });
        values.dedup();
    }

    /// The namespaced ID of `_cobble_init`, which holds the runtime setup and
    /// module variable initializers (see `ensure_init_function`), when the
    /// pack has one.
    pub fn init_function_id(&self) -> Option<String> {
        self.stdlib
            .get_event_handlers(&EventType::Load)
            .iter()
            .any(|name| name == INIT_FUNCTION)
            .then(|| format!("{}:{}", self.namespace, INIT_FUNCTION))
    }

    /// Orders a merged `minecraft:load` or `minecraft:tick` tag the way the
    /// source declares it: event listeners and `datapack.function_tag`
    /// entries in the order their statements run, so a listener added after
    /// a declaration comes after its entries. `_cobble_init` goes first in
    /// `minecraft:load`, so no entry runs before the pack's objectives exist.
    /// Other tags are returned unchanged.
    pub fn order_load_tag_json(
        &self,
        resource_namespace: &str,
        relative_path: &str,
        json: String,
    ) -> Result<String, String> {
        let event = match (resource_namespace, relative_path) {
            ("minecraft", "tags/function/load") => EventType::Load,
            ("minecraft", "tags/function/tick") => EventType::Tick,
            _ => return Ok(json),
        };
        let tag_id = format!("minecraft:{}", &relative_path["tags/function/".len()..]);
        let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&json) else {
            return Ok(json);
        };
        let Some(values) = value
            .get_mut("values")
            .and_then(|values| values.as_array_mut())
        else {
            return Ok(json);
        };
        let init_id = match event {
            EventType::Load => self.init_function_id(),
            _ => None,
        };
        let listeners: Vec<serde_json::Value> = self
            .stdlib
            .get_event_handlers(&event)
            .iter()
            .filter(|name| name.as_str() != INIT_FUNCTION)
            .map(|name| serde_json::Value::String(format!("{}:{}", self.namespace, name)))
            .collect();
        let declarations = self
            .event_tag_declarations
            .get(&tag_id)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let mut ordered: Vec<serde_json::Value> = Vec::new();
        if let Some(init_id) = &init_id {
            ordered.push(serde_json::Value::String(init_id.clone()));
        }
        for position in 0..=listeners.len() {
            for (before, declared) in declarations {
                if *before == position {
                    ordered.extend(Self::normalize_tag_values_lossy(declared));
                }
            }
            if let Some(listener) = listeners.get(position) {
                ordered.push(listener.clone());
            }
        }
        // Anything else the file holds keeps its place after these.
        ordered.extend(values.iter().cloned());
        let mut seen = std::collections::HashSet::new();
        ordered.retain(|entry| seen.insert(entry.to_string()));
        if *values == ordered {
            return Ok(json);
        }
        *values = ordered;
        serde_json::to_string_pretty(&value).map_err(|error| error.to_string())
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
