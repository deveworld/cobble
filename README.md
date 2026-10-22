# Cobble

A Python-like language that compiles to Minecraft Java Edition data packs.

[![crates.io](https://img.shields.io/crates/v/cobble-lang.svg)](https://crates.io/crates/cobble-lang)
[![CI](https://github.com/deveworld/cobble/actions/workflows/ci.yml/badge.svg)](https://github.com/deveworld/cobble/actions/workflows/ci.yml)
[![Minecraft](https://img.shields.io/badge/minecraft-26.3-green.svg)](https://minecraft.wiki/w/Data_pack)
[![Docs](https://img.shields.io/badge/docs-book-7bd66f.svg)](https://deveworld.github.io/cobble/docs/)

- [Documentation (nightly)](https://deveworld.github.io/cobble/docs/)
- [Try it in the browser (nightly)](https://deveworld.github.io/cobble/try/)
- [Website](https://deveworld.github.io/cobble/)
- [Examples](examples/)

```python
wave = 0

def next_wave():
    global wave
    wave += 1
    /title @a title {"text":"Wave {wave}"}
    for i in range(3):
        /summon minecraft:zombie ~ ~ ~
    if wave % 5 == 0:
        /give @a minecraft:golden_apple
```

Each `def` becomes a function you can run with `/function`, lines that start
with `/` are Minecraft commands, and `{wave}` inserts the variable's value. The
[browser compiler](https://deveworld.github.io/cobble/try/) shows what it
compiles to.

## Getting Started

Install Cobble from crates.io:

```bash
cargo install cobble-lang --locked
```

This installs the `cobble` command.
[Installation](https://deveworld.github.io/cobble/docs/getting-started/installation.html)
lists the requirements.

Create a project and build it into a world's `datapacks` folder:

```bash
cobble init --name my_datapack
cd my_datapack
cobble build -o ~/.minecraft/saves/MyWorld/datapacks/my_datapack
```

Run `/reload` in the world to load the pack. With the same `-o` path,
`cobble watch` rebuilds the pack whenever a source file changes, and another
`/reload` loads the new build.

[Your First Pack](https://deveworld.github.io/cobble/docs/getting-started/first-pack.html)
walks through these steps in more detail.

## Features

- [Python-like syntax](https://deveworld.github.io/cobble/docs/language/basics.html)
  with indentation blocks,
  [control flow](https://deveworld.github.io/cobble/docs/language/control-flow.html)
  (`if`/`elif`/`else`, `for`, `while`, `match`),
  [functions](https://deveworld.github.io/cobble/docs/language/functions.html)
  with parameters and integer return values, and
  [modules](https://deveworld.github.io/cobble/docs/language/modules.html).
- [Raw Minecraft commands](https://deveworld.github.io/cobble/docs/language/commands.html)
  with `{placeholders}` for variables and parameters.
- [Command validation](https://deveworld.github.io/cobble/docs/reference/cli.html#cobble-build):
  `--validate` checks every generated command against the Minecraft 26.3
  server's own command tree at build time.
- [Variables](https://deveworld.github.io/cobble/docs/language/variables.html)
  in scoreboards and storage, with
  [type checking](https://deveworld.github.io/cobble/docs/language/basics.html#type-system).
- [Load and tick events](https://deveworld.github.io/cobble/docs/language/events.html).
- [Data pack resources](https://deveworld.github.io/cobble/docs/stdlib/datapack-resources.html)
  declared in code: tags, predicates, advancements, loot tables, recipes, item
  modifiers, and dialogs.
- [Standard library](https://deveworld.github.io/cobble/docs/stdlib/modules.html)
  helpers for text, scores, storage, timers, bossbars, teams, and entities.
- A [CLI](https://deveworld.github.io/cobble/docs/reference/cli.html) with
  project templates, a formatter, watch mode, and ZIP output.
- [Diagnostics](https://deveworld.github.io/cobble/docs/reference/diagnostics.html)
  with file, line, and column, also available as JSON.

## Status

Cobble 0.10.0 is the current release and targets Minecraft Java Edition 26.3
(pack format 121.0). The nightly documentation and browser compiler are built
from `main`, where 0.11 is in progress, so they may show features that 0.10.0
lacks.

Cobble is pre-1.0, so language behavior and generated output can still change
between minor releases.
[Upgrading](https://deveworld.github.io/cobble/docs/upgrading.html) covers each
release since 0.8, and `cobble migrate` flags some lines to check. Report bugs
on [GitHub Issues](https://github.com/deveworld/cobble/issues).

## Development

```bash
cargo test --locked
mdbook serve docs        # preview the docs (needs mdBook)
cd web && npm install && npm run dev
```

The documentation source is in [`docs/src/`](docs/src/), the website and
browser compiler are in [`web/`](web/), and the roadmap is [PLAN.md](PLAN.md).
[Testing And Releases](https://deveworld.github.io/cobble/docs/contributing/qa.html)
lists the checks to run before a change lands and how to install mdBook.

## License

MIT. See [LICENSE](LICENSE).
