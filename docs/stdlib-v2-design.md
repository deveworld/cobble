# Stdlib V2 Design

Status: 0.8.0 implementation contract. Supersedes the 0.8.0-alpha.0 prep notes.

This document defines the standard-library contract for the 0.8.0 line. The
goal is to add authoring power without hiding generated Minecraft commands,
resource files, objectives, storage keys, tags, or helper functions.

## Release Contract

- Helpers are thin expansions over commands or data-pack JSON.
- Each accepted helper documents its Cobble source and generated output.
- Helper output is deterministic across platforms and repeated builds.
- Helper-generated identifiers are stable and documented.
- Helpers do not create implicit load/tick tags, temporary objectives, storage
  keys, or helper functions unless source explicitly opts in.
- Helpers that emit commands must have validation coverage when the bundled
  command tree can validate them.
- Helper output must be visible in generated file snapshots, build manifests,
  source maps where applicable, and `inspect --json`.

## Import And Versioning Model

0.8.0 introduces opt-in module imports while preserving the v1 `import stdlib`
form for backward compatibility. The helper call surface (names like
`text.tellraw()`, `score.set()`) is unchanged; only the import gating changes.

### Configuration

`cobble.toml` gains an optional `[stdlib]` section:

```toml
[stdlib]
version = 2  # default. Set to 1 to opt into v1 behavior.
```

- `version = 2` (default): per-module opt-in via `from stdlib import ...`.
  `import stdlib` still activates every module for compatibility.
- `version = 1`: every module is always active, matching 0.7.x behavior. A
  deprecation warning is emitted at build time directing users to migrate.

The `version` field does not change helper semantics or generated output. It
only changes whether calling a helper without importing its module is an
error or accepted.

### Import Forms

| Import | Effect |
| --- | --- |
| `import stdlib` | Activates all stdlib modules. Matches 0.7.x behavior. |
| `from stdlib import text` | Activates only the `text` module. |
| `from stdlib import text, score` | Activates `text` and `score`. |
| `from stdlib import event` | Activates only `event` (for `addEventListener`). |
| No stdlib import | No stdlib module is active. Any `stdlib.*` call is an error. |

### Module List

The importable module names are:

- `text`
- `score`
- `score.objective` (imported as `score.objective`; `from stdlib import score`
  does not automatically activate `score.objective`)
- `random`
- `timer`
- `storage`
- `schedule`
- `bossbar`
- `team`
- `entity`
- `math`
- `event`
- `datapack`
- `resource_pack` (experimental; see `resource-pack-design.md`)

### Gating Behavior

When a helper is called whose module is not active, Cobble emits diagnostic
`stdlib-module-not-imported`:

```
error: module 'text' not imported.
  Add `from stdlib import text` or use `import stdlib` to enable all modules.
  Source: src/main.cbl:3:5
```

This diagnostic is an error in `version = 2`. In `version = 1`, it is never
emitted because all modules are always active.

### Version 1 Deprecation

When `[stdlib] version = 1` is set, Cobble emits a build-time warning:

```
warning: [stdlib] version = 1 is deprecated.
  Migrate to version = 2 and use `from stdlib import ...` for per-module opt-in.
  See docs/stdlib-v2-design.md for the migration guide.
```

The warning does not fail the build. It is emitted once per build.

### Migration Path

Existing projects using `import stdlib` continue to work without changes
because `import stdlib` activates all modules. Projects that want per-module
gating switch to `from stdlib import text, score, ...` and add `[stdlib]
version = 2` to `cobble.toml` (which is the default, so explicit setting is
optional).

## Helper Naming

Helper names are unchanged from 0.7.x. The import form only gates availability.

| Module | Helpers |
| --- | --- |
| `text` | `text.plain`, `text.colored`, `text.score`, `text.selector`, `text.tellraw`, `text.title`, `text.subtitle`, `text.actionbar` |
| `score` | `score.set`, `score.add`, `score.remove`, `score.reset`, `score.copy`, `score.operation` |
| `score.objective` | `score.objective.add`, `score.objective.remove`, `score.objective.display` |
| `random` | `random.int`, `random.bool` |
| `timer` | `timer.set`, `timer.tick`, `timer.done`, `timer.reset` |
| `storage` | `storage.set`, `storage.merge`, `storage.remove`, `storage.copy`, `storage.append`, `storage.prepend`, `storage.insert`, `storage.get`, `storage.read_score`, `storage.copy_from` |
| `schedule` | `schedule.once`, `schedule.clear` |
| `bossbar` | `bossbar.add`, `bossbar.remove`, `bossbar.set_value`, `bossbar.set_max`, `bossbar.set_name`, `bossbar.set_color`, `bossbar.set_style`, `bossbar.set_visible`, `bossbar.set_players` |
| `team` | `team.add`, `team.remove`, `team.join`, `team.leave`, `team.modify` |
| `entity` | `entity.tag_add`, `entity.tag_remove`, `entity.effect_give`, `entity.effect_clear`, `entity.attribute_get`, `entity.attribute_base_set` |
| `math` | `math.abs`, `math.min`, `math.max`, `math.sqrt` |
| `event` | `addEventListener` |
| `datapack` | `datapack.function_tag`, `datapack.block_tag`, `datapack.item_tag`, `datapack.entity_type_tag`, `datapack.predicate`, `datapack.advancement`, `datapack.loot_table`, `datapack.recipe`, `datapack.item_modifier`, `datapack.dialog` |
| `resource_pack` | `resource_pack.item_model`, `resource_pack.block_model`, `resource_pack.lang` (experimental) |

## Required MVP Helper Clusters

### Text Components

Scope:

- Plain text components.
- Translated text components.
- Styled fragments only where the emitted JSON remains obvious.

Required tests:

- Helper expansion tests.
- Generated JSON snapshots.
- Positive and negative diagnostics for unsupported component shapes.

### Scoreboards

Scope:

- Objective creation.
- Player score operations.
- Reset and remove patterns.

Required tests:

- Generated `.mcfunction` snapshots.
- Command validation for emitted scoreboard commands.
- Diagnostics for invalid objective names and unsupported score holders.

### Storage And NBT

Scope:

- Storage path read/write helpers.
- Entity and block path helpers for literal paths only.
- Source-aware diagnostics for invalid literal paths.

Required tests:

- Generated command snapshots.
- Diagnostics for non-literal paths where literals are required.
- Source-map assertions for helper-emitted commands.

### Selectors

Scope:

- Reusable selector aliases.
- Common entity filters.
- Safer literal interpolation.

Out of scope:

- Opaque selector-builder runtime behavior.

Required tests:

- Selector alias expansion tests.
- Diagnostics for invalid selector literal fragments.
- CLI/WASM parity tests for supported single-file examples.

### Events

Scope:

- Load and tick registration helpers only when source explicitly opts in.

Required tests:

- Generated tag JSON snapshots.
- Generated function snapshots.
- Manifest and `inspect --json` assertions.

## Stretch Helper Clusters

These can ship only after every required MVP helper has docs, snapshots, and
validation coverage:

- Simple delayed function scheduling.
- Bossbar and team helpers.
- Entity helper conveniences.
- Scaled numeric read/write helpers.
- Small coordinate/vector helpers.
- Predicate helper conveniences.

## Deferred Helper Clusters

These are not part of 0.8.0 unless a separate design is accepted:

- Item component helper taxonomy.
- Automatic load/tick setup.
- Schedule cancellation.
- Broad vector math.
- Registry-wide generated helper APIs.
- Helpers requiring hidden temporary objectives, storage keys, or generated
  functions without explicit source opt-in.

## Implementation Checklist

- Add `[stdlib] version` to `cobble.toml` and `CobbleConfig`.
- Track active stdlib modules in `Transpiler`.
- Update `process_import` to populate the active set from stdlib imports.
- Gate each `process_*_intrinsic` on module activation.
- Add `stdlib-module-not-imported` diagnostic.
- Add `stdlib-v1-deprecated` warning for `version = 1`.
- Add unit tests for opt-in, full-activate, and not-imported diagnostics.
- Record `stdlib_version` and `active_stdlib_modules` in the build manifest.
- Add snapshots for generated functions and JSON.
- Add source-map assertions where helpers create commands or functions.
- Add manifest and `inspect --json` assertions for helper-created output.
- Add command-validation tests for emitted commands.
- Add docs showing Cobble source and generated output.