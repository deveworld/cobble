# Cobble Language Reference

Cobble is a high-level, Python-inspired language that compiles to Minecraft data packs. It brings modern programming features to Minecraft command development.

## Table of Contents

- [Basic Syntax](#basic-syntax)
- [Data Types](#data-types)
- [Variables](#variables)
- [Functions](#functions)
- [Control Flow](#control-flow)
- [Minecraft Commands](#minecraft-commands)
- [Standard Library](#standard-library)
- [Events](#events)

## Basic Syntax

Cobble uses Python-style indentation for code blocks. No braces or semicolons required!

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

## Functions

### Function Definition

```python
def greet():
    """Greet all players"""
    /tellraw @a {"text":"Hello!", "color":"green"}
```

### Functions with Parameters (Minecraft 1.21.7+)

Cobble supports function parameters using Minecraft's macro system:

```python
def give_reward(player, amount):
    """Give a reward to a player"""
    /give {player} minecraft:diamond {amount}
    /tellraw {player} {"text":"You received diamonds!", "color":"gold"}
```

**Important**: Use `{param_name}` syntax to use `$(param_name)` in commands for function parameters. This is Minecraft's macro syntax (1.20.2+).

### Calling Functions

```python
def main():
    greet()
    give_reward("Steve", 5)
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

### Boolean Operators

Cobble supports boolean operators for combining multiple conditions:

```python
def check_conditions():
    x = 5
    y = 10

    # AND operator - both conditions must be true
    if x > 0 and y < 15:
        /say Both conditions are true!

    # NOT operator - negates the condition
    if not x == 10:
        /say x is not equal to 10!

    # Complex combinations
    a = 10
    b = 20
    c = 30
    if a > 5 and b < 25 and not c == 40:
        /say Complex condition met!
```

**Transpilation Details:**
- `and` operator chains conditions using `execute if ... if ...`
- `not` operator converts `if` to `unless` (or vice versa)
- Double negatives are automatically simplified (`not not x == 5` → `if score x temp matches 5`)

**Example:**
```python
if x > 0 and y < 15:
    /say test
```
Transpiles to:
```mcfunction
execute if score x temp matches 1.. if score y temp matches ..14 run say test
```

**Note:** Boolean operators are only supported in regular `if` and `while` statements. Execute blocks (`as`, `at`, `asat`) use raw Minecraft syntax for their `if` modifiers:

```python
def check_players():
    # Execute blocks use raw Minecraft syntax
    as @a if entity @s[tag=special]:
        /say Special player!

    # Use regular if statements for Python boolean expressions
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

**Note:** The `or` operator is not yet implemented as it requires complex branching logic. Use separate if statements or nested conditions as a workaround.

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
```

For loops are compiled into recursive functions with automatic loop counters.

### While Loops

```python
def count_down():
    counter = 10
    while counter > 0:
        /say @a Count: {count}
        counter = counter - 1
```

While loops are also compiled into recursive functions.

**⚠️ Important Performance Warning**: While loops execute all iterations in a single game tick. This can cause severe server lag or crashes with large iteration counts (>100). For long-running operations, consider using:
- Scheduled functions with `/schedule`
- Tick-based iteration (use a tick event handler that runs incrementally)
- For loops with known small iteration counts

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

Use Minecraft's macro syntax `{name}` for function parameters:

```python
def teleport_player(player, x, y, z):
    /tp {player} {x} {y} {z}
    /tellraw {player} {"text":"Teleported!", "color":"green"}
```

## Standard Library

Import the standard library to access event handling:

```python
import stdlib
from stdlib import event
```

### Event Types

- `event.LOAD` - Runs when the data pack is loaded
- `event.TICK` - Runs every game tick (20 times per second)

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
    /give $(player) minecraft:diamond $(amount)

    # Give to a literal player named "Steve"
    /give Steve minecraft:gold_ingot 1

    # Use in JSON with parameters
    /tellraw $(player) {"text":"You got items!", "color":"gold"}
```

### Arithmetic Operations

Cobble supports comprehensive arithmetic with proper operator precedence:

```python
def calculate():
    a = 10
    b = 5
    c = 3

    # Basic operations
    sum = a + b          # Addition: 15
    diff = a - b         # Subtraction: 5
    product = a * b      # Multiplication: 50 (uses multiplier helper)
    quotient = a / b     # Division: 2 (uses divisor helper)

    # Multi-operator expressions
    result1 = a + b + c          # Chain addition: 18
    result2 = a - b - c          # Chain subtraction: 2
    result3 = a * b * c          # Chain multiplication: 150

    # Operator precedence (follows standard math rules)
    result4 = a + b * c          # Evaluates as: a + (b * c) = 25
    result5 = a * b + c          # Evaluates as: (a * b) + c = 53
    result6 = a - b / c          # Evaluates as: a - (b / c) = 9

    # Complex expressions
    complex = a + b * c - d / e  # Full precedence support
```

**Operator Precedence** (highest to lowest):
1. `*`, `/` - Multiplication and division (left to right)
2. `+`, `-` - Addition and subtraction (left to right)
3. `==`, `!=`, `<`, `<=`, `>`, `>=` - Comparisons

**Important Notes**:
- Operators follow standard mathematical precedence
- Multiplication and division use temporary scoreboard objectives (`multiplier`, `divisor`)
- Complex expressions automatically use `expr_temp` for intermediate results
- All operations work with both constants and variables
- Loop variables (like `i` in `for i in range(5)`) use the correct objective (`loop_counter`)

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

Cobble requires **Minecraft 1.21.7+** (minimum pack format 81) and defaults to **Minecraft 1.21.9** (pack format 88). Key features:

- **Macros**: Function parameters use the macro system introduced in 1.20.2+
- **Modern commands**: Uses latest command syntax
- **Data packs**: Full data pack specification support

## Limitations

- No support for classes (yet)
- No support for lists/arrays (yet)
- Boolean `and` and `not` operators are supported; `or` operator not yet implemented
- Function parameters require Minecraft 1.20.2+ for macro support
- For loops only support `range()` iterators
- While loops compile to recursive functions (performance impact for very long loops)

## Further Reading

- [CLI Documentation](cli.md)
- [API Documentation](api.md)
- [Examples](../examples/)
- [Minecraft Wiki - Data Packs](https://minecraft.wiki/w/Data_pack)