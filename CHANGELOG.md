# Changelog

All notable changes to Cobble will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- A raw command in a `.cbl` file can go on in the next line after a trailing
  `\`, as in a `.mcfunction` file. See
  [Long Commands](https://deveworld.github.io/cobble/docs/language/commands.html#long-commands).
- `datapack.item_modifier` takes a list of modifiers and writes it as one
  `minecraft:sequence`, since Minecraft 26.3 refuses a list in the file.
- `--validate` reads SNBT the way Minecraft 26.3 does in text components, `nbt`
  tags, compounds, NBT path filters, selector `nbt=`, block entity data, item
  component values, particle options, and inline resources.
- `--validate` reports unknown item components and item tests, wrongly shaped
  component values, particles without their required options, block states that
  repeat or have no value, inline dialogs with missing fields, and a selector
  that names one `type` and has another `type`.
- `--validate` looks up loot tables, predicates, item modifiers, dialogs,
  features, slot sources, number providers, enchantments, damage types, biomes,
  world clocks, timelines, and block, item, biome, and point of interest tags in
  the game and the pack. A `minecraft:` id counts if either one defines it, an
  id without a namespace is a `minecraft:` id, and other namespaces the pack
  doesn't have go unchecked.
- `--validate` reads selector arguments the way Minecraft 26.3 does, and reports
  `@e`, `@n`, and UUIDs where only players go, and a selector that can pick
  several targets where one goes, such as `execute if score @a obj matches 1`.
- `--validate` reports text component fields of the wrong type, targets, NBT
  paths, and block positions that don't read, links without `http` or `https`,
  and unknown items, entity types, and dialogs in events. A component reads if
  one of its kinds of content does, and `type` picks the kind.
- `--validate` reads selectors in `say`, `me`, `msg`, and `teammsg` text the way
  Minecraft does, so `say hi @a[` and `say @e[limit=0]` are errors.
- `--validate` reports names of more than 16 characters where a target goes,
  block coordinates past 32 bits, and criteria with an unknown stat or value.
- `--validate` reads the `[...]` of items and blocks the way Minecraft 26.3 does,
  and reports `custom_data` strings that aren't compounds, enchantment levels
  outside 1 to 255, `dyed_color` lists without 3 items, an `item_model` that
  isn't an id, and `food` without `nutrition` and `saturation`.
- `datapack.dialog` reports a missing `type` or required field, and block, item,
  and entity type tags report an unknown `minecraft:` entry unless it has
  `"required": False`.

### Changed
- A plain number as a number provider, such as `compute default integer 5`, is
  an error, because Minecraft reads it as the id `minecraft:5`.

### Fixed
- Score holders take everything up to a space, so names such as `#a>b` and `a:b`
  pass `--validate`.
- `--validate` reads spaces the way Brigadier does: `give @s stone  1` is an
  error, and `setblock 1 2  stone` and `tellraw @a  "x"` pass.
- `--validate` refuses `:`, `/`, `#`, and `*` in words without quotes, such as
  `tag @s add my:tag`, and numbers with an exponent or a `+` outside SNBT.
- `--validate` takes `minecraft:` enchantments, damage types, and biomes that
  the pack defines, and a slot source without a namespace.
- `waypoint modify @s color hex` takes `ff0000` and `fff`, and refuses a `#`.
- `--validate` reads a line that ends in `\` together with the next one in
  function files, and reports a file that ends after a `\`.
- `template_rotation` takes `180`, the name Minecraft uses for a half turn.
- An `execute` chain past the checked length no longer fails as a redirect
  loop, and very deep SNBT can't overflow the validator's stack.
- `--validate` accepts a macro line where a placeholder stands for a word of the
  command, as in `$execute run $(command)`, and coordinates such as `~-$(x)`.
- `--validate` refuses ids with a second `:` or a `:` after a `/`, and whole
  numbers outside the 32-bit range.
- The `input` of a `minecraft:brewing` recipe is no longer reported as an old
  ingredient.
- `--validate` reads quoted strings the way Brigadier does: `true` can have
  quotes, a `\` only goes before a quote or another `\`, and an NBT path key
  can hold `$` but can't be `""`.
- `--validate` rounds a time to ticks before it compares it to the minimum, and
  reads a float as 32 bits, so `weather rain 0.5t` passes and a float past the
  32-bit range is an error.
- `--validate` takes Java's short form of a UUID, `*` and `?` in `test run`, `#`
  alone in `function`, and `-0` in `distance` and `level`, and refuses leading
  zeros in slot numbers and unknown ids that start with `:`.
- `--validate` trims a line and skips spaces the way Java does, so U+00A0 at the
  end of a command is an error, and a lone carriage return ends a line.
- `--validate` takes `@e` with `level`, `gamemode`, or a filled `advancements`
  where only players go, since only players have those.
- `--validate` accepts more macro lines: a placeholder can stand for a literal
  after an argument took it, as in `$return $(how) function a:b`, hold the rest
  of the command, or close a bracket or a quote that a word before it opened.

## [0.11.0-rc.1] - 2026-10-31

Release candidate for 0.11.0. See
[Upgrading](https://deveworld.github.io/cobble/docs/upgrading.html#010-to-011)
for the upgrade steps.

### Added
- `persist name = value` declares a module-level variable that keeps its value
  across `/reload` and server restarts. Its initializer only runs while the
  score or storage value is unset. It works for numbers, booleans, strings,
  lists, and maps, is only allowed at module level, and needs a constant
  initializer.
- `cobble migrate` supports routes to 0.11 and defaults to `--from 0.10 --to
  0.11`. The report lists each raw command and execute header that names the
  `temp` objective (`temp-objective-reference`).
- The optional Purpur server gate loads two packs with the same variable names
  and checks that their scores stay separate and that a `persist` variable
  survives `/reload`.
- `resource_pack.item_model` also writes the item model definition
  `assets/<namespace>/items/<path>.json` that Minecraft needs since 1.21.4,
  except for `minecraft` models, whose vanilla definitions stay. A static file
  at that path replaces it. The build manifest counts these as
  `generated.resource_pack_item_definitions` with the resource kind
  `resource_pack_item_definition`.
- Entity templates add each `tag=` value of their selector to the created
  entity's `Tags`.
- The 0.11 `cobble migrate` report also marks lines whose output changes:
  `say-variable-text`, `load-function-tag`, `entity-template-tag`, and
  `item-model-definition`, and functions that assign a module variable without
  `global` (`function-local-variable`).
- Functions can call themselves, directly or through other functions. Each
  call saves the function's scores and local storage in `frames.<function>`
  and restores them before it returns. A raw `/return` in a recursive
  function is an error.
- A differential test runs random programs through the compiler and the
  command simulator and compares the results with the same programs run as
  Python. The programs use numbers, strings, and lists, and some are split
  into files that import each other. CI runs it with 300 programs.
- Python forms: `==` and `!=` on text, lists, and maps; `in` and `not in`;
  text, lists, and maps in conditions; conditional expressions; multiple
  assignment; slices; `+` and `*` on text and lists; the list methods
  `append`, `extend`, `insert`, `pop`, `remove`, `index`, `count`, `sort`,
  `reverse`, and `clear`; `abs`, `min`, `max`, `sum`, `sorted`, and `str`;
  `enumerate`; `for` over text; `match` on text; and `//`, `**`, `//=`, and
  `**=`. Forms that need a loop compile to `_cobble_...` helper functions, and
  a pack gets only the ones it uses. See
  [Python Forms](https://deveworld.github.io/cobble/docs/language/python.html#python-forms).
- A comparison or `and`/`or` can be an operand of arithmetic, as in
  `total + (x > 1)`.
- `end`, `to`, `by`, `at`, `asat`, `create`, `define`, `match`, `case`, and
  `const` work as names of variables, functions, and parameters. They are
  keywords only in their own forms.
- Execute blocks take `on <relation>`, `facing ...`, and `summon <entity>`.
- An unexpected letter outside ASCII says that names are ASCII.
- Compiler warnings are diagnostics with a kind and a source location.
  `cobble build` prints them with the line they point at, `cobble check`
  reports them with severity `warning` and counts them in `warning_count`,
  and the browser compiler lists them under Diagnostics. The kinds are listed
  in [Diagnostics](https://deveworld.github.io/cobble/docs/reference/diagnostics.html#warnings).

### Changed
- `and` and `or` used as a value give one of their operands, as in Python, so
  `x = a or 5` is `a` when `a` isn't 0. They used to give 1 or 0.
- `==` and `!=` between two parameters compare their stored values, so text
  compares as text. They used to compare numbers, which is a string's length.
- An execute block may start with `execute`, as in `execute as @a at @s:`.
- `/say` text with variables runs as a macro `say` in a helper function
  instead of becoming `tellraw @a`, so it keeps the `[name]` prefix and shows
  selectors as names. The text after filling in the values is limited to 256
  characters, like any `say`.
- The objective setup and module variable initializers move from the first
  load handler into their own load function, `_cobble_init`, which runs first
  in `minecraft:load`; the other load functions keep their order.
- The gamerule `max_command_sequence_length` is only raised in packs with a
  runtime loop, a runtime `^` power, or a recursive function.
- Validation keeps the default command tree in the user cache
  (`~/.cache/cobble/26.3/`, `~/Library/Caches/cobble/26.3/`, or
  `%LOCALAPPDATA%\cobble\26.3\`) instead of `data/commands.json` in the
  current folder, which is still used when it exists.
- Symlinked folders above an output or project folder, such as `/tmp` on
  macOS, are allowed. The folder itself and anything inside it still must not
  be a symlink.
- `cobble build --zip` refuses an output folder inside a world's `datapacks`
  folder (next to `level.dat`), where the folder and the ZIP would load as two
  packs.
- Compiler errors report the file, line, and column of the statement that
  caused them in `cobble build`, `cobble check`, and the browser compiler, and
  the variable placeholder error shows its example with indentation.
- Library users: `TranspileError` gains a `location` field,
  `Transpiler::take_warnings` returns the warnings as `TranspileWarning`
  values instead of printing them, `Transpiler::has_compiled` tells whether a
  file was compiled already, and
  `Transpiler::with_options(TranspilerOptions { .. })` replaces the
  `set_description`, `set_pack_format`, `set_stdlib_version`, and other build
  setters. Set the validation summary on `transpiler.data_pack`.
  `transpiler::module_variables_made_local` lists the functions that assign a
  module variable without `global`.
- Each pack keeps its numeric variables and internal helper scores on its own
  objective, `<namespace>.vars`, so Cobble packs no longer overwrite each
  other's variables. The shared `temp`, `math`, and `__internal__` objectives
  are gone from generated output.
- `temp` in raw commands, execute headers, `score.*` helpers, selector
  `scores={...}` filters, and score text components now compiles to
  `<namespace>.vars`; existing source keeps working. See
  [Upgrading](https://deveworld.github.io/cobble/docs/upgrading.html).
- The serde AST adds `Assignment.persist` (omitted when `false`).
- A bare value in a condition is true when it is not 0, as in Python.
  Negative numbers used to count as false.
- Declaring a function parameter `global` is an error.
- A constant `range` that is empty, such as `range(3, 1)` or `range(i, 2)`
  inside an expanded `for i in range(4)`, and an empty array `[]` run no
  passes, as in Python. They used to be `unroll-bad-range` errors.
- A name that a function assigns is local to it unless declared `global`, as
  in Python. Locals use the score holder `#<function>.<name>` and the storage
  path `locals.<function>.<name>`, and raw commands in the function that name
  a local reach the same holder. Reading a local before assigning it is a
  compile error, and a function's local with the name of a module variable
  reports a `local-shadows-module-variable` warning.
- Runtime division by a variable or an expression no longer warns. The
  divisor is not checked, as described under Division By Zero.
- `^` in a `const` declaration computes the real power, like `*`. A result
  past the 32-bit range used to become 2147483647 there.
- `while True` warns only when its body has no `return`.

### Fixed
- `tellraw` and `title` show a string variable as its text. Minecraft 26.3
  prints an `nbt` text component as SNBT, so `{name}` used to show
  `"Bread"` with quotes. A value of unknown type, such as a copy of a
  parameter, is copied as text right before the command, and a list or a map
  still shows as SNBT.
- `text.tellraw()`, `text.title()`, `text.subtitle()`, and `text.actionbar()`
  show `{name}` placeholders as raw `tellraw` and `title` do. They used to
  print the placeholder as written.
- An item of a `persist` list, an item of a list imported with
  `from other import name`, and a field of a loop variable that walks a list
  of maps can be read into a variable. The storage check used to reject
  them.
- `cobble build --validate` accepts a macro line with a token that mixes
  text and a placeholder, such as `kills_$(mob)`.
- An unknown `@Name` in a raw command, such as a misspelled alias or an
  entity template, is a compile error. It used to reach the pack as written,
  and Minecraft can't load a function with such a command. Chat text in
  `say`, `msg`, and the other message commands stays as written.
- An alias inside a `selector` text component, as in
  `{"selector":"@Fighters"}`, is expanded. `@Alias[...]` in a raw command is
  a compile error, as it is in execute headers; it used to expand to
  `@a[...][...]`.
- A condition that is neither a Cobble expression nor starts with a
  Minecraft condition is a compile error. `if 2 in items:` used to compile
  to `execute if 2 in items`, which Minecraft can't load; the error now
  says Cobble has no `in` operator. The `slots` and `stopwatch` conditions
  of 26.3 pass through as Minecraft conditions.
- `elif` or `else` after an `if` with a Minecraft condition, such as
  `if entity @s[tag=a]:`, reports `unsupported-control-flow` with a rewrite
  that stores the test in a variable. It used to be a bare parse error.
- Reading another file's variable through its module name, as in
  `economy.balance`, says to import it with `from economy import balance`.
  The message used to ask for a map literal.
- `datapack.advancement()` reports a criterion's `player` or `entity`
  condition written as a list or as an entity predicate without `type`.
  Minecraft 26.3 rejects the advancement, and then `/reload` fails for every
  pack.
- `datapack.*` reports more shapes that Minecraft 26.3 rejects, which make
  `/reload` fail for every pack: `{"item": ...}` and `{"tag": ...}` recipe
  ingredients, `item` in a recipe `result` or an advancement icon, and any
  predicate, item modifier, loot entry, recipe, or trigger type that 26.3
  doesn't have, such as `minecraft:set_nbt`.
- `--validate` and `cobble validate` report what stops a function from
  loading in Minecraft: unknown item, block, entity, effect, particle,
  attribute, enchantment, damage type, and biome ids, block states, scoreboard
  criteria, and item slots, including entity types in selectors; references to
  loot tables, predicates, item modifiers, dialogs, and slot sources the pack
  doesn't define; invalid JSON text components; coordinates that mix `^` with
  others, fractional block coordinates, reversed ranges, invalid NBT paths, and
  selector arguments that repeat or can't be negated. All of these used to
  pass as valid. `clickEvent` and `hoverEvent`, which Minecraft ignores, are
  reported too.
- The validator's item slots follow 26.3: `mob.inventory.<n>` replaces
  `villager.<n>`, and `horse.saddle`, `horse.armor`, and the crafting and
  smithing names are gone.
- A string parameter with a `"` or a `\` shows in `tellraw` and `title` text.
  The value used to go into the JSON as a macro, which broke the command,
  and Minecraft then ran none of the function, not even its other commands.
  A parameter in `tellraw` text now shows from `args`; a parameter in a
  target, a `color`, or a `selector` is still a macro value.
- `grid[r][c] = v` with parameter indexes writes the item of a list of lists.
  It used to write a field named after `c` in `grid[r]`, so nothing changed.
- `for ch in word` over a string parameter gives its characters, as `word[i]`
  does. `ch` used to be empty.
- The build summary counts every file the build compiled, imported files
  included. It used to count only the entry files, so a project with one
  entry point always showed `Source files: 1`. `cobble inspect` labels the
  manifest's entry files `Entry files`.
- A build compiles each file once. A file that an earlier file imports used
  to be compiled again when the build reached it in the source folder, which
  left unused helper functions in the pack and set its module variables
  twice.
- `cobble check` reports an error in an imported file in that file. It used
  to report it in the importing file too, at that file's line numbers.
- `s[i]` of a string variable is a one-character string, for a literal,
  negative, or runtime index. It used to read a number, which fails in
  Minecraft.
- A call can pass a list or map literal, as in `total([1, 2])`, and a
  character of a string, as in `show(word[1])`. The literal used to fail to
  compile, and the character was passed as a number.
- A function can iterate a list parameter with `for`, read it in an
  assignment such as `first = values[0]`, and assign it a list or map. These
  used to be errors.
- A string or list variable stays in storage when it is assigned a copy of
  itself (`name = name`), a character of itself (`s = s[0]`), or a parameter.
  It used to turn into a score.
- A module variable assigned a copy of a string or list variable, or a
  character of a string, lives in storage. It used to become a score.
- An item of a list or a field of a map that holds a string stays a string
  when it is assigned or passed on, as in `name = names[1]` or
  `show(config.title)`. It used to become a number, the string's length.
- An item or field that the program sets to a value of another type, as in
  `items[0] = "abc"` for a list of numbers, reads the value it holds when the
  pack runs, wherever the write is: in a function that runs first or in
  another file. A string read this way used to become its length.
- A string parameter can be indexed, and a copy of a parameter keeps a string
  as a string. Whether `value[i]` of a parameter gives an item or a character
  is checked when the pack runs. Indexing a string parameter used to fail in
  Minecraft, and the copy became a number.
- A list literal can hold characters and items, as in `[word[0], names[1]]`,
  and an item assignment can store one. They used to be stored as numbers.
- Inside an expanded `for` loop over strings, `word[0]` reads a character of
  the current string. It used to be a compile error.
- `cobble check` accepts indexing a `for` loop target and an item of a list,
  and no longer reports a type mismatch for a value whose type it can't tell.
- The target of a `for` loop over a list copies into a number variable as a
  number, as in `last = value`. The copy used to go to storage, where the
  variable's score never saw it, and a new variable assigned from the target
  couldn't be used in arithmetic.
- A comma inside brackets of an assignment target, as in
  `items[pick(0, 1)] = 9`, no longer reads as tuple unpacking.
- `cobble check` no longer reports a type mismatch when a function's local or
  parameter has the name of a module variable of another type.
- A call statement reads its arguments from left to right, as in Python. In
  `set_level(level, set_level(5, 0))`, `level` used to be read after the
  inner call ran.
- A comparison reads a variable on its left before a call on its right runs,
  as in Python. In `if level > raise_level():`, a `raise_level` that changed
  `level` used to change the result.
- A function with parameters keeps its arguments across a call to a function
  without parameters. When that function called one with parameters, the
  caller's later reads of its own parameters got the other call's arguments.
- A number or a negation as a condition, such as `if 3:` or `if -x:`,
  compiles. Both used to be errors.
- A function parameter hides a module variable or constant with the same
  name. Conditions and arithmetic used to read the module variable while
  `{name}` read the argument.
- Assigning to a parameter stores the new value with the call's arguments, so
  later reads and `{name}` see it. `while n > 0: n = n - 1` on a parameter
  used to loop forever.
- An assignment runs after the calls on its right side:
  `result = 1 + square(3)` gives 10 even when `square` sets `result`.
- A `while` condition is evaluated once per iteration. A function in the
  condition used to run twice.
- `return` inside an `as @a:` block stops the block for the remaining
  entities.
- A function call on the right of `and` or `or` runs only when the left side
  doesn't decide the result, and an `elif` condition runs only when no earlier
  branch ran.
- Constant folding uses the scoreboard's 32-bit arithmetic, so a folded
  expression and the same expression on variables agree on overflow.
- A `#name` in a block header, such as the fake player in
  `if score #round game matches 3:` or the tag in `as @a[tag=#x]:`, is part
  of the header instead of starting a comment.
- A raw command with an apostrophe, such as `/say it's done # note`, drops
  its trailing comment.
- A `]` inside a quoted selector value (`@e[name="a]b"]`) no longer ends the
  selector.
- String escapes decode as in Python: `"a\nb"` holds a line break, and a
  backslash before another character stays. The escape used to be dropped,
  leaving `anb`. Storage values and map keys keep the escape in the
  generated command.
- Parse errors after indentation with wide spaces such as U+3000 report the
  character column.
- `cobble build --validate` removes its staging folder when writing the pack
  fails, and `--zip` writes the entries in a stable order.
- `math.sqrt` and `math.abs` keep their whole argument when it contains
  another `math` call, as in `math.sqrt(1 + math.abs(a))`.
- Adding or subtracting the constant -2147483648 compiles to valid commands.
  The compiler used to emit an out-of-range `remove`.
- A list or map on the right of an arithmetic operator reads its size, as it
  does on the left. It used to read a score that was never set.
- Nested map reads such as `cfg.inner.c` pass `cobble check`, which used to
  report `inner` as an unknown base variable.
- `positioned as <selector>:` and `rotated as <selector>:` keep the
  `positioned`/`rotated` part. It used to be dropped without an error.
- Two statements on one line, such as `x = 1 y = 2` or `return 1 2`, are a
  parse error. They used to compile as two statements.
- A parse error inside deeply nested parentheses or `if` blocks is reported
  right away. The parse time used to double with each level.
- A Python `if` header followed by a body on the same line (`if x: pass`)
  reports the body as the problem.
- A raw execute header `unless A or B:` compiles to `unless A unless B`
  instead of being written unchanged as an invalid command.
- Raw `if`/`unless` headers no longer split on `or` or `and` inside quotes or
  brackets, and an `if A and B or C:` alternative compiles to a valid chain.
- `--validate` reports `message` arguments (`say`, `msg`, `me`, `teammsg`)
  over 256 characters, which Minecraft refuses to load.
- Commands inside unrolled `for` loops keep their own source locations
  instead of taking those of identical commands later in the file.
- First-time command tree generation uses a partial file of its own, so two
  validations at once do not overwrite each other's download.
- `cobble watch` keeps working after an editor saves by replacing a file. It
  used to lose `cobble.toml`, or the single file it was given, after the
  first such save. Moving `build.source` out of a folder that holds rebuilt
  output no longer fails and stops the config watch.
- `cobble link` saves absolute paths, and paths in older link state are read
  from the project folder. `watch --link`, `clean --linked`, `link --status`,
  and `doctor` used to look in the wrong place from another working folder.
- A file input finds the `cobble.toml` above the current folder, so
  `cobble build main.cbl` inside `src` uses the project's settings instead of
  the default namespace and a new `src/output`.
- A project can replace and clean its own output after its namespace changes.
  An output inside the project folder also stays the project's after the
  folder moves. `cobble clean` used to refuse both.
- `tellraw` and `title` text written as one quoted string, such as
  `/tellraw @a "Hi {name}"`, no longer shows the quotes when it has a
  placeholder. A `"` in a string parameter no longer breaks that line.
- Map keys that aren't plain names, such as `"minecraft:diamond"` and
  `"Iron Sword"`, work in subscripts, both as literals and as runtime values.
  They used to give an invalid command, or for `"a.b"` read a nested path.
- `return x > 3` and other conditions return 1 or 0, and `a < b < c` works as
  in Python. A multi-line `if (...)` or `while (...)` header is accepted.
- A function name with capital letters is an error at its `def` line, also in
  `cobble check`. It used to pass `check`, then fail `build` with no location.
- A comment inside the list of a `datapack.*_tag` call no longer counts as a
  tag value.
- `cobble fmt` keeps the indentation of a comment on the first line of a block
  or a bracket when the line before it ends with a comment or a bracket.
- A variable in an SNBT text component, as in
  `/tellraw @a {text:"Score {score}",color:gold}`, shows its value. The
  component used to print as raw text, and a list of components was an error.
  Variables also work in `extra`, `with`, and hover text.
- `xs = [xs[1], 0]` reads the old list, which it used to overwrite first.
- `name[f()]` on text reads the text before `f` runs, as Python does.
- A copy of a parameter shows its new value in a loop. `/say {a}` before
  `a = a + 1` used to print the first value on every pass.
- `--validate` accepts every NBT path name Minecraft does, such as `a:b` and
  names outside ASCII, and a token such as `a@b` is no longer read as a
  selector.

### Documentation
- The documentation is now a book built with mdBook from `docs/src/` and
  published with the website at <https://deveworld.github.io/cobble/docs/>.
  The language reference is split into chapters, the migration notes are one
  Upgrading page, and the cookbooks are folded into the chapters they cover.
  The hand-written Rust API reference is replaced by
  [docs.rs](https://docs.rs/cobble-lang).
- A Calling Convention reference describes where locals, arguments, return
  values, and recursion frames live in the generated pack.

## [0.10.0] - 2026-10-22

Cobble 0.10.0 targets Minecraft Java Edition 26.3 (data pack format 121.0)
and ships the runtime-semantics fixes below. See
[Upgrading](https://deveworld.github.io/cobble/docs/upgrading.html#09-to-010)
for the upgrade steps.

### Minecraft 26.3
- The only supported target moves from 26.1.2 (pack format 101.1) to 26.3
  (pack format 121.0). `pack.mcmeta` gets `min_format`/`max_format`
  `[121, 0]`, and `121` is accepted as `121.0`.
- `--validate` and `cobble validate` use the 26.3 command tree. An old 26.1.2
  tree at the default `data/commands.json` path fails the fingerprint check.
- The validator knows the 26.2/26.3 argument types `slot_source`,
  `team_color` (lowercase names, separate `reset`), `swing_animation`,
  `feature`, `context_int_provider`, and `context_float_provider`. Loot
  modifier, predicate, and table arguments accept inline SNBT.
- `datapack.*` declarations in pre-26.3 JSON shapes are
  `datapack-resource-format` errors, since a 26.3 server refuses to start
  when its packs fail to load. This covers predicate `condition` (now
  `type`), item modifier `function` (now `type`) and `conditions` (now one
  `condition`), loot table `functions`/`conditions` (now
  `modifier`/`condition`), tag entry `name` (now `items`), root advancements
  without `display.background`, and cooking recipes without `cookingtime`.
- `cobble migrate` supports 0.8 -> 0.10 and 0.9 -> 0.10 (the default) and sets
  `project.pack_format` to `121.0`. The 0.8 -> 0.9 route is removed.
- `cobble init` templates, examples, and browser samples use the 26.3 JSON
  shapes, SNBT text components, and the plain-map `enchantments` component.

### Added
- `return expr` compiles to Minecraft's function return value.
  Integer-returning functions, including imported ones, work in assignments,
  arithmetic, and conditions (`if ready():`), and `import m` allows
  `m.func()` calls.
- Compound assignment (`+=`, `-=`, `*=`, `/=`, `%=`, `^=`), also on
  storage-backed targets such as `values[i] += 1`.
- Storage-backed assignment targets (`config.count = ...`,
  `groups[i].value = ...`), with runtime list indexes and string map keys.
- `len(value)` for strings, lists, and maps.
- Runtime `for` loops over `range(...)` with runtime bounds or steps, over
  storage-backed lists, and over literal arrays with runtime values.
  Unsupported cases report `runtime-for-unsupported`, `runtime-for-bad-step`,
  or `runtime-for-bad-range`.
- `x ^ n` with a runtime exponent. A negative runtime exponent gives `0`.
- `{var}` substitution inside `tellraw`/`title` JSON array components.
- `datapack.dialog_tag(name, values)` for dialog tags such as
  `minecraft:pause_screen_additions` and `minecraft:quick_actions`.

### Changed
- Execute blocks and `if`/`elif`/`else` branches with more than one command
  run their body in a generated helper function, once per execution context.
  Before, each command repeated the condition, so a body that changed its own
  condition cut off the rest, and under a multi-entity `as` every entity ran
  the first command before any ran the second. Single commands are still
  inlined, and `store` blocks and bodies with a raw `/return` keep the
  per-command form. Under `as`, `and`/`not` flags around an `or` are
  per-entity `@s` scores.
- Compile-time integer arithmetic matches Minecraft's scoreboard division and
  modulo: `-7 / 2` is `-4` (was `-3`), `-7 % 3` is `2` (was `-1`), and
  `1 / 3 * 3` is `0` (was `1`). `const` keeps real `/`
  (`const HALF = 1 / 2` is `0.5`), and `%` is a floor modulus everywhere.
  `range(N / D, 0)` with integer constants no longer fails as a non-integer.
- `--validate` and `cobble validate` reject commands above the function
  permission level (`transfer`, `tick`, `op`, `stop`), which Minecraft
  refuses to load in a function. The level defaults to 2; set
  `COBBLE_FUNCTION_PERMISSION_LEVEL` (0-4) for a server that raised
  `function-permission-level`.
- The validator applies Minecraft's macro rules: a `$` line needs a
  `$(variable)`, and names use only letters, digits, and `_`. Generated packs
  no longer put `$` on execute lines without variables.
- `cobble init` keeps an existing `src/main.cbl`, `.gitignore`, or template
  file and lists what it kept. It checks options such as `--pack-format`
  before it creates the project folder.
- Merged tag values are normalized and sorted the same way on disk, in
  memory, and in the browser compiler. This can change the order of merged
  `minecraft:load`/`minecraft:tick` handlers.
- `cobble check` continues past a failing file and reports each file's first
  semantic error once.
- `check --json` and the browser compiler give `unroll-*`, `runtime-for-*`,
  stdlib module (`missing-stdlib-module`, `unknown-stdlib-module`), and
  `resource_pack.*` errors (`resource-pack-not-enabled`,
  `resource-pack-bad-id`, `resource-pack-non-object`,
  `resource-pack-duplicate`) their own kinds. Others stay `semantic`.
- Conflicting flags (`fmt --check`/`--diff`, `clean --linked`/`--output`,
  `watch --link`/`--output`, `link --datapacks`/`--world`/`--minecraft`) are
  rejected at argument parsing.
- `link --world` and `--minecraft` require the base folder to exist.
- `doctor` stops the `java` and `curl` probes after 10 seconds. `watch`
  rebuilds after at most 2 seconds during a busy event stream, and reloading
  `cobble.toml` re-resolves the linked output and project identity.
- Cleanup failures after a successful validated publish are warnings.
- `examples/` holds three practical packs (`lobby`, `parkour`, `magic_items`)
  plus `hello_world.cbl`. The single-feature examples are removed.
- User-function calls stage their arguments in generated per-call storage.
- Library users: transpiler errors are typed `TranspileError` values with a
  kind and a message (they were `String`). The serde AST adds
  `Assignment.target_expr` and drops the `ExecuteModifier` expression
  variants. The parser exports the statement span tree
  (`ProgramSpanTable.statement_tree`, `StatementSpanTree` and related types,
  `LocatedProgram::statement_spans()`).

### Fixed
- `not (a and b)` compiled as `not a and not b`. Negation follows De Morgan's
  laws in `if`, `elif`, `while`, condition assignments, and execute
  conditions.
- Assignments that read their own target (`x = 10 - x`, `z = 3 * z + z`),
  `flag = not flag`, and `done = done and c` overwrote or reset the target
  before reading it.
- A bare `const` in a condition (`if DEBUG:`) or `match`, and the right side
  of a comparison between two function parameters, read unset scores.
  Operands made only of literals and constants now fold at compile time, and
  literal-only conditions set up the objective they read.
- Deeply nested right operands (`a * (b * (c * d))`) shared one scratch
  score, and nested `math.*` calls copied results through the wrong
  objective.
- `a/b` without spaces compiled as `a` plus a raw command `b`. Only a `/`
  that starts a statement begins a raw command.
- String, list, and map variables in conditions or as `match` subjects are
  compile errors (they read unset scores), and so are non-integer `match`
  patterns such as `case 1.5:` (they crashed the compiler).
- Call statements passed a string, list, or map variable's name
  (`greet(msg)`) and failed when later calls used other argument types.
  Arguments to a parameterless function are an error; they were dropped.
- Recursive calls, direct or through a cycle, are a compile error that
  suggests a loop. Recursion compiled but ran incorrectly.
- A single macro command in `if`/`elif`/`else` came out as
  `execute ... run $tellraw ...`. The `$` now starts the line.
- Raw execute conditions keep glued tokens (`positioned ^1 ^ ^`,
  `stone[snowy=false]`, `ns:util/check`) and dotted paths (`ns:foo.bar`)
  intact, and write SNBT `True`/`False` as `true`/`false`.
- Raw conditions that start with a Minecraft keyword stay raw even with
  parentheses (`entity @s[name="Bob (Admin)"]`), and Python conditions
  without parentheses (`x > 3 and y < 2`) translate like parenthesized ones.
- In execute headers, a `#ns:tag` (`as @e[type=#ns:mobs]:`) was read as a
  comment and hung `check` and `build`, and a `:` inside SNBT
  (`if data storage ns:x {flag:"on"}:`) ended the header. `# note` is still a
  comment.
- `{CONST}` and `{param}` in execute selectors and positions
  (`as @a[distance=..{RADIUS}]`) were written unchanged. Selector aliases
  were not expanded in execute conditions (`if entity @Spawn:`) and matched
  name prefixes (`@Spawn` in `@SpawnPoint`).
- An unknown `@Name` in `as`/`at` is a compile error. It used to fail when
  the function loaded.
- A `{var}` in a styled text component lost the styling. It now inherits the
  component's color and formatting.
- `storage.read_score()` added a `data get` scale when none was given, so
  list sizes and string lengths read as 0.
- Tag values can reference other tags (`"#minecraft:skeletons"`).
- Multi-line `"""` docstrings were tokenized line by line, so their bodies
  compiled as code (a `/kill @e` line became a real command) and a backtick
  in a module docstring failed the build.
- The validator accepts selector float ranges with a lower bound
  (`@e[distance=1..5]`), particle options (`dust{color:[...]}`), text after a
  macro placeholder (`$say $(x) text`), `function #ns:tag`, the `*` item
  predicate, and macro offsets after a relative prefix (`^ ^ ^$(distance)`).
- `x = 7 % 0.5` and non-ASCII text in compound assignments and operator
  expressions (`val한e`) crashed the compiler.
- False errors: `undefined-variable`, `missing-block-colon`, and
  `unsupported-decorator` on Unicode identifiers (`força`); a missing block
  colon after `def main():  # comment`; undefined variables for multi-line
  `def` parameters and for Minecraft words in statement-level raw conditions
  (`if score @s kills matches 1..:`).
- Nested `def`s leaked parameters into the enclosing function, multi-line
  `def` headers skipped the duplicate-parameter check, placeholder scoping
  under bracket-continued headers was wrong, and `no-op-expression` skipped
  parenthesized and call-containing statements.
- Error positions: multi-line calls and docstring bodies point at the call
  site (they showed line 1), tokenizer errors at the bad character (they
  showed 1:1), browser missing-import errors at the import, and `datapack.*`
  ID carets are correct after non-ASCII text. Failed raw conditions show the
  original operand (`@s`, `{count}`).
- A file that fails to parse reports only its parse error and pre-parse
  findings. A file reached both as an entry point and through imports is
  reported once, and overlapping entry points compile once.
- Runtime division warnings describe the failed command. They claimed
  Minecraft "silently returns 0".
- `cobble build src`, `cobble build src/main.cbl`, and `cobble clean src`
  after a plain `cobble build` refused the output as "owned by another Cobble
  project".
- A `cobble.toml` without `[build]` used the project root as source and
  output. It now uses `src` and `output`.
- `cobble migrate` no longer flags formatting variants of the supported
  `pack_format`, and it records locations for `from stdlib import ...` lines.

### Removed
- Unused public API: `CobbleError` with `report_error`/`report_parse_errors`,
  the `ExecuteModifier::If(Expression)` and
  `ExecuteModifier::Unless(Expression)` variants, and the `stdlib::builtins`
  module.

### Security
- Plain `cobble build` (without `--validate`) refuses to write into a
  non-empty output folder that lacks this project's ownership marker
  (`.cobble/build_manifest.json` with a matching namespace and `project_id`),
  because stale-output cleanup could delete data in folders Cobble did not
  create. Output folders from before the build manifest or from other tools
  are refused; remove them once or build into a new folder.
- `cobble clean --output <DIR>` checks the same marker, so one project can no
  longer clean another project's output.

### Known Limitations
- Numeric variables of every Cobble pack share the `temp` objective, so two
  packs that use the same module variable name overwrite each other.
  Per-pack objectives are planned for 0.11.0.
- Module-level variables reset on every pack load (`/reload` or a server
  start). Keep values that must persist in your own objectives; opt-in
  persistent variables are planned for 0.11.0.

## [0.9.0] - 2026-06-27

### Added
- Experimental, diagnostics-only plugins through
  `cobble check --experimental-plugins`. It reads `plugins/*.toml` manifests,
  validates their capabilities, and adds lint rules for raw `tellraw`, `op`,
  creative mode, and long commands.
- Experimental Python compatibility report through
  `cobble check --experimental-python-compat` or
  `[experimental] python_compat = true`. Unsupported constructs stay errors
  and suggest Cobble alternatives where available.
- Experimental `cobble migrate`, which scans config and source files.
  `--apply` updates the config from 0.8 to 0.9, with a timestamped
  `cobble.toml` backup, change summaries, and source-location hints.
- stdlib v3 JSON-only `item_component.*` helpers, `storage.path`/`child`/
  `index`, `selector.*` and `position.*` helpers, and `entity.teleport()`.

### Changed
- `cobble check --json` reports config, plugin-manifest, and stdlib module
  errors in the same failure format as source diagnostics.
- `cobble migrate --json` uses the strict config schema, so unknown keys are
  errors.

### Security
- Validated builds publish into existing owned outputs through a staged
  merge and rename that keeps unrelated files.
- Rebuilding after turning off resource-pack output can no longer delete the
  project's `assets/` folder or put stale assets into data-pack ZIPs.
- stdlib module gating covers nested `text.*` helpers, and `check` gates
  modules like the build does.

## [0.8.0] - 2026-06-24

The stdlib v2 and resource authoring release for Minecraft Java Edition
26.1.2 (data pack format 101.1).

### Added
- stdlib v2 modules, imported one at a time with `from stdlib import ...`.
  `[stdlib] version = 1` and `import stdlib` still work.
- Data-pack resource authoring helpers with deterministic tag and resource
  output, duplicate handling, typed diagnostics, and manifest resource
  entries.
- Compile-time unrolling of `for` loops over literal `range(...)` and
  literal arrays, recorded as `Unrolled` in the source map and
  `unrolled_loops` in the build manifest.
- Experimental resource-pack output behind `--experimental-resource-pack`,
  with `assets/` generation, ZIP inclusion, manifest counts, and `inspect`
  output.

### Changed
- Configured projects default to stdlib v2, which needs an explicit import
  for each stdlib module used by new helper calls.
- The web compiler and its ZIP export include the new resources, assets, and
  unrolling metadata.

### Security
- Limits on total compile-time unrolling, so nested literal loops and
  command-heavy expansions fail before they exhaust compiler memory or freeze
  the browser compiler.

## [0.7.3] - 2026-06-24

A security hardening release for Minecraft Java Edition 26.1.2 (data pack
format 101.1).

### Security
- Link state (`.cobble/link_state.json`), default `data/commands.json`
  setup, and `cobble fmt` and `cobble init` writes refuse symlinked path
  components. `doctor --json` reports symlinked marker components as errors.
- `watch --link` refuses an existing linked pack that contains symlinks.
- The web demo rejects traversal-like namespaces and ZIP entries.

### Fixed
- Raw execute `unless` with Python-style `and`/`or`, prefixed comparisons,
  or out-of-range comparisons produced positive `if` chains or checks that
  depended on whether a score existed.
- `or` guards and `unless A and B` guards in one execute block leaked
  internal markers or dropped earlier guards.

## [0.7.2] - 2026-06-23

A security and workflow hardening release for Minecraft Java Edition 26.1.2
(data pack format 101.1).

### Changed
- `cobble clean --dry-run` shows the marker path, namespace, project
  identity, required pack files, symlink safety, and the next step.
- `cobble link --status` gives recovery hints for missing or tampered link
  state and missing or invalid markers.

### Security
- `cobble build --validate` writes into an existing output only if it has a
  matching build manifest, and it keeps unrelated files there. Before, the
  whole output folder was deleted and replaced.
- Unsafe namespaces (absolute, traversal, `/`, `\`) are rejected before they
  reach output paths or ZIP names.
- Library users: `DataPack` checks namespaces, function names, tag paths,
  and JSON resource paths before cleanup or writes.

## [0.7.1] - 2026-06-16

A workflow and tooling hardening release for Minecraft Java Edition 26.1.2
(data pack format 101.1).

### Added
- Workflow tooling for formatting, template discovery, link state, linked
  cleanup, `doctor --json`, and experimental `check --json --symbols`.
- `cobble fmt --diff` previews formatter changes without writing files.
- `doctor --json` reports the configured output under `experimental_output`.
- Build manifests record `project_root`, `project_id`, and a timestamp.
  Linked workflows must match both namespace and project identity before
  they replace or clean a pack, and tampered link state is rejected.
- A failed final move of a validated output restores the previous output
  when possible.
- Link, clean, and build outputs refuse symlinked path components, existing
  build and clean output trees that contain symlinks are refused, and builds
  refuse an output path that is a file.
- The formatter handles BOM/CRLF, multi-line docstrings, and trailing
  comments more safely, and a failure while formatting a folder leaves its
  files unchanged.

### Changed
- `watch --link` rejects `--namespace`; linked checks use the namespace from
  `cobble.toml`.

## [0.7.0] - 2026-06-04

The stable 0.7.0 release for Minecraft Java Edition 26.1.2 (data pack format
101.1). The language and diagnostics work is listed under 0.7.0-rc.1.

### Fixed
- Diagnostics accept a numeric `const` index on storage-backed values
  (`items[INDEX]`), matching the transpiler.
- A function that forwarded its own parameters to another parameterized
  function passed the parameter names as strings.
- Unknown or wrongly called `math.*` intrinsics are reported by `check`,
  `build`, and the browser compiler. `check` used to pass them.

## [0.7.0-rc.1] - 2026-06-04

The first 0.7.0 release candidate, focused on the language specification and
diagnostics.

### Added
- A language support matrix of supported Cobble syntax and unsupported
  Python-like syntax.
- `cobble check --json`, and structured diagnostics in the browser compiler.
- Errors for unsupported Python-like constructs: default parameters, varargs,
  decorators, compound assignment, comprehensions, dotted, relative, aliased,
  and wildcard imports, `break`, `continue`, `raise`, `assert`, `del`,
  `nonlocal`, `for ... else`, and non-identifier assignment targets.
- Syntax errors before parsing: missing block colons, bad indentation,
  unclosed or unmatched delimiters, and unterminated strings.
- Semantic checks before transpiling: duplicate functions and parameters,
  unsupported `return`, non-math calls as values, argument counts for
  same-file calls, nested calls as arguments, no-op expressions, undefined
  variables, type mismatches on reassignment, bad `{name}` placeholders in
  raw commands, and invalid `datapack.*` JSON, tag values, and resource IDs.
- Import-tree checks in the CLI: missing and circular imports, errors in
  imported files, unknown `from module import item` names, and duplicate
  functions or selector symbols and wrong argument counts across files.
- The browser compiler reports non-stdlib imports.
- Diagnostics show the source line with a caret, with correct offsets in
  CRLF files.
- Schema docs for `.cobble/build_manifest.json` and `.cobble/source_map.json`.

### Changed
- `check`, `build`, and the web compiler share one early diagnostic pass.
- Math intrinsics such as `math.sqrt()` are still allowed as assignment
  values. Cobble function calls as values are rejected.
- `cobble check` follows file imports. The browser compiler still compiles a
  single file.
- Function names that collide across files are rejected before generated
  functions can overwrite each other.
- Semantic checks skip multi-line docstring bodies.

## [0.6.3] - 2026-06-03

A stable release that mostly adds tests, CI, and release QA.

### Changed
- Typed JSON resources are written in sorted path order.

### Fixed
- `cobble inspect` shows `Validation: not recorded` for a null validation
  section.
- Duplicate resources report the conflict kind and both declaration
  locations.

## [0.6.2] - 2026-06-02

### Added
- `cobble doctor` reports the version, Minecraft target, pack format, Java
  and curl availability, config status, and command-tree fingerprint,
  without network access.
- `cobble build --dry-run` builds and optionally validates in temporary
  staging without writing the output.
- `cobble build --quiet` for scripts; validation errors still print.
- `cobble inspect <output>` summarizes the build manifest and source map,
  with `--json`.
- Text component helpers `text.plain`, `text.colored`, `text.score`, and
  `text.selector`.
- `.cobble/build_manifest.json` in each build, with versions, inputs,
  namespaces, resources, counts, and the validation summary.
- Build summaries with file, function, command, tag, and resource counts,
  validation counts, and the ZIP path.
- Web demo views for functions, resources, metadata, the manifest, and
  diagnostics, with samples, copy and download, ZIP export, and syntax
  highlighting.

### Changed
- `cobble init` prints the next commands, including dry-run and validation.
- Source maps use project- or source-relative paths where possible.
- The website root is an overview page, and the browser compiler is at
  `/try`.

### Fixed
- `scripts/setup_commands_json.sh` falls back to other manifests and the
  pinned server jar when a manifest fetch fails.
- Clearer resource errors for uppercase paths, slash-separated namespaces,
  and bad path separators.
- The default `data/commands.json` fingerprint is checked before validation.
- Stricter validation of nested NBT/JSON delimiters, scoreboard slots,
  execute swizzles, item slots, and legacy item-stack NBT.
- Build manifests no longer double-count merged stdlib and explicit tags.
- Web demo: manifests report the `cobble-lang` version, duplicate tag paths
  merge like the CLI, the `.wasm` asset loads on GitHub Pages, and the
  `__wbindgen_externrefs` error on `/try` is fixed.

## [0.6.1] - 2026-06-02

### Added
- Standard library v1.1 helpers for objectives, storage
  append/prepend/insert/read/copy-from, schedules, bossbars, teams, and
  entity tags, effects, and attributes.
- `cobble init --template <minimal|stdlib|validation>`.
- Namespaced `datapack.*` declarations such as `minecraft:load` tags.
- The first validating run generates `data/commands.json`, with fallback
  manifest hosts and URL or local-jar overrides.

### Changed
- Validation summaries count macro commands apart from skipped ones, and
  validation errors print caret context when available.

### Fixed
- `datapack.*` JSON helpers reject non-object values where Minecraft needs
  an object.
- Rebuilds remove resources in explicit namespaces that left the source.

## [0.6.0] - 2026-06-01

### Added
- `cobble build --validate` and `cobble watch --validate`, and
  `--commands-json <PATH>` for build, watch, and validation.
- Standard library v1: `text`, `score`, `random`, `timer`, `storage`, and
  `math.sqrt` without its placeholder behavior.
- `datapack.*` helpers for function, block, item, and entity tags,
  predicates, advancements, loot tables, recipes, item modifiers, and
  dialogs.
- `.cobble/source_map.json`, mapping each generated command to its source.
- Example projects for Minecraft Java Edition 26.1.2.

### Changed
- Circular imports fail with the import chain; they were skipped. Missing
  imports report the importing file and the expected path.
- Generated functions use the singular `data/<namespace>/function` layout
  and pack format `101.1`.

### Fixed
- Hardened validation diagnostics and macro-line skip reporting.

## [0.5.18] - 2025-10-21

### Changed
- The `multiplier`, `divisor`, `modulus`, `power_base`, and `expr_temp`
  objectives are gone. Their values are fake players in `temp`, with the same
  arithmetic results.
- `/say` with variables (`/say Value is {x}`) becomes `tellraw @a` with score
  components. Plain `/say` and macro parameters work as before.

### Fixed
- Results of complex expressions (`result = (x + y) * z`) could not be used
  in `/say {result}`.

## [0.5.17] - 2025-10-19

### Fixed
- `return` statements were ignored, and `x = helper()` assigned nothing.
  Both are compile errors that explain the limit and suggest alternatives.
- Assigning attribute access (`x = obj.attr`), subscripts (`x = arr[0]`), or
  `None` is a compile error.

## [0.5.16] - 2025-10-09

### Fixed
- Minecraft range syntax (`1..`, `..5`, `1..5`) parses.
- Circular imports are detected again. The per-file import stack reset in
  0.5.15 had turned detection off.
- Python expressions in execute blocks (`as @a if x > 5:`) are translated.

## [0.5.15] - 2025-10-08

### Fixed
- Building a folder no longer prints false circular import warnings.
- Selector aliases are replaced only in command text, so `@Boss` inside JSON
  text or NBT stays as written.
- `{{name}}` outputs a literal `{name}`, as documented.

## [0.5.14] - 2025-10-08

### Fixed
- Decimal pack formats are written as JSON numbers. `--pack-format 88.0`
  wrote the string `"88.0"`, which Minecraft rejects.
- Docs: the minimum Minecraft version is 1.20.2 (pack format 18), and
  decimal pack formats need 1.21.9 or later.

## [0.5.13] - 2025-10-08

### Fixed
- `10-5` was read as `10` and `-5`, and `2^3` as a coordinate marker. Both
  parse as arithmetic.
- Division or modulo by a compile-time zero is an error; it was a warning.
- Comparisons past the scoreboard range (`x > 2147483647`) produced invalid
  ranges. They compile to an always-false check.
- Power exponents are limited to 100.
- Decimal pack formats such as `88.0` (Minecraft 1.21.9) are accepted by
  `--pack-format`.

## [0.5.12] - 2025-10-07

### Fixed
- Negative-step `range()` loops started at the wrong value; they start at
  `count - 1`.
- Functions that use `$(param)` directly lacked the `$` line prefix.
- Deleted or renamed functions stayed in the output. Each build clears the
  functions folder first.

## [0.5.11] - 2025-10-06

### Fixed
- Nested `for` loops shared a wrapper function, which caused infinite loops
  and server freezes.

## [0.5.10] - 2025-10-05

### Fixed
- Invalid number literals are errors; they became `0.0`.
- `True`/`False` in a pack without variables did not set up the objective
  and constants they use. That objective is created only when needed.
- Loop variables and their types leaked out of loops.
- Float values in arithmetic print a truncation warning.
- Constant modulo uses integer arithmetic, matching runtime results for
  negative numbers.

## [0.5.9] - 2025-10-05

### Fixed
- `range(0)` loops ran once. Loops check the condition before the body.
- Boolean and expression arguments (`True`, `x + y`) were not passed to
  macro functions. Booleans are passed as `1`/`0`.
- JSON arrays without variables are kept as written. Arrays with variables
  are an error that suggests score components.
- `or` conditions used the `temp` objective without creating it.

## [0.5.8] - 2025-10-05

### Fixed
- Imports nested two or more levels deep failed to compile.
- Selector and item arguments (`give_item(@a, diamond)`) were read as
  scores. They are passed as strings.
- Calling a function defined later in the file left out `with storage`.
- `True`/`False` failed in load and tick functions, which have no `@s`.

## [0.5.7] - 2025-10-05

### Changed
- Dividing by a variable whose constant value is 0 prints a warning.

### Fixed
- Variable types leaked between functions, so two functions could not use
  the same name with different types.

## [0.5.5] - 2025-10-04

### Added
- Execute modifiers `positioned`, `rotated`, `in`, `anchored`, `align`, and
  `store (result|success)`.
- Unary `-` and `+` (`-x`, `-(a + b)`).

### Fixed
- Inline comments are stripped from commands, except a `#` inside a string.
- Unsupported condition expressions counted as true. They are errors.
- `for` over anything other than `range()` ran the body once. It is an
  error.

## [0.5.4] - 2025-10-04

### Fixed
- `cobble init --pack-format` accepts only 81 to 255. Larger values wrapped
  around.
- `/title` with variables put the `title`, `subtitle`, or `actionbar` word
  inside the text.

## [0.5.3] - 2025-10-04

### Fixed
- Generated packs use the official folder names (`functions/`,
  `tags/functions/`, `loot_tables/`).
- Constants fold to literal values. They were emitted as fake-player
  references.
- Comparisons accept a literal or constant on either side.

## [0.5.2] - 2025-10-04

### Fixed
- `asat <selector>` uses `at @s`. It repeated the selector, so commands ran
  several times when several entities matched.
- `x ^ 0` gives 1. It was rejected.

## [0.5.1] - 2025-10-03

### Added
- Nine example files in `examples/`, with a README learning path.
- Clearer errors for unsupported power operations, such as variable
  exponents.

### Fixed
- `^` is right-associative: `2^3^2` is `512` (was `64`).
- Nested constant expressions fold at compile time.

## [0.5.0] - 2025-10-03

### Added
- Static types. A variable's type (Integer, Boolean, or String in function
  parameters) comes from its first assignment. Breaking: changing it later
  (`x = 5` then `x = True`) is a compile error.
- Warnings for floats that lose precision and for values outside the
  scoreboard range, which are clamped.

### Fixed
- Breaking: overlapping `match` ranges (`case 1 to 5:`, `case 3 to 7:`) are
  compile errors.
- Module-level booleans (`active = True`) were not initialized.

## [0.4.3] - 2025-10-03

### Fixed
- Execute blocks with several `if`/`unless` conditions produced capitalized,
  invalid keywords (`If entity ...`).

## [0.4.2] - 2025-10-03

### Fixed
- `a or b or c` and `(a or b) and c` produced invalid `OR(...)` output.
- `case _:` ran unconditionally with one statement, and more than once with
  several. It now excludes all earlier cases in one command.

## [0.4.1] - 2025-10-03

### Added
- Loop variables in commands: `/say Count: {i}` in a loop body becomes
  `$say Count: $(i)` in a macro function.

### Fixed
- Functions without parameters were called with `with storage`.
- `for i in range(10) by 2:` failed to parse.

## [0.4.0] - 2025-10-03

### Added
- Selector aliases such as `@Admin = @a[tag=admin]`, replaced at compile
  time in commands and execute blocks.
- `import utils` loads `utils.cbl` relative to the importing file, prevents
  circular imports, and merges its functions and selectors.

## [0.3.0] - 2025-10-03

### Added
- `const NAME = value` for compile-time constants.
- `match` with literal (`case 5:`), inclusive range (`case 1 to 10:`), and
  wildcard (`case _:`) cases.

## [0.2.2] - 2025-10-02

### Added
- Load functions set the `maxCommandChainLength` gamerule to 1000000000.

### Fixed
- `if`/`elif`/`else` blocks with more than one statement, and `while`
  bodies, run in their own function, so a statement that changes the
  condition does not skip the rest.
- Init functions set variables before creating their objectives. They now
  set the gamerule, create objectives, and then set variables.

## [0.2.1] - 2025-10-02

### Added
- Arithmetic in `if` and `while` conditions (`if x % 3 == 1:`), on either
  side of a comparison and with `and`/`or`.

## [0.2.0] - 2025-10-02

### Added
- `%` modulo, `^` power with a constant exponent, and `or` in conditions.
  Precedence is `^`, then `*`/`/`/`%`, then `+`/`-`, then comparisons.
- Loop steps with `by` (`for i in range(10) by 2:`). Negative steps count
  down from `n-1`.

### Fixed
- Negative loop steps use `scoreboard players remove`.

## [0.1.2] - 2025-10-02

### Fixed
- `pack_format` must be between 1 and 255. Larger values overflowed.

## [0.1.1] - 2025-10-02

### Added
- Parenthesized expressions (`result = (a + b) * c`).

### Fixed
- `x = x + 1` no longer emits a redundant self-assignment.
