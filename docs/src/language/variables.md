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
`trigger`, `scores={...}` selector filters, the `scores` of an
`entity_scores` predicate, score text components, also inside an item's
components, the `score` of a score number provider, and the command of a
`run_command` click event. Resource JSON from `datapack.*`, such as a
predicate, is rewritten the same way. After a placeholder that stands for a
word, as in `/scoreboard players {op} coins temp 1` or `/execute {mode} score
x temp matches 1 run ...`, `temp` is rewritten where an objective stands for
every word the placeholder can be. Data that only looks like these, such as
`{scores:{temp:1}}` in `custom_data` or in storage, stays as written.

```python
def read_level():
    level = 0
    store result score level temp:
        /scoreboard players get @s lvl
    /execute if score level temp matches 10.. run say high level
```

This compiles to `execute store result score #read_level.level mypack.vars
run scoreboard players get @s lvl` and `execute if score #read_level.level
mypack.vars matches 10.. run say high level`. `level` is a local variable of
`read_level`, so its score holder is `#read_level.level` (see
[Variables In Functions](#variables-in-functions)). Other text, such as a chat
message that contains the word `temp`, stays unchanged. Commands typed in chat
or in command blocks use the real names, for example
`/scoreboard players get score mypack.vars` for a module variable `score`.

## Variables In Functions

A name that a function assigns is a local variable of that function, as in
Python. Each function's locals get their own score holders and storage paths,
so two functions can use the same name without changing each other's value:

```python
def my_function():
    score = 0
    score = score + 10  # Compiles to: scoreboard players add #my_function.score mypack.vars 10
    score = score - 5   # Compiles to: scoreboard players remove #my_function.score mypack.vars 5
    config = {"count": 1}  # Stored at locals.my_function.config
    values = [1, 2, 3]
    i = 1
    config.count = 5
    values[i] = config.count + 2
```

Reading a local before the function assigns it is a compile error. See
[Calling Convention](../reference/calling-convention.md) for the holder names
and the exact rules.

Storage-backed list, map, and string paths can be assigned through attribute
or subscript targets inside functions. Dynamic list indexes and string map keys
use the same runtime storage path support as storage reads.

## The `global` Keyword

A function that changes a module-level variable declares it `global`, as in
Python:

```python
score = 0

def increment_score():
    global score
    score = score + 1
```

Without `global score`, `score` would be a local of `increment_score`, and
`score = score + 1` would read it before assigning it, which is a compile
error. A function that only reads a module variable doesn't need `global`.
When a function assigns the name of a module variable without `global`,
Cobble reports a `local-shadows-module-variable` warning: the module variable
keeps its value.

As in Python, a function body runs after the whole module, so it can use a
module variable assigned further down the file, or one that another function
assigns under `global`. Module-level code runs from top to bottom: reading a
module variable there before its own assignment is a compile error.

A function parameter hides the module variable with the same name inside its
function (see
[Functions with Parameters](functions.md#functions-with-parameters)) and
can't be declared `global`.

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
