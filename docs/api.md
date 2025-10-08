# Cobble API Documentation

This document describes the internal API for Cobble's compiler and tools.

## Overview

Cobble consists of several modules that work together to compile high-level code into Minecraft data packs:

- **Parser** - Converts source code into an Abstract Syntax Tree (AST)
- **Transpiler** - Transforms AST into Minecraft commands
- **Standard Library** - Provides event handling and utilities
- **CLI** - Command-line interface for building projects

## Module: Parser

The parser module is organized into several files:

### `parser/mod.rs`

Public API for the parser module. Exports the main `parse` function and re-exports components from submodules.

### `parser/tokenizer.rs`

Manual tokenizer that handles indentation with INDENT/DEDENT tokens and distinguishes `/` division from `/` command prefix.

**Key Features:**
- Tracks indentation levels to generate INDENT/DEDENT tokens
- Handles string literals with proper escaping
- Distinguishes between division operator (`a / b`) and command prefix (`/say`)
- Processes inline comments (starting with `#`)

### `parser/combinators.rs`

The main parser built with chumsky combinator library that handles Python-style indented syntax.

#### Function: `parse(source: &str) -> Result<Program, Vec<String>>`

Parses Cobble source code into an AST using chumsky parser combinators.

**Parameters:**
- `source` - The source code string to parse

**Returns:**
- `Ok(Program)` - Successfully parsed program
- `Err(Vec<String>)` - Parse errors with messages

**Example:**
```rust
use cobble::parser::parse;

let source = r#"
def hello():
    /say Hello, world!
"#;

let program = parse(source)
    .map_err(|errors| errors.join(", "))?;
```

**Implementation Details:**
- Uses chumsky 0.11 parser combinator library
- Supports all Cobble syntax including execute blocks, global keyword, and complex expressions
- **Operator precedence**: Four-level precedence for expressions (pow > mul/div/mod > add/sub > comparisons)
- **Multi-operator expressions**: Chains operations using `.foldl().repeated()` pattern
- Error reporting integrated with ariadne for beautiful error messages

**Recent Enhancements (v0.1.0)**:
- Fixed division operator tokenization to correctly distinguish `a / b` from `/command`
- Implemented proper operator precedence (multiplication and division before addition and subtraction)
- Added support for chained multi-operator expressions like `a + b + c` and `x * y * z`

## Module: AST

### `ast.rs`

Defines the Abstract Syntax Tree structures.

#### Struct: `Program`

Represents a complete Cobble program.

```rust
pub struct Program {
    pub imports: Vec<Import>,
    pub statements: Vec<Statement>,
}
```

#### Enum: `Statement`

Represents different types of statements.

```rust
pub enum Statement {
    Import(Import),
    FunctionDef(FunctionDef),
    Assignment(Assignment),
    ConstAssignment(ConstAssignment),  // v0.3.0
    Expression(Expression),
    If(IfStatement),
    For(ForLoop),
    While(WhileLoop),
    Match(MatchStatement),  // v0.3.0
    Return(Option<Expression>),
    Pass,
    MinecraftCommand(String),
    Global(Vec<String>),
    Execute(ExecuteBlock),
    SelectorDef(SelectorDef),  // v0.4.0
}
```

**Recent Additions:**
- **v0.4.0**: `SelectorDef` - Entity selector definitions (e.g., `@Player = @a[type=player]`)
- **v0.3.0**: `ConstAssignment` - Compile-time constants (e.g., `const MAX_HEALTH = 100`)
- **v0.3.0**: `Match` - Match/switch statements with literal, range, and wildcard patterns

#### Struct: `FunctionDef`

Represents a function definition.

```rust
pub struct FunctionDef {
    pub name: String,
    pub params: Vec<Parameter>,
    pub body: Vec<Statement>,
    pub decorators: Vec<String>,
}
```

#### Struct: `Parameter`

Represents a function parameter.

```rust
pub struct Parameter {
    pub name: String,
    pub default: Option<Expression>,
}
```

#### Struct: `ConstAssignment` (v0.3.0)

Represents a compile-time constant assignment.

```rust
pub struct ConstAssignment {
    pub target: String,
    pub value: Expression,
}
```

Constants are evaluated at compile time and replaced with their literal values.

#### Struct: `MatchStatement` (v0.3.0)

Represents a match/switch statement.

```rust
pub struct MatchStatement {
    pub value: Expression,  // Expression to match against
    pub cases: Vec<MatchCase>,
}

pub struct MatchCase {
    pub pattern: MatchPattern,
    pub body: Vec<Statement>,
}

pub enum MatchPattern {
    Literal(i32),        // case 5:
    Range(i32, i32),     // case 1 to 10:
    Wildcard,            // case _:
}
```

Match statements compile to efficient 4-way split algorithm for optimal branching.

#### Struct: `SelectorDef` (v0.4.0)

Represents an entity selector definition.

```rust
pub struct SelectorDef {
    pub name: String,      // e.g., "Player"
    pub selector: String,  // e.g., "@a[type=player,gamemode=survival]"
}
```

Selector aliases are replaced at compile time with zero runtime overhead.

#### Enum: `Expression`

Represents different types of expressions.

```rust
pub enum Expression {
    Number(f64),
    String(String),
    Boolean(bool),
    None,
    Identifier(String),
    Binary(Box<Expression>, BinaryOp, Box<Expression>),
    Unary(UnaryOp, Box<Expression>),
    Call(Box<Expression>, Vec<Expression>),
    Attribute(Box<Expression>, String),
    Subscript(Box<Expression>, Box<Expression>),
}
```

#### Enum: `BinaryOp`

Binary operators.

```rust
pub enum BinaryOp {
    Add, Sub, Mul, Div, Mod, Pow,
    Eq, NotEq, Lt, LtEq, Gt, GtEq,
    And, Or,
}
```

## Module: Transpiler

The transpiler module is organized into several files:

### `transpiler/mod.rs`

Main transpiler that converts AST into Minecraft data pack format.

#### Struct: `Transpiler`

The main transpiler instance.

```rust
pub struct Transpiler {
    data_pack: DataPack,
    current_function: Option<Vec<String>>,
    current_context: FunctionContext,
    variables: HashMap<String, Expression>,
    module_level_vars: HashMap<String, Expression>,  // Top-level variables to initialize
    constants: HashMap<String, i32>,                  // v0.3.0: Compile-time constants
    selector_aliases: HashMap<String, String>,        // v0.4.0: Entity selector definitions
    imported_files: HashSet<PathBuf>,                 // v0.4.0: Imported file tracking
    current_file_dir: PathBuf,                        // v0.4.0: Current file directory for imports
    temp_counter: u32,
    variable_objectives: HashMap<String, String>,  // Tracks which objective each variable uses
    function_params: HashMap<String, Vec<String>>,  // Tracks parameter names for each function
}
```

### `transpiler/command_processor.rs`

Processes Minecraft command strings with variable substitution.

**Key Features:**
- Substitutes function parameters (macro variables)
- Converts scoreboard variables in JSON text components
- Handles selector alias replacement
- Escapes strings properly for Minecraft commands

### `transpiler/expression_evaluator.rs`

Evaluates complex expressions into scoreboard operations.

**Key Features:**
- Converts arithmetic expressions to scoreboard operations
- Handles power operator with compile-time expansion
- Generates temporary variables for intermediate results
- Optimizes self-assignment operations

### `transpiler/condition_translator.rs`

Translates Python-style conditions to Minecraft execute conditions.

**Key Features:**
- Converts comparison operators to scoreboard matches
- Handles boolean AND/OR/NOT operators
- Supports literal comparisons on both sides
- Generates OR logic using temporary variables

### `transpiler/data_pack.rs`

Manages data pack structure and file generation.

**Key Features:**
- Tracks objectives and functions
- Generates pack.mcmeta with correct format
- Creates function tag files (load/tick)
- Handles standard library event listeners

### `transpiler/statement_processors/`

Individual processors for each statement type:
- `assignment.rs` - Variable assignments with type checking
- `if_processor.rs` - If/elif/else statements with function extraction
- `loop_processor.rs` - For and while loops
- `match_processor.rs` - Match/switch statements with 4-way split
- `execute_processor.rs` - Execute block modifiers
- `selector_processor.rs` - Selector definitions

**Recent Enhancements:**
- **v0.4.0**:
  - **Entity selector definitions**: Custom selector aliases with compile-time replacement
  - **File import system**: Import functions and definitions from other `.cbl` files
  - **Circular dependency prevention**: Automatic detection via `imported_files` HashSet
  - **Relative import resolution**: Resolve imports relative to current file location
- **v0.3.0**:
  - **Compile-time constants**: `const` keyword for constant declarations
  - **Match/switch statements**: Efficient multi-way branching with 4-way split algorithm
- **v0.1.0**:
  - **Module-level variable initialization**: Top-level assignments are automatically initialized in `_cobble_init`
  - **Complex expression evaluation**: New `evaluate_expression_to_target()` helper handles nested binary expressions
  - **Correct loop variable objectives**: Loop variables (like `i` in for loops) now correctly track their objective
  - **Duplicate objective prevention**: `ensure_init_function()` checks for existing objectives before adding

#### Implementation: `Transpiler`

##### `new(namespace: String, output_dir: PathBuf) -> Self`

Creates a new transpiler instance.

**Parameters:**
- `namespace` - The data pack namespace
- `output_dir` - Output directory path

##### `transpile(&mut self, program: &Program) -> Result<(), String>`

Transpiles a program into a data pack.

**Parameters:**
- `program` - The parsed program AST

**Returns:**
- `Ok(())` - Successfully transpiled
- `Err(String)` - Transpilation error

##### `write_data_pack(&self) -> std::io::Result<()>`

Writes the data pack to disk.

**Example:**
```rust
use cobble::transpiler::Transpiler;
use cobble::parser::parse;
use std::path::PathBuf;

let source = "...";
let program = parse(source)
    .map_err(|errors| errors.join(", "))?;

let mut transpiler = Transpiler::new(
    "my_namespace".to_string(),
    PathBuf::from("./output")
);

transpiler.transpile(&program)?;
transpiler.write_data_pack()?;
```

### Struct: `DataPack`

Represents a Minecraft data pack.

```rust
pub struct DataPack {
    pub namespace: String,
    pub description: String,
    pub output_dir: PathBuf,
    pub functions: HashMap<String, Vec<String>>,
    pub tags: HashMap<String, Vec<String>>,
    pub advancements: HashMap<String, String>,
    pub loot_tables: HashMap<String, String>,
    pub recipes: HashMap<String, String>,
    pub predicates: HashMap<String, String>,
    pub item_modifiers: HashMap<String, String>,
    pub pack_format: PackFormat,  // Supports both integer (18) and decimal (88.0) formats
    pub stdlib: StdLib,
    pub used_objectives: HashSet<String>,
}
```

**Note**: `pack_format` uses the `PackFormat` enum which supports both integer formats (e.g., 18, 48, 88) and decimal formats (e.g., 88.0, 88.1) introduced in Minecraft 1.21.9+. The decimal format is serialized as a JSON number (float), not a string.

#### Methods

##### `new(namespace: String, output_dir: PathBuf) -> Self`

Creates a new data pack.

##### `add_function(&mut self, name: String, commands: Vec<String>)`

Adds a function to the data pack.

##### `write(&self) -> std::io::Result<()>`

Writes the data pack to the file system.

## Module: Standard Library

### `stdlib.rs`

Provides the Cobble standard library implementation.

#### Struct: `StdLib`

```rust
pub struct StdLib {
    event_listeners: HashMap<EventType, Vec<String>>,
}
```

#### Enum: `EventType`

```rust
pub enum EventType {
    Load,
    Tick,
    Custom(String),
}
```

#### Methods

##### `new() -> Self`

Creates a new standard library instance.

##### `add_event_listener(&mut self, event_type: EventType, function_name: String)`

Registers a function as an event listener.

**Parameters:**
- `event_type` - The type of event to listen for
- `function_name` - The name of the function to call

##### `generate_tags(&self, namespace: &str) -> HashMap<String, Vec<String>>`

Generates Minecraft function tags for registered events.

**Returns:**
- HashMap of tag names to function lists

## Module: Configuration

### `config.rs`

Handles project configuration.

#### Struct: `Config`

```rust
pub struct Config {
    pub project: ProjectConfig,
    pub build: BuildConfig,
}

pub struct ProjectConfig {
    pub name: String,
    pub description: String,
    pub version: String,
    pub pack_format: u8,
}

pub struct BuildConfig {
    pub output: PathBuf,
    pub namespace: String,
}
```

#### Functions

##### `load_config(path: &Path) -> Result<Config, ConfigError>`

Loads configuration from a `cobble.toml` file.

##### `create_default_config(name: String) -> Config`

Creates a default configuration.

## Module: Commands

### `commands/build.rs`

Handles the `build` command.

#### Function: `build_command(args: BuildArgs) -> Result<(), String>`

Executes the build command.

**Parameters:**
- `args` - Build command arguments

**Returns:**
- `Ok(())` - Build succeeded
- `Err(String)` - Build error

### `commands/init.rs`

Handles the `init` command.

#### Function: `init_command(args: InitArgs) -> Result<(), String>`

Initializes a new Cobble project.

### `commands/check.rs`

Handles the `check` command.

#### Function: `check_command(args: CheckArgs) -> Result<(), String>`

Checks source files for syntax errors.

### `commands/watch.rs`

Handles the `watch` command.

#### Function: `watch_command(args: WatchArgs) -> Result<(), String>`

Watches files for changes and rebuilds automatically.

## Error Handling

### Module: `error.rs`

#### Enum: `CompileError`

```rust
pub enum CompileError {
    ParseError(String),
    TranspileError(String),
    IoError(std::io::Error),
    ConfigError(String),
}
```

All compiler errors are wrapped in this enum for consistent handling.

## Usage Examples

### Basic Compilation

```rust
use cobble::parser::parse;
use cobble::transpiler::Transpiler;
use std::path::PathBuf;

fn compile(source: &str, output_dir: &str) -> Result<(), String> {
    // Parse
    let program = parse(source)
        .map_err(|errors| format!("Parse errors: {}", errors.join(", ")))?;

    // Transpile
    let mut transpiler = Transpiler::new(
        "my_pack".to_string(),
        PathBuf::from(output_dir)
    );

    transpiler.transpile(&program)
        .map_err(|e| format!("Transpile error: {}", e))?;

    // Write
    transpiler.write_data_pack()
        .map_err(|e| format!("IO error: {}", e))?;

    Ok(())
}
```

### Custom Data Pack Options

```rust
use cobble::transpiler::Transpiler;

let mut transpiler = Transpiler::new(
    "custom_pack".to_string(),
    PathBuf::from("./output")
);

// Customize pack settings
transpiler.set_description("My Custom Data Pack".to_string());
transpiler.set_pack_format(88);  // Minecraft 1.21.9+ (default)

transpiler.transpile(&program)?;
transpiler.write_data_pack()?;
```

### AST Traversal

```rust
use cobble::ast::{Statement, Expression};

fn count_functions(program: &Program) -> usize {
    program.statements.iter().filter(|stmt| {
        matches!(stmt, Statement::FunctionDef(_))
    }).count()
}

fn find_minecraft_commands(statements: &[Statement]) -> Vec<String> {
    statements.iter().filter_map(|stmt| {
        match stmt {
            Statement::MinecraftCommand(cmd) => Some(cmd.clone()),
            _ => None
        }
    }).collect()
}
```

## Internal Architecture

### Compilation Pipeline

```
Source Code (.cbl)
    ↓
[Parser] - Lexical Analysis & Parsing
    ↓
Abstract Syntax Tree (AST)
    ↓
[Transpiler] - Code Generation
    ↓
Minecraft Commands
    ↓
[DataPack Writer] - File Generation
    ↓
Data Pack (.mcfunction files, pack.mcmeta, tags)
```

### Key Design Decisions

1. **Python-style Syntax**: Familiar to many programmers, clean and readable
2. **chumsky Parser Combinators**: Modern, composable parsing with excellent error handling
3. **Operator Precedence**: Four-level precedence system matches standard mathematical conventions (pow > mul/div/mod > add/sub > comparisons)
4. **Complex Expression Handling**: Recursive evaluation for nested binary expressions with temporary variables
5. **Macro-based Parameters**: Uses Minecraft 1.20.2+ macro system for function parameters
6. **Automatic Recursion**: For loops and while loops compile to recursive functions
7. **Separate Functions for Complex Control Flow**: Nested if statements become separate functions to avoid command limit issues
8. **Scoreboard Variables**: All variables are stored as scoreboard objectives with proper tracking
9. **Module-level Initialization**: Top-level variables automatically initialized at pack load
10. **ariadne Error Reporting**: Beautiful, user-friendly error messages with context

## Performance Considerations

### Compilation Speed

- Parsing is O(n) where n is source code length
- Transpilation is O(m) where m is AST node count
- File writing is O(f) where f is function count

### Generated Code Size

- Simple functions: 1-10 commands
- For loops: 2-3 functions (init + loop function)
- While loops: 1-2 functions
- Nested control flow: Additional helper functions as needed

## Contributing

When adding new features to the API:

1. Update the AST structures in `ast.rs`
2. Add parser support in `parser.rs` using chumsky combinators
3. Implement transpilation in `transpiler.rs`
4. Add tests in `tests/`
5. Update this documentation

### Parser Development

When extending the parser:
- Tokenizer is in `parser.rs::tokenize()` - handles lexical analysis and indentation
- Parser combinators are in `parser.rs::token_parser()` - handles syntax analysis
- Use chumsky's `recursive()` for nested structures
- Test with both unit tests in `parser.rs` and integration tests in `tests/`

## Further Reading

- [Language Reference](language.md) - Cobble language syntax
- [CLI Documentation](cli.md) - Command-line usage
- [Examples](../examples/) - Example code
- [Rust Documentation](https://doc.rust-lang.org/book/) - Rust programming language
