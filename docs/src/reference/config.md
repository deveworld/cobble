# Project Configuration

`cobble init` writes a `cobble.toml` at the project root:

```toml
[project]
name = "my_datapack"
description = "my_datapack Data Pack"
namespace = "my_datapack"
version = "1.0.0"
pack_format = "121.0"

[build]
source = "src"
output = "output"
entry_points = []

[stdlib]
version = 2

[experimental]
resource_pack = false
plugins = false
python_compat = false
```

| Key | Meaning |
| --- | --- |
| `project.name` | Project name. |
| `project.description` | Pack description written to `pack.mcmeta`. |
| `project.namespace` | Namespace for functions, resources, the `<namespace>.vars` objective, and the `<namespace>:global` storage. Lowercase, no spaces. |
| `project.version` | Your project's version; Cobble does not use it. |
| `project.pack_format` | Must be `121.0` (or `121`). |
| `build.source` | Source directory. Default `src`. |
| `build.output` | Output directory. Default `output`. |
| `build.entry_points` | Files to compile, relative to `build.source`. Imports are resolved from these files. Empty means every file in the source directory. |
| `stdlib.version` | `2` (default) requires per-module stdlib imports; `1` is deprecated. See [Standard Library Modules](../stdlib/modules.md). |
| `experimental.resource_pack` | Enables [resource-pack output](../stdlib/resource-packs.md). |
| `experimental.plugins` | Enables the plugin diagnostics in `cobble check`. See [Experimental Features](experimental.md). |
| `experimental.python_compat` | Adds the Python compatibility report to `cobble check`. |

Unknown keys, misplaced keys, and experimental flags that are not booleans are
errors. Command-line options such as `--namespace` and `--output` override the
file for one build.

## Supported Minecraft Version

| Minecraft | Pack format |
| --- | --- |
| Java Edition 26.3 | 121.0 |

Cobble targets one Minecraft version at a time and rejects other pack formats.
`pack.mcmeta` gets `min_format` and `max_format` set to `[121, 0]`.

## Output Layout

```text
output/
├── pack.mcmeta
├── .cobble/
│   ├── build_manifest.json
│   └── source_map.json
└── data/
    ├── minecraft/
    │   └── tags/
    │       └── function/
    │           ├── load.json
    │           └── tick.json
    └── my_datapack/
        └── function/
            ├── _cobble_init.mcfunction
            ├── on_load.mcfunction
            └── ...
```

Resource declarations add folders such as `data/<namespace>/predicate/`, and
resource-pack output adds `assets/`. The `.cobble/` files are described in
[Metadata Files](metadata.md).
