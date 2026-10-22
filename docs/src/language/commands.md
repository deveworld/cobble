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

## Placeholders

`{name}` inserts a function parameter, a loop variable, a constant, or a Cobble
variable into a raw command:

```python
def teleport_player(player, x, y, z):
    /tp {player} {x} {y} {z}
    /tellraw {player} {"text":"Teleported!", "color":"green"}
```

Function parameters become macro arguments, so that line compiles to a macro
line such as `$tp $(player) $(x) $(y) $(z)`. Constants are substituted at
compile time.

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
