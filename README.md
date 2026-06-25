# Cobble

> A modern, Python-like language for creating Minecraft Data Packs

[![Rust](https://img.shields.io/badge/rust-2021%20edition-orange.svg)](https://www.rust-lang.org)
[![Minecraft](https://img.shields.io/badge/minecraft-26.1.2-green.svg)](https://minecraft.net)
[![Pack Format](https://img.shields.io/badge/pack%20format-101.1-blue.svg)](https://minecraft.wiki/w/Data_pack)
[![Web Demo](https://img.shields.io/badge/web%20demo-live-7bd66f.svg)](https://deveworld.github.io/cobble/)

![Cobble web demo preview](https://raw.githubusercontent.com/deveworld/cobble/main/web/public/cobble-workshop.jpg)

Cobble transpiles `.cbl` source into Minecraft Java Edition data packs. It lets
you write functions, events, resources, and command-heavy logic with a
Python-like syntax while still emitting plain `.mcfunction`, `pack.mcmeta`, tag,
and JSON resource files.

**Cobble 0.9.0** targets Minecraft Java Edition 26.1.2 and pack format
101.1. It is the current stable crates.io release.

## Website And Web Demo

**Website:** <https://deveworld.github.io/cobble/>
**Browser compiler:** <https://deveworld.github.io/cobble/try/>

The website introduces Cobble with short examples and links to the browser
compiler at `/try`. The compiler runs Cobble's Rust parser and transpiler
through WebAssembly, so you can inspect generated functions, resources,
metadata, diagnostics, and data pack ZIP output without installing the CLI.

> [!WARNING]
> Cobble is pre-1.0 software in active development. Bugs and API changes are
> still possible. Please report issues at
> <https://github.com/deveworld/cobble/issues>.

## Features

- Python-like syntax for functions, indentation, imports, and control flow
- Static type inference for variables, constants, and expressions
- Data pack output for functions, event tags, predicates, advancements, loot
  tables, recipes, item modifiers, and dialogs
- Minecraft command handling with JSON safety, macro parameters, and command
  validation
- Standard library helpers for events, text, scoreboards, storage, item
  components, schedules, selector and position values, bossbars, teams, and
  entities
- CLI workflow for project templates, formatting, build, check, watch,
  validation, and ZIP output
- Browser playground powered by Rust and WebAssembly

## Installation

For the latest stable release:

```bash
cargo install cobble-lang --locked
cobble --version
```

For an exact version:

```bash
cargo install cobble-lang --version 0.9.0 --locked
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
| `cobble init --list-templates` | List starter templates |
| `cobble build` | Build source files into a data pack |
| `cobble check` | Check Cobble syntax without building |
| `cobble check --json --symbols` | Emit diagnostic JSON with experimental editor symbols |
| `cobble check --experimental-plugins` | Enable the experimental diagnostics-only plugin host and read-only manifest checks |
| `cobble check --experimental-python-compat` | Include the experimental Python compatibility report |
| `cobble fmt` | Format Cobble source files |
| `cobble fmt --diff` | Preview formatter changes without writing files |
| `cobble doctor` | Inspect project and validation environment status |
| `cobble doctor --json` | Emit machine-readable project health status |
| `cobble migrate` | Scan a project and report an experimental 0.8 to 0.9 migration plan without rewriting files |
| `cobble clean` | Remove marked Cobble-generated output |
| `cobble link` | Configure a local datapacks target |
| `cobble inspect` | Summarize generated build metadata |
| `cobble validate` | Validate generated `.mcfunction` files |
| `cobble watch` | Rebuild when source files change |

Use `cobble build --dry-run` to compile and inspect the output plan without
writing the final data pack. Each build writes `.cobble/build_manifest.json`
with the Cobble version, Minecraft target, pack format, source inputs,
generated resource entries, validation summary when available, and generated
file counts. Use `cobble inspect output/` to summarize that metadata after a
build. Use `cobble build --quiet` when a script only needs the exit code.

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

- [Language Reference](https://github.com/deveworld/cobble/blob/main/docs/language.md)
- [CLI Documentation](https://github.com/deveworld/cobble/blob/main/docs/cli.md)
- [API Reference](https://github.com/deveworld/cobble/blob/main/docs/api.md)
- [QA Checklist](https://github.com/deveworld/cobble/blob/main/docs/qa.md)
- [Examples](examples/)
- [Web Demo Source](https://github.com/deveworld/cobble/tree/main/web/)
- [Roadmap](PLAN.md)

## Support

- [GitHub Issues](https://github.com/deveworld/cobble/issues)

## License

Cobble is licensed under the MIT License. See [LICENSE](LICENSE).
