# Control Flow

The examples on this page use the namespace `mypack`.

## If, Elif, And Else

```python
def check_score(score):
    if score >= 10:
        /say You have enough points!
        /give @p minecraft:diamond
    elif score >= 5:
        /say Almost there.
    else:
        /say You need more points!
```

Comparisons are `==`, `!=`, `>`, `>=`, `<`, and `<=`. Either side can be an
arithmetic expression or a call to a function that returns an integer:

```python
def has_key():
    return 1

def check_value(a, b):
    if a % 5 == 2 and b ^ 2 == 25:
        /say Both conditions are true!

    if has_key() and not a == 0:
        /say Function calls can drive conditions
```

Cobble evaluates complex operands into temporary scores before the comparison.
A nested `if` inside an `if` compiles to a helper function when its body needs
more than one command.

## Boolean Operators

`and`, `or`, and `not` combine conditions in `if`, `elif`, `while`, and
execute-block `if`/`unless` modifiers:

```python
def check_conditions():
    x = 5
    y = 10

    if x > 0 and y < 15:
        /say test

    if x == 5 or y == 10:
        /say test

    if not x == 10:
        /say x is not 10
```

`and` chains `if` clauses on one `execute` command, `not` turns `if` into
`unless`, and `not not` cancels out. `or` sets a temporary flag that each
operand can raise. The two tests above compile to:

```mcfunction
execute if score x mypack.vars matches 1.. if score y mypack.vars matches ..14 run say test
scoreboard players set or_temp_0 mypack.vars 0
execute if score x mypack.vars matches 5 run scoreboard players set or_temp_0 mypack.vars 1
execute if score y mypack.vars matches 10 run scoreboard players set or_temp_0 mypack.vars 1
execute if score or_temp_0 mypack.vars matches 1 run say test
```

`not (a and b)` compiles with De Morgan's laws, and `or` can nest inside `and`
and the other way round.

In an execute-block modifier, a condition that starts with a Minecraft
condition keyword (`score `, `entity `, `block `, `data `, `predicate `, ...)
is passed through as raw Minecraft syntax. Other conditions go through the same
translation as `if` statements:

```python
def check_players():
    as @a if entity @s[tag=special]:
        /say Special player!
```

## Execute Blocks

Execute blocks run their body under Minecraft `execute` modifiers:

```python
def reward_new_players():
    as @a if score @s level matches 0:
        /scoreboard players set @s level 1
        /tellraw @s {"text":"Welcome!"}
```

A body that compiles to a single command is inlined
(`execute as @a if ... run tellraw ...`). A longer body is compiled into a
helper function called once per execution context
(`execute as @a if ... run function ns:execute_temp_N`), so the modifiers and
conditions are evaluated once per entity and the whole body runs for that
entity before the next one starts. Updating the condition's own score inside
the body, loops, `if`/`else`, and multi-step assignments therefore behave per
entity. Blocks with a `store` modifier keep one `execute store ...` command per
body command, because storing a helper call would capture only the helper's
return value; bodies containing a raw `/return` also keep that form so the
return still leaves the enclosing function.

Headers accept the same placeholders as raw commands: selector aliases,
`{CONST}` values, and `{param}` macro arguments, anywhere in the header.
SNBT and strings can appear in raw conditions too:

```python
const RADIUS = 24
@Spawn = @e[type=minecraft:marker,tag=spawn,limit=1]

def heal_near_spawn():
    at @Spawn as @a[distance=..{RADIUS}]:
        /effect give @s minecraft:regeneration 3 0 true

def use_wand():
    if items entity @s weapon.mainhand *[minecraft:custom_data~{wand:"frost"}]:
        /say frost
```

## For Loops

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

def runtime_list():
    values = [2, 4, 6]
    total = 0
    for value in values by 2:
        total = total + value

def runtime_list_path():
    groups = [[2, 3], [7, 11]]
    row = 1
    total = 0
    for value in groups[row]:
        total = total + value

def runtime_literal_array():
    first = 2
    total = 0
    for value in [first, first + 3, 5]:
        total = total + value
```

For loops over literal ranges and literal scalar arrays are expanded at compile
time and emit the repeated commands directly. `range(...)` loops with runtime
start, stop, count, or step expressions compile to scoreboard-backed recursive
helper functions. Runtime literal arrays such as `[first, first + 3]` are
materialized into temporary storage and then iterated as runtime lists. Loops
over storage-backed list expressions such as `for value in values:` or
`for value in groups[row]:` compile to an index loop that reads the list length
at runtime and copies each element into the loop variable before running the
body. `by` steps also work for runtime list loops; negative steps start at the
last item.

**Unrolling Support:**
- `range(n)` iterates `0..n`.
- `range(start, stop, step)` uses Python-style half-open bounds.
- `for i in range(n) by step:` remains supported for compatibility.
- Negative `by` over `range(n)` starts at `n - 1` and counts down.
- Runtime `range(...)` supports dynamic count/start/stop/step expressions.
- A statically known step of `0` is rejected. If a dynamic step expression
  evaluates to `0` at runtime, the loop executes zero iterations so the
  generated datapack does not recurse forever.
- Literal arrays containing only numbers, strings, or booleans are unrolled.
- Literal arrays containing runtime expressions are materialized and iterated
  at runtime.
- Runtime storage-backed list iteration supports list expressions/paths. The
  loop variable is mirrored as storage and as a score, so numeric list items
  can be used in arithmetic and any item can be displayed through
  storage-backed text output.
- Runtime list `by` steps support positive, negative, and dynamic integer
  expressions. A statically known step of `0` is rejected; a dynamic step that
  evaluates to `0` executes zero iterations.
- A single loop may expand up to 1024 iterations. Expansions above 256 emit a warning.
- Nested unrolling is capped at 65,536 aggregate iterations and one outer
  unroll may emit at most 65,536 generated commands.

Loop problems are reported with these diagnostic kinds, located at the `for`
statement:

| Kind | Severity | Reported when |
| --- | --- | --- |
| `unroll-limit-exceeded` | error | a loop expands past 1024 iterations, nested loops past 65,536 iterations, or one outer loop past 65,536 commands |
| `unroll-large-expansion` | warning | a loop expands past 256 iterations |
| `unroll-non-literal` | error | the iterable is none of: a literal array, a literal or runtime `range`, a runtime literal array, or a storage-backed list |
| `unroll-bad-step` | error | a `range` step is zero or non-integer |
| `unroll-bad-range` | error | a `range` stops below its start with a positive step, or the array is empty |
| `runtime-for-unsupported` | error | a runtime `range` has a non-integer bound or step, or a runtime list loop iterates something that is not a list |
| `runtime-for-bad-step` | error | a static runtime step is zero, or a step is non-integer |

## While Loops

```python
def count_down():
    counter = 10
    while counter > 0:
        /say @a Count: {counter}
        counter = counter - 1
```

A `while` loop compiles to a function that calls itself until the condition is
false. All iterations run within one game tick, so a loop with many iterations
can stall the server. Spread long work across ticks with a tick event handler
or `/schedule`.

## Match Statements

`match` branches on an integer value:

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

- `case 5:` matches exactly 5.
- `case 1 to 10:` matches 1 through 10, inclusive.
- `case _:` matches anything the earlier cases did not.

The first matching case runs and no other. Cases must not overlap; the compiler
reports an error such as:

```text
Match case overlap detected: case 3 to 7 overlaps with a previous case.

Previous case ended at: 5
Current case starts at: 3
```

Cobble compiles a match into a search tree of `execute if score ... matches
...` checks that splits the case ranges into quarters at each level. A case
body with one command is inlined; a longer body becomes a helper function.
