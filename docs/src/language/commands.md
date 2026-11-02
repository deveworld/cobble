# Minecraft Commands

A line starting with `/` is written as a Minecraft command:

```python
def setup():
    /scoreboard objectives add deaths deathCount
    /gamerule spawn_mobs false
```

Inline comments need spaces around `#`, so names such as `#count` stay intact.

## Placeholders

Use `{name}` for a function parameter, a constant, or the value of an
expanded loop:

```python
def teleport(player, x, y, z):
    /tp {player} {x} {y} {z}
```

Ordinary runtime variables work in message text. For other command arguments,
pass the value through a function parameter:

```python
count = 3

def give_diamonds(amount):
    /give @a minecraft:diamond {amount}

def reward():
    give_diamonds(count)
```

Unknown names and unclosed placeholders are compile errors. Outside quoted
strings, use `{{` and `}}` for literal braces, such as `/say Type {{name}}`.
Normal JSON and SNBT object braces, such as `{Tags:["winner"]}`, need no escape.

## JSON And SNBT Text

Use an f-string to insert values into quoted text:

```python
def announce(wave):
    /title @a title f"Wave {wave}"
    /tellraw @a {"text": f"Wave {wave}", "color": "gold"}
    /tellraw @a "Wave {wave}"  # literal text
```

Plain quoted strings never interpolate. Inside an f-string, `{{` and `}}`
produce literal braces. Fields accept names, as described under
[Strings](basics.md#strings).

`tellraw` and `title` can show runtime variables, including in styled
components, lists, and hover text. Strings keep quotes, backslashes, and line
breaks when passed from Cobble code; lists and maps display as SNBT.

## Variables In `say`

```python
count = 3

def announce():
    /say Count: {count}
```

`say` keeps Minecraft's sender prefix and selector expansion. Its final
message must fit in 256 characters; use `tellraw` for longer text.

## Native Macros

To pass a ready text component instead of message text, use Minecraft's
`$(name)` syntax:

```python
def show(component):
    /tellraw @a $(component)
```

Native macros substitute command text directly. Prefer f-strings for messages
that contain quotes or backslashes. Calls from command blocks or other packs
must follow the [calling convention](../reference/calling-convention.md#arguments).

## Long Commands

End a line with `\` to continue it. Keep a space before the backslash where the
command needs an argument separator:

```python
def reward():
    /execute as @a[tag=winner] at @s \
        run give @s minecraft:diamond 3
```

## The `temp` Objective

In raw commands, `temp` names Cobble's variable objective and is rewritten
to `<namespace>.vars`. See
[Variables](variables.md#the-temp-objective-in-raw-commands).
