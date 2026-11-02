# CLI Commands

Run `cobble <command> --help` for all options. Project commands use
`cobble.toml` from the selected project; `build`, `check`, `watch`, and `fmt`
accept a source file or directory and default to the configured source.

## `cobble init`

Create a project with a config, source file, and `.gitignore`:

```bash
cobble init --name my_pack
cobble init --list-templates
```

Without `--name`, initializes the current directory. Use `--template <NAME>`
to select a starter; the default is `stdlib`.

## `cobble build`

Compile a project or source file:

```bash
cobble build --validate
cobble build src/main.cbl -o output --namespace mypack
```

| Option | Purpose |
| --- | --- |
| `-o, --output <DIR>` | Choose the output directory. |
| `--namespace <NAME>` | Override the project namespace. |
| `--validate` | Check generated Minecraft commands before replacing output. |
| `--zip` | Also write `<namespace>.zip` beside the output folder. |
| `--dry-run` | Compile and report without replacing output. |
| `--commands-json <PATH>` | Use a custom validation command tree. |
| `-q, --quiet` | Hide successful build progress; keep warnings and errors. |
| `-v, --verbose` | Show more build detail. |
| `--experimental-resource-pack` | Enable resource-pack output. |

The target pack format is `121.0`. Description and other defaults come from
[cobble.toml](config.md).

Use a new, empty, or Cobble-owned output directory. Existing output must
pass ownership checks; symlinks inside the output are not supported. A
validated build keeps the last output if compilation or validation fails.

`--dry-run --validate` validates temporary output. `--dry-run` cannot be
combined with `--zip`. `--zip` cannot target a world's `datapacks/` folder,
where both the folder and ZIP would load; build elsewhere and copy only the ZIP.

### Validation Setup

Without `--commands-json`, Cobble uses `data/commands.json` in the current
directory if present, otherwise a shared cache:

| Platform | Cache path |
| --- | --- |
| Linux | `$XDG_CACHE_HOME/cobble/26.3/commands.json`, or `~/.cache/cobble/26.3/commands.json` |
| macOS | `~/Library/Caches/cobble/26.3/commands.json` |
| Windows | `%LOCALAPPDATA%\cobble\26.3\commands.json` |

If missing, Cobble downloads the Minecraft server jar and generates the tree.
This needs Java and `curl`. Use `cobble doctor` to diagnose setup problems.
For offline or restricted environments:

| Environment variable | Purpose |
| --- | --- |
| `COBBLE_COMMANDS_JSON_AUTO_DOWNLOAD=0` | Fail instead of downloading missing validation data. |
| `COBBLE_COMMANDS_JSON_URL` | Download a prepared command tree. |
| `COBBLE_MINECRAFT_SERVER_JAR` | Use a local server jar. |
| `COBBLE_MINECRAFT_SERVER_URL` | Override the server jar download URL. |
| `COBBLE_MINECRAFT_SERVER_SHA1` | Set the expected server jar checksum. |
| `COBBLE_FUNCTION_PERMISSION_LEVEL` | Match a server's function permission level, `0`–`4` (default `2`). |

## `cobble check`

Report source errors and warnings without building:

```bash
cobble check
cobble check src/main.cbl --json
```

`--namespace <NAME>` overrides the namespace used for resource checks.
Warnings do not make the command fail; errors do. `--json` writes structured
[diagnostics](diagnostics.md) to stdout.

For editor symbols, plugin diagnostics, and the Python compatibility report,
see [Experimental Features](experimental.md).

## `cobble watch`

Rebuild after source or configuration changes:

```bash
cobble watch --validate
cobble watch --link --validate
```

Watch accepts the build options for output, namespace, description, pack
format, validation, ZIPs, and resource packs. `--link` uses the saved world
target and cannot be combined with `--output` or `--namespace`.
A failed validated rebuild keeps the last valid pack. Stop with Ctrl+C and
run `/reload` in Minecraft after a successful rebuild.

## `cobble fmt`

Format source files, or check formatting without writing:

```bash
cobble fmt src/
cobble fmt --check
cobble fmt --diff
```

`--check` and `--diff` exit non-zero if changes are needed. Formatting keeps
string contents and raw command payloads intact and refuses invalid source.

## `cobble validate`

Check an already generated pack:

```bash
cobble validate output
cobble validate output --commands-json /path/to/commands.json
```

Validation checks Minecraft command syntax, supported IDs and resources,
text components, and permission levels. It reports generated locations and
source locations when a source map is available. Runtime macro values and
resources from other namespaces still need testing in Minecraft.
See [Validation Setup](#validation-setup) for command-tree requirements.

## `cobble inspect`

Read a generated pack's build summary:

```bash
cobble inspect output
cobble inspect output --json
```

Inspect the output directory rather than its ZIP. The report reads the build
manifest and source map described in [Metadata Files](metadata.md).

## `cobble link`

Save a world target for `watch --link`:

```bash
cobble link --world ~/.minecraft/saves/MyWorld
cobble link --status
cobble watch --link --validate
```

Use `--datapacks <DIR>` for a direct target and `--pack-name <NAME>` to
change the output folder name. `--dry-run` previews the target; `--clear`
removes the saved link without deleting the pack.

Linked builds replace only output owned by the current project. If status
reports an unrelated or invalid pack, choose another target or move the
existing folder before rebuilding.

## `cobble clean`

Remove generated output belonging to the project:

```bash
cobble clean --dry-run
cobble clean
cobble clean --linked --dry-run
cobble clean --linked --yes
```

`--output <DIR>` selects another output. Cleanup requires a valid build
manifest, `pack.mcmeta`, and `data/`; unmarked or unrelated directories are
refused. Linked deletion also requires `--yes`.

## `cobble doctor`

Check project configuration, output ownership, validation data, and required
tools without accessing the network:

```bash
cobble doctor
cobble doctor --json
```

Use `--commands-json <PATH>` to inspect a custom tree. JSON status fields
let scripts distinguish missing tools, missing data, and invalid output.

## `cobble migrate`

Review changes needed to upgrade a project:

```bash
cobble migrate --json
cobble migrate --from 0.9 --to 0.11 --apply
```

The default route is 0.10 to 0.11. Supported starting versions are `0.8`,
`0.9`, and `0.10`; targets are `0.10` and `0.11`.

Report mode changes no files. `--apply` updates supported config settings
and writes a backup; source changes remain manual. See [Upgrading](../upgrading.md).
