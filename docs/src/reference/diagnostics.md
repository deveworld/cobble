# Diagnostics

`cobble check`, `cobble build`, and the browser compiler run the same
source-aware checks before compiling. They report:

- unsupported Python syntax;
- a missing `:` after a block header, unexpected or inconsistent indentation;
- unclosed delimiters and strings;
- duplicate functions and parameters, including across imported files or a
  compiled directory;
- expressions that produce no command;
- missing and circular imports, and diagnostics inside imported files;
- non-`stdlib` imports in the single-file browser compiler;
- undefined variables, functions, and stdlib modules;
- undefined, invalid, or unclosed `{name}` placeholders in raw commands;
- type mismatches;
- malformed `datapack.*` arguments, resource IDs, and tag entries.

Compilation then adds its own errors, such as loop expansion limits, JSON
resource formats, and variable placeholders in commands that cannot show them.
They point at the statement that caused them.

## Output Format

Each diagnostic names its kind, points at the source, and adds a hint when
there is one:

```text
Error: missing-block-colon: Block headers must end with `:`
   ╭─[ main.cbl:1:8 ]
   │
 1 │ def f()
   │        ┬
   │        ╰── Block headers must end with `:`
   │
   │ Note: Add `:` at the end of the line before the indented block.
───╯
```

`cobble check --json` writes the same diagnostics to stdout as JSON with
`file`, `line`, `column`, `severity`, `kind`, `message`, `help`, and a
`formatted` text version. The browser compiler shows the same fields.
A diagnostic in an imported file is reported in that file.

## Warnings

A warning reports code that compiles but may not do what it says. Warnings
don't stop `cobble build` or fail `cobble check`. `cobble build` prints them,
`cobble check` reports them with severity `warning`, and the browser compiler
lists them under Diagnostics.

| Kind | Reported when |
| --- | --- |
| `local-shadows-module-variable` | a function assigns the name of a module variable without `global`, so it changes a local variable (see [Calling Convention](calling-convention.md#local-and-module-variables)) |
| `unroll-large-expansion` | a `for` loop expands to more than 256 copies of its body |
| `infinite-loop` | a `while True` loop has no `return` in its body |
| `fractional-number` | a number with a fractional part is stored in a score, which keeps only the integer part |
| `number-out-of-range` | a number outside the 32-bit range is stored in a score, which clamps it |
| `selector-redefined` | a selector alias is defined again, which replaces the earlier definition |

## Kinds Listed Elsewhere

- Loop kinds such as `unroll-limit-exceeded`: [For Loops](../language/control-flow.md#for-loops).
- `missing-stdlib-module`: [Standard Library Modules](../stdlib/modules.md).
- `datapack-resource-format`: [Data Pack Resources](../stdlib/datapack-resources.md).
- `resource-pack-*` kinds: [Resource Packs](../stdlib/resource-packs.md#diagnostics).
- `temp-objective-reference`, `say-variable-text`, `load-function-tag`,
  `entity-template-tag`, and `item-model-definition` in `cobble migrate`:
  [Upgrading](../upgrading.md#010-to-011).
