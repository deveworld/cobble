# Resource Packs (Experimental)

Cobble can also write resource-pack assets next to the data pack. The feature
is experimental and off by default. Enable it with
`cobble build --experimental-resource-pack` or in `cobble.toml`:

```toml
[experimental]
resource_pack = true
```

Then import the `resource_pack` module.

## Models And Language Files

```python
from stdlib import resource_pack

resource_pack.item_model("mypack:custom_sword", {
    "parent": "minecraft:item/handheld",
    "textures": {
        "layer0": "mypack:item/custom_sword"
    }
})

resource_pack.block_model("display_block", {
    "parent": "minecraft:block/cube_all",
    "textures": {
        "all": "mypack:block/display_block"
    }
})

resource_pack.lang("en_us", {
    "item.mypack.custom_sword": "Custom Sword",
    "block.mypack.display_block": "Display Block"
})
```

`resource_pack.item_model` writes `assets/<namespace>/models/item/<path>.json`,
`resource_pack.block_model` writes
`assets/<namespace>/models/block/<path>.json`, and `resource_pack.lang` writes
`assets/<namespace>/lang/<locale>.json`. The same lowercase namespace/path
validation used by `datapack.*` applies here. JSON must be an object, and lang
values must be strings. Multiple lang declarations for the same namespace and
locale merge when repeated keys have identical values; conflicting values are
an error.

## Static Assets

When resource-pack output is enabled, Cobble also copies static files from the
project `assets/` directory into the generated pack. Use
`assets/<namespace>/...` for textures, sounds, blockstates, and other
resource-pack files:

```text
assets/mypack/textures/item/custom_sword.png
assets/mypack/models/item/generated_wand.json
```

Static asset paths must stay inside the project `assets/` tree, use lowercase
Minecraft-safe path segments, and must not be symlinks. A validated build
stages generated and static assets before it replaces the final output.

## Diagnostics

Resource-pack helpers report these diagnostic kinds:

| Kind | Reported when |
| --- | --- |
| `resource-pack-not-enabled` | a `resource_pack.*` helper is called without the opt-in |
| `resource-pack-bad-id` | a resource ID or locale name is malformed |
| `resource-pack-non-object` | a model or lang payload is not a JSON object |
| `resource-pack-duplicate` | two declarations write the same path with different JSON |

## Checking The Output

ZIP output includes `assets/`, and `cobble inspect --json` lists generated
models, lang files, and copied assets:

```bash
cobble build --experimental-resource-pack --validate --zip
cobble inspect output --json
```
