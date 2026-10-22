# Upgrading

Each section covers one release step. To skip a release, follow the sections
in order; `cobble migrate --from <old> --to <new>` reports the combined
changes. The [changelog](https://github.com/deveworld/cobble/blob/main/CHANGELOG.md)
lists every change.

## 0.10 To 0.11

0.11 is still in development. This section lists the changes on `main` so far
and will grow before the release.

Cobble 0.11 keeps the Minecraft Java Edition 26.3 target and pack format
`121.0`, so `cobble.toml` and the command tree stay as they are. The changes are
in generated output: each pack keeps its variables on its own objective, and
module variables can opt into keeping their values across loads.

### Recommended Flow

1. Review the project with the migration report (0.10 to 0.11 is the default
   route):

```bash
cobble migrate --json
```

   The report lists every raw command and execute header that names the
   `temp` objective (`temp-objective-reference`). These lines keep working;
   the list shows where your pack reads or writes Cobble variables, which helps
   with step 3.

2. Rebuild and re-run project checks:

```bash
cobble check
cobble build --validate
```

3. Update anything outside the pack that reads Cobble variables by objective
   name, such as command blocks, other data packs, plugins, or commands typed
   in chat. `/scoreboard players get round temp` becomes
   `/scoreboard players get round <namespace>.vars`.

### Per-Pack Variable Objective

0.10 kept every pack's numeric variables and internal helper scores on one
shared `temp` objective, so two Cobble packs on one server that used the same
variable name overwrote each other. The `math` and `__internal__` objectives
were shared the same way.

In 0.11 these scores live on `<namespace>.vars`, for example `parkour.vars`
for the namespace `parkour`. The load function creates it with
`scoreboard objectives add <namespace>.vars dummy`. Strings, lists, and maps
already lived in the per-namespace storage `<namespace>:global` and are
unchanged.

`temp` stays the name Cobble source uses for this objective. The compiler
rewrites it to `<namespace>.vars` in `scoreboard` arguments, `if score`,
`unless score`, `store result score`, `scores={...}` selector filters, and
score text components. Other text, such as a chat message containing the word
`temp`, is left alone. See
[The `temp` Objective In Raw Commands](language/variables.md#the-temp-objective-in-raw-commands).

Values already stored under `temp` in an existing world are not carried over.
Module variables reset on every load in 0.10 as well, so the new objective
starts from the same values. Once no other pack uses it, remove the old
objective with `/scoreboard objectives remove temp`; do the same for `math` and
`__internal__` if no other pack uses them.

### Persistent Module Variables

Module variables still reset on every load. Declare one with `persist` to keep
its value across `/reload` and server restarts:

```python
persist best_time = 0
persist champion = "nobody"
```

The initializer only runs while the score or storage value is unset. `persist`
is only allowed at module level and takes a constant initializer. Packs that
kept records in a hand-written objective to survive reloads can switch to
`persist` for pack-wide values; per-player records still need their own
objective.

`persist` is not a reserved word: a variable named `persist` keeps working.

### Tooling Changes

- `cobble migrate` defaults to `--from 0.10 --to 0.11`. Routes from 0.8 and 0.9
  to 0.11 are also supported; pass `--to 0.10` for the previous route.
- The serde AST adds `Assignment.persist`, omitted from JSON when `false`.

## 0.9 To 0.10

Cobble 0.10.0 targets Minecraft Java Edition 26.3 and requires pack format
`121.0` (`121` names the same format). Like 0.9, the migration path is
report-first: normal build, check, and watch commands never rewrite project
files.

### Recommended Flow

1. Review the project with the migration report (0.9 to 0.10 is the default
   route; use `--from 0.8` for older projects):

```bash
cobble migrate --json
```

2. Update `cobble.toml` only after reviewing the report:

```bash
cobble migrate --apply
```

3. Replace the 26.1.2 command tree. The default `data/commands.json` is
   fingerprint-checked against the 26.3 tree, so an old file is refused; delete
   it and let `--validate` regenerate it, or generate it explicitly:

```bash
rm data/commands.json
scripts/setup_commands_json.sh 26.3
```

4. Re-run project checks and a validated build:

```bash
cobble check
cobble build --validate
```

`--apply` updates `[project].pack_format` to `121.0` and writes a timestamped
backup of `cobble.toml` first. It does not rewrite source files.

### Minecraft 26.2 And 26.3 Changes To Review

Cobble emits `datapack.*` JSON and raw commands as written, so sources written
for 26.1.2 may need these updates.

JSON resources (Cobble reports these as `datapack-resource-format` errors,
because a 26.3 server whose packs fail to load refuses to start):

- Predicates: `condition` is now `type`.
- Item modifiers: `function` is now `type`; `conditions` lists are a single
  `condition` (combine several with `minecraft:all_of`).
- Loot tables: `functions` is now `modifier`, `conditions` is a single
  `condition`, and a `minecraft:tag` entry's `name` is now `items`
  (`"#minecraft:logs"`).
- Root advancements with `display` need `display.background`.
- Cooking recipes need `cookingtime` (it no longer defaults to 200).

Other resource changes Cobble does not check:

- Entity predicates (26.2): `type` is now `minecraft:entity_type`, unknown keys
  are rejected, and `type_specific` sub-predicates moved to top-level keys such
  as `minecraft:type_specific/player`.
- Advancement trigger fields were renamed (for example `recipe` to `recipes`
  and `block` to `blocks`), and predicate lists are no longer `all_of`
  shorthand.
- Number providers need an explicit `type`, and several were renamed (`sum` to
  `add`, `product` to `mul`).
- Block state SNBT: `Name`/`Properties` are now `id`/`properties`.

Raw commands:

- Slot arguments of `item` and `execute if items` are slot sources; existing
  slot strings such as `container.0` and `weapon.mainhand` keep working.
- Team and waypoint colors must be lowercase names such as `dark_purple`;
  `reset` is its own literal.
- `schedule clear` takes a namespaced function ID.
- Sign click events and commands only run when the sign sets
  `allow_op_features:true`.
- `give` over the item limit, `tick step`, and a `gamerule` set to its current
  value now fail, which changes what `execute store success` records.

Text components in item components and entity NBT are SNBT (formerly
stringified JSON), so `custom_name='{"text":"Sword"}'` becomes
`custom_name={"text":"Sword"}`. The `enchantments` component is a plain map
(`enchantments={"minecraft:sharpness":5}`). Both changed in 1.21.5; the 0.10
examples were updated.

### Cobble Behavior Changes

See the 0.10.0 changelog for the full list. The ones most likely to change a
working pack:

- Execute blocks with more than one command run their body in a helper
  function once per entity, so conditions are no longer re-checked between
  body commands.
- Expressions folded into scores use Minecraft's integer division and floor
  modulus (`-7 / 2` is `-4`).
- `not (a and b)` now compiles with De Morgan's laws.
- Recursive function calls are rejected at compile time.
- Plain `cobble build` and `cobble clean --output` refuse non-empty output
  directories that do not carry this project's ownership marker.
- `--validate` rejects commands above the function permission level (2 by
  default; set `COBBLE_FUNCTION_PERMISSION_LEVEL` for a server that raised it)
  and macro lines without `$(...)` variables.
- Merged function tags are sorted, which can reorder `minecraft:load` and
  `minecraft:tick` handlers.

Library users: the serde AST gained `Assignment.target_expr`, the
`ExecuteModifier::If/Unless(Expression)` variants and the `CobbleError` enum
were removed, and transpiler functions return a typed `TranspileError`.

## 0.8 To 0.9

Cobble 0.9.0 targets Minecraft Java Edition 26.1.2 and requires pack format
`101.1`. The migration path is report-first: normal build, check,
and watch commands never rewrite project files.

### Recommended Flow

1. Review the project with the migration report:

```bash
cobble migrate --from 0.8 --to 0.9 --json
```

2. Update `cobble.toml` only after reviewing the report:

```bash
cobble migrate --from 0.8 --to 0.9 --apply
```

3. Re-run project checks and a validated build:

```bash
cobble check --json --experimental-python-compat
cobble build --validate
```

In 0.9.0, `--apply` only updates supported config settings such as
`[project].pack_format` to `101.1`; it does not rewrite source files or enable
experimental features. Before changing
`cobble.toml`, Cobble writes a timestamped backup next to the original file and
reports that path as `config.backup_path`.

The JSON report is designed for CI review. It includes:

- `config.changes[]` with before/after values for supported config updates.
- `actions[]` entries with before/after values for pack-format changes.
- `source.file_details[]` with per-file counts and manual-review locations.
- `suggested_cobble_alternative` hints for unsupported Python-like constructs.

### Items To Review Manually

- `pack_format`: update old values such as `81` to `101.1`.
- `resource_pack.*`: keep `[experimental] resource_pack = true` or pass
  `--experimental-resource-pack` when using resource-pack helpers.
- Python-like source: use `cobble check --experimental-python-compat` to see
  the supported Python-inspired surface and unsupported constructs that remain
  errors.
- Plugins: `plugins/*.toml` manifests are experimental and diagnostics-only in
  0.9.0. Cobble can evaluate built-in declarative rules, but it does not run
  project plugin code.
