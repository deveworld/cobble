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
    /// The resource was written in SNBT, as in a command, where `true` and
    /// `1b` are the same byte, so numbers and booleans stand for each other.
    pub(crate) nbt: bool,
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

// ---------------------------------------------------------------------------
// The walk. Each resource is described by the fields of its objects, and one
// walk checks every field the same way: a missing required field (`None`
// counts as missing, as in Minecraft), a value of the wrong JSON kind (text
// for a list, a list for an object), the ids Minecraft looks up, the
// resources the field names, and the nested objects, whatever resource they
// sit in. The field lists come from Minecraft 26.3's codecs, and each rule
// was confirmed on a 26.3 server.
// ---------------------------------------------------------------------------

/// A registry of ids Minecraft looks up when it loads a resource.
#[derive(Clone, Copy)]
struct Registry {
    /// Its name for [`crate::validator::registries::vanilla_ids`].
    ids: &'static str,
    /// "an item", for messages.
    what: &'static str,
    /// What a `#tag` of it names.
    tags: Tags,
    /// `minecraft:air` is refused: item stacks and ingredients.
    no_air: bool,
}

#[derive(Clone, Copy)]
enum Tags {
    /// A tag the pack can declare, checked once every file is compiled.
    Pack(RefKind),
    /// Only Minecraft's own tags are known, under this name.
    Vanilla(&'static str),
    /// Not checked.
    Unchecked,
}

const fn registry(ids: &'static str, what: &'static str, tags: Tags) -> Registry {
    Registry {
        ids,
        what,
        tags,
        no_air: false,
    }
}

const ITEM: Registry = registry("item", "an item", Tags::Pack(RefKind::ItemTag));
/// An item of an item stack or an ingredient, which can't be air.
const STACK_ITEM: Registry = Registry {
    no_air: true,
    ..ITEM
};
const BLOCK: Registry = registry("block", "a block", Tags::Pack(RefKind::BlockTag));
const ENTITY_TYPE: Registry = registry(
    "entity_type",
    "an entity type",
    Tags::Pack(RefKind::EntityTypeTag),
);
const FLUID: Registry = registry("fluid", "a fluid", Tags::Unchecked);
const BIOME: Registry = registry("biome", "a biome", Tags::Vanilla("biome_tag"));
const ENCHANTMENT: Registry = registry(
    "enchantment",
    "an enchantment",
    Tags::Vanilla("enchantment_tag"),
);
const MOB_EFFECT: Registry = registry("mob_effect", "an effect", Tags::Unchecked);
const POTION: Registry = registry("potion", "a potion", Tags::Unchecked);
const INSTRUMENT: Registry = registry("instrument", "an instrument", Tags::Unchecked);
const ATTRIBUTE: Registry = registry("attribute", "an attribute", Tags::Unchecked);
const DAMAGE_TYPE: Registry = registry("damage_type", "a damage type", Tags::Unchecked);
const TRIM_PATTERN: Registry = registry("trim_pattern", "a trim pattern", Tags::Unchecked);
const WORLD_CLOCK: Registry = registry("world_clock", "a world clock", Tags::Unchecked);
const INT_PROVIDER: Registry =
    registry("context_int_provider", "a number provider", Tags::Unchecked);
const FLOAT_PROVIDER: Registry = registry(
    "context_float_provider",
    "a number provider",
    Tags::Unchecked,
);

/// What a field holds.
#[derive(Clone, Copy)]
enum Shape {
    /// Not checked.
    Any,
    Bool,
    Number,
    /// A string.
    Text,
    /// An object whose fields aren't checked.
    Object,
    /// A text component: text, a list, or an object.
    Component,
    List(&'static Shape),
    /// A list with at least one item.
    NonEmptyList(&'static Shape),
    /// An object whose values are all of the shape.
    Values(&'static Shape),
    /// An object with these fields.
    Fields(&'static [Field]),
    /// One id of the registry.
    Id(Registry),
    /// One id, a `#tag`, or a list of ids, of the registry.
    Holders(Registry),
    /// The id of a resource.
    Ref(RefKind),
    /// The id of a resource, or a list of them.
    Refs(RefKind),
    /// A number, a number provider with a `type`, or the id of one.
    Provider(Registry),
    /// One predicate, or the id of a predicate file.
    Predicate,
    /// A predicate about an entity, in an advancement criterion.
    EntityCondition,
    /// The `terms` of `all_of` and `any_of`: a list of predicates or ids, or
    /// one id.
    Terms,
    /// One item modifier, the id of one, or a list of them.
    Modifier,
    EntityPredicate,
    /// An object whose keys are effects.
    Effects,
    /// An object whose keys are enchantments.
    Enchantments,
    /// An item id, or an object with an `id`.
    ItemStack,
    /// The result of a transmute recipe: an item stack that may leave out
    /// its `id`.
    TransmuteResult,
    Ingredient,
    LootEntry,
    Pool,
    /// The id of a loot table, or one written in place.
    LootTable,
}

#[derive(Clone, Copy)]
struct Field {
    name: &'static str,
    shape: Shape,
    required: bool,
}

/// A required field. A macro rather than a function, so a list of fields in
/// a `match` arm is a constant.
macro_rules! req {
    ($name:expr, $shape:expr $(,)?) => {
        Field {
            name: $name,
            shape: $shape,
            required: true,
        }
    };
}

/// A field that may be left out.
macro_rules! opt {
    ($name:expr, $shape:expr $(,)?) => {
        Field {
            name: $name,
            shape: $shape,
            required: false,
        }
    };
}

const INT: Shape = Shape::Provider(INT_PROVIDER);
const FLOAT: Shape = Shape::Provider(FLOAT_PROVIDER);
const ITEM_PREDICATE: Shape = Shape::Fields(&[
    opt!("items", Shape::Holders(ITEM)),
    opt!("components", Shape::Object),
    opt!("predicates", Shape::Object),
]);
const BLOCK_PREDICATE: &[Field] = &[
    opt!("blocks", Shape::Holders(BLOCK)),
    opt!("state", Shape::Object),
];
const LOCATION_PREDICATE: Shape = Shape::Fields(&[
    opt!("position", Shape::Object),
    opt!("biomes", Shape::Holders(BIOME)),
    opt!("dimension", Shape::Text),
    opt!("smokey", Shape::Bool),
    opt!("light", Shape::Object),
    opt!("block", Shape::Fields(BLOCK_PREDICATE)),
    opt!(
        "fluid",
        Shape::Fields(&[
            opt!("fluids", Shape::Holders(FLUID)),
            opt!("state", Shape::Object)
        ]),
    ),
    opt!("can_see_sky", Shape::Bool),
]);
const DAMAGE_SOURCE_PREDICATE: Shape = Shape::Fields(&[
    opt!(
        "tags",
        Shape::List(&Shape::Fields(&[
            req!("id", Shape::Holders(DAMAGE_TYPE)),
            req!("expected", Shape::Bool),
        ])),
    ),
    opt!("direct_entity", Shape::EntityPredicate),
    opt!("source_entity", Shape::EntityPredicate),
    opt!("is_direct", Shape::Bool),
]);
const DAMAGE_PREDICATE: Shape = Shape::Fields(&[
    opt!("source_entity", Shape::EntityPredicate),
    opt!("blocked", Shape::Bool),
    opt!("type", DAMAGE_SOURCE_PREDICATE),
]);
const EQUIPMENT_PREDICATE: Shape = Shape::Fields(&[
    opt!("head", ITEM_PREDICATE),
    opt!("chest", ITEM_PREDICATE),
    opt!("legs", ITEM_PREDICATE),
    opt!("feet", ITEM_PREDICATE),
    opt!("body", ITEM_PREDICATE),
    opt!("mainhand", ITEM_PREDICATE),
    opt!("offhand", ITEM_PREDICATE),
]);

/// `null` is the same as a field left out, for Minecraft.
fn present<'a>(json: &'a Value, key: &str) -> Option<&'a Value> {
    json.get(key).filter(|value| !value.is_null())
}

fn has(json: &Value, key: &str) -> bool {
    present(json, key).is_some()
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

/// `value` in a message.
fn describe(value: &Value) -> String {
    match value {
        Value::Null => "None".to_string(),
        Value::Bool(flag) => if *flag { "True" } else { "False" }.to_string(),
        Value::Number(number) => format!("the number {number}"),
        Value::String(text) => format!("the text {}", Value::String(text.clone())),
        Value::Array(_) => "a list".to_string(),
        Value::Object(_) => "an object".to_string(),
    }
}

/// Reports a value of the wrong JSON kind.
fn wrong(value: &Value, path: &str, expected: &str, problems: &mut Problems) {
    problems.push(format!(
        "`{path}` is {}, but it must be {expected}; rejected",
        describe(value)
    ));
}

/// Checks each field of an object: required fields are there, and every
/// field holds its shape.
fn check_fields(json: &Value, path: &str, fields: &[Field], problems: &mut Problems) {
    for field in fields {
        match present(json, field.name) {
            None if field.required => {
                problems.push(format!("{} is missing; rejected", at(path, field.name)))
            }
            None => {}
            Some(value) => check_value(value, &join(path, field.name), field.shape, problems),
        }
    }
}

fn check_list(items: &[Value], path: &str, shape: Shape, problems: &mut Problems) {
    for (index, item) in items.iter().enumerate() {
        check_value(item, &format!("{path}[{index}]"), shape, problems);
    }
}

/// Checks that `value` holds `shape`.
fn check_value(value: &Value, path: &str, shape: Shape, problems: &mut Problems) {
    match shape {
        Shape::Any => {}
        Shape::Bool => {
            if !(value.is_boolean() || (problems.nbt && value.is_number())) {
                wrong(value, path, "True or False", problems);
            }
        }
        Shape::Number => {
            if !(value.is_number() || (problems.nbt && value.is_boolean())) {
                wrong(value, path, "a number", problems);
            }
        }
        Shape::Text => {
            if !value.is_string() {
                wrong(value, path, "text", problems);
            }
        }
        Shape::Object => {
            if !value.is_object() {
                wrong(value, path, "an object", problems);
            }
        }
        Shape::Component => {
            if value.is_number() || value.is_boolean() {
                wrong(
                    value,
                    path,
                    "a text component (text, a list, or an object)",
                    problems,
                );
            }
        }
        Shape::List(item) => match value.as_array() {
            Some(items) => check_list(items, path, *item, problems),
            None => wrong(value, path, "a list", problems),
        },
        Shape::NonEmptyList(item) => match value.as_array() {
            Some(items) if items.is_empty() => problems.push(format!(
                "`{path}` is an empty list; it needs at least one entry; rejected"
            )),
            Some(items) => check_list(items, path, *item, problems),
            None => wrong(value, path, "a list", problems),
        },
        Shape::Values(item) => match value.as_object() {
            Some(entries) => {
                for (key, entry) in entries {
                    check_value(entry, &join(path, key), *item, problems);
                }
            }
            None => wrong(value, path, "an object", problems),
        },
        Shape::Fields(fields) => {
            if value.is_object() {
                check_fields(value, path, fields, problems);
            } else {
                wrong(value, path, "an object", problems);
            }
        }
        Shape::Id(registry) => match value.as_str() {
            Some(id) => check_id(id, path, registry, problems),
            None => wrong(
                value,
                path,
                &format!("the id of {}", registry.what),
                problems,
            ),
        },
        Shape::Holders(registry) => check_holder_set(value, path, registry, problems),
        Shape::Ref(kind) => match value.as_str() {
            Some(id) => problems.refer(kind, id, path),
            None => wrong(
                value,
                path,
                &format!("the id of a {}", kind.what()),
                problems,
            ),
        },
        Shape::Refs(kind) => match value {
            Value::String(id) => refer_holder(kind, id, path, problems),
            Value::Array(ids) => {
                for (index, id) in ids.iter().enumerate() {
                    let item_path = format!("{path}[{index}]");
                    match id.as_str() {
                        Some(id) => refer_holder(kind, id, &item_path, problems),
                        None => wrong(
                            id,
                            &item_path,
                            &format!("the id of a {}", kind.what()),
                            problems,
                        ),
                    }
                }
            }
            _ => wrong(
                value,
                path,
                &format!("the id of a {}, or a list of them", kind.what()),
                problems,
            ),
        },
        Shape::Provider(registry) => check_provider(value, path, registry, problems),
        Shape::Predicate => check_condition(value, path, false, problems),
        Shape::EntityCondition => check_condition(value, path, true, problems),
        Shape::Terms => match value {
            Value::Array(terms) => check_list(terms, path, Shape::Predicate, problems),
            Value::String(id) => refer_holder(RefKind::Predicate, id, path, problems),
            _ => wrong(value, path, "a list of predicates", problems),
        },
        Shape::Modifier => match value {
            Value::Array(modifiers) => {
                for (index, modifier) in modifiers.iter().enumerate() {
                    check_modifier(modifier, &format!("{path}[{index}]"), problems);
                }
            }
            _ => check_modifier(value, path, problems),
        },
        Shape::EntityPredicate => check_entity_predicate(value, path, problems),
        Shape::Effects => check_keys(value, path, MOB_EFFECT, problems),
        Shape::Enchantments => check_keys(value, path, ENCHANTMENT, problems),
        Shape::ItemStack => check_item_stack(value, path, true, problems),
        Shape::TransmuteResult => check_item_stack(value, path, false, problems),
        Shape::Ingredient => check_ingredient(value, path, problems),
        Shape::LootEntry => check_loot_entry(value, path, problems),
        Shape::Pool => check_pool(value, path, problems),
        Shape::LootTable => match value {
            Value::String(id) => problems.refer(RefKind::LootTable, id, path),
            Value::Object(_) => check_loot_table_at(value, path, problems),
            _ => wrong(
                value,
                path,
                "the id of a loot table, or a loot table object",
                problems,
            ),
        },
    }
}

/// One item modifier, or the id of an item modifier file.
fn check_modifier(value: &Value, path: &str, problems: &mut Problems) {
    match value {
        Value::String(id) => refer_holder(RefKind::ItemModifier, id, path, problems),
        Value::Object(_) => check_item_modifier(value, path, problems),
        _ => wrong(
            value,
            path,
            "an item modifier object or the id of an item modifier file",
            problems,
        ),
    }
}

/// An id or a `#tag` where Minecraft takes either. Tags of resources aren't
/// declared by `datapack.*`, so they aren't looked up.
fn refer_holder(kind: RefKind, id: &str, path: &str, problems: &mut Problems) {
    if !id.starts_with('#') {
        problems.refer(kind, id, path);
    }
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
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return;
    };
    let Some(id) = value.as_str() else {
        wrong(
            value,
            &join(path, key),
            &format!("the id of {what}"),
            problems,
        );
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

/// Checks a resource's `type`: it's there (`old_key` was its name before)
/// and Minecraft knows it.
fn check_type(
    json: &Value,
    path: &str,
    old_key: Option<(&str, &str)>,
    known: &[&str],
    what: &str,
    problems: &mut Problems,
) {
    match old_key {
        Some((old, example)) if has(json, old) && !has(json, "type") => problems.push(format!(
            "{} is now `type` ({}); rejected",
            at(path, old),
            renamed_to_type(json, old, example)
        )),
        _ if !has(json, "type") => {
            problems.push(format!("{} is missing; rejected", at(path, "type")))
        }
        _ => check_registry_id(json.get("type"), path, "type", known, what, problems),
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
fn check_id(id: &str, path: &str, registry: Registry, problems: &mut Problems) {
    if id.contains("$(") {
        return;
    }
    let Some(name) = vanilla_name(id) else {
        return;
    };
    if registry.no_air && name == "air" {
        problems.push(format!(
            "`{path}` `{id}` is not an item Minecraft takes here: an item stack or an ingredient can't be air; rejected"
        ));
        return;
    }
    let Some(known) = crate::validator::registries::vanilla_ids(registry.ids) else {
        return;
    };
    if !known.contains(name) {
        problems.push(format!(
            "`{path}` `{id}` is not {} in Minecraft 26.3; rejected",
            registry.what
        ));
    }
}

/// An item, block, or entity type list: one id, a `#tag`, or a list of ids.
/// A list can't hold a tag.
fn check_holder_set(value: &Value, path: &str, registry: Registry, problems: &mut Problems) {
    match value {
        Value::String(id) => match id.strip_prefix('#') {
            Some(tag) => check_tag(tag, path, registry, problems),
            None => check_id(id, path, registry, problems),
        },
        Value::Array(ids) => {
            for (index, id) in ids.iter().enumerate() {
                let item_path = format!("{path}[{index}]");
                match id.as_str() {
                    Some(id) if id.starts_with('#') => problems.push(format!(
                        "`{item_path}` is the tag `{id}`, but a list holds ids only; write the tag on its own instead of the list (\"{id}\"), or list the ids; rejected"
                    )),
                    Some(id) => check_id(id, &item_path, registry, problems),
                    None => wrong(id, &item_path, &format!("the id of {}", registry.what), problems),
                }
            }
        }
        _ => wrong(
            value,
            path,
            &format!("the id of {}, a `#tag`, or a list of ids", registry.what),
            problems,
        ),
    }
}

fn check_tag(tag: &str, path: &str, registry: Registry, problems: &mut Problems) {
    match registry.tags {
        Tags::Pack(kind) => problems.refer(kind, tag, path),
        Tags::Vanilla(tags) => {
            if tag.contains("$(") {
                return;
            }
            let Some(name) = vanilla_name(tag) else {
                return;
            };
            let Some(known) = crate::validator::registries::vanilla_ids(tags) else {
                return;
            };
            if !known.contains(name) {
                problems.push(format!(
                    "`{path}` names the tag `#{tag}`, which Minecraft doesn't define; rejected"
                ));
            }
        }
        Tags::Unchecked => {}
    }
}

/// A number provider: a number, an object with a `type`, or the id of a
/// provider Minecraft defines.
fn check_provider(value: &Value, path: &str, registry: Registry, problems: &mut Problems) {
    match value {
        Value::Number(_) => {}
        Value::Bool(_) if problems.nbt => {}
        Value::String(id) => check_id(id, path, registry, problems),
        Value::Object(_) if !has(value, "type") => problems.push(format!(
            "`{path}` is a number provider without a `type`; write the number, or name its type, as in {{\"type\": \"minecraft:uniform\", \"min\": 1, \"max\": 3}}; rejected"
        )),
        Value::Object(_) => {}
        _ => wrong(
            value,
            path,
            "a number, a number provider object with a `type`, or the id of one",
            problems,
        ),
    }
}

/// The keys of an object are ids of `registry`.
fn check_keys(value: &Value, path: &str, registry: Registry, problems: &mut Problems) {
    match value.as_object() {
        Some(entries) => {
            for key in entries.keys() {
                check_id(key, path, registry, problems);
            }
        }
        None => wrong(value, path, "an object", problems),
    }
}

/// A `condition` field: one predicate, or the id of a predicate file. In an
/// advancement criterion, an entity condition was a list of predicates or an
/// entity predicate on its own before.
fn check_condition(condition: &Value, path: &str, entity: bool, problems: &mut Problems) {
    match condition {
        Value::String(id) => problems.refer(RefKind::Predicate, id, path),
        Value::Array(list) if entity => {
            problems.push(format!(
                "`{path}` is now one predicate, not a list (combine several with \"type\": \"minecraft:all_of\"); rejected"
            ));
            check_list(list, path, Shape::Predicate, problems);
        }
        Value::Array(_) => problems.push(format!(
            "`{path}` is one predicate, not a list (combine several with \"type\": \"minecraft:all_of\" and \"terms\"); rejected"
        )),
        Value::Object(_) if entity && !has(condition, "type") && !has(condition, "condition") => {
            problems.push(format!(
                "`{path}` is now a predicate with a `type`; wrap an entity predicate as {{\"type\": \"minecraft:entity_properties\", \"entity\": \"this\", \"predicate\": {{...}}}}; rejected"
            ))
        }
        Value::Object(_) => check_predicate(condition, path, problems),
        _ => wrong(
            condition,
            path,
            "one predicate, or the id of a predicate file",
            problems,
        ),
    }
}

/// Predicates: `condition` is now `type`, including the predicates nested in
/// `all_of`/`any_of` terms and `inverted`, and each type has the fields it
/// can't go without.
pub(crate) fn check_predicate(json: &Value, path: &str, problems: &mut Problems) {
    if !json.is_object() {
        return;
    }
    check_type(
        json,
        path,
        Some(("condition", "minecraft:random_chance")),
        LOOT_CONDITION_TYPES,
        "a predicate type",
        problems,
    );
    let kind = type_name(json);
    if kind == "time_check" && !has(json, "clock") {
        problems.push(format!(
            "{} is missing; a time check names the clock it reads, such as \"clock\": \"minecraft:overworld\"; rejected",
            at(path, "clock")
        ));
    }
    check_fields(json, path, predicate_fields(kind), problems);
}

/// The `"type": ...` that replaces the old `key`, with its value when it is
/// text and `example` otherwise.
fn renamed_to_type(json: &Value, key: &str, example: &str) -> String {
    let value = json.get(key).and_then(Value::as_str).unwrap_or(example);
    format!("\"type\": {}", Value::String(value.to_string()))
}

/// The fields of each predicate type (`clock` of a time check has its own
/// message when it's missing).
fn predicate_fields(kind: &str) -> &'static [Field] {
    match kind {
        "all_of" | "any_of" => &[req!("terms", Shape::Terms)],
        "damage_source_properties" => &[opt!("predicate", DAMAGE_SOURCE_PREDICATE)],
        "enchantment_active_check" => &[req!("active", Shape::Bool)],
        "entity_properties" => &[
            req!("entity", Shape::Text),
            opt!("predicate", Shape::EntityPredicate),
        ],
        "entity_scores" => &[req!("entity", Shape::Text), req!("scores", Shape::Object)],
        "environment_attribute_check" => {
            &[req!("attribute", Shape::Text), req!("value", Shape::Any)]
        }
        "float_value_check" => &[req!("value", FLOAT), req!("test", Shape::Any)],
        "int_value_check" => &[req!("value", INT), req!("test", Shape::Any)],
        "inverted" => &[req!("term", Shape::Predicate)],
        "location_check" => &[opt!("predicate", LOCATION_PREDICATE)],
        "match_block" => BLOCK_PREDICATE,
        "match_tool" => &[opt!("predicate", ITEM_PREDICATE)],
        "random_chance" => &[req!("chance", FLOAT)],
        "random_chance_with_enchanted_bonus" => &[
            req!("unenchanted_chance", Shape::Number),
            req!("enchanted_chance", Shape::Any),
            req!("enchantment", Shape::Id(ENCHANTMENT)),
        ],
        "table_bonus" => &[
            req!("enchantment", Shape::Id(ENCHANTMENT)),
            req!("chances", Shape::NonEmptyList(&Shape::Number)),
        ],
        "time_check" => &[
            opt!("clock", Shape::Id(WORLD_CLOCK)),
            req!("value", Shape::Any),
        ],
        "weather_check" => &[
            opt!("raining", Shape::Bool),
            opt!("thundering", Shape::Bool),
        ],
        _ => &[],
    }
}

/// An entity predicate is an object of sub-predicates, whose names Minecraft
/// knows. Other namespaces' names, which a mod can add, aren't checked.
fn check_entity_predicate(json: &Value, path: &str, problems: &mut Problems) {
    let Some(predicates) = json.as_object() else {
        wrong(json, path, "an entity predicate object", problems);
        return;
    };
    for (key, value) in predicates {
        let Some(name) = vanilla_name(key) else {
            continue;
        };
        let key_path = join(path, key);
        let shape = match name {
            "entity_type" => Shape::Holders(ENTITY_TYPE),
            "location" | "stepping_on" | "movement_affected_by" => LOCATION_PREDICATE,
            "effects" => Shape::Effects,
            "equipment" => EQUIPMENT_PREDICATE,
            "vehicle" | "passenger" | "targeted_entity" => Shape::EntityPredicate,
            "slots" => Shape::Values(&ITEM_PREDICATE),
            "flags" | "distance" | "movement" | "nbt" | "periodic_tick" | "team" | "components"
            | "predicates" | "entity_tags" => Shape::Any,
            name if name.starts_with("type_specific/") => Shape::Any,
            _ => {
                let hint = if name == "type" {
                    " (the entity type is \"minecraft:entity_type\")"
                } else {
                    ""
                };
                problems.push(format!(
                    "`{key_path}` is not a part of an entity predicate in Minecraft 26.3{hint}; rejected"
                ));
                continue;
            }
        };
        check_value(value, &key_path, shape, problems);
    }
}

/// `conditions` lists on loot functions, pools, and entries are now a single
/// `condition` (use `minecraft:all_of` to combine several), and `functions`
/// lists of loot tables are now `modifier`.
fn check_old_lists(json: &Value, path: &str, functions: bool, problems: &mut Problems) {
    if has(json, "conditions") {
        problems.push(format!(
            "{} is now a single `condition` (combine several with \"type\": \"minecraft:all_of\"); ignored, so the conditions would never apply",
            at(path, "conditions")
        ));
    }
    if functions && has(json, "functions") {
        problems.push(format!(
            "{} is now `modifier`; ignored, so the functions would never apply",
            at(path, "functions")
        ));
    }
}

/// The fields of each item modifier type, besides `condition`.
fn item_modifier_fields(kind: &str) -> &'static [Field] {
    const PAGES: &[Field] = &[
        req!("pages", Shape::List(&Shape::Any)),
        req!("mode", Shape::Text),
    ];
    const OPTIONS: Field = opt!("options", Shape::Holders(ENCHANTMENT));
    match kind {
        "apply_bonus" => &[
            req!("enchantment", Shape::Id(ENCHANTMENT)),
            req!("formula", Shape::Text),
            opt!("parameters", Shape::Object),
        ],
        "copy_components" => &[req!("source", Shape::Text)],
        "copy_custom_data" => &[
            req!("source", Shape::Any),
            req!("ops", Shape::List(&Shape::Object)),
        ],
        "copy_name" => &[req!("source", Shape::Text)],
        "copy_state" => &[
            req!("block", Shape::Id(BLOCK)),
            req!("properties", Shape::List(&Shape::Text)),
        ],
        "enchant_randomly" => &[OPTIONS, opt!("only_compatible", Shape::Bool)],
        "enchant_with_levels" => &[req!("levels", INT), OPTIONS],
        "enchanted_count_increase" => &[
            req!("enchantment", Shape::Id(ENCHANTMENT)),
            req!("count", FLOAT),
            opt!("limit", Shape::Number),
        ],
        "exploration_map" => &[req!("destination", Shape::Any)],
        "fill_player_head" => &[req!("entity", Shape::Text)],
        "filtered" => &[
            req!("item_filter", ITEM_PREDICATE),
            opt!("on_pass", Shape::Modifier),
            opt!("on_fail", Shape::Modifier),
        ],
        "limit_count" => &[req!("limit", Shape::Any)],
        "modify_contents" => &[
            req!("component", Shape::Text),
            req!("modifier", Shape::Modifier),
        ],
        "sequence" => &[req!("functions", Shape::Modifier)],
        "set_attributes" => &[
            req!(
                "modifiers",
                Shape::List(&Shape::Fields(&[
                    req!("id", Shape::Text),
                    req!("attribute", Shape::Id(ATTRIBUTE)),
                    req!("operation", Shape::Text),
                    req!("amount", FLOAT),
                    req!("slot", Shape::Any),
                ])),
            ),
            opt!("replace", Shape::Bool),
        ],
        "set_banner_pattern" => &[
            req!("patterns", Shape::List(&Shape::Any)),
            req!("append", Shape::Bool),
        ],
        "set_components" => &[req!("components", Shape::Object)],
        "set_contents" => &[
            req!("component", Shape::Text),
            req!("entries", Shape::List(&Shape::LootEntry)),
        ],
        "set_count" => &[req!("count", INT), opt!("add", Shape::Bool)],
        "set_custom_data" => &[req!("tag", Shape::Any)],
        "set_damage" => &[req!("damage", FLOAT), opt!("add", Shape::Bool)],
        "set_enchantments" => &[
            opt!("enchantments", Shape::Enchantments),
            opt!("add", Shape::Bool),
        ],
        "set_instrument" => &[req!("options", Shape::Holders(INSTRUMENT))],
        "set_item" => &[req!("item", Shape::Id(STACK_ITEM))],
        "set_loot_table" => &[req!("loot_table_id", Shape::Ref(RefKind::LootTable))],
        "set_lore" => &[
            req!("lore", Shape::List(&Shape::Component)),
            req!("mode", Shape::Text),
        ],
        "set_name" => &[opt!("name", Shape::Component)],
        "set_ominous_bottle_amplifier" => &[req!("amplifier", INT)],
        "set_potion" => &[req!("id", Shape::Id(POTION))],
        "set_random_dyes" => &[req!("number_of_dyes", INT)],
        "set_random_potion" => &[opt!("options", Shape::Holders(POTION))],
        "set_stew_effect" => &[opt!(
            "effects",
            Shape::List(&Shape::Fields(&[
                req!("type", Shape::Id(MOB_EFFECT)),
                req!("duration", INT),
            ])),
        )],
        "set_writable_book_pages" | "set_written_book_pages" => PAGES,
        "toggle_tooltips" => &[req!("toggles", Shape::Object)],
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
    check_type(
        json,
        path,
        Some(("function", "minecraft:set_components")),
        LOOT_FUNCTION_TYPES,
        "an item modifier type",
        problems,
    );
    check_old_lists(json, path, false, problems);
    check_fields(json, path, &[opt!("condition", Shape::Predicate)], problems);
    check_fields(json, path, item_modifier_fields(type_name(json)), problems);
}

/// Loot tables: `functions` lists are now `modifier`, `conditions` lists are
/// a single `condition`, a `minecraft:tag` entry names its items in `items`,
/// and pools and entries have the fields they can't go without.
pub(crate) fn check_loot_table(json: &Value, problems: &mut Problems) {
    check_loot_table_at(json, "", problems);
}

fn check_loot_table_at(json: &Value, path: &str, problems: &mut Problems) {
    check_old_lists(json, path, true, problems);
    check_fields(
        json,
        path,
        &[
            opt!("type", Shape::Text),
            opt!("random_sequence", Shape::Text),
            opt!("pools", Shape::List(&Shape::Pool)),
            opt!("modifier", Shape::Modifier),
        ],
        problems,
    );
}

fn check_pool(pool: &Value, path: &str, problems: &mut Problems) {
    if !pool.is_object() {
        wrong(pool, path, "a pool object", problems);
        return;
    }
    check_old_lists(pool, path, true, problems);
    check_fields(
        pool,
        path,
        &[
            req!("rolls", INT),
            opt!("bonus_rolls", FLOAT),
            req!("entries", Shape::List(&Shape::LootEntry)),
            opt!("condition", Shape::Predicate),
            opt!("modifier", Shape::Modifier),
        ],
        problems,
    );
}

fn check_loot_entry(entry: &Value, path: &str, problems: &mut Problems) {
    if !entry.is_object() {
        wrong(entry, path, "a loot entry object", problems);
        return;
    }
    check_old_lists(entry, path, true, problems);
    check_type(
        entry,
        path,
        None,
        LOOT_ENTRY_TYPES,
        "a loot entry type",
        problems,
    );
    let kind = type_name(entry);
    let mut fields = vec![
        opt!("condition", Shape::Predicate),
        opt!("modifier", Shape::Modifier),
    ];
    match kind {
        "alternatives" | "group" | "sequence" => {
            fields.push(opt!("children", Shape::List(&Shape::LootEntry)))
        }
        _ => fields.extend([
            opt!("weight", Shape::Number),
            opt!("quality", Shape::Number),
        ]),
    }
    match kind {
        "item" => match present(entry, "name") {
            Some(Value::String(id)) if vanilla_name(id) == Some("air") => problems.push(format!(
                "{} `{id}` is not an item a loot table can drop; for a pool entry that drops nothing, use {{\"type\": \"minecraft:empty\"}}; rejected",
                at(path, "name")
            )),
            Some(Value::String(id)) if id.starts_with('#') => problems.push(format!(
                "{} `{id}` names a tag; a `minecraft:tag` entry drops the items of a tag (\"type\": \"minecraft:tag\", \"items\": \"{id}\"); rejected",
                at(path, "name")
            )),
            _ => fields.push(req!("name", Shape::Id(ITEM))),
        },
        "tag" if has(entry, "name") && !has(entry, "items") => problems.push(format!(
            "{} of a `minecraft:tag` entry is now `items` (\"items\": \"#minecraft:logs\"); rejected",
            at(path, "name")
        )),
        "tag" => fields.extend([req!("items", Shape::Holders(ITEM)), opt!("expand", Shape::Bool)]),
        "loot_table" if has(entry, "name") && !has(entry, "value") => problems.push(format!(
            "{} of a `minecraft:loot_table` entry is now `value` (\"value\": \"minecraft:chests/simple_dungeon\"); rejected",
            at(path, "name")
        )),
        "loot_table" => fields.push(req!("value", Shape::LootTable)),
        "dynamic" => fields.push(req!("name", Shape::Text)),
        "slots" => fields.push(req!("slot_source", Shape::Any)),
        _ => {}
    }
    check_fields(entry, path, &fields, problems);
}

/// The conditions of each advancement trigger, besides `player`, which all
/// but `minecraft:impossible` have. `None` for a trigger that reads none.
fn trigger_fields(trigger: &str) -> Option<&'static [Field]> {
    const EC: Shape = Shape::EntityCondition;
    const P: Shape = Shape::Predicate;
    const IP: Shape = ITEM_PREDICATE;
    const BLOCKS: &[Field] = &[
        opt!("blocks", Shape::Holders(BLOCK)),
        opt!("state", Shape::Object),
    ];
    const DISTANCE: &[Field] = &[
        opt!("start_position", LOCATION_PREDICATE),
        opt!("distance", Shape::Object),
    ];
    const KILLED: &[Field] = &[
        opt!("entity", EC),
        opt!("killing_blow", DAMAGE_SOURCE_PREDICATE),
    ];
    const ITEM_ONLY: &[Field] = &[opt!("item", IP)];
    const ITEM_AND_ENTITY: &[Field] = &[opt!("item", IP), opt!("entity", EC)];
    const LOCATION: &[Field] = &[opt!("location", P)];
    const ENTITY: &[Field] = &[opt!("entity", EC)];
    const RECIPES: &[Field] = &[
        req!("recipes", Shape::Refs(RefKind::Recipe)),
        opt!("ingredients", Shape::List(&IP)),
    ];
    Some(match trigger {
        "impossible" => return None,
        "allay_drop_item_on_block"
        | "any_block_use"
        | "default_block_use"
        | "item_used_on_block"
        | "placed_block" => LOCATION,
        "avoid_vibration"
        | "hero_of_the_village"
        | "location"
        | "slept_in_bed"
        | "started_riding"
        | "tick"
        | "voluntary_exile" => &[],
        "bee_nest_destroyed" => &[
            opt!("blocks", Shape::Holders(BLOCK)),
            opt!("state", Shape::Object),
            opt!("item", IP),
            opt!("num_bees_inside", Shape::Any),
        ],
        "bred_animals" => &[opt!("parent", EC), opt!("partner", EC), opt!("child", EC)],
        "brewed_potion" => &[opt!("potion", Shape::Object)],
        "changed_dimension" => &[opt!("from", Shape::Text), opt!("to", Shape::Text)],
        "channeled_lightning" => &[opt!("victims", Shape::List(&EC))],
        "construct_beacon" => &[opt!("level", Shape::Any)],
        "consume_item" | "filled_bucket" | "shot_crossbow" | "used_totem" | "using_item" => {
            ITEM_ONLY
        }
        "crafter_recipe_crafted" | "recipe_crafted" => RECIPES,
        "cured_zombie_villager" => &[opt!("zombie", EC), opt!("villager", EC)],
        "effects_changed" => &[opt!("effects", Shape::Effects), opt!("source", EC)],
        "enchanted_item" => &[opt!("item", IP), opt!("levels", Shape::Any)],
        "enter_block" | "slide_down_block" => BLOCKS,
        "entity_hurt_player" => &[opt!("damage", DAMAGE_PREDICATE)],
        "entity_killed_player" | "kill_mob_near_sculk_catalyst" | "player_killed_entity" => KILLED,
        "fall_after_explosion" => &[
            opt!("start_position", LOCATION_PREDICATE),
            opt!("distance", Shape::Object),
            opt!("cause", EC),
        ],
        "fall_from_height" | "nether_travel" | "ride_entity_in_lava" => DISTANCE,
        "fishing_rod_hooked" => &[opt!("rod", IP), opt!("entity", EC), opt!("item", IP)],
        "inventory_changed" => &[
            opt!("slots", Shape::Object),
            opt!("items", Shape::List(&IP)),
        ],
        "item_durability_changed" => &[
            opt!("item", IP),
            opt!("durability", Shape::Any),
            opt!("delta", Shape::Any),
        ],
        "killed_by_arrow" => &[
            opt!("victims", Shape::List(&EC)),
            opt!("unique_entity_types", Shape::Any),
            opt!("fired_from_weapon", IP),
        ],
        "levitation" => &[
            opt!("distance", Shape::Object),
            opt!("duration", Shape::Any),
        ],
        "lightning_strike" => &[opt!("lightning", EC), opt!("bystander", EC)],
        "player_generates_container_loot" => {
            &[req!("loot_tables", Shape::Refs(RefKind::LootTable))]
        }
        "player_hurt_entity" => &[opt!("damage", DAMAGE_PREDICATE), opt!("entity", EC)],
        "player_interacted_with_entity"
        | "player_sheared_equipment"
        | "thrown_item_picked_up_by_entity"
        | "thrown_item_picked_up_by_player" => ITEM_AND_ENTITY,
        "recipe_unlocked" => &[req!("recipes", Shape::Refs(RefKind::Recipe))],
        "spear_mobs" => &[opt!("count", Shape::Number)],
        "summoned_entity" | "tame_animal" => ENTITY,
        "target_hit" => &[opt!("signal_strength", Shape::Any), opt!("projectile", EC)],
        "used_ender_eye" => &[opt!("distance", Shape::Any)],
        "villager_trade" => &[opt!("villager", EC), opt!("item", IP)],
        _ => return None,
    })
}

/// Advancements: criteria and their triggers, `requirements`, `display`,
/// `parent`, and `rewards`.
pub(super) fn check_advancement(json: &Value, problems: &mut Problems) {
    let criteria = present(json, "criteria");
    match criteria {
        None => require(json, "", &["criteria"], problems),
        Some(Value::Object(criteria)) if criteria.is_empty() => problems.push(
            "`criteria` is empty; an advancement needs at least one criterion; rejected"
                .to_string(),
        ),
        Some(Value::Object(_)) => {}
        Some(other) => wrong(other, "criteria", "an object of criteria", problems),
    }
    let criteria = criteria.and_then(Value::as_object);
    for (name, criterion) in criteria.into_iter().flatten() {
        check_criterion(criterion, &format!("criteria.{name}"), problems);
    }
    check_requirements(json, criteria, problems);
    check_fields(
        json,
        "",
        &[
            opt!("parent", Shape::Ref(RefKind::Advancement)),
            opt!(
                "rewards",
                Shape::Fields(&[
                    opt!("experience", Shape::Number),
                    opt!("loot", Shape::Refs(RefKind::LootTable)),
                    opt!("recipes", Shape::List(&Shape::Text)),
                    opt!("function", Shape::Text),
                ]),
            ),
            opt!("sends_telemetry_event", Shape::Bool),
        ],
        problems,
    );
    // `"display": None` is the same as no display.
    let Some(display) = present(json, "display") else {
        return;
    };
    check_value(
        display,
        "display",
        Shape::Fields(&[
            req!("icon", Shape::ItemStack),
            req!("title", Shape::Component),
            req!("description", Shape::Component),
            opt!("background", Shape::Text),
            opt!("frame", Shape::Text),
            opt!("show_toast", Shape::Bool),
            opt!("announce_to_chat", Shape::Bool),
            opt!("hidden", Shape::Bool),
        ]),
        problems,
    );
    if let Some(frame) = present(display, "frame").and_then(Value::as_str) {
        if !matches!(frame, "task" | "challenge" | "goal") {
            problems.push(format!(
                "`display.frame` `{frame}` is not a frame; use \"task\", \"goal\", or \"challenge\"; rejected"
            ));
        }
    }
    if display.is_object() && !has(json, "parent") && !has(display, "background") {
        problems.push(
            "a root advancement with `display` must set `display.background` (or give it a `parent`); rejected"
                .to_string(),
        );
    }
}

/// One criterion: its `trigger` and the conditions that trigger reads.
/// Minecraft ignores the others, so they're warnings.
fn check_criterion(criterion: &Value, path: &str, problems: &mut Problems) {
    if !criterion.is_object() {
        wrong(
            criterion,
            path,
            "a criterion object with a `trigger`",
            problems,
        );
        return;
    }
    if !has(criterion, "trigger") {
        problems.push(format!("{} is missing; rejected", at(path, "trigger")));
    }
    check_registry_id(
        criterion.get("trigger"),
        path,
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
    let Some(conditions) = present(criterion, "conditions") else {
        return;
    };
    let conditions_path = join(path, "conditions");
    let Some(entries) = conditions.as_object() else {
        wrong(conditions, &conditions_path, "an object", problems);
        return;
    };
    if !TRIGGER_TYPES.contains(&trigger) {
        return;
    }
    let fields = trigger_fields(trigger);
    if let Some(fields) = fields {
        check_fields(
            conditions,
            &conditions_path,
            &[opt!("player", Shape::EntityCondition)],
            problems,
        );
        check_fields(conditions, &conditions_path, fields, problems);
    }
    for key in entries.keys() {
        let read = fields
            .is_some_and(|fields| key == "player" || fields.iter().any(|field| field.name == key));
        if !read {
            problems.warn(format!(
                "`{conditions_path}.{key}` is ignored: `minecraft:{trigger}` has no `{key}` condition"
            ));
        }
    }
}

/// Reports each of `keys` that `json` lacks.
fn require(json: &Value, path: &str, keys: &[&str], problems: &mut Problems) {
    for key in keys {
        if !has(json, key) {
            problems.push(format!("{} is missing; rejected", at(path, key)));
        }
    }
}

/// `requirements` is a list of lists of criterion names, and every criterion
/// is in one of them.
fn check_requirements(
    json: &Value,
    criteria: Option<&serde_json::Map<String, Value>>,
    problems: &mut Problems,
) {
    let Some(requirements) = present(json, "requirements") else {
        return;
    };
    let Some(requirements) = requirements.as_array() else {
        wrong(
            requirements,
            "requirements",
            "a list of lists of criterion names, as in [[\"a\"], [\"b\", \"c\"]]",
            problems,
        );
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
                wrong(
                    name,
                    &format!("requirements[{index}][{item_index}]"),
                    "the name of a criterion",
                    problems,
                );
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

/// An item stack is an item id or an object that names its item in `id`
/// (formerly `item`). A transmute result may leave out its id.
fn check_item_stack(stack: &Value, path: &str, needs_id: bool, problems: &mut Problems) {
    match stack {
        Value::String(id) => check_id(id, path, STACK_ITEM, problems),
        Value::Object(_) if has(stack, "item") && !has(stack, "id") => problems.push(format!(
            "{} is now `id` (\"id\": \"minecraft:diamond\"); rejected",
            at(path, "item")
        )),
        Value::Object(_) => check_fields(
            stack,
            path,
            &[
                Field {
                    required: needs_id,
                    ..opt!("id", Shape::Id(STACK_ITEM))
                },
                opt!("count", Shape::Number),
                opt!("components", Shape::Object),
            ],
            problems,
        ),
        _ => wrong(
            stack,
            path,
            "an item id, or an object with the item's `id`",
            problems,
        ),
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
            check_holder_set(ingredient, path, STACK_ITEM, problems);
        }
        _ => check_holder_set(ingredient, path, STACK_ITEM, problems),
    }
}

/// Minecraft holds one recipe of a type without fields, so a pack can only
/// replace `minecraft:repair_item`, not add a second one under `id`.
pub(super) fn check_recipe_id(json: &Value, id: &str, problems: &mut Problems) {
    if type_name(json) == "crafting_special_repairitem" && id != "minecraft:repair_item" {
        problems.push(
            "`type` `minecraft:crafting_special_repairitem` is the recipe `minecraft:repair_item`, which Minecraft holds once; a pack can replace it under that id, but not add a second one; rejected"
                .to_string(),
        );
    }
}

/// The fields of each recipe type (`cookingtime` has its own messages).
fn recipe_fields(kind: &str) -> &'static [Field] {
    const I: Shape = Shape::Ingredient;
    const RESULT: Field = req!("result", Shape::ItemStack);
    const POTION_INGREDIENT: Shape = Shape::Fields(&[
        req!("item", Shape::Ingredient),
        opt!("potion_contents", Shape::Any),
    ]);
    match kind {
        "smelting" | "blasting" | "smoking" | "campfire_cooking" => &[
            req!("ingredient", I),
            RESULT,
            opt!("experience", Shape::Number),
        ],
        "stonecutting" => &[req!("ingredient", I), RESULT],
        "crafting_shaped" => &[
            req!("pattern", Shape::List(&Shape::Text)),
            req!("key", Shape::Values(&I)),
            RESULT,
        ],
        "crafting_shapeless" => &[req!("ingredients", Shape::List(&I)), RESULT],
        "crafting_transmute" => &[
            req!("input", I),
            req!("material", I),
            req!("result", Shape::TransmuteResult),
        ],
        "crafting_dye" => &[req!("target", I), req!("dye", I), RESULT],
        "crafting_imbue" => &[req!("source", I), req!("material", I), RESULT],
        "crafting_decorated_pot" => &[
            req!("back", I),
            req!("left", I),
            req!("right", I),
            req!("front", I),
            RESULT,
        ],
        "crafting_special_bannerduplicate" => &[req!("banner", I), RESULT],
        "crafting_special_bookcloning" => &[req!("source", I), req!("material", I), RESULT],
        "crafting_special_firework_rocket" => {
            &[req!("shell", I), req!("fuel", I), req!("star", I), RESULT]
        }
        "crafting_special_firework_star" => &[
            req!("shapes", Shape::Values(&I)),
            req!("trail", I),
            req!("twinkle", I),
            req!("fuel", I),
            req!("dye", I),
            RESULT,
        ],
        "crafting_special_firework_star_fade" => &[req!("target", I), req!("dye", I), RESULT],
        "crafting_special_mapextending" => &[
            req!("map", I),
            req!("material", I),
            req!("result", Shape::TransmuteResult),
        ],
        "crafting_special_shielddecoration" => &[req!("banner", I), req!("target", I), RESULT],
        "smithing_transform" => &[
            opt!("template", I),
            req!("base", I),
            opt!("addition", I),
            RESULT,
        ],
        "smithing_trim" => &[
            req!("template", I),
            req!("base", I),
            req!("addition", I),
            req!("pattern", Shape::Id(TRIM_PATTERN)),
        ],
        "brewing" => &[
            req!("input", POTION_INGREDIENT),
            req!("reagent", POTION_INGREDIENT),
            req!("output", Shape::ItemStack),
        ],
        _ => &[],
    }
}

/// Recipes: the type must exist, ingredients are ids or tags, a shaped
/// pattern matches its key, the result names an item, and cooking recipes
/// set a whole `cookingtime`.
pub(super) fn check_recipe(json: &Value, problems: &mut Problems) {
    check_type(json, "", None, RECIPE_TYPES, "a recipe type", problems);
    let recipe_type = type_name(json);
    check_fields(json, "", recipe_fields(recipe_type), problems);
    check_fields(
        json,
        "",
        &[
            opt!("group", Shape::Text),
            opt!("show_notification", Shape::Bool),
        ],
        problems,
    );
    if recipe_type == "crafting_shaped" {
        check_shaped_pattern(json, problems);
    }
    if recipe_type == "crafting_shapeless" {
        if let Some(ingredients) = present(json, "ingredients").and_then(Value::as_array) {
            if ingredients.is_empty() || ingredients.len() > 9 {
                problems.push(format!(
                    "`ingredients` has {} ingredients; a shapeless recipe takes 1 to 9; rejected",
                    ingredients.len()
                ));
            }
        }
    }
    check_recipe_category(json, recipe_type, problems);
    if let Some(count) = present(json, "result")
        .and_then(|result| present(result, "count"))
        .and_then(Value::as_f64)
    {
        if !(1.0..=99.0).contains(&count) {
            problems.push(format!(
                "`result.count` is {count}; a recipe makes 1 to 99 items; rejected"
            ));
        }
    }
    if matches!(
        recipe_type,
        "smelting" | "blasting" | "smoking" | "campfire_cooking"
    ) {
        match present(json, "cookingtime") {
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
        "crafting_shaped" | "crafting_shapeless" | "crafting_transmute" | "crafting_dye"
        | "crafting_imbue" => &["building", "redstone", "equipment", "misc"],
        "smelting" | "blasting" | "smoking" | "campfire_cooking" => &["food", "blocks", "misc"],
        _ => return,
    };
    let Some(category) = present(json, "category") else {
        return;
    };
    let listed = categories
        .iter()
        .map(|category| format!("\"{category}\""))
        .collect::<Vec<_>>()
        .join(", ");
    match category.as_str() {
        Some(name) if categories.contains(&name) => {}
        Some(name) => problems.push(format!(
            "`category` `{name}` is not a category of this recipe; use {listed}; rejected"
        )),
        None => wrong(category, "category", &format!("one of {listed}"), problems),
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
    if width > 0
        && rows
            .iter()
            .all(|row| row.chars().all(|symbol| symbol == ' '))
    {
        problems.push(
            "`pattern` is all spaces; a shaped recipe needs at least one ingredient in it; rejected"
                .to_string(),
        );
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
