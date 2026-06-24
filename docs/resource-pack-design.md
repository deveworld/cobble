# Resource Pack Design

Status: 0.8.0 experimental implementation contract.

This document defines the experimental resource-pack support added in
0.8.0. Resource packs are not part of the stable 0.8 contract; they ship
behind an explicit opt-in flag so the data-pack workflow stays unaffected
for users who do not need assets.

## Goals

- Let a single `cobble build` produce a unified pack containing both
  `data/` and `assets/` for Minecraft 1.20+ combined packs.
- Generate model and language JSON from Cobble source so authors can keep
  asset declarations alongside their data-pack logic.
- Keep the feature opt-in and labeled experimental so 1.0 can reserve the
  right to change the helper surface.

## Non-Goals

- Texture, audio, or other binary asset generation.
- External asset processors or pipeline plugins.
- Resource-pack-only output (no data pack).
- Model or language JSON schema validation beyond the top-level object
  check. Full schema validation is deferred until a per-kind schema source
  and snapshot contract are accepted.
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

The web compiler (`/try`) never enables resource-pack output because it has
no filesystem access. The same `resource_pack.*` call in `/try` emits the
same error so users see the limitation before trying it locally.

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
        └── lang/
            └── <locale>.json
```

`pack.mcmeta` is unchanged. Minecraft 1.20+ accepts a single pack containing
both `data/` and `assets/` as long as `pack_format` is set. Cobble uses the
same `pack_format` (101.1) for both halves.

When `--experimental-resource-pack` is not enabled, `assets/` is not created
and the output is identical to a data-pack-only build.

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
Cobble does not merge multiple `resource_pack.lang` calls for the same
locale; the second call is an `invalid-overwrite` error (matching the
pass-through resource contract in `resource-authoring-design.md`).

## Validation

### Resource ID

`resource_pack.*` helpers use the same `plain_resource_id_to_parts`
validation as data-pack resources. Namespace and path rules are identical:
lowercase letters, digits, `_`, `-`, `.`, and `/` for path segments; no
empty, `.`, or `..` segments.

### JSON Content

Model and lang JSON are pass-through. Cobble validates only that the
top-level value is an object (JSON object). The content is serialized with
`serde_json::to_string_pretty` without rewriting keys or reordering arrays.

### Filesystem Safety

`assets/` output paths are validated with the same `is_safe_namespace_path`
and `is_safe_resource_path` checks used for `data/`. Symlink refusal in
`output_safety.rs` applies to the entire output directory including
`assets/`.

### Duplicate Resources

Two declarations for the same `namespace:path` model or lang locale with
identical JSON are accepted as an exact duplicate. Two declarations with
different JSON are an `invalid-overwrite` error. This matches the
pass-through data-pack resource contract.

## Build Manifest Impact

New optional `generated` fields:

| Field | Type | Meaning |
| --- | --- | --- |
| `resource_pack_models` | number | Generated model JSON count. |
| `resource_pack_langs` | number | Generated lang JSON count. |

New top-level optional field:

| Field | Type | Meaning |
| --- | --- | --- |
| `experimental_features` | array | List of experimental features enabled, e.g. `["resource_pack"]`. |

These fields are additive and do not require a schema version bump. They
are omitted (or empty) when resource-pack output is not enabled.

`resources` entries for resource-pack assets use `kind` values
`resource_pack_model` and `resource_pack_lang`.

## ZIP Output

When `--zip` is used with `--experimental-resource-pack`, the ZIP archive
includes both `data/` and `assets/` entries plus `pack.mcmeta`. The
existing `create_zip` filter (which currently keeps only `pack.mcmeta` and
`data/`) is extended to also keep `assets/`.

## Inspect

`cobble inspect` and `cobble inspect --json` report resource-pack assets
alongside data-pack resources. The generated counts include
`resource_pack_models` and `resource_pack_langs` when non-zero.

## Diagnostics

| Code | Severity | Condition |
| --- | --- | --- |
| `resource-pack-not-enabled` | error | `resource_pack.*` called without opt-in |
| `resource-pack-bad-id` | error | invalid namespace:path |
| `resource-pack-non-object` | error | JSON value is not an object |
| `resource-pack-duplicate` | error | same path, different JSON |

## Stability

Resource-pack support is experimental in 0.8.0:

- The helper names (`resource_pack.item_model`, `resource_pack.block_model`,
  `resource_pack.lang`) may change in 0.9 or 1.0.
- The `[experimental] resource_pack` config key may be renamed or moved.
- The `--experimental-resource-pack` flag may be renamed.
- Generated `assets/` structure follows Minecraft conventions and is
  expected to remain stable, but Cobble reserves the right to add
  validation or change path handling.

Users who rely on this feature should pin to `cobble-lang 0.8.x` and watch
the changelog for 0.9 changes.

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
- [x] Add `resource_pack_models`, `resource_pack_langs`, and
  `experimental_features` to the build manifest.
- [x] Add path and JSON validation for resource-pack assets.
- [x] Add unit tests for opt-in, JSON pass-through, manifest metadata, and ZIP
  inclusion.
- [x] Add `examples/resource_pack` with source.
- [x] Document the feature in `docs/cli.md` and `docs/language.md`.
