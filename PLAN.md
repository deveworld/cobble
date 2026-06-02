# Cobble 0.6.0 Release Plan

## Theme

Cobble 0.6.0 should make Cobble feel dependable for real Minecraft Java Edition
26.1.2 data pack projects.

The release theme is:

> Build data packs that are not only generated successfully, but also validated
> against the Minecraft 26.1.2 command tree before users try them in-game.

This release should not try to become a full pack development ecosystem like
Beet. Cobble should stay focused as a strong DSL compiler, while improving the
quality of generated output, validation, standard library coverage, and project
workflow.

## Target

- Cobble version: `0.6.0`
- Minecraft target: Java Edition `26.1.2`
- Data pack format: `101.1`
- Primary output format:
  - `pack.mcmeta` uses `min_format` and `max_format` arrays for decimal format.
  - Function folders use the modern singular `function` layout.
  - Tags use the modern singular `tags/function` layout.

## Release Goals

1. Integrate command validation into the normal build workflow.
2. Ship a practical standard library v1 for common data pack tasks.
3. Add first-class support for common data pack JSON resources.
4. Track generated command source locations so diagnostics can improve.
5. Stabilize multi-file projects, imports, and configured entry points.
6. Promote example projects into repeatable build-and-validate fixtures.
7. Remove or block placeholder runtime behavior from release builds.

## Non-Goals

These are explicitly out of scope for `0.6.0`.

- Resource pack authoring.
- A Beet-style plugin ecosystem.
- Package manager or remote module imports.
- Full LSP or editor integration.
- Supporting multiple Minecraft versions at the same time.
- Typed DSLs for every Minecraft JSON schema.
- A complete runtime framework for all gameplay systems.

## Workstreams

### 1. Build And Validation Workflow

Add validation as a first-class part of the CLI instead of a separate manual
step.

#### Tasks

- Add `cobble build --validate`.
- Add `cobble build --commands-json <PATH>`.
- Add `cobble watch --validate`.
- Add `cobble watch --commands-json <PATH>`.
- Keep `cobble validate <DATAPACK_DIR>` as a standalone command.
- Make validation failures include:
  - generated `.mcfunction` path,
  - line number,
  - command text,
  - parser error message.
- Print a validation summary after successful validation:
  - files checked,
  - commands checked,
  - macro lines skipped,
  - command tree path used.
- Fail the build when `--validate` is enabled and command validation fails.
- Make missing `commands.json` errors actionable:
  - mention `scripts/setup_commands_json.sh 26.1.2`,
  - show the expected default path.

#### Acceptance Criteria

- `cobble build --validate` builds and validates a valid project.
- `cobble build --validate` exits non-zero for invalid generated commands.
- `cobble watch --validate` validates after each successful rebuild.
- `cobble validate output --commands-json data/commands.json` remains supported.
- Tests cover success, validation failure, and missing command tree cases.

### 2. Command Validator Hardening

The command validator should remain strict enough to catch real generated
command bugs, while documenting what it cannot validate yet.

#### Tasks

- Keep the validator based on Minecraft's exported Brigadier command tree.
- Ensure 26.1.2 commands are covered:
  - `dialog`,
  - `fetchprofile`,
  - `transfer`,
  - `waypoint`,
  - `stopwatch`,
  - `version`,
  - `return run`,
  - `test run`.
- Improve skipped macro-line reporting.
- Add tests for validation error formatting.
- Add tests for command tree redirect cases, including:
  - `execute run`,
  - `return run`.
- Document current limitation:
  - lines starting with `$` are macro function lines and are skipped.

#### Acceptance Criteria

- Latest command smoke tests pass against generated `data/commands.json`.
- Validator does not reject valid `execute run` or `return run` commands.
- Macro skips are counted and visible in CLI output.

### 3. Standard Library v1

0.6.0 should include enough helpers to make real Cobble projects less raw-command
heavy, without hiding Minecraft from advanced users.

#### Modules

##### `text`

- `text.tellraw(target, component)`
- `text.title(target, component)`
- `text.subtitle(target, component)`
- `text.actionbar(target, component)`

Initial implementation can generate direct Minecraft commands. The API should
not require a full text-component builder in 0.6.0.

##### `score`

- `score.set(name, value)`
- `score.add(name, value)`
- `score.remove(name, value)`
- `score.reset(name)`
- `score.copy(dst, src)`
- `score.operation(dst, op, src)`

This should wrap common scoreboard commands while still allowing raw scoreboard
commands when needed.

##### `random`

- `random.int(name, min, max)`
- `random.bool(name)`

Prefer Minecraft's modern `random` command when valid for 26.1.2. Avoid older
entity-UUID randomness unless needed as a fallback.

##### `timer`

- `timer.set(name, ticks)`
- `timer.tick(name)`
- `timer.done(name)`
- `timer.reset(name)`

Timers should compile to scoreboard operations and simple conditions.

##### `storage`

- `storage.set(path, value)`
- `storage.merge(path, value)`
- `storage.remove(path)`
- `storage.copy(dst, src)`

This can start as a thin wrapper over `data modify storage`.

##### `math`

- Keep existing arithmetic operators.
- Implement or remove placeholder helpers.
- `math.sqrt` must not ship as a fake implementation.

#### Acceptance Criteria

- Every stdlib function has at least one integration test.
- Generated commands validate against the 26.1.2 command tree.
- No stdlib helper emits placeholder `tellraw` warnings as runtime behavior.
- Documentation includes concise examples for each module.

### 4. Data Pack JSON Resources

Cobble should be able to generate common data pack JSON files without users
manually maintaining separate output directories.

#### Initial Resource Types

- Function tags.
- Block tags.
- Item tags.
- Entity type tags.
- Predicates.
- Advancements.
- Loot tables.
- Recipes.
- Item modifiers.
- Dialog files.

#### 0.6.0 Design

Start with a pragmatic raw JSON declaration model:

```python
datapack.predicate("is_sneaking", {
    "condition": "minecraft:entity_properties",
    "entity": "this",
    "predicate": {
        "flags": {
            "is_sneaking": True
        }
    }
})
```

Do not attempt full schema-level typing for 0.6.0. That belongs in a later
release after the resource model is stable.

#### Tasks

- Add AST representation for data pack resource declarations.
- Add parser support for resource declaration calls if needed.
- Add `DataPack` storage for each resource type.
- Emit JSON resources into modern 26.1.2 folders:
  - `advancement`,
  - `loot_table`,
  - `predicate`,
  - `recipe`,
  - `item_modifier`,
  - `dialog`,
  - `tags/function`,
  - `tags/block`,
  - `tags/item`,
  - `tags/entity_type`.
- Validate JSON syntax before writing.
- Detect duplicate resource IDs.
- Add tests for folder layout and JSON output.

#### Acceptance Criteria

- A Cobble project can generate at least one file for each initial resource type.
- Duplicate resource IDs fail with a clear compile error.
- Generated JSON is pretty-printed and deterministic.
- Generated example project validates all generated `.mcfunction` commands.

### 5. Generated Command Source Tracking

Validation errors currently point to generated files. 0.6.0 should add the
internal structure needed to map generated commands back to Cobble source later.

#### Proposed Internal Type

```rust
pub struct GeneratedCommand {
    pub text: String,
    pub source: Option<SourceLocation>,
    pub kind: GeneratedCommandKind,
}

pub struct SourceLocation {
    pub file: PathBuf,
    pub line: usize,
    pub column: usize,
}

pub enum GeneratedCommandKind {
    UserCommand,
    StdLib,
    RuntimeSetup,
    ControlFlow,
    JsonGenerated,
}
```

The exact shape can change during implementation, but the compiler should stop
treating every command as an unannotated `String` internally.

#### Tasks

- Introduce source-location structures.
- Preserve source location through parsing or attach it during transpilation.
- Store generated commands with metadata internally.
- Convert to plain strings only at final write time.
- Add validation diagnostics that can optionally include source location when
  available.

#### Acceptance Criteria

- Existing generated output stays stable unless intentionally changed.
- At least user-written raw Minecraft commands carry source location metadata.
- Internal setup commands are distinguishable from user commands.
- Validation diagnostics are ready to include source file locations.

### 6. Multi-File Project Stability

Imports and configured entry points should be reliable enough for larger
projects.

#### Tasks

- Keep `build.entry_points` respected when building from `cobble.toml`.
- Prevent imported files from being compiled as independent top-level entry
  files when entry points are configured.
- Add import cycle detection.
- Add clear errors for missing imports.
- Add namespace-safe generated names for imported functions.
- Document import semantics:
  - entry files,
  - imported files,
  - top-level initialization behavior,
  - selector aliases,
  - constants.

#### Acceptance Criteria

- Multi-file fixture builds exactly the configured entry points.
- Import cycle errors include the cycle path.
- Missing import errors include the importing file.
- Rebuilding after deleting an imported function does not leave stale output.

### 7. Example Projects And Fixtures

The two verification projects created during development should become official
fixtures.

#### Candidate Fixtures

- `examples/26_smoke`
  - latest command coverage,
  - imports,
  - selector aliases,
  - events,
  - function macro parameters.
- `examples/26_feature_matrix`
  - boolean expressions,
  - `if`/`elif`/`else`,
  - `match`,
  - positive and negative step loops,
  - storage/string usage,
  - stdlib helpers,
  - validation coverage.

#### Tasks

- Move curated verification projects into `examples/`.
- Keep generated `output/` out of git unless snapshots are intentionally added.
- Add tests that build each fixture.
- Add tests that validate each fixture against `data/commands.json` when present.
- Add README links to the examples.

#### Acceptance Criteria

- `cargo test` builds example fixtures.
- A documented command validates all examples:

```bash
scripts/setup_commands_json.sh 26.1.2
cargo test --quiet
```

### 8. Testing And CI Quality

0.6.0 should increase confidence without making tests too brittle.

#### Test Types

- Parser tests for new syntax.
- Transpiler tests for generated command fragments.
- Integration tests for full `.cbl` programs.
- Example fixture build tests.
- Command validation tests.
- Snapshot or golden tests for selected generated packs.

#### Tasks

- Add focused regression tests for bugs found during 26.1.2 verification:
  - entry point handling,
  - complex `if` + `elif` boolean lowering,
  - no invalid `execute unless if`,
  - no raw `OR(...)` output.
- Add fixture tests for stdlib v1.
- Add JSON resource output tests.
- Add a test helper for build + validate.
- Keep tests deterministic:
  - sorted output where possible,
  - stable generated helper names,
  - no dependence on local Minecraft install.

#### Acceptance Criteria

- `cargo test --quiet` passes.
- `cargo check --quiet` passes.
- Targeted `rustfmt --check` passes for touched Rust files.
- Example projects build and validate.

### 9. Documentation

Documentation should match actual behavior exactly.

#### Files To Update

- `README.md`
- `docs/cli.md`
- `docs/language.md`
- `docs/api.md`
- `CHANGELOG.md`
- Example project READMEs if useful.

#### Topics

- 0.6.0 release theme.
- `build --validate`.
- Command tree setup.
- Stdlib v1 modules.
- Data pack JSON declarations.
- Multi-file project entry points.
- Validation limitations for macro lines.
- Supported Minecraft version and pack format.

#### Acceptance Criteria

- No docs mention unsupported pack formats as accepted.
- No docs claim placeholder or incomplete behavior as released functionality.
- Quick start can be followed from a clean checkout.

## Suggested Milestones

### Milestone A: Validation-First Build

- Implement `build --validate`.
- Implement `watch --validate`.
- Improve validation diagnostics.
- Add CLI tests.

Exit criteria:

- Invalid generated commands fail `build --validate`.
- Valid example project passes `build --validate`.

### Milestone B: Project Stability

- Harden entry points and imports.
- Add cycle/missing import errors.
- Add source tracking foundations.
- Promote example fixtures.

Exit criteria:

- Multi-file examples compile once and validate.
- Import errors are clear and tested.

### Milestone C: Stdlib v1

- Implement `text`, `score`, `random`, `timer`, `storage`.
- Fix or remove `math.sqrt`.
- Add docs and tests.

Exit criteria:

- Each stdlib helper has tests.
- Generated helper commands validate.

### Milestone D: JSON Resources

- Add raw JSON resource declarations.
- Emit modern folder layout.
- Add duplicate detection.
- Add resource tests.

Exit criteria:

- Fixture generates tags, predicate, advancement, loot table, recipe, item
  modifier, and dialog.

### Milestone E: Release Hardening

- Update docs and changelog.
- Run full test suite.
- Run example build + validate.
- Audit placeholder behavior.
- Prepare release notes.

Exit criteria:

- No known placeholder runtime behavior remains.
- All documented examples compile.

## Release Checklist

- [x] `Cargo.toml` version set to `0.6.0`.
- [x] README version text updated.
- [x] CLI docs updated.
- [x] Language docs updated.
- [x] API docs updated.
- [x] Changelog entry written.
- [x] `scripts/setup_commands_json.sh 26.1.2` succeeds.
- [x] `cargo fmt --check` or targeted `rustfmt --check` succeeds.
- [x] `cargo check --quiet` succeeds.
- [x] `cargo test --quiet` succeeds.
- [x] Official examples build.
- [x] Official examples validate.
- [x] `cobble build --validate` tested manually.
- [x] `cobble watch --validate` smoke tested.
- [x] No placeholder stdlib behavior remains.
- [x] No generated command validation regressions remain.

## Open Design Questions

1. Should stdlib helpers be real Cobble imports, built-in compiler intrinsics, or
   a mix of both?
2. Should JSON resource declarations use `datapack.*` calls, decorators, or
   top-level declarations?
3. Should `build --validate` become default in a later release?
4. Should Cobble eventually emit a Beet-compatible project or remain fully
   independent?
5. Should source maps be written to disk for external tools, or kept internal
   until diagnostics need them?

## Post-0.6 Candidates

These should be considered for `0.7.0` or later.

- Typed JSON schema builders.
- Beet interop.
- Resource pack support.
- Plugin API.
- Package/module registry.
- LSP/editor diagnostics.
- Multi-version Minecraft target support.
- More advanced macro validation with sample argument expansion.
- Source map files for generated data packs.
