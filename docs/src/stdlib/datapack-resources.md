# Data Pack Resources

Top-level `datapack.*` declarations generate tags, predicates, advancements,
loot tables, recipes, item modifiers, and dialogs as JSON files in the 26.x
folder layout. Resource IDs must use lowercase `namespace:path` syntax or lowercase relative
paths. Cobble reports uppercase characters, invalid path separators, and common
`namespace/path` mistakes with focused diagnostics.

```python
from stdlib import datapack

datapack.function_tag("utility", ["mypack:setup"])
datapack.function_tag("minecraft:load", ["mypack:setup"])
datapack.block_tag("solid_blocks", ["minecraft:stone"])
datapack.item_tag("reward_items", [
    "minecraft:diamond",
    {"id": "minecraft:netherite_ingot", "required": False}
])
datapack.entity_type_tag("targets", ["minecraft:zombie"])

datapack.predicate("is_sneaking", {
    "type": "minecraft:entity_properties",
    "entity": "this",
    "predicate": {
        "flags": {
            "is_sneaking": True
        }
    }
})

datapack.dialog("notice", {
    "type": "minecraft:notice",
    "title": {"text": "Notice"}
})
# Adds a button for the dialog to the pause screen.
datapack.dialog_tag("minecraft:pause_screen_additions", ["mypack:notice"])
```

Supported resource declarations:

- `datapack.function_tag(name, values, replace=False?)`
- `datapack.block_tag(name, values, replace=False?)`
- `datapack.item_tag(name, values, replace=False?)`
- `datapack.entity_type_tag(name, values, replace=False?)`
- `datapack.dialog_tag(name, values, replace=False?)`
- `datapack.predicate(name, json)`
- `datapack.advancement(name, json)`
- `datapack.loot_table(name, json)`
- `datapack.recipe(name, json)`
- `datapack.item_modifier(name, json)`
- `datapack.dialog(name, json)`

Typed tag declarations with the same ID are merged and deduplicated. Minecraft
runs a function tag in the order of its values, so a function tag keeps the
declared order: later declarations are appended in declaration order, and a
repeated value stays where it first appeared. In `minecraft:load` and
`minecraft:tick`, `event.LOAD` and `event.TICK` listeners take their place in the
same source order, so a listener added after a declaration runs after its
entries. The pack's setup function `_cobble_init` runs first in
`minecraft:load`. Block, item, entity type, and dialog tags are sorted.
Tag values may be string resource IDs or object entries with `id` and optional
`required`. Object entries with `required: true` are equivalent to strings;
`required: false` entries remain optional Minecraft tag entries.
An unknown `minecraft:` id in a block, item, or entity type tag is an error,
because Minecraft doesn't load a tag with an entry it can't find, and a function
that names that tag doesn't load either. With `"required": False`, the entry is
skipped when it's missing and the rest of the tag loads, which suits an id from
another Minecraft version. Ids of other namespaces aren't checked, so a mod's
ids need `"required": False` when the mod may be missing.
Pass an optional third literal boolean argument to emit tag `replace`; when
multiple declarations are merged, `True` wins over `False`.
Pass-through JSON declarations with identical JSON are accepted once; different
JSON for the same ID is a compile error.
Resource names may use nested paths and explicit namespaces, such as
`other_pack:checks/is_ready`. A name is a string literal: a variable's value
isn't known when the pack is built, so a variable as a name is a compile error.
JSON values are literals, consts, and the `text.*` and `item_component.*`
helpers; write the result of arithmetic as a literal. Numbers keep their
value: a whole number up to 64 bits is written exactly, even past the 32 bits
of a score, a number with a fraction or an exponent (`1e-3`) is written as a
float, and a number JSON can't hold exactly is a compile error. Predicate, advancement, loot table, recipe, and
dialog declarations require object JSON values. An item modifier is an object
or a list of objects. Minecraft 26.3 refuses a list in the file itself, so a
list is written as one `minecraft:sequence`, which applies the modifiers in
order:

```python
datapack.item_modifier("reward", [
    {"type": "minecraft:set_count", "count": 3},
    {"type": "minecraft:set_name", "name": "Reward"}
])
```

```json
{
  "type": "minecraft:sequence",
  "functions": [
    {"type": "minecraft:set_count", "count": 3},
    {"type": "minecraft:set_name", "name": "Reward"}
  ]
}
```

Other JSON resources are written as given, in Minecraft 26.3's formats. Because
a server whose packs fail to load refuses to start, Cobble checks each field
the way Minecraft reads it and reports what it would refuse as
`datapack-resource-format` errors. The same rules apply to a field wherever it
appears, at the top of a resource or nested in another one (a predicate inside
an advancement criterion, an item modifier inside a loot entry, an item
predicate inside an entity predicate):

- a required field must be there, and `None` counts as left out, as it does
  for Minecraft. `None` in a field that may be left out is the field left out;
- a field holds the kind of value Minecraft reads: text, a number, `True` or
  `False` (not `1`), a list, or an object. For example `"rolls": [1]`,
  `"entries": {}`, `"pattern": "#"`, `"count": "2"`, and `"title": 5` are
  errors;
- a number provider (`rolls`, `chance`, `count`, `damage`, `levels`, ...) is a
  number, an object with a `type`, such as
  `{"type": "minecraft:uniform", "min": 1, "max": 3}`, or the id of a provider
  Minecraft defines; `{"min": 1, "max": 3}` without a `type` is an error;
- a `condition`, a `term`, and an advancement condition such as `location` or
  `player` are one predicate or the id of a predicate file, not a list
  (combine several with `minecraft:all_of`). `terms` is a list of them, or one
  id;
- an item modifier field (`modifier`, `functions` of a `minecraft:sequence`,
  `on_pass`) is one item modifier, the id of one, or a list of them.

The fields each type needs:

- predicates need a `type` (formerly `condition`), and `minecraft:all_of` and
  `minecraft:any_of` `terms`, `minecraft:inverted` a `term`,
  `minecraft:random_chance` a `chance`, `minecraft:random_chance_with_enchanted_bonus`
  an `unenchanted_chance`, an `enchanted_chance`, and an `enchantment`,
  `minecraft:entity_properties` an `entity`, `minecraft:entity_scores` an
  `entity` and `scores`, `minecraft:table_bonus` an `enchantment` and a
  non-empty list of `chances`, `minecraft:int_value_check` and
  `minecraft:float_value_check` a `value` and a `test`,
  `minecraft:environment_attribute_check` an `attribute` and a `value`,
  `minecraft:enchantment_active_check` `active`, and `minecraft:time_check` a
  `clock` (new in 26.3) and a `value`;
- item modifiers need a `type` (formerly `function`) and take a single
  `condition` (formerly a `conditions` list), and these types need these
  fields: `apply_bonus` an `enchantment` and a `formula`, `copy_components`
  and `copy_name` a `source`, `copy_custom_data` a `source` and `ops`,
  `copy_state` a `block` and `properties`, `enchant_with_levels` `levels`,
  `enchanted_count_increase` an `enchantment` and a `count`, `exploration_map`
  a `destination`, `fill_player_head` an `entity`, `filtered` an
  `item_filter`, `limit_count` a `limit`, `modify_contents` a `component` and a
  `modifier`, `sequence` `functions`, `set_attributes` `modifiers` (each with
  an `id`, an `attribute`, an `operation`, an `amount`, and a `slot`),
  `set_banner_pattern` `patterns` and `append`, `set_components`
  `components`, `set_contents` a `component` and `entries`, `set_count` a
  `count`, `set_custom_data` a `tag`, `set_damage` `damage`, `set_instrument`
  `options`, `set_item` an `item`, `set_loot_table` a `loot_table_id`,
  `set_lore` a `lore` and a `mode`, `set_ominous_bottle_amplifier` an
  `amplifier`, `set_potion` an `id`, `set_random_dyes` a `number_of_dyes`,
  `set_writable_book_pages` and `set_written_book_pages` `pages` and a `mode`,
  and `toggle_tooltips` `toggles`;
- loot tables use `modifier` (formerly `functions`) and a single `condition`
  (formerly `conditions`) on tables, pools, and entries; a pool needs `rolls`
  and `entries`, and an entry a `type`; a `minecraft:item` entry needs a `name`
  that isn't `minecraft:air` (a pool entry that drops nothing is
  `{"type": "minecraft:empty"}`); a `minecraft:tag` entry names its items in
  `items`, an item id, a `#tag`, or a list of item ids; a
  `minecraft:loot_table` entry names its table in `value` (formerly `name`);
  `minecraft:dynamic` needs a `name` and `minecraft:slots` a `slot_source`;
- an advancement needs at least one criterion, each with a `trigger`;
  `requirements` is a list of lists that names every criterion and no other;
  a `display` needs an `icon`, a `title`, and a `description`, its `frame` is
  `task`, `goal`, or `challenge`, and a root advancement with a `display` sets
  `display.background` (`"display": None` is no display, and
  `"background": None` or `"parent": None` is none);
- each trigger reads its own conditions. Its entity conditions, such as
  `player`, the `entity` of `minecraft:fishing_rod_hooked` or
  `minecraft:player_killed_entity`, and the `child` of
  `minecraft:bred_animals`, are one predicate with a `type` (formerly a list
  of predicates, or an entity predicate on its own), such as
  `{"type": "minecraft:entity_properties", "entity": "this", "predicate": {...}}`.
  `minecraft:recipe_unlocked`, `minecraft:recipe_crafted`, and
  `minecraft:crafter_recipe_crafted` need `recipes`, and
  `minecraft:player_generates_container_loot` needs `loot_tables`. Minecraft
  ignores a condition its trigger doesn't read, such as `entity` on
  `minecraft:tick` or any condition on `minecraft:impossible`, so that's a
  warning;
- an entity predicate is made of the parts Minecraft 26.3 knows
  (`minecraft:entity_type`, `minecraft:location`, `minecraft:equipment`,
  `minecraft:flags`, ...); `"type"` is an error, since the entity type is
  `minecraft:entity_type`. Parts of other namespaces, which a mod can add,
  aren't checked;
- an item stack (a recipe's `result`, a brewing `output`, an advancement's
  `display.icon`) is an item id or an object that names its item in `id`
  (formerly `item`);
- a recipe needs a `type` and the fields of its type: an `ingredient` and a
  `result` for cooking and stonecutting (cooking also a `cookingtime`, as a
  number), `ingredients` for `minecraft:crafting_shapeless`, a `pattern` and a
  `key` for `minecraft:crafting_shaped`, a `base` for
  `minecraft:smithing_transform`, a `template`, a `base`, an `addition`, and
  a `pattern` for `minecraft:smithing_trim`, an `input`, a `reagent` (objects
  that name their item in `item`), and an `output` for `minecraft:brewing`,
  and the ingredients and `result` of every other crafting type;
- recipe ingredients are an item id, a `#tag`, or a non-empty list of item
  ids, without tags (formerly `{"item": ...}` or `{"tag": ...}` objects);
- a shaped recipe's `pattern` has 1 to 3 rows of 1 to 3 symbols, all as wide,
  not all spaces, and its `key` defines each symbol of the pattern, and no
  other, and not the space;
- a shapeless recipe takes 1 to 9 ingredients;
- a recipe's `result` makes 1 to 99 items;
- a recipe's `category` is text and one of its type's: `building`,
  `redstone`, `equipment`, or `misc` for `minecraft:crafting_shaped`,
  `crafting_shapeless`, `crafting_transmute`, `crafting_dye`, and
  `crafting_imbue`; `food`, `blocks`, or `misc` for cooking. Other types don't
  read a category;
- a pack can replace Minecraft's `minecraft:repair_item`, but a second
  `minecraft:crafting_special_repairitem` recipe under another id is an error,
  since Minecraft holds that recipe once;
- every dialog needs a `type` and a `title`. `minecraft:confirmation` also needs
  `yes` and `no`, `minecraft:multi_action` needs `actions`, and
  `minecraft:dialog_list` needs `dialogs`.

Ids that Minecraft looks up when it loads the pack must exist. A data pack
can't add items, blocks, entity types, fluids, potions, effects, or
attributes, so an unknown `minecraft:` id of these is an error wherever it
appears: a loot entry's item, a recipe's ingredients and result, an
advancement's icon, the `items`, `blocks`, `fluids`, and
`minecraft:entity_type` of item, block, fluid, and entity predicates (also the
`blocks` of `minecraft:match_block`: an unknown block there stops the server,
while a `block` field, which it doesn't read, loads and isn't checked), the
`effects` of entity predicates and `minecraft:effects_changed`, and the items,
blocks, potions, effects, and attributes of item modifiers. `minecraft:air`
isn't an item stack or an ingredient. Minecraft's own enchantments,
instruments, biomes, damage types, trim patterns, world clocks, and number
providers are looked up too, along with its biome and enchantment tags. Ids
of other namespaces aren't checked, since a mod can add them.

References to other resources must exist once every file is compiled: a
`#tag` in a tag, a recipe ingredient, a loot `minecraft:tag` entry, or an
item, block, or entity predicate; the dialogs of a dialog tag, a
`minecraft:dialog_list`, and a `minecraft:show_dialog` action; a predicate,
item modifier, or loot table that a resource names by id, in a `condition`, a
`term` or `terms`, a `modifier`, the `functions` of a `minecraft:sequence`, a
`minecraft:loot_table` entry, `minecraft:set_loot_table`, an advancement
criterion's conditions, or an advancement's `rewards.loot`; the recipes of
`minecraft:recipe_unlocked`, `minecraft:recipe_crafted`, and
`minecraft:crafter_recipe_crafted`, and the loot tables of
`minecraft:player_generates_container_loot`, as one id or a list; an
advancement's `parent`; and the functions and function tags of a function
tag. A `minecraft:` id counts if Minecraft or the pack defines it. An id
without a namespace in a function tag means `minecraft:`, so write
`mypack:setup` for the pack's own `setup`. Only `minecraft:` and the pack's own
namespace are checked: another pack or a mod can define ids in any other
namespace, even one the pack declares something in, and a tag entry with
`"required": False` may be missing. A block, item, or entity type tag can't
name a plain id in the pack's own namespace, since a data pack can't add
those. A tag that includes itself through required entries, directly or
through other tags, is an error too: Minecraft drops the tag and every
function that names it. An optional entry that would close the loop is
skipped, and the tags load. An advancement that is its own parent, directly
or through others, is an error, since Minecraft doesn't load advancements in
a loop. `cobble check` checks the pack's own namespace like `cobble build`:
the project's, or the one `--namespace` gives.

Types must exist in Minecraft 26.3, because data packs can't add new ones: the
`type` of a predicate, an item modifier, a loot entry, a recipe, and a dialog,
and the `trigger` of an advancement criterion. For example, `minecraft:set_nbt` is
now `minecraft:set_custom_data`, and `minecraft:alternative` is now
`minecraft:any_of`.

Minecraft silently ignores lists such as `conditions` and `functions`, so
Cobble reports them too.

Item component helpers build the component JSON for `minecraft:set_components`
item modifiers:

```python
from stdlib import datapack, item_component, text

datapack.item_modifier("items/reward_components", {
    "type": "minecraft:set_components",
    "components": item_component.components(
        item_component.custom_name(text.colored("Cobble Reward", "gold")),
        item_component.lore([text.plain("Generated by Cobble")]),
        item_component.unbreakable()
    )
})
```

`item_component.*` helpers return JSON component objects for resource
declarations and emit no commands when used by themselves.

## Checking The Output

Generated resources are listed in the build manifest and the source map,
included in ZIP output, and reported by `cobble inspect --json`:

```bash
cobble check --json
cobble build --validate --zip
cobble inspect output --json
```
