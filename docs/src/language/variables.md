# Variables

## Module-level Variables

Top-level assignments run in source order whenever the pack loads or reloads:

```python
round = 1
lives = 3
```

Top-level commands, calls, and loops also run on load. Functions run when
called and can read module variables declared later in the file.

## Persistent Variables

Use `persist` to keep a value across `/reload` and server restarts:

```python
persist best_time = 0
persist champion = "nobody"
```

The initializer runs only while the value is unset. `persist` is allowed
only at module level and requires a literal or constant initializer.

## Variables In Functions

A name assigned inside a function is local to that function:

```python
def reward():
    amount = 3
    /say Reward: {amount}
```

Initialize locals before reading them. Use `global` when assigning a module
variable; a function that only reads it needs no declaration:

```python
score = 0

def add_point():
    global score
    score += 1
```

Without `global`, `score += 1` would read an uninitialized local. A parameter
also belongs to its function and hides a module variable of the same name.

Items and fields can be updated directly:

```python
config = {"count": 1}
values = [2, 4]

def update():
    config.count = 5
    values[0] += 1
```

Updating an item or field does not rebind its container, so it needs no
`global` declaration.

## Compile-time Constants

`const` declares a value substituted when the pack is built:

```python
const MAX_HEALTH = 100
const HALF = 1 / 2

def announce():
    /say Max health: {MAX_HEALTH}
```

Constants cannot be reassigned. Division in a constant declaration can
produce a fraction. See [Arithmetic](arithmetic.md) for numeric types.

## The `temp` Objective In Raw Commands

Use `temp` to read or update integer variables from raw commands:

```python
def read_level():
    level = 0
    store result score level temp:
        /scoreboard players get @s lvl
```

Cobble rewrites `temp` to `<namespace>.vars` and resolves local variable
names automatically and synchronizes these commands with storage. Outside
Cobble source, read or write the storage value directly. See
[Calling Convention](../reference/calling-convention.md) when integrating
with command blocks or another pack.
