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
- `--pack-format <NUM>` - Set the pack format version (default: `101.1`; Cobble v0.6.3 requires Minecraft Java Edition 26.1.2)
- `--template <NAME>` - Starter template: `minimal`, `stdlib`, or `validation` (default: `stdlib`)

**Example:**
```bash
cobble init --name my_datapack --description "My awesome data pack"
cobble init --name smoke_pack --template validation
```

After creation, Cobble prints the exact next commands for the selected target
directory, including `cobble build --dry-run`, `cobble build --validate`, and
`cobble watch`.

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

When `--validate` is enabled, Cobble fails the build if any generated command is
invalid for Minecraft Java Edition 26.1.2. If the default `data/commands.json`
is missing, Cobble downloads the Minecraft server jar and generates it
automatically. This requires `curl` and Java. Cobble tries Mojang's current
Piston manifest host, the legacy launcher manifest host, and a pinned 26.1.2
server jar URL.

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

### `cobble check`

Check Cobble source files for syntax errors without building.

```bash
cobble check [SOURCE]
```

**Example:**
```bash
cobble check src/main.cbl
cobble check examples/
```

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

**Examples:**
```bash
cobble doctor
cobble doctor examples/26_smoke --commands-json data/commands.json
```

The report includes the Cobble version, Minecraft target, pack format, Java and
`curl` availability, `cobble.toml` status, and the default command tree SHA-1
match when `data/commands.json` exists.

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

# Watch with all options
cobble watch src/ -o output/ --namespace mypack --zip --validate --verbose
```

This will:
1. Perform an initial build
2. Watch for changes in `.cbl` files
3. Automatically rebuild when files are modified
4. Show build status and any errors
5. Continue watching until Ctrl+C is pressed

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

Cobble v0.6.3 targets Minecraft Java Edition 26.1.2 and rejects other pack formats. This keeps generated data packs on the command and data pack schema version the compiler is tested against.

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

Note: Cobble v0.6.3 requires Minecraft Java Edition 26.1.2 and pack format 101.1.

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
