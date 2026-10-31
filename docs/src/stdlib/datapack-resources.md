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
a server whose packs fail to load refuses to start, Cobble reports the common
pre-26.3 shapes and missing fields as `datapack-resource-format` errors:

- predicates need a `type` (formerly `condition`), including inside `all_of`,
  `any_of`, and `inverted` terms, and the fields of their type:
  `minecraft:random_chance` a `chance`, `minecraft:entity_properties` an
  `entity`, `minecraft:entity_scores` an `entity` and `scores`,
  `minecraft:table_bonus` an `enchantment` and `chances`, `all_of` and `any_of`
  `terms`, `inverted` a `term`, `minecraft:int_value_check` a `value` and a
  `test`, `minecraft:enchantment_active_check` `active`, and
  `minecraft:time_check` a `clock` (new in 26.3) and a `value`;
- a `condition` is one predicate or the id of a predicate file, not a list
  (combine several with `minecraft:all_of`);
- item modifiers need a `type` (formerly `function`) and take a single
  `condition` (formerly a `conditions` list), also inside a
  `minecraft:sequence`, and the fields of their type: `minecraft:set_count` a
  `count`, `minecraft:set_lore` a `lore` and a `mode`,
  `minecraft:set_components` `components`, `minecraft:set_custom_data` a `tag`,
  `minecraft:sequence` `functions`, `minecraft:set_damage` `damage`,
  `minecraft:set_item` an `item`, `minecraft:set_potion` an `id`,
  `minecraft:enchant_with_levels` `levels`, `minecraft:limit_count` a `limit`,
  `minecraft:copy_name` a `source`, `minecraft:toggle_tooltips` `toggles`, and
  `minecraft:apply_bonus` an `enchantment` and a `formula`;
- loot tables use `modifier` (formerly `functions`) and a single `condition`
  (formerly `conditions`) on tables, pools, and entries; a pool needs `rolls`
  and `entries`, and an entry a `type`; a `minecraft:item` entry needs a `name`
  that isn't `minecraft:air` (a pool entry that drops nothing is
  `{"type": "minecraft:empty"}`); a `minecraft:tag` entry names its tag in
  `items`, with its `#` (`"#minecraft:logs"`); and a `minecraft:loot_table`
  entry names its table in `value` (formerly `name`);
- an advancement needs at least one criterion, each with a `trigger`;
  `requirements` is a list of lists that names every criterion and no other;
  a `display` needs an `icon`, a `title`, and a `description`, its `frame` is
  `task`, `goal`, or `challenge`, and a root advancement with a `display` sets
  `display.background` (`"display": None` is no display);
- an advancement criterion's `player` condition, and its `entity` condition on
  the triggers that have one, are one predicate with a `type` (formerly a list
  of predicates, or an entity predicate on its own), such as
  `{"type": "minecraft:entity_properties", "entity": "this", "predicate": {...}}`.
  Minecraft ignores `entity` on the other triggers, such as `minecraft:tick`,
  so it's a warning there;
- an advancement's `display.icon` and a recipe's `result` name the item in `id`
  (formerly `item`);
- a recipe needs a `type`; recipe ingredients are an item id, a `#tag`, or a
  non-empty list of item ids, without tags (formerly `{"item": ...}` or
  `{"tag": ...}` objects), in every ingredient field of the type, except the
  `input` and `reagent` of a `minecraft:brewing` recipe, which are objects;
- a shaped recipe's `pattern` has 1 to 3 rows of 1 to 3 symbols, all as wide,
  and its `key` defines each symbol of the pattern, and no other, and not the
  space;
- a shapeless recipe takes 1 to 9 ingredients;
- crafting, cooking, stonecutting, and smithing transform recipes, and
  `minecraft:crafting_dye`, need a `result`; the result of the first four
  names its item in `id`, which isn't `minecraft:air`, and makes 1 to 99 items
  (a `minecraft:crafting_transmute` result may leave out its id);
- a recipe's `category` is one of its type's (`building`, `redstone`,
  `equipment`, `misc` for crafting; `food`, `blocks`, `misc` for cooking);
- cooking recipes set `cookingtime`, as a number;
- every dialog needs a `type` and a `title`. `minecraft:confirmation` also needs
  `yes` and `no`, `minecraft:multi_action` needs `actions`, and
  `minecraft:dialog_list` needs `dialogs`.

Ids that Minecraft looks up when it loads the pack must exist. A data pack
can't add items, blocks, entity types, or effects, so an unknown `minecraft:`
id of these is an error: a loot entry's item, a recipe's ingredients and
result, an advancement's icon, and the `items`, `blocks`, and
`minecraft:entity_type` lists and the `effects` of item, location, and entity
predicates. The enchantments of `minecraft:enchant_randomly` are checked too.
A `minecraft:match_block` predicate with an unknown block loads, so its block
isn't checked.

References to other resources must exist once every file is compiled: a
`#tag` in a tag, a recipe ingredient, a loot `minecraft:tag` entry, or a
predicate; the dialogs of a dialog tag, a `minecraft:dialog_list`, and a
`minecraft:show_dialog` action; a predicate, item modifier, or loot table that
a loot table names by id; the loot tables of an advancement's `rewards.loot`;
the recipes of `minecraft:recipe_unlocked`; an advancement's `parent`; and the
functions and function tags of a function tag. A `minecraft:` id counts if
Minecraft or the pack defines it. An id without a namespace in a function tag
means `minecraft:`, so write `mypack:setup` for the pack's own `setup`. Ids of
namespaces the pack declares nothing in aren't checked, since another pack or
a mod can define them, and a tag entry with `"required": False` may be
missing. A tag that includes itself, directly or through other tags, is an
error too: Minecraft drops the tag and every function that names it.

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
