# Resource Authoring Cookbook

Status: 0.9.0 cookbook.

This cookbook shows small, copyable patterns for generated data-pack resources.
Run `cobble build --validate` after adapting examples to your project.

## Merge Function Tags

```python
from stdlib import datapack

datapack.function_tag("minecraft:load", ["my_pack:setup"])
datapack.function_tag("minecraft:load", ["my_pack:extra_setup"])
```

Cobble writes one deterministic
`data/minecraft/tags/function/load.json` file with sorted merged values.

## Optional Tag Entries

```python
from stdlib import datapack

datapack.item_tag("rewards", [
    "minecraft:diamond",
    {"id": "minecraft:netherite_ingot", "required": False},
])
```

String and object-shaped tag entries can be mixed. Duplicate entries are
normalized before writing.

## JSON Resources

```python
from stdlib import datapack

datapack.predicate("always", {
    "condition": "minecraft:random_chance",
    "chance": 1,
})

datapack.loot_table("empty", {"type": "minecraft:empty"})
```

Generated JSON resources are included in the build manifest, source map, ZIP
output, and `cobble inspect --json`.

## Review Commands

```bash
cobble check --json
cobble build --validate --zip
cobble inspect output --json
```
