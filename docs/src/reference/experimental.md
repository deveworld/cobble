# Experimental Features

These features are off by default and may change between releases. Each one
is enabled with a command-line flag or a key under `[experimental]` in
`cobble.toml`.

| Feature | Flag | `cobble.toml` |
| --- | --- | --- |
| Resource-pack output | `--experimental-resource-pack` | `resource_pack = true` |
| Plugin diagnostics | `cobble check --experimental-plugins` | `plugins = true` |
| Python compatibility report | `cobble check --experimental-python-compat` | `python_compat = true` |
| Editor symbols | `cobble check --json --symbols` | |

Resource-pack output has its own page: [Resource Packs](../stdlib/resource-packs.md).

## Plugin Diagnostics

Plugins are lint manifests in `plugins/*.toml`. Cobble reads them and runs the
built-in rules they list. It never loads or runs code from a plugin. Start from
the template:

```bash
cobble init --template plugin-diagnostics --name linted-pack
cd linted-pack
cobble check --experimental-plugins
```

```toml
plugin_version = 1
name = "example_lints"
kind = "diagnostics"
description = "Example diagnostics-only lint manifest"
minimum_cobble_version = "0.10.0"
diagnostic_rules = [
    "example_lints.no_tellraw",
    "example_lints.no_raw_op",
    "example_lints.no_gamemode_creative",
    "example_lints.max_raw_command_length",
]

[capabilities]
read_project_metadata = true
read_source_text = true
emit_diagnostics = true
```

Rules only run when the manifest grants both `read_source_text` and
`emit_diagnostics`. Unknown manifest fields and capabilities are errors. With
`--json`, findings appear under `experimental_plugins.diagnostics[]` with
`kind = "experimental-plugin-diagnostic"`.

## Python Compatibility Report

`cobble check --experimental-python-compat` adds a report of the Python-like
constructs Cobble supports, the ones your sources use, and the unsupported ones
it found. Unsupported constructs stay errors; an entry includes a
`suggested_cobble_alternative` when there is a direct rewrite. With `--json`,
the report is the `experimental_python_compat` object. The browser compiler
has the same report behind its Python compatibility toggle.

## Editor Symbols

`cobble check --json --symbols` adds an `experimental_symbols` array of
document symbols for editor integrations. The rest of the JSON report keeps its
stable `schema_version`.
