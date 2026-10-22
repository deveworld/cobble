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
- dotted or relative imports, import aliases, and `import a, b`;
- recursion, direct or through other functions.

`cobble check --experimental-python-compat` lists the supported Python-like
constructs and suggests a Cobble rewrite for unsupported ones; see
[Experimental Features](../reference/experimental.md).

## Values And Expressions

- Numbers are 32-bit integers. Floats are truncated, with a warning.
- Lists and maps live in storage. Subscripts work when the base resolves to a
  storage path. List and string indexes may be literals, constants, or runtime
  integers such as `items[i]`; map keys may be attributes, string literals, or
  runtime strings such as `config[key]`.
- Functions return integers only. Use storage or a module variable for other
  results.
- A bare expression such as `score + 1` produces no command and is rejected.
  Use an assignment, a call, a raw command, or `pass`.
- `for` loops iterate `range(...)`, literal arrays, and storage-backed lists.
- All of a pack's variables share one scoreboard objective, so there is no
  local scope; see [The `global` Keyword](variables.md#the-global-keyword).
- `while` loops and runtime `for` loops compile to functions that call
  themselves, and every iteration runs in the same game tick.

## Browser Compiler

The browser compiler at `/try` compiles one in-memory file. Check projects
with local imports using the CLI, which resolves files from disk.
