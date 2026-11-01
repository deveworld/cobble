//! Checks for arguments Minecraft looks up when it loads a function, such as
//! item, block, and entity ids. An unknown id stops the whole function from
//! loading, and every function that calls it. `registries.json` comes from
//! `scripts/update_registries.py`.
//!
//! Only `minecraft:` ids are checked, so a modded server's own ids pass, and
//! tags and values with a `$(name)` placeholder are skipped. A data pack can
//! add to some registries, so `PackResources` checks those against the pack
//! and the game.

use super::arg_parsers::{is_id, is_item_slot_value, split_top_level};
use super::string_reader::{is_java_whitespace, StringReader};
use super::values::{compound_problem, message_selectors, read_id, selector_arguments};
use super::{snbt, text_component};
use crate::transpiler::resource_format;
use serde::Deserialize;
use serde_json::Value;
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
    data_component: HashSet<String>,
    data_component_predicate: HashSet<String>,
    poi_type: HashSet<String>,
    potion: HashSet<String>,
    fluid: HashSet<String>,
    instrument: HashSet<String>,
    trim_pattern: HashSet<String>,
    block_tag: HashSet<String>,
    item_tag: HashSet<String>,
    biome_tag: HashSet<String>,
    poi_tag: HashSet<String>,
    loot_table: HashSet<String>,
    predicate: HashSet<String>,
    item_modifier: HashSet<String>,
    dialog: HashSet<String>,
    feature: HashSet<String>,
    context_int_provider: HashSet<String>,
    context_float_provider: HashSet<String>,
    slot_source: HashSet<String>,
    world_clock: HashSet<String>,
    timeline: HashSet<String>,
    entity_type_tag: HashSet<String>,
    dialog_tag: HashSet<String>,
    enchantment_tag: HashSet<String>,
    advancement: HashSet<String>,
    recipe: HashSet<String>,
    block_properties: HashMap<String, HashMap<String, Vec<String>>>,
}

/// The names of Minecraft's own entries of `registry`, without the
/// `minecraft:` namespace: `item`, `block`, `entity_type`, `mob_effect`,
/// `enchantment`, `potion`, `fluid`, `instrument`, `trim_pattern`,
/// `attribute`, `world_clock`, `context_int_provider`,
/// `context_float_provider`, the tags `block_tag`, `item_tag`,
/// `entity_type_tag`, `enchantment_tag`, `biome_tag`, and `dialog_tag`, and
/// the data pack resources `loot_table`, `predicate`, `item_modifier`,
/// `dialog`, `advancement`, and `recipe`. Any other name has no entries.
/// `None` where the lists aren't built in (the browser build), so nothing is
/// checked against them.
pub(crate) fn vanilla_ids(registry: &str) -> Option<&'static HashSet<String>> {
    static EMPTY: LazyLock<HashSet<String>> = LazyLock::new(HashSet::new);
    let registries = &*REGISTRIES;
    Some(match registry {
        "item" => &registries.item,
        "block" => &registries.block,
        "entity_type" => &registries.entity_type,
        "mob_effect" => &registries.mob_effect,
        "enchantment" => &registries.enchantment,
        "potion" => &registries.potion,
        "fluid" => &registries.fluid,
        "instrument" => &registries.instrument,
        "trim_pattern" => &registries.trim_pattern,
        "attribute" => &registries.attribute,
        "biome" => &registries.biome,
        "damage_type" => &registries.damage_type,
        "world_clock" => &registries.world_clock,
        "context_int_provider" => &registries.context_int_provider,
        "context_float_provider" => &registries.context_float_provider,
        "enchantment_tag" => &registries.enchantment_tag,
        "biome_tag" => &registries.biome_tag,
        "block_tag" => &registries.block_tag,
        "item_tag" => &registries.item_tag,
        "entity_type_tag" => &registries.entity_type_tag,
        "dialog_tag" => &registries.dialog_tag,
        "loot_table" => &registries.loot_table,
        "predicate" => &registries.predicate,
        "item_modifier" => &registries.item_modifier,
        "dialog" => &registries.dialog,
        "advancement" => &registries.advancement,
        "recipe" => &registries.recipe,
        _ => &EMPTY,
    })
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
/// The options a particle can't go without, as in `dust{color:[1,0,0],scale:1}`.
const PARTICLE_OPTIONS: &[(&str, &[&str])] = &[
    ("block", &["block_state"]),
    ("block_crumble", &["block_state"]),
    ("block_marker", &["block_state"]),
    ("dust", &["color", "scale"]),
    (
        "dust_color_transition",
        &["from_color", "to_color", "scale"],
    ),
    ("dust_pillar", &["block_state"]),
    ("entity_effect", &["color"]),
    ("falling_dust", &["block_state"]),
    ("flash", &["color"]),
    ("geyser", &["water_blocks"]),
    ("geyser_base", &["water_blocks", "burst_impulse_base"]),
    ("geyser_plume", &["water_blocks"]),
    ("geyser_poof", &["water_blocks", "burst_impulse_base"]),
    ("item", &["item"]),
    ("sculk_charge", &["roll"]),
    ("shriek", &["delay"]),
    ("tinted_leaves", &["color"]),
    ("trail", &["target", "color", "duration"]),
    ("vibration", &["destination", "arrival_in_ticks"]),
];
const RARITIES: &[&str] = &["common", "uncommon", "rare", "epic"];
/// The name part of a `minecraft:` id at the start of `token`, up to its
/// `[` or `{`. `None` for a tag, a wildcard, or another namespace.
fn vanilla_name(token: &str) -> Option<&str> {
    let id = id_part(token);
    if id.starts_with('#') || id.starts_with('*') {
        return None;
    }
    match id_parts(id) {
        ("minecraft", name) => Some(name),
        _ => None,
    }
}

/// `token` up to its `[` or `{`.
fn id_part(token: &str) -> &str {
    token
        .split(['[', '{'])
        .next()
        .unwrap_or(token)
        .trim_end_matches(is_java_whitespace)
}

fn unknown(what: &str, token: &str) -> Option<String> {
    Some(format!("Unknown {what} `{}`", id_part(token)))
}

/// What is wrong with the argument `token` of the parser `parser_type`.
pub fn problem(
    parser_type: &str,
    properties: Option<&serde_json::Value>,
    token: &str,
) -> Option<String> {
    let token = token.trim_start_matches(is_java_whitespace);
    if token.contains("$(") {
        return None;
    }
    let registries = &*REGISTRIES;
    match parser_type {
        "minecraft:item_stack" => {
            if token.starts_with('#') {
                return Some(format!("`{token}` names a tag, and an item goes here"));
            }
            item_name_problem(token).or_else(|| components_problem(token))
        }
        "minecraft:item_predicate" => {
            item_name_problem(token).or_else(|| item_tests_problem(token))
        }
        "minecraft:block_state" | "minecraft:block_predicate" => {
            if parser_type == "minecraft:block_state" && token.starts_with('#') {
                return Some(format!("`{token}` names a tag, and a block goes here"));
            }
            block_problem(token)
        }
        "minecraft:particle" => particle_problem(token),
        "minecraft:loot_predicate"
        | "minecraft:loot_modifier"
        | "minecraft:loot_table"
        | "minecraft:dialog"
        | "minecraft:feature"
        | "minecraft:context_int_provider"
        | "minecraft:context_float_provider"
            if token.starts_with(['{', '[']) =>
        {
            inline_resource_problem(parser_type, token)
        }
        // Minecraft reads a plain number here as the id `minecraft:1`.
        "minecraft:context_int_provider" | "minecraft:context_float_provider"
            if !token.starts_with('+') && token.parse::<f64>().is_ok_and(f64::is_finite) =>
        {
            Some(format!(
                "A number provider is an id or a compound, not `{token}`: \
                 use `{{type:\"minecraft:constant\",value:{token}}}`"
            ))
        }
        "minecraft:resource" | "minecraft:resource_or_tag" => {
            let registry = properties?.get("registry")?.as_str()?;
            let (names, what) = match registry {
                "minecraft:entity_type" => (&registries.entity_type, "entity type"),
                "minecraft:mob_effect" => (&registries.mob_effect, "effect"),
                "minecraft:attribute" => (&registries.attribute, "attribute"),
                "minecraft:point_of_interest_type" => {
                    (&registries.poi_type, "point of interest type")
                }
                _ => return None,
            };
            let name = vanilla_name(token)?;
            (!names.contains(name))
                .then(|| unknown(what, token))
                .flatten()
        }
        "minecraft:objective_criteria" => criterion_problem(token),
        "minecraft:entity" | "minecraft:score_holder" | "minecraft:game_profile" => {
            selector_problem(token)
        }
        "minecraft:message" => message_selectors(token)
            .iter()
            .find_map(|selector| selector_problem(selector)),
        _ => None,
    }
}

/// What is wrong with the item a hover event shows.
pub(super) fn shown_item_problem(id: &str) -> Option<String> {
    if vanilla_name(id) == Some("air") {
        return Some("A hover event can't show `minecraft:air`".to_string());
    }
    item_name_problem(id)
}

pub(super) fn entity_type_problem(id: &str) -> Option<String> {
    vanilla_name(id)
        .is_some_and(|name| !REGISTRIES.entity_type.contains(name))
        .then(|| unknown("entity type", id))
        .flatten()
}

fn item_name_problem(token: &str) -> Option<String> {
    let name = vanilla_name(token)?;
    (!REGISTRIES.item.contains(name))
        .then(|| unknown("item", token))
        .flatten()
}

/// The `[...]` after the id at the start of `token`, and what follows it.
fn bracket_part(token: &str) -> Option<(String, String)> {
    let start = token
        .find(['[', '{'])
        .filter(|at| token[*at..].starts_with('['))?;
    let mut reader = StringReader::new(&token[start..]);
    if !reader.read_nbt() {
        return None;
    }
    let inner = reader.slice(1, reader.cursor() - 1);
    Some((inner, reader.remaining()))
}

/// The name of a component or a test at the cursor, which has to read as an
/// id. `what` says which of them it is.
fn read_component_name(reader: &mut StringReader, what: &str) -> Result<String, String> {
    let start = reader.cursor();
    let is_valid = read_id(reader);
    let name = reader.slice(start, reader.cursor());
    if !is_valid {
        return Err(format!("`{name}` is not the id of an {what}"));
    }
    if !name.is_empty() {
        return Ok(name);
    }
    let rest = reader.remaining();
    Err(if rest.is_empty() {
        format!("An {what} list ends in an empty entry")
    } else {
        format!("Unknown {what} `{rest}`")
    })
}

fn components_problem(token: &str) -> Option<String> {
    let (inner, _) = bracket_part(token)?;
    read_components(&inner).err()
}

/// `damage=1` and `!damage` in an item stack's `[...]`, the way Minecraft's
/// `ItemParser` reads them. A `,` can end the list, and spaces can't stand
/// after a `!` or in an empty list.
fn read_components(list: &str) -> Result<(), String> {
    let mut reader = StringReader::new(list);
    let mut seen = Vec::new();
    while reader.can_read() {
        reader.skip_whitespace();
        let removed = reader.peek() == Some('!');
        if removed {
            reader.read_char();
        }
        let name = read_component_name(&mut reader, "item component")?;
        let (namespace, short) = id_parts(&name);
        let id = format!("{namespace}:{short}");
        if seen.contains(&id) {
            return Err(format!("Item component `{short}` appears twice"));
        }
        // A component of another namespace, such as a mod's, goes unchecked.
        let vanilla = namespace == "minecraft";
        if vanilla && !REGISTRIES.data_component.contains(short) {
            return Err(format!("Unknown item component `{name}`"));
        }
        if !removed {
            reader.skip_whitespace();
            if reader.read_char() != Some('=') {
                return Err(format!("Item component `{name}` needs a value"));
            }
            reader.skip_whitespace();
            let value = snbt::read(&mut reader)?;
            vanilla
                .then(|| component_value_problem(short, &value))
                .flatten()
                .map_or(Ok(()), Err)?;
        }
        seen.push(id);
        reader.skip_whitespace();
        match reader.read_char() {
            None | Some(',') => reader.skip_whitespace(),
            Some(found) => return Err(format!("Item component `{name}` can't end in `{found}`")),
        }
    }
    Ok(())
}

fn component_value_problem(name: &str, parsed: &Value) -> Option<String> {
    let must_be = |shape: &str| Some(format!("Item component `{name}` must be {shape}"));
    match name {
        // Numbers and booleans here are the forms from before 1.21.5.
        "custom_model_data" | "unbreakable" if !parsed.is_object() => {
            must_be("a compound (`{...}`)")
        }
        "custom_data" if !parsed.is_object() => match parsed.as_str() {
            Some(text) => compound_problem(text),
            None => must_be("a compound or a string"),
        },
        "lore" | "attribute_modifiers" if !parsed.is_array() => must_be("a list"),
        "can_break" | "can_place_on"
            if parsed
                .get("predicates")
                .is_some_and(|tests| !tests.is_object()) =>
        {
            Some(format!("`predicates` of `{name}` must be a compound"))
        }
        "dyed_color" if !is_color(parsed) => must_be("a color number or a list of three numbers"),
        "damage" | "repair_cost" if !whole_number(parsed).is_some_and(|number| number >= 0) => {
            must_be("a number from 0")
        }
        "max_damage" if !whole_number(parsed).is_some_and(|number| number >= 1) => {
            must_be("a number from 1")
        }
        "max_stack_size" if !whole_number(parsed).is_some_and(|size| (1..=99).contains(&size)) => {
            must_be("a number from 1 to 99")
        }
        "item_model" if !parsed.as_str().is_some_and(is_id) => must_be("an id in quotes"),
        "food" if !parsed.is_object() => must_be("a compound"),
        "food" => food_problem(parsed),
        "rarity"
            if !parsed
                .as_str()
                .is_some_and(|rarity| RARITIES.contains(&rarity)) =>
        {
            must_be("common, uncommon, rare, or epic")
        }
        "enchantments" | "stored_enchantments" => enchantments_problem(name, parsed),
        "custom_name" | "item_name" => text_component::value_problem(parsed),
        "lore" => parsed
            .as_array()?
            .iter()
            .find_map(text_component::value_problem),
        _ => None,
    }
}

/// A value the way Minecraft reads a whole number from it: `true` is 1, and a
/// fraction and the high bits of a long are dropped.
pub(super) fn whole_number(value: &Value) -> Option<i32> {
    match value {
        Value::Bool(flag) => Some(i32::from(*flag)),
        Value::Number(number) => number
            .as_i64()
            .map(|number| number as i32)
            .or_else(|| number.as_f64().map(|number| number as i32)),
        _ => None,
    }
}

fn is_color(value: &Value) -> bool {
    whole_number(value).is_some()
        || value.as_array().is_some_and(|channels| {
            channels.len() == 3
                && channels
                    .iter()
                    .all(|channel| whole_number(channel).is_some())
        })
}

fn food_problem(food: &Value) -> Option<String> {
    let needs = |key: &str| Some(format!("Item component `food` needs a number `{key}`"));
    let number = |key: &str| food.get(key).and_then(whole_number);
    match (number("nutrition"), number("saturation")) {
        (None, _) => needs("nutrition"),
        (_, None) => needs("saturation"),
        (Some(nutrition), _) if nutrition < 0 => {
            Some("`nutrition` of `food` can't be negative".to_string())
        }
        _ => None,
    }
}

fn enchantments_problem(name: &str, enchantments: &Value) -> Option<String> {
    let Some(levels) = enchantments.as_object() else {
        return Some(format!("Item component `{name}` must be a compound"));
    };
    if levels.contains_key("levels") {
        return Some(format!(
            "`{name}` lists enchantments directly: `{{sharpness:1}}`, without `levels`"
        ));
    }
    levels
        .iter()
        .find(|(_, level)| !whole_number(level).is_some_and(|level| (1..=255).contains(&level)))
        .map(|(id, _)| format!("The level of `{id}` must be a number from 1 to 255"))
}

/// The enchantments that the `enchantments` and `stored_enchantments` of an
/// item's `[...]` name: the ids of `enchantments={sharpness:1}`, and the ids
/// and `#tags` of `enchantments~[{enchantments:"#minecraft:curse"}]`.
fn enchantment_ids(token: &str) -> Vec<String> {
    let Some((inner, _)) = bracket_part(token) else {
        return Vec::new();
    };
    let tests = split_top_level(&inner, ',')
        .into_iter()
        .flat_map(|part| split_top_level(&part, '|'));
    let mut ids = Vec::new();
    for test in tests {
        let Some(at) = test.find(['=', '~']) else {
            continue;
        };
        let (name, value) = (&test[..at], &test[at + 1..]);
        let name = name.trim_matches(|ch: char| ch == '!' || is_java_whitespace(ch));
        if !matches!(
            name.strip_prefix("minecraft:").unwrap_or(name),
            "enchantments" | "stored_enchantments"
        ) {
            continue;
        }
        match snbt::parse(value) {
            Ok(Value::Object(levels)) if test[at..].starts_with('=') => {
                ids.extend(levels.into_iter().map(|(id, _)| id));
            }
            Ok(Value::Array(tests)) if test[at..].starts_with('~') => {
                for named in tests.iter().filter_map(|test| test.get("enchantments")) {
                    match named {
                        Value::String(id) => ids.push(id.clone()),
                        Value::Array(list) => {
                            ids.extend(list.iter().filter_map(Value::as_str).map(str::to_string))
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    ids
}

fn item_tests_problem(token: &str) -> Option<String> {
    let (inner, _) = bracket_part(token)?;
    read_item_tests(&inner).err()
}

/// `damage=1`, `damage~{}`, `!damage`, and `a|b` in an item predicate's `[...]`,
/// where spaces can stand anywhere and a list can't end in `,` or `|`.
fn read_item_tests(list: &str) -> Result<(), String> {
    let mut reader = StringReader::new(list);
    reader.skip_whitespace();
    if !reader.can_read() {
        return Ok(());
    }
    loop {
        read_item_test(&mut reader)?;
        reader.skip_whitespace();
        match reader.read_char() {
            None => return Ok(()),
            Some(',' | '|') => {}
            Some(found) => return Err(format!("An item test can't end in `{found}`")),
        }
    }
}

fn read_item_test(reader: &mut StringReader) -> Result<(), String> {
    reader.skip_whitespace();
    if reader.peek() == Some('!') {
        reader.read_char();
        reader.skip_whitespace();
    }
    let name = read_component_name(reader, "item test")?;
    let (namespace, short) = id_parts(&name);
    let vanilla = namespace == "minecraft";
    let registries = &*REGISTRIES;
    let is_component = registries.data_component.contains(short);
    if vanilla
        && short != "count"
        && !is_component
        && !registries.data_component_predicate.contains(short)
    {
        return Err(format!("Unknown item test `{name}`"));
    }
    reader.skip_whitespace();
    let exact = match reader.peek() {
        Some('=') => true,
        Some('~') => false,
        _ => return Ok(()),
    };
    reader.read_char();
    reader.skip_whitespace();
    let value = snbt::read(reader)?;
    if vanilla
        && !exact
        && matches!(short, "enchantments" | "stored_enchantments")
        && !value.is_array()
    {
        return Err(format!(
            "`{name}~` takes a list of tests, as in `{name}~[{{enchantments:\"sharpness\",levels:{{min:1}}}}]`"
        ));
    }
    // `custom_data~` reads its value like `custom_data=`.
    let reads_as_component = vanilla && is_component && (exact || short == "custom_data");
    reads_as_component
        .then(|| component_value_problem(short, &value))
        .flatten()
        .map_or(Ok(()), Err)
}

/// The `[...]` of a block, the way Minecraft's `BlockStateParser` reads it.
/// `name` is `None` for a tag and a block of another namespace, whose
/// properties aren't known here. It reads the same, but any value goes after
/// `=`, or none.
fn read_block_states(name: Option<&str>, states: &str) -> Result<(), String> {
    let broken = || "A block state has a broken quoted string".to_string();
    let properties = name.and_then(|name| REGISTRIES.block_properties.get(name));
    let mut reader = StringReader::new(states);
    let mut seen = Vec::new();
    reader.skip_whitespace();
    while reader.can_read() {
        reader.skip_whitespace();
        let property = reader.read_string().ok_or_else(broken)?;
        if seen.contains(&property) {
            return Err(format!("Block state `{property}` appears twice"));
        }
        reader.skip_whitespace();
        if reader.read_char() != Some('=') {
            return Err(format!("Block state `{property}` needs a value"));
        }
        reader.skip_whitespace();
        let value = reader.read_string().ok_or_else(broken)?;
        if let Some(name) = name {
            if value.is_empty() {
                return Err(format!("Block state `{property}` needs a value"));
            }
            let Some(values) = properties.and_then(|properties| properties.get(&property)) else {
                return Err(format!("Block `{name}` has no property `{property}`"));
            };
            let number = value.parse::<i32>().ok();
            let same = |known: &String| {
                *known == value || (number.is_some() && known.parse::<i32>().ok() == number)
            };
            if !values.iter().any(same) {
                return Err(format!(
                    "Block `{name}` has no `{property}={value}` (use one of: {})",
                    values.join(", ")
                ));
            }
        }
        seen.push(property);
        reader.skip_whitespace();
        match reader.read_char() {
            None | Some(',') => {}
            Some(found) => return Err(format!("A block state can't end in `{found}`")),
        }
    }
    Ok(())
}

fn block_problem(token: &str) -> Option<String> {
    let name = vanilla_name(token);
    let registries = &*REGISTRIES;
    if name.is_some_and(|name| !registries.block.contains(name)) {
        return unknown("block", token);
    }
    let (states, data) = bracket_part(token).unwrap_or_default();
    let data = match token.find(['[', '{']) {
        Some(at) if token[at..].starts_with('{') => token[at..].to_string(),
        _ => data,
    };
    if let Err(problem) = read_block_states(name, &states) {
        return Some(problem);
    }
    if data.is_empty() {
        return None;
    }
    compound_problem(&data)
}

/// `text` without the quotes around it, if it has them.
pub(super) fn unquoted(text: &str) -> &str {
    ['"', '\'']
        .iter()
        .find_map(|quote| text.strip_prefix(*quote)?.strip_suffix(*quote))
        .unwrap_or(text)
}

fn particle_problem(token: &str) -> Option<String> {
    let name = vanilla_name(token)?;
    if !REGISTRIES.particle_type.contains(name) {
        return unknown("particle", token);
    }
    let options = match token.find('{') {
        Some(start) => match snbt::parse(&token[start..]) {
            Ok(options) => options,
            Err(problem) => return Some(problem),
        },
        None => Value::Null,
    };
    let (_, required) = PARTICLE_OPTIONS.iter().find(|(id, _)| *id == name)?;
    required
        .iter()
        .find(|key| options.get(**key).is_none())
        .map(|key| format!("Particle `{name}` needs `{key}`: `{name}{{{key}:...}}`"))
        .or_else(|| particle_block_problem(name, options.get("block_state")?))
}

/// The `block_state` of a particle: a block id, or `{id:...}` with optional
/// `properties`. The `Name` and `Properties` of a block in NBT don't read.
fn particle_block_problem(particle: &str, state: &Value) -> Option<String> {
    let shape = || {
        format!(
            "`block_state` of particle `{particle}` is a block id or `{{id:\"minecraft:stone\"}}`, with `properties` for its states"
        )
    };
    let id = match state {
        Value::String(id) => id,
        Value::Object(fields) => match fields.get("id") {
            Some(Value::String(id)) => id,
            Some(_) => return Some(shape()),
            None if fields.contains_key("Name") => {
                return Some(format!(
                    "`block_state` of particle `{particle}` names its block with `id`, not `Name`, and its states with `properties`; rejected"
                ))
            }
            None => return Some(shape()),
        },
        _ => return Some(shape()),
    };
    if !is_id(id) {
        return Some(shape());
    }
    match id_parts(id) {
        ("minecraft", name) if !REGISTRIES.block.contains(name) => unknown("block", id),
        _ => None,
    }
}

/// A resource written in place of its id.
fn inline_resource_problem(parser_type: &str, token: &str) -> Option<String> {
    let value = match snbt::parse(token) {
        Ok(value) => value,
        Err(problem) => return Some(problem),
    };
    let mut problems = resource_format::Problems {
        nbt: true,
        ..Default::default()
    };
    match parser_type {
        "minecraft:loot_predicate" if !value.is_object() => {
            problems.push("an inline predicate must be one compound".to_string())
        }
        "minecraft:loot_predicate" => resource_format::check_predicate(&value, "", &mut problems),
        "minecraft:loot_modifier" => match value.as_array() {
            Some(items) => items.iter().for_each(|item| {
                resource_format::check_item_modifier(item, "", &mut problems);
            }),
            None => resource_format::check_item_modifier(&value, "", &mut problems),
        },
        "minecraft:loot_table" => resource_format::check_loot_table(&value, &mut problems),
        "minecraft:dialog" => resource_format::check_dialog(&value, &mut problems),
        _ => {}
    }
    problems.errors.into_iter().next()
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
    let unknown = || Some(format!("Unknown scoreboard criterion `{criterion}`"));
    let Some((stat, value)) = criterion.split_once(':') else {
        return unknown();
    };
    // Both are ids with `.` for `:`, so they can go without a namespace.
    let (namespace, stat) = split_id(stat, '.');
    if namespace != "minecraft" {
        // A stat of another namespace, such as a mod's.
        return (!is_id(&format!("{namespace}:{stat}")))
            .then(unknown)
            .flatten();
    }
    let names = match stat {
        "killed" | "killed_by" => &registries.entity_type,
        "mined" => &registries.block,
        "crafted" | "used" | "broken" | "picked_up" | "dropped" => &registries.item,
        "custom" => &registries.custom_stat,
        _ => return unknown(),
    };
    let (namespace, name) = split_id(value, '.');
    let known = if namespace == "minecraft" {
        names.contains(name)
    } else {
        is_id(&format!("{namespace}:{name}"))
    };
    (!known).then(unknown).flatten()
}

/// The `type=` values of a selector, such as `@e[type=!minecraft:zombie]`.
fn selector_problem(selector: &str) -> Option<String> {
    if !selector.starts_with('@') {
        return None;
    }
    let arguments = selector_arguments(selector).ok()?;
    arguments
        .iter()
        .filter(|(key, _)| key == "type")
        .find_map(|(_, value)| {
            entity_type_problem(
                value
                    .trim_start_matches('!')
                    .trim_start_matches(is_java_whitespace),
            )
        })
}

/// The registry a pack can add to behind an argument: its folder under a
/// namespace, what an entry is called, and Minecraft's own entries.
fn pack_registry(
    parser_type: &str,
    properties: Option<&Value>,
) -> Option<(&'static str, &'static str, &'static HashSet<String>)> {
    let registries = &*REGISTRIES;
    Some(match parser_type {
        "minecraft:loot_table" => ("loot_table", "loot table", &registries.loot_table),
        "minecraft:loot_modifier" => ("item_modifier", "item modifier", &registries.item_modifier),
        "minecraft:loot_predicate" => ("predicate", "predicate", &registries.predicate),
        "minecraft:dialog" => ("dialog", "dialog", &registries.dialog),
        "minecraft:feature" => ("worldgen/feature", "feature", &registries.feature),
        "minecraft:slot_source" => ("slot_source", "slot source", &registries.slot_source),
        "minecraft:context_int_provider" => (
            "context_int_provider",
            "number provider",
            &registries.context_int_provider,
        ),
        "minecraft:context_float_provider" => (
            "context_float_provider",
            "number provider",
            &registries.context_float_provider,
        ),
        "minecraft:resource" | "minecraft:resource_or_tag" => {
            match properties?.get("registry")?.as_str()? {
                "minecraft:enchantment" => ("enchantment", "enchantment", &registries.enchantment),
                "minecraft:damage_type" => ("damage_type", "damage type", &registries.damage_type),
                "minecraft:worldgen/biome" => ("worldgen/biome", "biome", &registries.biome),
                "minecraft:world_clock" => ("world_clock", "world clock", &registries.world_clock),
                "minecraft:timeline" => ("timeline", "timeline", &registries.timeline),
                _ => return None,
            }
        }
        _ => return None,
    })
}

/// An id's namespace and path. Without a namespace, or with an empty one, an
/// id is in `minecraft`.
fn id_parts(id: &str) -> (&str, &str) {
    split_id(id, ':')
}

fn split_id(id: &str, separator: char) -> (&str, &str) {
    match id.split_once(separator) {
        Some((namespace, path)) if !namespace.is_empty() => (namespace, path),
        Some((_, path)) => ("minecraft", path),
        None => ("minecraft", id),
    }
}

/// The resources of the data pack being validated. Minecraft looks up a
/// registry entry or a tag when it loads a function that names one, so a
/// missing one stops that function from loading. A pack can add to the game's
/// registries, so `minecraft:` ids count if either one defines them.
pub struct PackResources {
    data: Option<PathBuf>,
}

impl PackResources {
    pub fn new(pack: &Path) -> Self {
        Self {
            data: Some(pack.join("data")),
        }
    }

    /// For a command checked on its own, where only the game's ids count.
    pub fn none() -> Self {
        Self { data: None }
    }

    /// What is wrong with the reference `token` of the parser `parser_type`.
    /// Only `minecraft` and the namespaces the pack has are checked: other
    /// packs and mods can define the rest.
    pub fn problem(
        &self,
        parser_type: &str,
        properties: Option<&Value>,
        token: &str,
    ) -> Option<String> {
        let id = token.trim_start_matches(is_java_whitespace);
        if id.contains("$(") {
            return None;
        }
        if matches!(parser_type, "minecraft:component" | "minecraft:style") {
            let vanilla = &REGISTRIES.dialog;
            return text_component::dialog_ids(id)
                .iter()
                .find_map(|id| self.entry_problem("dialog", "dialog", vanilla, id));
        }
        if id.starts_with(['{', '[']) {
            return self.inline_reference_problem(parser_type, id);
        }
        if id.starts_with('+') {
            return None;
        }
        if let Some(tag) = id.strip_prefix('#') {
            return self.tag_problem(parser_type, properties, tag);
        }
        if matches!(
            parser_type,
            "minecraft:item_stack" | "minecraft:item_predicate"
        ) {
            return enchantment_ids(id).iter().find_map(|id| match id.strip_prefix('#') {
                Some(tag) => {
                    let (namespace, path) = id_parts(tag);
                    let vanilla = &REGISTRIES.enchantment_tag;
                    (!self.is_known(namespace, path, "tags/enchantment", vanilla)).then(|| {
                        format!("Unknown enchantment tag `#{tag}`: Minecraft can't load this function")
                    })
                }
                None => self.entry_problem("enchantment", "enchantment", &REGISTRIES.enchantment, id),
            });
        }
        let (folder, what, vanilla) = pack_registry(parser_type, properties)?;
        // Slot names have to touch the space before them. After more spaces,
        // Minecraft looks for a slot source of that name.
        if parser_type == "minecraft:slot_source" && id == token && is_item_slot_value(id, true) {
            return None;
        }
        self.entry_problem(folder, what, vanilla, id)
    }

    /// A tag or a resource that a predicate, an item modifier, a loot table,
    /// or a dialog written in place names, which Minecraft looks up when it
    /// loads the function.
    fn inline_reference_problem(&self, parser_type: &str, token: &str) -> Option<String> {
        if !matches!(
            parser_type,
            "minecraft:loot_predicate"
                | "minecraft:loot_modifier"
                | "minecraft:loot_table"
                | "minecraft:dialog"
        ) {
            return None;
        }
        let value = snbt::parse(token).ok()?;
        let mut problems = resource_format::Problems {
            nbt: true,
            ..Default::default()
        };
        match parser_type {
            "minecraft:loot_predicate" => {
                resource_format::check_predicate(&value, "", &mut problems)
            }
            "minecraft:loot_modifier" => match value.as_array() {
                Some(items) => items.iter().for_each(|item| {
                    resource_format::check_item_modifier(item, "", &mut problems);
                }),
                None => resource_format::check_item_modifier(&value, "", &mut problems),
            },
            "minecraft:loot_table" => resource_format::check_loot_table(&value, &mut problems),
            _ => resource_format::check_dialog(&value, &mut problems),
        }
        problems.references.iter().find_map(|reference| {
            let vanilla = vanilla_ids(reference.kind.registry())?;
            let (namespace, path) = id_parts(&reference.id);
            if self.is_known(namespace, path, reference.kind.folder(), vanilla) {
                return None;
            }
            let shown = if reference.kind.is_tag() {
                format!("#{namespace}:{path}")
            } else {
                format!("{namespace}:{path}")
            };
            let who = if namespace == "minecraft" {
                "neither Minecraft nor the pack defines it"
            } else {
                "the pack doesn't define it"
            };
            Some(format!(
                "Unknown {} `{shown}` at `{}`: {who}, so Minecraft can't load this function",
                reference.kind.what(),
                reference.path
            ))
        })
    }

    fn entry_problem(
        &self,
        folder: &str,
        what: &str,
        vanilla: &HashSet<String>,
        id: &str,
    ) -> Option<String> {
        let (namespace, path) = id_parts(id);
        (!self.is_known(namespace, path, folder, vanilla)).then(|| {
            if namespace == "minecraft" {
                format!("Unknown {what} `{id}`: neither Minecraft nor the pack defines it")
            } else {
                format!("Unknown {what} `{id}`: the pack doesn't define it")
            }
        })
    }

    /// Whether `namespace:path` is in `folder` of the pack, in `vanilla` for
    /// `minecraft`, or in a namespace the pack doesn't have.
    fn is_known(
        &self,
        namespace: &str,
        path: &str,
        folder: &str,
        vanilla: &HashSet<String>,
    ) -> bool {
        let root = self.data.as_ref().map(|data| data.join(namespace));
        let file = format!("{path}.json");
        root.as_ref()
            .is_some_and(|root| root.join(folder).join(file).is_file())
            || if namespace == "minecraft" {
                vanilla.contains(path)
            } else {
                !root.is_some_and(|root| root.is_dir())
            }
    }

    /// `#tag`, for the arguments where Minecraft looks the tag up on load.
    /// `minecraft:` tags are the game's own or the pack's.
    fn tag_problem(
        &self,
        parser_type: &str,
        properties: Option<&Value>,
        tag: &str,
    ) -> Option<String> {
        let registries = &*REGISTRIES;
        let (folder, what, vanilla) = match parser_type {
            "minecraft:block_predicate" => ("tags/block", "block", &registries.block_tag),
            "minecraft:item_predicate" => ("tags/item", "item", &registries.item_tag),
            "minecraft:resource_or_tag" => match properties?.get("registry")?.as_str()? {
                "minecraft:worldgen/biome" => {
                    ("tags/worldgen/biome", "biome", &registries.biome_tag)
                }
                "minecraft:point_of_interest_type" => (
                    "tags/point_of_interest_type",
                    "point of interest",
                    &registries.poi_tag,
                ),
                _ => return None,
            },
            _ => return None,
        };
        let tag = id_part(tag);
        let (namespace, path) = id_parts(tag);
        (!self.is_known(namespace, path, folder, vanilla))
            .then(|| format!("Unknown {what} tag `#{tag}`: Minecraft can't load this function"))
    }
}

/// What is wrong with the tag entry `id` of a `tag_type` tag: a tag that names
/// an unknown `minecraft:` id doesn't load, and functions that use it don't
/// either. Another tag and other namespaces can't be checked here.
pub fn tag_entry_problem(tag_type: &str, id: &str) -> Option<String> {
    let registries = &*REGISTRIES;
    let (names, what) = match tag_type {
        "block" => (&registries.block, "block"),
        "item" => (&registries.item, "item"),
        "entity_type" => (&registries.entity_type, "entity type"),
        _ => return None,
    };
    let name = vanilla_name(id)?;
    (!names.contains(name)).then(|| {
        format!(
            "Unknown {what} `{id}`: Minecraft doesn't load a tag with an unknown entry. \
                 Use `{{\"id\": \"{id}\", \"required\": False}}` to skip it when it's missing"
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn particles_need_their_options() {
        let parser = "minecraft:particle";
        for token in [
            "heart",
            "heart{}",
            "minecraft:dust{color:[1,0,0],scale:1}",
            "entity_effect{color:1}",
            "minecraft:block{block_state:\"stone\"}",
            "item{item:\"stone\"}",
            "mymod:spark",
        ] {
            assert_eq!(problem(parser, None, token), None, "{token}");
        }
        for (token, expected) in [
            ("dust", "`color`"),
            ("dust{color:[1,0,0]}", "`scale`"),
            ("entity_effect", "`color`"),
            ("block", "`block_state`"),
            ("trail{}", "`target`"),
            ("heart{a:1c:2}", "Invalid SNBT"),
        ] {
            let found = problem(parser, None, token).unwrap_or_default();
            assert!(found.contains(expected), "{token}: {found}");
        }
    }

    #[test]
    fn item_components_follow_the_game() {
        let parser = "minecraft:item_stack";
        for token in [
            "stone[]",
            "stone[ damage = 1 ]",
            "stone[damage=1,]",
            "stone[minecraft:damage=1]",
            "stone[!damage]",
            "stone[tooltip_display={hide_tooltip:true}]",
            "stone[custom_model_data={floats:[1.0f]}]",
            "stone[unbreakable={}]",
            "stone[enchantments={sharpness:1}]",
            "stone[custom_name={text:\"a\",color:\"red\"}]",
            "stone[dyed_color=255]",
            "stone[dyed_color=true]",
            "stone[mymod:thing=1]",
            // Minecraft takes any number or boolean for a whole number.
            "stone[damage=1.5]",
            "stone[damage=true]",
            "stone[damage=false]",
            "stone[damage=-0.5]",
            "stone[damage=1b]",
            "stone[repair_cost=0.5]",
            "stone[max_damage=1.5]",
            "stone[max_damage=true]",
            "stone[max_stack_size=99.5]",
            "stone[max_stack_size=true]",
            "stone[max_stack_size=4294967297L]",
        ] {
            assert_eq!(problem(parser, None, token), None, "{token}");
        }
        for (token, expected) in [
            ("stone[,damage=1]", "Unknown item component `,damage=1`"),
            ("stone[damage=1,damage=2]", "appears twice"),
            ("stone[!damage,damage=1]", "appears twice"),
            ("stone[hide_tooltip={}]", "Unknown item component"),
            ("stone[Damage=1]", "Unknown item component"),
            ("stone[damage]", "needs a value"),
            ("stone[damage=]", "Invalid SNBT"),
            ("stone[damage=\"x\"]", "must be a number"),
            ("stone[damage=-1]", "must be a number"),
            ("stone[damage=-1.5]", "must be a number"),
            ("stone[max_damage=0]", "must be a number"),
            ("stone[max_damage=0.5]", "must be a number"),
            ("stone[max_damage=false]", "must be a number"),
            ("stone[max_stack_size=0.5]", "from 1 to 99"),
            ("stone[max_stack_size=false]", "from 1 to 99"),
            ("stone[max_stack_size=100.5]", "from 1 to 99"),
            ("stone[custom_model_data=1]", "must be a compound"),
            ("stone[unbreakable=true]", "must be a compound"),
            ("stone[custom_data=1]", "must be a compound"),
            ("stone[lore=\"a\"]", "must be a list"),
            (
                "stone[attribute_modifiers={modifiers:[]}]",
                "must be a list",
            ),
            ("stone[dyed_color={rgb:255}]", "must be a color"),
            ("stone[max_stack_size=100]", "from 1 to 99"),
            ("stone[rarity=\"nope\"]", "must be common"),
            (
                "stone[enchantments={levels:{sharpness:1}}]",
                "without `levels`",
            ),
            ("stone[custom_name={text:\"a\",color:nope}]", "not a color"),
            ("stone[custom_data={a:1c:2}]", "Invalid SNBT"),
            ("#minecraft:logs", "names a tag"),
        ] {
            let found = problem(parser, None, token).unwrap_or_default();
            assert!(found.contains(expected), "{token}: {found}");
        }
    }

    #[test]
    fn item_tests_follow_the_game() {
        let parser = "minecraft:item_predicate";
        for token in [
            "*[]",
            "*[damage=1]",
            "stone[damage~{}]",
            "stone[!damage]",
            "stone[damage]",
            "stone[damage|count=1]",
            "*[count=1]",
            "*[minecraft:rarity=\"epic\"]",
            "stone[custom_data~{a:1}]",
            "#minecraft:logs[damage=1]",
        ] {
            assert_eq!(problem(parser, None, token), None, "{token}");
        }
        for (token, expected) in [
            ("stone[nope=1]", "Unknown item test"),
            ("stone[nope]", "Unknown item test"),
            ("*[rarity=\"nope\"]", "must be common"),
            ("stone[custom_data~{a:1c:2}]", "Invalid SNBT"),
        ] {
            let found = problem(parser, None, token).unwrap_or_default();
            assert!(found.contains(expected), "{token}: {found}");
        }
    }

    #[test]
    fn block_states_follow_the_game() {
        for token in [
            "stone[]",
            "oak_stairs[facing=north,]",
            "oak_stairs[ facing = north ]",
            "chest{Items:[]}",
            "chest{Items:[{a:\"x=y\"}]}",
            "chest[facing=north]{Items:[]}",
            // Names and values can be quoted, and numbers read as numbers.
            "oak_stairs[facing=\"north\"]",
            "oak_stairs['facing'=north]",
            "oak_stairs[\"facing\"=\"north\"]",
            "oak_stairs[waterlogged=\"true\"]",
            "snow[layers=01]",
            "snow[layers=\"1\"]",
            "snow[layers=+1]",
        ] {
            assert_eq!(
                problem("minecraft:block_state", None, token),
                None,
                "{token}"
            );
        }
        for token in [
            "#minecraft:logs[axis=y]",
            "#minecraft:logs[a=]",
            "#minecraft:logs[a=b,]",
            "#minecraft:logs[\"a\"=\"b\"]",
        ] {
            assert_eq!(
                problem("minecraft:block_predicate", None, token),
                None,
                "{token}"
            );
        }
        for (token, expected) in [
            ("oak_stairs[,facing=north]", "Block state `` needs a value"),
            ("oak_stairs[facing=north,facing=south]", "appears twice"),
            ("oak_stairs[facing]", "needs a value"),
            ("oak_stairs[facing=]", "needs a value"),
            ("oak_stairs[facing=\"nope\"]", "no `facing=nope`"),
            ("oak_stairs[\"nope\"=north]", "no property `nope`"),
            ("oak_stairs[waterlogged=TRUE]", "no `waterlogged=TRUE`"),
            ("snow[layers=9]", "no `layers=9`"),
            ("snow[layers=009]", "no `layers=009`"),
            ("chest{a:1c:2}", "Invalid SNBT"),
            ("#minecraft:logs", "names a tag"),
        ] {
            let found = problem("minecraft:block_state", None, token).unwrap_or_default();
            assert!(found.contains(expected), "{token}: {found}");
        }
    }

    /// `problem` for `token` under `parser`, which each list says is exact.
    fn assert_problems(parser: &str, tokens: &[(&str, &str)]) {
        for (token, expected) in tokens {
            assert_eq!(
                problem(parser, None, token).as_deref(),
                Some(*expected),
                "{token}"
            );
        }
    }

    fn assert_no_problems(parser: &str, tokens: &[&str]) {
        for token in tokens {
            assert_eq!(problem(parser, None, token), None, "{token}");
        }
    }

    #[test]
    fn item_stacks_read_their_list_like_the_game() {
        let parser = "minecraft:item_stack";
        assert_no_problems(
            parser,
            &[
                "stone[damage=1,]",
                "stone[damage=1, ]",
                "stone[!damage,]",
                "stone[!damage, ]",
                "stone[ !damage ]",
                "stone[ damage = 1 , ]",
                "stone[\tdamage=1]",
                "stone[damage\u{3000}=1]",
                "stone[!mymod:thing]",
                "stone[:damage=1]",
            ],
        );
        assert_problems(
            parser,
            &[
                ("stone[ ]", "An item component list ends in an empty entry"),
                ("stone[,]", "Unknown item component `,`"),
                ("stone[damage=1,,]", "Unknown item component `,`"),
                ("stone[! damage]", "Unknown item component ` damage`"),
                ("stone[!!damage]", "Unknown item component `!damage`"),
                (
                    "stone[minecraft::damage=1]",
                    "`minecraft::damage` is not the id of an item component",
                ),
                (
                    "stone[a:b:c=1]",
                    "`a:b:c` is not the id of an item component",
                ),
                (
                    "stone[a/b:c=1]",
                    "`a/b:c` is not the id of an item component",
                ),
                (
                    "stone[!a/b:c]",
                    "`a/b:c` is not the id of an item component",
                ),
                (
                    "stone[Minecraft:damage=1]",
                    "Unknown item component `Minecraft:damage=1`",
                ),
                (
                    "stone[damage=1 count=2]",
                    "Item component `damage` can't end in `c`",
                ),
                (
                    "stone[damage=1\u{a0}]",
                    "Item component `damage` can't end in `\u{a0}`",
                ),
                (
                    "stone[!:damage,damage=1]",
                    "Item component `damage` appears twice",
                ),
            ],
        );
    }

    #[test]
    fn item_tests_read_their_list_like_the_game() {
        let parser = "minecraft:item_predicate";
        assert_no_problems(
            parser,
            &[
                "stone[ ]",
                "stone[ damage ]",
                "stone[! damage]",
                "stone[!  damage]",
                "stone[!damage=1]",
                "stone[!damage~{}]",
                "stone[damage , count]",
                "stone[damage | count]",
                "stone[!damage|!count]",
                "stone[damage =1]",
                "stone[damage= 1]",
                "stone[damage ~{}]",
                "stone[count~1]",
                "stone[mymod:thing]",
                "stone[!mymod:thing=1]",
                "stone[mymod:thing~{}]",
                "stone[:damage]",
            ],
        );
        assert_problems(
            parser,
            &[
                ("stone[damage,]", "An item test list ends in an empty entry"),
                (
                    "stone[damage, ]",
                    "An item test list ends in an empty entry",
                ),
                ("stone[damage|]", "An item test list ends in an empty entry"),
                ("stone[,damage]", "Unknown item test `,damage`"),
                ("stone[|damage]", "Unknown item test `|damage`"),
                ("stone[damage||count]", "Unknown item test `|count`"),
                ("stone[!!damage]", "Unknown item test `!damage`"),
                ("stone[a:b:c]", "`a:b:c` is not the id of an item test"),
                ("stone[a/b:c=1]", "`a/b:c` is not the id of an item test"),
                (
                    "stone[minecraft::damage=1]",
                    "`minecraft::damage` is not the id of an item test",
                ),
                ("stone[damage x]", "An item test can't end in `x`"),
            ],
        );
    }

    #[test]
    fn component_values_have_their_shape() {
        let parser = "minecraft:item_stack";
        assert_no_problems(
            parser,
            &[
                "stone[custom_data=\"{a:1}\"]",
                "stone[custom_data=\"{a:1} \"]",
                "stone[custom_data=\" {a:1}\"]",
                "stone[custom_data=\"{}\"]",
                "stone[custom_data='{a:\"b\"}']",
                "stone[enchantments={sharpness:1,smite:255}]",
                "stone[enchantments={}]",
                // A level reads like any whole number.
                "stone[enchantments={sharpness:1.5}]",
                "stone[enchantments={sharpness:255.5}]",
                "stone[enchantments={sharpness:true}]",
                "stone[enchantments={sharpness:1b}]",
                "stone[stored_enchantments={sharpness:255}]",
                "stone[dyed_color=[1,2,3]]",
                "stone[dyed_color=[0.5,0.5f,0.5d]]",
                "stone[dyed_color=[I;1,2,3]]",
                "stone[dyed_color=[B;1b,2b,3b]]",
                "stone[dyed_color=[true,false,true]]",
                "stone[dyed_color=-1]",
                "stone[dyed_color=1.5]",
                "stone[item_model=\"stone\"]",
                "stone[item_model=stone]",
                "stone[item_model=\"a:b/c\"]",
                "stone[item_model=\"\"]",
                "stone[item_model=\":stone\"]",
                "stone[item_model=\"a:\"]",
                "stone[food={nutrition:1,saturation:1}]",
                "stone[food={nutrition:0,saturation:-1,can_always_eat:true}]",
                "stone[food={nutrition:1.5,saturation:1,x:1}]",
                "stone[food={nutrition:true,saturation:true}]",
            ],
        );
        let shape = |name: &str, what: &str| format!("Item component `{name}` must be {what}");
        let color = shape("dyed_color", "a color number or a list of three numbers");
        let model = shape("item_model", "an id in quotes");
        let food = shape("food", "a compound");
        let level = "The level of `sharpness` must be a number from 1 to 255";
        let compound = "`a` must be a compound (`{...}`)";
        assert_problems(
            parser,
            &[
                (
                    "stone[custom_data=\"a\"]",
                    "`a` must be a compound (`{...}`)",
                ),
                ("stone[custom_data=a]", compound),
                ("stone[custom_data=\"\"]", "Invalid SNBT: expected a value"),
                (
                    "stone[custom_data=\"[1]\"]",
                    "`[1]` must be a compound (`{...}`)",
                ),
                (
                    "stone[custom_data=\"{a:1c:2}\"]",
                    "Invalid SNBT: `1c` is not a number; quote it if it is text",
                ),
                ("stone[enchantments={sharpness:0}]", level),
                ("stone[enchantments={sharpness:256}]", level),
                ("stone[enchantments={sharpness:-1}]", level),
                ("stone[enchantments={sharpness:0.5}]", level),
                ("stone[enchantments={sharpness:false}]", level),
                ("stone[enchantments={sharpness:\"a\"}]", level),
                ("stone[enchantments={sharpness:{}}]", level),
                ("stone[enchantments={sharpness:[]}]", level),
                ("stone[stored_enchantments={sharpness:0}]", level),
                (
                    "stone[enchantments={\"mymod:x\":256}]",
                    "The level of `mymod:x` must be a number from 1 to 255",
                ),
                ("stone[dyed_color=[]]", &color),
                ("stone[dyed_color=[1,2]]", &color),
                ("stone[dyed_color=[1,2,3,4]]", &color),
                ("stone[dyed_color=[I;1,2]]", &color),
                ("stone[dyed_color=[\"a\",1,2]]", &color),
                ("stone[dyed_color=[[1],2,3]]", &color),
                ("stone[item_model=\"a b\"]", &model),
                ("stone[item_model=\"A\"]", &model),
                ("stone[item_model=A]", &model),
                ("stone[item_model=\"a:b:c\"]", &model),
                ("stone[item_model=\"a/b:c\"]", &model),
                ("stone[item_model=1]", &model),
                ("stone[item_model=true]", &model),
                ("stone[item_model={}]", &model),
                ("stone[food=1]", &food),
                ("stone[food=[]]", &food),
                ("stone[food=\"a\"]", &food),
                (
                    "stone[food={}]",
                    "Item component `food` needs a number `nutrition`",
                ),
                (
                    "stone[food={nutrition:1}]",
                    "Item component `food` needs a number `saturation`",
                ),
                (
                    "stone[food={saturation:1}]",
                    "Item component `food` needs a number `nutrition`",
                ),
                (
                    "stone[food={nutrition:\"a\",saturation:1}]",
                    "Item component `food` needs a number `nutrition`",
                ),
                (
                    "stone[food={nutrition:1,saturation:\"a\"}]",
                    "Item component `food` needs a number `saturation`",
                ),
                (
                    "stone[food={nutrition:-1,saturation:1}]",
                    "`nutrition` of `food` can't be negative",
                ),
            ],
        );

        // Item tests with `=` read values the same way, and so does
        // `custom_data~`. Other `~` tests have their own shapes.
        let tests = "minecraft:item_predicate";
        assert_no_problems(
            tests,
            &[
                "stone[custom_data=\"{a:1}\"]",
                "stone[custom_data~\"{a:1}\"]",
                "stone[custom_data~{a:1}]",
                "stone[enchantments~[{enchantments:\"sharpness\",levels:0}]]",
                "stone[food~{}]",
            ],
        );
        assert_problems(
            tests,
            &[
                ("stone[custom_data=\"a\"]", compound),
                ("stone[custom_data~\"a\"]", compound),
                (
                    "stone[custom_data~1]",
                    "Item component `custom_data` must be a compound or a string",
                ),
                ("stone[enchantments={sharpness:0}]", level),
                (
                    "stone[food={}]",
                    "Item component `food` needs a number `nutrition`",
                ),
            ],
        );
    }

    #[test]
    fn block_states_read_like_the_game() {
        let tag = "minecraft:block_predicate";
        assert_no_problems(
            tag,
            &[
                "#minecraft:logs[]",
                "#minecraft:logs[ ]",
                "#minecraft:logs[a=b]",
                "#minecraft:logs[a=]",
                "#minecraft:logs[=c]",
                "#minecraft:logs[=]",
                "#minecraft:logs[ a = b ]",
                "#minecraft:logs[a=b,]",
                "#minecraft:logs[\"a:b\"=\"c:d\"]",
                "#minecraft:logs[a=\"\"]",
                "#minecraft:logs[\"\"=b]",
                "#minecraft:logs[a=1.5]",
                "#minecraft:logs[a=-1]",
                "#minecraft:logs[a=+1]",
                "#minecraft:logs[a=b]{}",
                "mymod:block[a=b,c=\"d\"]{x:1}",
                "mymod:block[a=,b=c]",
            ],
        );
        assert_problems(
            tag,
            &[
                ("#minecraft:logs[a=b:c]", "A block state can't end in `:`"),
                ("#minecraft:logs[a=b/c]", "A block state can't end in `/`"),
                ("#minecraft:logs[a=b c]", "A block state can't end in `c`"),
                (
                    "#minecraft:logs[a=\u{e9}]",
                    "A block state can't end in `\u{e9}`",
                ),
                ("#minecraft:logs[a=#b]", "A block state can't end in `#`"),
                ("#minecraft:logs[a:b=c]", "Block state `a` needs a value"),
                ("#minecraft:logs[a b=c]", "Block state `a` needs a value"),
                ("#minecraft:logs[\"a\"]", "Block state `a` needs a value"),
                ("#minecraft:logs[,]", "Block state `` needs a value"),
                ("#minecraft:logs[a=b, ]", "Block state `` needs a value"),
                ("#minecraft:logs[a=b,,]", "Block state `` needs a value"),
                ("#minecraft:logs[a=b,a=c]", "Block state `a` appears twice"),
                (
                    "#minecraft:logs[\"a\\nb\"=c]",
                    "A block state has a broken quoted string",
                ),
                (
                    "#minecraft:logs[a=\"\\n\"]",
                    "A block state has a broken quoted string",
                ),
                ("mymod:block[a=b:c]", "A block state can't end in `:`"),
            ],
        );
        let block = "minecraft:block_state";
        assert_no_problems(
            block,
            &[
                "oak_stairs[ ]",
                "oak_stairs[facing=north, half=top]",
                "oak_stairs[facing=north ,half=top]",
                "stone[]{}",
            ],
        );
        assert_problems(
            block,
            &[
                ("oak_stairs[facing=north, ]", "Block state `` needs a value"),
                (
                    "oak_stairs[facing=north half=top]",
                    "A block state can't end in `h`",
                ),
                (
                    "oak_stairs[facing:north]",
                    "Block state `facing` needs a value",
                ),
                (
                    "oak_stairs[facing=\"nor\\nth\"]",
                    "A block state has a broken quoted string",
                ),
            ],
        );
    }

    #[test]
    fn a_leading_colon_means_minecraft() {
        for (parser, known, unknown_id) in [
            ("minecraft:item_stack", ":stone", ":nope"),
            ("minecraft:item_predicate", ":stone", ":nope"),
            ("minecraft:block_state", ":stone", ":nope"),
            ("minecraft:block_predicate", ":stone", ":nope"),
            ("minecraft:particle", ":flame", ":nope"),
            ("minecraft:entity", "@e[type=:pig]", "@e[type=:nope]"),
        ] {
            assert_eq!(problem(parser, None, known), None, "{parser}");
            assert!(problem(parser, None, unknown_id).is_some(), "{parser}");
        }
        for (registry, known) in [
            ("minecraft:entity_type", ":pig"),
            ("minecraft:mob_effect", ":speed"),
        ] {
            let properties = serde_json::json!({ "registry": registry });
            let check = |token| problem("minecraft:resource", Some(&properties), token);
            assert_eq!(check(known), None, "{registry}");
            assert!(check(":nope").is_some(), "{registry}");
        }
        assert_eq!(
            problem("minecraft:particle", None, ":dust").as_deref(),
            Some("Particle `dust` needs `color`: `dust{color:...}`")
        );
    }

    #[test]
    fn selectors_of_score_holders_and_messages_are_looked_up() {
        for (parser, token) in [
            ("minecraft:score_holder", "@s[type=nope]"),
            ("minecraft:game_profile", "@e[type=nope]"),
            ("minecraft:message", "hi @p[type=nope] there"),
        ] {
            assert_eq!(
                problem(parser, None, token).as_deref(),
                Some("Unknown entity type `nope`"),
                "{token}"
            );
        }
        assert_no_problems(
            "minecraft:message",
            &[
                "@p[type=pig]",
                "@p[type=mymod:thing]",
                "@x[type=nope]",
                "@e [type=nope]",
            ],
        );
    }

    #[test]
    fn criteria_are_ids_with_dots() {
        let parser = "minecraft:objective_criteria";
        assert_no_problems(
            parser,
            &[
                "minecraft.killed:zombie",
                "minecraft.killed:minecraft.zombie",
                "minecraft.killed:.zombie",
                "killed:zombie",
                "killed:minecraft.zombie",
                ".killed:zombie",
                ".killed:.zombie",
                "mined:stone",
                "minecraft.mined:stone",
                "minecraft.used:stone",
                "minecraft.custom:jump",
                "custom:jump",
                "minecraft.killed_by:zombie",
                "minecraft.broken:diamond_pickaxe",
                "minecraft.picked_up:stone",
                "minecraft.dropped:stone",
                "minecraft.crafted:stone",
                // Mods can add these.
                "foo.bar:zombie",
                "minecraft.killed:foo.bar",
                "a.b.c:d",
            ],
        );
        for criterion in [
            "minecraft.killed:zombi",
            "minecraft.custom:nope",
            "minecraft.used:nope",
            "minecraft.killed:",
            "killed:",
            "dummy:x",
            "nope:zombie",
            ":",
            ":zombie",
            "foo:",
            "minecraft.nope:zombie",
            "minecraft.Killed:zombie",
            "minecraft.killed:Zombie",
            "minecraft.killed.x:zombie",
            "minecraft.killed:minecraft.zombie.x",
            "minecraft.killed:minecraft:zombie",
            "Foo.bar:x",
            "foo.Bar:x",
            "minecraft.killed:foo.Bar",
        ] {
            assert_eq!(
                problem(parser, None, criterion),
                Some(format!("Unknown scoreboard criterion `{criterion}`")),
                "{criterion}"
            );
        }
    }

    #[test]
    fn inline_resources_are_checked() {
        for (parser, token) in [
            (
                "minecraft:loot_predicate",
                "{type:\"minecraft:random_chance\",chance:0.5}",
            ),
            ("minecraft:loot_modifier", "{type:\"set_count\",count:1}"),
            ("minecraft:loot_modifier", "[{type:\"set_count\",count:1}]"),
            ("minecraft:loot_table", "{pools:[]}"),
            (
                "minecraft:dialog",
                "{type:\"minecraft:notice\",title:\"x\"}",
            ),
            ("minecraft:dialog", "{type:\"notice\",title:\"x\"}"),
            ("minecraft:dialog", "minecraft:custom_options"),
        ] {
            assert_eq!(problem(parser, None, token), None, "{token}");
        }
        for (parser, token, expected) in [
            ("minecraft:loot_predicate", "{}", "`type` is missing"),
            ("minecraft:loot_predicate", "[]", "one compound"),
            (
                "minecraft:loot_predicate",
                "{condition:\"minecraft:random_chance\",chance:0.5}",
                "is now `type`",
            ),
            (
                "minecraft:loot_predicate",
                "{type:\"nope\"}",
                "not a predicate",
            ),
            (
                "minecraft:loot_modifier",
                "[{function:\"set_count\",count:1}]",
                "is now `type` (\"type\": \"set_count\")",
            ),
            ("minecraft:dialog", "{}", "`type` is missing"),
            ("minecraft:dialog", "{title:\"x\"}", "`type` is missing"),
            (
                "minecraft:dialog",
                "{type:\"notice\"}",
                "`title` is missing",
            ),
            (
                "minecraft:dialog",
                "{type:\"nope\",title:\"x\"}",
                "not a dialog",
            ),
            (
                "minecraft:dialog",
                "{type:\"confirmation\",title:\"x\"}",
                "`yes`",
            ),
            (
                "minecraft:dialog",
                "{type:\"multi_action\",title:\"x\"}",
                "`actions`",
            ),
            ("minecraft:feature", "{a:1c:2}", "Invalid SNBT"),
        ] {
            let found = problem(parser, None, token).unwrap_or_default();
            assert!(found.contains(expected), "{token}: {found}");
        }
    }

    #[test]
    fn tag_entries_must_exist() {
        assert_eq!(tag_entry_problem("block", "minecraft:stone"), None);
        assert_eq!(tag_entry_problem("block", "stone"), None);
        assert_eq!(tag_entry_problem("block", "#minecraft:logs"), None);
        assert_eq!(tag_entry_problem("block", "mymod:thing"), None);
        assert_eq!(tag_entry_problem("function", "minecraft:nope"), None);
        for (kind, id) in [
            ("block", "minecraft:grass"),
            ("item", "minecraft:chain"),
            ("entity_type", "minecraft:potion"),
        ] {
            let found = tag_entry_problem(kind, id).unwrap_or_default();
            assert!(found.contains("Unknown"), "{id}: {found}");
        }
    }

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
            ("minecraft:entity", None, "@e[type=mymod:thing]"),
            ("minecraft:entity", None, "@s"),
        ] {
            assert_eq!(problem(parser, properties, token), None, "{token}");
        }
        let entity = resource("minecraft:entity_type");
        assert_eq!(problem("minecraft:resource", Some(&entity), "zombie"), None);
        // A pack can add to these, so `PackResources` looks them up.
        for registry in [
            "minecraft:enchantment",
            "minecraft:damage_type",
            "minecraft:worldgen/biome",
            "minecraft:world_clock",
            "minecraft:timeline",
        ] {
            let properties = resource(registry);
            let parser = "minecraft:resource";
            assert_eq!(
                problem(parser, Some(&properties), "nope"),
                None,
                "{registry}"
            );
        }
    }

    #[test]
    fn shown_items_and_entities_are_looked_up() {
        assert_eq!(shown_item_problem("stone"), None);
        assert_eq!(shown_item_problem("mymod:thing"), None);
        for id in ["air", "minecraft:air"] {
            let found = shown_item_problem(id).unwrap_or_default();
            assert!(found.contains("can't show"), "{id}: {found}");
        }
        for id in ["nope", "minecraft:nope"] {
            let found = shown_item_problem(id).unwrap_or_default();
            assert!(found.contains("Unknown item"), "{id}: {found}");
        }
        assert_eq!(entity_type_problem("pig"), None);
        assert_eq!(entity_type_problem("mymod:thing"), None);
        let found = entity_type_problem("nope").unwrap_or_default();
        assert_eq!(found, "Unknown entity type `nope`");
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
            ("minecraft:entity", None, "@e[\"type\"=zombi]", "`zombi`"),
            ("minecraft:entity", None, "@e[type=! zombi]", "`zombi`"),
            ("minecraft:entity", None, "@e[type=]", "Unknown entity type"),
            (
                "minecraft:entity",
                None,
                "@e[type=!]",
                "Unknown entity type",
            ),
        ] {
            let found = problem(parser, properties, token).unwrap_or_default();
            assert!(found.contains(expected), "{token}: {found}");
        }
        for (registry, what) in [
            ("minecraft:entity_type", "entity type"),
            ("minecraft:mob_effect", "effect"),
            ("minecraft:attribute", "attribute"),
            ("minecraft:point_of_interest_type", "point of interest type"),
        ] {
            let properties = resource(registry);
            let found = problem("minecraft:resource", Some(&properties), "minecraft:bogus");
            assert_eq!(found, Some(format!("Unknown {what} `minecraft:bogus`")));
        }
    }

    #[test]
    fn number_providers_of_the_pack_must_exist() {
        let pack = tempfile::TempDir::new().unwrap();
        for file in [
            "data/mypack/context_int_provider/dice.json",
            "data/mypack/context_float_provider/odds/even.json",
        ] {
            let path = pack.path().join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "{}").unwrap();
        }
        let resources = PackResources::new(pack.path());
        let (int, float) = (
            "minecraft:context_int_provider",
            "minecraft:context_float_provider",
        );
        for (parser, token) in [
            (int, "mypack:dice"),
            (float, "mypack:odds/even"),
            (int, "other:dice"),
            (int, "mypack:$(name)"),
            (int, "{type:\"minecraft:constant\",value:5}"),
        ] {
            assert_eq!(resources.problem(parser, None, token), None, "{token}");
        }
        for (parser, token) in [
            (int, "mypack:gone"),
            (float, "mypack:gone"),
            // Each provider type has its own folder.
            (float, "mypack:dice"),
            (int, "mypack:odds/even"),
        ] {
            assert_eq!(
                resources.problem(parser, None, token),
                Some(format!(
                    "Unknown number provider `{token}`: the pack doesn't define it"
                )),
                "{parser}"
            );
        }
        // `+1` is the number itself, and Minecraft has no provider `dice`.
        assert_eq!(resources.problem(int, None, "+1"), None);
        assert_eq!(
            resources.problem(int, None, "minecraft:dice"),
            Some(
                "Unknown number provider `minecraft:dice`: \
                 neither Minecraft nor the pack defines it"
                    .to_string()
            )
        );
    }

    /// A pack with one entry of `file`, as `mypack:here` and `minecraft:here`.
    fn pack_with(folders: &[&str]) -> tempfile::TempDir {
        let pack = tempfile::TempDir::new().unwrap();
        for namespace in ["mypack", "minecraft"] {
            for folder in folders {
                let path = pack.path().join("data").join(namespace).join(folder);
                std::fs::create_dir_all(&path).unwrap();
                std::fs::write(path.join("here.json"), "{}").unwrap();
            }
        }
        pack
    }

    #[test]
    fn registries_look_up_ids_in_the_pack_and_the_game() {
        let resource = |registry: &str| serde_json::json!({ "registry": registry });
        let enchantment = resource("minecraft:enchantment");
        let damage = resource("minecraft:damage_type");
        let biome = resource("minecraft:worldgen/biome");
        let clock = resource("minecraft:world_clock");
        let timeline = resource("minecraft:timeline");
        let kinds: [(&str, Option<&Value>, &str, &str, &str); 13] = [
            (
                "minecraft:loot_table",
                None,
                "loot_table",
                "loot table",
                "chests/simple_dungeon",
            ),
            (
                "minecraft:loot_predicate",
                None,
                "predicate",
                "predicate",
                "block/fast_cooking",
            ),
            (
                "minecraft:loot_modifier",
                None,
                "item_modifier",
                "item modifier",
                "",
            ),
            ("minecraft:dialog", None, "dialog", "dialog", "server_links"),
            (
                "minecraft:feature",
                None,
                "worldgen/feature",
                "feature",
                "oak",
            ),
            (
                "minecraft:context_int_provider",
                None,
                "context_int_provider",
                "number provider",
                "compostable/low",
            ),
            (
                "minecraft:context_float_provider",
                None,
                "context_float_provider",
                "number provider",
                "",
            ),
            (
                "minecraft:slot_source",
                None,
                "slot_source",
                "slot source",
                "",
            ),
            (
                "minecraft:resource",
                Some(&enchantment),
                "enchantment",
                "enchantment",
                "sharpness",
            ),
            (
                "minecraft:resource",
                Some(&damage),
                "damage_type",
                "damage type",
                "fall",
            ),
            (
                "minecraft:resource_or_tag",
                Some(&biome),
                "worldgen/biome",
                "biome",
                "plains",
            ),
            (
                "minecraft:resource",
                Some(&clock),
                "world_clock",
                "world clock",
                "overworld",
            ),
            (
                "minecraft:resource",
                Some(&timeline),
                "timeline",
                "timeline",
                "day",
            ),
        ];
        for (parser, properties, folder, what, vanilla) in kinds {
            let pack = pack_with(&[folder]);
            let resources = PackResources::new(pack.path());
            let problem = |token: &str| resources.problem(parser, properties, token);

            for token in [
                "here",
                ":here",
                "minecraft:here",
                "mypack:here",
                // Other packs and mods can define these.
                "other:gone",
                "other:here",
                // Known when the function runs.
                "mypack:$(name)",
                "$(name)",
                "{a:1}",
            ] {
                assert_eq!(problem(token), None, "{what} {token}");
            }
            if !vanilla.is_empty() {
                let bare = problem(vanilla);
                assert_eq!(bare, None, "{what} {vanilla}");
                let full = problem(&format!("minecraft:{vanilla}"));
                assert_eq!(full, None, "{what} {vanilla}");
            }
            for token in ["gone", ":gone", "minecraft:gone"] {
                assert_eq!(
                    problem(token),
                    Some(format!(
                        "Unknown {what} `{token}`: neither Minecraft nor the pack defines it"
                    )),
                    "{what}"
                );
            }
            assert_eq!(
                problem("mypack:gone"),
                Some(format!(
                    "Unknown {what} `mypack:gone`: the pack doesn't define it"
                )),
                "{what}"
            );
        }
    }

    #[test]
    fn a_command_on_its_own_counts_the_games_ids() {
        let resources = PackResources::none();
        let enchantment = serde_json::json!({ "registry": "minecraft:enchantment" });
        let loot = "minecraft:loot_table";
        for (parser, properties, token) in [
            (loot, None, "chests/simple_dungeon"),
            (loot, None, "other:thing"),
            ("minecraft:resource", Some(&enchantment), "sharpness"),
            ("minecraft:block_predicate", None, "#minecraft:logs"),
        ] {
            assert_eq!(
                resources.problem(parser, properties, token),
                None,
                "{token}"
            );
        }
        for (parser, properties, token) in [
            (loot, None, "chests/gone"),
            (
                "minecraft:resource",
                Some(&enchantment),
                "minecraft:sharpnes",
            ),
            ("minecraft:block_predicate", None, "#minecraft:gone"),
        ] {
            assert!(
                resources.problem(parser, properties, token).is_some(),
                "{token}"
            );
        }
    }

    #[test]
    fn dialogs_of_click_events_are_looked_up() {
        let pack = pack_with(&["dialog"]);
        let resources = PackResources::new(pack.path());
        let shown = |id: &str| {
            format!(r#"{{text:"a",click_event:{{action:"show_dialog",dialog:"{id}"}}}}"#)
        };
        for parser in ["minecraft:component", "minecraft:style"] {
            for id in [
                "server_links",
                "minecraft:server_links",
                "here",
                "mypack:here",
                "other:gone",
                "a:$(b)",
            ] {
                assert_eq!(resources.problem(parser, None, &shown(id)), None, "{id}");
            }
            for id in ["gone", "minecraft:gone", "mypack:gone"] {
                let found = resources
                    .problem(parser, None, &shown(id))
                    .unwrap_or_default();
                assert!(found.contains(&format!("Unknown dialog `{id}`")), "{found}");
            }
        }
    }

    #[test]
    fn slot_names_are_not_slot_sources() {
        let pack = pack_with(&["slot_source"]);
        let resources = PackResources::new(pack.path());
        let parser = "minecraft:slot_source";
        for token in ["weapon", "weapon.*", "container.53", "here", " here"] {
            assert_eq!(resources.problem(parser, None, token), None, "{token}");
        }
        for token in ["armor.tail", " weapon"] {
            let found = resources.problem(parser, None, token).unwrap_or_default();
            assert!(found.contains("Unknown slot source"), "{token}: {found}");
        }
    }

    #[test]
    fn enchantments_of_an_item_are_looked_up() {
        let pack = pack_with(&["enchantment"]);
        let resources = PackResources::new(pack.path());
        for (parser, token) in [
            ("minecraft:item_stack", "stone[enchantments={sharpness:1}]"),
            ("minecraft:item_stack", "stone[enchantments={here:1}]"),
            (
                "minecraft:item_stack",
                "stone[stored_enchantments={here:1}]",
            ),
            (
                "minecraft:item_stack",
                "stone[enchantments={\"mypack:here\":1,\"other:gone\":1}]",
            ),
            ("minecraft:item_stack", "stone[damage=1]"),
            (
                "minecraft:item_predicate",
                "stone[minecraft:enchantments={here:1}]",
            ),
            ("minecraft:item_predicate", "*"),
        ] {
            assert_eq!(resources.problem(parser, None, token), None, "{token}");
        }
        for (parser, token, id) in [
            (
                "minecraft:item_stack",
                "stone[enchantments={gone:1}]",
                "gone",
            ),
            (
                "minecraft:item_stack",
                "stone[stored_enchantments={\"mypack:gone\":1}]",
                "mypack:gone",
            ),
            (
                "minecraft:item_predicate",
                "stone[enchantments={gone:1}]",
                "gone",
            ),
        ] {
            let found = resources.problem(parser, None, token).unwrap_or_default();
            assert!(
                found.contains(&format!("Unknown enchantment `{id}`")),
                "{token}: {found}"
            );
        }
    }

    #[test]
    fn tags_are_looked_up_in_the_pack_and_the_game() {
        let pack = tempfile::TempDir::new().unwrap();
        for file in [
            "data/mypack/tags/block/ores.json",
            "data/mypack/tags/block/sub/deep.json",
            "data/mypack/tags/item/gems.json",
            "data/mypack/tags/worldgen/biome/hot.json",
            "data/minecraft/tags/block/custom.json",
            "data/minecraft/tags/item/custom.json",
            "data/minecraft/tags/worldgen/biome/custom.json",
            "data/mypack/tags/point_of_interest_type/jobs.json",
            "data/minecraft/tags/point_of_interest_type/custom.json",
        ] {
            let path = pack.path().join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "{\"values\":[]}").unwrap();
        }
        let resources = PackResources::new(pack.path());
        let biome = serde_json::json!({ "registry": "minecraft:worldgen/biome" });
        let poi = serde_json::json!({ "registry": "minecraft:point_of_interest_type" });
        let timeline = serde_json::json!({ "registry": "minecraft:timeline" });
        let blocks = ("minecraft:block_predicate", None);
        let items = ("minecraft:item_predicate", None);
        let biomes = ("minecraft:resource_or_tag", Some(&biome));
        let pois = ("minecraft:resource_or_tag", Some(&poi));

        for ((parser, properties), token) in [
            // In the pack, with or without what can follow a tag.
            (blocks, "#mypack:ores"),
            (blocks, "#mypack:sub/deep"),
            (blocks, "#mypack:ores[axis=y]"),
            (blocks, "#mypack:ores{a:1}"),
            (items, "#mypack:gems"),
            (items, "#mypack:gems[damage=1]"),
            (biomes, "#mypack:hot"),
            // Minecraft's own, or a `minecraft:` tag the pack adds.
            (blocks, "#minecraft:logs"),
            (blocks, "#logs"),
            (blocks, "#minecraft:mineable/pickaxe"),
            (blocks, "#minecraft:custom"),
            (items, "#minecraft:logs"),
            (items, "#minecraft:custom"),
            (biomes, "#minecraft:is_forest"),
            (biomes, "#minecraft:custom"),
            (pois, "#mypack:jobs"),
            (pois, "#minecraft:acquirable_job_site"),
            (pois, "#acquirable_job_site"),
            (pois, "#minecraft:custom"),
            (blocks, "#:logs"),
            // Another pack or a mod can define these.
            (blocks, "#other:thing"),
            (items, "#other:thing"),
            (biomes, "#other:thing"),
            (pois, "#other:thing"),
            // Known only when the function runs.
            (blocks, "#mypack:$(tag)"),
            (items, "#$(tag)"),
            (biomes, "#minecraft:$(tag)"),
        ] {
            assert_eq!(
                resources.problem(parser, properties, token),
                None,
                "{parser} {token}"
            );
        }
        for ((parser, properties), token, expected) in [
            (blocks, "#mypack:gone", "Unknown block tag `#mypack:gone`"),
            (
                blocks,
                "#mypack:gone[axis=y]",
                "Unknown block tag `#mypack:gone`",
            ),
            (
                blocks,
                "#mypack:gone{a:1}",
                "Unknown block tag `#mypack:gone`",
            ),
            (
                blocks,
                "#minecraft:gone",
                "Unknown block tag `#minecraft:gone`",
            ),
            (blocks, "#gone", "Unknown block tag `#gone`"),
            // A tag of another kind doesn't count.
            (blocks, "#mypack:gems", "Unknown block tag `#mypack:gems`"),
            (items, "#mypack:gone", "Unknown item tag `#mypack:gone`"),
            (
                items,
                "#mypack:gone[damage=1]",
                "Unknown item tag `#mypack:gone`",
            ),
            (
                items,
                "#minecraft:gone",
                "Unknown item tag `#minecraft:gone`",
            ),
            (biomes, "#mypack:gone", "Unknown biome tag `#mypack:gone`"),
            (
                pois,
                "#mypack:gone",
                "Unknown point of interest tag `#mypack:gone`",
            ),
            (
                pois,
                "#minecraft:gone",
                "Unknown point of interest tag `#minecraft:gone`",
            ),
            (pois, "#gone", "Unknown point of interest tag `#gone`"),
            (
                biomes,
                "#minecraft:gone",
                "Unknown biome tag `#minecraft:gone`",
            ),
        ] {
            assert_eq!(
                resources.problem(parser, properties, token),
                Some(format!("{expected}: Minecraft can't load this function")),
                "{parser} {token}"
            );
        }
        // Other registries and arguments that take no block or item tag.
        for (parser, properties) in [
            ("minecraft:resource_or_tag", Some(&timeline)),
            ("minecraft:resource_or_tag", None),
            ("minecraft:function", None),
            ("minecraft:entity", None),
        ] {
            assert_eq!(
                resources.problem(parser, properties, "#mypack:gone"),
                None,
                "{parser}"
            );
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
        assert_eq!(resources.problem(loot, None, "mypack:chests/reward"), None);
        assert_eq!(
            resources.problem(loot, None, "minecraft:chests/simple_dungeon"),
            None
        );
        assert_eq!(resources.problem(loot, None, "other:thing"), None);
        assert_eq!(resources.problem(loot, None, "mypack:$(table)"), None);
        assert_eq!(resources.problem(loot, None, "{pools:[]}"), None);
        assert_eq!(
            resources.problem("minecraft:slot_source", None, "weapon.*"),
            None
        );
        let missing = resources
            .problem(loot, None, "mypack:chests/gone")
            .unwrap_or_default();
        assert!(missing.contains("Unknown loot table"), "{missing}");
        let missing = resources
            .problem("minecraft:slot_source", None, "mypack:hands")
            .unwrap_or_default();
        assert!(missing.contains("Unknown slot source"), "{missing}");
    }

    #[test]
    fn particle_block_states_read_like_minecraft() {
        // Each of these was checked on a 26.3 server.
        let check = |token: &str| problem("minecraft:particle", None, token);
        for token in [
            "block{block_state:\"stone\"}",
            "block{block_state:\"minecraft:stone\"}",
            "block{block_state:{id:\"stone\"}}",
            "block{block_state:{id:\"minecraft:oak_log\",properties:{axis:\"x\"}}}",
            "block{block_state:{id:\"stone\",Name:\"stone\"}}",
            "block{block_state:{id:\"oak_log\",Properties:{axis:\"x\"}}}",
            "falling_dust{block_state:{id:\"sand\"}}",
            "block{block_state:\"mymod:thing\"}",
        ] {
            assert_eq!(check(token), None, "{token}");
        }
        for (token, expected) in [
            (
                "block{block_state:{Name:\"stone\"}}",
                "with `id`, not `Name`",
            ),
            (
                "block{block_state:{Name:\"stone\",Properties:{}}}",
                "with `id`, not `Name`",
            ),
            ("falling_dust{block_state:{Name:\"sand\"}}", "with `id`"),
            ("block_marker{block_state:{Name:\"barrier\"}}", "with `id`"),
            ("dust_pillar{block_state:{Name:\"stone\"}}", "with `id`"),
            ("block_crumble{block_state:{Name:\"stone\"}}", "with `id`"),
            ("block{block_state:{}}", "is a block id or"),
            ("block{block_state:1}", "is a block id or"),
            ("block{block_state:[]}", "is a block id or"),
            ("block{block_state:{id:1}}", "is a block id or"),
            ("block{block_state:\"stone[axis=x]\"}", "is a block id or"),
            ("block{block_state:{id:\"nope\"}}", "Unknown block `nope`"),
            ("block{block_state:\"minecraft:nope\"}", "Unknown block"),
        ] {
            let found = check(token).unwrap_or_default();
            assert!(found.contains(expected), "{token}: {found}");
        }
    }

    #[test]
    fn enchantment_tests_name_known_enchantments_and_tags() {
        let pack = tempfile::TempDir::new().unwrap();
        for file in [
            "data/mypack/tags/enchantment/sharp.json",
            "data/minecraft/tags/enchantment/custom.json",
        ] {
            let path = pack.path().join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "{\"values\":[]}").unwrap();
        }
        let resources = PackResources::new(pack.path());
        let check = |token: &str| {
            problem("minecraft:item_predicate", None, token)
                .or_else(|| resources.problem("minecraft:item_predicate", None, token))
        };
        // A 26.3 server loads the first ones, and refuses the rest.
        for token in [
            "*[enchantments~[{enchantments:\"#minecraft:exclusive_set/mining\",levels:{min:1}}]]",
            "*[enchantments~[{enchantments:\"sharpness\"}]]",
            "*[enchantments~[{levels:1}]]",
            "stone[enchantments~[{enchantments:\"#curse\"}]]",
            "*[enchantments~[{enchantments:[\"sharpness\",\"minecraft:smite\"]}]]",
            "*[enchantments~[{enchantments:\"#mypack:sharp\"}]]",
            "*[enchantments~[{enchantments:\"#minecraft:custom\"}]]",
            "*[enchantments~[{enchantments:\"#other:thing\"}]]",
            "*[enchantments~[{enchantments:\"#$(tag)\"}]]",
            "*[enchantments~[]]",
        ] {
            assert_eq!(check(token), None, "{token}");
        }
        for (token, expected) in [
            (
                "*[enchantments~[{enchantments:\"#minecraft:mining_exclusive\",levels:{min:1}}]]",
                "Unknown enchantment tag `#minecraft:mining_exclusive`",
            ),
            (
                "*[stored_enchantments~[{enchantments:\"#nope\"}]]",
                "Unknown enchantment tag `#nope`",
            ),
            (
                "*[!enchantments~[{enchantments:\"#nope\"}]]",
                "Unknown enchantment tag `#nope`",
            ),
            (
                "*[minecraft:enchantments~[{enchantments:\"#minecraft:nope\"}]|damage=1]",
                "Unknown enchantment tag `#minecraft:nope`",
            ),
            (
                "*[enchantments~[{enchantments:\"#mypack:gone\"}]]",
                "Unknown enchantment tag `#mypack:gone`",
            ),
            (
                "*[enchantments~[{enchantments:\"nope\"}]]",
                "Unknown enchantment `nope`",
            ),
            (
                "*[enchantments~[{enchantments:[\"sharpness\",\"minecraft:nope\"]}]]",
                "Unknown enchantment `minecraft:nope`",
            ),
            (
                "*[enchantments~{enchantments:\"#nope\"}]",
                "`enchantments~` takes a list",
            ),
        ] {
            let found = check(token).unwrap_or_default();
            assert!(found.contains(expected), "{token}: {found}");
        }
    }
}
