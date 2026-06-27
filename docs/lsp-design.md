# LSP Design Notes

Status: 0.9.0 editor-tooling draft.

Cobble does not ship an LSP server in 0.9.0, but the release exposes enough
machine-readable output for editor prototypes.

## Inputs

An editor integration should call:

```bash
cobble check --json --symbols --experimental-python-compat <path>
```

For plugin-aware projects, add `--experimental-plugins` only after the user
opts in for the workspace.

## Diagnostic Mapping

Use `diagnostics[]` for stable compiler diagnostics:

- `file`, `line`, and `column` are 1-based.
- `severity` is `error` or `warning`.
- `kind` is suitable for diagnostic codes.
- `help` can be shown as secondary text.

Python compatibility diagnostics remain experimental and are mirrored under
`experimental_python_compat.unsupported_detected[]` with
`suggested_cobble_alternative` hints when available.

## Symbols

`--symbols` adds `experimental_symbols[]`:

- `kind = "function"` for Cobble functions.
- `kind = "import"` for import declarations.
- `detail` is optional and may change before 1.0.

The symbol schema is intentionally experimental in 0.9. Editors should tolerate
unknown fields and missing `detail`.

## Future LSP Server Shape

A future local LSP server can wrap the same compiler pipeline:

- `textDocument/didOpen` and `didChange` run in-memory parsing.
- `textDocument/publishDiagnostics` maps `check --json` diagnostics.
- `textDocument/documentSymbol` maps `experimental_symbols`.
- Code actions can use `help` and `suggested_cobble_alternative`.

The server must not execute experimental plugins from project config without an
explicit editor/user opt-in for that workspace.
