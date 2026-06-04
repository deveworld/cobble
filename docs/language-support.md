# Cobble Language Support Matrix

This document records the language surface that Cobble 0.7.0 should treat as
intentional. It is a planning and QA companion to `docs/language.md`; the
language reference should explain user-facing behavior, while this matrix keeps
implementation coverage explicit.

Cobble is Python-inspired, not Python-compatible. Syntax listed as unsupported
should fail with an actionable diagnostic.
The 0.7.0 implementation already runs a shared CLI/WASM preflight diagnostic
pass for the unsupported constructs listed below when they can be detected from
source text before parsing.

## Supported Syntax

| Area | Supported surface | Coverage target |
| --- | --- | --- |
| Comments | Full-line comments and raw-command inline comments | Parser/tokenizer tests and command output tests |
| Docstrings | Single-line and multi-line triple-quoted docstrings in blocks | Parser/transpiler tests and semantic diagnostic tests |
| Blocks | Indentation-based blocks after `:` | Parser tests and integration tests |
| Literals | Numbers, strings, `True`, `False`, arrays, maps; `None` as JSON `null` only in JSON resource helper contexts | Parser tests and transpiler tests |
| Expressions | Arithmetic, comparisons, `and`, `or`, `not`, unary `+`/`-`, power, modulo | Parser tests and integration tests |
| Access | Attribute and subscript expressions for storage-backed values, including literal/constant subscript paths | Parser tests and semantic/transpiler tests |
| Variables | Assignment, `const`, `global`, static type inference | Integration tests |
| Functions | `def name(params):` with identifier-only parameters | Parser tests and integration tests |
| Raw commands | Lines beginning with `/` | Source-map and validation tests |
| Control flow | `if`/`elif`/`else`, `while`, `for ... in ... by ...`, `match`/`case` | Parser tests, integration tests, snapshots |
| Execute blocks | `as`, `at`, `asat`, `if`, `unless`, `positioned`, `rotated`, `in`, `anchored`, `align`, `store` | Parser and integration tests |
| Imports | `import module`, `from module import item` | Parser, CLI, and import diagnostics tests |
| Selectors | `@Name = @selector[...]` aliases | Integration tests |
| Entity templates | `define @Name = @Selector create {...} end`, `create @Name` | Parser and integration tests |
| Data pack resources | `datapack.*` helper calls | Resource tests and snapshots |
| Stdlib calls | `stdlib.*` and imported stdlib helper calls | Stdlib tests |

## Intentionally Unsupported Python Syntax

| Syntax | Desired 0.7 behavior | Alternative |
| --- | --- | --- |
| Default parameters, `*args`, `**kwargs` | Reject with function-parameter diagnostic | Use explicit parameters and call sites |
| Duplicate function parameter names | Reject with duplicate-parameter diagnostic | Rename one parameter |
| Decorators | Reject with unsupported syntax diagnostic | Use stdlib event registration |
| Compound assignment such as `+=` | Reject with assignment diagnostic | Use `x = x + value` |
| Augmented loops such as `for ... else` | Reject with unsupported control-flow diagnostic | Use explicit flags and `if` |
| List/dict comprehensions | Reject with unsupported expression diagnostic | Use explicit loops or storage helpers |
| `class`, `try`, `except`, `finally`, `with`, `lambda`, `yield`, `async`, `await`, `break`, `continue`, `raise`, `assert`, `del`, `nonlocal` | Reject with unsupported Python feature diagnostic | Keep Cobble source explicit |
| Relative or dotted imports | Reject with module-resolution diagnostic | Use simple module names such as `helpers` |
| Import aliases, wildcard imports, and comma-separated `import a, b` | Reject with import diagnostic | Use explicit names and one `import module` per line |
| Runtime return values | Reject with function-return diagnostic | Use scoreboard/storage output variables |
| Arbitrary assignment targets | Reject with assignment-target diagnostic | Assign to simple identifiers |
| Standalone value expressions | Reject with no-op diagnostic | Assign the value, call a helper, use a raw command, or write `pass` |
| Missing `:` after block headers | Reject with missing-block-colon diagnostic | Add `:` before the indented block |
| Unexpected or inconsistent indentation | Reject with indentation diagnostic | Indent only after `:` and dedent to a previous block level |
| Unclosed delimiters or strings | Reject with structural syntax diagnostic | Close `()`, `[]`, `{}`, or string quotes before continuing |
| Unknown, invalid, unsupported imported-symbol, or unclosed raw command `{name}` placeholders | Reject with placeholder diagnostic | Use identifier names, define a value, pass a parameter, or escape literal braces as `{{name}}` |

## 0.7.0 Diagnostic Priorities

1. Preserve current successful generated output.
2. Reject unsupported Python-like syntax before transpilation when practical.
3. Include source file, line, and column in new diagnostics.
4. Include import-chain context when a diagnostic originates in an imported
   file.
5. Keep CLI diagnostics and WASM structured diagnostics aligned.

The first three priorities and CLI/WASM alignment for early language-surface
diagnostics are covered in 0.7.0. The first semantic preflight checks also
cover duplicate function definitions, unsupported `return` statements, and
function calls used as assignment values except supported math intrinsics.
They also check same-file user function call argument counts, including calls
that appear before the function definition, and reject nested function-call
expressions passed to same-file user functions. Undefined variables are reported
for variable-dependent expressions such as assignment RHS values, control-flow
conditions, and standalone helper/function call arguments. Standalone value
expressions that would otherwise be ignored are reported as no-op expressions.
Common structural syntax mistakes such as unexpected/inconsistent indentation,
unclosed delimiters, unmatched closing delimiters, and unterminated strings are
reported before parser fallback, with CRLF-aware source offsets. Raw command
placeholders are checked against defined Cobble variables, function parameters,
active loop variables, and imported value symbols backed by assignments, consts,
or globals. Imported functions, selector aliases, and entity templates are not
valid placeholder values. Invalid names such as `{bad-name}` are rejected while
preserving JSON/NBT compound braces. Later
semantic scans skip multi-line docstring bodies so documentation text does not
produce false unsupported-syntax, undefined-variable, or placeholder
diagnostics. Clearly inferred type changes are reported before transpilation.
`datapack.*` helper calls report non-object JSON resource values, invalid tag
value arrays, and non-string tag entries before transpilation. Literal datapack
resource names and tag values also report common ID mistakes such as
`minecraft/load`, uppercase paths, invalid namespaces, and invalid path
separators before transpilation. CLI import preflight now reports missing
imports, circular import chains, and imported-file language diagnostics before
transpilation. It
also reports missing `from module import item` symbols, rejects import-wide and
directory-wide function name collisions, selector alias collisions, and entity
template collisions, and catches calls to imported functions with the wrong
argument count. The browser compiler reports non-stdlib imports as
`missing-import` diagnostics because it compiles one in-memory file. Import
aliases, wildcard imports, and comma-separated module imports now fail with
source-aware diagnostics before parser fallback. Broader type checking for
ambiguous or runtime-dependent expressions remains planned work.
