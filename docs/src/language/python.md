# Differences From Python

Cobble uses Python-like syntax, but runs as Minecraft commands. These are
the differences to keep in mind when porting code.

## Values And Expressions

| Area | Cobble behavior |
| --- | --- |
| Numbers | Signed 32-bit integers, 32-bit floats, and explicit 64-bit doubles. Integer overflow wraps. |
| Division | Integer `/` rounds down; real `/` divides. `//` rounds down. `^` and `**` both mean power. |
| Lists and maps | Assignment and function arguments copy values rather than sharing an object. |
| Map keys | Non-empty strings only. |
| Function results | Numbers only; conditions return `1` or `0`. |
| Text indexing | Runtime positions count UTF-16 units; an emoji may occupy two positions. |
| Missing values | Missing items and fields give `0`, or an empty value when the type is known, rather than raising an exception. |
| Searching | `index()` returns `-1` when no item matches. |
| Sorting | `sort()` and `sorted()` order numbers. |
| Execution | Calls and loop iterations run in the same game tick. |

All variables use storage. Numeric expressions preserve real values;
explicit raw scoreboard commands still use Minecraft's integer rules.

A function assignment creates a local unless declared `global`. Imported
files share names rather than providing separate Python module objects;
see [Modules And Imports](modules.md).

## Python Forms

These forms are supported:

| Form | Example |
| --- | --- |
| Equality and membership | `name == "Alex"`, `item in items`, `"key" not in config` |
| Truth of containers | `if items:`, `while queue:` |
| Conditional expression | `value = "ready" if flag else [1, 2]` |
| Operand choices | `name = given or "anon"` |
| Chained comparison | `0 < score <= 10` |
| Multiple assignment | `a, b = b, a`, `x, y = pair` |
| Slices | `text[1:3]`, `items[-2:]` |
| Joining and repeating | `"Hello " + name`, `items + [4]`, `"-" * n` |
| List methods | `append`, `extend`, `insert`, `pop`, `remove`, `index`, `count`, `sort`, `reverse`, `clear` |
| Builtins | `int`, `float`, `double`, `abs`, `min`, `max`, `sum`, `sorted`, `str`, `len` |
| Iteration | `for i, item in enumerate(items):`, `for ch in text:` |
| Text matching | `case "sword":` |

Conditional expressions and `and` / `or` evaluate only the chosen branch or
needed operand. A new variable can hold alternatives of different types;
an existing variable with a known type still requires that type.

When both operands are untyped parameters, `+` and `*` use numeric
arithmetic. Use `str(a) + b` to request text joining.

## Not Supported

- Classes, decorators, exceptions, context managers, lambdas, generators,
  and async code.
- `break`, `continue`, `for ... else`, comprehensions, and iteration over map keys.
- Default arguments, `*args`, and `**kwargs`.
- Slice steps such as `items[::2]`, and assignment to string characters.
- Import aliases, wildcard imports, and dotted or relative module names.
- General Python f-string expressions or format specifiers; see
  [Strings](basics.md#strings).

`cobble check --experimental-python-compat` reports unsupported constructs
and suggests rewrites where available.

## Browser Compiler

The [browser compiler](https://deveworld.github.io/cobble/try/) accepts one
file. Use the CLI for projects with local imports.
