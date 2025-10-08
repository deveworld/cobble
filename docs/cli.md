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

Initialize a new Cobble project in the current directory.

```bash
cobble init [OPTIONS]
```

**Options:**
- `--name <NAME>` - Set the project name (default: current directory name)
- `--description <DESC>` - Set the project description
- `--pack-format <NUM>` - Set the pack format version (default: 18 for Minecraft 1.20.2+, supports decimal like 88.0)

**Example:**
```bash
cobble init --name my_datapack --description "My awesome data pack"
```

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
- `SOURCE` - Source file or directory to compile (default: current directory)

**Options:**
- `-o, --output <DIR>` - Output directory for the data pack (default: `./output`)
- `--namespace <NAME>` - Override the namespace (default: from cobble.toml or directory name)
- `--pack-format <NUM>` - Override pack format version (supports decimal like 88.0 for Minecraft 1.21.9+)
- `--description <DESC>` - Override pack description
- `-v, --verbose` - Show verbose output
- `--zip` - Create a ZIP archive of the data pack

**Examples:**
```bash
# Build current project
cobble build

# Build specific file
cobble build src/main.cbl

# Build with custom output
cobble build -o ~/minecraft/saves/MyWorld/datapacks/my_pack

# Build entire directory
cobble build examples/

# Build and create ZIP file
cobble build --zip

# Build with all options
cobble build src/ -o output/ --namespace mypack --pack-format 88 --zip --verbose

# Build with decimal pack format (Minecraft 1.21.9+)
cobble build --pack-format 88.0
```

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

### `cobble watch`

Watch source files for changes and automatically rebuild.

```bash
cobble watch [SOURCE] [OPTIONS]
```

**Arguments:**
- `SOURCE` - Source file or directory to watch (default: current directory)

**Options:**
- `-o, --output <DIR>` - Output directory for the data pack
- `--namespace <NAME>` - Override the namespace
- `--pack-format <NUM>` - Override pack format version
- `--description <DESC>` - Override pack description
- `-v, --verbose` - Show verbose output
- `--zip` - Create a ZIP archive after each build

**Examples:**
```bash
# Watch current directory
cobble watch

# Watch with custom output
cobble watch src/ -o ~/minecraft/saves/MyWorld/datapacks/my_pack

# Watch with all options
cobble watch src/ -o output/ --namespace mypack --zip --verbose
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
pack_format = 88  # Minecraft 1.21.9+

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

## Pack Format Versions

| Minecraft Version | Pack Format |
|-------------------|-------------|
| 1.21.9+           | 88.0        |
| 1.21.7 - 1.21.8   | 81          |
| 1.21.6            | 80          |
| 1.21.5            | 71          |
| 1.21.4            | 61          |
| 1.21.2 - 1.21.3   | 57          |
| 1.21 - 1.21.1     | 48          |
| 1.20.5 - 1.20.6   | 41          |
| 1.20.3 - 1.20.4   | 26          |
| 1.20.2            | 18 (default) |
| 1.20 - 1.20.1     | 15          |

Cobble requires Minecraft 1.20.2+ (minimum pack format 18) for function macro support and defaults to pack format 18 for maximum compatibility across Minecraft versions.

**Note**: Starting from Minecraft 1.21.9, pack format includes minor versions (e.g., 88.0). Cobble uses integer pack format internally, which is compatible with both formats.

## Workflow

### 1. Create a New Project

```bash
mkdir my_datapack
cd my_datapack
cobble init --name my_datapack
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
cobble build --pack-format 48  # For Minecraft 1.21-1.21.1
cobble build --pack-format 81  # For Minecraft 1.21.7-1.21.8
cobble build --pack-format 88  # For Minecraft 1.21.9+
cobble build --pack-format 88.0  # For Minecraft 1.21.9+ (decimal format)
```

Note: Cobble requires Minecraft 1.20.2+ (pack format 18 or higher) for macro function support

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