# Differences From Python

Cobble borrows Python's look, but programs run as Minecraft commands. Values
are scoreboard integers or storage entries, and every function is a
`.mcfunction` file.

## Not Supported

These fail with a diagnostic before compilation:

- classes, decorators, `try`/`except`/`finally`, `with`, `lambda`, `yield`,
  `async`, `await`, `break`, `continue`, `raise`, `assert`, `del`, and
  `nonlocal`;
- default parameter values, duplicate parameter names, `*args`, and `**kwargs`;
- list and dict comprehensions;
- `for ... else`, and `for` over the keys of a map;
- a slice with a step, such as `items[::2]`;
- dotted or relative imports, import aliases, and `import a, b`;
- a statement that nests more than 1000 levels deep, counting blocks,
  brackets, and operators, such as a chain of 1000 `and`s or a JSON value
  nested 1000 levels deep.

`cobble check --experimental-python-compat` lists the supported Python-like
constructs and suggests a Cobble rewrite for unsupported ones; see
[Experimental Features](../reference/experimental.md).

## Values And Expressions

- Numbers are 32-bit integers. Floats are truncated, with a warning, and so
  is a fractional item or field read as a number, as `y[0]` after
  `y = [0.6, 2.5]`. Number literals may have an exponent, as in Python: `1e3`,
  `1E3`, `2.5e-3`, and `1e+3`.
- Lists and maps live in storage. Subscripts work when the base resolves to a
  storage path. List and string indexes may be literals, constants, or runtime
  integers such as `items[i]`, and a negative index counts from the end;
  `name[i]` of a string is a one-character string. Map keys may be
  attributes, string literals, or runtime strings such as `config[key]` or
  `config[names[0]]`. A key can hold any text, such as spaces, dots, colons,
  quotes, backslashes, and line breaks, as in `prices["minecraft:diamond"]`,
  but it can't be empty: Minecraft refuses an empty key in a path and in a
  map. An empty literal key, as in `config[""]` or `{"": 1}`, is a compile
  error. An empty runtime key stores nothing, reads as a missing key, and
  shows `<file>:<line>: a map key can't be empty` in chat; `"" in config` is
  False.
- At any depth, as in `grid[r][c]`, the key's type decides: text picks a
  field of a map, and a number an item of a list or a character of text.
  When Cobble can't tell the key's type, as for a parameter, the type of
  what it indexes decides, and when that isn't known either, the pack checks
  when it runs whether the key holds text or a number. A number is never a
  map key, so `5 in {"5": 1}` is false, as in Python. A text key of a value
  Cobble knows is a list, as `rows[0]["k"]` after `rows = [[1, 2]]`, is a
  compile error, where Python raises `TypeError`.
- A list or map is a value: assigning it to another variable or passing it to
  a function copies it, so a change through one name doesn't show through the
  other.
- An item of a list or a field of a map keeps its type when you assign it or
  pass it on, at any depth and through unpacking and `pop()`: `name =
  names[1]` holds a string when `names` holds strings, and `a, b = names`
  gives two strings.
  When Cobble can't tell the type, as for a parameter or an item that the
  program also sets to a value of another type, the value is kept as text and
  as a number.
- Functions return integers only. Use storage or a module variable for other
  results. `return x > 3` and other conditions return 1 or 0.
- `/` and `//` are both floor division, and `^` and `**` are both power.
- `+` joins text and lists when Cobble can tell that one side is text or a
  list. Two parameters add as numbers, so write `str(a) + b` to join them.
- Text joined, repeated, or passed to `str()` while the pack runs keeps every
  character, `"` and `\` included.
- In `items[f()]`, a list that `f` changes in place shows the change, as in
  Python, and so does one that `f` replaces, where Python keeps the old list.
- `sort()` and `sorted()` order numbers. `index()` gives -1 when there is
  nothing to find, where Python raises an error.
- Reading an item or a field that isn't there, and `pop()` on an empty list,
  give 0, or empty text, an empty list, or an empty map when Cobble knows the
  value is one, where Python raises an error. The variable never keeps the
  value it had before. Passed to a function, as in `show(config.nick)`, such
  a read gives the same empty value, and the function still runs.
- Text positions count UTF-16 units, so an emoji is two long in a running pack.
- A bare expression such as `score + 1` produces no command and is rejected.
  Use an assignment, a call, a raw command, or `pass`.
- `for` loops iterate `range(...)`, literal arrays, storage-backed lists, and
  text.
- A name a function assigns is local to it unless the function declares it
  `global`. Reading a local before assigning it is a compile error instead of
  an `UnboundLocalError` at run time; see
  [Calling Convention](../reference/calling-convention.md#reads-before-assignment).
- Recursive functions work, and every call runs in the same game tick.
- `while` loops and runtime `for` loops compile to functions that call
  themselves, and every iteration runs in the same game tick.

## Python Forms

These work as in Python on numbers, text, lists, and maps:

| Form | Example |
| --- | --- |
| Comparing text, lists, and maps | `if name == "Steve":`, `if a != [1, 2]:` |
| Membership | `if x in items:`, `"key" not in config`, `"ell" in text` |
| Truth of text, lists, and maps | `if items:`, `while queue:` |
| Conditional expression | `x = 1 if ready else 2` |
| Chained comparison | `if 0 < x <= 10:` |
| Multiple assignment | `a, b = b, a`, `x, y = pair`, `a[0], a[1] = a[1], a[0]` |
| `and` and `or` giving an operand | `name = given or "anon"` |
| Slices | `text[1:3]`, `items[-2:]`, `items[:n]` |
| Joining and repeating | `"a" + name`, `items + [4]`, `[0] * 9`, `"-" * n` |
| List methods | `append`, `extend`, `insert`, `pop`, `remove`, `index`, `count`, `sort`, `reverse`, `clear` |
| Builtins | `abs`, `min`, `max`, `sum`, `sorted`, `str`, `len` |
| Loops | `for i, item in enumerate(items):`, `for ch in text:` |
| `match` on text | `case "sword":` |

`min` and `max` take several numbers or one list, and `sum` takes a list. A
function you define with one of these names replaces it. Forms that need a loop compile to helper
functions in the pack, named `_cobble_...`, and only the ones a pack uses are
added. A value that takes commands to compute can't initialize a module
variable; assign it inside a function.

## Browser Compiler

The browser compiler at `/try` compiles one in-memory file. Check projects
with local imports using the CLI, which resolves files from disk.
