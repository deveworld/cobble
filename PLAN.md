# Cobble 0.6.1 Implementation Plan

## Status

- Planning date: 2026-06-02
- Base release: `0.6.0`
- Target release: `0.6.1`
- Minecraft target: Java Edition `26.1.2`
- Data pack format: `101.1`

## Theme

Cobble 0.6.1 should be broader than a narrow patch. It should turn the 0.6.0
foundation into a more practical release for real projects while preserving
compatibility with existing 0.6.0 code.

The release theme is:

> Expand the 0.6.0 validation-first compiler into a more complete project
> workflow: better diagnostics, richer stdlib helpers, stronger data pack
> resources, repeatable server QA, and clearer docs.

0.6.1 can include non-breaking user-facing features because Cobble is still
pre-1.0, but it should avoid broad rewrites. The main rule is: every added
feature must be validated, documented, and represented in examples or tests.

## Release Goals

1. Improve command validation accuracy, macro handling, and diagnostics.
2. Expand source-map diagnostics so generated failures point back to Cobble
   source reliably.
3. Add stdlib v1.1 helpers for common Minecraft systems beyond 0.6.0 basics.
4. Expand the raw JSON data pack resource model with better ergonomics and
   guardrails.
5. Improve project workflows: init templates, build/watch behavior, ZIP output,
   and release packaging.
6. Add repeatable real-server QA with Purpur as an optional release gate.
7. Promote examples and docs into a complete 26.1.2 learning and QA path.
8. Keep all 0.6.0-compatible projects building without source changes.

## Scope Policy

0.6.1 may add features if they satisfy all of these constraints:

- The feature compiles to ordinary Minecraft Java Edition 26.1.2 data packs.
- The generated commands validate against `data/commands.json` when applicable.
- The implementation fits the existing compiler architecture.
- The docs can explain the behavior without promising future-only semantics.
- The feature has focused tests and at least one realistic example or fixture.

If a feature needs a new type system, a plugin system, cross-version targeting,
or a major parser redesign, defer it to 0.7.0 or later.

## Non-Goals

These are explicitly out of scope for `0.6.1`.

- Supporting a second Minecraft version.
- Changing the `101.1` data pack target.
- Adding a package manager or remote module imports.
- Adding a Beet-style plugin ecosystem.
- Full LSP/editor integration.
- Resource pack authoring.
- Schema-typed builders for every Minecraft JSON format.
- A runtime framework that hides Minecraft commands completely.
- Making the Minecraft server smoke test part of default `cargo test`.

## Workstreams

### 1. Validation Semantics And Diagnostics

The validator is now central to normal builds. 0.6.1 should make it more
accurate, more transparent, and easier to debug.

#### Current Findings

- `ValidationReport::commands_skipped` exists, but macro lines are not currently
  counted as skipped.
- `validate_command()` strips a leading `$` and validates the remaining command.
- CLI output says `skipped macro lines`, which can be misleading if macro lines
  were actually validated.
- `ValidationError::position` exists, but reported errors currently use
  `position: 0`.

#### Tasks

- Define one consistent macro validation policy:
  - validate `$` macro commands when the static command skeleton is parseable,
  - accept `$(name)` placeholders in argument positions where Minecraft macros
    are valid,
  - skip only macro lines whose dynamic expansion prevents meaningful checking,
  - report checked macro lines and skipped macro lines separately if needed.
- Populate error positions from the parser failure state.
- Add caret-style command diagnostics when a position is available.
- Improve validation report wording so summary counters cannot misrepresent
  actual behavior.
- Add command-tree regression tests that do not require `data/commands.json`.
- Add full 26.1.2 validation tests that run when `data/commands.json` exists.
- Add negative tests for close misspellings and incomplete command tails.
- Keep `fetchprofile` in static validator coverage, but do not execute it in
  server tests because it can contact Mojang services.

#### Acceptance Criteria

- `cobble validate` reports counters that match actual validator behavior.
- Invalid generated commands include generated file, line, command text, parser
  message, and useful position context when available.
- Existing valid macro-function output remains accepted.
- Intentional typos such as `titel` fail reliably.
- Validator unit tests can run without a locally generated command tree.

### 2. Source Map Diagnostics

Source maps should become useful in day-to-day debugging, not only an internal
metadata file.

#### Tasks

- Keep source map format version at `1` unless a breaking field change is
  unavoidable.
- Audit source locations for:
  - module-level initialization,
  - imported files,
  - stdlib calls,
  - function calls with generated storage setup,
  - control-flow helper functions,
  - macro functions,
  - generated runtime setup commands.
- Make validation diagnostics prefer Cobble source locations when available.
- Add imported-file validation failure coverage.
- Add duplicate-generated-command coverage, especially in `match` and helper
  functions.
- Decide whether generated JSON resources need source-map entries in 0.6.1 or
  should stay command-only.
- Avoid committing generated source maps because they contain local absolute
  paths.

#### Acceptance Criteria

- A validation failure in generated `.mcfunction` output points back to the
  originating `.cbl` file when source metadata exists.
- Source map validation catches stale or missing command entries without false
  positives on clean builds.
- Source map output remains deterministic except for expected source file paths.

### 3. Standard Library v1.1

0.6.0 introduced practical stdlib helpers. 0.6.1 should expand coverage for
common data pack systems while keeping helpers thin and predictable.

#### Candidate Modules

##### `text`

- Add convenience builders for common JSON text components:
  - plain text,
  - colored text,
  - bold/italic/underlined text,
  - score components,
  - selector components.
- Keep raw JSON component strings accepted.
- Ensure macro parameters in text commands still normalize correctly.

##### `score`

- Add objective helpers:
  - `score.objective.add(name, criteria, display_name?)`,
  - `score.objective.remove(name)`,
  - `score.objective.display(slot, objective?)`.
- Add comparison helpers that generate `execute if score` fragments where the
  existing compiler architecture can support them cleanly.

##### `storage`

- Add list/object path helpers for common `data modify storage` operations:
  - append,
  - prepend,
  - insert,
  - copy from entity/block/storage,
  - read into score.
- Make generated NBT/JSON values deterministic and validate command output.

##### `schedule`

- Add thin wrappers around Minecraft's `schedule function` command:
  - `schedule.once(function, delay, mode?)`,
  - `schedule.clear(function)`.
- Ensure function names are namespace-safe.

##### `bossbar`

- Add helpers for simple bossbar creation and updates:
  - add,
  - remove,
  - set value/max/name/color/style/visible/players.

##### `team`

- Add helpers for common team operations:
  - add/remove,
  - join/leave,
  - modify color/prefix/suffix/collision/nametag visibility.

##### `entity`

- Add small helpers that remain close to Minecraft commands:
  - tag add/remove,
  - effect give/clear,
  - attribute get/base set/modifier operations where 26.1.2 syntax is stable.

#### Tasks

- Choose the v1.1 module subset based on implementation risk.
- Keep helpers compiler-backed rather than runtime magic.
- Add tests for every helper.
- Validate generated helper commands against the 26.1.2 command tree.
- Add concise docs and example snippets.
- Avoid helpers that require hidden long-running runtime systems.

#### Acceptance Criteria

- Every shipped stdlib v1.1 helper has at least one integration test.
- Generated commands validate when `data/commands.json` is present.
- Helpers do not emit placeholder runtime warnings.
- Users can still write raw Minecraft commands for unsupported cases.

### 4. Data Pack Resource Model v1.1

0.6.0 added raw JSON data pack declarations. 0.6.1 should make them safer and
more ergonomic without attempting full schema typing.

#### Tasks

- Add clearer resource ID validation:
  - namespace/path rules,
  - lowercase enforcement,
  - helpful errors for invalid separators,
  - clear duplicate resource diagnostics.
- Add optional namespace-qualified resource declarations where useful:
  - `datapack.predicate("other_ns:path", {...})`,
  - `datapack.function_tag("minecraft:load", [...])`.
- Add resource merge/replace policy documentation.
- Add support or tests for nested resource paths for every supported resource
  type.
- Add deterministic ordering for emitted JSON and resource writes.
- Add light semantic validation for common mistakes where cheap:
  - tag `values` must be a list,
  - recipe/predicate/advancement top-level value must be an object,
  - dialog top-level value must be an object.
- Keep raw JSON escape hatches available.

#### Acceptance Criteria

- All supported resource types accept nested paths and reject invalid names.
- Duplicate resources fail with clear type/name information.
- Generated JSON remains pretty-printed and deterministic.
- Fixture projects cover tags, predicates, advancements, loot tables, recipes,
  item modifiers, and dialogs.

### 5. Language And Compiler Ergonomics

0.6.1 can add small, contained language improvements when they remove friction
for real data pack code.

#### Candidate Improvements

- Improve import diagnostics:
  - missing import path,
  - circular import chain,
  - duplicate symbol or function conflicts,
  - imported-file source locations.
- Add safer handling for configured entry points:
  - imported files are not compiled as top-level entries when entry points are
    configured,
  - stale output is removed after entry-point changes.
- Improve string and JSON command interpolation errors.
- Improve function-call diagnostics when parameter counts or unsupported return
  usage are involved.
- Add better errors for unsupported expressions instead of fallback no-ops.
- Add small parser/tokenizer hardening for Minecraft command syntax edge cases:
  - NBT compounds,
  - quoted strings,
  - ranges,
  - coordinates,
  - macro placeholders.

#### Deferred Candidates

- User-defined structs or classes.
- General list/dict runtime semantics.
- Async/task syntax.
- Full expression support inside every command argument.
- Function return values.

#### Acceptance Criteria

- Any language change is backward-compatible with 0.6.0 examples.
- New diagnostics are covered by regression tests.
- Unsupported features fail loudly and explain the supported alternative.

### 6. Project Workflow And CLI UX

The CLI should support the common edit-build-validate-test loop without requiring
manual cleanup or fragile commands.

#### Tasks

- Confirm `build --validate` leaves existing output untouched on validation
  failure.
- Confirm `build --validate --zip` only creates or updates the ZIP after
  validation succeeds.
- Clean stale staging directories after failed validation builds where possible.
- Improve watch mode:
  - debounce bursty filesystem events,
  - avoid repeated rebuilds for generated output changes,
  - watch updated `cobble.toml` source paths,
  - keep validation output readable.
- Expand `cobble init` templates:
  - minimal project,
  - stdlib starter,
  - validation-ready fixture-style project.
- Add a documented command for validating all examples.
- Run `cargo package --allow-dirty --locked` during release prep.

#### Acceptance Criteria

- Validation failure does not replace a previously good output directory.
- ZIP output is not refreshed from an invalid staging build.
- Watch mode is usable for normal source edits.
- New project templates build immediately from a clean checkout.

### 7. Real Server QA

Static command validation is necessary, but a real server catches pack loading,
tag loading, storage, reload, and runtime command behavior.

#### Tasks

- Add a documented wrapper script for the ignored server test, for example:

```bash
scripts/test_minecraft_server.sh
```

- Keep EULA acceptance explicit through `COBBLE_MINECRAFT_EULA_ACCEPTED=1`.
- Support a predownloaded jar through `COBBLE_PURPUR_JAR`.
- Cache server runtime files under `target/minecraft-server-test/`.
- Add optional Purpur jar checksum verification when metadata is available.
- Make failure output easy to inspect:
  - server console log path,
  - latest.log path,
  - exact command that timed out.
- Keep network-sensitive Minecraft commands out of server execution.
- Keep Paper as a later optional backend after Purpur remains stable.

#### Acceptance Criteria

- The server smoke test can be run with one documented command after EULA
  acceptance.
- A failed server test prints enough information to debug without rerunning.
- The test shuts down the server process on timeout or failure.
- Default `cargo test` remains fast and does not require Java or network access.

### 8. Example Projects And Documentation

Examples should function as both learning material and release fixtures.

#### Tasks

- Keep `examples/26_smoke` focused on latest command coverage, imports, events,
  macros, and validation.
- Expand `examples/26_feature_matrix` to cover the 0.6.1 stdlib/resource subset.
- Add one new realistic example project if the 0.6.1 feature set warrants it:
  - scoreboard HUD,
  - bossbar timer,
  - advancement/reward flow,
  - storage-driven state machine.
- Update docs:
  - README,
  - `docs/cli.md`,
  - `docs/language.md`,
  - `docs/api.md`,
  - examples README,
  - changelog.
- Replace hardcoded `Cobble v0.6.0` strings in runtime diagnostics where a
  package version or version-neutral phrase is more accurate.
- Document macro validation behavior exactly after implementation.

#### Acceptance Criteria

- Quick start works from a clean checkout.
- Examples build with documented commands.
- Examples validate when `data/commands.json` is present.
- Docs do not describe unsupported or placeholder behavior.

### 9. Release QA And Packaging

The wider 0.6.1 scope needs a stronger release gate than a narrow patch.

#### Required Checks

- `cargo fmt --check`
- `cargo test --quiet`
- `cargo clippy --all-targets --quiet -- -D warnings`
- `cargo package --allow-dirty --locked`
- Automatic `data/commands.json` generation code path is covered, and the
  external Mojang download path is smoke-tested where network access permits.
- Example fixture build and validation.
- Manual `cobble build --validate`.
- Manual `cobble watch --validate` smoke test.
- Ignored Purpur server smoke test for release validation.

#### Acceptance Criteria

- No generated data pack output, ZIP, or local source map artifacts are
  committed.
- Repository is clean before release tagging.
- Release notes list user-facing additions and validation caveats.

## Suggested Milestones

### Milestone A: Validation And Diagnostics

- Fix macro accounting and summary text.
- Improve validation error positions.
- Add command-tree fixture tests.
- Improve source-map-backed diagnostics.

Exit criteria:

- Validation reports are internally consistent.
- Generated command failures can be traced back to source in tested cases.

### Milestone B: Stdlib And Resource Expansion

- Select final stdlib v1.1 helper subset.
- Implement helpers with integration tests.
- Harden resource ID validation and nested paths.
- Expand fixture coverage.

Exit criteria:

- Every shipped helper and resource enhancement is tested and documented.
- Generated commands validate against 26.1.2 where applicable.

### Milestone C: Project Workflow

- Improve build/watch validation UX.
- Add or improve init templates.
- Add example validation commands/scripts.
- Run packaging checks.

Exit criteria:

- New users can create, build, validate, and package a project without ad hoc
  setup beyond `commands.json`.

### Milestone D: Server QA And Release Hygiene

- Add the server-test wrapper.
- Run the ignored real-server test.
- Update docs and changelog.
- Bump version to `0.6.1`.
- Prepare tag and release notes.

Exit criteria:

- Full release checklist is complete.
- Repository is clean and ready to tag.

## Release Checklist

- [x] Final 0.6.1 scope selected from candidate workstreams.
- [x] `Cargo.toml` version set to `0.6.1`.
- [x] `Cargo.lock` version updated.
- [x] `CHANGELOG.md` has a `0.6.1` entry.
- [x] README version text updated.
- [x] CLI docs updated.
- [x] Language docs updated.
- [x] API docs updated.
- [x] Examples README updated.
- [x] Runtime diagnostics do not hardcode stale `0.6.0` release text.
- [x] New stdlib helpers documented and tested.
- [x] Resource model changes documented and tested.
- [x] Validation macro policy documented and tested.
- [x] Source-map diagnostics tested.
- [x] Automatic `data/commands.json` generation policy and failure path tested.
- [x] Automatic `data/commands.json` generation succeeds end-to-end with
      `COBBLE_MINECRAFT_SERVER_JAR`.
- [x] Automatic `data/commands.json` generation succeeds end-to-end against the
      Mojang manifest endpoint.
- [x] `cargo fmt --check` succeeds.
- [x] `cargo test --quiet` succeeds.
- [x] `cargo clippy --all-targets --quiet -- -D warnings` succeeds.
- [x] `cargo package --allow-dirty --locked` succeeds.
- [x] Official examples build.
- [x] Official examples validate when `data/commands.json` is present.
- [x] `cobble build --validate` tested manually.
- [x] `cobble watch --validate` smoke tested.
- [x] Ignored Purpur server smoke test run for release validation.
- [x] No generated data pack output, ZIP, or local source map artifacts are
      committed.

## Resolved Decisions

1. Stdlib v1.1 includes objective, storage list/read, schedule, bossbar, team,
   and entity helpers. Particles, sounds, damage, and loot remain stretch goals.
2. Macro-function lines are validated as static command skeletons and counted
   separately. Malformed placeholders fail validation.
3. Resource declarations support explicit namespaces in 0.6.1.
4. `cobble init` exposes `--template minimal|stdlib|validation`.
5. Server smoke testing targets Purpur for now. Paper remains a later optional
   backend after the Purpur path stays stable.
6. Source-map validation remains part of `cobble validate` for 0.6.1.
7. `scripts/setup_commands_json.sh` jar metadata verification is deferred.

## Stretch Goals

These are useful but should not block 0.6.1 if core workstreams are complete.

- Optional Paper backend for the real-server smoke test.
- More stdlib helpers for particles, sounds, damage, and loot.
- Lightweight generated-pack snapshot tests.
- A `cobble examples validate` command or script.
- Initial machine-readable diagnostic output for editor tooling experiments.
