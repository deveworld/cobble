# Cobble Examples

This directory contains example Cobble programs demonstrating various language features.

## Basic Examples

- **hello_world.cbl** - Simple "Hello World" example
- **counter.cbl** - Tick counter with module-level variables
- **functions.cbl** - Function parameters and macro system

## Control Flow

- **conditionals.cbl** - If/elif/else and match statements
- **loops.cbl** - For loops and while loops

## Advanced Features

- **execute_blocks.cbl** - Execute command modifiers
- **selectors.cbl** - Custom selector definitions
- **arithmetic.cbl** - Arithmetic operations and precedence
- **type_system.cbl** - Static type system examples

## Project Fixtures

- **26_smoke/** - Minecraft Java Edition 26.1.2 command smoke project with imports, events, macros, and latest commands.
- **26_feature_matrix/** - Full project fixture covering control flow, storage, stdlib v1.1 helpers, explicit namespaced JSON resources, and build validation.

## Running Examples

To compile an example:

```bash
# From the examples directory
cobble build hello_world.cbl -o output/hello_world

# Or with zip output
cobble build counter.cbl -o output/counter --zip

# Project fixtures use cobble.toml; run from the fixture directory
cd 26_feature_matrix
cobble build --validate
```

To run the optional real-server QA path from the repository root:

```bash
COBBLE_MINECRAFT_EULA_ACCEPTED=1 scripts/test_minecraft_server.sh
```

## Learning Path

Recommended order for learning:

1. Start with **hello_world.cbl** - Basic syntax
2. Try **functions.cbl** - Function parameters
3. Explore **conditionals.cbl** - Control flow
4. Practice with **loops.cbl** - Iteration
5. Study **type_system.cbl** - Type safety
6. Advanced: **execute_blocks.cbl** and **selectors.cbl**

Each example is self-contained and demonstrates specific language features.
