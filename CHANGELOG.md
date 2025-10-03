# Changelog

All notable changes to Cobble will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.5.1] - 2025-10-03

### Fixed
- **Power operator associativity**: Fixed power operator to be right-associative following mathematical conventions
  - `2^3^2` now correctly evaluates as `2^(3^2) = 512` instead of `(2^3)^2 = 64`
  - Matches standard mathematical notation where exponentiation is right-associative
  - Parser updated to build right-associative expression trees for power operations
- **Constant expression folding**: Implemented recursive constant evaluation for nested expressions
  - Complex constant expressions like `2^3^2` are now evaluated at compile time
  - Added `try_eval_const()` method that recursively evaluates arithmetic expressions
  - Reduces generated commands and improves runtime performance
- **Compiler warnings**: Removed unused variable warnings in type inference code
  - Prefixed unused variables with underscore to silence warnings
  - Zero compiler warnings in clean build

### Added
- **Example files**: Added 9 comprehensive example files to `examples/` directory
  - `hello_world.cbl` - Basic syntax and event handlers
  - `counter.cbl` - Tick counter with module-level variables
  - `loops.cbl` - For loops and while loops with various patterns
  - `conditionals.cbl` - If/elif/else and match statements
  - `functions.cbl` - Function parameters and macro system
  - `execute_blocks.cbl` - Execute block modifiers and conditions
  - `selectors.cbl` - Custom selector definitions
  - `arithmetic.cbl` - Arithmetic operations and operator precedence
  - `type_system.cbl` - Static type system examples
  - `README.md` - Examples guide with learning path
- **Power operator error messages**: Improved error messages for unsupported power operations
  - Clear guidance when attempting to use variables or complex expressions as exponents
  - Suggests using constant exponents for supported operations

### Technical Details
- Modified `src/parser/combinators.rs` to build right-associative power expression trees
- Added `try_eval_const()` recursive evaluator in `src/transpiler/mod.rs`
- Updated `process_assignment()` to use constant folding before expression evaluation
- Added test `test_power_operator_simple` to verify correct power operation compilation
- All 66 tests passing (7 parser + 59 integration tests)

### Documentation
- Added 9 example files demonstrating all major language features
- Examples provide hands-on learning materials for new users
- README.md in examples directory provides recommended learning path

## [0.5.0] - 2025-10-03

### Added
- **Type System**: Implemented static, immutable type system with compile-time type checking
  - All variables have immutable types that are inferred from their first assignment
  - Type mismatches are caught at compile time with clear error messages
  - Prevents accidental type changes (e.g., overwriting a score with a boolean)
  - Types supported: Integer, Boolean, String (in function parameters only)
  - Type inference works for expressions: arithmetic operations return Integer, comparisons/logical ops return Boolean
- **Numeric Range Warnings**: Added compile-time warnings for numeric precision issues
  - Float precision warning: Alerts when float values lose precision (e.g., `3.14159` → `3`)
  - Overflow warning: Alerts when values exceed Minecraft scoreboard range (-2,147,483,648 to 2,147,483,647)
  - Values are automatically clamped to valid range with clear warning messages

### Fixed
- **CRITICAL: Match range overlap validation**: Match statements now validate that case ranges don't overlap
  - Prevents bugs where multiple cases would execute for the same value
  - Compile-time error with clear message showing which cases overlap
  - Example: `case 1 to 5:` and `case 3 to 7:` now produces an error
  - Follows CBScript's validation approach for consistency
- **Boolean module-level initialization**: Fixed bug where boolean variables at module level weren't initialized
  - `active = True` now correctly generates `scoreboard players set active temp 1` in `_cobble_init`
  - `disabled = False` now correctly generates `scoreboard players set disabled temp 0`
  - Previously, boolean module-level variables would have undefined values

### Documentation
- Added comprehensive Type System section to `docs/language.md`
  - Explains type inference, immutable types, and type safety benefits
  - Documents numeric ranges and precision limitations
  - Includes examples of type errors and warnings
- Added Match Validation section documenting overlap detection
  - Shows error messages and correct usage patterns
  - Explains that only the first matching case executes
- Updated Table of Contents with new Type System section

### Technical Details
- Added `CobbleType` enum to `src/ast.rs` with Integer, Boolean, String, and Unknown variants
- Added `variable_types: HashMap<String, CobbleType>` to Transpiler struct
- Implemented `infer_type()` method for expression type inference
- Implemented `check_type_assignment()` method for type validation
- Added type tracking to `process_assignment()` and `process_const_assignment()`
- Added overlap validation to match processor with sorted range checking
- Added warning output for float truncation and range overflow in assignment processor
- Fixed boolean initialization in module-level variable processing (line 211-217 of transpiler/mod.rs)

### Breaking Changes
- Variables can no longer change types after initial assignment
  - Code like `x = 5; x = True` will now produce a compile error
  - This is a safety feature that catches bugs at compile time
- Match statements with overlapping ranges will now fail compilation
  - Previously would silently generate incorrect behavior
  - Update match cases to use mutually exclusive ranges

## [0.4.3] - 2025-10-03

### Fixed
- **CRITICAL: Execute block keyword capitalization bug**: Fixed bug where Minecraft keywords in execute blocks were incorrectly capitalized
  - Multiple `if`/`unless` conditions in execute blocks now generate lowercase keywords
  - Before: `execute as @a if entity @s[tag=a] If entity @s[tag=b]` (invalid - crashes in Minecraft)
  - After: `execute as @a if entity @s[tag=a] if entity @s[tag=b]` (valid ✅)
  - Root cause: `Token::Display` trait was using debug format (`{:?}`) which outputs enum names with capital letters
  - Fixed by adding explicit lowercase mappings for all keyword tokens
  - This bug affected ANY execute block with multiple conditions - generated commands would fail silently in Minecraft

### Added
- **Regression tests**: Added 3 new integration tests to prevent future capitalization bugs (58 total, all passing ✅)
  - `test_multiple_if_in_execute_block` - Validates multiple `if` conditions are lowercase
  - `test_if_unless_combination_in_execute` - Validates `if` + `unless` combinations
  - `test_complex_execute_chain` - Validates complex execute chains with all keywords lowercase
- **Documentation**: Added comprehensive warning about division/modulo by zero in `docs/language.md`
  - Explains compile-time vs runtime checking
  - Provides best practices for safe division
  - Notes that runtime behavior is undefined in Minecraft

### Technical Details
- Modified `src/parser/tokenizer.rs`: Added explicit lowercase Display implementations for all keyword tokens
- Added pattern matching for: `if`, `unless`, `as`, `at`, `and`, `or`, `not`, `in`, `for`, `while`, `elif`, `else`, `def`, `return`, `pass`, `global`, `import`, `from`, `asat`, `match`, `case`, `const`, `to`, `by`
- This ensures all keywords are always lowercase when converted to strings for command generation

## [0.4.2] - 2025-10-03

### Fixed
- **CRITICAL: Nested OR operator bug**: Fixed bug where multiple OR expressions or OR combined with AND would generate invalid `OR(...)` syntax
  - `a or b or c` now correctly expands to multiple scoreboard checks
  - `(a or b) and c` now properly combines OR and AND operators
  - All OR expressions are recursively processed and expanded to valid Minecraft commands
- **CRITICAL: Match wildcard single-statement bug**: Fixed bug where single-statement wildcard cases would execute unconditionally
  - Wildcard cases now properly check all previous ranges with chained `unless` conditions
  - Example: `case _: /say Other` now generates `execute unless ... unless ... run say Other`
- **CRITICAL: Match wildcard multi-statement bug**: Fixed bug where multi-statement wildcard functions would be called multiple times
  - Wildcard functions now called exactly once with all `unless` conditions chained in single execute command
  - Prevents duplicate execution of wildcard code

### Added
- **Comprehensive tests**: Added 4 new integration tests (55 total, all passing ✅)
  - `test_nested_or_operators` - Validates triple OR expressions
  - `test_or_with_and_combination` - Validates OR combined with AND
  - `test_match_wildcard_single_statement` - Validates single-statement wildcard cases
  - `test_match_wildcard_multi_statement` - Validates multi-statement wildcard cases

### Improved
- **OR operator**: Now fully supports nested and combined expressions
- **Match statements**: Wildcard cases now work correctly in all scenarios
- **Test coverage**: Increased from 51 to 55 integration tests

## [0.4.1] - 2025-10-03

### Added
- **Loop variable macro support**: Loop variables can now be used directly in Minecraft commands
  - For loop bodies are compiled as macro functions with the loop variable as a parameter
  - Syntax: `/say Count: {i}` in loop body compiles to `$say Count: $(i)` in macro function
  - Loop variable values are passed via storage to macro functions
  - Works in all command contexts: coordinates (`~{i} ~ ~`), JSON text components, command arguments
  - Example:
    ```python
    for i in range(5):
        /say Countdown: {i}
        /title @a title {"text":"{i}", "color":"red"}
    ```
- **Comprehensive tests**: Added 3 new integration tests (51 total, all passing ✅)
  - `test_loop_variable_in_commands`
  - `test_loop_variable_with_step`
  - `test_parameterless_function_call`

### Fixed
- **Function call bug**: Fixed parameterless functions being incorrectly called with `with storage` syntax
  - Functions without parameters now use simple `function namespace:name` syntax
  - Only functions with parameters use `function namespace:name with storage namespace:global args`
- **Parser bug**: Fixed `by` keyword not being recognized in for loop step syntax
  - Changed from string matching to proper token matching
  - For loops like `for i in range(10) by 2:` now parse correctly

### Improved
- **Loop processor**: Enhanced to create macro functions for loop bodies
- **Test coverage**: Updated 3 existing tests to verify new loop body structure
  - `test_for_loop`
  - `test_for_loop_with_arithmetic`
  - `test_for_loop_variable_in_tellraw`

## [0.4.0] - 2025-10-03

### Added
- **Entity Selector Definitions**: Define custom selector aliases for cleaner code
  - Syntax: `@Name = @a[type=player,gamemode=survival]`
  - Use aliases in execute blocks and commands
  - Compile-time replacement for zero runtime overhead
  - Example:
    ```python
    @Admin = @a[tag=admin]
    as @Admin:
        /say Hello admin
    ```
- **File Import System**: Import functions and definitions from other `.cbl` files
  - Syntax: `import filename` imports `filename.cbl`
  - Relative import resolution
  - Automatic circular dependency prevention
  - Imported functions and selectors are merged into current namespace
  - Example:
    ```python
    import utils
    helper_function()  # From utils.cbl
    ```
- **Comprehensive tests**: Added 3 new integration tests (48 total, all passing ✅)
  - `test_selector_definition`
  - `test_selector_in_commands`
  - `test_file_import`

### Improved
- **Tokenizer**: Enhanced `@` selector parsing to support multi-character names (e.g., `@Player`, `@Boss`)
- **Documentation**: Added comprehensive examples for selector definitions and file imports
- **Command processor**: Selector alias replacement in all contexts
- **Execute processor**: Selector alias replacement in execute blocks

## [0.3.0] - 2025-10-03

### Added
- **Compile-time constants**: Use `const NAME = value` to declare constants that are evaluated at compile time
  - Constants can be used in expressions and assignments
  - Example: `const MAX_HEALTH = 100`
- **Match/switch statements**: Efficient multi-way branching based on integer values
  - Literal matching: `case 5:` - matches exactly 5
  - Range matching: `case 1 to 10:` - matches values from 1 to 10 (inclusive)
  - Wildcard pattern: `case _:` - matches anything not matched by previous cases
  - Uses 4-way split algorithm for efficient branching
  - Example:
    ```python
    match score:
        case 0 to 59:
            /say Fail
        case 60 to 100:
            /say Pass
        case _:
            /say Invalid
    ```
- **Comprehensive tests**: Added 6 new integration tests for const and match features
  - `test_const_variable`
  - `test_const_declaration`
  - `test_match_literal`
  - `test_match_range`
  - `test_match_wildcard`
  - `test_match_with_multiple_statements`

### Improved
- Documentation: Added comprehensive examples for const and match features in `docs/language.md`
- Test coverage: Now 45 integration tests (previously 39)

## [0.2.2] - 2025-10-02

### Fixed
- **CRITICAL: If statement inlining bug**: Fixed bug where if/elif/else statements with 2-3 statements that modify condition variables would not execute all statements correctly
  - Changed threshold from `> 3` to `> 1` to force function extraction for multi-statement blocks
  - Prevents incorrect behavior when earlier statements modify variables used in conditions
  - Example fixed: `if x >= 20: x = 0; /say Done` now correctly executes both statements
- **CRITICAL: While loop condition bug**: Fixed bug where while loop body statements were individually wrapped in conditions
  - Body now executes unconditionally in a separate function, with condition checked only once per iteration
  - Prevents incorrect behavior when body modifies loop condition variables
  - Example fixed: `while i < 3: i = i + 1; /say Loop` now correctly runs 3 times
- **Module variable initialization order**: Fixed incorrect command order in init functions
  - Now properly orders: gamerule → objectives → variable initialization
  - Previously variables were initialized before objectives were created

### Added
- **Automatic gamerule configuration**: `maxCommandChainLength` is now automatically set to 1000000000 in init functions
  - Prevents command chain limit errors in complex recursive loops
  - Added to all load event handlers automatically
- **Comprehensive regression tests**: Added 5 new tests for bug scenarios
  - `test_if_modifies_condition_variable`
  - `test_elif_modifies_condition_variable`
  - `test_else_modifies_condition_variable`
  - `test_while_modifies_condition_variable`
  - `test_tick_counter_example` (validates README example)

### Documentation
- **Power operator associativity**: Documented left-associative behavior of `^` operator in `docs/language.md`
  - `2^3^2` evaluates as `(2^3)^2 = 64`, not mathematical convention `2^(3^2) = 512`
  - Added clear examples and warnings

### Improved
- Test count increased to 46 tests (7 parser + 39 integration)
- All tests now pass with bug fixes

## [0.2.1] - 2025-10-02

### Added
- **Complex expressions in conditions**: Arithmetic expressions can now be used directly in `if`/`while` conditions
  - Example: `if x % 3 == 1:` (no temporary variable needed)
  - Example: `if y ^ 2 == 16:`
  - Works with nested conditions: `if x % 3 == 1 and y ^ 2 == 25:`
  - Automatically creates unique temporary variables (`expr_cond_temp_N`) for evaluation
  - Supports complex expressions on both left and right sides of comparisons
  - Works seamlessly with AND/OR operators

### Improved
- Intelligent condition preprocessing that recursively handles nested expressions
- Each complex expression in AND/OR chains gets a unique temporary variable to prevent conflicts

## [0.2.0] - 2025-10-02

### Added
- **Modulo operator (`%`)**: Compute remainders with `x % y`
  - Works with constants and variables
  - Uses temporary `modulus` scoreboard objective
  - Compile-time evaluation for constant expressions
- **Power operator (`^`)**: Exponentiation with `x ^ n`
  - Compile-time expansion: `x^3` becomes `x*x*x`
  - Exponent must be a constant (variables not supported)
  - Uses temporary `power_base` scoreboard objective
- **OR operator (`or`)**: Boolean OR for conditions
  - Syntax: `if x == 5 or y == 10:`
  - Uses temporary `or_result` scoreboard to track results
  - Works in `if` and `while` statements
- **For loop step support**: Control loop increment/decrement
  - Syntax: `for i in range(10) by 2:` or `for i in range(10) by -1:`
  - Positive step: starts at 0, increments, continues while `i < n`
  - Negative step: starts at `n-1`, decrements, continues while `i >= 0`
  - Default step is 1 if not specified

### Fixed
- **Java Edition compatibility**: Negative loop steps now use `scoreboard players remove` instead of `add` with negative values

### Improved
- Updated operator precedence: `^` > `*/%` > `+-` > comparisons
- Enhanced assignment processor to handle all new operators
- Comprehensive documentation updates for all new features

## [0.1.2] - 2025-10-02

### Fixed
- Fixed example file `examples/basics.cbl` to remove unsupported module-level string variable
- Updated documentation in `language.md` to remove outdated parentheses limitation
- Added validation for pack_format values to prevent overflow (must be between 1 and 255)

### Improved
- Better error messages for invalid pack_format values

## [0.1.1] - 2025-10-02

### Added
- Parenthesized expressions support (e.g., `result = (a + b) * c`)

### Fixed
- Self-assignment optimization: Removed unnecessary `x = x` operations in statements like `x = x + 1`
- Removed unused variable warning in `check.rs`
- Documentation accuracy: Updated test count from 36 to 41 tests
- Documentation clarity: Corrected boolean operator support status (and/not supported, or planned for v0.2.0)

### Improved
- Performance: Self-assignment operations now generate more efficient commands
- Code quality: Cleaner code with fewer compiler warnings
