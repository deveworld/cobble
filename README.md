# Cobble

> A modern, Python-like language for creating Minecraft Data Packs

[![Rust](https://img.shields.io/badge/rust-2021%20edition-orange.svg)](https://www.rust-lang.org)
[![Minecraft](https://img.shields.io/badge/minecraft-26.1.2-green.svg)](https://minecraft.net)
[![Pack Format](https://img.shields.io/badge/pack%20format-101.1-blue.svg)](https://minecraft.wiki/w/Data_pack)
[![Web Demo](https://img.shields.io/badge/web%20demo-live-7bd66f.svg)](https://deveworld.github.io/cobble/)

![Cobble web demo preview](web/public/cobble-workshop.jpg)

Cobble transpiles `.cbl` source into Minecraft Java Edition data packs. It lets
you write functions, events, resources, and command-heavy logic with a
Python-like syntax while still emitting plain `.mcfunction`, `pack.mcmeta`, tag,
and JSON resource files.

**Version 0.6.1** targets Minecraft Java Edition 26.1.2 and pack format 101.1.

## Web Demo

**Live demo:** <https://deveworld.github.io/cobble/>

The browser demo runs Cobble's Rust parser and transpiler through WebAssembly,
so you can edit Cobble code and inspect the generated data pack files without
installing the CLI.

> [!WARNING]
> Cobble is in active development (v0.6.1 Pre-Alpha). Bugs and API changes are
> expected. Please report issues at
> <https://github.com/deveworld/cobble/issues>.

## Features

- Python-like syntax for functions, indentation, imports, and control flow
- Static type inference for variables, constants, and expressions
- Data pack output for functions, event tags, predicates, advancements, loot
  tables, recipes, item modifiers, and dialogs
- Minecraft command handling with JSON safety, macro parameters, and command
  validation
- Standard library helpers for events, text, scoreboards, storage, schedules,
  bossbars, teams, and entities
- CLI workflow for project templates, build, check, watch, validation, and ZIP
  output
- Browser playground powered by Rust and WebAssembly

## Installation

```bash
cargo install cobble-lang
cobble --help
```

The crates.io package is named `cobble-lang`; the installed command is
`cobble`.

To build from source:

```bash
git clone https://github.com/deveworld/cobble.git
cd cobble
cargo build --release
```

## Quick Start

```bash
cobble init --name my-datapack
cd my-datapack
```

Edit `src/main.cbl`:

```python
import stdlib
from stdlib import event

def on_load():
    /tellraw @a {"text":"Cobble loaded","color":"green"}

def reward(player, amount):
    /give {player} minecraft:diamond {amount}

stdlib.addEventListener(event.LOAD, on_load)
```

Build the data pack:

```bash
cobble build --validate --zip
```

Copy the generated output to your world's `datapacks` directory:

```text
.minecraft/saves/YourWorld/datapacks/
```

## Commands

| Command | Purpose |
| ------- | ------- |
| `cobble init` | Create a new project |
| `cobble build` | Build source files into a data pack |
| `cobble check` | Check Cobble syntax without building |
| `cobble validate` | Validate generated `.mcfunction` files |
| `cobble watch` | Rebuild when source files change |

`cobble build --validate` and `cobble validate` check generated `.mcfunction`
files against Minecraft Java Edition 26.1.2 commands. The default
`data/commands.json` file is generated automatically on first validation when
missing; this requires `curl` and Java.

## Configuration

`cobble.toml` controls the project namespace, pack metadata, source directory,
output directory, and entry points.

```toml
[project]
name = "my-datapack"
namespace = "my_namespace"
version = "1.0.0"
pack_format = "101.1"

[build]
source = "src"
output = "output"
entry_points = []
```

## Development

```bash
cargo test --locked

cd web
npm install
npm run dev
```

## Documentation

- [Language Reference](docs/language.md)
- [CLI Documentation](docs/cli.md)
- [API Reference](docs/api.md)
- [Examples](examples/)
- [Web Demo Source](web/)
- [Roadmap](PLAN.md)

## Support

- [GitHub Issues](https://github.com/deveworld/cobble/issues)

## License

Cobble is licensed under the MIT License. See [LICENSE](LICENSE).
