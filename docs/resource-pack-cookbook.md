# Resource Pack Cookbook

Status: 0.9.0 beta cookbook.

Resource-pack authoring is still behind the explicit 0.9 opt-in:

```bash
cobble build --experimental-resource-pack --zip
```

or:

```toml
[experimental]
resource_pack = true
```

## Generate Models And Language Entries

```python
from stdlib import resource_pack

resource_pack.item_model("my_pack:wand", {
    "parent": "minecraft:item/handheld",
    "textures": {"layer0": "my_pack:item/wand"},
})

resource_pack.lang("en_us", {
    "item.my_pack.wand": "Cobble Wand",
})
```

Generated assets are written under `assets/<namespace>/...`, included in ZIP
output, and reported by `cobble inspect --json`.

## Pass Through Static Assets

Place static files under project `assets/`:

```text
assets/my_pack/textures/item/wand.png
assets/my_pack/models/item/generated_wand.json
```

Cobble copies these files into the output only when resource-pack support is
enabled. Validated builds stage generated and static assets before replacing
the final output.

## Review Commands

```bash
cobble build --experimental-resource-pack --validate --zip
cobble inspect output --json
```

Static asset paths are checked for containment and symlink safety before copy.
