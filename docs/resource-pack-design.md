# Resource Pack Design

Status: 0.9.0 beta implementation contract with experimental opt-in.

This document defines the beta resource-pack support for 0.9.0. Resource-pack
output still ships behind an explicit opt-in flag so the data-pack workflow
stays unaffected for users who do not need assets, but the CLI workflow, path
safety rules, and ZIP behavior are treated as release-gated behavior.

## Goals

- Let a single `cobble build` produce a unified pack containing both
  `data/` and `assets/` for Minecraft 1.20+ combined packs.
- Generate model and language JSON from Cobble source so authors can keep
  asset declarations alongside their data-pack logic.
- Copy static resource-pack assets from the project `assets/` tree for beta
  projects that already own textures, sounds, and other binary files.
- Keep the feature opt-in and labeled experimental so 1.0 can reserve the
  right to change the helper surface.

## Non-Goals

- Texture, audio, or other binary asset generation.
- External asset processors or pipeline plugins.
- Resource-pack-only output (no data pack).
- Full Minecraft model or language JSON schema validation. Cobble performs
  beta-level type checks for common model fields and lang entries, but full
  schema validation is deferred until a per-kind schema source and snapshot
  contract are accepted.
- Automatic asset deduplication or merging across declarations (model and
  language JSON are pass-through; duplicates are errors, like data-pack
  pass-through resources).

## Opt-In

Resource-pack output is off by default. It is enabled by either:

```bash
cobble build --experimental-resource-pack
```

or in `cobble.toml`:

```toml
[experimental]
resource_pack = true
```

When the flag is absent and `[experimental] resource_pack` is not `true`,
any `resource_pack.*` helper call emits:

```
error: resource_pack.* requires --experimental-resource-pack or [experimental] resource_pack = true.
  Source: src/main.cbl:4:5
```

The web compiler (`/try`) can enable resource-pack output through its explicit
experimental toggle and materializes generated model/lang assets in memory for
download. It still has no project filesystem access, so native static
`assets/` passthrough is CLI-only.

## Output Structure

A unified pack writes both `data/` and `assets/` under the configured
output directory:

```
output/
├── pack.mcmeta
├── data/
│   └── <namespace>/
│       ├── function/
│       ├── tags/
│       └── ...
└── assets/
    └── <namespace>/
        ├── models/
        │   ├── item/<name>.json
        │   └── block/<name>.json
        ├── lang/
        │   └── <locale>.json
        └── textures/
            └── ...
```

`pack.mcmeta` is unchanged. Minecraft 1.20+ accepts a single pack containing
both `data/` and `assets/` as long as `pack_format` is set. Cobble uses the
same `pack_format` (101.1) for both halves.

When `--experimental-resource-pack` is not enabled, `assets/` is not created
by Cobble and the output is identical to a data-pack-only build.

## Static Asset Passthrough

When resource-pack output is enabled, native CLI builds copy static files from
the project `assets/` directory into the output `assets/` directory. For a
configured project this means `assets/` next to `cobble.toml`; for direct
file or directory builds without a config file, Cobble uses the inferred
source root.

Files are copied byte-for-byte. Cobble does not parse or rewrite textures,
audio, font, atlas, blockstate, or other static asset formats. Static asset
paths must be shaped like `assets/<namespace>/<path>`, use lowercase
Minecraft-safe path segments, stay under the project `assets/` directory, and
must not be symlinks. Cobble walks with symlink following disabled and refuses
any symlink path it finds.

Static passthrough runs after generated `resource_pack.*` JSON is written.
A static file that would land on the same output path as a generated
resource-pack asset is an error instead of an overwrite.

## Helper API

Resource-pack helpers live under the `resource_pack` stdlib module (see
`stdlib-v2-design.md` for the module list). The module is only active when
resource-pack output is enabled.

### `resource_pack.item_model`

```python
from stdlib import resource_pack

resource_pack.item_model("my_ns:custom_sword", {
    "parent": "minecraft:item/handheld",
    "textures": {
        "layer0": "my_ns:item/custom_sword"
    }
})
```

Generates `assets/my_ns/models/item/custom_sword.json` with the given JSON
content. The resource ID uses `namespace:path` form; the path is split into
`models/item/<path>.json`.

### `resource_pack.block_model`

```python
resource_pack.block_model("my_ns:custom_block", {
    "parent": "minecraft:block/cube_all",
    "textures": {
        "all": "my_ns:block/custom_block"
    }
})
```

Generates `assets/my_ns/models/block/<path>.json`.

### `resource_pack.lang`

```python
resource_pack.lang("en_us", {
    "item.my_ns.custom_sword": "Custom Sword",
    "block.my_ns.custom_block": "Custom Block"
})
```

Generates `assets/<namespace>/lang/<locale>.json`. The namespace defaults to
the project namespace when the locale string does not contain a colon. A
resource ID form `my_ns:en_us` is also accepted and overrides the namespace.

The lang JSON is a flat object mapping translation keys to string values.
Multiple `resource_pack.lang` calls for the same namespace and locale are
merged deterministically. Repeated translation keys with the same value are
accepted; repeated keys with different values are an `invalid-overwrite`
error.

## Validation

### Resource ID

`resource_pack.*` helpers use the same `plain_resource_id_to_parts`
validation as data-pack resources. Namespace and path rules are identical:
lowercase letters, digits, `_`, `-`, `.`, and `/` for path segments; no
empty, `.`, or `..` segments.

### JSON Content

Model JSON is pass-through for unknown fields, but Cobble validates the
top-level object and the type of common model fields when present: `parent`,
`gui_light`, and `credit` must be strings; `display` and `textures` must be
objects; `elements` and `overrides` must be arrays; texture values must be
strings. The content is then serialized with `serde_json::to_string_pretty`
without rewriting keys or reordering arrays. Lang JSON is validated as an
object with string values, then normalized by translation key for deterministic
merging.

### Filesystem Safety

`assets/` output paths are validated with the same `is_safe_namespace_path`
and `is_safe_resource_path` checks used for `data/`. Symlink refusal in
`output_safety.rs` applies to the entire output directory including
`assets/`. Static asset passthrough additionally rejects symlinks in the
project `assets/` tree, verifies source containment under the project root,
and rejects traversal-style relative paths before copying.

### Duplicate Resources

Two declarations for the same `namespace:path` model with identical JSON are
accepted as an exact duplicate. Two model declarations with different JSON are
an `invalid-overwrite` error. Language declarations for the same namespace and
locale are merged by translation key: identical repeated keys are accepted,
new keys are added, and conflicting values are an `invalid-overwrite` error.

## Build Manifest Impact

New optional `generated` fields:

| Field | Type | Meaning |
| --- | --- | --- |
| `resource_pack_models` | number | Generated model JSON count. |
| `resource_pack_langs` | number | Generated lang JSON count. |
| `resource_pack_static_assets` | number | Static assets copied from project `assets/`. |

New top-level optional field:

| Field | Type | Meaning |
| --- | --- | --- |
| `experimental_features` | array | List of experimental features enabled, e.g. `["resource_pack"]`. |

These fields are additive and do not require a schema version bump. They
are omitted (or empty) when resource-pack output is not enabled.

`resources` entries for resource-pack assets use `kind` values
`resource_pack_model`, `resource_pack_lang`, and
`resource_pack_static_asset`.

Static passthrough assets are counted in `generated.resource_pack_static_assets`,
listed in `resources` as `resource_pack_static_asset`, and surfaced by
`cobble inspect` through the manifest summary. Generated resource-pack JSON
files are also represented in `.cobble/source_map.json` as `JsonGenerated`
entries. Copied static passthrough assets are not source-mapped because Cobble
does not generate their contents.

## ZIP Output

When `--zip` is used with `--experimental-resource-pack`, the ZIP archive
includes `data/`, copied/generated `assets/`, and `pack.mcmeta`.

## Inspect

`cobble inspect` and `cobble inspect --json` report resource-pack assets
alongside data-pack resources. The generated counts include
`resource_pack_models`, `resource_pack_langs`, and
`resource_pack_static_assets` when non-zero.

## Diagnostics

| Code | Severity | Condition |
| --- | --- | --- |
| `resource-pack-not-enabled` | error | `resource_pack.*` called without opt-in |
| `resource-pack-bad-id` | error | invalid namespace:path |
| `resource-pack-non-object` | error | JSON value is not an object |
| `resource-pack-duplicate` | error | same path, different JSON |

## Stability

Resource-pack support is beta-gated in 0.9.0:

- The helper names (`resource_pack.item_model`, `resource_pack.block_model`,
  `resource_pack.lang`) may change before 1.0.
- The `[experimental] resource_pack` config key may be renamed or moved.
- The `--experimental-resource-pack` flag may be renamed before 1.0.
- Generated `assets/` structure follows Minecraft conventions and is
  expected to remain stable, but Cobble reserves the right to add
  validation or change path handling.

Users who rely on this feature should pin to the 0.9 minor line and watch the
changelog for beta changes.

## Implementation Checklist

- [x] Add `experimental_resource_pack: bool` to `BuildOptions` and CLI.
- [x] Add `[experimental] resource_pack` to `CobbleConfig`.
- [x] Add resource-pack model/lang stores to `DataPack`.
- [x] Add `process_resource_pack_intrinsic` to `Transpiler`.
- [x] Add `resource_pack` to the stdlib module list.
- [x] Gate `resource_pack.*` calls on both module activation and the
  experimental flag.
- [x] Extend `DataPack::write` to emit `assets/` when enabled.
- [x] Extend `create_zip` and the web ZIP builder to include `assets/`
  entries.
- [x] Add `resource_pack_models`, `resource_pack_langs`,
  `resource_pack_static_assets`, and `experimental_features` to the build
  manifest.
- [x] Add path and JSON validation for resource-pack assets.
- [x] Add beta model-field and lang-entry validation.
- [x] Add unit tests for opt-in, JSON pass-through, manifest metadata, and ZIP
  inclusion.
- [x] Add `examples/resource_pack` with source.
- [x] Document the feature in `docs/cli.md` and `docs/language.md`.
- [x] Add bounded static `assets/` passthrough for native CLI builds.
- [x] Add manifest and inspect entries for static passthrough asset counts.
