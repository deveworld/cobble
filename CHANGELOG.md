# Changelog

All notable changes to Cobble will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
