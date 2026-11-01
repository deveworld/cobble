//! Checks of the references between a pack's resources, once every file is
//! compiled: a tag entry, a function tag entry, or an id in a `datapack.*`
//! resource that names something neither Minecraft nor the pack defines, and
//! tags that include themselves. Minecraft refuses to start with a resource
//! that names a missing registry entry, and drops a tag with a missing entry
//! or a cycle, along with every function that names the tag.
//!
//! A `minecraft:` id counts if Minecraft or the pack defines it. Ids of
//! other namespaces than `minecraft` and the pack's own aren't checked, even
//! where the pack declares something in them, since another pack or a mod
//! can define them.

use super::resource_format::{RefKind, Reference};
use super::{SourceLocation, TranspileError, TranspileErrorKind, Transpiler};
use crate::validator::registries::vanilla_ids;
use std::collections::{BTreeMap, HashMap, HashSet};

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
        let stdlib_tags = self
            .data_pack
            .stdlib
            .generate_tags(&self.data_pack.namespace);
        let defined = |kind: RefKind, id: &str| self.is_defined(kind, id, &stdlib_tags);
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
                        // A data pack can't add blocks, items, or entity
                        // types, so none exist in the pack's own namespace.
                        if split(entry).0 == self.data_pack.namespace {
                            let message = format!(
                                "The {} tag `{}` names `{}`, which doesn't exist: a data pack can't add {}. Minecraft doesn't load a tag with a missing entry, nor the functions that name the tag; for a tag of this pack, write `#{}`",
                                folder_what(folder),
                                tag.id,
                                full_id(entry),
                                match folder.as_str() {
                                    "block" => "blocks",
                                    "item" => "items",
                                    _ => "entity types",
                                },
                                full_id(entry),
                            );
                            return Err(Self::reference_error(message, tag.source.as_ref()));
                        }
                        continue;
                    };
                    if defined(kind, id) != Some(false) {
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

        if let Some(cycle) = self.advancement_parent_cycle() {
            let message = format!(
                "The advancement `{}` is its own parent ({}). Minecraft doesn't load advancements whose parents go round in a cycle",
                cycle[0],
                cycle.join(" -> ")
            );
            let key = cycle[0].replacen(':', "/advancement/", 1);
            let source = self
                .data_pack
                .json_resource_origins
                .get(&key)
                .and_then(|sources| sources.first());
            return Err(Self::reference_error(message, source));
        }

        for declared in &self.resource_references {
            let reference = &declared.reference;
            if defined(reference.kind, &reference.id) != Some(false) {
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
    /// because another pack or a mod can define it. That is every namespace
    /// but `minecraft` and the pack's own, even one the pack declares
    /// something in, since another pack can add to it.
    fn is_defined(
        &self,
        kind: RefKind,
        id: &str,
        stdlib_tags: &HashMap<String, Vec<String>>,
    ) -> Option<bool> {
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
        if namespace != "minecraft" && namespace != pack.namespace {
            return None;
        }
        let defined = match kind {
            RefKind::Function => namespace == pack.namespace && pack.functions.contains_key(path),
            RefKind::FunctionTag => {
                let full = format!("{namespace}:{path}");
                pack.json_resources
                    .contains_key(&format!("{namespace}/tags/function/{path}"))
                    || stdlib_tags.contains_key(&full)
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

    /// The advancements of a cycle of `parent`s, from the first one back to
    /// it, in a stable order.
    fn advancement_parent_cycle(&self) -> Option<Vec<String>> {
        let mut parents: BTreeMap<String, String> = BTreeMap::new();
        for (key, json) in &self.data_pack.json_resources {
            let Some((namespace, path)) = key.split_once('/').and_then(|(namespace, rest)| {
                Some((namespace, rest.strip_prefix("advancement/")?))
            }) else {
                continue;
            };
            let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
                continue;
            };
            if let Some(parent) = value.get("parent").and_then(|parent| parent.as_str()) {
                parents.insert(format!("{namespace}:{path}"), full_id(parent));
            }
        }
        for start in parents.keys() {
            let mut chain = vec![start.clone()];
            let mut current = start;
            while let Some(parent) = parents.get(current) {
                if parent == start {
                    chain.push(parent.clone());
                    return Some(chain);
                }
                if chain.contains(parent) {
                    break;
                }
                chain.push(parent.clone());
                current = parent;
            }
        }
        None
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
        // Minecraft skips an optional entry that would close a cycle, and
        // the tags load.
        for (entry, _) in tag.entries.iter().filter(|(_, required)| *required) {
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
