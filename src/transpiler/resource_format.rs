//! Shape checks for `datapack.*` JSON resources against the Minecraft
//! version Cobble targets. Each check appends a human-readable problem for a
//! pre-26.3 field: Minecraft 26.3 either rejects the resource (and a server
//! whose packs fail to load refuses to start) or silently ignores the field.
//! Every rule was confirmed against a 26.3 server.

use serde_json::Value;

// The type registries below come from the 26.3 data generator's registries
// report (`java -DbundlerMainClass=net.minecraft.data.Main -jar server.jar
// --reports`). Data packs can't add entries to them, so Minecraft rejects
// any other type.

/// Dialog types in Minecraft 26.3.
const DIALOG_TYPES: &[&str] = &[
    "confirmation",
    "dialog_list",
    "multi_action",
    "notice",
    "server_links",
];
/// Predicate (loot condition) types in Minecraft 26.3.
const LOOT_CONDITION_TYPES: &[&str] = &[
    "all_of",
    "any_of",
    "damage_source_properties",
    "enchantment_active_check",
    "entity_properties",
    "entity_scores",
    "environment_attribute_check",
    "float_value_check",
    "int_value_check",
    "inverted",
    "killed_by_player",
    "location_check",
    "match_block",
    "match_tool",
    "random_chance",
    "random_chance_with_enchanted_bonus",
    "survives_explosion",
    "table_bonus",
    "time_check",
    "weather_check",
];

/// Item modifier (loot function) types in Minecraft 26.3.
const LOOT_FUNCTION_TYPES: &[&str] = &[
    "apply_bonus",
    "copy_components",
    "copy_custom_data",
    "copy_name",
    "copy_state",
    "discard",
    "enchant_randomly",
    "enchant_with_levels",
    "enchanted_count_increase",
    "exploration_map",
    "explosion_decay",
    "fill_player_head",
    "filtered",
    "furnace_smelt",
    "limit_count",
    "modify_contents",
    "sequence",
    "set_attributes",
    "set_banner_pattern",
    "set_book_cover",
    "set_components",
    "set_contents",
    "set_count",
    "set_custom_data",
    "set_custom_model_data",
    "set_damage",
    "set_enchantments",
    "set_firework_explosion",
    "set_fireworks",
    "set_instrument",
    "set_item",
    "set_loot_table",
    "set_lore",
    "set_name",
    "set_ominous_bottle_amplifier",
    "set_potion",
    "set_random_dyes",
    "set_random_potion",
    "set_stew_effect",
    "set_writable_book_pages",
    "set_written_book_pages",
    "toggle_tooltips",
];

/// Loot pool entry types in Minecraft 26.3.
const LOOT_ENTRY_TYPES: &[&str] = &[
    "alternatives",
    "dynamic",
    "empty",
    "group",
    "item",
    "loot_table",
    "sequence",
    "slots",
    "tag",
];

/// Recipe types in Minecraft 26.3.
const RECIPE_TYPES: &[&str] = &[
    "blasting",
    "brewing",
    "campfire_cooking",
    "crafting_decorated_pot",
    "crafting_dye",
    "crafting_imbue",
    "crafting_shaped",
    "crafting_shapeless",
    "crafting_special_bannerduplicate",
    "crafting_special_bookcloning",
    "crafting_special_firework_rocket",
    "crafting_special_firework_star",
    "crafting_special_firework_star_fade",
    "crafting_special_mapextending",
    "crafting_special_repairitem",
    "crafting_special_shielddecoration",
    "crafting_transmute",
    "smelting",
    "smithing_transform",
    "smithing_trim",
    "smoking",
    "stonecutting",
];

/// Advancement criterion triggers in Minecraft 26.3.
const TRIGGER_TYPES: &[&str] = &[
    "allay_drop_item_on_block",
    "any_block_use",
    "avoid_vibration",
    "bee_nest_destroyed",
    "bred_animals",
    "brewed_potion",
    "changed_dimension",
    "channeled_lightning",
    "construct_beacon",
    "consume_item",
    "crafter_recipe_crafted",
    "cured_zombie_villager",
    "default_block_use",
    "effects_changed",
    "enchanted_item",
    "enter_block",
    "entity_hurt_player",
    "entity_killed_player",
    "fall_after_explosion",
    "fall_from_height",
    "filled_bucket",
    "fishing_rod_hooked",
    "hero_of_the_village",
    "impossible",
    "inventory_changed",
    "item_durability_changed",
    "item_used_on_block",
    "kill_mob_near_sculk_catalyst",
    "killed_by_arrow",
    "levitation",
    "lightning_strike",
    "location",
    "nether_travel",
    "placed_block",
    "player_generates_container_loot",
    "player_hurt_entity",
    "player_interacted_with_entity",
    "player_killed_entity",
    "player_sheared_equipment",
    "recipe_crafted",
    "recipe_unlocked",
    "ride_entity_in_lava",
    "shot_crossbow",
    "slept_in_bed",
    "slide_down_block",
    "spear_mobs",
    "started_riding",
    "summoned_entity",
    "tame_animal",
    "target_hit",
    "thrown_item_picked_up_by_entity",
    "thrown_item_picked_up_by_player",
    "tick",
    "used_ender_eye",
    "used_totem",
    "using_item",
    "villager_trade",
    "voluntary_exile",
];

/// Renamed or replaced registry entries, with what to write instead.
const REPLACED_TYPES: &[(&str, &str)] = &[
    ("alternative", "minecraft:any_of"),
    ("value_check", "minecraft:int_value_check"),
    ("set_nbt", "minecraft:set_custom_data"),
    ("copy_nbt", "minecraft:copy_custom_data"),
];

/// What the checks of one resource found: errors, which stop Minecraft from
/// loading it, warnings about fields Minecraft ignores, and references to
/// other resources, which can only be checked once every declaration is
/// known.
#[derive(Default)]
pub(crate) struct Problems {
    pub(crate) errors: Vec<String>,
    pub(crate) warnings: Vec<String>,
    pub(crate) references: Vec<Reference>,
}

impl Problems {
    pub(crate) fn push(&mut self, problem: String) {
        self.errors.push(problem);
    }

    fn warn(&mut self, warning: String) {
        self.warnings.push(warning);
    }

    /// Records that `path` names `id` (a `#tag` without its `#`) of `kind`.
    fn refer(&mut self, kind: RefKind, id: &str, path: &str) {
        self.references.push(Reference {
            kind,
            id: id.to_string(),
            path: path.to_string(),
        });
    }
}

/// What a reference in a resource names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RefKind {
    BlockTag,
    ItemTag,
    EntityTypeTag,
    DialogTag,
    FunctionTag,
    LootTable,
    Predicate,
    ItemModifier,
    Dialog,
    Recipe,
    Advancement,
    Function,
}

impl RefKind {
    /// The folder of a namespace the resource is in.
    pub(crate) fn folder(self) -> &'static str {
        match self {
            Self::BlockTag => "tags/block",
            Self::ItemTag => "tags/item",
            Self::EntityTypeTag => "tags/entity_type",
            Self::DialogTag => "tags/dialog",
            Self::FunctionTag => "tags/function",
            Self::LootTable => "loot_table",
            Self::Predicate => "predicate",
            Self::ItemModifier => "item_modifier",
            Self::Dialog => "dialog",
            Self::Recipe => "recipe",
            Self::Advancement => "advancement",
            Self::Function => "function",
        }
    }

    /// The name of Minecraft's own list of these (see
    /// [`crate::validator::registries::vanilla_ids`]).
    pub(crate) fn registry(self) -> &'static str {
        match self {
            Self::BlockTag => "block_tag",
            Self::ItemTag => "item_tag",
            Self::EntityTypeTag => "entity_type_tag",
            Self::DialogTag => "dialog_tag",
            Self::FunctionTag | Self::Function => "",
            Self::LootTable => "loot_table",
            Self::Predicate => "predicate",
            Self::ItemModifier => "item_modifier",
            Self::Dialog => "dialog",
            Self::Recipe => "recipe",
            Self::Advancement => "advancement",
        }
    }

    pub(crate) fn what(self) -> &'static str {
        match self {
            Self::BlockTag => "block tag",
            Self::ItemTag => "item tag",
            Self::EntityTypeTag => "entity type tag",
            Self::DialogTag => "dialog tag",
            Self::FunctionTag => "function tag",
            Self::LootTable => "loot table",
            Self::Predicate => "predicate",
            Self::ItemModifier => "item modifier",
            Self::Dialog => "dialog",
            Self::Recipe => "recipe",
            Self::Advancement => "advancement",
            Self::Function => "function",
        }
    }

    pub(crate) fn is_tag(self) -> bool {
        matches!(
            self,
            Self::BlockTag
                | Self::ItemTag
                | Self::EntityTypeTag
                | Self::DialogTag
                | Self::FunctionTag
        )
    }

    /// What Minecraft does with a resource that names a missing one.
    pub(crate) fn consequence(self) -> &'static str {
        match self {
            Self::Advancement => "doesn't load an advancement whose parent is missing",
            _ => "refuses to start with a resource that names a missing one",
        }
    }
}

/// A reference from a resource to another resource or a tag.
#[derive(Clone, Debug)]
pub(crate) struct Reference {
    pub(crate) kind: RefKind,
    /// The id, without the `#` of a tag.
    pub(crate) id: String,
    /// Where in the resource, as in `pools[0].entries[1].value`.
    pub(crate) path: String,
}

/// Reports a `type`-like id that `known` doesn't list. `what` names the
/// registry in the message.
fn check_registry_id(
    value: Option<&Value>,
    path: &str,
    key: &str,
    known: &[&str],
    what: &str,
    problems: &mut Problems,
) {
    let Some(id) = value.and_then(Value::as_str) else {
        return;
    };
    let name = id.strip_prefix("minecraft:").unwrap_or(id);
    if known.contains(&name) {
        return;
    }
    let hint = REPLACED_TYPES
        .iter()
        .find(|(old, _)| *old == name)
        .map(|(_, new)| format!(" (use `{new}`)"))
        .unwrap_or_default();
    problems.push(format!(
        "{} `{id}` is not {what} in Minecraft 26.3{hint}; rejected",
        at(path, key)
    ));
}

fn has(json: &Value, key: &str) -> bool {
    json.get(key).is_some()
}

fn at(path: &str, key: &str) -> String {
    format!("`{}`", join(path, key))
}

/// `path.key`, or `key` at the top.
fn join(path: &str, key: &str) -> String {
    if path.is_empty() {
        key.to_string()
    } else {
        format!("{}.{}", path, key)
    }
}

fn prefix(path: &str) -> String {
    if path.is_empty() {
        String::new()
    } else {
        format!("{}.", path)
    }
}

/// The type of `json` without `minecraft:`, or "" when it has none.
fn type_name(json: &Value) -> &str {
    let kind = json.get("type").and_then(Value::as_str).unwrap_or("");
    kind.strip_prefix("minecraft:").unwrap_or(kind)
}

/// Reports each of `keys` that `json` lacks.
fn require(json: &Value, path: &str, keys: &[&str], problems: &mut Problems) {
    for key in keys {
        if !has(json, key) {
            problems.push(format!("{} is missing; rejected", at(path, key)));
        }
    }
}

/// The name of a `minecraft:` id (or an id without a namespace), or `None`
/// for another namespace, which a mod can define.
fn vanilla_name(id: &str) -> Option<&str> {
    match id.split_once(':') {
        Some(("minecraft" | "", name)) => Some(name),
        Some(_) => None,
        None => Some(id),
    }
}

/// Reports a `minecraft:` id that isn't in Minecraft's `registry`. Data packs
/// can't add to these registries.
fn check_vanilla_id(id: &str, registry: &str, what: &str, path: &str, problems: &mut Problems) {
    if id.contains("$(") {
        return;
    }
    let Some(name) = vanilla_name(id) else {
        return;
    };
    let Some(known) = crate::validator::registries::vanilla_ids(registry) else {
        return;
    };
    if !known.contains(name) {
        problems.push(format!(
            "`{path}` `{id}` is not {what} in Minecraft 26.3; rejected"
        ));
    }
}

/// An item, block, or entity type list: one id, a `#tag`, or a list of ids.
/// A list can't hold a tag.
fn check_holder_set(
    value: &Value,
    path: &str,
    registry: &str,
    what: &str,
    tag: Option<RefKind>,
    problems: &mut Problems,
) {
    match value {
        Value::String(id) => match id.strip_prefix('#') {
            Some(tag_id) => {
                if let Some(tag) = tag {
                    problems.refer(tag, tag_id, path);
                }
            }
            None => check_vanilla_id(id, registry, what, path, problems),
        },
        Value::Array(ids) => {
            for (index, id) in ids.iter().enumerate() {
                let item_path = format!("{path}[{index}]");
                match id.as_str() {
                    Some(id) if id.starts_with('#') => problems.push(format!(
                        "`{item_path}` is the tag `{id}`, but a list holds ids only; write the tag on its own instead of the list (\"{id}\"), or list the ids; rejected"
                    )),
                    Some(id) => check_vanilla_id(id, registry, what, &item_path, problems),
                    None => {}
                }
            }
        }
        _ => {}
    }
}

fn check_items(value: &Value, path: &str, problems: &mut Problems) {
    check_holder_set(
        value,
        path,
        "item",
        "an item",
        Some(RefKind::ItemTag),
        problems,
    );
}

/// A `condition` field: one predicate, or the id of a predicate file.
fn check_condition(condition: &Value, path: &str, problems: &mut Problems) {
    match condition {
        Value::String(id) => problems.refer(RefKind::Predicate, id, path),
        Value::Array(_) => problems.push(format!(
            "`{path}` is one predicate, not a list (combine several with \"type\": \"minecraft:all_of\" and \"terms\"); rejected"
        )),
        _ => check_predicate(condition, path, problems),
    }
}

/// Predicates: `condition` is now `type`, including the predicates nested in
/// `all_of`/`any_of` terms and `inverted`, and each type has the fields it
/// can't go without.
pub(crate) fn check_predicate(json: &Value, path: &str, problems: &mut Problems) {
    if !json.is_object() {
        return;
    }
    if has(json, "condition") && !has(json, "type") {
        problems.push(format!(
            "{} is now `type` ({}); rejected",
            at(path, "condition"),
            renamed_to_type(json, "condition", "minecraft:random_chance")
        ));
    } else if !has(json, "type") {
        problems.push(format!("{} is missing; rejected", at(path, "type")));
    }
    check_registry_id(
        json.get("type"),
        path,
        "type",
        LOOT_CONDITION_TYPES,
        "a predicate type",
        problems,
    );
    if type_name(json) == "time_check" && !has(json, "clock") {
        problems.push(format!(
            "{} is missing; a time check names the clock it reads, such as \"clock\": \"minecraft:overworld\"; rejected",
            at(path, "clock")
        ));
    }
    require(json, path, predicate_fields(type_name(json)), problems);
    if let Some(terms) = json.get("terms").and_then(Value::as_array) {
        for (index, term) in terms.iter().enumerate() {
            check_predicate(term, &format!("{}terms[{}]", prefix(path), index), problems);
        }
    }
    if let Some(term) = json.get("term") {
        check_condition(term, &join(path, "term"), problems);
    }
    match type_name(json) {
        "entity_properties" => {
            if let Some(predicate) = json.get("predicate") {
                check_entity_predicate(predicate, &join(path, "predicate"), problems);
            }
        }
        "location_check" => {
            if let Some(predicate) = json.get("predicate") {
                check_location_predicate(predicate, &join(path, "predicate"), problems);
            }
        }
        "match_tool" => {
            if let Some(predicate) = json.get("predicate") {
                check_item_predicate(predicate, &join(path, "predicate"), problems);
            }
        }
        _ => {}
    }
}

/// The `"type": ...` that replaces the old `key`, with its value when it is
/// text and `example` otherwise.
fn renamed_to_type(json: &Value, key: &str, example: &str) -> String {
    let value = json.get(key).and_then(Value::as_str).unwrap_or(example);
    format!("\"type\": {}", Value::String(value.to_string()))
}

/// The fields each predicate type can't go without (`clock` of a time check
/// has its own message).
fn predicate_fields(kind: &str) -> &'static [&'static str] {
    match kind {
        "random_chance" => &["chance"],
        "entity_properties" => &["entity"],
        "entity_scores" => &["entity", "scores"],
        "table_bonus" => &["enchantment", "chances"],
        "all_of" | "any_of" => &["terms"],
        "inverted" => &["term"],
        "time_check" => &["value"],
        "int_value_check" => &["value", "test"],
        "enchantment_active_check" => &["active"],
        _ => &[],
    }
}

/// An entity predicate: the entity type and the effects are looked up.
fn check_entity_predicate(json: &Value, path: &str, problems: &mut Problems) {
    if let Some(types) = json.get("minecraft:entity_type") {
        check_holder_set(
            types,
            &join(path, "minecraft:entity_type"),
            "entity_type",
            "an entity type",
            Some(RefKind::EntityTypeTag),
            problems,
        );
    }
    if let Some(effects) = json.get("effects").and_then(Value::as_object) {
        for effect in effects.keys() {
            check_vanilla_id(
                effect,
                "mob_effect",
                "an effect",
                &join(path, "effects"),
                problems,
            );
        }
    }
}

/// A location predicate: the blocks of `block` are looked up.
fn check_location_predicate(json: &Value, path: &str, problems: &mut Problems) {
    if let Some(blocks) = json.get("block").and_then(|block| block.get("blocks")) {
        check_holder_set(
            blocks,
            &join(path, "block.blocks"),
            "block",
            "a block",
            Some(RefKind::BlockTag),
            problems,
        );
    }
}

/// An item predicate: the items are looked up.
fn check_item_predicate(json: &Value, path: &str, problems: &mut Problems) {
    if let Some(items) = json.get("items") {
        check_items(items, &join(path, "items"), problems);
    }
}

/// `conditions` lists on loot functions, pools, and entries are now a single
/// `condition` (use `minecraft:all_of` to combine several).
fn check_conditions_list(json: &Value, path: &str, problems: &mut Problems) {
    if has(json, "conditions") {
        problems.push(format!(
            "{} is now a single `condition` (combine several with \"type\": \"minecraft:all_of\"); ignored, so the conditions would never apply",
            at(path, "conditions")
        ));
    }
    if let Some(condition) = json.get("condition") {
        check_condition(condition, &join(path, "condition"), problems);
    }
}

/// The fields each item modifier type can't go without.
fn item_modifier_fields(kind: &str) -> &'static [&'static str] {
    match kind {
        "set_count" => &["count"],
        "set_lore" => &["lore", "mode"],
        "set_components" => &["components"],
        "set_custom_data" => &["tag"],
        "sequence" => &["functions"],
        "set_damage" => &["damage"],
        "set_item" => &["item"],
        "set_potion" => &["id"],
        "enchant_with_levels" => &["levels"],
        "limit_count" => &["limit"],
        "copy_name" => &["source"],
        "toggle_tooltips" => &["toggles"],
        "apply_bonus" => &["enchantment", "formula"],
        _ => &[],
    }
}

/// Item modifiers (loot functions): `function` is now `type`, `conditions`
/// is a single `condition`, and each type has the fields it can't go
/// without.
pub(crate) fn check_item_modifier(json: &Value, path: &str, problems: &mut Problems) {
    if !json.is_object() {
        return;
    }
    if has(json, "function") && !has(json, "type") {
        problems.push(format!(
            "{} is now `type` ({}); rejected",
            at(path, "function"),
            renamed_to_type(json, "function", "minecraft:set_components")
        ));
    } else if !has(json, "type") {
        problems.push(format!("{} is missing; rejected", at(path, "type")));
    }
    check_registry_id(
        json.get("type"),
        path,
        "type",
        LOOT_FUNCTION_TYPES,
        "an item modifier type",
        problems,
    );
    check_conditions_list(json, path, problems);
    let kind = type_name(json);
    require(json, path, item_modifier_fields(kind), problems);
    match kind {
        "sequence" => {
            if let Some(functions) = json.get("functions").and_then(Value::as_array) {
                for (index, function) in functions.iter().enumerate() {
                    check_item_modifier(
                        function,
                        &format!("{}functions[{index}]", prefix(path)),
                        problems,
                    );
                }
            }
        }
        "enchant_randomly" => {
            if let Some(options) = json.get("options") {
                check_holder_set(
                    options,
                    &join(path, "options"),
                    "enchantment",
                    "an enchantment",
                    // Enchantment tags aren't listed, so they pass.
                    None,
                    problems,
                );
            }
        }
        _ => {}
    }
}

/// An item modifier field of a loot table: one modifier, a list, or the id
/// of an item modifier file.
fn check_loot_modifiers(json: &Value, path: &str, problems: &mut Problems) {
    if has(json, "functions") {
        problems.push(format!(
            "{} is now `modifier`; ignored, so the functions would never apply",
            at(path, "functions")
        ));
    }
    match json.get("modifier") {
        Some(Value::Array(modifiers)) => {
            for (index, modifier) in modifiers.iter().enumerate() {
                check_item_modifier(
                    modifier,
                    &format!("{}modifier[{}]", prefix(path), index),
                    problems,
                );
            }
        }
        Some(Value::String(id)) => {
            problems.refer(RefKind::ItemModifier, id, &join(path, "modifier"))
        }
        Some(modifier) => check_item_modifier(modifier, &join(path, "modifier"), problems),
        None => {}
    }
}

/// Loot tables: `functions` lists are now `modifier`, `conditions` lists are
/// a single `condition`, a `minecraft:tag` entry names its tag in `items`,
/// and pools and entries have the fields they can't go without.
pub(crate) fn check_loot_table(json: &Value, problems: &mut Problems) {
    check_loot_modifiers(json, "", problems);
    let Some(pools) = json.get("pools").and_then(Value::as_array) else {
        return;
    };
    for (pool_index, pool) in pools.iter().enumerate() {
        let pool_path = format!("pools[{}]", pool_index);
        check_conditions_list(pool, &pool_path, problems);
        check_loot_modifiers(pool, &pool_path, problems);
        if pool.is_object() {
            require(pool, &pool_path, &["rolls", "entries"], problems);
        }
        if let Some(entries) = pool.get("entries").and_then(Value::as_array) {
            for (entry_index, entry) in entries.iter().enumerate() {
                check_loot_entry(
                    entry,
                    &format!("{}.entries[{}]", pool_path, entry_index),
                    problems,
                );
            }
        }
    }
}

fn check_loot_entry(entry: &Value, path: &str, problems: &mut Problems) {
    if !entry.is_object() {
        return;
    }
    check_conditions_list(entry, path, problems);
    check_loot_modifiers(entry, path, problems);
    if !has(entry, "type") {
        problems.push(format!("{} is missing; rejected", at(path, "type")));
    }
    check_registry_id(
        entry.get("type"),
        path,
        "type",
        LOOT_ENTRY_TYPES,
        "a loot entry type",
        problems,
    );
    match type_name(entry) {
        "item" => match entry.get("name").and_then(Value::as_str) {
            None => require(entry, path, &["name"], problems),
            Some(id) if vanilla_name(id) == Some("air") => problems.push(format!(
                "{} `{id}` is not an item a loot table can drop; for a pool entry that drops nothing, use {{\"type\": \"minecraft:empty\"}}; rejected",
                at(path, "name")
            )),
            Some(id) if id.starts_with('#') => problems.push(format!(
                "{} `{id}` names a tag; a `minecraft:tag` entry drops the items of a tag (\"type\": \"minecraft:tag\", \"items\": \"{id}\"); rejected",
                at(path, "name")
            )),
            Some(id) => check_vanilla_id(id, "item", "an item", &join(path, "name"), problems),
        },
        "tag" => {
            if has(entry, "name") && !has(entry, "items") {
                problems.push(format!(
                    "{} of a `minecraft:tag` entry is now `items` (\"items\": \"#minecraft:logs\"); rejected",
                    at(path, "name")
                ));
            } else {
                match entry.get("items") {
                    None => require(entry, path, &["items"], problems),
                    Some(Value::String(id)) if !id.starts_with('#') => problems.push(format!(
                        "{} of a `minecraft:tag` entry names a tag, written with `#` (\"items\": \"#{}\"); rejected",
                        at(path, "items"),
                        id
                    )),
                    Some(Value::String(id)) => {
                        problems.refer(RefKind::ItemTag, &id[1..], &join(path, "items"))
                    }
                    Some(_) => {}
                }
            }
        }
        "loot_table" => {
            if has(entry, "name") && !has(entry, "value") {
                problems.push(format!(
                    "{} of a `minecraft:loot_table` entry is now `value` (\"value\": \"minecraft:chests/simple_dungeon\"); rejected",
                    at(path, "name")
                ));
            } else {
                match entry.get("value") {
                    None => require(entry, path, &["value"], problems),
                    Some(Value::String(id)) => {
                        problems.refer(RefKind::LootTable, id, &join(path, "value"))
                    }
                    Some(table) if table.is_object() => check_loot_table(table, problems),
                    Some(_) => {}
                }
            }
        }
        _ => {}
    }
    if let Some(children) = entry.get("children").and_then(Value::as_array) {
        for (index, child) in children.iter().enumerate() {
            check_loot_entry(child, &format!("{}.children[{}]", path, index), problems);
        }
    }
}

/// The triggers whose conditions have an `entity` field. Minecraft ignores
/// the field on the others.
const ENTITY_TRIGGERS: &[&str] = &[
    "player_interacted_with_entity",
    "player_killed_entity",
    "player_sheared_equipment",
    "summoned_entity",
    "tame_animal",
    "thrown_item_picked_up_by_entity",
    "thrown_item_picked_up_by_player",
    "entity_killed_player",
    "kill_mob_near_sculk_catalyst",
    "player_hurt_entity",
];

/// Advancements: criteria and their triggers, `requirements`, `display`,
/// `parent`, and `rewards`.
pub(super) fn check_advancement(json: &Value, problems: &mut Problems) {
    let criteria = json.get("criteria").and_then(Value::as_object);
    match criteria {
        None => require(json, "", &["criteria"], problems),
        Some(criteria) if criteria.is_empty() => problems.push(
            "`criteria` is empty; an advancement needs at least one criterion; rejected"
                .to_string(),
        ),
        Some(_) => {}
    }
    for (name, criterion) in criteria.into_iter().flatten() {
        let criterion_path = format!("criteria.{name}");
        if criterion.is_object() && !has(criterion, "trigger") {
            problems.push(format!(
                "{} is missing; rejected",
                at(&criterion_path, "trigger")
            ));
        }
        check_registry_id(
            criterion.get("trigger"),
            &criterion_path,
            "trigger",
            TRIGGER_TYPES,
            "an advancement trigger",
            problems,
        );
        let trigger = criterion
            .get("trigger")
            .and_then(Value::as_str)
            .unwrap_or("");
        let trigger = trigger.strip_prefix("minecraft:").unwrap_or(trigger);
        let Some(conditions) = criterion.get("conditions") else {
            continue;
        };
        let conditions_path = format!("{criterion_path}.conditions");
        if let Some(condition) = conditions.get("player") {
            check_entity_condition(condition, &join(&conditions_path, "player"), problems);
        }
        if let Some(condition) = conditions.get("entity") {
            let path = join(&conditions_path, "entity");
            if ENTITY_TRIGGERS.contains(&trigger) {
                check_entity_condition(condition, &path, problems);
            } else if !trigger.is_empty() {
                problems.warn(format!(
                    "`{path}` is ignored: `minecraft:{trigger}` has no `entity` condition"
                ));
            }
        }
        if let Some(item) = conditions.get("item") {
            check_item_predicate(item, &join(&conditions_path, "item"), problems);
        }
        if trigger == "recipe_unlocked" {
            if let Some(recipe) = conditions.get("recipe").and_then(Value::as_str) {
                problems.refer(RefKind::Recipe, recipe, &join(&conditions_path, "recipe"));
            }
            if let Some(recipes) = conditions.get("recipes").and_then(Value::as_array) {
                for (index, recipe) in recipes.iter().enumerate() {
                    if let Some(recipe) = recipe.as_str() {
                        problems.refer(
                            RefKind::Recipe,
                            recipe,
                            &format!("{conditions_path}.recipes[{index}]"),
                        );
                    }
                }
            }
        }
    }
    check_requirements(json, criteria, problems);
    if let Some(parent) = json.get("parent").and_then(Value::as_str) {
        problems.refer(RefKind::Advancement, parent, "parent");
    }
    if let Some(rewards) = json.get("rewards") {
        if let Some(loot) = rewards.get("loot").and_then(Value::as_array) {
            for (index, table) in loot.iter().enumerate() {
                if let Some(table) = table.as_str() {
                    problems.refer(RefKind::LootTable, table, &format!("rewards.loot[{index}]"));
                }
            }
        }
    }
    // `"display": None` is the same as no display.
    let Some(display) = json.get("display").filter(|display| !display.is_null()) else {
        return;
    };
    if display.is_object() {
        require(
            display,
            "display",
            &["icon", "title", "description"],
            problems,
        );
    }
    if let Some(icon) = display.get("icon") {
        check_item_stack(icon, "display.icon", problems);
    }
    if let Some(frame) = display.get("frame").and_then(Value::as_str) {
        if !matches!(frame, "task" | "challenge" | "goal") {
            problems.push(format!(
                "`display.frame` `{frame}` is not a frame; use \"task\", \"goal\", or \"challenge\"; rejected"
            ));
        }
    }
    if !has(json, "parent") && display.get("background").is_none() {
        problems.push(
            "a root advancement with `display` must set `display.background` (or give it a `parent`); rejected"
                .to_string(),
        );
    }
}

/// `requirements` is a list of lists of criterion names, and every criterion
/// is in one of them.
fn check_requirements(
    json: &Value,
    criteria: Option<&serde_json::Map<String, Value>>,
    problems: &mut Problems,
) {
    let Some(requirements) = json.get("requirements").and_then(Value::as_array) else {
        return;
    };
    let mut named = std::collections::HashSet::new();
    for (index, group) in requirements.iter().enumerate() {
        let Some(group) = group.as_array() else {
            problems.push(format!(
                "`requirements[{index}]` must be a list of criterion names, as in [[\"a\"], [\"b\", \"c\"]]; rejected"
            ));
            continue;
        };
        for (item_index, name) in group.iter().enumerate() {
            let Some(name) = name.as_str() else {
                continue;
            };
            named.insert(name);
            if criteria.is_some_and(|criteria| !criteria.contains_key(name)) {
                problems.push(format!(
                    "`requirements[{index}][{item_index}]` names the criterion `{name}`, which `criteria` doesn't have; rejected"
                ));
            }
        }
    }
    for name in criteria.into_iter().flat_map(|criteria| criteria.keys()) {
        if !named.contains(name.as_str()) {
            problems.push(format!(
                "the criterion `{name}` is in no list of `requirements`; rejected"
            ));
        }
    }
}

/// A `player` or `entity` condition of an advancement criterion is one
/// predicate with a `type`. A list of predicates and a bare entity predicate
/// were the older forms.
fn check_entity_condition(condition: &Value, path: &str, problems: &mut Problems) {
    if let Some(list) = condition.as_array() {
        problems.push(format!(
            "`{path}` is now one predicate, not a list (combine several with \"type\": \"minecraft:all_of\"); rejected"
        ));
        for (index, predicate) in list.iter().enumerate() {
            check_predicate(predicate, &format!("{path}[{index}]"), problems);
        }
        return;
    }
    if !condition.is_object() || has(condition, "type") {
        check_predicate(condition, path, problems);
        return;
    }
    if has(condition, "condition") {
        check_predicate(condition, path, problems);
    } else {
        problems.push(format!(
            "`{path}` is now a predicate with a `type`; wrap an entity predicate as {{\"type\": \"minecraft:entity_properties\", \"entity\": \"this\", \"predicate\": {{...}}}}; rejected"
        ));
    }
}

/// An item stack names its item in `id` (formerly `item`), and the item
/// exists.
fn check_item_stack(stack: &Value, path: &str, problems: &mut Problems) {
    if has(stack, "item") && !has(stack, "id") {
        problems.push(format!(
            "{} is now `id` (\"id\": \"minecraft:diamond\"); rejected",
            at(path, "item")
        ));
        return;
    }
    if let Some(id) = stack.get("id").and_then(Value::as_str) {
        check_vanilla_id(id, "item", "an item", &join(path, "id"), problems);
    }
}

/// A recipe ingredient is an item id, a `#tag`, or a list of item ids
/// (formerly an `{"item": ...}` or `{"tag": ...}` object).
fn check_ingredient(ingredient: &Value, path: &str, problems: &mut Problems) {
    match ingredient {
        Value::Object(fields) if fields.contains_key("item") => problems.push(format!(
            "`{path}` is now the item id itself (\"minecraft:stick\"); rejected"
        )),
        Value::Object(fields) if fields.contains_key("tag") => problems.push(format!(
            "`{path}` is now the tag as a string (\"#minecraft:logs\"); rejected"
        )),
        Value::Array(items) if items.is_empty() => problems.push(format!(
            "`{path}` is an empty list; an ingredient names at least one item; rejected"
        )),
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                if item.is_object() {
                    check_ingredient(item, &format!("{path}[{index}]"), problems);
                }
            }
            check_items(ingredient, path, problems);
        }
        _ => check_items(ingredient, path, problems),
    }
}

/// The ingredient fields of each recipe type, besides `key` and
/// `ingredients`.
fn ingredient_fields(kind: &str) -> &'static [&'static str] {
    match kind {
        "smelting" | "blasting" | "smoking" | "campfire_cooking" | "stonecutting" => {
            &["ingredient"]
        }
        "smithing_transform" | "smithing_trim" => &["template", "base", "addition"],
        "crafting_transmute" => &["input", "material"],
        "crafting_dye" => &["target", "dye"],
        "crafting_imbue" => &["source", "material"],
        "crafting_decorated_pot" => &["back", "front", "left", "right"],
        "crafting_special_bannerduplicate" => &["banner"],
        "crafting_special_bookcloning" => &["source", "material"],
        "crafting_special_firework_rocket" => &["shell", "fuel", "star"],
        "crafting_special_firework_star" => &["fuel", "dye", "trail", "twinkle"],
        "crafting_special_firework_star_fade" => &["target", "dye"],
        "crafting_special_mapextending" => &["map", "material"],
        "crafting_special_shielddecoration" => &["target", "banner"],
        // The `input` and `reagent` of a brewing recipe are objects that
        // name a potion and an item.
        "brewing" => &[],
        _ => &[
            "ingredient",
            "template",
            "base",
            "addition",
            "input",
            "material",
        ],
    }
}

/// Recipes: the type must exist, ingredients are ids or tags, a shaped
/// pattern matches its key, the result names an item, and cooking recipes
/// set a whole `cookingtime`.
pub(super) fn check_recipe(json: &Value, problems: &mut Problems) {
    if json.is_object() && !has(json, "type") {
        problems.push("`type` is missing; rejected".to_string());
    }
    check_registry_id(
        json.get("type"),
        "",
        "type",
        RECIPE_TYPES,
        "a recipe type",
        problems,
    );
    let recipe_type = type_name(json);
    if let Some(key) = json.get("key").and_then(Value::as_object) {
        for (symbol, ingredient) in key {
            check_ingredient(ingredient, &format!("key.{symbol}"), problems);
        }
    }
    if let Some(ingredients) = json.get("ingredients").and_then(Value::as_array) {
        for (index, ingredient) in ingredients.iter().enumerate() {
            check_ingredient(ingredient, &format!("ingredients[{index}]"), problems);
        }
    }
    // The older ingredient fields of any type are checked too, so an
    // `{"item": ...}` object is caught where a type doesn't read it.
    let older: &[&str] = if recipe_type == "brewing" {
        &[]
    } else {
        &[
            "ingredient",
            "template",
            "base",
            "addition",
            "input",
            "material",
        ]
    };
    for field in ingredient_fields(recipe_type) {
        if let Some(ingredient) = json.get(*field) {
            check_ingredient(ingredient, field, problems);
        }
    }
    for field in older {
        if !ingredient_fields(recipe_type).contains(field) {
            if let Some(ingredient) = json.get(*field) {
                check_ingredient(ingredient, field, problems);
            }
        }
    }
    if recipe_type == "crafting_special_firework_star" {
        if let Some(shapes) = json.get("shapes").and_then(Value::as_object) {
            for (shape, ingredient) in shapes {
                check_ingredient(ingredient, &format!("shapes.{shape}"), problems);
            }
        }
    }
    if recipe_type == "crafting_shaped" {
        check_shaped_pattern(json, problems);
    }
    if recipe_type == "crafting_shapeless" {
        if let Some(ingredients) = json.get("ingredients").and_then(Value::as_array) {
            if ingredients.is_empty() || ingredients.len() > 9 {
                problems.push(format!(
                    "`ingredients` has {} ingredients; a shapeless recipe takes 1 to 9; rejected",
                    ingredients.len()
                ));
            }
        }
    }
    check_recipe_category(json, recipe_type, problems);
    // The types whose result is an item stack with an `id`. A transmute
    // result may leave out its id, and some special recipes have none.
    let stack_result = matches!(
        recipe_type,
        "crafting_shaped"
            | "crafting_shapeless"
            | "stonecutting"
            | "smelting"
            | "blasting"
            | "smoking"
            | "campfire_cooking"
            | "smithing_transform"
    );
    if (stack_result || recipe_type == "crafting_dye") && json.is_object() {
        require(json, "", &["result"], problems);
    }
    if let Some(result) = json.get("result").filter(|result| result.is_object()) {
        check_item_stack(result, "result", problems);
        if stack_result && !has(result, "id") && !has(result, "item") {
            problems.push("`result.id` is missing; rejected".to_string());
        }
        if let Some(id) = result.get("id").and_then(Value::as_str) {
            if vanilla_name(id) == Some("air") {
                problems.push(format!(
                    "`result.id` `{id}` is not an item a recipe can make; rejected"
                ));
            }
        }
        if let Some(count) = result.get("count").and_then(Value::as_f64) {
            if !(1.0..=99.0).contains(&count) {
                problems.push(format!(
                    "`result.count` is {count}; a recipe makes 1 to 99 items; rejected"
                ));
            }
        }
    }
    if matches!(
        recipe_type,
        "smelting" | "blasting" | "smoking" | "campfire_cooking"
    ) {
        match json.get("cookingtime") {
            None => problems.push(format!(
                "`minecraft:{}` recipes must set `cookingtime` (200 was the smelting default); rejected",
                recipe_type
            )),
            Some(time) if !time.is_number() => problems.push(format!(
                "`cookingtime` is a number of ticks, such as 200, not {time}; rejected"
            )),
            Some(_) => {}
        }
    }
}

/// A recipe book `category`: one of the crafting or the cooking categories.
fn check_recipe_category(json: &Value, recipe_type: &str, problems: &mut Problems) {
    let categories: &[&str] = match recipe_type {
        "crafting_shaped" | "crafting_shapeless" | "crafting_transmute" => {
            &["building", "redstone", "equipment", "misc"]
        }
        "smelting" | "blasting" | "smoking" | "campfire_cooking" => &["food", "blocks", "misc"],
        _ => return,
    };
    let Some(category) = json.get("category").and_then(Value::as_str) else {
        return;
    };
    if !categories.contains(&category) {
        problems.push(format!(
            "`category` `{category}` is not a category of this recipe; use {}; rejected",
            categories
                .iter()
                .map(|category| format!("\"{category}\""))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
}

/// A shaped recipe's `pattern` is up to 3 rows of up to 3 symbols, all rows
/// as wide, and its `key` defines every symbol but the space and no other.
fn check_shaped_pattern(json: &Value, problems: &mut Problems) {
    let Some(rows) = json.get("pattern").and_then(Value::as_array) else {
        return;
    };
    let rows: Vec<&str> = rows.iter().filter_map(Value::as_str).collect();
    if rows.is_empty() || rows.len() > 3 {
        problems.push(format!(
            "`pattern` has {} rows; a shaped recipe has 1 to 3; rejected",
            rows.len()
        ));
    }
    let width = rows.first().map_or(0, |row| row.chars().count());
    if width == 0 || width > 3 {
        problems.push(format!(
            "`pattern` rows are {width} symbols wide; a shaped recipe is 1 to 3 wide; rejected"
        ));
    }
    if rows.iter().any(|row| row.chars().count() != width) {
        problems.push("`pattern` rows must all be as wide; rejected".to_string());
    }
    let Some(key) = json.get("key").and_then(Value::as_object) else {
        return;
    };
    for symbol in key.keys() {
        if symbol == " " {
            problems.push(
                "`key` can't define \" \", which is an empty slot in the pattern; rejected"
                    .to_string(),
            );
        } else if symbol.chars().count() != 1 {
            problems.push(format!("`key.{symbol}` must be one symbol; rejected"));
        } else if !rows.iter().any(|row| row.contains(symbol.as_str())) {
            problems.push(format!("`key.{symbol}` is not used in `pattern`; rejected"));
        }
    }
    let mut missing: Vec<char> = rows
        .iter()
        .flat_map(|row| row.chars())
        .filter(|symbol| *symbol != ' ' && !key.contains_key(&symbol.to_string()))
        .collect();
    missing.sort_unstable();
    missing.dedup();
    for symbol in missing {
        problems.push(format!(
            "`pattern` uses `{symbol}`, which `key` doesn't define; rejected"
        ));
    }
}

/// The fields a dialog can't go without, and the dialogs it names.
pub(crate) fn check_dialog(json: &Value, problems: &mut Problems) {
    check_dialog_at(json, "", problems);
}

fn check_dialog_at(json: &Value, path: &str, problems: &mut Problems) {
    let Some(kind) = json.get("type").and_then(Value::as_str) else {
        problems.push(format!("{} is missing; rejected", at(path, "type")));
        return;
    };
    check_registry_id(
        Some(&Value::from(kind)),
        path,
        "type",
        DIALOG_TYPES,
        "a dialog type",
        problems,
    );
    let required: &[&str] = match kind.trim_start_matches("minecraft:") {
        "confirmation" => &["title", "yes", "no"],
        "multi_action" => &["title", "actions"],
        "dialog_list" => &["title", "dialogs"],
        _ => &["title"],
    };
    require(json, path, required, problems);
    match json.get("dialogs") {
        Some(Value::String(id)) => match id.strip_prefix('#') {
            Some(tag) => problems.refer(RefKind::DialogTag, tag, &join(path, "dialogs")),
            None => problems.refer(RefKind::Dialog, id, &join(path, "dialogs")),
        },
        Some(Value::Array(dialogs)) => {
            for (index, dialog) in dialogs.iter().enumerate() {
                let dialog_path = format!("{}dialogs[{index}]", prefix(path));
                match dialog {
                    Value::String(id) => problems.refer(RefKind::Dialog, id, &dialog_path),
                    Value::Object(_) => check_dialog_at(dialog, &dialog_path, problems),
                    _ => {}
                }
            }
        }
        _ => {}
    }
    let mut buttons: Vec<(String, &Value)> = Vec::new();
    for field in ["action", "yes", "no", "exit_action"] {
        if let Some(button) = json.get(field) {
            buttons.push((join(path, field), button));
        }
    }
    if let Some(actions) = json.get("actions").and_then(Value::as_array) {
        for (index, button) in actions.iter().enumerate() {
            buttons.push((format!("{}actions[{index}]", prefix(path)), button));
        }
    }
    for (button_path, button) in buttons {
        let Some(action) = button.get("action") else {
            continue;
        };
        let kind = type_name(action);
        if kind != "show_dialog" {
            continue;
        }
        let action_path = join(&button_path, "action.dialog");
        match action.get("dialog") {
            Some(Value::String(id)) => problems.refer(RefKind::Dialog, id, &action_path),
            Some(dialog) if dialog.is_object() => check_dialog_at(dialog, &action_path, problems),
            _ => {}
        }
    }
}
