# Migrating From Cobble 0.9 To 0.10

Status: 0.10.0 migration notes.

Cobble 0.10.0 targets Minecraft Java Edition 26.3 and requires pack format
`121.0` (`121` names the same format). Like 0.9, the migration path is
report-first: normal build, check, and watch commands never rewrite project
files.

## Recommended Flow

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

## Minecraft 26.2 And 26.3 Changes To Review

Cobble emits `datapack.*` JSON and raw commands as written, so sources written
for 26.1.2 may need these updates.

JSON resources (Cobble reports these as `datapack-resource-format` errors,
because a 26.3 server whose packs fail to load refuses to start):

- Predicates: `condition` is now `type`.
- Item modifiers: `function` is now `type`; `conditions` lists are a single
  `condition` (combine several with `minecraft:all_of`).
- Loot tables: `functions` is now `modifier`, `conditions` is a single
  `condition`, and a `minecraft:tag` entry uses `items` (`"#minecraft:logs"`)
  instead of `name`.
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
- Block state SNBT uses `id`/`properties` instead of `Name`/`Properties`.

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

Text components in item components and entity NBT are SNBT, not stringified
JSON (`custom_name={"text":"Sword"}`, not `custom_name='{"text":"Sword"}'`),
and the `enchantments` component is a plain map
(`enchantments={"minecraft:sharpness":5}`). Both changed in 1.21.5; the 0.10
examples were updated.

## Cobble Behavior Changes

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

## CI Gate

For release candidates and larger project upgrades, run:

```bash
scripts/qa_10_release_gate.sh
```

Use `COBBLE_QA_ALLOW_DIRTY=1` only for local rehearsal against uncommitted
changes. The final release gate should run on a clean worktree.
