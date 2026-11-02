# Calling Convention

Use this reference when calling Cobble from command blocks or other packs,
or reading variables from generated output. The examples use `mypack`.

## Local And Module Variables

Numbers and booleans use the objective `mypack.vars`. Strings, lists, and
maps use the storage `mypack:global`:

| Value | Score holder | Storage path |
| --- | --- | --- |
| Module variable `score` | `score` | `vars.score` |
| Local `score` in `tick` | `#tick.score` | `locals.tick.score` |
| Parameter `amount` | — | `args.amount` |

Use module variables for data other packs need to read. Compiler scratch
scores and paths beginning with `_` are internal.

### Reads Before Assignment

A name assigned in a function is local unless declared `global`. Initialize
locals before use, including before branches that may leave them unset.
Cobble catches common reads before assignment but does not prove that every
execution path initializes a value.

## Raw Commands

In Cobble source, use source variable names and the `temp` objective:

```python
def read_level():
    level = 0
    store result score level temp:
        /scoreboard players get @s lvl
```

Outside Cobble, use the generated holder and objective:

```mcfunction
scoreboard players get #read_level.level mypack.vars
```

## Arguments

Calls from Cobble handle argument storage automatically. For a call from
another pack, set `args` and use `with storage`:

```mcfunction
data modify storage mypack:global args set value {amount:3}
function mypack:double with storage mypack:global args
```

This works with a Cobble function such as:

```python
def double(amount):
    return amount * 2
```

An inline `/function mypack:double {amount:3}` fills Minecraft macro fields
but does not set the storage used by Cobble expressions. Prefer a Cobble
wrapper for text arguments that need escaping or for nested calls.

## Return Values

Cobble functions return integers. Other packs can capture the result with:

```mcfunction
execute store result score result points run function mypack:double with storage mypack:global args
```

A Cobble `return` inside a loop or execute block still ends the enclosing
function.

## Recursion

Recursive calls preserve locals and arguments. Changes to module variables,
entity scores, and external storage remain visible to the caller. All calls
run in one tick, so keep recursion bounded. Raw `/return` is not supported
in recursive functions; use Cobble's `return`.

## Reserved Storage Paths

Do not write compiler-managed paths such as `call_stack`, `frames`, or paths
beginning with `_`. Access `vars`, `locals`, and `args` only as described above.
