# Cobble

[![crates.io](https://img.shields.io/crates/v/cobble-lang.svg)](https://crates.io/crates/cobble-lang)
[![CI](https://github.com/deveworld/cobble/actions/workflows/ci.yml/badge.svg)](https://github.com/deveworld/cobble/actions/workflows/ci.yml)
[![Minecraft](https://img.shields.io/badge/minecraft-26.3-green.svg)](https://minecraft.wiki/w/Data_pack)
[![Docs](https://img.shields.io/badge/docs-book-7bd66f.svg)](https://deveworld.github.io/cobble/docs/)

Cobble is a Python-like language for Minecraft Java Edition data packs. You
write functions, conditions, loops, events, and resources in `.cbl` files, and
the compiler turns them into an ordinary data pack: `.mcfunction` files,
function tags, and JSON resources that you can read, edit, and ship.

**[Website](https://deveworld.github.io/cobble/)** ·
**[Documentation](https://deveworld.github.io/cobble/docs/)** ·
**[Try it in the browser](https://deveworld.github.io/cobble/try/)** ·
**[Examples](examples/)**

![Cobble web demo preview](https://raw.githubusercontent.com/deveworld/cobble/main/web/public/cobble-workshop.jpg)

## Example

```python
from stdlib import event

countdown = 0

def start():
    global countdown
    countdown = 100

def tick():
    global countdown
    if countdown > 0:
        countdown -= 1
        /title @a actionbar {"text":"Starting in {countdown}"}
        if countdown == 0:
            /title @a title {"text":"Go!","color":"green"}

stdlib.addEventListener(event.TICK, tick)
```

With the namespace `race`, `tick` and its helper compile to:

```mcfunction
# race:tick
execute if score countdown race.vars matches 1.. run function race:if_temp_0

# race:if_temp_0
scoreboard players remove countdown race.vars 1
title @a actionbar {"extra":[{"score":{"name":"countdown","objective":"race.vars"}}],"text":"Starting in "}
execute if score countdown race.vars matches 0 run title @a title {"text":"Go!","color":"green"}
```

Variables become scoreboard values, an `if` with more than one command becomes
a helper function, and `{countdown}` inside the JSON text becomes a score
component. `cobble build --validate` checks every generated command against
the Minecraft 26.3 command tree.

## Install

```bash
cargo install cobble-lang --locked
cobble --version
```

The crates.io package is `cobble-lang`; the command it installs is `cobble`.
To build from source, clone the repository and run `cargo build --release`.

## Quick Start

```bash
cobble init --name my_datapack
cd my_datapack
cobble build --validate -o ~/.minecraft/saves/MyWorld/datapacks/my_datapack
```

Run `/reload` in the world to load the pack. `cobble watch` with the same `-o`
path rebuilds on every save. [Your First Pack](https://deveworld.github.io/cobble/docs/getting-started/first-pack.html)
walks through the rest.

## What You Get

- Python-like syntax: indentation blocks, `if`/`elif`/`else`, `for`, `while`,
  `match`, functions with parameters and integer return values, and imports.
- Raw Minecraft commands with `{placeholders}` for variables and parameters.
- Variables in scoreboards and storage, with type checking and `persist`
  variables that survive `/reload`.
- `datapack.*` declarations for tags, predicates, advancements, loot tables,
  recipes, item modifiers, and dialogs.
- Standard library helpers for text, scores, storage, timers, bossbars, teams,
  and entities.
- A CLI with project templates, a formatter, watch mode, command validation,
  and ZIP output.
- Source-aware diagnostics with file, line, and column, also available as JSON.

## Status

Cobble 0.10.0 is the current release on crates.io and targets Minecraft Java
Edition 26.3 (pack format 121.0). The `main` branch is working toward 0.11,
which so far gives each pack its own variable objective and adds `persist`
variables.

Cobble is pre-1.0: language behavior and generated output can still change
between minor releases, and each such change comes with notes in
[Upgrading](https://deveworld.github.io/cobble/docs/upgrading.html) and a
`cobble migrate` report. Please report bugs at
[GitHub Issues](https://github.com/deveworld/cobble/issues).

## Development

```bash
cargo test --locked
mdbook serve docs        # preview the documentation
cd web && npm install && npm run dev
```

The documentation source is in [`docs/src/`](docs/src/), the website and
browser compiler are in [`web/`](web/), and the roadmap is [PLAN.md](PLAN.md).
[Testing And Releases](https://deveworld.github.io/cobble/docs/contributing/qa.html)
lists the checks to run before a change lands.

## License

MIT. See [LICENSE](LICENSE).
