# Installation

Cobble is a Rust program published on crates.io as `cobble-lang`. Install it
with Cargo, which [rustup](https://rustup.rs) installs along with Rust. The
installed command is `cobble`.

```bash
cargo install cobble-lang --locked
cobble --version
```

For a specific version:

```bash
cargo install cobble-lang --version 0.10.0 --locked
```

To build from source:

```bash
git clone https://github.com/deveworld/cobble.git
cd cobble
cargo build --release
# the binary is target/release/cobble
```

## Requirements

- A current stable Rust toolchain to install or build Cobble.
- Minecraft Java Edition 26.3 to load the generated packs.
- Java and `curl` for command validation (`cobble build --validate`). The first
  validated build downloads the Minecraft server jar and saves its command
  tree in your user cache folder, where later builds in any project reuse it.
  [`cobble build`](../reference/cli.md#cobble-build) lists the cache path for
  each system. Builds without `--validate` need neither.

`cobble doctor` reports what is installed and whether the command tree matches
the supported Minecraft version.
