# Cobble 🧱

> A modern, Python-like language for creating Minecraft Data Packs

[![Rust](https://img.shields.io/badge/rust-2021%20edition-orange.svg)](https://www.rust-lang.org)
[![Minecraft](https://img.shields.io/badge/minecraft-26.1.2-green.svg)](https://minecraft.net)
[![Pack Format](https://img.shields.io/badge/pack%20format-101.1-blue.svg)](https://minecraft.wiki/w/Data_pack)

Cobble is a transpiler that converts Python-like code into Minecraft Data Packs, making it easier and more intuitive to create complex Minecraft command systems.

**✨ Version 0.6.1** - Expanded validation diagnostics, standard library v1.1 helpers, namespaced data pack resources, project templates, and Minecraft Java Edition 26.1.2 support | Pack Format 101.1

## ⚠️ Pre-release Notice

**Cobble is currently in active development (v0.6.1 Pre-Alpha).** While we've implemented many features and extensive tests, the project may contain bugs and unexpected behavior. Features and APIs may change between releases.

**We appreciate your feedback!** If you encounter any issues, unexpected behavior, or have suggestions, please report them at:

- **GitHub Issues**: <https://github.com/deveworld/cobble/issues>

Your bug reports and feature requests help make Cobble better for everyone. Thank you for being an early adopter! 🙏

## ✨ Features

- ✅ **Static Type System** - Immutable types with compile-time inference and validation
- ✅ **Python-like Syntax** - Familiar, clean syntax with proper indentation
- ✅ **Function Parameters** - Full support using Minecraft function macro system
- ✅ **Event System** - Built-in event handling for load and tick events
- ✅ **Control Flow** - If statements, for loops (with step support), while loops with smart optimization
- ✅ **Match/Switch Statements** - Efficient multi-way branching with overlap validation
- ✅ **Boolean Operators** - `and`, `or`, `not` operators for complex conditions (e.g., `if x > 0 and y < 5 or z == 10:`)
- ✅ **Complex Expressions** - Multi-operator expressions with proper precedence (e.g., `a + b * c`)
- ✅ **Arithmetic Operations** - Full support for +, -, *, /, %, ^ with variable and constant operands
- ✅ **Advanced Operators** - Modulo (%) and power (^) operators with compile-time optimization
- ✅ **Expressions in Conditions** - Use arithmetic directly in if/while (e.g., `if x % 3 == 1:`)
- ✅ **Compile-time Constants** - Define constants with `const` keyword for compile-time evaluation
- ✅ **Entity Selector Definitions** - Create custom selector aliases (e.g., `@Player = @a[type=player]`)
- ✅ **File Import System** - Import functions and definitions from other `.cbl` files
- ✅ **Module-level Variables** - Top-level assignments automatically initialized at pack load
- ✅ **Numeric Range Warnings** - Compile-time warnings for float precision and overflow
- ✅ **Modern CLI** - Full-featured command-line interface with watch mode and ZIP creation
- ✅ **Project Management** - Configuration via `cobble.toml`
- ✅ **Correct Command Format** - Follows Minecraft data pack specifications (no slash prefix)
- ✅ **JSON Safety** - Preserves JSON commands without breaking syntax
- ✅ **Command Tree Validation** - Validate generated `.mcfunction` files against Minecraft Java Edition 26.1.2 commands
- ✅ **Nested If Optimization** - Automatically splits complex control flow
- ✅ **Comprehensive Tests** - Extensive unit, integration, fixture, validation, and source-map coverage
- ✅ **Modern Parser** - Built with chumsky combinator library for reliability
- ✅ **Beautiful Errors** - Clear error messages powered by ariadne

## 📦 Installation

### From Source

```bash
# Clone the repository
git clone https://github.com/deveworld/cobble.git
cd cobble

# Build with Cargo
cargo build --release

# Binary will be in target/release/cobble
```

### Add to PATH (Optional)

```bash
# Linux/macOS
export PATH="$PATH:/path/to/cobble/target/release"

# Or copy to system bin
sudo cp target/release/cobble /usr/local/bin/
```

## 🚀 Quick Start

### 1. Create a New Project

```bash
cobble init --name my-datapack
cd my-datapack
```

This creates:

```text
my-datapack/
├── cobble.toml      # Project configuration
├── src/
│   └── main.cbl     # Main source file
└── .gitignore
```

### 2. Write Your Code

Edit `src/main.cbl`:

```python
import stdlib
from stdlib import event

def init():
    """Initialize the data pack"""
    /scoreboard objectives add score dummy "Score"
    /tellraw @a {"text":"Data pack loaded!", "color":"green"}

def on_tick():
    """Called every game tick"""
    as @a at @s:
        /particle minecraft:happy_villager ~ ~2 ~ 0.5 0.5 0.5 0 1

def give_reward(player, amount):
    """Give reward to player using macro parameters"""
    /give {player} minecraft:diamond {amount}
    /tellraw {player} {"text":"You received diamonds!", "color":"gold"}

# Register event handlers
stdlib.addEventListener(event.LOAD, init)
stdlib.addEventListener(event.TICK, on_tick)
```

### 3. Build the Data Pack

```bash
# Build to output directory
cobble build

# Build with ZIP
cobble build --zip
```

### 4. Use in Minecraft

Copy the output folder to your Minecraft world's `datapacks` directory:

```text
.minecraft/saves/YourWorld/datapacks/
```

## 📖 Language Guide

### Functions

Define functions with Python-style syntax:

```python
def function_name(param1, param2):
    """Documentation string"""
    /give {param1} minecraft:diamond {param2}
    /tellraw @a {"text":"Message"}
```

**Parameter Substitution:**

- Use `{param}` syntax directly for macro parameters
- Cobble convert it to the `$()` syntax for function parameters

### Variables and Type System

Cobble has a **static type system** where variable types are inferred from their first assignment and cannot change:

```python
# Module-level variables (initialized automatically at pack load)
score = 0           # Type: Integer
active = True       # Type: Boolean
player_count = 5    # Type: Integer

def my_function():
    # Local variables (initialized when function is called)
    temp = 100      # Type: Integer
    result = temp * 2  # Type: Integer (arithmetic result)

    # Type error: cannot change type
    # score = True  # ERROR: cannot reassign integer to boolean
```

**Type System Features:**

- **Automatic type inference** - Types are inferred from first assignment
- **Immutable types** - Variables cannot change their type
- **Compile-time checking** - Type errors are caught before generating the data pack
- **Expression types** - Arithmetic operations return Integer, comparisons return Boolean

**Module-level variables** are automatically initialized in the `_cobble_init` function when the data pack loads.

### Compile-time Constants

Define constants that are evaluated at compile time:

```python
const MAX_HEALTH = 100
const PLAYER_SPEED = 2
const GAME_DURATION = 300

def setup():
    health = MAX_HEALTH  # Replaced with 100 at compile time
    /say Game duration: {GAME_DURATION}
```

Constants are replaced with their values during compilation, resulting in optimized code.

### Entity Selector Definitions

Create custom selector aliases for cleaner, more maintainable code:

```python
# Define custom selectors
@Player = @a[type=player,gamemode=survival]
@Admin = @a[tag=admin]
@Boss = @e[type=zombie,tag=boss]

def give_rewards():
    as @Player:
        /give @s diamond 5

    as @Admin:
        /tellraw @s {"text":"Admin panel opened"}

def buff_boss():
    as @Boss:
        /effect give @s strength 999999 2
```

Selector aliases are replaced at compile time with zero runtime overhead.

### File Import System

Import functions and definitions from other `.cbl` files:

```python
# utils.cbl
def helper_function():
    /say Helper called

@Admin = @a[tag=admin]

# main.cbl
import utils  # Imports utils.cbl

def test():
    helper_function()  # Use imported function
    as @Admin:         # Use imported selector
        /say Hello admin
```

Features:

- Relative import resolution
- Circular import errors with the import chain
- Missing import errors with the importing file and expected path
- Functions and selectors are merged into current namespace

### Minecraft Commands

Direct Minecraft commands start with `/`:

```python
def spawn_villager():
    /summon minecraft:villager ~ ~ ~
    /effect give @e[type=villager,distance=..5] minecraft:glowing 30
```

### Control Flow

#### If Statements

```python
def check_score():
    if score >= 10:
        /say High score!
        /advancement grant @p only namespace:achievement

    # Complex expressions in conditions
    x = 10
    if x % 3 == 1:
        /say x mod 3 equals 1

    # Multiple conditions with AND/OR
    if score >= 10 and x % 2 == 0 or score >= 20:
        /say Complex condition met!
```

#### For Loops

```python
def spawn_multiple():
    # Basic loop
    for i in range(5):
        /summon minecraft:pig ~i ~ ~

    # Loop with step
    for i in range(10) by 2:
        /say Count by 2: {i}

    # Countdown with negative step
    for i in range(10) by -1:
        /say Countdown by -1: {i}
```

#### While Loops

```python
def countdown():
    counter = 10
    while counter > 0:
        /tellraw @a {counter}
        counter = counter - 1
```

#### Match/Switch Statements

Efficient multi-way branching based on integer values:

```python
def check_score():
    score = 75

    match score:
        case 0 to 59:
            /say Grade: F
        case 60 to 69:
            /say Grade: D
        case 70 to 79:
            /say Grade: C
        case 80 to 89:
            /say Grade: B
        case 90 to 100:
            /say Grade: A
        case _:
            /say Invalid score

def handle_difficulty(level):
    match level:
        case 1:
            /difficulty easy
        case 2:
            /difficulty normal
        case 3:
            /difficulty hard
        case _:
            /say Invalid difficulty level
```

Features:

- **Literal matching**: `case 5:` - matches exactly 5
- **Range matching**: `case 1 to 10:` - matches values from 1 to 10 (inclusive)
- **Wildcard pattern**: `case _:` - matches anything not matched by previous cases
- **Overlap validation**: Compiler ensures case ranges don't overlap (prevents bugs)
- Uses efficient 4-way split algorithm for optimal branching

### Arithmetic Operations

Cobble supports full arithmetic operations with proper operator precedence:

```python
def calculations():
    a = 10
    b = 5
    c = 3

    # Simple operations
    sum = a + b          # 15
    diff = a - b         # 5
    product = a * b      # 50
    quotient = a / b     # 2
    remainder = a % c    # 1 (modulo)
    power = b ^ 2        # 25 (exponentiation)

    # Multi-operator expressions
    result1 = a + b + c          # (a + b) + c = 18
    result2 = a * b * c          # (a * b) * c = 150

    # Operator precedence
    result3 = a + b * c          # a + (b * c) = 25, NOT (a + b) * c
    result4 = a * b + c          # (a * b) + c = 53
    result5 = a % c + b          # (a % c) + b = 6
    result6 = b ^ 2 * c          # (b ^ 2) * c = 75

    # Loop variable arithmetic
    for i in range(5):
        x = i * 10       # Uses correct loop_counter objective
        y = i + 5        # Loop variables work in all operations

    # Complex expressions in conditions
    if a % 3 == 1:       # Use arithmetic directly in conditions
        /say Modulo check passed
```

**Operator Precedence** (highest to lowest):

1. `^` - Power/exponentiation
2. `*`, `/`, `%` - Multiplication, division, and modulo
3. `+`, `-` - Addition and subtraction
4. `==`, `!=`, `<`, `<=`, `>`, `>=` - Comparisons

**Implementation Details**:

- Simple operations compile to optimized scoreboard commands
- Complex expressions use temporary variables automatically
- Power operator uses compile-time expansion (e.g., `x^3` becomes `x*x*x`)
- Modulo and division use temporary helper variables (`modulus`, `divisor`)
- Loop variables correctly track their objective (e.g., `loop_counter`)
- Arithmetic expressions in conditions are automatically evaluated to temporary variables

### Imports and Modules

Import from the standard library or other `.cbl` files:

```python
# Standard library imports
import stdlib
from stdlib import event

# File imports (imports from other .cbl files)
import utils      # Imports utils.cbl
import helpers    # Imports helpers.cbl

# Use imported functions and selectors
stdlib.addEventListener(event.LOAD, my_function)
helper_function()  # From utils.cbl
```

See [File Import System](#file-import-system) for more details on importing from `.cbl` files.

### Standard Library Helpers

Cobble 0.6.1 includes compiler-backed helpers for common data pack tasks:

```python
def utilities():
    text.tellraw("@a", {"text": "Hello", "color": "green"})
    score.set("points", 10)
    random.int("roll", 1, 6)
    timer.tick("cooldown")
    storage.set("status", {"ready": True})
    score.objective.add("points", "dummy", "Points")
    bossbar.add("timer", "Timer")
    schedule.once("utilities", "5s")
    root = math.sqrt(100)
```

Supported helper modules:

- `text` - `tellraw`, `title`, `subtitle`, `actionbar`
- `score` - `set`, `add`, `remove`, `reset`, `copy`, `operation`
- `random` - `int`, `bool`
- `timer` - `set`, `tick`, `done`, `reset`
- `storage` - `set`, `merge`, `remove`, `copy`, `append`, `prepend`, `insert`, `get`, `read_score`, `copy_from`
- `score.objective` - `add`, `remove`, `display`
- `schedule` - `once`, `clear`
- `bossbar` - `add`, `remove`, `set_value`, `set_max`, `set_name`, `set_color`, `set_style`, `set_visible`, `set_players`
- `team` - `add`, `remove`, `join`, `leave`, `modify`
- `entity` - `tag_add`, `tag_remove`, `effect_give`, `effect_clear`, `attribute_get`, `attribute_base_set`
- `math` - `sqrt`, `abs`, `min`, `max`

### Data Pack JSON Resources

Top-level `datapack.*` declarations generate common JSON resources:

```python
datapack.function_tag("utility", ["my_pack:setup"])
datapack.function_tag("minecraft:load", ["my_pack:setup"])
datapack.predicate("is_sneaking", {
    "condition": "minecraft:entity_properties",
    "entity": "this",
    "predicate": {"flags": {"is_sneaking": True}}
})
datapack.dialog("notice", {
    "type": "minecraft:notice",
    "title": {"text": "Notice"}
})
```

Supported resources include function/block/item/entity type tags, predicates,
advancements, loot tables, recipes, item modifiers, and dialogs. Resource names
can use nested paths and optional explicit namespaces such as
`other_namespace:checks/is_ready`.

### Event System

Register functions to run on specific events:

```python
from stdlib import event

def on_load():
    /say Data pack loaded!

def on_tick():
    # Runs 20 times per second
    pass

stdlib.addEventListener(event.LOAD, on_load)
stdlib.addEventListener(event.TICK, on_tick)
```

## 🛠️ CLI Commands

### `cobble init [OPTIONS]`

Initialize a new Cobble project.

```bash
cobble init                     # In current directory
cobble init --name my-project   # Create new directory named 'my-project'
cobble init --template minimal  # Use minimal, stdlib, or validation template
```

**Options:**

- `--name <NAME>` - Project name (creates a new directory if specified)
- `--description <DESCRIPTION>` - Project description
- `--pack-format <FORMAT>` - Pack format version (default: 101.1)
- `--template <NAME>` - Project template: `minimal`, `stdlib`, or `validation`

### `cobble build [input] [options]`

Build the data pack from source files.

```bash
cobble build                  # Use cobble.toml settings
cobble build src/             # Build specific directory
cobble build -o dist          # Custom output directory
cobble build --zip            # Create ZIP file
cobble build --validate       # Validate generated commands
```

Options:

- `-o, --output <dir>` - Output directory
- `--zip` - Create ZIP archive
- `--validate` - Validate generated `.mcfunction` files against Minecraft Java Edition 26.1.2 commands
- `--commands-json <path>` - Path to the exported command tree (default: `data/commands.json`; auto-generated when missing)

The default `data/commands.json` is generated automatically on first validation.
This requires `curl` and Java because Cobble downloads the Minecraft server jar
and runs the server reports generator. For custom paths, generate the file
manually and copy it to the requested path:

```bash
scripts/setup_commands_json.sh 26.1.2
cp data/commands.json /tmp/commands.json
```

If Mojang's manifest endpoint is blocked on your network, Cobble also tries the
legacy launcher manifest host and a pinned 26.1.2 server jar URL. You can
override the source explicitly:

```bash
COBBLE_COMMANDS_JSON_URL=https://example.com/commands.json cobble build --validate
COBBLE_MINECRAFT_SERVER_URL=https://example.com/server.jar cobble build --validate
COBBLE_MINECRAFT_SERVER_JAR=/path/to/server.jar cobble build --validate
COBBLE_MINECRAFT_SERVER_SHA1=<sha1> COBBLE_MINECRAFT_SERVER_URL=https://example.com/server.jar cobble build --validate
```

### `cobble watch [input] [options]`

Watch files and rebuild on changes.

```bash
cobble watch                  # Watch project
cobble watch src/             # Watch specific directory
cobble watch -o output        # With custom output
cobble watch --validate       # Validate after each rebuild
```

### `cobble check [input]`

Check syntax without building.

```bash
cobble check                  # Check all project files
cobble check src/main.cbl     # Check specific file
```

## ⚙️ Configuration

`cobble.toml` configures your project:

```toml
[project]
name = "my-datapack"
description = "My awesome data pack"
namespace = "my_namespace"
version = "1.0.0"
pack_format = "101.1"  # Minecraft Java Edition 26.1.2

[build]
source = "src"         # Source directory
output = "output"      # Output directory
entry_points = []      # Main files to compile; imported files are pulled in by those entries
```

### Supported Minecraft Version

| Minecraft Version | Pack Format |
| ----------------- | ----------- |
| 26.1.2 | 101.1 (required) |

> **Note**: Cobble v0.6.1 exclusively supports **Minecraft Java Edition 26.1.2** (pack format 101.1). No backward compatibility with older versions is provided. This allows us to leverage the latest Minecraft features without worrying about legacy constraints.

## 📁 Project Structure

```text
my-datapack/
├── cobble.toml           # Configuration
├── src/
│   ├── main.cbl         # Main entry point
│   ├── events.cbl       # Event handlers
│   ├── functions.cbl    # Utility functions
│   └── entities/
│       └── mobs.cbl     # Mob-related functions
├── output/              # Generated data pack
│   ├── pack.mcmeta
│   └── data/
│       └── namespace/
│           ├── function/
│           └── tags/
└── my-datapack.zip      # Distributable pack
```

## 🎮 Complete Example

### Boss Fight System

`src/boss.cbl`:

```python
import stdlib
from stdlib import event

# Global variables
boss_health = 100
phase = 1

def spawn_boss():
    """Spawn the boss entity"""
    /summon minecraft:wither_skeleton ~ ~1 ~ {CustomName:'{"text":"Dark Lord","color":"dark_red","bold":true}',Health:200f,attributes:[{id:"minecraft:max_health",base:200.0d}],HandItems:[{id:"minecraft:netherite_sword",Count:1b},{}],ArmorItems:[{},{},{},{id:"minecraft:dragon_head",Count:1b}]}
    /bossbar add namespace:boss {"text":"Dark Lord"}
    /bossbar set namespace:boss players @a
    /bossbar set namespace:boss max 200
    /bossbar set namespace:boss value 200
    /bossbar set namespace:boss color red

def boss_tick():
    """Boss fight logic - runs every tick"""
    global phase

    # Update boss bar
    /execute store result bossbar namespace:boss value run data get entity @e[type=wither_skeleton,name="Dark Lord",limit=1] Health

    # Phase transitions
    if boss_health <= 50 and phase == 1:
        phase = 2
        enter_phase_2()

def enter_phase_2():
    """Boss enters rage mode"""
    /tellraw @a {"text":"The Dark Lord enters rage mode!","color":"red","bold":true}
    /effect give @e[type=wither_skeleton,name="Dark Lord"] minecraft:strength 999999 1
    /effect give @e[type=wither_skeleton,name="Dark Lord"] minecraft:speed 999999 0

    # Spawn minions
    for i in range(3):
        asat @s:
            /summon minecraft:zombie ~ ~ ~ {IsBaby:0b,ArmorItems:[{},{},{},{id:"minecraft:iron_helmet",Count:1b}]}

def boss_defeated():
    """Called when boss is defeated"""
    /bossbar remove namespace:boss
    /tellraw @a {"text":"Victory! The Dark Lord has been defeated!","color":"gold","bold":true}
    /advancement grant @a only namespace:defeat_boss
    asat @s:
        /summon minecraft:firework_rocket ~ ~1 ~ {FireworksItem:{id:"minecraft:firework_rocket",Count:1,tag:{Fireworks:{Flight:2,Explosions:[{Type:1,Colors:[I;16711680,16776960],FadeColors:[I;16777215]}]}}}}

# Register events
stdlib.addEventListener(event.TICK, boss_tick)
```

### Parkour System

`src/parkour.cbl`:

```python
def create_checkpoint(x, y, z):
    """Create a checkpoint at coordinates"""
    /summon minecraft:armor_stand {x} {y} {z} {Invisible:1b,Marker:1b,CustomName:'{"text":"Checkpoint"}'}
    /particle minecraft:end_rod {x} {y} {z} 0.5 1 0.5 0.01 20

def on_checkpoint():
    """Player reaches checkpoint"""
    /spawnpoint @p ~ ~ ~
    /playsound minecraft:entity.player.levelup master @p
    /title @p subtitle {"text":"Checkpoint Saved!","color":"green"}
    /title @p title ""

def reset_player():
    """Teleport player to last checkpoint"""
    /tp @p @e[type=armor_stand,name="Checkpoint",limit=1,sort=nearest]
    /effect give @p minecraft:resistance 3 255 true
```

## 🧪 Testing

Run the test suite:

```bash
cargo test
```

Check syntax of example files:

```bash
cobble check examples/
```

## 🤝 Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

1. Fork the repository
2. Create your feature branch (`git checkout -b feature/AmazingFeature`)
3. Commit your changes (`git commit -m 'Add some AmazingFeature'`)
4. Push to the branch (`git push origin feature/AmazingFeature`)
5. Open a Pull Request

### Development Setup

```bash
# Clone and setup
git clone https://github.com/deveworld/cobble.git
cd cobble

# Install development dependencies
cargo install cargo-watch

# Run in watch mode
cargo watch -x test -x "run -- check examples/"
```

## 📚 Documentation

- [Language Reference](docs/language.md)
- [CLI Documentation](docs/cli.md)
- [API Reference](docs/api.md)
- [Examples](examples/)

## 🐛 Known Limitations

- **Variable Scope**: All variables are effectively global due to Minecraft's scoreboard architecture. The `global` keyword is accepted for code clarity but has no functional effect. Variables defined in one function can affect variables in another function if they share the same name.
- For loops only support `range()` iterators
- **Minecraft Java Edition 26.1.2 Required**: Cobble exclusively supports Minecraft Java Edition 26.1.2 (pack format 101.1). Older versions are not supported.
- No array/list data structures yet
- **While loops**: Execute all iterations in a single game tick, which can cause server lag with large iteration counts (>100)

## 🗺️ Roadmap

### Recently Completed

#### v0.6.1 (2026-06-02)

- [x] **Validation diagnostics** - Macro commands are counted explicitly and invalid commands now include caret-position diagnostics when available
- [x] **Standard library v1.1** - Added objective, storage list/read, schedule, bossbar, team, and entity helper commands
- [x] **Namespaced resources** - `datapack.*` declarations can emit explicit `namespace:path` resources and reject non-object JSON resources where Minecraft expects objects
- [x] **Project templates** - `cobble init --template minimal|stdlib|validation` creates more targeted starter projects
- [x] **Server QA workflow** - Added `scripts/test_minecraft_server.sh` for the ignored real-server integration test

#### v0.6.0 (2026-06-01)

- [x] **Build validation workflow** - `cobble build --validate` and `cobble watch --validate` validate generated `.mcfunction` files against Minecraft Java Edition 26.1.2 commands
- [x] **Standard library v1** - Added compiler-backed `text`, `score`, `random`, `timer`, `storage`, and non-placeholder `math.sqrt` helpers
- [x] **Data pack JSON resources** - Added `datapack.*` helpers for tags, predicates, advancements, loot tables, recipes, item modifiers, and dialogs
- [x] **Generated command source maps** - Generated packs include `.cobble/source_map.json` with command text, generated location, source location when available, and command kind
- [x] **Project stability** - Circular imports now fail with import chains, missing imports include the importing file, and example projects are covered by build-and-validate fixtures

#### v0.5.18 (2025-10-21)

- [x] **Performance optimization** - Removed unnecessary scoreboard objectives (multiplier, divisor, modulus, power_base, expr_temp) - now uses only temp objective with fake players
- [x] **Enhanced /say command** - Scoreboard variables in /say commands are automatically converted to tellraw with proper score display components
- [x] **Cleaner datapacks** - Generated packs now have fewer objectives in the objectives list, reducing clutter

#### v0.5.17 (2025-01-19)

- [x] **Return statement error handling** - Return statements now produce clear compile-time errors instead of being silently ignored
- [x] **Function call assignment validation** - Assignments like `x = helper()` now error with helpful messages explaining Minecraft limitations
- [x] **Enhanced expression errors** - Attribute, subscript, and None assignments now have explicit error handling
- [x] **Comprehensive regression tests** - Added 7 new tests to prevent these bugs from reoccurring (102 total tests)

#### v0.5.16 (2025-01-09)

- [x] **Critical bug fixes** - Fixed three major issues that prevented proper Minecraft functionality
- [x] **Range syntax parsing** - Fixed tokenizer to correctly parse Minecraft range syntax (`1..`, `..5`, `1..5`)
- [x] **Circular import detection** - Restored proper circular dependency detection that was broken in v0.5.15
- [x] **Python expression translation** - Execute blocks now properly translate Python expressions to Minecraft conditions

#### v0.5.6 (2025-10-04)

- [x] **Execute modifiers fix** - All execute modifiers (positioned, in, rotated, etc.) now work as first modifier
- [x] **Unary operators** - Negation (-x) and positive (+x) operators now work correctly
- [x] **Macro parameters in /say** - Function parameters can be used in /say commands (e.g., `/say Count: {counter}`)
- [x] **For loop macro support** - Loop variables work in /say commands (e.g., `/say Number {i}`)
- [x] **If condition fixes** - Boolean literals (True/False) now generate correct execute conditions
- [x] **For loop error handling** - Clear error messages for unsupported variable ranges instead of silent failure

#### v0.5.5 (2025-10-03)

- [x] **Inline comments** - Support for # comments on same line as code
- [x] **Execute modifier improvements** - Initial support for various execute modifiers
- [x] **Parser enhancements** - Better error recovery and reporting

#### v0.5.0 (2025-10-03)

- [x] **Type System** - Static, immutable type system with compile-time inference
- [x] **Type checking** - Prevents accidental type changes (e.g., overwriting score with boolean)
- [x] **Match validation** - Compile-time detection of overlapping case ranges
- [x] **Boolean initialization fix** - Module-level boolean variables now properly initialized
- [x] **Numeric warnings** - Float precision and overflow warnings at compile time
- [x] **Documentation updates** - New Type System section in language reference

#### v0.4.3 (2025-10-03)

- [x] **Critical bug fix** - Fixed execute block keyword capitalization (if/unless were incorrectly capitalized)
- [x] **Token Display trait fix** - Added explicit lowercase mappings for all keywords
- [x] **Enhanced test coverage** - 58 integration tests with regression tests for execute chains
- [x] **Documentation improvements** - Added division by zero warnings and safe usage patterns

#### v0.4.2 (2025-10-03)

- [x] **Critical bug fixes** - Fixed nested OR operators, NOT+OR combination, and match wildcard cases
- [x] **OR operator improvements** - Recursive flattening for complex OR expressions
- [x] **Match wildcard fixes** - Proper conditional execution for single and multi-statement wildcards
- [x] **Comprehensive testing** - 58 integration tests, all passing

#### v0.4.1 (2025-10-03)

- [x] **Loop variable macro support** - Use loop variables directly in commands (e.g., `/say Count: {i}`)
- [x] **Loop body as macro functions** - Loop bodies compile to macro functions for variable access
- [x] **Function call fix** - Fixed parameterless function calls (no longer incorrectly use `with storage`)
- [x] **Parser improvements** - Fixed `by` keyword recognition in for loops

#### v0.4.0 (2025-10-03)

- [x] **Entity Selector Definitions** - Custom selector aliases (e.g., `@Player = @a[type=player]`)
- [x] **File Import System** - Import functions and definitions from other `.cbl` files
- [x] **Circular dependency prevention** - Automatic detection and prevention
- [x] **Relative import resolution** - Import files relative to current file location

#### v0.3.0 (2025-10-03)

- [x] **Compile-time constants** - `const` keyword for compile-time evaluation
- [x] **Match/switch statements** - Efficient multi-way branching
- [x] **Literal matching** - Match exact values (e.g., `case 5:`)
- [x] **Range matching** - Match value ranges (e.g., `case 1 to 10:`)
- [x] **Wildcard pattern** - Default case with `case _:`
- [x] **4-way split algorithm** - Optimized branching implementation

#### v0.2.2 (2025-10-02)

- [x] **Critical bug fixes** - If/elif/else inlining bug, while loop condition bug
- [x] **Automatic gamerule configuration** - `maxCommandChainLength` set automatically
- [x] **Module variable initialization order** - Proper command ordering

#### v0.2.1 (2025-10-02)

- [x] **Complex expressions in conditions** - Use arithmetic directly in if/while (e.g., `if x % 3 == 1:`)
- [x] **Automatic temporary variables** - Unique variables for each expression in conditions
- [x] **Nested expression support** - Works with AND/OR operators

#### v0.2.0 (2025-10-02)

- [x] **Modulo operator (%)** - Compute remainders (e.g., `x % y`)
- [x] **Power operator (^)** - Exponentiation with compile-time expansion (e.g., `x ^ 2`)
- [x] **OR operator** - Boolean OR in conditions (e.g., `if x == 5 or y == 10:`)
- [x] **For loop step support** - Control increment/decrement (e.g., `for i in range(10) by 2:`)
- [x] **Java Edition compatibility** - Fixed negative loop steps

#### v0.1.1 (2025-10-02)

- [x] **Parenthesized expressions** - Support for `(a + b) * c` style expressions
- [x] **Self-assignment optimization** - Removed unnecessary operations

#### v0.1.0 (Pre-release)

- [x] elif and else branch support
- [x] Scoreboard objectives auto-generation
- [x] User function calls
- [x] **Multi-operator expressions** - Chained operations like `a + b + c`
- [x] **Operator precedence** - Proper math order (`*`/`/` before `+`/`-`)
- [x] **Complex nested expressions** - Full arithmetic expression support
- [x] **Module-level variable initialization** - Top-level vars auto-initialized
- [x] **Loop variable arithmetic fix** - Correct objective tracking in loops
- [x] **Variable division** - Full division operation support
- [x] **CLI watch enhancements** - All build options available in watch mode
- [x] **Boolean operators** - `and`, `not` operators in conditions
- [x] chumsky parser integration
- [x] Improved error messages with ariadne
- [x] as/at/asat/if execute support
- [x] global keyword
- [x] Comprehensive test suite (58 integration tests)

### Near Term

- [ ] **Template functions** - Parameterized code generation (e.g., `def summon[entity]: /summon {entity} ~ ~ ~`)
- [ ] Array and list data structures
- [ ] More built-in functions (math, strings)
- [ ] Enhanced loop optimizations

### Medium Term

- [ ] Class and object support for entities
- [ ] NBT data manipulation
- [x] Custom advancement generation
- [x] Recipe generation
- [x] Loot table generation
- [x] Predicate support

### Long Term

- [ ] Resource pack integration
- [ ] VS Code extension with syntax highlighting
- [ ] Language server protocol (LSP) support
- [ ] Online playground/REPL
- [ ] Standard library expansion

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Acknowledgments

- Minecraft command system documentation
- Rust community for excellent libraries:
  - [chumsky](https://github.com/zesterer/chumsky) - Parser combinator library
  - [ariadne](https://github.com/zesterer/ariadne) - Beautiful error reporting
  - [clap](https://github.com/clap-rs/clap) - Command-line argument parsing

## Support

- **Issues**: [GitHub Issues](https://github.com/deveworld/cobble/issues)
- **Discussions**: [GitHub Discussions](https://github.com/deveworld/cobble/discussions)

---

Made with ❤️ for the Minecraft community
