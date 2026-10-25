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
- `for ... else`;
- dotted or relative imports, import aliases, and `import a, b`.

`cobble check --experimental-python-compat` lists the supported Python-like
constructs and suggests a Cobble rewrite for unsupported ones; see
[Experimental Features](../reference/experimental.md).

## Values And Expressions

- Numbers are 32-bit integers. Floats are truncated, with a warning.
- Lists and maps live in storage. Subscripts work when the base resolves to a
  storage path. List and string indexes may be literals, constants, or runtime
  integers such as `items[i]`, and a negative index counts from the end;
  `name[i]` of a string is a one-character string. Map keys may be
  attributes, string literals, or runtime strings such as `config[key]`.
- A list or map is a value: assigning it to another variable or passing it to
  a function copies it, so a change through one name doesn't show through the
  other.
- Functions return integers only. Use storage or a module variable for other
  results.
- A bare expression such as `score + 1` produces no command and is rejected.
  Use an assignment, a call, a raw command, or `pass`.
- `for` loops iterate `range(...)`, literal arrays, and storage-backed lists.
- A name a function assigns is local to it unless the function declares it
  `global`. Reading a local before assigning it is a compile error instead of
  an `UnboundLocalError` at run time; see
  [Calling Convention](../reference/calling-convention.md#reads-before-assignment).
- Recursive functions work, and every call runs in the same game tick.
- `while` loops and runtime `for` loops compile to functions that call
  themselves, and every iteration runs in the same game tick.

## Browser Compiler

The browser compiler at `/try` compiles one in-memory file. Check projects
with local imports using the CLI, which resolves files from disk.
