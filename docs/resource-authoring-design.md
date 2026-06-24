# Resource Authoring Design

Status: 0.8.0 implementation contract.

This document defines the 0.8.0 resource-authoring contract. The release
makes existing data-pack resource declarations clearer and more deterministic
before adding broad new resource kinds.

## Release Contract

- Resource output is deterministic across platforms and repeated builds.
- Supported resource kinds have consistent namespace and path validation.
- Each resource kind is classified as either Cobble-owned typed structure or
  pass-through JSON before implementation.
- Typed resources document their schema source and validation behavior.
- Pass-through JSON resources are not silently rewritten.
- Duplicate diagnostics distinguish exact duplicates, merge-compatible
  duplicates, and invalid overwrites.
- Diagnostics include source locations where available.
- Generated resources appear in snapshots, build manifests, and `inspect
  --json`.

## Resource Kind Classification

Each data-pack resource kind Cobble recognizes is classified before
implementation. The classification determines merge, validation, and
diagnostic behavior.

### Typed Resources

Typed resources are owned by Cobble. Cobble validates their structure, merges
duplicate declarations deterministically, and emits canonical JSON.

| Kind | Schema | Merge | Source |
| --- | --- | --- | --- |
| `function_tag` | `{ "values": string[], "replace"?: boolean }` | auto-merge + dedup + sort | Cobble |
| `block_tag` | `{ "values": string[], "replace"?: boolean }` | auto-merge + dedup + sort | Cobble |
| `item_tag` | `{ "values": string[], "replace"?: boolean }` | auto-merge + dedup + sort | Cobble |
| `entity_type_tag` | `{ "values": string[], "replace"?: boolean }` | auto-merge + dedup + sort | Cobble |

Typed tag contract:

- `values` must be an array of string resource IDs in `namespace:path` form.
- Duplicate `values` entries across declarations of the same tag ID are
  removed. String equality is used; object-shaped tag entries (with `id` and
  `required`) are deferred to 0.9 because their equality semantics need a
  separate decision.
- Merged `values` are sorted lexicographically by byte order so repeated
  builds produce identical JSON.
- `replace` is accepted but ignored in 0.8.0. A `replace: true` declaration
  emits a warning explaining that `replace` semantics are deferred; the value
  is still serialized as given so existing packs are not broken. A separate
  `replace` contract is a 0.9 or 1.0 decision.
- The merged JSON object key order is `values` then `replace` (when present),
  emitted deterministically by `serde_json::to_string_pretty`.

### Pass-Through Resources

Pass-through resources are user-authored JSON. Cobble validates only the
namespace/path and the top-level object shape. The JSON content is serialized
as given without rewriting keys, reordering arrays, or merging duplicates.

| Kind | Top-level check | Duplicate behavior |
| --- | --- | --- |
| `predicate` | must be object | exact duplicate ok; overwrite error |
| `advancement` | must be object | exact duplicate ok; overwrite error |
| `loot_table` | must be object | exact duplicate ok; overwrite error |
| `recipe` | must be object | exact duplicate ok; overwrite error |
| `item_modifier` | must be object | exact duplicate ok; overwrite error |
| `dialog` | must be object | exact duplicate ok; overwrite error |

Pass-through contract:

- Two declarations with identical JSON content are accepted as an exact
  duplicate and stored once.
- Two declarations with different JSON content for the same `namespace:path`
  are an error (`invalid-overwrite`).
- Cobble does not merge, sort, or rewrite pass-through JSON.

## Tag Auto-Merge Semantics

When `datapack.function_tag("namespace:path", [...])` is called more than
once for the same `namespace:path`, Cobble merges the `values` arrays.

### Algorithm

1. Parse the existing JSON (if any) and extract its `values` array.
2. Parse the new declaration's `values` array.
3. Append new entries that are not already present (string equality).
4. Sort the combined `values` lexicographically by byte order.
5. Serialize the merged object with `values` first, then `replace` if any
   declaration supplied it.
6. Record every declaration's source location in `json_resource_origins` as
   a `Vec<SourceLocation>` so diagnostics can list all contributors.

### Example

Source:

```python
from stdlib import datapack

datapack.function_tag("my_ns:custom", ["minecraft:stone", "minecraft:dirt"])
datapack.function_tag("my_ns:custom", ["minecraft:dirt", "minecraft:oak_log"])
```

Generated `data/my_ns/tags/function/custom.json`:

```json
{
  "values": [
    "minecraft:dirt",
    "minecraft:oak_log",
    "minecraft:stone"
  ]
}
```

### Cross-Namespace Behavior

Tag merging is scoped to the same `namespace:path` ID. Two declarations with
different namespaces or paths are independent resources and are never merged.

### `replace` Handling

If any declaration supplies `"replace": true`, the merged output includes
`"replace": true` and a warning is emitted:

```
warning: tag 'my_ns:custom' declared with replace=true.
  replace semantics are deferred in 0.8.0; the value is serialized as given
  but does not change merge behavior.
```

If declarations disagree on `replace`, the `true` value wins and the warning
names the first `replace: true` declaration.

## Path Validation And Suggestions

`plain_resource_id_to_parts` validates resource IDs. 0.8.0 adds suggestions
for common mistakes:

- `minecraft/stone` (slash instead of colon) suggests `minecraft:stone`.
- `MyNamespace:path` (uppercase namespace) reports the uppercase character
  position and suggests the lowercase form.
- `namespace:` (empty path) reports the empty path segment.
- `:path` (empty namespace) reports the empty namespace.

Suggestions use prefix matching against known namespaces in the current build
(`minecraft`, the project namespace, and any namespace seen in prior
declarations). Levenshtein distance is not used in 0.8.0 to keep the
diagnostic path simple and dependency-free.

## Duplicate Diagnostics

`json_resource_duplicate_kind` classifies duplicates:

| Classification | Condition | Behavior |
| --- | --- | --- |
| `exact duplicate` | same JSON content | accepted, stored once |
| `merge-compatible duplicate` | tags/ path, typed tag | merged, no error |
| `invalid overwrite` | different JSON, pass-through | error |
| `invalid duplicate tag declaration` | tags/ path, schema violation | error |

Diagnostics include the first and second declaration source locations using
`format_source_location`, which renders `file:line:column` relative to the
source display root. For merge-compatible duplicates, a warning (not error)
names both contributors so users can see where values came from.

## Resource Output Contract

### Ordering

- JSON object keys that Cobble owns are emitted deterministically.
  `serde_json::to_string_pretty` preserves insertion order for
  `serde_json::Map` unless `preserve_order` is disabled; Cobble constructs
  maps in the documented key order.
- Arrays generated by Cobble preserve source order unless the helper contract
  explicitly sorts them. Tag `values` are sorted after merge.
- Merged tag values have deterministic ordering and duplicate handling.

### Paths

- Resource names must use safe Minecraft namespace and path syntax.
- Absolute paths, parent-directory traversal, platform separators, and empty
  path segments are invalid.
- Diagnostics suggest the likely valid namespace/path form when the mistake
  is obvious.

### Validation

- Typed resources need positive and negative tests for schema validation.
- Unsupported schema features fail with source-aware diagnostics instead of
  being partially rewritten.
- Validation behavior is the same in CLI and WASM for supported single-file
  resources.

## Build Manifest Impact

`generated` counts are unchanged in shape. The existing `function_tags`,
`custom_function_tags`, and `json_function_tags` fields already account for
merged tags because they count resource entries, not declaration calls.

`resources` entries for merged tags appear once per `namespace:path` with
`kind` set to `function_tag`/`block_tag`/`item_tag`/`entity_type_tag`.

`json_resource_origins` changes from `HashMap<String, SourceLocation>` to
`HashMap<String, Vec<SourceLocation>>` to track every contributor to a
merged tag. This is an internal field, not serialized to the manifest, so no
schema bump is required.

## Resource-Pack Boundary

Resource-pack support is experimental in 0.8.0 and documented separately in
`resource-pack-design.md`. It does not affect data-pack resource authoring.

## Implementation Checklist

- Classify each resource kind as typed or pass-through (table above).
- Extend `add_json_resource_in_namespace_with_source` to branch on `tags/`
  paths for the typed merge path.
- Add `merge_tag_resource` for auto-merge + dedup + sort.
- Change `json_resource_origins` to `Vec<SourceLocation>`.
- Add `validate_tag` schema validation for typed tags.
- Extend `json_resource_duplicate_kind` with `merge-compatible duplicate`.
- Add path suggestions in `plain_resource_id_to_parts` for slash/uppercase
  mistakes.
- Add `replace` warning emission for typed tags.
- Add generated JSON snapshots for every changed kind.
- Add positive and negative diagnostics tests for invalid names and
  duplicate declarations.
- Add manifest and `inspect --json` assertions for generated resources.
- Add examples that build and validate when command data is available.
