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

These are compile errors:

- the same function, selector alias, or entity template name in two files;
- a circular import (the message shows the import chain);
- a missing file (the message shows the importing file and the expected path).

Use one `import` per line. Import aliases (`import a as b`), wildcard imports,
`import a, b`, and dotted or relative module names are not supported.

## Building A Directory

`cobble build src/` compiles every file in the directory. With a
`cobble.toml`, `[build] entry_points` limits the build to the listed files, and
their imports are resolved from there. See
[Project Configuration](../reference/config.md).
