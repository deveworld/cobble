# Syntax And Types

## Blocks And Comments

Use indentation for blocks, `#` for comments, and `/` for Minecraft commands:

```python
def greet():
    """Greet every player."""
    /say Hello, world!
```

Block headers end with `:`. Expressions inside brackets can span multiple
lines without a continuation character.

## Data Types

| Type | Example |
| --- | --- |
| Number | `score = 10` |
| Boolean | `ready = True` |
| String | `name = "Alex"` |
| List | `items = ["diamond", "emerald"]` |
| Map | `config = {"count": 3}` |

Numbers are signed 32-bit scoreboard integers (`-2147483648` to `2147483647`).
Fractional assignments are truncated and out-of-range literals are clamped,
with a warning. Booleans use `0` and `1`.

Strings, lists, and maps are stored in Minecraft storage. `None` is available
in JSON resource declarations as `null`; it cannot be stored in NBT or used
as a number.

## Strings

Single and double quotes work, with escapes such as `\n`, `\t`, `\\`, `\"`,
and `\u00a7`. Add `f` to insert named values:

```python
wave = 3
label = f"Wave {wave}"   # Wave 3
literal = "Wave {wave}"  # Wave {wave}
```

F-strings support `{name}` fields and `{{` / `}}` for literal braces. Fields
cannot contain expressions, conversions, or format specifiers. Multiline
f-strings are not supported. A stored f-string formats text and numbers;
[command text](commands.md#json-and-snbt-text) can also display lists and maps.

## Lists And Maps

```python
items = ["diamond", "emerald"]
config = {"count": 3, "label": "Rewards"}
first = items[0]
last = items[-1]
count = config.count
label = config["label"]
size = len(items)
```

Lists and strings use integer indexes; maps use non-empty string keys.
Indexes and keys may be variables. Slices such as `items[1:3]` and
`name[:2]` are supported, without a step.

## Type System

A variable with a known type keeps that type:

```python
score = 5
score = 20     # OK
score = "ten"  # type-mismatch
```

Use a new variable for a value of another type. See
[Differences From Python](python.md) for copying, missing values, and other
behavior that differs from Python.
