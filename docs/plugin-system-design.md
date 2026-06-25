# Experimental Plugin System Design

Status: 0.9.0 experimental implementation contract.

The 0.9.0 plugin system is an opt-in experiment. It exists to test extension
points before 1.0, not to freeze a stable plugin API.

The default experiment is diagnostics-only and read-only. A plugin can inspect
project metadata and source text and return diagnostics. It cannot mutate the
AST, write generated resources, run shell commands, open network connections,
or modify files.

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
```

The 0.9.0 config flag enables only the built-in host skeleton:

```toml
[experimental]
plugins = true
```

Project config alone must not enable executable plugins. The 0.9.0 flag can
parse draft manifests from `plugins/*.toml` in read-only mode, but it does not
execute project-supplied code. If any future plugin prototype can execute local
code, the CLI must require an explicit user action in the current invocation.

## Prototype Manifest

The manifest format is a draft and may change before 1.0.

```toml
plugin_version = 1
name = "example_lints"
kind = "diagnostics"

[capabilities]
read_project_metadata = true
read_source_text = true
emit_diagnostics = true
```

The host must reject manifests that request unknown capabilities. Capability
checks should be deny-by-default.

Cobble discovers draft manifests at `plugins/*.toml` relative to the project
configuration directory. Discovery does not recurse and refuses symlinked
manifest directories or manifest files.

## Diagnostics Contract

Plugin diagnostics must be marked as experimental and must not be confused with
core Cobble diagnostics.

Human output should include:

```text
warning: experimental plugin example_lints reported custom-rule
```

`check --json` should include a stable wrapper shape even while the plugin API
is experimental:

```json
{
  "kind": "experimental-plugin-diagnostic",
  "plugin": "example_lints",
  "plugin_kind": "custom-rule",
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
- Manifest diagnostics for unsupported versions, unsupported kinds, unknown
  capabilities, malformed TOML, and symlinked manifest paths.
- JSON output shape for plugin diagnostics.
- Tests proving plugins are disabled by default.

This is intentionally small. Broader execution models can be designed after the
read-only diagnostics path is tested.
