# Functions

## Function Definition

```python
def greet():
    /tellraw @a "Hello!"
```

Each `def` becomes a Minecraft function, called in game as
`/function <namespace>:greet`. Use lowercase names; `_cobble` names are
reserved for the compiler.

## Functions with Parameters

Use parameters in expressions and as `{name}` command arguments:

```python
def give_reward(player, amount):
    /give {player} minecraft:diamond {amount}
    /tellraw {player} f"You received {amount} diamonds!"

def main():
    give_reward("@s", 3)
```

Parameters can hold numbers, strings, lists, or maps, and can be reassigned.
Arguments are copied, so changing a list parameter leaves the caller's list
unchanged. Default arguments, `*args`, and `**kwargs` are not supported.

Fractions passed only to command text or f-strings keep their precision.
Arithmetic and return values preserve integer, float, and double values.

## Calling Functions

Calls can forward arguments to other functions and return numeric values:

```python
def twice(value):
    return value * 2

def reward(amount):
    return twice(amount) + 1
```

`return` ends the enclosing function, including inside a loop or execute
block. A returned condition gives `1` or `0`. Use a module variable or
storage for strings, lists, and maps.

Calls also work in conditions:

```python
def ready():
    return 1

def start():
    if ready():
        /say Starting
```

## Recursion

Functions can call themselves, directly or through another function:

```python
def fact(n):
    if n <= 1:
        return 1
    return n * fact(n - 1)
```

Each call keeps its own locals and arguments. All calls run in one game
tick, so bound the recursion. Use Cobble's `return` rather than raw
`/return` in a recursive function.

For calls from command blocks or other packs, see
[Calling Convention](../reference/calling-convention.md#arguments).
