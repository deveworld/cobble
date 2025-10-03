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

## Running Examples

To compile an example:

```bash
# From the examples directory
cobble build hello_world.cbl -o output/hello_world

# Or with zip output
cobble build counter.cbl -o output/counter --zip
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
