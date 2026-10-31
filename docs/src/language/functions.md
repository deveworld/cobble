# Functions

## Function Definition

```python
def greet():
    """Greet all players"""
    /tellraw @a {"text":"Hello!", "color":"green"}
```

Each `def` becomes the function file `<name>.mcfunction`, so a name has no
capital letters and at most 244 bytes, which keeps the file name within 255
bytes. Names that start with `_cobble` are reserved for the helper functions
Cobble generates, and a name that Cobble also gives a generated helper, such
as `if_temp_0`, is a compile error.

## Functions with Parameters

Cobble supports function parameters using Minecraft's macro system:

```python
def give_reward(player, amount):
    """Give a reward to a player"""
    /give {player} minecraft:diamond {amount}
    /tellraw {player} {"text":"You received diamonds!", "color":"gold"}
```

Use `{param_name}` in Cobble source. Cobble compiles it to Minecraft's
`$(param_name)` macro syntax, so the line becomes a macro line in the generated
function. Text outside the braces, such as `minecraft:diamond`, stays
literal.

A fractional number passed to a parameter that the function only uses in
command text, as in `play_sound(0.6)` for `/playsound ... {pitch}`, arrives as
written. When the function uses the parameter as a value, such as
`pitch + 1`, the number is a score, so `1.5` is stored as `1` with the same
`fractional-number` warning as `x = 1.5`.

Parameter names must be unique. Default parameter values, `*args`, and
`**kwargs` are not supported.

A parameter hides a module variable or constant with the same name inside its
function, as in Python. Conditions, arithmetic, and `{name}` all read the
argument:

```python
amount = 100

def reward(amount):
    if amount > 50:
        /say big reward {amount}
    return amount * 2      # reward(5) returns 10
```

A function can assign to its parameters. The new value is stored with the
call's arguments, so later conditions, arithmetic, and `{name}` read it:

```python
def countdown(n):
    while n > 0:
        /say {n}
        n = n - 1
```

A parameter can hold a number, a string, a list, or a map. An argument is
copied into the call, so a function that changes a list it received leaves the
caller's list as it was, where Python would change it:

```python
def total(values):
    result = 0
    for value in values:
        result += value
    return result

def main():
    pairs = [3, 4]
    a = total(pairs)       # 7
    b = total([1, 2, 3])   # 6
```

Cobble doesn't know a parameter's type, so a copy of a parameter keeps its
value in storage and in a score: `{name}` and `len(name)` read the value, and
arithmetic reads the number. `name[i]` of a parameter checks when the pack
runs whether the value is a list or a string, and gives an item or a
character; text in `i`, such as a string parameter, gives the field of a
map. `for` goes through the items of a list parameter or the
characters of a string parameter. In `tellraw` and `title` text, `{name}`
shows the argument as text, so quotes and backslashes in a string are shown
as they are. A parameter can't also be
declared `global`.

## Calling Functions

```python
def main():
    greet()
    give_reward("Steve", 5)
```

Functions can return integer scoreboard values. A condition returns 1 or 0:

```python
def double(value):
    return value * 2

def main():
    score = double(5) + 1
```

Cobble compiles `return expr` to Minecraft's function return value. Calls used
inside expressions are executed with `execute store result score ... run
function ...`, so returned values are scoreboard integers. Integer-returning
user functions can be used in assignments, arithmetic expressions, and
conditions such as `if ready():` or `while should_continue():`. Imported
functions have the same return metadata after their module is processed. Use
storage or explicit output variables for non-integer data.

A function can call itself, directly or through other functions:

```python
def fact(n):
    if n <= 1:
        return 1
    return n * fact(n - 1)
```

Each call has its own local variables and arguments, and the caller gets its
values back when the call returns. All calls run in the same game tick. See
[Recursion](../reference/calling-convention.md#recursion) for how a recursive
function compiles and what it doesn't save.

Parameterized functions can call other parameterized functions and forward their
own parameters:

```python
def give_reward(player, item, count):
    /give {player} minecraft:{item} {count}

def reward_nearby(player, item, count):
    give_reward(player, item, count)
    /tag {player} add rewarded
```
