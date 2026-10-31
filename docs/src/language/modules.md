# Modules And Imports

A project can split its code across several `.cbl` files:

```text
src/
├── main.cbl
├── player.cbl
└── utils.cbl
```

`import name` loads `name.cbl` from the importing file's directory:

```python
# utils.cbl
def helper_function():
    /say Helper called

@AllPlayers = @a[gamemode=!spectator]
```

```python
# main.cbl
import utils

def test():
    helper_function()
    utils.helper_function()
    as @AllPlayers:
        /say Test
```

Imported functions, selector aliases, and entity templates join one shared set
of names, so a function can be called as `helper_function()` or
`utils.helper_function()`. `from utils import helper_function` also checks that
the name exists in `utils.cbl`. Imported functions that return an integer can
be used in expressions and conditions.

Module variables are shared the same way: every file that names `score` reads
and changes one value, and `from utils import score` checks that the name
exists in `utils.cbl`. So define each module variable at module level in one
file only, and use it from the others.

An imported file is compiled before the module code of the file that imports
it. So a function of `utils.cbl` can't use a list, map, or string that only
`main.cbl` defines, as Python raises `NameError` there: it is a compile error
that names the variable and its file. Pass the value as a parameter, or
define it in `utils.cbl` and import it into `main.cbl`.

These are compile errors:

- the same function, selector alias, entity template, or module variable name
  in two files;
- a circular import (the message shows the import chain);
- a missing file (the message shows the importing file and the expected path).

Use one `import` per line. Import aliases (`import a as b`), wildcard imports,
`import a, b`, and dotted or relative module names are not supported.

## Building A Directory

`cobble build src/` compiles every file in the directory. With a
`cobble.toml`, `[build] entry_points` limits the build to the listed files, and
their imports are resolved from there. See
[Project Configuration](../reference/config.md).
