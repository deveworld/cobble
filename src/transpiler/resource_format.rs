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

/// Reports a `type`-like id that `known` doesn't list. `what` names the
/// registry in the message.
fn check_registry_id(
    value: Option<&Value>,
    path: &str,
    key: &str,
    known: &[&str],
    what: &str,
    problems: &mut Vec<String>,
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
    if path.is_empty() {
        format!("`{}`", key)
    } else {
        format!("`{}.{}`", path, key)
    }
}

/// Predicates: `condition` is now `type`, including the predicates nested in
/// `all_of`/`any_of` terms and `inverted`.
pub(super) fn check_predicate(json: &Value, path: &str, problems: &mut Vec<String>) {
    if !json.is_object() {
        return;
    }
    if has(json, "condition") && !has(json, "type") {
        problems.push(format!(
            "{} is now `type` (\"type\": \"minecraft:random_chance\"); rejected",
            at(path, "condition")
        ));
    }
    check_registry_id(
        json.get("type"),
        path,
        "type",
        LOOT_CONDITION_TYPES,
        "a predicate type",
        problems,
    );
    if let Some(terms) = json.get("terms").and_then(Value::as_array) {
        for (index, term) in terms.iter().enumerate() {
            check_predicate(term, &format!("{}terms[{}]", prefix(path), index), problems);
        }
    }
    if let Some(term) = json.get("term") {
        check_predicate(term, &format!("{}term", prefix(path)), problems);
    }
}

fn prefix(path: &str) -> String {
    if path.is_empty() {
        String::new()
    } else {
        format!("{}.", path)
    }
}

/// `conditions` lists on loot functions, pools, and entries are now a single
/// `condition` (use `minecraft:all_of` to combine several).
fn check_conditions_list(json: &Value, path: &str, problems: &mut Vec<String>) {
    if has(json, "conditions") {
        problems.push(format!(
            "{} is now a single `condition` (combine several with \"type\": \"minecraft:all_of\"); ignored, so the conditions would never apply",
            at(path, "conditions")
        ));
    }
    if let Some(condition) = json.get("condition") {
        check_predicate(condition, &format!("{}condition", prefix(path)), problems);
    }
}

/// Item modifiers (loot functions): `function` is now `type`, and
/// `conditions` is a single `condition`.
pub(super) fn check_item_modifier(json: &Value, path: &str, problems: &mut Vec<String>) {
    if !json.is_object() {
        return;
    }
    if has(json, "function") && !has(json, "type") {
        problems.push(format!(
            "{} is now `type` (\"type\": \"minecraft:set_components\"); rejected",
            at(path, "function")
        ));
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
}

/// Loot tables: `functions` lists are now `modifier`, `conditions` lists are
/// a single `condition`, and a `minecraft:tag` entry names its tag in
/// `items` instead of `name`.
pub(super) fn check_loot_table(json: &Value, problems: &mut Vec<String>) {
    check_loot_modifiers(json, "", problems);
    let Some(pools) = json.get("pools").and_then(Value::as_array) else {
        return;
    };
    for (pool_index, pool) in pools.iter().enumerate() {
        let pool_path = format!("pools[{}]", pool_index);
        check_conditions_list(pool, &pool_path, problems);
        check_loot_modifiers(pool, &pool_path, problems);
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

fn check_loot_modifiers(json: &Value, path: &str, problems: &mut Vec<String>) {
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
        Some(modifier) => {
            check_item_modifier(modifier, &format!("{}modifier", prefix(path)), problems)
        }
        None => {}
    }
}

fn check_loot_entry(entry: &Value, path: &str, problems: &mut Vec<String>) {
    check_conditions_list(entry, path, problems);
    check_loot_modifiers(entry, path, problems);
    check_registry_id(
        entry.get("type"),
        path,
        "type",
        LOOT_ENTRY_TYPES,
        "a loot entry type",
        problems,
    );
    let entry_type = entry.get("type").and_then(Value::as_str).unwrap_or("");
    if matches!(entry_type, "tag" | "minecraft:tag") && has(entry, "name") && !has(entry, "items") {
        problems.push(format!(
            "{} of a `minecraft:tag` entry is now `items` (\"items\": \"#minecraft:logs\"); rejected",
            at(path, "name")
        ));
    }
    if let Some(children) = entry.get("children").and_then(Value::as_array) {
        for (index, child) in children.iter().enumerate() {
            check_loot_entry(child, &format!("{}.children[{}]", path, index), problems);
        }
    }
}

/// Advancements: a visible root advancement (`display` without `parent`)
/// must set `display.background`, and a criterion's `player` and `entity`
/// conditions are one predicate.
pub(super) fn check_advancement(json: &Value, problems: &mut Vec<String>) {
    if let Some(criteria) = json.get("criteria").and_then(Value::as_object) {
        for (name, criterion) in criteria {
            check_registry_id(
                criterion.get("trigger"),
                &format!("criteria.{name}"),
                "trigger",
                TRIGGER_TYPES,
                "an advancement trigger",
                problems,
            );
            let Some(conditions) = criterion.get("conditions") else {
                continue;
            };
            for field in ["player", "entity"] {
                if let Some(condition) = conditions.get(field) {
                    let path = format!("criteria.{name}.conditions.{field}");
                    check_entity_condition(condition, &path, problems);
                }
            }
        }
    }
    let Some(display) = json.get("display") else {
        return;
    };
    if let Some(icon) = display.get("icon") {
        check_item_stack(icon, "display.icon", problems);
    }
    if !has(json, "parent") && display.get("background").is_none() {
        problems.push(
            "a root advancement with `display` must set `display.background` (or give it a `parent`); rejected"
                .to_string(),
        );
    }
}

/// A `player` or `entity` condition of an advancement criterion is one
/// predicate with a `type`. A list of predicates and a bare entity predicate
/// were the older forms.
fn check_entity_condition(condition: &Value, path: &str, problems: &mut Vec<String>) {
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

/// An item stack names its item in `id` (formerly `item`).
fn check_item_stack(stack: &Value, path: &str, problems: &mut Vec<String>) {
    if has(stack, "item") && !has(stack, "id") {
        problems.push(format!(
            "{} is now `id` (\"id\": \"minecraft:diamond\"); rejected",
            at(path, "item")
        ));
    }
}

/// A recipe ingredient is an item id, a `#tag`, or a list of item ids
/// (formerly an `{"item": ...}` or `{"tag": ...}` object).
fn check_ingredient(ingredient: &Value, path: &str, problems: &mut Vec<String>) {
    match ingredient {
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                check_ingredient(item, &format!("{path}[{index}]"), problems);
            }
        }
        Value::Object(fields) if fields.contains_key("item") => problems.push(format!(
            "`{path}` is now the item id itself (\"minecraft:stick\"); rejected"
        )),
        Value::Object(fields) if fields.contains_key("tag") => problems.push(format!(
            "`{path}` is now the tag as a string (\"#minecraft:logs\"); rejected"
        )),
        _ => {}
    }
}

/// Recipes: the type must exist, ingredients are ids or tags, the result
/// names its item in `id`, and cooking recipes must set `cookingtime` (it
/// no longer has a default).
pub(super) fn check_recipe(json: &Value, problems: &mut Vec<String>) {
    check_registry_id(
        json.get("type"),
        "",
        "type",
        RECIPE_TYPES,
        "a recipe type",
        problems,
    );
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
    for field in [
        "ingredient",
        "template",
        "base",
        "addition",
        "input",
        "material",
    ] {
        if let Some(ingredient) = json.get(field) {
            check_ingredient(ingredient, field, problems);
        }
    }
    if let Some(result) = json.get("result").filter(|result| result.is_object()) {
        check_item_stack(result, "result", problems);
    }
    let recipe_type = json.get("type").and_then(Value::as_str).unwrap_or("");
    let recipe_type = recipe_type
        .strip_prefix("minecraft:")
        .unwrap_or(recipe_type);
    if matches!(
        recipe_type,
        "smelting" | "blasting" | "smoking" | "campfire_cooking"
    ) && !has(json, "cookingtime")
    {
        problems.push(format!(
            "`minecraft:{}` recipes must set `cookingtime` (200 was the smelting default); rejected",
            recipe_type
        ));
    }
}
