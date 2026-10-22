# Variables

Numbers and booleans are fake players on the pack's variable objective,
`<namespace>.vars`. Strings, lists, and maps live in the storage
`<namespace>:global`. Each pack gets its own objective and storage, so two
Cobble packs can use the same variable names on one server. The examples on
this page use the namespace `mypack`.

## Module-level Variables

Variables defined at the top level (outside functions) are set in the
`_cobble_init` function every time the data pack loads, which includes
`/reload` and server starts:

```python
score = 0
lives = 3
max_health = 20
```

## Persistent Variables

Declare a module-level variable with `persist` to keep its value across
`/reload` and server restarts:

```python
persist best_time = 0
persist champion = "nobody"
round = 1  # back to 1 on every load
```

The initializer only runs while the value is unset:

```mcfunction
execute unless score best_time mypack.vars matches -2147483648.. run scoreboard players set best_time mypack.vars 0
execute unless data storage mypack:global vars.champion run data modify storage mypack:global vars.champion set value "nobody"
```

`persist` is only allowed at module level, and its initializer must be a
constant: a number, boolean, string, list, or map literal, or a `const`
expression. Assign the variable in a function to change it; the new value
stays until something changes it again.

## The `temp` Objective In Raw Commands

Raw commands and execute headers name the variable objective `temp`. Cobble
rewrites `temp` to `<namespace>.vars` wherever a command expects an objective:
`scoreboard` arguments, `if score` and `unless score`, `store result score`,
`scores={...}` selector filters, and score text components.

```python
def read_level():
    level = 0
    store result score level temp:
        /scoreboard players get @s lvl
    /execute if score level temp matches 10.. run say high level
```

This compiles to `execute store result score level mypack.vars run scoreboard
players get @s lvl` and `execute if score level mypack.vars matches 10.. run
say high level`. Other text, such as a chat message that contains the word
`temp`, stays unchanged. Commands typed in chat or in command blocks use the
real objective name, for example `/scoreboard players get level mypack.vars`.

## Variables In Functions

Assignments inside a function run each time the function is called:

```python
def my_function():
    score = 0
    score = score + 10  # Compiles to: scoreboard players add score mypack.vars 10
    score = score - 5   # Compiles to: scoreboard players remove score mypack.vars 5
    config = {"count": 1}
    values = [1, 2, 3]
    i = 1
    config.count = 5
    values[i] = config.count + 2
```

Storage-backed list, map, and string paths can be assigned through attribute
or subscript targets inside functions. Dynamic list indexes and string map keys
use the same runtime storage path support as storage reads.

## The `global` Keyword

`global name` marks that a function changes a module-level variable:

```python
score = 0

def increment_score():
    global score
    score = score + 1
```

Minecraft scoreboards have no local scope. Every variable with the same name
is the same fake player on the pack's objective, so a function that assigns
`score` changes the module-level `score` with or without `global`. Use
`global` to make that intent visible, and use distinct names for values that
should stay separate.

## Compile-time Constants

`const` declares a value that the compiler substitutes wherever the name is
used. Constants can appear in expressions, in raw commands as `{NAME}`, and as
`persist` initializers:

```python
const MAX_HEALTH = 100
const BASE_DAMAGE = 10
const CRITICAL_MULTIPLIER = 2

def apply_damage():
    health = MAX_HEALTH
    damage = BASE_DAMAGE * CRITICAL_MULTIPLIER
    /say Max health is {MAX_HEALTH}
```

`/` inside a `const` declaration is real division; see
[Integer Division And Modulo](arithmetic.md#integer-division-and-modulo).
