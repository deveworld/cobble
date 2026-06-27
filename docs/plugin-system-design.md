# Experimental Plugin System Design

Status: 0.9.0 experimental implementation contract.

The 0.9.0 plugin system is an opt-in experiment. It exists to test extension
points before 1.0, not to freeze a stable plugin API.

The default experiment is diagnostics-only and read-only. In 0.9.0, Cobble
parses project plugin manifests and can evaluate Cobble-owned declarative lint
rules requested by those manifests. Project-supplied plugin code is not loaded
or executed, and plugin manifests cannot mutate the AST, write generated
resources, run shell commands, open network connections, or modify files.

## Goals

- Let advanced users prototype project-specific lints.
- Learn which compiler and metadata surfaces should become stable before 1.0.
- Keep plugin output visible in human diagnostics and `check --json`.
- Avoid project-controlled code execution by default.

## Non-Goals

- Stable plugin API.
- Public plugin registry.
- Automatic plugin execution from checked-out projects.
- Server-side or cloud plugin execution.
- Build-output mutation hooks.
- Native executable plugins without an explicit separate trust decision.

## Opt-In

Plugin support is off by default. The initial experiment requires an explicit
CLI flag or config opt-in:

```bash
cobble check --experimental-plugins
cobble init --template plugin-diagnostics
```

The 0.9.0 config flag enables only the built-in host skeleton:

```toml
[experimental]
plugins = true
```

Project config alone must not enable executable plugins. The 0.9.0 flag can
parse draft manifests from `plugins/*.toml` in read-only mode and evaluate
Cobble-owned declarative diagnostics rules, but it does not execute
project-supplied code. If any future plugin prototype can execute local code,
the CLI must require an explicit user action in the current invocation.

## Prototype Manifest

The manifest format is a draft and may change before 1.0.

```toml
plugin_version = 1
name = "example_lints"
kind = "diagnostics"
description = "Example diagnostics-only lint manifest"
minimum_cobble_version = "0.9.0"
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

The host must reject manifests that request unknown capabilities. Capability
checks should be deny-by-default. Draft metadata fields are read-only:
`description`, `minimum_cobble_version`, and `diagnostic_rules` are reported by
`check --json`. Supported `diagnostic_rules` are interpreted by Cobble as
declarative built-in lints and do not cause plugin code to run.

Cobble discovers draft manifests at `plugins/*.toml` relative to the project
configuration directory. Discovery does not recurse and refuses symlinked
manifest directories or manifest files.
The `plugin-diagnostics` init template creates a read-only example manifest at
`plugins/example_lints.toml` and enables `[experimental] plugins = true` so
`cobble check` can exercise manifest parsing without running plugin code.

## Declarative Rules

0.9.0 supports a small set of example declarative diagnostics rules:

| Rule | Behavior |
| ---- | -------- |
| `example_lints.no_tellraw` | Warns on raw `/tellraw` commands. |
| `example_lints.no_raw_op` | Warns on raw `/op` commands. |
| `example_lints.no_gamemode_creative` | Warns on commands that switch players into creative mode. |
| `example_lints.max_raw_command_length` | Warns on raw command lines longer than 120 characters. |

When declared by a diagnostics manifest with `read_source_text` and
`emit_diagnostics`, Cobble checks source text and emits experimental plugin
warnings. Unknown safe rule ids are skipped with a warning. These rules are
implemented inside Cobble; manifest files cannot provide executable rule code.

## Diagnostics Contract

Plugin diagnostics must be marked as experimental and must not be confused with
core Cobble diagnostics.

Human output should include:

```text
warning: experimental plugin example_lints reported declarative-rule
```

`check --json` should include a stable wrapper shape even while the plugin API
is experimental:

```json
{
  "kind": "experimental-plugin-diagnostic",
  "plugin": "example_lints",
  "plugin_kind": "declarative-rule",
  "severity": "warning",
  "message": "..."
}
```

## Safety Contract

- Plugins are disabled by default.
- Diagnostics-only plugins cannot change build output.
- Plugin diagnostics are deterministic for the same source tree and plugin
  input.
- Unknown manifest versions fail closed.
- Unknown capabilities fail closed.
- Plugin loading never follows unsafe symlink paths without the same safety
  checks used for project output and config paths.
- WASM/web compilation does not run local plugins.

## First Implementation Slice

The first 0.9 implementation adds the host skeleton and read-only manifest
validation:

- CLI/config opt-in parsing for `check --experimental-plugins` and
  `[experimental] plugins = true`.
- A no-execution built-in host diagnostic that proves the JSON wrapper shape.
- Read-only parsing of `plugins/*.toml` draft manifests.
- Cobble-owned declarative diagnostics rule evaluation for
  the `example_lints.*` rule slice.
- Manifest diagnostics for unsupported versions, unsupported kinds, unknown
  capabilities, malformed TOML, and symlinked manifest paths.
- JSON output shape for plugin diagnostics.
- Tests proving plugins are disabled by default.

This is intentionally small. Broader execution models can be designed after the
read-only diagnostics path is tested.
