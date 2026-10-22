# Cobble

Cobble is a Python-like language for Minecraft Java Edition data packs. You
write functions, events, conditions, loops, and resources in `.cbl` files, and
the compiler turns them into a data pack of `.mcfunction` files, function tags,
and JSON resources.

```python
from stdlib import event

def on_load():
    /tellraw @a {"text":"Cobble loaded","color":"green"}

def reward(player, amount):
    /give {player} minecraft:diamond {amount}

stdlib.addEventListener(event.LOAD, on_load)
```

Cobble targets Minecraft Java Edition 26.3 (pack format 121.0). The current
crates.io release is 0.10.0. This documentation follows the `main` branch,
where 0.11 is in development. What it says about 0.11, such as the per-pack
`<namespace>.vars` objective and `persist` variables, may change before the
release.

## Where To Start

- [Installation](getting-started/installation.md) and
  [Your First Pack](getting-started/first-pack.md) set up a project and load it
  in a world.
- The [Language Guide](language/basics.md) covers the syntax chapter by
  chapter.
- The [Standard Library](stdlib/modules.md) lists the built-in helpers and
  resource declarations.
- The [CLI reference](reference/cli.md) documents every command.

You can also try Cobble without installing anything in the
[browser compiler](https://deveworld.github.io/cobble/try/).

## Links

- Source and issues: [github.com/deveworld/cobble](https://github.com/deveworld/cobble)
- Rust library API: [docs.rs/cobble-lang](https://docs.rs/cobble-lang)
- [Changelog](https://github.com/deveworld/cobble/blob/main/CHANGELOG.md) and
  [roadmap](https://github.com/deveworld/cobble/blob/main/PLAN.md)
