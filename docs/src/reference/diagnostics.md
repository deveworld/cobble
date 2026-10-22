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

Compilation then adds its own errors, such as loop expansion limits and JSON
resource formats.

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

## Kinds Listed Elsewhere

- Loop kinds such as `unroll-limit-exceeded`: [For Loops](../language/control-flow.md#for-loops).
- `missing-stdlib-module`: [Standard Library Modules](../stdlib/modules.md).
- `datapack-resource-format`: [Data Pack Resources](../stdlib/datapack-resources.md).
- `resource-pack-*` kinds: [Resource Packs](../stdlib/resource-packs.md#diagnostics).
- `temp-objective-reference` in `cobble migrate`: [Upgrading](../upgrading.md).
