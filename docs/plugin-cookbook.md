# Plugin Diagnostics Cookbook

Status: 0.9.0 experimental cookbook.

The 0.9 plugin experiment is diagnostics-only. Cobble reads draft manifests and
evaluates built-in declarative rules; it does not load or execute project
plugin code.

## Start A Project

```bash
cobble init --template plugin-diagnostics --name linted-pack
cd linted-pack
cobble check --experimental-plugins
```

## Manifest

```toml
plugin_version = 1
name = "example_lints"
kind = "diagnostics"
minimum_cobble_version = "0.9.0"
diagnostic_rules = [
  "example_lints.no_tellraw",
  "example_lints.no_raw_op",
  "example_lints.no_gamemode_creative",
  "example_lints.max_raw_command_length",
]

[capabilities]
read_source_text = true
emit_diagnostics = true
```

Rules are skipped unless the manifest grants both `read_source_text` and
`emit_diagnostics`.

## JSON Gate

```bash
cobble check --json --experimental-plugins
```

Plugin diagnostics appear under `experimental_plugins.diagnostics[]` with
`kind = "experimental-plugin-diagnostic"`.
