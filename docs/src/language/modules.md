# Modules And Imports

Split a project into `.cbl` files in the same directory:

```python
# utils.cbl
def greet():
    /say Hello!
```

```python
# main.cbl
import utils

def start():
    utils.greet()
```

`import utils` loads `utils.cbl` from the importing file's directory.
`from utils import greet` also checks that `greet` exists. Imported functions
can be called as either `utils.greet()` or `greet()`.

## Shared Names

Functions, module variables, selector aliases, and entity templates share
one set of names across a project. Define each name in one file. Circular
imports and conflicting definitions are compile errors.

Imported module code runs before the importing module's code. Define shared
data in the imported file or pass it as a parameter.

Use one import per line. Aliases, wildcard imports, dotted or relative
module names, and `import a, b` are not supported. For built-in helpers, see
[Standard Library Modules](../stdlib/modules.md).

## Building A Directory

`cobble build src/` compiles every Cobble file in the directory. To choose
starting files and follow their imports, set `[build] entry_points` in
[cobble.toml](../reference/config.md).
