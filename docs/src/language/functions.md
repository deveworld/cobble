# Functions

## Function Definition

```python
def greet():
    """Greet all players"""
    /tellraw @a {"text":"Hello!", "color":"green"}
```

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

Parameter names must be unique. Default parameter values, `*args`, and
`**kwargs` are not supported.

## Calling Functions

```python
def main():
    greet()
    give_reward("Steve", 5)
```

Functions can return integer scoreboard values:

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

**Recursion is not supported.** A function may not call itself, directly or
through a cycle of other functions. Recursive calls are rejected at compile
time with an error such as
`Recursive call to 'fact' is not supported. Rewrite the recursion as a loop (while/for) instead.`
Rewrite recursion as a `while` or `for` loop with explicit state.

Parameterized functions can call other parameterized functions and forward their
own parameters:

```python
def give_reward(player, item, count):
    /give {player} minecraft:{item} {count}

def reward_nearby(player, item, count):
    give_reward(player, item, count)
    /tag {player} add rewarded
```
