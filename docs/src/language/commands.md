# Minecraft Commands

A line that starts with `/` is a Minecraft command. Cobble writes it to the
`.mcfunction` file without the slash:

```python
def setup():
    /scoreboard objectives add deaths deathCount
    /gamerule spawn_mobs false
```

A `#` comment after a command is removed from the output. The `#` needs a
space before and after it, so `#` inside a name, such as the fake player
`#count`, stays in the command.

## Long Commands

End a line with `\` to go on in the next one, as in a `.mcfunction` file:

```python
def reward():
    /execute as @a[tag=winner] at @s \
        run give @s minecraft:diamond 3
    /tellraw @a [ \
        {"text":"Round over","color":"gold"}, \
        {"text":" Next round soon"} \
    ]
```

```mcfunction
execute as @a[tag=winner] at @s run give @s minecraft:diamond 3
tellraw @a [ {"text":"Round over","color":"gold"}, {"text":" Next round soon"} ]
```

The `\` goes and the next line's indent doesn't count, so keep a space before
the `\` where the command needs one. A blank line ends the command, and a `\`
on the last line of a file is dropped. Each line can have its own `#` comment
after the `\`, and a `\` at the end of a comment continues nothing. Errors point
to the first line of the command, and later lines keep their numbers.

## JSON And SNBT Text

`tellraw`, `title`, and other commands with text components work as written,
in JSON or in SNBT such as `{text:"Hello",color:gold}`:

```python
def announce():
    score = 10
    /tellraw @a {"text":"Game Started!", "color":"gold", "bold":true}
    /title @a title {"text":"Welcome", "color":"aqua"}
    /tellraw @a [{"text":"Score: {score}", "color":"gold"}, {"text":" ready"}]
```

In `tellraw` and `title`, a `{variable}` inside a `text` field, a string in a
component list, `extra`, the `with` arguments of a translation, or hover text
becomes a score or NBT text component. The styling of the surrounding text is
kept. A string shows as its text, and a list or a map
shows as SNBT, such as `[1, 2]`.

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

In `tellraw` and `title`, also after `execute ... run`, a parameter in text
written without a JSON or SNBT component, as `/tellraw @a {msg}` or
`/tellraw @a Hi {msg}`, accepts any text when Cobble calls the function,
including quotes, backslashes, and line breaks:

```python
def show(msg):
    /tellraw @a Hi {msg}

def greet():
    show("say \"hello\" \\ goodbye")
```

The public Minecraft function also accepts simple inline arguments, such as
`/function mypack:show {msg:"hello world"}`. That entry uses Minecraft's raw
macro substitution inside a text wrapper, so quotes and backslashes in an
inline value need JSON escaping as well as SNBT escaping. Cobble calls handle
this automatically by reading the argument from storage.

Writing a component yourself, as `/tellraw @a {"text":"{msg}"}`, also reads
the value from storage and accepts any text when Cobble calls the function
(see [Arguments](../reference/calling-convention.md#arguments)). The same holds
for hover text. In `show_entity` names and quoted `show_item` component values,
Cobble escapes the parameter before the command runs, so quotes and
backslashes survive there too. Single-quoted text works the same way. Lists
and maps in these quoted hover fields display their SNBT text, including
quoted strings and nested containers. A parameter that stands for the action, as in `/title @a {kind}
{text}`, is taken for `title`, `subtitle`, or `actionbar`.

To pass a ready text component, such as `{text:"hi",color:"red"}`, write the
macro argument yourself:

```python
def show(msg):
    /tellraw @a $(msg)
```

A line written with `$(name)` is kept exactly as written, apart from the
leading `$` Cobble adds. `title ... times` takes numbers and is never
wrapped, and a variable there is a compile error.

A Cobble variable, including the variable of a runtime loop, works as a
placeholder only in `say`, `tellraw`, and `title` text, as described under
[JSON Text](#json-and-snbt-text) and [Variables In `say`](#variables-in-say). In any
other command it is a compile error. To use a variable's value there, pass it
to a function parameter:

```python
count = 3

def give_diamonds(amount):
    /give @a minecraft:diamond {amount}

def reward():
    give_diamonds(count)
```

Unknown, invalid, and unclosed placeholders are errors, and so is a
placeholder for a name Cobble has no value for where the command runs.
Write `{{` and `}}` for literal braces. They work as in a Python format
string: they nest, a placeholder between them is still filled in, so
`{{{x}}}` is `{`, the placeholder `x`, and `}`, and a single `}` that closes
a `{{`, as in `{{k}:1}`, is an error:

```python
def help(x):
    /say Type {{name}} to see a placeholder
    /data merge storage mypack:a {{a:{{b:1}},c:2}}
    /say {{{x}}}
```

```mcfunction
say Type {name} to see a placeholder
data merge storage mypack:a {a:{b:1},c:2}
$say {$(x)}
```

A single brace that holds no name, as in the SNBT `{Tags:["a"]}`, stays as
it is. The value of a constant or a loop variable is placed as it is, so its
own braces are not read again: `for n in ["{{a}}"]: /say {n}` says `{{a}}`.

## The `temp` Objective

Raw commands refer to Cobble variables through the objective name `temp`,
which Cobble rewrites to the pack's `<namespace>.vars` objective. See
[The `temp` Objective In Raw Commands](variables.md#the-temp-objective-in-raw-commands).
