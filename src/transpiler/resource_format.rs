//! Shape checks for `datapack.*` JSON resources against the Minecraft
//! version Cobble targets. Each check appends a human-readable problem for a
//! pre-26.3 field: Minecraft 26.3 either rejects the resource (and a server
//! whose packs fail to load refuses to start) or silently ignores the field.
//! Every rule was confirmed against a 26.3 server.

use serde_json::Value;

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
}

fn check_loot_entry(entry: &Value, path: &str, problems: &mut Vec<String>) {
    check_conditions_list(entry, path, problems);
    check_loot_modifiers(entry, path, problems);
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
/// must set `display.background`.
pub(super) fn check_advancement(json: &Value, problems: &mut Vec<String>) {
    let Some(display) = json.get("display") else {
        return;
    };
    if !has(json, "parent") && display.get("background").is_none() {
        problems.push(
            "a root advancement with `display` must set `display.background` (or give it a `parent`); rejected"
                .to_string(),
        );
    }
}

/// Cooking recipes must set `cookingtime` (it no longer has a default).
pub(super) fn check_recipe(json: &Value, problems: &mut Vec<String>) {
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
