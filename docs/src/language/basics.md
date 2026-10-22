# Syntax And Types

## Blocks And Comments

Cobble uses indentation-based blocks after `:` and needs no braces or
semicolons. Unexpected indentation and dedents that do not match a previous
block level are reported as diagnostics before parsing. Lines starting with `/`
are Minecraft commands; see [Minecraft Commands](commands.md).

```python
# A comment runs to the end of the line.

def my_function():
    """
    A docstring describes the function.
    It can span multiple lines.
    """
    /say Hello, world!
```

## Data Types

### Numbers
```python
score = 10
health = 100
```

Numbers are internally stored as Minecraft scoreboard values.

### Strings
```python
message = "Hello, world!"
```

### Booleans
```python
is_active = True
is_disabled = False
```

Booleans are stored as integers in Minecraft scoreboards (0 for False, 1 for True).

### Lists, Maps, and None
```python
from stdlib import datapack

items = ["minecraft:diamond", "minecraft:emerald"]
config = {"chance": 1, "note": "missing"}
first_item = items[0]
chance = config.chance

datapack.predicate("maybe", {
    "type": "minecraft:random_chance",
    "chance": 1,
    "comment": None
})
```

Lists and maps are storage-backed values. Cobble supports attribute and
subscript access when the base value resolves to a storage path, including
literal or constant subscript segments, runtime integer list/string indexes, and
runtime string map keys. Arbitrary Python-style indexing is not a general
runtime feature yet.

`len(value)` returns the length of literal arrays, maps, and strings, as well as
storage-backed list, map, and string values. Storage-backed lengths compile to a
Minecraft `data get storage ...` result stored in a scoreboard integer.

`None` is supported where Cobble serializes data pack JSON resources and is
emitted as JSON `null`. It has no scoreboard value, so keep it out of numeric
and boolean expressions. Minecraft NBT/SNBT storage has no null type, so
storage literals reject `None`; use an explicit sentinel value for storage.

## Type System

A variable's type is inferred from its first assignment and stays fixed after
that:

```python
x = 5        # integer
y = True     # boolean
name = "Bob" # string

x = 20       # OK: still an integer
x = True     # error: x is an integer
```

Assigning a value of another type is a `type-mismatch` error:

```text
Error: type-mismatch: Type mismatch for variable 'x'.
   ╭─[ main.cbl:6:1 ]
   │
 6 │ x = True
   │ ┬
   │ ╰── Type mismatch for variable 'x'.
   │
   │ Note: Variable was previously defined as type: integer
   │       Cannot reassign to type: boolean
   │       Use a different variable name or keep all assignments to 'x' the same type.
───╯
```

## Numeric Range And Precision

Numbers are scoreboard integers, so they range from `-2147483648` to
`2147483647`. A float literal is truncated to an integer and a literal outside
the range is clamped, each with a warning:

```python
pi = 3.14159       # warning: truncated to 3
huge = 9999999999  # warning: clamped to 2147483647
```
