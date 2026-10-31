//! Checks of the references between a pack's resources, once every file is
//! compiled: a tag entry, a function tag entry, or an id in a `datapack.*`
//! resource that names something neither Minecraft nor the pack defines, and
//! tags that include themselves. Minecraft refuses to start with a resource
//! that names a missing registry entry, and drops a tag with a missing entry
//! or a cycle, along with every function that names the tag.
//!
//! A `minecraft:` id counts if Minecraft or the pack defines it. Ids of
//! namespaces the pack doesn't declare anything in aren't checked, since
//! another pack or a mod can define them.

use super::resource_format::{RefKind, Reference};
use super::{SourceLocation, TranspileError, TranspileErrorKind, Transpiler};
use crate::validator::registries::vanilla_ids;
use std::collections::{BTreeMap, HashSet};

/// A reference found in a `datapack.*` declaration.
pub(in crate::transpiler) struct DeclaredReference {
    /// The declaration, as in "recipe `mypack:a`".
    pub(in crate::transpiler) declaration: String,
    pub(in crate::transpiler) source: Option<SourceLocation>,
    pub(in crate::transpiler) reference: Reference,
}

/// The tag folders `datapack.*_tag` declarations write, with the kind of a
/// `#tag` entry in each.
const TAG_FOLDERS: &[(&str, RefKind)] = &[
    ("block", RefKind::BlockTag),
    ("item", RefKind::ItemTag),
    ("entity_type", RefKind::EntityTypeTag),
    ("dialog", RefKind::DialogTag),
    ("function", RefKind::FunctionTag),
];

/// A declared tag: its id, where it was declared, and its required and
/// optional entries.
struct DeclaredTag {
    id: String,
    source: Option<SourceLocation>,
    entries: Vec<(String, bool)>,
}

impl Transpiler {
    /// Reports the first declaration that names a resource, a tag, or a
    /// function neither Minecraft nor the pack defines, or a tag that
    /// includes itself. Call it once every file is compiled.
    pub fn check_resource_references(&self) -> Result<(), TranspileError> {
        let tags = self.declared_tags();
        for (folder, tags) in &tags {
            let entry_kind = TAG_FOLDERS
                .iter()
                .find(|(name, _)| name == folder)
                .map(|(_, kind)| *kind);
            for tag in tags.values() {
                for (entry, required) in &tag.entries {
                    if !required {
                        continue;
                    }
                    let (kind, id) = match entry.strip_prefix('#') {
                        Some(tag_id) => (entry_kind, tag_id),
                        None => match folder.as_str() {
                            "function" => (Some(RefKind::Function), entry.as_str()),
                            "dialog" => (Some(RefKind::Dialog), entry.as_str()),
                            _ => (None, entry.as_str()),
                        },
                    };
                    let Some(kind) = kind else {
                        continue;
                    };
                    if self.is_defined(kind, id) != Some(false) {
                        continue;
                    }
                    let shown = if entry.starts_with('#') {
                        format!("#{}", full_id(id))
                    } else {
                        full_id(id)
                    };
                    let mut message = format!(
                        "The {} tag `{}` names the {} `{}`, which {}. Minecraft doesn't load a tag with a missing entry, nor the functions that name the tag",
                        folder_what(folder),
                        tag.id,
                        kind.what(),
                        shown,
                        self.missing_reason(id),
                    );
                    if kind == RefKind::Function && !id.contains(':') {
                        message.push_str(&format!(
                            "; an id without a namespace means `minecraft:{id}`, so write `{}:{id}` for this pack's function",
                            self.data_pack.namespace
                        ));
                    } else {
                        message.push_str(&format!(
                            "; use {{\"id\": \"{entry}\", \"required\": False}} to skip it when it's missing"
                        ));
                    }
                    return Err(Self::reference_error(message, tag.source.as_ref()));
                }
            }
            if let Some((tag, cycle)) = find_cycle(tags) {
                let message = format!(
                    "The {} tag `{}` includes itself ({}). Minecraft doesn't load tags in a cycle, nor the functions that name them",
                    folder_what(folder),
                    tag.id,
                    cycle
                        .iter()
                        .map(|id| format!("#{id}"))
                        .collect::<Vec<_>>()
                        .join(" -> ")
                );
                return Err(Self::reference_error(message, tag.source.as_ref()));
            }
        }

        for declared in &self.resource_references {
            let reference = &declared.reference;
            if self.is_defined(reference.kind, &reference.id) != Some(false) {
                continue;
            }
            let shown = if reference.kind.is_tag() {
                format!("#{}", full_id(&reference.id))
            } else {
                full_id(&reference.id)
            };
            let message = format!(
                "The {} names the {} `{}` at `{}`, which {}. Minecraft {}",
                declared.declaration,
                reference.kind.what(),
                shown,
                reference.path,
                self.missing_reason(&reference.id),
                reference.kind.consequence(),
            );
            return Err(Self::reference_error(message, declared.source.as_ref()));
        }
        Ok(())
    }

    fn reference_error(message: String, source: Option<&SourceLocation>) -> TranspileError {
        TranspileError::new(TranspileErrorKind::DatapackResourceFormat, message)
            .with_location_if_missing(source.cloned())
    }

    fn missing_reason(&self, id: &str) -> &'static str {
        let (namespace, _) = split(id);
        if namespace == "minecraft" {
            "neither Minecraft nor the pack defines"
        } else {
            "the pack doesn't define"
        }
    }

    /// Whether `id` of `kind` exists: `None` when it can't be checked,
    /// because another pack or a mod can define it.
    fn is_defined(&self, kind: RefKind, id: &str) -> Option<bool> {
        let pack = &self.data_pack;
        let (namespace, path) = split(id);
        if namespace == "minecraft" {
            match vanilla_ids(kind.registry()) {
                // Minecraft's lists aren't built in, so its ids can't be told.
                None => return None,
                Some(known) if known.contains(path) => return Some(true),
                Some(_) => {}
            }
        }
        if namespace != "minecraft" && !self.pack_namespaces().contains(namespace) {
            return None;
        }
        let defined = match kind {
            RefKind::Function => namespace == pack.namespace && pack.functions.contains_key(path),
            RefKind::FunctionTag => {
                let full = format!("{namespace}:{path}");
                pack.json_resources
                    .contains_key(&format!("{namespace}/tags/function/{path}"))
                    || pack
                        .stdlib
                        .generate_tags(&pack.namespace)
                        .contains_key(&full)
                    || pack.tags.contains_key(&full)
                    || (namespace == pack.namespace && pack.tags.contains_key(path))
            }
            _ => {
                let declared = pack
                    .json_resources
                    .contains_key(&format!("{namespace}/{}/{path}", kind.folder()));
                let legacy = namespace == pack.namespace
                    && match kind {
                        RefKind::Advancement => pack.advancements.contains_key(path),
                        RefKind::LootTable => pack.loot_tables.contains_key(path),
                        RefKind::Recipe => pack.recipes.contains_key(path),
                        RefKind::Predicate => pack.predicates.contains_key(path),
                        RefKind::ItemModifier => pack.item_modifiers.contains_key(path),
                        _ => false,
                    };
                declared || legacy
            }
        };
        Some(defined)
    }

    /// The pack's namespace and every namespace it declares a resource in.
    fn pack_namespaces(&self) -> HashSet<&str> {
        let pack = &self.data_pack;
        let mut namespaces: HashSet<&str> = pack
            .json_resources
            .keys()
            .filter_map(|key| key.split_once('/').map(|(namespace, _)| namespace))
            .collect();
        namespaces.insert(pack.namespace.as_str());
        namespaces
    }

    /// The typed tags the pack declares, by folder (`block`, `function`, ...)
    /// and id, in a stable order.
    fn declared_tags(&self) -> BTreeMap<String, BTreeMap<String, DeclaredTag>> {
        let pack = &self.data_pack;
        let mut tags: BTreeMap<String, BTreeMap<String, DeclaredTag>> = BTreeMap::new();
        for (key, json) in &pack.json_resources {
            let Some((namespace, relative_path)) = key.split_once('/') else {
                continue;
            };
            let Some((folder, path)) = relative_path
                .strip_prefix("tags/")
                .and_then(|rest| rest.split_once('/'))
            else {
                continue;
            };
            if !TAG_FOLDERS.iter().any(|(name, _)| *name == folder) {
                continue;
            }
            let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
                continue;
            };
            let entries = value
                .get("values")
                .and_then(|values| values.as_array())
                .into_iter()
                .flatten()
                .filter_map(|entry| match entry {
                    serde_json::Value::String(id) => Some((id.clone(), true)),
                    serde_json::Value::Object(object) => {
                        let id = object.get("id")?.as_str()?.to_string();
                        let required = object
                            .get("required")
                            .and_then(|required| required.as_bool())
                            .unwrap_or(true);
                        Some((id, required))
                    }
                    _ => None,
                })
                .collect();
            let id = format!("{namespace}:{path}");
            tags.entry(folder.to_string()).or_default().insert(
                id.clone(),
                DeclaredTag {
                    id,
                    source: pack
                        .json_resource_origins
                        .get(key)
                        .and_then(|sources| sources.first())
                        .cloned(),
                    entries,
                },
            );
        }
        tags
    }
}

/// A tag of `tags` that includes itself, with the tags of the cycle from it
/// back to it.
fn find_cycle(tags: &BTreeMap<String, DeclaredTag>) -> Option<(&DeclaredTag, Vec<String>)> {
    fn visit<'a>(
        id: &'a str,
        tags: &'a BTreeMap<String, DeclaredTag>,
        path: &mut Vec<&'a str>,
        done: &mut HashSet<&'a str>,
    ) -> Option<Vec<String>> {
        if let Some(start) = path.iter().position(|seen| *seen == id) {
            let mut cycle: Vec<String> = path[start..].iter().map(|id| id.to_string()).collect();
            cycle.push(id.to_string());
            return Some(cycle);
        }
        if done.contains(id) {
            return None;
        }
        let tag = tags.get(id)?;
        path.push(id);
        for (entry, _) in &tag.entries {
            let Some(next) = entry.strip_prefix('#') else {
                continue;
            };
            let next = full_id(next);
            let Some((key, _)) = tags.get_key_value(&next) else {
                continue;
            };
            if let Some(cycle) = visit(key, tags, path, done) {
                return Some(cycle);
            }
        }
        path.pop();
        done.insert(id);
        None
    }
    let mut done = HashSet::new();
    for id in tags.keys() {
        if let Some(cycle) = visit(id, tags, &mut Vec::new(), &mut done) {
            let tag = tags.get(&cycle[0])?;
            return Some((tag, cycle));
        }
    }
    None
}

fn folder_what(folder: &str) -> &str {
    match folder {
        "entity_type" => "entity type",
        folder => folder,
    }
}

/// `namespace:path`, with `minecraft` for an id without a namespace.
fn full_id(id: &str) -> String {
    let (namespace, path) = split(id);
    format!("{namespace}:{path}")
}

fn split(id: &str) -> (&str, &str) {
    match id.split_once(':') {
        Some(("", path)) => ("minecraft", path),
        Some((namespace, path)) => (namespace, path),
        None => ("minecraft", id),
    }
}
