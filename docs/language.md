# Cobble Language Reference

Cobble is a high-level, Python-inspired language that compiles to Minecraft
data packs. Cobble is not Python-compatible; this reference describes the
syntax and behavior that Cobble intentionally supports.

For the implementation-facing support matrix, see
[Language Support Matrix](language-support.md).

## Table of Contents

- [Language Support Matrix](language-support.md)
- [Basic Syntax](#basic-syntax)
- [Data Types](#data-types)
- [Type System](#type-system)
- [Variables](#variables)
- [Functions](#functions)
- [Control Flow](#control-flow)
- [Minecraft Commands](#minecraft-commands)
- [Entity Selector Definitions](#entity-selector-definitions)
- [Entity Templates](#entity-templates)
- [File Imports](#file-imports)
- [Standard Library](#standard-library)
- [Events](#events)
- [Unsupported Python-Like Constructs](#unsupported-python-like-constructs)
- [Diagnostics](#diagnostics)

## Basic Syntax

Cobble uses indentation-based blocks after `:`. No braces or semicolons are
required.
Unexpected indentation and dedents that do not match a previous block level are
diagnostics before parser fallback.

```python
# This is a comment
def my_function():
    /say Hello, world!
```

## Data Types

### Numbers
```python
score = 10
health = 100
```

Numbers are internally stored as Minecraft scoreboard values.

### Strings
```python
message = "Hello, world!"
```

### Booleans
```python
is_active = True
is_disabled = False
```

Booleans are stored as integers in Minecraft scoreboards (0 for False, 1 for True).

### Lists, Maps, and None
```python
items = ["minecraft:diamond", "minecraft:emerald"]
config = {"chance": 1, "note": "missing"}
first_item = items[0]
chance = config.chance

datapack.predicate("maybe", {
    "condition": "minecraft:random_chance",
    "chance": 1,
    "comment": None
})
```

Lists and maps are storage-backed values. Cobble supports attribute and
subscript access when the base value resolves to a storage path, including
literal or constant subscript segments. Arbitrary Python-style indexing is not a
general runtime feature yet.

`None` is supported where Cobble serializes data pack JSON resources. It is
emitted as JSON `null`; it is not a scoreboard value and should not be used as a
numeric or boolean expression. Minecraft NBT/SNBT storage has no null type, so
storage literals reject `None`; use an explicit sentinel value for storage.

## Type System

Cobble has a **static, immutable type system**. Once a variable is assigned a value, its type is fixed and cannot change.

### Type Inference

Variable types are automatically inferred from their first assignment:

```python
x = 5        # x is type: integer
y = True     # y is type: boolean
name = "Bob" # name is type: string
```

### Immutable Types

Once a variable has been assigned, you cannot change its type:

```python
x = 10       # x is type: integer
x = 20       # ✓ OK - still an integer
x = True     # ✗ ERROR - cannot reassign integer to boolean

score = 5 + 3   # score is type: integer (result of arithmetic)
score = x > 10  # ✗ ERROR - cannot reassign integer to boolean
```

**Error message:**
```
Type mismatch for variable 'x'.

Variable was previously defined as type: integer
Cannot reassign to type: boolean

In Cobble, all variables have immutable types.
Once a variable is assigned a value, its type cannot change.
```

### Type Safety Benefits

The type system prevents common errors:

1. **Prevents accidental type changes** - Can't accidentally overwrite a score with a boolean
2. **Compile-time checking** - Type errors are caught before generating the data pack
3. **Better code clarity** - Variable types are clear from their usage

### Numeric Ranges and Precision

Because Cobble compiles to Minecraft scoreboards, numeric values have limitations:

**Integer Range:**
- Minimum: `-2,147,483,648` (i32::MIN)
- Maximum: `2,147,483,647` (i32::MAX)

**Float Precision:**
Floats are automatically converted to integers with a warning:

```python
pi = 3.14159  # ⚠️  Warning: Float will lose precision, truncated to 3
```

**Out-of-Range Values:**
Values exceeding the scoreboard range are clamped with a warning:

```python
huge = 9999999999  # ⚠️  Warning: Value exceeds range, clamped to 2147483647
```

## Variables

Variables are automatically managed as scoreboard objectives.

### Module-level Variables

Variables defined at the top level (outside functions) are automatically initialized when the data pack loads:

```python
# These are initialized in the _cobble_init function automatically
score = 0
lives = 3
max_health = 20
```

### Local Variables

Variables inside functions are initialized when the function is called:

```python
def my_function():
    score = 0
    score = score + 10  # Compiles to: scoreboard players add score temp 10
    score = score - 5   # Compiles to: scoreboard players remove score temp 5
```

### The `global` Keyword

The `global` keyword is used to indicate that a function should modify a module-level variable:

```python
# Module-level variable
score = 0

def increment_score():
    global score  # Declare that we're using the module-level variable
    score = score + 1

def reset_score():
    global score
    score = 0
```

**Important Note About Scope:**

Unlike Python, Minecraft scoreboards don't support true local scope. All scoreboard variables are stored in global objectives and are accessible from any function.

The `global` keyword in Cobble serves as **documentation** to clarify your intent, but due to Minecraft's architecture, all variables effectively behave as global regardless of whether you use the keyword.

**Best Practice:**
- Use `global` when you intend to modify module-level variables for code clarity
- This helps other developers understand your code's intent
- Be aware that variable names may conflict across functions since they share the same objective

```python
# Example showing the reality of Minecraft's scope
counter = 0

def func1():
    # Even without 'global', this modifies the module-level counter
    # because Minecraft scoreboards are always global
    counter = counter + 1

def func2():
    # This also modifies the same scoreboard value
    counter = counter + 10
```

### Compile-time Constants

Cobble supports compile-time constants using the `const` keyword. Constants are evaluated at compile time and can be used in expressions:

```python
# Define constants
const MAX_HEALTH = 100
const BASE_DAMAGE = 10
const CRITICAL_MULTIPLIER = 2

def apply_damage():
    health = MAX_HEALTH
    damage = BASE_DAMAGE * CRITICAL_MULTIPLIER
```

**Key Points:**
- Constants are evaluated at compile time when possible
- Constants can be used in arithmetic expressions
- Constants are stored as Expression values and used like variables in most contexts

**Example:**

```python
const PI = 3.14159
const RADIUS = 10

def calculate_area():
    # PI and RADIUS will be used in the calculation
    area = PI * RADIUS * RADIUS
```

## Functions

### Function Definition

```python
def greet():
    """Greet all players"""
    /tellraw @a {"text":"Hello!", "color":"green"}
```

### Functions with Parameters

Cobble supports function parameters using Minecraft's macro system:

```python
def give_reward(player, amount):
    """Give a reward to a player"""
    /give {player} minecraft:diamond {amount}
    /tellraw {player} {"text":"You received diamonds!", "color":"gold"}
```

**Important**: Use `{param_name}` in Cobble source. Cobble compiles it to
Minecraft's `$(param_name)` macro syntax when needed. Cobble v0.7.3 targets
Minecraft Java Edition 26.1.2, where function macros are available.

Parameter names must be unique. Default parameter values, `*args`, and
`**kwargs` are intentionally unsupported.

### Calling Functions

```python
def main():
    greet()
    give_reward("Steve", 5)
```

Parameterized functions can call other parameterized functions and forward their
own parameters:

```python
def give_reward(player, item, count):
    /give {player} minecraft:{item} {count}

def reward_nearby(player, item, count):
    give_reward(player, item, count)
    /tag {player} add rewarded
```

## Control Flow

### If Statements

```python
def check_score(score):
    if score >= 10:
        /say You have enough points!
        /give @p minecraft:diamond

    if score < 5:
        /say You need more points!
```

Supported operators: `==`, `!=`, `>`, `>=`, `<`, `<=`

**Complex Expressions in Conditions:**

You can use arithmetic expressions directly in conditions:

```python
def check_value():
    x = 10

    # Modulo in condition
    if x % 3 == 1:
        /say x mod 3 equals 1

    # Power in condition
    y = 2
    if y ^ 3 == 8:
        /say y cubed equals 8

    # Complex expressions with AND/OR
    a = 17
    b = 5
    if a % 5 == 2 and b ^ 2 == 25:
        /say Both conditions are true!
```

Under the hood, Cobble automatically creates temporary variables to evaluate complex expressions before comparison.

### Boolean Operators

Cobble supports boolean operators for combining multiple conditions:

```python
def check_conditions():
    x = 5
    y = 10

    # AND operator - both conditions must be true
    if x > 0 and y < 15:
        /say Both conditions are true!

    # OR operator - at least one condition must be true
    if x == 5 or y == 20:
        /say At least one condition is true!

    # NOT operator - negates the condition
    if not x == 10:
        /say x is not equal to 10!

    # Complex combinations
    a = 10
    b = 20
    c = 30
    if a > 5 and b < 25 and not c == 40:
        /say Complex condition met!

    if a == 10 or b == 30 or c > 25:
        /say OR combination works!
```

**Transpilation Details:**
- `and` operator chains conditions using `execute if ... if ...`
- `or` operator uses a temporary scoreboard variable (`or_result`) to track if any condition is true
- `not` operator converts `if` to `unless` (or vice versa)
- Double negatives are automatically simplified (`not not x == 5` → `if score x temp matches 5`)

**Examples:**

AND operator:
```python
if x > 0 and y < 15:
    /say test
```
Transpiles to:
```mcfunction
execute if score x temp matches 1.. if score y temp matches ..14 run say test
```

OR operator:
```python
if x == 5 or y == 10:
    /say test
```
Transpiles to:
```mcfunction
scoreboard players set or_result temp 0
execute if score x temp matches 5 run scoreboard players set or_result temp 1
execute if score y temp matches 10 run scoreboard players set or_result temp 1
execute if score or_result temp matches 1 run say test
```

**Note:** Boolean operators are only supported in regular `if` and `while` statements. Execute blocks (`as`, `at`, `asat`) use raw Minecraft syntax for their `if` modifiers:

```python
def check_players():
    # Execute blocks use raw Minecraft syntax
    as @a if entity @s[tag=special]:
        /say Special player!

    # Use regular if statements for Cobble boolean expressions
    x = 5
    if x > 0 and not x == 10:
        as @a:
            /say Regular if with boolean operators
```

**Boolean operators in while loops:**
```python
def loop_example():
    x = 0
    y = 0
    while x < 5 and y < 10:
        /say Loop running
        x = x + 1
        y = y + 1
```

**Note:** The `or` operator is fully implemented with automatic temporary variable handling. Complex nested OR expressions and combinations with AND are supported.

### Nested If Statements

Complex if statements are automatically split into separate functions:

```python
phase = 0
def boss_logic(boss_health):
    global phase
    if boss_health <= 50:
        if phase == 1:
            phase = 2
            /say Boss entered phase 2!
```

### For Loops

```python
def spawn_particles():
    for i in range(5):
        asat @s:
            /summon minecraft:pig ~ ~1 ~
            /particle minecraft:heart ~ ~ ~ 0.5 0.5 0.5 0 10

def count_by_twos():
    # Step support: increment by 2 each iteration
    for i in range(10) by 2:
        /say i = 0, 2, 4, 6, 8

def countdown():
    # Negative step: count backwards
    for i in range(10) by -1:
        /say i = 9, 8, 7, 6, 5, 4, 3, 2, 1, 0

def stepped_range():
    for i in range(1, 6, 2):
        /say i = 1, 3, 5

def literal_array():
    for direction in ["north", "south"]:
        /say direction = {direction}
```

For loops over literal ranges and literal arrays are expanded at compile time.
They emit the repeated commands directly instead of runtime scoreboard loop
counters or helper functions.

**Unrolling Support:**
- `range(n)` iterates `0..n`.
- `range(start, stop, step)` uses Python-style half-open bounds.
- `for i in range(n) by step:` remains supported for compatibility.
- Negative `by` over `range(n)` starts at `n - 1` and counts down.
- Literal arrays may contain numbers, strings, or booleans.
- A single loop may expand up to 1024 iterations. Expansions above 256 emit a warning.
- Nested unrolling is capped at 65,536 aggregate iterations and one outer
  unroll may emit at most 65,536 generated commands.

### While Loops

```python
def count_down():
    counter = 10
    while counter > 0:
        /say @a Count: {counter}
        counter = counter - 1
```

While loops are also compiled into recursive functions.

**⚠️ Important Performance Warning**: While loops execute all iterations in a single game tick. This can cause severe server lag or crashes with large iteration counts (>100). For long-running operations, consider using:
- Scheduled functions with `/schedule`
- Tick-based iteration (use a tick event handler that runs incrementally)
- For loops with known small iteration counts

### Match Statements

Match statements (also known as switch statements) provide efficient multi-way branching based on integer values:

```python
def check_score():
    score = 75
    match score:
        case 0:
            /say No score
        case 1 to 50:
            /say Low score
        case 51 to 80:
            /say Medium score
        case 81 to 100:
            /say High score
        case _:
            /say Out of range
```

**Pattern Types:**
- **Literal match**: `case 5:` - Matches exactly 5
- **Range match**: `case 1 to 10:` - Matches any value from 1 to 10 (inclusive)
- **Wildcard**: `case _:` - Matches anything not matched by previous cases

**Implementation Details:**
- Match statements use a 4-way split algorithm for efficient branching
- Single-statement cases are inlined when possible
- Multi-statement cases create separate functions
- Generates optimal `execute if score ... matches ...` commands
- **Case ranges must not overlap** - The compiler validates that all cases have mutually exclusive ranges
- Only the FIRST matching case executes (similar to C switch with implicit break)

**Examples:**

Simple literal matching:
```python
def handle_phase():
    phase = 2
    match phase:
        case 0:
            /say Phase 0: Waiting
        case 1:
            /say Phase 1: Starting
        case 2:
            /say Phase 2: Running
```

Range matching for grades:
```python
def assign_grade():
    score = 85
    match score:
        case 0 to 59:
            /tellraw @s {"text":"F","color":"red"}
        case 60 to 69:
            /tellraw @s {"text":"D","color":"gold"}
        case 70 to 79:
            /tellraw @s {"text":"C","color":"yellow"}
        case 80 to 89:
            /tellraw @s {"text":"B","color":"green"}
        case 90 to 100:
            /tellraw @s {"text":"A","color":"aqua"}
        case _:
            /tellraw @s {"text":"Invalid score","color":"red"}
```

Multi-statement cases:
```python
def handle_event():
    event_type = 3
    match event_type:
        case 1:
            /say Event 1 triggered
            /playsound minecraft:block.note_block.pling master @a
            /particle minecraft:happy_villager ~ ~ ~ 1 1 1 0 20
        case 2:
            /say Event 2 triggered
            /playsound minecraft:block.note_block.bass master @a
```

**Match Validation:**

The compiler validates that match cases don't overlap. This prevents bugs where multiple cases would execute:

```python
# ✗ ERROR - Overlapping ranges
match score:
    case 1 to 5:
        /say "1-5"
    case 3 to 7:  # ERROR: Overlaps with previous case
        /say "3-7"
```

Error message:
```
Match case overlap detected: case 3 to 7 overlaps with a previous case.

Previous case ended at: 5
Current case starts at: 3

Each case in a match statement must have non-overlapping ranges.
Match statements execute the FIRST matching case only.

To fix: Ensure all case ranges are mutually exclusive.
```

Correct version:
```python
# ✓ OK - Non-overlapping ranges
match score:
    case 1 to 5:
        /say "1-5"
    case 6 to 10:
        /say "6-10"
```

## Minecraft Commands

Minecraft commands are prefixed with `/`:

```python
def setup():
    /scoreboard objectives add score dummy
    /scoreboard objectives add health health
    /gamerule doMobSpawning false
```

**Note**: The `/` is automatically stripped when compiling to `.mcfunction` files, as per Minecraft specifications.

### JSON Commands

JSON-based commands are fully supported:

```python
def announce():
    /tellraw @a {"text":"Game Started!", "color":"gold", "bold":true}
    /title @a title {"text":"Welcome", "color":"aqua"}
```

### Variable Substitution in Commands

Use `{name}` for function parameters, loop variables, and defined Cobble
variables inside raw commands:

```python
def teleport_player(player, x, y, z):
    /tp {player} {x} {y} {z}
    /tellraw {player} {"text":"Teleported!", "color":"green"}
```

Unknown, invalid, and unclosed placeholders are diagnostics. Use identifier
names such as `{player_name}`, define the value, pass it as a function
parameter, or escape literal braces as `{{name}}`.

## Entity Selector Definitions

Define custom selector aliases to simplify your code:

```python
# Define selector aliases
@Player = @a[type=player,gamemode=survival]
@Boss = @e[type=zombie,tag=boss]
@Admin = @a[tag=admin]

# Use in commands
def give_rewards():
    as @Player:
        /give @s diamond

    as @Boss:
        /effect give @s strength 10 2

def admin_command():
    /tellraw @Admin {"text":"Admin message"}
```

**Key Points:**
- Selector definitions use `@Name = @selector[...]` syntax
- Aliases are replaced at compile time
- Works in execute blocks and commands
- Helps avoid repeating complex selectors

## Entity Templates

Entity templates name a reusable summon shape. Define a template with
`define @Name = @selector[...]`, provide the NBT payload after `create`, and end
the declaration with `end`. Create an entity from the template with
`create @Name` inside a function or execute block.

```python
define @ShopKeeper = @e[type=villager,tag=shop_keeper]
create {
    "VillagerData": {
        "profession": "minecraft:librarian",
        "level": 5,
        "type": "minecraft:plains"
    },
    "CustomName": '{"text":"Shop Keeper","color":"gold"}',
    "PersistenceRequired": True,
    "NoAI": True
}
end

def spawn_shop():
    as @a[tag=spawn_shop] at @s:
        create @ShopKeeper
```

Cobble lowers `create @ShopKeeper` to a `summon` command at the current execute
position. The entity type is taken from the template selector's `type=`
argument, so templates should include an explicit selector type. The template
name is part of the named symbol table, which means duplicate entity template
names across imports or directory builds are diagnostics.

**Key Points:**
- Template definitions use `define @Name = @selector[...] create {...} end`
- `create @Name` must reference a template that has been defined or imported
- The NBT payload uses Cobble map/list literals and is serialized as SNBT
- Use an execute block around `create @Name` to control spawn position

## File Imports

Import functions and definitions from other files:

```python
# utils.cbl
def helper_function():
    /say Helper called

@AllPlayers = @a[gamemode=!spectator]
```

```python
# main.cbl
import utils

def test():
    helper_function()
    as @AllPlayers:
        /say Test
```

**Key Points:**
- Use `import filename` to import another `.cbl` file
- Imported files are resolved relative to the importing file
- All functions and selector definitions are merged
- Duplicate function names, selector aliases, and entity template names across
  imported files are compile errors
- Circular imports are compile errors and include the import chain
- Missing imports include the importing file and expected path
- `from module import item` verifies that each requested item exists in the
  imported file
- Import aliases, wildcard imports, and comma-separated `import a, b` module
  lists are not supported; use one explicit import per line
- Standard library imports (`import stdlib`) work as before

## Standard Library

Cobble includes compiler intrinsics for common data pack tasks. Event handling
uses `stdlib`, while helper modules can be called directly from functions.

```python
import stdlib
from stdlib import event
```

### Event Types

- `event.LOAD` - Runs when the data pack is loaded
- `event.TICK` - Runs every game tick (20 times per second)

### Text Helpers

```python
def notify():
    text.tellraw("@a", {"text": "Loaded", "color": "green"})
    text.tellraw("@a", text.plain("Plain message"))
    text.tellraw("@a", text.colored("Gold message", "gold"))
    text.tellraw("@a", text.score("@s", "points"))
    text.tellraw("@a", text.selector("@p"))
    text.title("@a", "Ready")
    text.subtitle("@a", {"text": "Go", "bold": True})
    text.actionbar("@a", "Running")
```

`text.plain`, `text.colored`, `text.score`, and `text.selector` return JSON
text components for use inside other helpers or JSON resource declarations.
Colors accept Minecraft named colors or `#RRGGBB`.

### Score Helpers

```python
def update_score():
    score.set("points", 10)
    score.add("points", 5)
    score.remove("points", 2)
    score.copy("backup", "points")
    score.operation("points", "+=", "backup")
    score.reset("backup")
    score.objective.add("points", "dummy", "Points")
    score.objective.display("sidebar", "points")
```

Score helpers use the default Cobble `temp` objective.

### Random Helpers

```python
def roll():
    random.int("roll", 1, 6)
    random.bool("coin")
```

Random helpers compile to Minecraft's 26.1.2 `random value` command and store
the result in a scoreboard value.

### Timer Helpers

```python
def cooldown():
    timer.set("cooldown", 20)
    timer.tick("cooldown")
    timer.done("cooldown")   # writes cooldown_done as 0 or 1
    timer.reset("cooldown")
```

### Storage Helpers

```python
def save_status():
    storage.set("status", {"ready": True, "count": 3})
    storage.merge("status", {"extra": "ok"})
    storage.copy("status_copy", "status")
    storage.append("events", "loaded")
    storage.read_score("event_count", "events", 1)
    storage.remove("status.extra")
```

Storage helpers write to `<namespace>:global`.

### Schedule, Bossbar, Team, And Entity Helpers

```python
def setup_ui():
    schedule.once("tick", "20t", "replace")
    bossbar.add("timer", "Timer")
    bossbar.set_max("timer", 100)
    bossbar.set_players("timer", "@a")
    team.add("runners", "Runners")
    team.modify("runners", "color", "green")
    entity.tag_add("@a", "runner")
    entity.effect_give("@a", "minecraft:speed", 10, 1, True)
```

These helpers are thin wrappers over Minecraft commands. Use raw commands when a
specialized option is not covered yet.

### Data Pack JSON Resources

Top-level `datapack.*` declarations generate JSON resources in the pack's
namespace using the modern 26.1.2 folder layout.
Resource IDs must use lowercase `namespace:path` syntax or lowercase relative
paths. Cobble reports uppercase characters, invalid path separators, and common
`namespace/path` mistakes with focused diagnostics.

```python
from stdlib import datapack

datapack.function_tag("utility", ["mypack:setup"])
datapack.function_tag("minecraft:load", ["mypack:setup"])
datapack.block_tag("solid_blocks", ["minecraft:stone"])
datapack.item_tag("reward_items", ["minecraft:diamond"])
datapack.entity_type_tag("targets", ["minecraft:zombie"])

datapack.predicate("is_sneaking", {
    "condition": "minecraft:entity_properties",
    "entity": "this",
    "predicate": {
        "flags": {
            "is_sneaking": True
        }
    }
})

datapack.dialog("notice", {
    "type": "minecraft:notice",
    "title": {"text": "Notice"}
})
```

Supported resource declarations:

- `datapack.function_tag(name, values)`
- `datapack.block_tag(name, values)`
- `datapack.item_tag(name, values)`
- `datapack.entity_type_tag(name, values)`
- `datapack.predicate(name, json)`
- `datapack.advancement(name, json)`
- `datapack.loot_table(name, json)`
- `datapack.recipe(name, json)`
- `datapack.item_modifier(name, json)`
- `datapack.dialog(name, json)`

Typed tag declarations with the same ID are merged, deduplicated, and sorted.
Pass-through JSON declarations with identical JSON are accepted once; different
JSON for the same ID is a compile error.
Resource names may use nested paths and explicit namespaces, such as
`other_pack:checks/is_ready`. Predicate, advancement, loot table, recipe, item
modifier, and dialog declarations require object JSON values.

### Experimental Resource-Pack Assets

Resource-pack output is opt-in in 0.8.0. Enable it with
`cobble build --experimental-resource-pack` or `[experimental] resource_pack =
true` in `cobble.toml`, then import the `resource_pack` stdlib module.

```python
from stdlib import resource_pack

resource_pack.item_model("mypack:custom_sword", {
    "parent": "minecraft:item/handheld",
    "textures": {
        "layer0": "mypack:item/custom_sword"
    }
})

resource_pack.block_model("display_block", {
    "parent": "minecraft:block/cube_all",
    "textures": {
        "all": "mypack:block/display_block"
    }
})

resource_pack.lang("en_us", {
    "item.mypack.custom_sword": "Custom Sword",
    "block.mypack.display_block": "Display Block"
})
```

`resource_pack.item_model` writes `assets/<namespace>/models/item/<path>.json`,
`resource_pack.block_model` writes
`assets/<namespace>/models/block/<path>.json`, and `resource_pack.lang` writes
`assets/<namespace>/lang/<locale>.json`. The same lowercase namespace/path
validation used by `datapack.*` applies here. JSON must be an object, and lang
values must be strings.

### Math Helpers

```python
def calculate():
    root = math.sqrt(100)
    magnitude = math.abs(-50)
    low = math.min(10, 20)
    high = math.max(10, 20)
```

`math.sqrt` uses a generated scoreboard helper and no longer emits placeholder
runtime output.

## Events

### Registering Event Listeners

```python
import stdlib
from stdlib import event

def on_load():
    """Called when the data pack loads"""
    /scoreboard objectives add score dummy
    /tellraw @a {"text":"Data pack loaded!", "color":"green"}

def on_tick():
    """Called every tick"""
    as @a at @s:
        /particle minecraft:happy_villager ~ ~2 ~ 0.5 0.5 0.5 0 1

# Register event handlers
stdlib.addEventListener(event.LOAD, on_load)
stdlib.addEventListener(event.TICK, on_tick)
```

## Advanced Features

### Mixing Parameters and Literals

Function parameters use macro syntax:

```python
def complex_give(player, amount):
    # Give to the parameter player
    /give {player} minecraft:diamond {amount}

    # Give to a literal player named "Steve"
    /give Steve minecraft:gold_ingot 1

    # Use in JSON with parameters
    /tellraw {player} {"text":"You got items!", "color":"gold"}
```

### Arithmetic Operations

Cobble supports comprehensive arithmetic with proper operator precedence:

```python
def calculate():
    a = 10
    b = 5
    c = 3
    d = 8
    e = 2

    # Basic operations
    sum = a + b          # Addition: 15
    diff = a - b         # Subtraction: 5
    product = a * b      # Multiplication: 50 (uses multiplier helper)
    quotient = a / b     # Division: 2 (uses divisor helper)
    remainder = a % c    # Modulo: 1 (uses modulus helper)
    power = b ^ 2        # Power: 25 (compile-time expansion)

    # Multi-operator expressions
    result1 = a + b + c          # Chain addition: 18
    result2 = a - b - c          # Chain subtraction: 2
    result3 = a * b * c          # Chain multiplication: 150

    # Operator precedence (follows standard math rules)
    result4 = a + b * c          # Evaluates as: a + (b * c) = 25
    result5 = a * b + c          # Evaluates as: (a * b) + c = 53
    result6 = a - b / c          # Evaluates as: a - (b / c) = 9
    result7 = a % c + b          # Evaluates as: (a % c) + b = 6

    # Complex expressions
    complex = a + b * c - d / e  # Full precedence support
```

**Operator Precedence** (highest to lowest):
1. `^` - Power/exponentiation (right to left)
2. `*`, `/`, `%` - Multiplication, division, and modulo (left to right)
3. `+`, `-` - Addition and subtraction (left to right)
4. `==`, `!=`, `<`, `<=`, `>`, `>=` - Comparisons

**Important Notes**:
- Operators follow standard mathematical precedence
- Multiplication, division, and modulo use temporary fake players such as `#multiplier`, `#divisor`, and `#modulus` in the `temp` objective
- Power operator (`^`) uses compile-time expansion: `x^3` becomes `x*x*x`
- Power exponent must be a constant (variables not supported)
- Complex expressions automatically use temporary fake players such as `#expr_temp` for intermediate results
- All operations work with both constants and variables
- Literal loop variables are substituted at compile time during unrolling

**⚠️ Division and Modulo by Zero:**

Division and modulo by zero are **only checked at compile-time for constants**:
```python
x = 10 / 0  # ✅ Compile error: Division by zero
```

Division/modulo by a **variable** that may be zero at runtime is **not checked**:
```python
a = 10
b = 0  # Could come from earlier scoreboard/storage state at runtime
c = a / b  # ⚠️ No compile-time check - undefined behavior in Minecraft
```

Runtime division by zero behavior is **undefined** and may vary by Minecraft version. Always validate divisors:
```python
# ✅ Good practice: Validate before dividing
if divisor != 0:
    result = numerator / divisor
else:
    result = 0
    /tellraw @a {"text":"Error: Division by zero","color":"red"}
```

### Comments and Docstrings

```python
# Single line comment

def my_function():
    """
    This is a docstring.
    It can span multiple lines.
    """
    /say Hello
```

## Best Practices

1. **Use meaningful function names**: `spawn_boss()` is better than `func1()`
2. **Add docstrings to functions**: Document what your functions do
3. **Keep functions focused**: Each function should do one thing well
4. **Use the event system**: Register functions for LOAD and TICK events
5. **Test in creative mode first**: Always test your data packs before using in survival

## Example: Complete Data Pack

```python
import stdlib
from stdlib import event

# Global Variables
score = 0
game_active = 0

def init():
    """Initialize the game"""
    /tellraw @a {"text":"Game initialized!", "color":"green", "bold":true}

def game_loop():
    """Main game loop - runs every tick"""
    if game_active == 1:
        as @a at @s:
            /particle minecraft:end_rod ~ ~2 ~ 0.1 0.1 0.1 0 1

def check_win():
    """Check if a player has won"""
    global game_active
    if score >= 100:
        /tellraw @a {"text":"Someone won!", "color":"gold"}
        game_active = 0

# Register events
stdlib.addEventListener(event.LOAD, init)
stdlib.addEventListener(event.TICK, game_loop)
```

## Minecraft Version Compatibility

Cobble v0.7.3 requires **Minecraft Java Edition 26.1.2** and pack format **101.1**. Older Minecraft versions are intentionally not supported by this release.

- **Macros**: Function parameters use Minecraft's function macro system
- **Modern commands**: Uses latest command syntax
- **Data packs**: Selected modern data pack resources through `datapack.*` helpers
- **Decimal pack formats**: Emits `pack.mcmeta` with `min_format` and `max_format` set to `[101, 1]`

## Unsupported Python-Like Constructs

Cobble is Python-inspired, but it is not a Python runtime. Unsupported
constructs fail before transpilation when practical.

- Classes, decorators, `try`/`except`/`finally`, `with`, `lambda`, `yield`,
  `async`, `await`, `break`, `continue`, `raise`, `assert`, `del`, and
  `nonlocal` are not supported.
- Runtime `return` statements and return values are not supported.
- Default parameters, duplicate parameter names, `*args`, and `**kwargs` are not
  supported.
- List and dict comprehensions are not supported.
- `for ... else` is not supported.
- Dotted or relative imports are not supported. Use simple local module names.
- Assignment targets must be simple identifiers.
- Cobble function calls are standalone statements. Assignment expressions may
  use math intrinsics such as `math.sqrt(...)`, but arbitrary function calls in
  expressions are rejected.
- Standalone value expressions such as `score + 1` do not generate commands and
  are rejected. Use an assignment, function/helper call, raw command, or `pass`.
- The browser compiler accepts a single in-memory file. Non-`stdlib` imports
  should be checked with the CLI so Cobble can resolve local files from disk.

## Diagnostics

`cobble check`, `cobble build`, and the browser compiler share early
source-aware diagnostics for common mistakes:

- unsupported Python-like syntax,
- missing `:` after block headers,
- unclosed delimiters or strings,
- duplicate functions and duplicate parameters, including duplicate functions
  across imported or directory-compiled files,
- standalone no-op expressions,
- missing/circular imports and imported-file diagnostics,
- browser diagnostics for non-`stdlib` imports in single-file compilation,
- undefined variables in expressions and standalone helper/function call
  arguments,
- undefined Cobble function names and unknown helper module calls,
- undefined, invalid, unsupported imported-symbol, or unclosed raw command
  `{name}` placeholders,
- clearly inferred type mismatches,
- malformed `datapack.*` helper argument shapes, literal resource IDs, and tag
  value IDs, including non-string tag entries.

CLI diagnostics include file, line, column, source snippets, caret markers, and
actionable help text where available. Source snippets account for CRLF line
endings, and semantic scans ignore docstring bodies after parsing. The browser
compiler exposes the same line, column, kind, message, and help fields in
structured diagnostics.

## Limitations

- No Python class, exception, decorator, or runtime return-value semantics
- Lists and maps are storage-backed values; access works only when the base
  resolves to storage, and dynamic indexing remains limited
- Function parameters require Minecraft's function macro support
- For loops only support literal `range(...)` iterators or literal arrays
- While loops compile to recursive functions (performance impact for very long loops)

## Further Reading

- [CLI Documentation](cli.md)
- [API Documentation](api.md)
- [Examples](../examples/)
- [Minecraft Wiki - Data Packs](https://minecraft.wiki/w/Data_pack)
