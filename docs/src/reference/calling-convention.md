# Calling Convention

This page describes where a function's variables, arguments, and return values
live in the generated pack. It matters when raw commands, command blocks, or
other packs read a function's variables, and when you read the generated
`.mcfunction` files. The examples use the namespace `mypack`.

## Local And Module Variables

A name is local to a function when the function assigns it (`=`, `+=`, or
another compound operator) or uses it as a `for` target, and doesn't declare
it `global`. This follows Python's rule. Every other name in the function
refers to a module variable.

Numbers and booleans live on the pack's objective `mypack.vars`. Strings,
lists, and maps live in the storage `mypack:global`:

| Variable | Score holder on `mypack.vars` | Storage path in `mypack:global` |
|---|---|---|
| Module variable `score` | `score` | `vars.score` |
| Local `score` of `tick` | `#tick.score` | `locals.tick.score` |
| Parameter `amount` | | `args.amount` |

The holders of locals start with `#`, so they stay out of the scoreboard
sidebar. Generated function names have no module prefix, so the locals of an
imported module's `helper` are also `#helper.<name>`.

```python
score = 0

def add_bonus():
    global score
    bonus = 5          # local: #add_bonus.bonus
    score += bonus     # module variable: score
```

Assigning an attribute or an item, as in `config.count = 5` or
`items[i] = 0`, doesn't make `config` or `items` local.

### Reads Before Assignment

Python raises `UnboundLocalError` when a function reads a local before
assigning it. Cobble reports the same mistake when it compiles the function:

```python
count = 0

def bump():
    count += 1   # error: `count` is read before it is assigned in `bump`
```

Add `global count` to change the module variable instead. The check follows
the order the statements run in:

- After an `if` or `match`, a name assigned in any branch counts as assigned.
  When the branch that assigns it didn't run, the read gets the value the
  holder kept from an earlier call, where Python would raise an error.
- In a loop body, a read inside a nested block may rely on an assignment
  later in the body, because a later pass reaches it after that assignment:

  ```python
  def walk():
      for i in range(3):
          if i > 0:
              gap = i - last   # allowed: the first pass skips this block
          last = i
  ```

  A read directly in the loop body, or in the header of a nested block, is
  checked as on the first pass.
- In raw commands, only `{name}` placeholders are checked. Raw text such as
  `level temp` is not.

When a function assigns the name of a module variable without `global`,
Cobble reports a `local-shadows-module-variable` warning at the first
assignment: the module variable keeps its value. `cobble migrate` lists these
functions as `function-local-variable`.

## Raw Commands

Raw commands and execute headers name variables by their source names.
Inside a function, Cobble renames a local's holder wherever a command names a
score holder on the variable objective: `scoreboard` arguments, `if score`
and `unless score`, `store ... score`, and score text components.

```python
def read_level():
    level = 0
    store result score level temp:
        /scoreboard players get @s lvl
    /execute if score level temp matches 10.. run say high level
```

compiles to:

```mcfunction
scoreboard players set #read_level.level mypack.vars 0
execute store result score #read_level.level mypack.vars run scoreboard players get @s lvl
execute if score #read_level.level mypack.vars matches 10.. run say high level
```

A name the function doesn't assign keeps naming the module variable. Commands
typed in chat, command blocks, and other packs use the generated names, such
as `/scoreboard players get #read_level.level mypack.vars`. A local keeps its
value until the next call of its function assigns it, so use a module
variable for a value that something outside the function reads.

## Arguments

A function with parameters runs as a macro function with the storage
`mypack:global args`. It reads its parameters from `args.<name>`: macro lines
use `$(name)`, and conditions and arithmetic use
`data get storage mypack:global args.<name>`. Assigning a parameter writes
`args.<name>`, and so does changing one of its items or fields, as
`items[0] = 9` or `items.append(3)` do. A macro line fixes its arguments when
the function is called, so after such a change each macro line that uses the
parameter runs in a helper called `with storage mypack:global args`, and shows
the current value. A local assigned from a parameter, such as `copy = value`,
gets both `locals.<function>.copy` and `#<function>.copy`, because the
parameter can hold a string as well as a number. When a raw command in the
function writes the local's score, the local holds a number and only the
score is kept.

A call builds the arguments in a storage path of its own and copies them to
`args` right before the `function` command:

```mcfunction
data remove storage mypack:global _call_args_5
data modify storage mypack:global _call_args_5.n set value 5
data modify storage mypack:global args set from storage mypack:global _call_args_5
execute store result score #main.f mypack.vars run function mypack:fact with storage mypack:global args
data remove storage mypack:global _call_args_5
```

The number in `_call_args_<n>` identifies the call site. A caller with
parameters of its own copies its `args` to `_caller_args_<n>` before the call
and back afterwards, so it reads its own arguments again. It does the same
around a call to a function without parameters, because that function can
call others that replace `args`.

Calls inside a recursive function use the list `call_stack` instead, because
one call site can be running in several calls at once. The caller appends its
`args` to `call_stack`, builds the new arguments in another appended entry,
copies that entry to `args`, and removes it. After the call it takes its own
`args` back from the list.

Call functions that have parameters from Cobble code. A raw `/function`
command with inline arguments fills the macro lines, but the rest of the
function reads `args`, which the raw command doesn't set.

## Return Values

`return value` ends the function with Minecraft's return value, so a caller
reads it with `execute store result score ... run function ...`. Only
integers can be returned.

A `return` inside a block, such as an `if` body, a loop, or `as @a:`, runs in
a helper function, so it can't end the function directly. It sets the scores
`#return_value_<n>` and `#return_flag_<n>` on `mypack.vars`. After the block,
the function checks the flag and returns the value.

## Recursion

A function is recursive when it can call itself, directly or through other
functions. Modules can't import each other in a cycle, so a call cycle stays
inside one file.

```python
def fact(n):
    if n <= 1:
        return 1
    return n * fact(n - 1)
```

Each call of a recursive function saves the scores it writes and restores
them before it returns, so the caller finds its own values again:

1. On entry, the function appends an entry to the storage list
   `frames.<function>`. The entry holds the function's locals, the scratch
   scores the compiler uses in the function and its block helpers, the return
   value and flag, and the storage `locals.<function>`.
2. Every way out of the function goes through the helper
   `_cobble_exit_<function>`. It copies the return value to
   `#<function>.return`, restores the saved scores and storage, removes the
   entry, and returns `#<function>.return`.

For `fact`, the exit helper looks like this (some restores left out):

```mcfunction
scoreboard players operation #fact.return mypack.vars = #return_value_0 mypack.vars
execute store result score #return_flag_0 mypack.vars run data get storage mypack:global frames.fact[-1].v2
execute store result score #return_value_0 mypack.vars run data get storage mypack:global frames.fact[-1].v3
data remove storage mypack:global frames.fact[-1]
return run scoreboard players get #fact.return mypack.vars
```

Module variables and `global` names are not saved, so a change to them in a
deeper call lasts. Parameters are kept in `call_stack` by the caller, as
described under [Arguments](#arguments).

Limits:

- A raw `/return` in a recursive function is a compile error, because it
  would skip the exit helper. Use Cobble's `return`.
- Only scores on `mypack.vars` are saved. Entity scores, such as `@s` on
  another objective, and storage outside `locals.<function>` keep the changes
  of deeper calls.
- All calls run in the same game tick. A pack with a recursive function
  raises the gamerule `max_command_sequence_length`, as it does for runtime
  loops, so deep recursion can take a long time in one tick.

## Reserved Storage Paths

Cobble uses these paths in `mypack:global`. Raw commands should read them
only as described above and never write them:

- `vars.<name>`: module variables that hold strings, lists, or maps
- `locals.<function>.<name>`: locals that hold strings, lists, or maps
- `args`: the arguments of the running function
- `call_stack`: arguments during a call in a recursive function
- `frames.<function>`: saved state of recursive calls
- paths that start with `_`, such as `_call_args_<n>` and `_caller_args_<n>`:
  values Cobble builds while a statement runs
