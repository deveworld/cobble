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
  `any_of`, and `inverted` terms;
- item modifiers need a `type` (formerly `function`) and take a single
  `condition` (formerly a `conditions` list);
- loot tables use `modifier` (formerly `functions`) and a single `condition`
  (formerly `conditions`) on tables, pools, and entries, and a
  `minecraft:tag` entry names its tag in `items` (`"#minecraft:logs"`);
- a root advancement with `display` sets `display.background`;
- an advancement criterion's `player` and `entity` conditions are one
  predicate with a `type` (formerly a list of predicates, or an entity
  predicate on its own), such as `{"type": "minecraft:entity_properties",
  "entity": "this", "predicate": {...}}`;
- an advancement's `display.icon` and a recipe's `result` name the item in `id`
  (formerly `item`);
- recipe ingredients are an item id, a `#tag`, or a list of item ids (formerly
  `{"item": ...}` or `{"tag": ...}` objects), except the `input` of a
  `minecraft:brewing` recipe, which is an object that names a potion;
- cooking recipes set `cookingtime`;
- every dialog needs a `type` and a `title`. `minecraft:confirmation` also needs
  `yes` and `no`, `minecraft:multi_action` needs `actions`, and
  `minecraft:dialog_list` needs `dialogs`.

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
