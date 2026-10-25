# Minecraft Commands

A line that starts with `/` is a Minecraft command. Cobble writes it to the
`.mcfunction` file without the slash:

```python
def setup():
    /scoreboard objectives add deaths deathCount
    /gamerule spawn_mobs false
```

A `#` comment after a command is removed from the output.

## JSON Text

`tellraw`, `title`, and other commands with JSON text work as written:

```python
def announce():
    score = 10
    /tellraw @a {"text":"Game Started!", "color":"gold", "bold":true}
    /title @a title {"text":"Welcome", "color":"aqua"}
    /tellraw @a [{"text":"Score: {score}", "color":"gold"}, {"text":" ready"}]
```

In `tellraw` and `title`, a `{variable}` inside a JSON `text` field or a JSON
array string becomes a score or NBT text component. The styling of the
surrounding text is kept.

## Variables In `say`

A `/say` line can show variables too. Cobble copies each value into the
storage `<namespace>:global _say_args` and runs the line as a macro `say` in a
helper function, so the message keeps the `[name]` prefix and shows selectors
such as `@s` as names:

```python
count = 3

def show():
    /say Count is {count}
```

`show` copies the value and calls the helper:

```mcfunction
execute store result storage mypack:global _say_args.count int 1 run scoreboard players get count mypack.vars
function mypack:_cobble_say_0 with storage mypack:global _say_args
```

The helper `_cobble_say_0` holds the macro line and returns 1, the result
`say` itself has:

```mcfunction
$say Count is $(count)
return 1
```

Minecraft reads the line as `say` text after filling in the values, so a
selector inside a string value, such as `@p`, also shows as a name, and the
whole text after filling in the values may be at most 256 characters; a longer
line fails when it runs and says nothing. Use `tellraw` for long text. A
`/say` line without variables is written as it is, and one that also uses a
hand-written `$(name)` that is not a parameter of the function compiles to
`tellraw @a` as before.

## Placeholders

`{name}` inserts a function parameter, a constant, or the variable of a loop
that Cobble expands at compile time (see
[For Loops](control-flow.md#for-loops)) into a raw command:

```python
def teleport_player(player, x, y, z):
    /tp {player} {x} {y} {z}
    /tellraw {player} {"text":"Teleported!", "color":"green"}
```

Function parameters become macro arguments, so that line compiles to a macro
line such as `$tp $(player) $(x) $(y) $(z)`. Constants and the values of those
loops are substituted at compile time.

A Cobble variable, including the variable of a runtime loop, works as a
placeholder only in `say`, `tellraw`, and `title` text, as described under
[JSON Text](#json-text) and [Variables In `say`](#variables-in-say). In any
other command it is a compile error. To use a variable's value there, pass it
to a function parameter:

```python
count = 3

def give_diamonds(amount):
    /give @a minecraft:diamond {amount}

def reward():
    give_diamonds(count)
```

Unknown, invalid, and unclosed placeholders are errors. Write `{{` and `}}` for
literal braces:

```python
def help():
    /say Type {{name}} to see a placeholder
```

```mcfunction
say Type {name} to see a placeholder
```

## The `temp` Objective

Raw commands refer to Cobble variables through the objective name `temp`,
which Cobble rewrites to the pack's `<namespace>.vars` objective. See
[The `temp` Objective In Raw Commands](variables.md#the-temp-objective-in-raw-commands).
