# Cobble CLI Documentation

The Cobble command-line interface provides tools for creating, building, and managing Minecraft data pack projects.

## Installation

```bash
# Build from source
cargo build --release

# The binary will be at target/release/cobble
```

Add the binary to your PATH for easy access.

## Commands

### `cobble init`

Initialize a new Cobble project. Without `--name`, Cobble initializes the
current directory. With `--name <NAME>`, Cobble creates and initializes that
project directory.

```bash
cobble init [OPTIONS]
```

**Options:**
- `--name <NAME>` - Set the project name (default: current directory name)
- `--description <DESC>` - Set the project description
- `--pack-format <NUM>` - Set the pack format version (default: `101.1`; Cobble v0.7.3 requires Minecraft Java Edition 26.1.2)
- `--template <NAME>` - Starter template: `minimal`, `stdlib`,
  `validation`, `resource-heavy`, `game-mechanic`, or `web-demo`
  (default: `stdlib`)
- `--list-templates` - List available templates and exit without writing files

**Example:**
```bash
cobble init --name my_datapack --description "My awesome data pack"
cobble init --name smoke_pack --template validation
cobble init --name demo_pack --template web-demo
cobble init --list-templates
```

After creation, Cobble prints the exact next commands for the selected target
directory, including `cobble build --dry-run`, `cobble build --validate`, and
`cobble watch`.

Before writing project files, `init` refuses target paths that traverse an
existing symlink component.

This creates:
- `cobble.toml` - Project configuration file
- `src/main.cbl` - Main source file with example code
- `.gitignore` - Git ignore file

### `cobble build`

Compile Cobble source files into a Minecraft data pack.

```bash
cobble build [SOURCE] [OPTIONS]
```

**Arguments:**
- `SOURCE` - Source file or directory to compile. When omitted, Cobble uses `build.source` and `build.entry_points` from `cobble.toml`.

**Options:**
- `-o, --output <DIR>` - Output directory for the data pack (default: `./output`)
- `--namespace <NAME>` - Override the namespace (default: from cobble.toml or directory name)
- `--pack-format <NUM>` - Override pack format version (currently must be `101.1`)
- `--description <DESC>` - Override pack description
- `-v, --verbose` - Show verbose output
- `-q, --quiet` - Suppress successful build progress and summary output
- `--zip` - Create a ZIP archive of the data pack
- `--experimental-resource-pack` - Enable experimental `resource_pack.*` asset output under `assets/`
- `--validate` - Validate generated `.mcfunction` files after building
- `--dry-run` - Compile and print the build summary without writing final output
- `--commands-json <PATH>` - Path to `commands.json` for validation (default: `data/commands.json`)

**Examples:**
```bash
# Build current project
cobble build

# Build specific file
cobble build src/main.cbl

# Build with custom output
cobble build -o ~/minecraft/saves/MyWorld/datapacks/my_pack

# Build a single example file
cobble build examples/hello_world.cbl

# Build and create ZIP file
cobble build --zip

# Build and validate generated commands
cobble build --validate

# Build quietly for scripts
cobble build --quiet

# Compile and inspect the output plan without writing output
cobble build --dry-run

# Dry-run with command validation through a temporary staging output
cobble build --dry-run --validate

# Build with all options
cobble build src/ -o output/ --namespace mypack --pack-format 101.1 --zip --validate --verbose

# Build with the supported pack format explicitly
cobble build --pack-format 101.1
```

When a build would write files, Cobble refuses output paths that are existing
non-directories, traverse an existing symlink component, or contain existing
symlink descendants. This applies to normal builds, validated builds, and
`--dry-run --validate` staging output. A pure `--dry-run` does not write output.

When `--validate` is enabled, Cobble fails the build if any generated command is
invalid for Minecraft Java Edition 26.1.2. If the default `data/commands.json`
is missing, Cobble downloads the Minecraft server jar and generates it
automatically. This requires `curl` and Java. Cobble tries Mojang's current
Piston manifest host, the legacy launcher manifest host, and a pinned 26.1.2
server jar URL. The default `data/commands.json` path is refused if it would
traverse an existing symlink component; use an explicit `--commands-json` path
for a deliberate custom command tree.

`--dry-run` parses and transpiles sources, prints the same build summary, and
does not replace or clean the final output directory. When combined with
`--validate`, Cobble writes only a temporary staging data pack, validates it,
and removes the staging directory afterward. `--dry-run` cannot be combined
with `--zip`.

`--quiet` suppresses successful build progress and summary output. If validation
fails, Cobble still prints validation diagnostics before returning an error.
Compiler warnings are still shown. `--quiet` cannot be combined with
`--verbose`.

If your network blocks those endpoints, use one of these overrides:

```bash
COBBLE_COMMANDS_JSON_URL=https://example.com/commands.json cobble build --validate
COBBLE_MINECRAFT_SERVER_URL=https://example.com/server.jar cobble build --validate
COBBLE_MINECRAFT_SERVER_JAR=/path/to/server.jar cobble build --validate
COBBLE_MINECRAFT_SERVER_SHA1=<sha1> COBBLE_MINECRAFT_SERVER_URL=https://example.com/server.jar cobble build --validate
```

For custom command-tree paths, generate the file manually:

```bash
scripts/setup_commands_json.sh 26.1.2
cp data/commands.json /tmp/commands.json
```

Every non-dry-run build writes `.cobble/build_manifest.json` with the Cobble
version, Minecraft target, pack format, namespace, source input, configured
entry points, compiled files, generated namespaces, generated function/resource
counts, generated resource entries, and validation summary when validation ran.
`.cobble/source_map.json` is written when generated commands are available to
map generated commands back to Cobble source. Source-map file paths are written
relative to the project/source root when Cobble can determine one, avoiding
unnecessary absolute paths in generated metadata.
See [`metadata.md`](metadata.md) for the stable field list.

### `cobble check`

Check Cobble source files for language-surface and syntax errors without
building. The check command reports source file, line, and column for early
diagnostics such as unsupported Python-like syntax, missing imports, circular
imports, import item mistakes, indentation mistakes, duplicate imported or
directory-compiled symbols, undefined variables in standalone helper call
arguments, undefined Cobble function names, unknown helper module calls, and
language diagnostics in imported files.

```bash
cobble check [SOURCE] [OPTIONS]
```

**Options:**
- `--json` - Print a machine-readable report with `schema_version`, `ok`, `files`, `diagnostics`, and `error_count`
- `--symbols` - Include experimental document-symbol metadata; requires `--json`

**Example:**
```bash
cobble check src/main.cbl
cobble check examples/
cobble check --json src/main.cbl
cobble check --json --symbols src/main.cbl
```

JSON output is written to stdout. On failure the process still exits non-zero;
human-oriented error text may be written to stderr while stdout remains valid
JSON. `--symbols` adds an `experimental_symbols` array for editor prototypes;
that field is explicitly experimental even though the top-level JSON report has
a stable `schema_version`.

### `cobble fmt`

Format Cobble source files.

```bash
cobble fmt [SOURCE] [OPTIONS]
```

**Arguments:**
- `SOURCE` - Source file or directory to format. When omitted, Cobble uses
  `build.source` from `cobble.toml`. Formatting a directory formats all
  `.cbl` and `.cobble` files below it.

**Options:**
- `--check` - Check formatting without writing changes. The command exits
  non-zero when any file would be reformatted.
- `--diff` - Print formatter differences without writing changes. The command
  exits non-zero when any file would be reformatted.

**Examples:**
```bash
cobble fmt src/
cobble fmt --check examples/
cobble fmt --diff src/main.cbl
cobble fmt src/main.cbl
```

The formatter is conservative. It normalizes indentation, line endings,
trailing whitespace, a UTF-8 BOM, blank EOF padding, and the final newline
while preserving raw Minecraft command payloads, string contents, multiline
docstring bodies, inline JSON/SNBT-looking text, and comments. Cobble validates
the formatted candidate before writing; if any target file still has language
or syntax diagnostics, formatting aborts and no files are written. Formatting
refuses source paths and target files that traverse existing symlink
components.

### `cobble doctor`

Report project and validation environment status without contacting the
network.

```bash
cobble doctor [PROJECT_PATH] [OPTIONS]
```

**Arguments:**
- `PROJECT_PATH` - Project directory or file to inspect (default: current directory)

**Options:**
- `--commands-json <PATH>` - Command tree path to inspect (default: `data/commands.json`)
- `--json` - Print a machine-readable report with `schema_version`, top-level
  `status`, Cobble target metadata, config status, experimental output status,
  command tree status, experimental link status, and tool checks

**Examples:**
```bash
cobble doctor
cobble doctor examples/26_smoke --commands-json data/commands.json
cobble doctor --json
```

The report includes the Cobble version, Minecraft target, pack format, Java and
`curl` availability, `cobble.toml` status, configured output status, and the
default command tree SHA-1 match when `data/commands.json` exists.

JSON output is written to stdout. `doctor --json` never downloads validation
data; missing tools, missing config, and missing command trees are reported as
status fields so CI and editor integrations can decide how strict to be.
Configured output status is reported under `experimental_output`; it includes
the resolved output path, whether the directory exists, and whether it has a
Cobble build marker for the current project namespace and project identity. Link
status is reported under `experimental_link`; it includes saved link state,
whether the linked pack currently has Cobble build metadata, and whether that
metadata matches the project namespace and `project_id`.

### `cobble clean`

Remove Cobble-generated output after safety checks.

```bash
cobble clean [PROJECT_PATH] [OPTIONS]
```

**Arguments:**
- `PROJECT_PATH` - Project directory or file used to find `cobble.toml`
  (default: current directory)

**Options:**
- `-o, --output <DIR>` - Output directory to clean. When omitted, Cobble uses
  `build.output` from `cobble.toml`.
- `--dry-run` - Print what would be removed without deleting files
- `--linked` - Clean the pack path configured by `cobble link`
- `--yes` - Confirm destructive linked cleanup

**Examples:**
```bash
cobble clean --dry-run
cobble clean --output output
cobble clean --linked --dry-run
cobble clean --linked --yes
```

`clean` only removes directories that look like Cobble-generated data pack
output. The target must be a directory and contain `.cobble/build_manifest.json`,
`pack.mcmeta`, and `data/`. Cobble refuses symlink outputs, existing symlink
parent components, symlink descendants, non-directories, unmarked directories,
and a configured output that resolves to the project root.
Linked cleanup also requires the saved link state to resolve under the saved
`datapacks/` target and the linked pack manifest namespace and `project_id` to
match the current project. Linked cleanup requires `--yes` for real deletion;
`--dry-run` does not require confirmation.

`clean --dry-run` prints the marker path that made the output eligible for
cleanup, the marker namespace, the marker `project_id` when present, the
required data pack files that were checked, and a symlink safety summary. For
linked output, the dry run ends with the exact confirmation shape:
`cobble clean --linked --yes`.

### `cobble link`

Configure a local Minecraft `datapacks/` target for watch workflows.

```bash
cobble link [PROJECT_PATH] [OPTIONS]
```

**Arguments:**
- `PROJECT_PATH` - Project directory or file used to find `cobble.toml`
  (default: current directory)

**Options:**
- `--datapacks <DIR>` - Direct path to a world `datapacks/` directory
- `--world <DIR>` - Path to a world directory; Cobble uses `<DIR>/datapacks`
- `--minecraft <DIR>` - Path to `.minecraft`; Cobble uses
  `<DIR>/saves/<pack-name>/datapacks`
- `--pack-name <NAME>` - Directory name for the linked data pack
  (default: project namespace)
- `--dry-run` - Show the resolved target without writing link state
- `--status` - Show saved link state and whether generated Cobble metadata is
  present at the linked pack path
- `--clear` - Remove saved link state without deleting the linked data pack

**Examples:**
```bash
cobble link --datapacks ~/minecraft/saves/TestWorld/datapacks
cobble link --world ~/minecraft/saves/TestWorld
cobble link --status
cobble link --clear
```

`link` writes project-local state to `.cobble/link_state.json`. It creates the
target `datapacks/` directory when configuring a real link, but it does not
delete or replace existing data packs. Link state reads, writes, and clears
refuse symlink components in the project-local `.cobble/link_state.json` path.
`link --status`, `doctor --json`, `watch --link`, and `clean --linked` all
reject a saved `pack_path` that is not under the saved `target_path` or would
traverse an existing target symlink. Use `cobble watch --link` to build into
the configured pack path. If that pack path already exists, `watch --link`
requires a valid `.cobble/build_manifest.json`, `pack.mcmeta`, and `data/`; the
manifest namespace and `project_id` must match the current project. Copied,
stale, or namespace-only forged markers are refused
until the path is moved aside or rebuilt by the owning Cobble project.
`link --status` includes recovery hints for the common cases: configure a
missing link with `cobble link --datapacks <DIR>`, clear and recreate tampered
link state with `cobble link --clear`, create a missing linked pack with
`cobble watch --link`, or move aside an unrelated pack before rebuilding.

### `cobble inspect`

Summarize Cobble metadata in a generated data pack directory.

```bash
cobble inspect <DATAPACK_DIR> [OPTIONS]
```

**Arguments:**
- `DATAPACK_DIR` - Generated data pack directory containing `.cobble/build_manifest.json`

**Options:**
- `--json` - Print the manifest and source-map entry count as formatted JSON

**Examples:**
```bash
cobble build --validate -o output
cobble inspect output
cobble inspect output --json
```

The command reads `.cobble/build_manifest.json` and, when present,
`.cobble/source_map.json`. ZIP archives created by `cobble build --zip` include
only data pack files (`pack.mcmeta` and `data/**`), so inspect a generated
directory before or alongside ZIP packaging.

### `cobble validate`

Validate generated `.mcfunction` files against Minecraft Java Edition 26.1.2's command tree.

```bash
cobble validate <DATAPACK_DIR> [OPTIONS]
```

**Arguments:**
- `DATAPACK_DIR` - Generated data pack directory to validate

**Options:**
- `--commands-json <PATH>` - Path to `commands.json` generated from the Minecraft server reports (default: `data/commands.json`; auto-generated when missing)

**Examples:**
```bash
# Build and validate a data pack; data/commands.json is generated if missing
cobble build -o output
cobble validate output

# Use a custom command tree path
scripts/setup_commands_json.sh 26.1.2
cp data/commands.json /tmp/commands.json
cobble validate output --commands-json /tmp/commands.json
```

The validator uses Minecraft's exported Brigadier command tree, including 26.1.2 commands such as `dialog`, `fetchprofile`, `transfer`, `waypoint`, `stopwatch`, `version`, and `return run`.
Automatic generation for the default `data/commands.json` path refuses existing
symlink components before creating temporary files or replacing the command
tree.

Validation output includes the number of macro commands checked and skipped.
When the validator can identify an error position, it prints a caret under the
generated command text. If `.cobble/source_map.json` is present, diagnostics also
include the originating Cobble source location when available.

### `cobble watch`

Watch source files for changes and automatically rebuild.

```bash
cobble watch [SOURCE] [OPTIONS]
```

**Arguments:**
- `SOURCE` - Source file or directory to watch. When omitted, Cobble uses `build.source` from `cobble.toml`.

**Options:**
- `-o, --output <DIR>` - Output directory for the data pack
- `--namespace <NAME>` - Override the namespace
- `--pack-format <NUM>` - Override pack format version
- `--description <DESC>` - Override pack description
- `-v, --verbose` - Show verbose output
- `--zip` - Create a ZIP archive after each build
- `--experimental-resource-pack` - Enable experimental `resource_pack.*` asset output under `assets/`
- `--link` - Build into the pack path configured by `cobble link`
- `--validate` - Validate generated `.mcfunction` files after each successful build
- `--commands-json <PATH>` - Path to `commands.json` for validation (default: `data/commands.json`)

**Examples:**
```bash
# Watch current directory
cobble watch

# Watch with custom output
cobble watch src/ -o ~/minecraft/saves/MyWorld/datapacks/my_pack

# Watch and validate after each rebuild
cobble watch src/ --validate

# Watch into a configured linked datapack
cobble link --datapacks ~/minecraft/saves/TestWorld/datapacks
cobble watch --link --validate

# Watch with all options
cobble watch src/ -o output/ --namespace mypack --zip --validate --verbose
```

`--link` cannot be combined with `--output` or `--namespace`. Linked outputs use
the project namespace from `cobble.toml` so `link --status`, `doctor --json`,
`watch --link`, and `clean --linked` can validate the same manifest ownership
marker.

This will:
1. Perform an initial build
2. Watch for changes in `.cbl`, `.cobble`, and `cobble.toml` files
3. Coalesce rapid editor save events into one rebuild
4. Ignore generated output, `.cobble/`, staging directories, ZIP files, and
   common editor temporary files
5. Reload valid `cobble.toml` changes and update the watched source directory
6. Show timestamped build status and any errors
7. Continue watching until Ctrl+C is pressed

When `--validate` is enabled, watch uses the same staging behavior as
`cobble build --validate`: a failed rebuild or validation failure does not
replace the last valid output.

## Project Configuration

### cobble.toml

The project configuration file uses TOML format:

```toml
[project]
name = "my_datapack"
description = "My awesome data pack"
namespace = "mypack"
version = "1.0.0"
pack_format = "101.1"  # Minecraft Java Edition 26.1.2

[build]
source = "src"
output = "output"
entry_points = []
```

**Configuration Options:**

- `project.name` - Project name
- `project.description` - Pack description (shown in Minecraft)
- `project.version` - Project version (for your reference)
- `project.namespace` - Pack namespace (must be lowercase, no spaces)
- `project.pack_format` - Minecraft pack format version
- `build.output` - Default output directory
- `build.source` - Source directory (default: "src")
- `build.entry_points` - Main files or directories to compile when using `cobble build` from config. Imported files are resolved from these entry points and are not compiled independently.

## Supported Minecraft Version

| Minecraft Version | Pack Format |
|-------------------|-------------|
| Java Edition 26.1.2 | 101.1 |

Cobble v0.7.3 targets Minecraft Java Edition 26.1.2 and rejects other pack formats. This keeps generated data packs on the command and data pack schema version the compiler is tested against.

**Note**: Pack format 101.1 is written to `pack.mcmeta` as `min_format` and `max_format` arrays: `[101, 1]`.

## Workflow

### 1. Create a New Project

```bash
mkdir my_datapack
cd my_datapack
cobble init
```

### 2. Edit Your Code

Edit `src/main.cbl`:

```python
import stdlib
from stdlib import event

def on_load():
    /tellraw @a {"text":"Hello from Cobble!", "color":"green"}

stdlib.addEventListener(event.LOAD, on_load)
```

### 3. Build the Data Pack

```bash
cobble build -o ~/minecraft/saves/MyWorld/datapacks/my_datapack
```

### 4. Test in Minecraft

1. Open Minecraft
2. Load your world
3. Run `/reload` in-game
4. Your data pack should load and run!

### 5. Development Workflow

For faster development, use watch mode:

```bash
cobble watch src/ -o ~/minecraft/saves/MyWorld/datapacks/my_datapack
```

Then in Minecraft, just run `/reload` whenever you make changes.

## Output Structure

The compiled data pack follows the standard Minecraft structure:

```
datapack/
├── pack.mcmeta
└── data/
    ├── minecraft/
    │   └── tags/
    │       └── function/
    │           ├── load.json
    │           └── tick.json
    └── your_namespace/
        └── function/
            ├── main.mcfunction
            ├── on_load.mcfunction
            ├── on_tick.mcfunction
            └── ...
```

## Error Messages

Cobble provides clear error messages when compilation fails:

```
Error: Parse error at line 5
Expected ':', found 'def'

4 | def my_function()
5 |     /say Hello
    | ^^^
```

## Tips and Tricks

### 1. Multiple Source Files

You can organize your code across multiple files:

```bash
src/
├── main.cbl      # Entry point
├── player.cbl    # Player-related functions
├── world.cbl     # World-related functions
└── utils.cbl     # Utility functions
```

Build the entire directory:

```bash
cobble build src/
```

### 2. Quick Testing

Create a test world specifically for data pack development:

```bash
# Build directly into test world
cobble build -o ~/.minecraft/saves/TestWorld/datapacks/mypack
```

### 3. Version Control

Always commit your `cobble.toml` and source files, but add build output to `.gitignore`:

```gitignore
datapack/
*.mcfunction
target/
```

### 4. Debugging

Use `cobble check` to quickly validate syntax:

```bash
# Check before building
cobble check src/main.cbl && cobble build
```

### 5. Custom Namespaces

Use meaningful namespaces to avoid conflicts:

```bash
cobble build --namespace myname_mypack
```

## Integration with Game

### Loading Your Data Pack

1. Place compiled data pack in `saves/YourWorld/datapacks/`
2. In-game, run `/reload`
3. Check with `/datapack list`

### Enabling/Disabling

```minecraft
/datapack enable "file/your_pack"
/datapack disable "file/your_pack"
```

### Debugging in Game

```minecraft
/datapack list  # List all data packs
/function your_namespace:function_name  # Manually run a function
/scoreboard objectives list  # List scoreboards
```

## Advanced Usage

### Escaping Special Characters

When you need to use literal braces in commands:

```python
def test():
    # This gives to a player named "Steve"
    /give {player} diamond 1  # Where player is a parameter

    # This gives to a player literally named "{player}"
    /give {{player}} diamond 1  # Escaped braces
```

Generated output:
```mcfunction
$give $(player) diamond 1
give {player} diamond 1
```

## Common Issues

### Issue: Pack doesn't load

**Solution:** Check pack format matches your Minecraft version

```bash
cobble build --pack-format 101.1
```

Note: Cobble v0.7.3 requires Minecraft Java Edition 26.1.2 and pack format 101.1.

### Issue: Functions not found

**Solution:** Run `/reload` in-game after building

### Issue: Syntax errors

**Solution:** Use `cobble check` first:

```bash
cobble check src/
```

### Issue: Changes not appearing

**Solution:** Make sure you're using `/reload` and the correct output path

## Next Steps

- Read the [Language Reference](language.md) for syntax details
- Check out [examples](../examples/) for sample code
- Learn about the [API](api.md) for advanced usage

## Getting Help

- GitHub Issues: https://github.com/deveworld/cobble/issues
- Documentation: https://github.com/deveworld/cobble/tree/main/docs
- Minecraft Wiki: https://minecraft.wiki/w/Data_pack
