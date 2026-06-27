# Cobble 0.9.0 Plan

Status: implemented release contract.

0.9.0 is the authoring-platform beta release. The release should make Cobble
feel less like a data-pack command transpiler and more like a project system
for authoring data packs, resource packs, metadata, editor tooling, and
controlled experiments that can graduate before 1.0.

0.9.0 can be wider than 0.8.0, but it should keep one rule: stable behavior is
documented and tested, while risky or still-fluid behavior is behind explicit
experimental opt-in.

## Release Thesis

- Promote the 0.8 resource-authoring work into a broader authoring platform.
- Move resource-pack support from experimental proof to beta-quality workflow.
- Add machine-readable compiler/tooling surfaces that editors and CI can rely
  on.
- Start experimental work on plugins, Python compatibility, and migrations
  without committing to their 1.0 API shape.
- Keep filesystem, namespace, symlink, validation, and compile-time expansion
  safety as release gates, not best-effort checks.

## Must Ship

### Resource Authoring V2

- Object-shaped tag entries:
  - `{"id": "namespace:path", "required": false}`
  - deterministic equality and merge semantics
  - mixed string/object tag arrays
- Real `replace` semantics for typed tags.
- Stable duplicate diagnostics for typed and pass-through resources.
- Source-aware diagnostics for invalid resource IDs, invalid tag entries, and
  invalid overwrites.
- Manifest/source-map entries for all generated resources.
- Snapshot coverage for merged tags, pass-through resources, and invalid
  declarations.

### Resource Pack Beta

- Promote `resource_pack.*` from 0.8 experimental to 0.9 beta.
- Keep the opt-in gate until the API is stable enough for 1.0.
- Add asset passthrough under `assets/` with path containment checks.
- Merge compatible `lang` declarations deterministically.
- Add stricter validation for item models, block models, and language files.
- Include generated resource-pack resources in source maps; include generated
  and static resource-pack assets in ZIP output, build manifests, and
  `inspect --json`.
- Add web and CLI examples that demonstrate data-pack and resource-pack output
  together.

### Project And Build Workflow

- Improve `cobble init` templates for resource-heavy projects, validation-heavy
  projects, and web-demo projects.
- Add clearer build profile language for development, validation, release, and
  web workflows.
- Strengthen config schema and runtime diagnostics for unknown keys, misplaced
  sections, invalid experimental flags, and unsafe paths.
- Keep no-write checks and dry-run behavior reliable for CI.

### CLI And Tooling Contracts

- Stabilize `cobble check --json` fields needed by CI and editor integrations.
- Keep `--symbols` explicitly experimental unless the schema is complete.
- Extend `cobble inspect --json` with stable resource-pack and manifest fields.
- Add a schema version and compatibility notes for JSON outputs.
- Add diagnostics that are specific enough for editor squiggles and quick fixes.

### Stdlib V3

- Expand helpers while keeping generated output visible:
  - item component helpers (initial JSON-only `item_component.*` helpers for
    `datapack.item_modifier()` are implemented; see `stdlib-v3-design.md`)
  - selector/entity helpers (initial `selector.*` value helpers and
    `entity.teleport()` are implemented; see `stdlib-v3-design.md`)
  - schedule helpers, including cancellation (`schedule.once` and
    `schedule.clear` are implemented and covered; see `stdlib-v3-design.md`)
  - storage path helpers (initial `storage.path/child/index` value helpers are
    implemented; see `stdlib-v3-design.md`)
  - small coordinate/vector helpers (initial `position.*` coordinate helpers
    are implemented; see `stdlib-v3-design.md`)
- Require each helper cluster to document:
  - accepted source form
  - generated commands or JSON
  - diagnostics
  - snapshot/validation coverage
- Avoid hidden objectives, storage keys, generated functions, load tags, or tick
  tags unless source explicitly opts in.

### Security And Performance Gates

- Keep aggregate loop expansion budgets enforced in CLI and WASM.
- Add regression coverage for resource-pack path containment and asset
  passthrough.
- Keep validated-build staging replacement ownership checks.
- Keep namespace and resource path validation shared between data-pack and
  resource-pack writers.
- Run a 0.9 release gate that includes the existing security regression gate,
  resource-pack browser ZIP tests, docs link checks, and representative
  validated builds.

## Experimental Track

The experimental track is allowed in 0.9.0, but every item must be opt-in,
clearly labeled in diagnostics and docs, and safe to remove or change before
1.0.

### Experimental Plugin System

Goal: let advanced users explore compiler/tooling extension points without
freezing the plugin API too early.

Detailed contract: `plugin-system-design.md`.

The first implementation should bias toward safe, deterministic, read-only
plugins. A useful 0.9 prototype is a diagnostics-only plugin host that can read
project metadata and source text and return warnings, but cannot write files,
run shell commands, open network connections, or mutate generated resources.

0.9.0 scope:

- Write a plugin RFC covering trust, capability boundaries, versioning, web
  support, and release compatibility.
- Add a plugin manifest draft, not a stable registry format. The initial
  implementation parses `plugins/*.toml` manifests in read-only mode and
  reports requested capabilities without executing plugin code.
- Prototype one narrow extension point behind an explicit gate, such as:
  - diagnostics-only lints, or
  - read-only project metadata inspection, or
  - generated-resource preview hooks.
- Require explicit enablement through CLI/config, for example
  `--experimental-plugins` and `[experimental] plugins = true`.
- Never auto-run plugins from a checked-out project without user opt-in.
- Keep native arbitrary code execution out of the default experiment. If a
  prototype needs executable plugins, it must document trust and local-only
  behavior prominently.

Prototype acceptance criteria:

- Plugin execution is disabled by default.
- A project-controlled config cannot enable executable code without an explicit
  CLI opt-in.
- Diagnostics-only plugins cannot change build output.
- `check --json` and human diagnostics identify plugin diagnostics as
  experimental.
- The plugin manifest format includes a version field and requested
  capabilities.

Not in 0.9.0:

- Stable plugin API.
- Public plugin registry.
- Unprompted project-controlled plugin execution.
- Server-side or cloud plugin execution.

### Experimental Python Compatibility

Goal: define Cobble's Python-inspired dialect precisely and test whether small
compatibility improvements help authors without turning Cobble into a Python
runtime.

The first implementation should improve predictability before it broadens
syntax. Compatibility mode should never make currently invalid syntax silently
mean something surprising; it should either compile a deliberately small
feature or produce a more actionable diagnostic.

0.9.0 scope:

- Publish a Cobble dialect support matrix for 0.9.
- Add machine-readable diagnostics for common unsupported Python constructs.
- Add compatibility tests for parser/transpiler behavior that intentionally
  resembles Python.
- Add an opt-in diagnostics-only compatibility report through
  `check --experimental-python-compat` and `[experimental] python_compat =
  true`; it documents the supported subset and reports detected unsupported
  Python-like constructs without changing compile semantics.
- Report unsupported constructs with suggested Cobble alternatives.

Compatibility candidates:

- `pass` as an explicit no-op statement.
- More Python-like boolean and comparison diagnostics.
- Clearer errors for comprehensions, decorators, classes, exceptions, imports,
  and unsupported assignment targets.
- Parser recovery that points at the unsupported construct instead of producing
  a generic parse failure.

Prototype acceptance criteria:

- Compatibility mode is disabled by default.
- The supported subset is listed in `docs/language-support.md`.
- Unsupported Python constructs remain errors, not runtime no-ops.
- CLI and WASM diagnostics stay aligned for supported single-file examples.
- Any syntax that compiles in compatibility mode has snapshot or integration
  coverage showing generated output.

Not in 0.9.0:

- Full Python compatibility.
- Python runtime semantics.
- Exceptions, classes, generators, comprehensions, arbitrary imports, or
  CPython-compatible evaluation behavior.

### Experimental Migration And Auto-Upgrade

Goal: help users move projects across Cobble and Minecraft target versions
without silently changing build behavior.

The first implementation should be report-first, with only narrow config-only
apply actions. It can identify changes that a future migration would make,
produce stable JSON for CI, and preserve all source files by default.

0.9.0 scope:

- Add a migration design for Cobble project files, source syntax, stdlib usage,
  pack format, and resource schemas.
- Prototype an explicit migration command, for example:

```bash
cobble migrate --from 0.8 --to 0.9
cobble migrate --from 0.8 --to 0.9 --json
cobble migrate --from 0.8 --to 0.9 --apply
```

- Make dry-run/report mode the default for the experiment.
- Require an explicit apply flag before modifying files.
- Allow `--apply` only for supported config-only changes, such as updating
  `project.pack_format` to the current target.
- Write a timestamped backup next to `cobble.toml` before applying config
  changes, and report that backup path in JSON and human output.
- Include source file summaries, planned/applied/skipped actions, and manual
  changes that cannot be automated.
- Add migration diagnostics to explain manual edits that cannot be automated.
- Include config before/after summaries, source-location review hints, and
  Python compatibility suggestions in migration JSON.

Auto-upgrade boundaries:

- `cobble build` must not silently rewrite project files.
- Automatic pack-format or resource-schema changes must not happen without an
  explicit migration command or opt-in flag.
- Build-time warnings may suggest a migration command, but should not apply it.

Prototype acceptance criteria:

- `cobble migrate` reports without modifying files by default.
- JSON output includes schema version, `from`, `to`, actions, skipped actions,
  diagnostics, and `changed: false` when no apply flag is supplied.
- Apply mode requires an explicit flag and refuses unknown source/target
  versions.
- Apply mode does not rewrite source files or enable experimental feature flags.
- Successful config apply reports `changed: true`, the resulting
  `project.pack_format`, and `config.backup_path`.
- Dry-run migration reports are deterministic across repeated runs; apply
  reports include a timestamped backup path.

Not in 0.9.0:

- Silent auto-upgrade during build.
- Guaranteed migration support for every old Cobble release.
- Semantic rewrites that cannot be explained in a dry-run report.

## Should Ship

- `cobble check --json --symbols` improvements for editor prototypes.
- More complete web compiler diagnostics and example selection.
- Browser-side resource-pack ZIP export with generated assets.
- Resource authoring cookbook examples.
- Resource-pack cookbook examples.
- Plugin diagnostics cookbook examples.
- 0.8 to 0.9 migration notes.
- A release-candidate QA script for the 0.9 gate.

## Stretch

- Initial LSP design document.
- Resource-pack-only planning document.
- Additional typed builders for predicates, loot tables, recipes, advancements,
  and dialogs.
- More complete static type diagnostics for ambiguous expressions.

## Out Of Scope

- Full Python compatibility.
- Stable plugin API or public plugin registry.
- Silent automatic upgrades.
- External asset processors.
- Server/cloud compilation service.
- Runtime framework features that hide generated state from the author.
- Minecraft version auto-upgrade without explicit migration review.

## Implementation Order

1. Write 0.9 contracts before code changes:
   - resource authoring v2
   - resource pack beta
   - experimental plugins
   - Python dialect compatibility
   - migration command
2. Add tests that encode the contracts before broad implementation.
3. Implement resource authoring v2.
4. Implement resource-pack beta workflow and asset passthrough.
5. Expand stdlib helper clusters with snapshots and validation coverage.
6. Add CLI/editor JSON schema improvements.
7. Prototype experimental features behind explicit flags.
8. Add 0.9 examples, migration notes, and release-gate scripts.

## Release Criteria

- No known high or medium security findings remain untriaged.
- Existing security regression gate passes.
- New resource-pack path and ZIP regression tests pass.
- CLI and WASM compiler behavior is aligned for supported single-file cases.
- Docs link checks pass.
- Generated-output snapshots are reviewed.
- `cargo package --locked` and publish dry-run pass.
- Experimental features are labeled in CLI help, config docs, diagnostics, and
  release notes.
