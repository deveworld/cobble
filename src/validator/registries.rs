//! Checks for arguments Minecraft looks up when it loads a function, such as
//! item, block, and entity ids. An unknown id stops the whole function from
//! loading, and every function that calls it. `registries.json` comes from
//! `scripts/update_registries.py`.
//!
//! Only `minecraft:` ids are checked, so a modded server's own ids pass, and
//! tags and values with a `$(name)` placeholder are skipped.

use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

#[derive(Deserialize)]
struct Registries {
    item: HashSet<String>,
    block: HashSet<String>,
    entity_type: HashSet<String>,
    mob_effect: HashSet<String>,
    particle_type: HashSet<String>,
    attribute: HashSet<String>,
    custom_stat: HashSet<String>,
    enchantment: HashSet<String>,
    damage_type: HashSet<String>,
    biome: HashSet<String>,
    block_properties: HashMap<String, HashMap<String, Vec<String>>>,
}

static REGISTRIES: LazyLock<Registries> = LazyLock::new(|| {
    serde_json::from_str(include_str!("registries.json")).expect("registries.json is valid")
});

/// The criteria without a `:` that take no team color.
const SIMPLE_CRITERIA: &[&str] = &[
    "dummy",
    "trigger",
    "deathCount",
    "playerKillCount",
    "totalKillCount",
    "health",
    "xp",
    "level",
    "food",
    "air",
    "armor",
];
const TEAM_COLORS: &[&str] = &[
    "black",
    "dark_blue",
    "dark_green",
    "dark_aqua",
    "dark_red",
    "dark_purple",
    "gold",
    "gray",
    "dark_gray",
    "blue",
    "green",
    "aqua",
    "red",
    "light_purple",
    "yellow",
    "white",
];
/// The name part of a `minecraft:` id at the start of `token`, up to its
/// `[` or `{`. `None` for a tag, a wildcard, or another namespace.
fn vanilla_name(token: &str) -> Option<&str> {
    let id = token.split(['[', '{']).next().unwrap_or(token);
    if id.starts_with('#') || id.starts_with('*') {
        return None;
    }
    match id.split_once(':') {
        Some(("minecraft", name)) => Some(name),
        Some(_) => None,
        None => Some(id),
    }
}

fn unknown(what: &str, token: &str) -> Option<String> {
    let id = token.split(['[', '{']).next().unwrap_or(token);
    Some(format!("Unknown {what} `{id}`"))
}

/// What is wrong with the argument `token` of the parser `parser_type`.
pub fn problem(
    parser_type: &str,
    properties: Option<&serde_json::Value>,
    token: &str,
) -> Option<String> {
    if token.contains("$(") {
        return None;
    }
    let registries = &*REGISTRIES;
    match parser_type {
        "minecraft:item_stack" | "minecraft:item_predicate" => {
            let name = vanilla_name(token)?;
            (!registries.item.contains(name))
                .then(|| unknown("item", token))
                .flatten()
        }
        "minecraft:block_state" | "minecraft:block_predicate" => block_problem(token),
        "minecraft:particle" => {
            let name = vanilla_name(token)?;
            (!registries.particle_type.contains(name))
                .then(|| unknown("particle", token))
                .flatten()
        }
        "minecraft:resource" | "minecraft:resource_or_tag" => {
            let registry = properties?.get("registry")?.as_str()?;
            let (names, what) = match registry {
                "minecraft:entity_type" => (&registries.entity_type, "entity type"),
                "minecraft:mob_effect" => (&registries.mob_effect, "effect"),
                "minecraft:attribute" => (&registries.attribute, "attribute"),
                "minecraft:enchantment" => (&registries.enchantment, "enchantment"),
                "minecraft:damage_type" => (&registries.damage_type, "damage type"),
                "minecraft:worldgen/biome" => (&registries.biome, "biome"),
                _ => return None,
            };
            let name = vanilla_name(token)?;
            (!names.contains(name))
                .then(|| unknown(what, token))
                .flatten()
        }
        "minecraft:objective_criteria" => criterion_problem(token),
        "minecraft:entity" => selector_problem(token),
        _ => None,
    }
}

fn block_problem(token: &str) -> Option<String> {
    let name = vanilla_name(token)?;
    let registries = &*REGISTRIES;
    if !registries.block.contains(name) {
        return unknown("block", token);
    }
    let (_, rest) = token.split_once('[')?;
    let (states, _) = rest.split_once(']')?;
    let properties = registries.block_properties.get(name);
    for state in states.split(',').filter(|state| !state.trim().is_empty()) {
        let (property, value) = state.split_once('=')?;
        let (property, value) = (property.trim(), value.trim());
        let Some(values) = properties.and_then(|properties| properties.get(property)) else {
            return Some(format!("Block `{name}` has no property `{property}`"));
        };
        if !values.iter().any(|known| known == value) {
            return Some(format!(
                "Block `{name}` has no `{property}={value}` (use one of: {})",
                values.join(", ")
            ));
        }
    }
    None
}

fn criterion_problem(criterion: &str) -> Option<String> {
    let registries = &*REGISTRIES;
    if SIMPLE_CRITERIA.contains(&criterion) {
        return None;
    }
    for prefix in ["teamkill.", "killedByTeam."] {
        if let Some(color) = criterion.strip_prefix(prefix) {
            return (!TEAM_COLORS.contains(&color))
                .then(|| format!("Unknown scoreboard criterion `{criterion}`"));
        }
    }
    let Some((stat, value)) = criterion.split_once(':') else {
        return Some(format!("Unknown scoreboard criterion `{criterion}`"));
    };
    let names = match stat {
        "minecraft.killed" | "minecraft.killed_by" => &registries.entity_type,
        "minecraft.mined" => &registries.block,
        "minecraft.crafted"
        | "minecraft.used"
        | "minecraft.broken"
        | "minecraft.picked_up"
        | "minecraft.dropped" => &registries.item,
        "minecraft.custom" => &registries.custom_stat,
        // A stat of another namespace, such as a mod's.
        other if !other.starts_with("minecraft.") => return None,
        _ => return Some(format!("Unknown scoreboard criterion `{criterion}`")),
    };
    let name = value.strip_prefix("minecraft.")?;
    (!names.contains(name)).then(|| format!("Unknown scoreboard criterion `{criterion}`"))
}

/// A selector's `type=` value, such as `@e[type=!minecraft:zombie]`, at the
/// top level of its arguments.
fn selector_problem(selector: &str) -> Option<String> {
    let arguments = selector.strip_prefix('@')?.get(1..)?.strip_prefix('[')?;
    let mut depth = 0;
    let mut quoted = false;
    let mut start = 0;
    for (index, ch) in arguments.char_indices() {
        match ch {
            '"' => quoted = !quoted,
            '{' | '[' if !quoted => depth += 1,
            '}' if !quoted => depth -= 1,
            ']' if !quoted && depth > 0 => depth -= 1,
            ',' | ']' if !quoted && depth == 0 => {
                if let Some(value) = arguments[start..index].trim().strip_prefix("type=") {
                    let value = value.trim_start_matches('!');
                    let unknown_type = vanilla_name(value)
                        .is_some_and(|name| !REGISTRIES.entity_type.contains(name));
                    if unknown_type {
                        return unknown("entity type", value);
                    }
                }
                start = index + 1;
            }
            _ => {}
        }
    }
    None
}

/// The resources of the data pack being validated. Minecraft looks up a loot
/// table, a predicate, an item modifier, a dialog, or a slot source when it
/// loads a function that names one, so a missing one stops that function from
/// loading.
pub struct PackResources {
    data: PathBuf,
}

impl PackResources {
    pub fn new(pack: &Path) -> Self {
        Self {
            data: pack.join("data"),
        }
    }

    /// What is wrong with the reference `token` of the parser `parser_type`.
    /// Only namespaces the pack has are checked: other packs and Minecraft
    /// can define the rest.
    pub fn problem(&self, parser_type: &str, token: &str) -> Option<String> {
        let (folder, what) = match parser_type {
            "minecraft:loot_table" => ("loot_table", "loot table"),
            "minecraft:loot_modifier" => ("item_modifier", "item modifier"),
            "minecraft:loot_predicate" => ("predicate", "predicate"),
            "minecraft:dialog" => ("dialog", "dialog"),
            "minecraft:slot_source" => ("slot_source", "slot source"),
            _ => return None,
        };
        if token.contains("$(") || token.starts_with('{') || token.starts_with('[') {
            return None;
        }
        let (namespace, path) = token.split_once(':')?;
        if namespace == "minecraft" || !self.data.join(namespace).is_dir() {
            return None;
        }
        let file = self
            .data
            .join(namespace)
            .join(folder)
            .join(format!("{path}.json"));
        (!file.is_file()).then(|| format!("Unknown {what} `{token}`: the pack doesn't define it"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_ids_pass() {
        let resource = |registry: &str| serde_json::json!({ "registry": registry });
        for (parser, properties, token) in [
            ("minecraft:item_stack", None, "minecraft:diamond"),
            ("minecraft:item_stack", None, "diamond[custom_data={a:1}]"),
            ("minecraft:item_predicate", None, "#minecraft:logs"),
            ("minecraft:item_predicate", None, "*"),
            ("minecraft:item_stack", None, "mymod:ruby"),
            ("minecraft:item_stack", None, "minecraft:$(item)"),
            (
                "minecraft:block_state",
                None,
                "minecraft:oak_stairs[facing=north]",
            ),
            ("minecraft:block_predicate", None, "#minecraft:logs"),
            ("minecraft:particle", None, "minecraft:heart"),
            (
                "minecraft:particle",
                None,
                "minecraft:dust{color:[1,0,0],scale:1}",
            ),
            ("minecraft:objective_criteria", None, "dummy"),
            ("minecraft:objective_criteria", None, "teamkill.red"),
            (
                "minecraft:objective_criteria",
                None,
                "minecraft.killed:minecraft.zombie",
            ),
            (
                "minecraft:objective_criteria",
                None,
                "minecraft.custom:minecraft.jump",
            ),
            ("minecraft:entity", None, "@e[type=minecraft:zombie,tag=x]"),
            ("minecraft:entity", None, "@e[type=!zombie]"),
            ("minecraft:entity", None, "@e[type=#minecraft:skeletons]"),
            ("minecraft:entity", None, "@e[nbt={type:\"bogus\"}]"),
            ("minecraft:entity", None, "@s"),
        ] {
            assert_eq!(problem(parser, properties, token), None, "{token}");
        }
        let entity = resource("minecraft:entity_type");
        assert_eq!(problem("minecraft:resource", Some(&entity), "zombie"), None);
        let timeline = resource("minecraft:timeline");
        assert_eq!(problem("minecraft:resource", Some(&timeline), "x"), None);
    }

    #[test]
    fn unknown_ids_are_reported() {
        let resource = |registry: &str| serde_json::json!({ "registry": registry });
        for (parser, properties, token, expected) in [
            (
                "minecraft:item_stack",
                None,
                "minecraft:diamnd",
                "Unknown item `minecraft:diamnd`",
            ),
            (
                "minecraft:item_predicate",
                None,
                "stik",
                "Unknown item `stik`",
            ),
            (
                "minecraft:block_state",
                None,
                "minecraft:stonee",
                "Unknown block",
            ),
            (
                "minecraft:block_state",
                None,
                "minecraft:oak_stairs[facing=up]",
                "no `facing=up`",
            ),
            (
                "minecraft:block_state",
                None,
                "minecraft:stone[facing=up]",
                "no property `facing`",
            ),
            (
                "minecraft:particle",
                None,
                "minecraft:hartt",
                "Unknown particle",
            ),
            (
                "minecraft:objective_criteria",
                None,
                "notacriterion",
                "scoreboard criterion",
            ),
            (
                "minecraft:objective_criteria",
                None,
                "minecraft.killed:minecraft.zombi",
                "scoreboard criterion",
            ),
            (
                "minecraft:objective_criteria",
                None,
                "teamkill.pink",
                "scoreboard criterion",
            ),
            (
                "minecraft:entity",
                None,
                "@e[type=minecraft:zombi]",
                "Unknown entity type",
            ),
            (
                "minecraft:entity",
                None,
                "@e[tag=x,type=!zombi,limit=1]",
                "`zombi`",
            ),
        ] {
            let found = problem(parser, properties, token).unwrap_or_default();
            assert!(found.contains(expected), "{token}: {found}");
        }
        for (registry, what) in [
            ("minecraft:entity_type", "entity type"),
            ("minecraft:mob_effect", "effect"),
            ("minecraft:attribute", "attribute"),
            ("minecraft:enchantment", "enchantment"),
            ("minecraft:damage_type", "damage type"),
        ] {
            let properties = resource(registry);
            let found = problem("minecraft:resource", Some(&properties), "minecraft:bogus");
            assert_eq!(found, Some(format!("Unknown {what} `minecraft:bogus`")));
        }
    }

    #[test]
    fn references_to_the_pack_must_exist() {
        let pack = tempfile::TempDir::new().unwrap();
        let folder = pack.path().join("data/mypack/loot_table/chests");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("reward.json"), "{}").unwrap();
        let resources = PackResources::new(pack.path());
        let loot = "minecraft:loot_table";
        assert_eq!(resources.problem(loot, "mypack:chests/reward"), None);
        assert_eq!(
            resources.problem(loot, "minecraft:chests/simple_dungeon"),
            None
        );
        assert_eq!(resources.problem(loot, "other:thing"), None);
        assert_eq!(resources.problem(loot, "mypack:$(table)"), None);
        assert_eq!(resources.problem(loot, "{pools:[]}"), None);
        assert_eq!(resources.problem("minecraft:slot_source", "weapon.*"), None);
        let missing = resources
            .problem(loot, "mypack:chests/gone")
            .unwrap_or_default();
        assert!(missing.contains("Unknown loot table"), "{missing}");
        let missing = resources
            .problem("minecraft:slot_source", "mypack:hands")
            .unwrap_or_default();
        assert!(missing.contains("Unknown slot source"), "{missing}");
    }
}
