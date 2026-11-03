# Control Flow

## If, Elif, And Else

```python
def reward(score):
    if score >= 10:
        /give @s minecraft:diamond
    elif score >= 5:
        /say Almost there.
    else:
        /say You need more points.
```

Comparisons are `==`, `!=`, `<`, `<=`, `>`, and `>=`. Text, lists, and maps
also support equality and membership (`in`, `not in`). Chained comparisons
such as `0 < score <= 10` work; their middle operands cannot call functions.

## Boolean Operators

Use `and`, `or`, and `not` to combine conditions:

```python
def can_start(ready, lives):
    if ready and lives > 0:
        /say Starting
    if not ready:
        /say Waiting
```

Zero and empty strings or containers are false. Other values, including
negative numbers, are true. `and` and `or` short-circuit: the right side runs
only when needed. An `elif` is tested only if no earlier branch ran.

Minecraft conditions work in `if`, `elif`, and `while`, and can be mixed
with Cobble expressions:

```python
def reward_winners(level):
    if entity @a[tag=winner] and level > 3:
        /give @a[tag=winner] minecraft:diamond
    elif not entity @a[tag=winner]:
        /say No winners yet.
```

## Execute Blocks

An execute block applies Minecraft execution context to its body:

```python
def welcome():
    as @a[tag=new] at @s:
        /tellraw @s "Welcome!"
        /tag @s remove new
```

Headers accept Minecraft modifiers such as `as`, `at`, `if`, `unless`,
`positioned`, `rotated`, `in`, `anchored`, `align`, `store`, `on`, `facing`,
and `summon`. `asat @a:` is shorthand for `as @a at @s:`. A leading `execute`
is optional. Selectors and arguments use the same
[placeholders](commands.md#placeholders) as raw commands.

Minecraft selects the execution contexts before the body runs. To test a
changing value separately for each entity, put the condition inside the body:

```python
remaining = 1

def pick_one():
    global remaining
    as @a:
        if remaining > 0:
            /tag @s add picked
            remaining -= 1
```

Put membership tests and conditions that join text inside the block rather
than in the same header as `as`. A Cobble `return` in the body ends the
enclosing function and stops any remaining contexts.

## For Loops

```python
def spawn():
    for i in range(3):
        /summon minecraft:zombie ~ ~ ~

def count_by_twos():
    for i in range(1, 6, 2):
        /say {i}

def announce():
    for direction in ["north", "south"]:
        /say {direction}
```

`range(stop)` starts at zero; `range(start, stop, step)` excludes `stop`.
A negative step counts down. `for i in range(n) by step:` is also supported;
negative `by` starts at `n - 1`.

Lists and strings can be iterated at runtime:

```python
def total(values):
    result = 0
    for value in values:
        result += value
    return result
```

`enumerate(items)` supplies `(index, item)` pairs. The loop variable keeps
its last value after the loop. Forward list loops see items appended or
removed in place; rebinding the list does not change the list being walked.
A zero literal step is an error; a runtime zero step runs no iterations.

Literal ranges and scalar lists are expanded at build time. Large expansions
have [compiler limits](../reference/diagnostics.md#compiler-limits).
`break` exits the innermost loop; `continue` skips to its next iteration.
`for ... else` is not supported.

## While Loops

```python
def drain_queue():
    count = 0
    while entity @e[tag=queued] and count < 10:
        /tag @e[tag=queued,limit=1] remove queued
        count += 1
```

The complete condition is checked before each pass, including any function
calls. All loop iterations run in one game tick. Bound the work; use a
[tick handler](events.md) or `/schedule` to spread it across ticks.

Packs leave the world's command limit unchanged by default. If a pack needs
a higher limit, explicitly set `build.command_limit` in
[cobble.toml](../reference/config.md). This changes a world-wide gamerule on load.

## Match Statements

`match` supports numbers, inclusive ranges, and text. The first matching
case runs; numeric ranges must not overlap:

```python
def describe(score):
    match score:
        case 0:
            /say No score
        case 1 to 10:
            /say Getting started
        case _:
            /say High score
```

For text, use a quoted case such as `case "sword":`.
