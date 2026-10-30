# Upgrading

Each section covers one release step. To skip a release, follow the sections
in order; `cobble migrate --from <old> --to <new>` reports the combined
changes. The [changelog](https://github.com/deveworld/cobble/blob/main/CHANGELOG.md)
lists every change.

## 0.10 To 0.11

0.11 is still in development. This section lists the changes on `main` so far
and will grow before the release.

Cobble 0.11 keeps the Minecraft Java Edition 26.3 target and pack format
`121.0`, so `cobble.toml` and the command tree stay as they are. Most changes
are in generated output: each pack keeps its variables on its own objective,
module variables can opt into keeping their values across loads, and several
commands compile differently (see [Other Output Changes](#other-output-changes)).

### Recommended Flow

1. Review the project with the migration report (0.10 to 0.11 is the default
   route):

```bash
cobble migrate --json
```

   The report lists every raw command and execute header that names the
   `temp` objective (`temp-objective-reference`). These lines keep working;
   the list shows where your pack reads or writes Cobble variables, which helps
   with step 3. It also marks the lines whose output changes for the reasons
   under [Other Output Changes](#other-output-changes): `say-variable-text`,
   `load-function-tag`, `entity-template-tag`, and `item-model-definition`.
   `function-local-variable` marks each function that assigns a module
   variable without `global`; see
   [Function-Local Variables](#function-local-variables).

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

### Function-Local Variables

In 0.10 every variable was a module variable: a function that assigned
`score` changed the module's `score`, with or without `global`. In 0.11 a
name that a function assigns is local to that function unless the function
declares it `global`, as in Python. A local has its own score holder,
`#<function>.<name>`, and storage path, `locals.<function>.<name>`.

```python
score = 0

def add_point():
    global score   # needed in 0.11 to change the module variable
    score += 1
```

- Add `global name` to each function that changes a module variable.
  `cobble migrate` lists the functions that need it
  (`function-local-variable`), and `cobble build` and `cobble check` report a
  `local-shadows-module-variable` warning for each one.
- A function that reads a module variable before assigning it, such as
  `score += 1` without `global`, no longer compiles. The error names the
  variable and the function.
- Raw commands in a function that name a local, such as
  `/scoreboard players add level temp 1` after `level = 0`, now change the
  local's holder. Command blocks, other packs, and commands typed in chat that
  read such a variable need the new holder name, or the function can keep the
  value in a module variable.

See [Calling Convention](reference/calling-convention.md) for the full rules.

### Recursion

0.10 rejected a function that called itself, directly or through other
functions. 0.11 compiles it: each call saves the function's scores in the
storage `frames.<function>` and restores them before returning, so the caller
finds its own values again. A raw `/return` in a recursive function is a
compile error; use Cobble's `return`. See
[Recursion](reference/calling-convention.md#recursion).

### Other Output Changes

- **`say` text with variables.** 0.10 turned `/say Count {count}` into
  `tellraw @a`, which dropped the `[name]` prefix and printed selectors such as
  `@s` as plain text. 0.11 copies the values into the storage
  `<namespace>:global _say_args` and runs the line as a macro `say` in a
  helper function (`_cobble_say_<n>`), so the message looks like any other
  `say`. Selectors inside string values now show as names too, and the text
  after filling in the values must fit in 256 characters. See
  [Variables In `say`](language/commands.md#variables-in-say).
- **Load order.** 0.10 put the objective setup and module variable
  initializers at the top of the first load handler. A
  `datapack.function_tag("minecraft:load", [...])` entry whose name sorted
  before that handler ran before the objectives existed, and the handler then
  reset module variables the entry had set. 0.11 moves the setup into its own
  load function, `_cobble_init`, which runs first; the other load functions
  keep their 0.10 order.
- **Command limit.** 0.10 set the gamerule `max_command_sequence_length` to
  1,000,000,000 in every pack. 0.11 sets it only in packs with a runtime loop
  (a `while` loop, or a `for` loop that is not expanded at compile time), a
  runtime `^` power, or a recursive function. The
  gamerule is stored in the world, so a world that loaded a 0.10 pack keeps
  the raised value until you set it back with
  `/gamerule max_command_sequence_length 65536`.
- **Entity templates.** `create @Name` now adds each `tag=` value of the
  template's selector to the entity's `Tags`. A payload that already lists the
  same tags gets no duplicates.
- **Item model definitions.** `resource_pack.item_model` also writes
  `assets/<namespace>/items/<path>.json`, which Minecraft needs since 1.21.4
  to use the model. A static file at that path replaces the generated one.
  Models in the `minecraft` namespace get no definition, so vanilla items keep
  theirs.
- **String escapes.** 0.10 dropped the backslash of every escape, so
  `"a\nb"` became `anb`. 0.11 decodes `\n`, `\t`, `\r`, and `\uXXXX` as
  Python does and keeps a backslash before any other character. Check strings
  that contain a backslash, such as Windows paths.
- **One statement per line.** 0.10 split `x = 1 y = 2` into two statements.
  0.11 reports a parse error. `positioned as @s:` and `rotated as @s:` used
  to lose their `positioned`/`rotated` part and now compile in full.
- **`#` in block headers.** 0.10 read every `#` in a header as the start of a
  comment unless it began a namespaced tag, so `if score #round game matches
  3:` failed to parse. 0.11 reads a `#name` before the header's `:` as part of
  the header.
- **`unless A or B`.** A raw execute header such as
  `unless entity @a[tag=x] or entity @a[tag=y]:` was written unchanged, which
  is not a valid command. It now compiles to
  `execute unless entity @a[tag=x] unless entity @a[tag=y]`. Raw `if` and
  `unless` headers also stop splitting on `or` and `and` inside quotes and
  brackets, such as `@a[name="Tom and Jerry"]`.

### Conditions, Parameters, And Evaluation Order

0.11 follows Python's rules in several places where 0.10 did not. Review the
lines that rely on the old behavior:

- **Negative values are true.** 0.10 tested a bare value in a condition
  (`if lives:`, `not lives`, `while ready():`) with `matches 1..`, so a
  negative number counted as false. 0.11 treats every value other than 0 as
  true.
- **`and` and `or` as values.** 0.10 gave 1 or 0 for `x = a or b`. 0.11 gives
  one of the operands, as Python does. Conditions read the same.
- **Comparing two parameters.** 0.10 compared `a == b` as numbers, which for
  text compared lengths. 0.11 compares the stored values.
- **Parameters hide module variables.** In 0.10, a function whose parameter
  had the name of a module variable read the module variable in conditions and
  arithmetic, and the argument only in `{name}`. 0.11 reads the argument
  everywhere. Declaring a parameter `global` is now an error.
- **Assigning to a parameter.** 0.10 wrote the new value to a score that the
  rest of the function didn't read, so `while n > 0: n = n - 1` never ended.
  0.11 stores it with the call's arguments, and later reads, including
  `{name}`, see it.
- **Assignment order.** In `result = 1 + square(3)`, 0.10 wrote 1 into
  `result` before calling `square`, so a `square` that also set `result`
  changed the sum. 0.11 assigns after the whole right side.
- **Comparison order.** In `if level > raise_level():`, 0.10 read `level`
  after calling `raise_level`, so a change the call made to `level` counted.
  0.11 reads the left side first, as Python does.
- **`while` conditions.** 0.10 evaluated the condition twice per iteration,
  so a function in it ran twice. 0.11 evaluates it once.
- **`return` in `as @a:`.** In 0.10 the entities after the one that returned
  still ran the block. In 0.11 they skip it.
- **`and`, `or`, and `elif`.** 0.10 called every function in a condition. In
  0.11 the right side of `and` or `or` runs only when the left side doesn't
  decide the result, and an `elif` condition runs only when no earlier branch
  ran.
- **Constant folding.** 0.10 folded constant expressions with real numbers,
  so `100000 * 100000 / 100000` gave 100000 while the same expression on
  variables gave 14100 after the 32-bit overflow. 0.11 folds with the
  scoreboard's 32-bit arithmetic, and both give 14100.

### Tooling Changes

- `cobble migrate` defaults to `--from 0.10 --to 0.11`. Routes from 0.8 and 0.9
  to 0.11 are also supported; pass `--to 0.10` for the previous route.
- Validation keeps the default command tree in the user cache
  (`~/.cache/cobble/26.3/commands.json` on Linux; see
  [`cobble build`](reference/cli.md#cobble-build) for macOS and Windows), so
  each folder you build from no longer gets its own `data/commands.json`. A
  `data/commands.json` in the current folder is still used when it exists.
  Delete leftover `data/commands.json` files in project folders, or move one
  into the cache to skip the next download.
- `cobble build --zip` refuses an output folder inside a world's `datapacks`
  folder, because the folder and the ZIP would load as two packs.
- `--validate` reports message text over 256 characters in `say`, `msg`,
  `me`, and `teammsg`, which Minecraft refuses to load.
- Output, clean, link, init, and format paths may now pass through symlinked
  folders above the output or project folder, such as `/tmp` on macOS. The
  folder itself and anything inside it still must not be a symlink.
- Compiler errors, such as a variable placeholder in `give`, now report the
  file, line, and column of the statement in `cobble build`, `cobble check`,
  and the browser compiler.
- The build manifest adds `generated.resource_pack_item_definitions` and the
  resource kind `resource_pack_item_definition`.
- The serde AST adds `Assignment.persist`, omitted from JSON when `false`.
  Library users: `TranspileError` gains a `location` field, so code that
  builds it with a struct literal needs to set it (or use `TranspileError::new`).

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
