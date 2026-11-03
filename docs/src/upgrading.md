# Upgrading

Run the migration report, update the flagged source, then rebuild:

```bash
cobble migrate --json
cobble check
cobble build --validate
```

`migrate` reports changes without modifying files. `--apply` only updates
supported `cobble.toml` settings and creates a backup; source edits are manual.
The [changelog](https://github.com/deveworld/cobble/blob/main/CHANGELOG.md)
contains the full release history.

## 0.10 To 0.11

The target remains Minecraft Java Edition 26.3, pack format `121.0`.
The default migration route is 0.10 to 0.11.

### Explicit String Interpolation

Add `f` to strings that should interpolate values:

```python
wave = 3

# Before
/title @a title {"text": "Wave {wave}"}

# After
/title @a title f"Wave {wave}"
/title @a title {"text": f"Wave {wave}", "color": "gold"}
```

The same rule applies to helper arguments: `text.plain(f"Wave {wave}")`.
Plain strings, including doubled braces, are now literal. Unquoted command
arguments such as `/tp {player} {x} {y} {z}` are unchanged.

### Function-Local Variables

A name assigned inside a function is now local. Add `global` to change a
module variable:

```python
score = 0

def add_point():
    global score
    score += 1
```

The migration report flags affected functions. Raw commands outside Cobble
read locals from `<namespace>:global locals.<function>.<name>`; see
[Calling Convention](reference/calling-convention.md).

### Variable Storage And Numeric Types

Variables now live in `<namespace>:global` storage, under `vars.<name>` or
`locals.<function>.<name>`. Integer calculation registers use
`<namespace>.vars`. Keep using `temp` in Cobble source; the compiler
synchronizes source variables with storage. Update external commands to
read and write the storage paths in [Calling Convention](reference/calling-convention.md).

Existing scores are not migrated automatically. Module variables still reset
on load; `persist` initializes a value only when unset:

```python
persist best_time = 0
```

Decimal and exponent literals now use float32. Use `double(value)` for
software float64 calculations or `int(value)` to truncate toward zero.
See [Arithmetic](language/arithmetic.md) for promotion rules and limits.

`/` now returns a real quotient, including for integers. Replace `/` and `/=`
with `//` and `//=` where integer division is intended. Use `**` and `**=` for
powers; `^` and `^=` are no longer accepted. `math.sqrt` returns a real value.
Numeric variables can change between integer, float, and double values.

Command helpers accept runtime expressions directly. `storage.get(path)` and
`timer.done(name)` return values, and user functions can return strings, lists,
and maps. F-string fields accept expressions; loops support `break` and `continue`.

`+` and `*` also use the runtime types of parameters and returned values;
text and lists join or repeat. Selector, position, storage-path, and text-component
builders can be stored and returned. Import `math` before using its value helpers.

### Other Output Changes

- **Load and tag order:** setup runs first; event listeners and function tag
  entries then run in source order rather than being sorted by name.
- **Messages:** `say` with variables keeps Minecraft's sender prefix and
  selector expansion. Use `tellraw` for text longer than 256 characters.
- **String escapes:** standard escapes are decoded and unknown backslashes
  are preserved. Review paths and strings containing `\`.
- **Conditions:** negative numbers are true; `and` and `or` select operands
  and short-circuit; `elif` tests run only when reached. Conditions compare
  stored parameter values rather than string lengths.
- **Evaluation:** assignments happen after their right side, comparisons
  read left to right, and loop tests run once before each pass.
- **Recursion:** supported with per-call locals and arguments. Use Cobble's
  `return`, including inside execute blocks.
- **Entity templates:** `create` includes the template selector's tags.
- **Item models:** resource-pack item models also generate item definitions.
- **Stricter checks:** invalid names, unresolved placeholders, conflicting
  module definitions, and malformed resources may now be compile errors.

Packs leave `max_command_sequence_length` unchanged unless `[build] command_limit`
is explicitly configured. This option changes a world-wide gamerule on load.
A world that loaded an older pack keeps its previous gamerule value until
changed explicitly.

### Tooling Changes

- Validation caches the command tree per user. A local `data/commands.json`
  still takes precedence; remove stale copies when changing Minecraft versions.
- `build --zip` refuses a folder output inside a world's `datapacks/`,
  where the folder and ZIP would both load. Build elsewhere and copy the ZIP.
- Use a new or Cobble-owned output directory. `clean` and linked builds
  check project ownership before removing or replacing an existing pack.

## 0.9 To 0.10

The target changed from Minecraft 26.1.2 to 26.3, pack format `121.0`.
Update the config and command tree, then review resources and raw commands
for Minecraft format changes. In particular, predicates and item modifiers
use `type`, and loot tables use `modifier` and a single `condition`.
See [Data Pack Resources](stdlib/datapack-resources.md#json-resources).

To migrate directly to the current development version:

```bash
cobble migrate --from 0.9 --to 0.11 --json
cobble migrate --from 0.9 --to 0.11 --apply
```

## 0.8 To 0.9

0.9 targeted Minecraft 26.1.2 with pack format `101.1`. Current releases
use the later target, so prefer a direct migration:

```bash
cobble migrate --from 0.8 --to 0.11 --json
cobble migrate --from 0.8 --to 0.11 --apply
```

Review the report before applying config changes, keep resource-pack output
explicitly enabled if used, and re-run `check` and `build --validate`.
