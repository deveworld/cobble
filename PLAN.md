# Cobble Roadmap

## Status

- Planning date: 2026-06-16
- Current stable release: `0.7.3`
- Current development version: none
- Next planned release: `0.8.0`
- Active pre-release: none
- Current Minecraft target: Java Edition `26.1.2`
- Current data pack format: `101.1`
- Package name: `cobble-lang`
- CLI command: `cobble`
- Website: <https://deveworld.github.io/cobble/>
- Browser compiler: <https://deveworld.github.io/cobble/try/>

This roadmap tracks what Cobble should become after 0.7.3. Historical release
details and completed version plans belong in `CHANGELOG.md`.

## North Star

Cobble should be the practical authoring language for modern Minecraft Java
Edition data packs:

- Python-like enough to feel readable and productive.
- Explicit enough that generated `.mcfunction` output remains understandable.
- Modern enough to track current Minecraft command and pack-format changes.
- Tooling-friendly enough to support diagnostics, source maps, web demos, and
  editor integrations.
- Small enough that a standalone Rust CLI remains the default experience.

The goal is not to become a general Minecraft pack SDK like `beet`, a full
Python interpreter, or a hidden runtime framework. Cobble should stay focused
on turning `.cbl` source into transparent data pack output.

## Reference Project Lessons

The following projects were reviewed on 2026-06-04 as comparable or adjacent
tools:

- `beet` (`019783f`): strongest reference for project configuration,
  build/watch/link workflow, generated-pack inspection, example snapshots, and
  pack SDK maturity.
- `jmc` (`256836c`): strongest reference for authoring ergonomics, inline
  source-to-generated-output tests, NBT and vanilla macro shortcuts, and
  pack-feature gates.
- `cbscript` (`157c9b6`): strongest reference for real-world data pack examples,
  save-to-world iteration, selector/NBT/vector conveniences, compile-time
  unrolling, template functions, and output traceability.
- `MC-Language` (`bf50516`): useful reference for small package/class/macro
  structure and documentation that shows source plus generated JSON/functions.
- `CakeLang` (`3b38443`): useful as a stated module/static-analysis vision, but
  the reviewed implementation is still much thinner than the README claims.

Cobble should borrow engineering lessons, not copy the whole shape of any one
project. The best strategic position is:

- More current and easier to install than older transpilers.
- More compiler-focused and standalone than `beet`.
- Simpler and more transparent than a broad scripting runtime.
- Better validated against current Minecraft than most small language projects.

Concrete lessons to apply:

- Copy `beet`'s workflow clarity, not its whole SDK: config validation,
  `watch`/`link`, build summaries, and snapshot-tested examples are in scope;
  plugin injection, worker pools, cache ecosystems, and resource-pack pipelines
  are not.
- Copy `jmc`'s output discipline, not its JavaScript surface: small tests should
  make source input and generated file trees obvious, and future feature gates
  should be named around Minecraft capabilities even while Cobble supports only
  one active target.
- Copy `cbscript`'s authoring pain-point fixes, not its environment coupling:
  world iteration, source traceability, selector/NBT/vector helpers, and real
  examples matter; hard-coded world paths, arbitrary Python evaluation, and
  unbounded registry-generated code do not.
- Copy `MC-Language`'s transparent helper docs, not generic string macros:
  helpers should show generated output and validate where possible.
- Treat `CakeLang` as a warning about roadmap honesty: do not advertise broad
  static analysis, modules, or package support until the implementation and
  tests exist.

## Product Pillars

### 1. Command Correctness

Cobble must keep generated commands compatible with the documented target
Minecraft version. Validation should be strict when Cobble knows the parser and
conservative when the command tree contains argument shapes that are not fully
implemented yet.

### 2. Authoring Language

Cobble's language should remain Python-like, but it needs a clearer spec,
better semantic checks, and more predictable compile-time behavior. Users
should not need to guess which Python-looking features are supported.

### 3. Data Pack Resources

Functions are only one part of data packs. Cobble should keep improving
resources such as tags, predicates, advancements, loot tables, recipes, item
modifiers, dialogs, and future supported resource types.

### 4. Workflow And Tooling

The CLI should make normal project work boring: initialize, check, build,
validate, inspect, watch, link, and package. Future editor and formatting tools
should build on the same parser and metadata.

### 5. Web And Documentation

The website and `/try` compiler should remain a real product surface, not only
a marketing page. It should demonstrate current language behavior and generated
output accurately.

### 6. Release QA

Every release should have a clear gate: Rust tests, command validation,
examples, package dry-run, web build when relevant, and optional real-server
QA when EULA acceptance is available.

## Version Roadmap

### Release Sequence

- `0.7.1`: workflow and tooling hardening on top of the stable 0.7.0 line.
- `0.7.2`: follow-up stabilization for the new workflow surface. No new helper
  families, resource behavior, or schema commitments unless needed to fix a
  compatible bug.
- `0.7.3`: security hardening for workflow filesystem safety and execute-guard
  correctness before larger 0.8.0 work.
- `0.8.0`: stdlib v2, data-pack resource authoring, and compatibility prep that
  is too broad for a 0.7.x patch release.
- `1.0.0`: long-term compatibility contract for language behavior, CLI
  behavior, metadata schemas, and release gates.
- Future 1.x: plugin APIs, package/dependency management, richer resource-pack
  pipelines, multi-target Minecraft support, LSP, and ecosystem integrations
  after the core workflow and compatibility story are stable.

### 0.x Compatibility Rule

0.7.x releases may add opt-in workflow commands because Cobble is still pre-1.0,
but they must preserve the stable 0.7.0 source language and generated output
unless a documented bug fix requires a narrow behavior correction.

Rules for 0.7.x:

- no required `cobble.toml` migration,
- no grammar expansion,
- no broad stdlib or resource-authoring expansion,
- no change to existing command output unless it is a documented diagnostic or
  bug fix,
- no hidden runtime behavior,
- new JSON or metadata surfaces must include a `schema_version`,
- new JSON or metadata fields are experimental unless explicitly listed as
  stable,
- experimental fields must be namespaced or labeled so 1.0 can reserve forward
  compatibility without breaking early users.

### 0.7.1: Tooling And Authoring Workflow

Theme: make Cobble pleasant to use in larger projects.

#### Goals

- Ship first-class formatting as the next visible authoring-tool feature.
- Make save/build/validate/inspect loops safer and quieter for real projects.
- Add an explicit, reversible local-world linking workflow.
- Add cleanup, status, and machine-readable workflow commands for CI and editor
  tooling.
- Expand starter templates and examples into small but realistic data packs.
- Prepare editor metadata without introducing a second parser or full LSP
  commitment.

#### Implementation Progress

- Implemented in the current working tree:
  - `cobble fmt`, `cobble fmt --check`, and `cobble fmt --diff`
  - formatter regression coverage for raw command payloads, trailing comments,
    BOM/CRLF normalization, multiline docstring preservation, and directory
    failure atomicity
  - `cobble init --list-templates`
  - `cobble doctor --json` with experimental link-state and marker status
  - `cobble doctor --json` with experimental configured-output marker status
  - local `cobble clean` with safety markers and `--dry-run`
  - `cobble watch` debounce, ignore filtering, and config-reload watch-set
    helpers
  - `cobble link` state, status, clear, dry-run, and `watch --link` output
    resolution
  - `cobble clean --linked` with `--dry-run` and `--yes` confirmation
  - linked marker namespace and `project_id` ownership checks for
    `watch --link`, `link --status`, `clean --linked`, and `doctor --json`
  - linked state path-containment checks for `watch --link`, `link --status`,
    `clean --linked`, and `doctor --json`
  - build-manifest ownership fields for project root, stable project id, and
    generation timestamp
  - validated output replacement rollback that restores the previous output if
    the final staging move fails
  - build output refusal for existing non-directory paths so files are not
    replaced by generated data-pack directories
  - symlink component and descendant refusal for build outputs, link targets,
    and clean targets
  - `cobble link --world` and `--minecraft` path-resolution regression tests
  - `resource-heavy` init template with template build/check coverage
  - `game-mechanic` and `web-demo` init templates with template build/check
    coverage
  - `examples/resource_authoring` with generated-output snapshot coverage
  - `cobble check --json --symbols` with `schema_version` and experimental
    document-symbol metadata
  - focused 0.7.1 workflow QA scripts for templates, link/clean safety, and
    bounded watch smoke
  - aggregate 0.7.1 release-gate QA script that composes Rust checks, examples,
    workflow scripts, full-gallery validated builds, JSON smoke checks,
    link/clean checks, the full web gate, optional server smoke, and Cargo
    package dry-runs
  - focused QA coverage for mismatched linked markers, namespace-only forged
    markers, tampered link state, and validated rebuild failure preserving the
    previous linked pack
  - bounded `cobble watch` smoke QA with validation failure preservation,
    recovery, output-ignore coverage, and safe cleanup

#### Candidate Work

- Add `cobble fmt`:
  - deterministic formatting for `.cbl` files and project source directories
  - `--check` mode for CI
  - stable indentation and blank-line rules
  - preservation for raw `/` commands, macro commands, docstrings, comments,
    string literals, and inline JSON
  - formatter golden tests against examples and malformed-input tests that leave
    files untouched
- Improve `cobble watch`:
  - debounce and coalesce save bursts
  - ignore generated output, `.cobble/`, zip files, and editor temporary files
  - detect repeated rebuild loops caused by generated files
  - print timestamped changed-file summaries and validation summaries
  - reload `cobble.toml` safely when it changes
  - preserve the previous validated output when a rebuild or validation fails
- Add `cobble link` MVP:
  - accept explicit `--world`, `--minecraft`, or `--datapacks` paths
  - support `--dry-run` and `--clear`
  - support `--status` to show the configured link target and marker state
  - store link state in project metadata, not source code
  - write a Cobble marker in generated packs and refuse to replace unrelated
    directories or zips without a matching marker
  - allow `cobble watch --link` only after a link target is configured or
    explicitly provided
  - keep real-world mutation out of default tests by using temporary directories
- Add safe cleanup and workflow status commands:
  - `cobble clean` for generated project output only
  - `cobble clean --linked` for Cobble-marked linked output only
  - `--dry-run` and explicit confirmation for destructive cleanup
  - `cobble doctor --json` for CI/editor health checks
  - optional build and link status in `doctor`
- Design or prototype editor-oriented metadata without a full server:
  - keep `check --json` as the diagnostic source of truth
  - document the command shape before implementation
  - add or expose document symbols only if they can be schema-versioned and
    labeled experimental
  - keep generated-output navigation based on `.cobble/source_map.json`
  - defer completion and full LSP until formatter/watch/link are stable
- Expand `cobble init` templates:
  - minimal
  - stdlib
  - validation
  - resource-heavy
  - game-mechanic starter
  - web-demo sample
  - `--list-templates` to make templates discoverable
- Design project workflow profiles, but do not ship them in 0.7.1:
  - named profiles for output directory, validation, zip, and link target may be
    designed
  - `--profile <NAME>` implementation is deferred until link/watch/clean have
    real usage
  - no remote dependencies or per-profile language behavior
- Add more example projects that feel like real data packs, not only feature
  tests.
- Document examples in the source-plus-generated-output style used well by
  smaller languages:
  - Cobble source
  - generated `.mcfunction`
  - generated tag/resource JSON
  - command validation result

#### Acceptance Criteria

- Formatting is deterministic.
- `cobble fmt --check` passes on repository examples.
- Watch rebuilds do not trigger from Cobble's own output.
- Link operations avoid destructive writes outside marked Cobble-generated pack
  paths.
- `watch --link --validate` preserves the last known-good pack on validation
  failure.
- `clean` refuses to delete unmarked or unrelated output.
- `doctor --json` has a tested stable core shape with `schema_version`, status,
  and checks; any extra fields are labeled experimental.
- Every `cobble init` template builds, checks, and is covered by CLI tests.
- Larger examples build, validate when `commands.json` is available, and have
  generated-output snapshots.
- Editor metadata uses the same parser and diagnostics as the CLI.

#### Non-Goals

- No remote package imports.
- No editor feature that requires a separate parser implementation.
- No automatic world modification without explicit user opt-in.
- No broad plugin API or `beet`-style pack SDK.
- No resource-pack asset pipeline.
- No JavaScript-like header macro system.
- No arbitrary Python evaluation or general-purpose embedded interpreter.
- No project profile behavior that changes language semantics.
- No multi-target Minecraft support in 0.7.1.

#### Detailed 0.7.1 Plan

0.7.1 should be a workflow release. It should not compete with 0.8.0 for
stdlib or resource expansion. The release is successful when a user can create a
project, format it, watch it, validate it, link it to a test world, clean the
generated result safely, and inspect project health with fewer manual steps and
less risk of overwriting unrelated files.

##### Release Thesis

Cobble already has a useful compiler, validator, source map, build manifest,
examples, and web demo. The next gap is not more syntax; it is day-to-day
project ergonomics.

The release should therefore prioritize:

1. deterministic source formatting,
2. reliable rebuild loops,
3. explicit local-world linking,
4. safe cleanup and project health reporting,
5. realistic starter projects,
6. editor-ready metadata built from existing parser and diagnostic paths.

The release should avoid:

1. broad stdlib v2 helper expansion,
2. selector/NBT/vector language syntax,
3. package/dependency management,
4. plugin APIs,
5. project profiles that change language behavior,
6. multi-version Minecraft targeting.

##### Phase 0: Design And Baseline Audit

Before implementation starts, write short design notes or issue descriptions
for the three user-facing commands: `fmt`, improved `watch`, and `link`.

Tasks:

- Audit current parser/tokenizer behavior for formatter viability:
  - comments,
  - docstrings,
  - raw `/` commands,
  - macro commands with `$`,
  - multiline arrays/maps,
  - inline JSON/SNBT-looking values,
  - indentation diagnostics.
- Audit build output ownership:
  - existing `.cobble/build_manifest.json`,
  - existing `.cobble/source_map.json`,
  - any new `.cobble/link_state.json` or ownership marker,
  - output replacement behavior,
  - dry-run and validation staging behavior,
  - zip packaging exclusions.
- Audit project config extension points:
  - whether link state belongs in `.cobble/` metadata or `cobble.toml`,
  - workflow profile design only; implementation belongs in 0.7.3 or later
    after link/watch/clean behavior is proven,
  - how `doctor --json` should expose project, output, commands-json, and link
    health without performing network work.
- Decide 0.7.1 link scope:
  - directory data pack links are required,
  - zip links are stretch unless a safe marker/sidecar design is accepted,
  - real Minecraft save discovery is convenience, not required for the MVP.
- Decide cleanup scope:
  - local configured output is required,
  - linked output cleanup is required only when link state and marker match,
  - zip cleanup is allowed only when Cobble can prove ownership.
- Decide formatter safety rules:
  - format only syntactically valid files,
  - never write partial output after parse or formatting failure,
  - preserve raw command text unless the rule is explicitly documented.

Deliverables:

- A short formatter design section in `docs/cli.md` or a new `docs/fmt.md`.
- A short link safety section in `docs/cli.md`.
- A short cleanup safety section in `docs/cli.md`.
- A JSON schema note for any new `doctor --json`, link-state, or marker fields.
- A 0.x metadata note that separates stable fields from experimental fields.
- Focused TODO issues or checklist entries for the implementation phases.

##### Phase 1: `cobble fmt`

`cobble fmt` is the primary visible feature for 0.7.1. It should be useful even
before editor integrations exist.

Required CLI behavior:

- `cobble fmt [SOURCE]` formats one file, a directory, or the configured project
  source directory.
- `cobble fmt --check [SOURCE]` exits non-zero when any file would change.
- `cobble fmt --diff [SOURCE]` is optional but useful if it can be implemented
  without adding heavy dependencies.
- The command should use the same file discovery and config lookup expectations
  as `check` and `build` where practical.

Formatter rules for 0.7.1:

- Normalize indentation to four spaces.
- Normalize blank lines around top-level imports, constants, selectors,
  functions, and resource declarations.
- Preserve comments and docstrings.
- Preserve raw command lines exactly except leading indentation.
- Preserve inline JSON/SNBT-looking payloads exactly unless the parser already
  owns that structured value.
- Preserve string literal contents.
- Preserve quote style for unchanged string literals; if the parser cannot do
  that safely, formatter implementation must be cut or scoped before release.
- Preserve trailing comments.
- Keep formatting deterministic across platforms, including CRLF input.
- Normalize EOF newline behavior.
- Define BOM, tabs, mixed indentation, multiline strings, and blank lines inside
  blocks through named formatter fixtures.

Tests:

- Unit tests for individual formatting rules.
- CLI regression tests for `fmt`, `fmt --check`, and malformed inputs.
- Golden tests for existing examples.
- A regression test proving raw command text and inline JSON are not rewritten.
- A regression test proving failed formatting leaves the original file content
  unchanged.
- Regression tests for trailing comments, EOF newline, CRLF input, BOM, tabs,
  mixed indentation, multiline strings, and blank lines inside blocks.

Exit criteria:

- `cobble fmt --check examples` passes after repository examples are formatted.
- Running `cobble fmt examples` twice produces no second diff.
- Formatting failures report file, line, column, and a clear reason.

##### Phase 2: `cobble watch` Hardening

`watch` already exists. 0.7.1 should make it reliable enough to use as the
normal development loop.

Required behavior:

- Coalesce rapid file events into one rebuild.
- Ignore generated output paths, `.cobble/`, zip files, editor swap files, and
  temporary files.
- Ignore staging directories and generated output even when output is nested
  under the project root.
- Define symlink handling for watched source, configured output, and linked
  output.
- Watch `cobble.toml` and reload project config after valid changes.
- If config reload fails, report the error and keep watching the previous valid
  source path when possible.
- After a valid config reload, stop watching old source directories that are no
  longer part of the project.
- If the initial build fails, keep watching and make the next valid edit recover
  without restarting the process.
- Print a concise timestamped rebuild summary:
  - changed file count or primary changed file,
  - source files compiled,
  - generated functions/resources,
  - validation result when enabled.
- Preserve the previous validated output when a rebuild or validation fails.

Tests:

- Unit tests for event filtering and debounce decisions.
- CLI-level tests for config reload helper functions where they can be tested
  without long-lived watcher sessions.
- Regression tests for validation failure preserving previous output.
- Regression tests for nested output ignores, staging-dir ignores, symlink
  policy, initial-build failure recovery, and config reload watch-set changes.
- Manual smoke test for a real watcher session documented in `docs/qa.md`.

Exit criteria:

- Saving generated output does not trigger another rebuild.
- A broken source edit reports the error and the next valid edit rebuilds
  successfully.
- A failed validated rebuild does not delete or replace the last valid output.

##### Phase 3: `cobble link` MVP

`link` should make local world iteration explicit and reversible. It should be
safer than telling users to point `build -o` directly at a world folder.

Required CLI behavior:

- `cobble link --datapacks <DIR>` records a target datapacks directory.
- `cobble link --world <DIR>` resolves `<DIR>/datapacks`.
- `cobble link --minecraft <DIR>` resolves
  `<DIR>/saves/<pack-name>/datapacks`.
- `cobble link --dry-run ...` prints the target and actions without writing.
- `cobble link --status` reports configured target, resolved output path, and
  whether a matching Cobble marker exists.
- `cobble link --clear` removes project link state.
- `cobble watch --link` builds to the configured linked target.

Safety rules:

- Link state must live in project metadata or config, never inside `.cbl`
  source.
- The first linked build writes a Cobble ownership marker.
- Subsequent linked builds may replace only a pack with a matching marker.
- If the target exists without a matching marker, fail with an actionable error.
- The marker should include project namespace, Cobble version, project root or
  stable project id, and generated timestamp.
- `link --clear` removes link state, not the linked data pack; cleanup belongs
  to `clean --linked`.
- Default tests must use temporary directories, not a real Minecraft save.

Filesystem safety contract:

- Canonicalize link, output, and cleanup paths before comparing them.
- Refuse symlink traversal for generated output, linked targets, and cleanup
  targets unless a future explicit unsafe mode is designed.
- Require the resolved pack output to remain under the resolved `datapacks`
  directory.
- Refuse pack-name collisions with unmarked directories or zips.
- Treat copied, stale, or tampered markers as unsafe unless project id,
  namespace, and marker schema match.
- Use staging and atomic replacement where the platform supports it.
- Preserve the last validated output if replacement, validation, or cleanup
  fails.
- Document stale-marker recovery without requiring users to delete world data by
  hand.

Implementation notes:

- Prefer directory data pack output for the MVP because `.cobble/` metadata can
  act as a clear marker.
- Treat zip-linked packs as stretch work unless a sidecar marker or embedded
  safe-marker design is accepted.
- Reuse build staging and validation-preservation behavior where possible.
- `inspect` should recognize linked outputs through the same manifest path.

Tests:

- CLI tests for `link --dry-run`, `link --clear`, explicit `--datapacks`, and
  explicit `--world`.
- CLI tests for `link --status` with no link, a configured link, and a stale or
  missing target.
- Safety tests for refusing unmarked existing targets.
- Safety tests for accepting matching Cobble-marked targets.
- Integration test for `watch --link` may be scoped to helper functions if a
  long-lived watcher test would be flaky.

Exit criteria:

- A user can configure a temporary world datapacks directory, build through
  `watch --link`, and inspect the result.
- Cobble never overwrites an unrelated directory or zip in default behavior.
- `link --status` is useful enough to diagnose a broken link target without
  running a build.

##### Phase 3.5: Cleanup, Status, And Profiles

These features are smaller than `fmt`, `watch`, and `link`, but they make the
workflow feel complete and reduce support/debugging cost.

Required CLI behavior:

- `cobble clean` removes the configured project output only when Cobble can
  prove it is generated output.
- `cobble clean --dry-run` prints what would be removed.
- `cobble clean --linked` removes the linked generated pack only when link state
  and ownership marker match.
- `cobble clean --linked --dry-run` is required before any real linked cleanup
  implementation is accepted.
- `cobble doctor --json` emits machine-readable project health data.
- `cobble init --list-templates` lists available templates and short
  descriptions.

Deferred profile behavior:

- 0.7.1 may write a design note for `[profiles.<name>]`, but should not ship
  `--profile` behavior.
- Future profile fields may cover `output`, `validate`, `zip`, `commands_json`,
  and link target selection.
- `build --profile <name>` and `watch --profile <name>` should wait until
  `fmt`, `watch`, `link`, and `clean` have real usage.
- Profiles must never affect language semantics, parser behavior, stdlib
  version, or Minecraft target in 0.7.x.

Safety rules:

- `clean` must refuse paths outside configured output or linked output.
- `clean` must refuse unmarked directories unless a future explicit force design
  is accepted.
- `doctor --json` must not perform network downloads.
- `doctor --json` must write valid JSON to stdout and human diagnostics to
  stderr.
- `doctor --json` must include `schema_version`, top-level status, check ids,
  check status levels, and documented exit-code behavior.
- Experimental `doctor --json` fields must be labeled or namespaced.

Tests:

- CLI tests for `clean --dry-run`, safe local cleanup, unmarked output refusal,
  and linked cleanup refusal.
- JSON shape tests for `doctor --json`.
- CLI tests for `init --list-templates`.
- Design-only profile notes if profiles remain deferred.

Exit criteria:

- Users can tell what Cobble thinks the project, output, validation, and link
  state are without building.
- Users can clean generated output without risking unrelated world data.
- Templates are discoverable without reading docs.

##### Phase 4: Templates, Examples, And Docs

0.7.1 should make Cobble look useful immediately after `init`, not only after
reading the language reference.

Required templates:

- `minimal`: smallest valid data pack.
- `stdlib`: event-based load/tick starter.
- `validation`: demonstrates commands that pass current validation.
- `resource-heavy`: includes tags plus multiple JSON resource kinds.
- `game-mechanic`: small playable or observable mechanic with scoreboards,
  events, and validation-friendly commands.
- `web-demo`: compact source that demonstrates what `/try` should show.

Example project requirements:

- Add at least one realistic project fixture beyond feature matrix coverage.
- Prefer a tiny gameplay loop or map utility that uses load/tick, scoreboard
  state, at least one JSON resource, and source-map-friendly functions.
- Keep it small enough to review.
- Build it in snapshot tests.
- Validate it when `data/commands.json` is available.
- Document source plus generated `.mcfunction` and generated JSON output.

Docs:

- Update README command list if `fmt` and `link` ship.
- Update `docs/cli.md` for `fmt`, improved `watch`, `link`, `clean`,
  `doctor --json`, and template discovery.
- Update `docs/qa.md` with 0.7.1 workflow-specific QA.
- Update `docs/metadata.md` if link markers, link state, or `doctor --json`
  schemas are introduced.
- Keep `/try` examples aligned if a new web-demo sample is introduced.

Exit criteria:

- Every template is covered by a CLI test.
- `init --list-templates` output is covered by a CLI test.
- `scripts/check_examples.sh` covers new examples correctly.
- Snapshot updates are intentional and reviewable.

##### Phase 5: Experimental Editor Metadata

0.7.1 may prepare editor integration without shipping a full LSP. This phase is
not a release blocker unless the command shape and stable/experimental field
split are documented before implementation.

Required behavior:

- Keep `check --json` as the diagnostic contract.
- Add document-symbol style metadata if it can be extracted from the existing
  parser without a second implementation.
- Include functions, imports, selector aliases, and resource declarations.
- Keep generated-output navigation based on `.cobble/source_map.json`.
- Include diagnostic codes, related ranges, symbol ids, target context, and
  generated-resource mappings in the 1.0 metadata audit even if 0.7.1 ships only
  a subset.

Possible CLI shape:

- Extend `check --json` with optional `--symbols` data, or
- Add `cobble inspect-source --json [SOURCE]` only if extending `check` would
  make the diagnostic contract confusing.

Exit criteria:

- Any shipped metadata includes `schema_version` and stable-vs-experimental
  field labeling.
- Metadata tests assert only documented stable fields unless the test is
  explicitly for experimental output.
- Editor-facing metadata does not require generated output unless source-map
  navigation is requested.

##### Phase 6: Release Candidate And QA

Before the first 0.7.1 release candidate:

```bash
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo run --locked -- --version
scripts/check_examples.sh
cargo run --locked -- fmt --check examples
cargo run --locked -- check --json examples/26_smoke/src/main.cbl
cargo run --locked -- build examples/26_smoke --validate -o /tmp/cobble-qa-26-smoke
cargo run --locked -- build examples/26_feature_matrix --validate -o /tmp/cobble-qa-26-feature-matrix
cargo run --locked -- build examples/inventory.cbl --validate -o /tmp/cobble-qa-inventory
cargo run --locked -- init --name /tmp/cobble-qa-init-resource --template resource-heavy
cargo run --locked -- build /tmp/cobble-qa-init-resource --validate -o /tmp/cobble-qa-init-resource-output
cargo run --locked -- init --list-templates
mkdir -p /tmp/cobble-qa-world/datapacks
cargo run --locked -- link /tmp/cobble-qa-init-resource --dry-run --datapacks /tmp/cobble-qa-world/datapacks --pack-name qa_resource
cargo run --locked -- link /tmp/cobble-qa-init-resource --datapacks /tmp/cobble-qa-world/datapacks --pack-name qa_resource
cargo run --locked -- link /tmp/cobble-qa-init-resource --status
cargo run --locked -- build /tmp/cobble-qa-init-resource -o /tmp/cobble-qa-world/datapacks/qa_resource
cargo run --locked -- clean --dry-run --output /tmp/cobble-qa-26-smoke
cargo run --locked -- doctor --json
cargo run --locked -- doctor
cargo run --locked -- clean /tmp/cobble-qa-init-resource --linked --dry-run
cargo run --locked -- build examples/26_smoke --dry-run --validate
cargo run --locked -- inspect /tmp/cobble-qa-26-smoke
cargo run --locked -- inspect /tmp/cobble-qa-26-smoke --json
cargo package --locked
cargo publish --dry-run --locked
```

Before release, add and run focused QA scripts or equivalent CI jobs:

```bash
scripts/qa_07_templates.sh
scripts/qa_07_link_clean_safety.sh
scripts/qa_07_watch_smoke.sh
scripts/qa_07_release_gate.sh
```

Required focused QA coverage:

- initialize every template,
- run `fmt --check`, `check`, `build --validate`, and `inspect` for every
  template,
- perform a real temp-dir linked build into a Cobble-marked pack,
- verify `link --status` for configured, missing, stale, and unmarked targets,
- verify unmarked target refusal,
- verify stale or tampered marker refusal,
- verify symlink and ancestor-containment refusal,
- verify `clean --linked --dry-run` and real linked cleanup on a marked temp
  pack,
- verify failed validation preserves the last good linked output,
- verify existing file outputs are refused and preserved,
- run a bounded watcher smoke or watcher-helper integration test for
  `watch --link --validate`.

If web build inputs changed:

```bash
cd web
npm run test:wasm
npm run test:zip
npm run lint
npm run build:github
npm run test:e2e:run
npm run test:links
```

Optional before final release:

```bash
COBBLE_MINECRAFT_EULA_ACCEPTED=1 scripts/test_minecraft_server.sh
```

Manual QA:

- Run `cobble init` for every template in a temporary directory.
- Run `cobble fmt`, `check`, `build --validate`, and `inspect` on each template.
- Run `link --dry-run` and a real marked linked build against a temporary
  world-like directory.
- Run `clean --dry-run` and marked linked cleanup against temporary output.
- Run a bounded `watch --link --validate` smoke or documented watcher-helper
  test.
- Run `doctor --json` and confirm it reports config, validation environment,
  output, and link status without network access.
- If using a real Minecraft save, do it only as an explicit manual step and
  verify the target pack is disposable.

##### Cut Or Defer Rules

Cut from 0.7.1 if implementation would require:

- new parser grammar beyond what formatting or symbol extraction needs,
- broad stdlib helper design,
- selector/NBT/vector syntax,
- plugin or package architecture,
- resource-pack asset generation,
- hidden runtime behavior,
- real-world filesystem mutation without a marker,
- long-lived watcher tests that are flaky in CI.

Move to 0.7.3 or later 0.7.x patch releases if:

- the feature is already shipped but needs hardening,
- docs need clarification,
- a safety bug is found in `fmt`, `watch`, or `link`,
- a template or example needs small compatibility maintenance.

### 0.7.3: Workflow Stabilization And 0.8 Scope Freeze

Theme: stabilize the expanded 0.7 workflow before broad stdlib or resource
changes.

#### Goals

- Use 0.7.1 feedback to harden `fmt`, `watch`, `link`, `clean`, `doctor`,
  templates, and metadata.
- Make the new workflow commands predictable enough to build larger 0.8 work on
  top of them.
- Close documentation, examples, and QA gaps discovered during 0.7.1 usage.
- Finalize the 0.8.0 scope before starting larger helper or resource work.

#### Candidate Work

- Workflow hardening:
  - fix formatter edge cases found in real projects
  - improve `fmt --check` and optional `fmt --diff` output
  - tune watch debounce and generated-output ignore rules
  - improve config reload diagnostics
  - harden linked-output marker checks
  - improve `clean --dry-run` explanations
  - stabilize `doctor --json` fields or clearly mark experimental fields
- Documentation and QA:
  - update `docs/cli.md` with every 0.7.1 command edge case
  - document link-state and marker recovery steps
  - add troubleshooting docs for validation data and command-tree cache issues
  - turn manual 0.7.1 QA findings into tests where practical
  - keep README, website, `/try`, and examples aligned with CLI behavior
- Template and example maintenance:
  - improve starter project naming and generated output readability
  - add one more small realistic example only if it validates cleanly
  - keep generated-output snapshots concise and reviewable
  - verify templates do not rely on local machine paths
- 0.8.0 preparation:
  - finalize required, stretch, and deferred stdlib helper families
  - finalize data-pack resource kind scope
  - decide stdlib import/versioning model before implementation
  - decide Minecraft target policy before helper contracts are implemented

#### Acceptance Criteria

- 0.7.1 workflow commands have regression tests for reported edge cases.
- Formatting remains deterministic across repeated runs and line-ending styles.
- Link and clean operations still refuse unmarked or unrelated outputs.
- `doctor --json` has tested schema behavior for all stable fields.
- The release can be cut without updating the language grammar.
- No new helper family, resource behavior, or metadata schema commitment ships.

#### Non-Goals

- No stdlib v2.
- No broad resource-pack pipeline.
- No plugin API.
- No package manager or remote dependency model.
- No multi-target Minecraft support.
- No arbitrary Python/eval or embedded interpreter.
- No breaking language changes.

#### Release Candidate And QA

Before the first 0.7.3 release candidate:

```bash
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo run --locked -- --version
scripts/check_examples.sh
scripts/qa_07_templates.sh
scripts/qa_07_link_clean_safety.sh
scripts/qa_07_watch_smoke.sh
cargo run --locked -- doctor
cargo run --locked -- doctor --json
cargo package --locked
cargo publish --dry-run --locked
```

Focused 0.7.3 QA must include a regression test or documented manual result for
every 0.7.1 issue fixed in the release.

### 0.8.0: Stdlib V2 And Resource Authoring

Theme: make Cobble useful for real data pack authors beyond basic functions.

#### Goals

- Expand the standard library while keeping helpers thin and transparent.
- Improve resource declaration ergonomics.
- Add a small set of high-value abstractions seen in mature datapack languages.
- Prepare 1.0 by documenting helper contracts, resource output contracts, and
  validation expectations.

#### Candidate Work

- Design stdlib v2 with versioned docs and a fixed MVP before implementation.
- Required 0.8.0 helper MVP:
  - basic text components with obvious generated JSON
  - scoreboard objective creation and score operations
  - literal selector aliases and common entity filters
  - literal storage/NBT path read/write helpers
  - load/tick registration helpers only when source explicitly opts in
- Stretch helpers, cut unless the MVP is complete and tested:
  - simple schedules
  - bossbar and team helpers
  - entity helper conveniences
  - scaled numeric read/write helpers
  - small coordinate/vector helpers
  - predicate helper conveniences
- Deferred helper families:
  - randomization helpers
  - item component helper taxonomy
  - automatic load/tick setup
  - schedule cancellation
  - broad vector math
  - registry-wide helper generation
  - helper behavior that requires hidden temp objectives, storage keys, or
    generated functions without explicit source opt-in
- Add carefully scoped compile-time conveniences:
  - literal constant folding already supported by Cobble should stay predictable
  - pure unrolling over literal arrays or explicit integer ranges may be
    considered after formatter and watch/link work is stable
  - dynamic Python/eval-style code generation remains out of scope
- Improve resource authoring:
  - better tag merging
  - resource path suggestions
  - typed JSON resource validation for supported kinds
  - deterministic resource ordering
- Keep data-pack resource scope to existing supported resource kinds unless a
  per-kind schema, ownership, merge, and snapshot contract is added.
- Treat resource-pack support as design-only for 0.8.0 unless the implementation
  is limited to:
  - pack metadata
  - assets namespace layout
  - no generated model or language JSON without separate acceptance tests

#### Acceptance Criteria

- Every helper has generated output tests.
- Helper output validates against the target command tree when it emits
  commands.
- Helper-generated resources appear in generated file-tree snapshots,
  source-map assertions where applicable, build manifests, and `inspect --json`
  output.
- Helper-generated names are deterministic and documented.
- Helpers do not create implicit load/tick tags, temp objectives, storage keys,
  or helper functions unless source explicitly opts in.
- Docs show both Cobble source and generated command/resource output.
- Stdlib additions do not require users to trust invisible runtime behavior.

#### Non-Goals

- No broad asset pipeline unless a separate design is accepted.
- No generated model or language JSON unless explicit resource-pack acceptance
  tests are added.
- No full `beet`-style pack SDK replacement.
- No helper that cannot explain its generated output.
- No remote dependency manager.
- No block-registry-wide code generation without a separate performance and
  Minecraft-version design.
- No arbitrary Python/eval or hidden script runtime.
- No multi-target Minecraft support unless the 1.0 target policy has already
  been designed.

#### Detailed 0.8.0 Plan

0.8.0 should be the main authoring-power release before the 1.0 compatibility
push. It may expand stdlib and resource ergonomics, but it should avoid changes
that make the language or generated output harder to explain.

##### Release Thesis

After 0.7.x workflow hardening, users should have a safer project workflow.
The next gap is authoring reach: common scoreboard, storage, selector, text,
schedule, NBT, and resource patterns should be easier to express without hiding
the commands and JSON that Cobble generates.

The release should therefore prioritize:

1. transparent stdlib helpers with generated-output tests,
2. resource declaration ergonomics for existing data-pack resource kinds,
3. command-validation-aware helper output where possible,
4. documentation that shows source plus generated functions and JSON,
5. compatibility notes for helper APIs and metadata that 1.0 can stabilize.

The release should avoid:

1. a broad asset pipeline,
2. a remote package ecosystem,
3. a generic plugin architecture,
4. arbitrary dynamic code execution,
5. Minecraft multi-target behavior before the target policy is designed.

##### Phase 0: Scope And Contract Design

Before implementation starts, decide which helper families and resource
ergonomics are actually in scope for 0.8.0.

Tasks:

- Write a stdlib v2 design note:
  - helper naming,
  - import style,
  - generated-output guarantees,
  - validation expectations,
  - compatibility expectations before 1.0.
- Classify helper candidates into required, stretch, and deferred before any
  helper implementation:
  - text components,
  - scoreboards,
  - storage,
  - schedules,
  - bossbars,
  - teams,
  - entities,
  - selectors,
  - NBT paths,
  - item components,
  - predicates,
  - randomization.
- Decide which resource declarations get better ergonomics in 0.8.0:
  - tags,
  - predicates,
  - advancements,
  - loot tables,
  - recipes,
  - item modifiers,
  - dialogs,
  - no newly supported resource kind unless a per-kind schema and merge contract
    is accepted.
- Define the resource output contract:
  - deterministic ordering,
  - duplicate handling,
  - merge behavior,
  - path normalization,
  - diagnostics for invalid names or conflicting declarations.
- Decide whether scoped resource-pack support is design-only or implementation:
  - pack metadata is acceptable,
  - assets namespace layout is acceptable,
  - generated model or language JSON is deferred by default,
  - textures, binary assets, external processors, and full pipeline behavior are
    deferred.

Deliverables:

- A stdlib v2 design document.
- A resource authoring design document.
- Tests planned for every accepted helper family.
- Explicit defer list for helpers or resources that would make 0.8.0 too broad.
- A stdlib import/versioning decision before helper implementation.
- A Minecraft target policy decision before helper contracts are implemented.

##### Phase 1: Transparent Stdlib Helpers

Required behavior:

- Helpers must expand to understandable commands or resources.
- Helper docs must show at least one Cobble source example and generated output.
- Helper output that emits commands must validate against the current bundled
  command tree when possible.
- Helpers must avoid global hidden state unless it is explicit in source or
  generated output.
- Helper-generated file names, function names, storage keys, objective names,
  and tags must be deterministic and documented.
- Helper output must be visible through generated-output snapshots, source-map
  checks where applicable, build manifests, and `inspect --json`.

Required MVP helper clusters:

- Text components:
  - plain text,
  - translated text,
  - styled fragments only where JSON output stays obvious.
- Scoreboards:
  - objective creation,
  - player score operations,
  - reset/remove patterns.
- Storage and NBT:
  - read/write helpers for storage paths,
  - entity/block path helpers for literal paths only,
  - clear diagnostics for invalid literal paths.
- Selectors:
  - reusable selector aliases,
  - common entity filters,
  - safer literal interpolation,
  - no opaque selector builder runtime.
- Events:
  - load/tick registration helpers only when source explicitly opts in.

Stretch helper clusters:

- simple delayed function scheduling,
- bossbar and team helpers,
- entity helper conveniences,
- scaled numeric read/write helpers,
- small coordinate/vector helpers,
- predicate helper conveniences.

Deferred helper clusters:

- randomization helpers,
- item component helper taxonomy,
- automatic load/tick setup,
- schedule cancellation,
- broad vector math,
- registry-wide generated helper APIs,
- helpers that require hidden temp objectives, storage keys, or generated
  functions without explicit source opt-in.

Tests:

- Unit tests for helper expansion.
- Snapshot tests for the full generated file tree.
- Snapshot tests for generated `.mcfunction` and JSON.
- Source-map assertions for helper-created functions or resources where
  applicable.
- Manifest and `inspect --json` assertions for helper-created output.
- Validation tests for commands emitted by helpers.
- Regression tests that preserve clear source spans in diagnostics.

Exit criteria:

- Every shipped helper has docs, tests, and generated-output snapshots.
- Generated output remains readable without consulting Cobble internals.
- Helper failures point to source locations users can act on.

##### Phase 2: Resource Authoring Ergonomics

Required behavior:

- Existing resource declarations should get clearer diagnostics and more
  deterministic output.
- Supported resource kinds should have consistent path and namespace handling.
- Each resource kind must be classified as Cobble-owned typed structure or
  pass-through JSON before implementation.
- Each resource kind must document schema source, validation failure behavior,
  merge behavior, and snapshot requirements.
- Duplicate-resource diagnostics should distinguish exact duplicate,
  merge-compatible duplicate, and invalid overwrite cases.
- Tag merging should stay deterministic and visible in generated output.

Candidate improvements:

- Resource path suggestions for likely namespace/path mistakes.
- Typed validation for supported JSON resource kinds where Cobble owns the
  structure.
- Better generated-output snapshots for tags, predicates, loot tables, recipes,
  item modifiers, dialogs, and advancements.
- Resource docs that pair Cobble source with generated JSON.

Exit criteria:

- Resource output ordering is deterministic.
- Duplicate and merge diagnostics include source locations where available.
- Every changed resource kind has generated JSON snapshots.
- Typed resource validation has positive and negative tests.
- Pass-through JSON resources are not silently rewritten.
- New resource docs are backed by examples that build and validate when
  validation data is available.

##### Phase 3: Scoped Compile-Time Conveniences

0.8.0 may add small compile-time conveniences only when their behavior is easy
to reason about.

Allowed candidates:

- literal constant folding that preserves existing semantics,
- unrolling over literal arrays,
- unrolling over explicit integer ranges.

Deferred candidates:

- static data import unless JSON schema, project-root access, manifest, web, and
  expansion-limit criteria are written,
- arbitrary Python/eval,
- dynamic filesystem imports without schema validation,
- template functions that obscure generated output,
- registry-wide code generation without performance and versioning design.

Exit criteria:

- Compile-time helpers are deterministic.
- Generated output snapshots are stable.
- Error messages explain which literal or static input failed.
- Expansion limits are documented and tested.
- Every unrolled generated command or resource has source-span mapping.
- Output ordering is deterministic.
- Web compiler behavior is specified; unsupported compile-time filesystem
  access must fail clearly in the browser.
- Any static input that affects output appears in the build manifest.

##### Phase 4: Examples, Docs, And Web Parity

Required work:

- Add `examples/stdlib_v2` as the required stdlib-heavy release fixture.
- Add `examples/resource_authoring` as the required resource-heavy release
  fixture.
- Update `/try` samples if new helpers are important to show.
- Update README, language reference, stdlib docs, resource docs, and QA docs.
- Keep examples small enough that source and generated output can be reviewed.

Exit criteria:

- Example projects build and validate when command data is available.
- Docs show source plus generated functions/resources for all major additions.
- Web demo and CLI output agree for supported single-file examples.

##### Phase 5: Release Candidate And QA

Before the first 0.8.0 release candidate, run the cross-cutting matrix plus
focused stdlib/resource checks:

```bash
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
scripts/check_examples.sh
cargo run --locked -- build examples/26_smoke --validate -o /tmp/cobble-qa-26-smoke
cargo run --locked -- build examples/26_feature_matrix --validate -o /tmp/cobble-qa-26-feature-matrix
cargo run --locked -- build examples/inventory.cbl --validate -o /tmp/cobble-qa-inventory
cargo run --locked -- build examples/stdlib_v2 --validate -o /tmp/cobble-qa-stdlib-v2
cargo run --locked -- build examples/resource_authoring --validate -o /tmp/cobble-qa-resource-authoring
cargo run --locked -- inspect /tmp/cobble-qa-stdlib-v2 --json
cargo run --locked -- inspect /tmp/cobble-qa-resource-authoring --json
scripts/check_resource_snapshots.sh
scripts/check_resource_schemas.sh
cargo package --locked
cargo publish --dry-run --locked
```

Cut from 0.8.0 if implementation would require:

- unstable helper semantics,
- hidden runtime state,
- plugin or package architecture,
- full resource-pack asset processing,
- multi-target Minecraft behavior,
- arbitrary dynamic code execution.

### 1.0.0: Stable Cobble

Theme: make Cobble stable enough for projects that expect compatibility.

#### Goals

- Define SemVer policy for language, CLI, metadata, stdlib, and generated
  output.
- Lock a stable core language.
- Lock stable project configuration.
- Lock stable metadata contracts for source maps and build manifests.
- Establish release gates that are strong enough for long-term use.

#### Candidate Work

- Publish compatibility policy:
  - source compatibility
  - generated output compatibility
  - Minecraft target policy
  - stdlib versioning
  - metadata schema versioning
  - web demo compatibility
- Decide Minecraft versioning model:
  - one active target at a time
  - multiple command-tree targets
  - target aliases such as `latest`
  - pack-format compatibility behavior
- Add command-tree version management:
  - generated tree fingerprints
  - source of truth for bundled command data
  - stale-cache diagnostics
  - upgrade path when Minecraft releases change command syntax
- Make release QA mandatory:
  - Rust test suite
  - clippy
  - example validation
  - package dry-run
  - install smoke
  - web build and live smoke when web changed
  - real-server smoke for major releases when EULA acceptance is available
- Write migration docs from the last 0.x release to 1.0.

#### Acceptance Criteria

- Users can rely on documented language behavior across patch releases.
- Metadata consumers have a schema version and compatibility rules.
- Release notes clearly separate breaking and non-breaking changes.
- The website, README, docs, crates.io, and GitHub release all agree.

#### Non-Goals

- No broad new language surface unless needed to close a compatibility hole.
- No plugin API as a release blocker.
- No package manager as a release blocker.
- No full resource-pack pipeline as a release blocker.
- No arbitrary Python/eval.
- No unsupported promise of compatibility across Minecraft versions that Cobble
  cannot validate.

#### Detailed 1.0.0 Plan

1.0.0 should turn Cobble from a useful stable 0.x tool into a project users can
depend on for longer-lived data packs.

##### Release Thesis

0.7.0 is already a stable release. 1.0.0 means something narrower and stricter:
Cobble should publish a compatibility policy and then follow it. The release is
successful when users know which changes can happen in patch releases, which
changes require minor releases, and which changes would be breaking.

##### Phase 0: Compatibility Audit

Audit all public surfaces:

- `.cbl` syntax and semantics,
- stdlib imports and helper output,
- CLI commands and exit codes,
- `cobble.toml` config,
- generated data-pack layout,
- `.cobble/build_manifest.json`,
- `.cobble/source_map.json`,
- `check --json`,
- `doctor --json`,
- link-state and ownership-marker metadata,
- diagnostic codes, related ranges, symbol ids, target context, and
  generated-resource mappings needed by future editor tooling,
- web compiler behavior.

Deliverables:

- A compatibility policy document.
- A metadata schema/versioning document.
- An ecosystem extension preflight document.
- A migration note from the last 0.x release.
- A list of any behavior that remains explicitly experimental.

##### Phase 0.5: Ecosystem Extension Preflight

Before freezing 1.0 metadata and config contracts, reserve enough structure for
future 1.x ecosystem work without promising those features yet.

Required decisions:

- reserved metadata namespaces for future plugins, packages, resource-pack
  processors, and editor tooling,
- config extension policy for unknown future fields,
- source-map and build-manifest forward-compat rules,
- stable vs experimental metadata field labeling,
- target pinning in `cobble.toml`,
- `latest` target alias semantics if aliases are allowed,
- per-target diagnostics and helper compatibility expectations,
- rule that packages, plugins, static imports, and resource processors cannot
  make builds nondeterministic without explicit opt-in and visible generated
  output.

Exit criteria:

- 1.0 can lock core metadata without blocking later plugin/package/resource-pack
  work.
- Future extension fields can be added in minor releases without breaking
  documented 1.0 consumers.

##### Phase 1: Spec And Docs Freeze

Required work:

- Update the language reference so supported Python-like behavior is explicit.
- Document unsupported Python-looking behavior clearly.
- Document generated output expectations and known non-guarantees.
- Document stdlib helper compatibility rules.
- Document Minecraft target policy and pack-format behavior.
- Ensure README, website, crates.io, and GitHub release notes use the same
  terminology.

Exit criteria:

- A user can decide whether an existing project should expect source changes
  when upgrading from the last 0.x release to 1.0.0.
- Experimental fields and commands are labeled consistently.

##### Phase 2: Metadata And CLI Contracts

Required work:

- Add schema version fields where missing.
- Add stable tests for JSON output shapes.
- Define exit-code behavior for common success and failure cases.
- Lock source-map and build-manifest compatibility expectations.
- Define how new fields may be added in future minor releases.

Exit criteria:

- Editor or external-tool consumers can rely on documented stable fields.
- JSON output tests fail on accidental breaking changes.

##### Phase 3: Minecraft Target Policy

Required work:

- Decide whether 1.0 supports one active Minecraft target or multiple targets.
- If one active target is chosen, document how Cobble handles future Minecraft
  releases.
- If multiple targets are chosen, require command-tree tests and pack-format
  behavior before 1.0 ships.
- Add stale command-data diagnostics and generated command-tree fingerprints.

Exit criteria:

- The bundled target version, data-pack format, command tree, and docs agree.
- Users get clear diagnostics when validation data is missing or stale.

##### Phase 4: Release Gates

Before the first 1.0.0 release candidate:

```bash
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo run --locked -- --version
scripts/check_examples.sh
cargo run --locked -- check --json examples/26_smoke/src/main.cbl
cargo run --locked -- build examples/26_smoke --validate -o /tmp/cobble-qa-26-smoke
cargo run --locked -- build examples/26_feature_matrix --validate -o /tmp/cobble-qa-26-feature-matrix
cargo run --locked -- build examples/inventory.cbl --validate -o /tmp/cobble-qa-inventory
cargo run --locked -- doctor
cargo run --locked -- doctor --json
cargo run --locked -- inspect /tmp/cobble-qa-26-smoke
cargo run --locked -- inspect /tmp/cobble-qa-26-smoke --json
cargo install --locked --path . --root /tmp/cobble-qa-install
/tmp/cobble-qa-install/bin/cobble --version
/tmp/cobble-qa-install/bin/cobble check examples/26_smoke/src/main.cbl
cargo package --locked
cargo publish --dry-run --locked
```

If web build inputs changed, run the full web test and build matrix. For 1.0.0,
real-server smoke should be strongly preferred when EULA acceptance is
available.

## Future 1.x And Later Plans

These are intentionally deferred until the core language, workflow, and 1.0
compatibility story are stable.

### 1.1.0: Minecraft Target Substrate

- Multi-target Minecraft support if command-tree and pack-format versioning are
  ready.
- Target aliases such as `latest` only if they have deterministic behavior.
- Target pinning in `cobble.toml`.
- Per-target diagnostics.
- Generated output layout rules for target-specific builds.
- Helper compatibility expectations across target versions.
- Better stale-command-data diagnostics.

### 1.1.x: Editor Integration Beta

- LSP or editor extension beta built on stable `check --json`, source maps,
  formatter behavior, and document-symbol metadata.
- Diagnostic codes, related ranges, symbol ids, target context, and
  generated-resource mappings should be available before broad editor support.
- Minecraft log tailing for linked worlds if `watch --link` is stable.

### 1.2.x: Plugin And Pack-Tooling Interop

- Stable generated-pack metadata for external tools.
- Inspect-only metadata API first.
- External command hooks second, with deterministic inputs and outputs.
- Mutating build plugins only after a capability, trust, versioning, and
  rollback design.
- Interop with `beet` or other pack tooling where it does not make normal Cobble
  usage depend on a Python runtime.
- Extension hooks should be explicit, versioned, and testable.
- Plugins should not be required for normal single-project Cobble usage.
- Plugins must not alter source, generated files, validation, or packaging
  without explicit opt-in and visible generated output.

### 1.3.x: Package And Dependency Model

- Decide package taxonomy before implementation:
  - Cobble source library,
  - stdlib module,
  - helper package,
  - plugin,
  - resource asset package.
- Package manager or dependency model for Cobble libraries.
- Lockfile design if remote dependencies are accepted.
- Namespace conflict diagnostics.
- Version compatibility rules for stdlib and third-party helpers.
- Offline and reproducible-build expectations before publishing any package
  ecosystem promise.

### 1.4.x: Resource-Pack Pipeline And Rich Assets

- Rich resource-pack authoring after data-pack resources are stable.
- Asset namespace layout, pack metadata, language files, generated model JSON,
  and generated item-model helpers.
- Ownership, merge order, conflict diagnostics, and binary hashing rules.
- Processor trust model and execution model before external processors ship.
- Binary assets, texture processing, model generation, and external processors
  only after a separate pipeline design.
- Clear separation between Cobble-owned generated assets and user-authored
  static assets.

### Later Or Explicitly Deferred

- Advanced compile-time macros and template functions.
- Generated documentation for Cobble projects.
- Larger example gallery and template catalog.
- Schema-aware data import for JSON, YAML, or CSV fixtures.
- Arbitrary Python/eval remains not planned; if data-driven generation is
  needed, prefer schema-aware static imports and explicit generated output.

## Cross-Cutting QA Matrix

Use this baseline matrix before release commits. Add focused tests when a
release touches a specific subsystem.

```bash
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo run --locked -- --version
scripts/check_examples.sh
cargo run --locked -- check --json examples/26_smoke/src/main.cbl
cargo run --locked -- build examples/26_smoke --validate -o /tmp/cobble-qa-26-smoke
cargo run --locked -- build examples/26_feature_matrix --validate -o /tmp/cobble-qa-26-feature-matrix
cargo run --locked -- build examples/inventory.cbl --validate -o /tmp/cobble-qa-inventory
cargo run --locked -- doctor
cargo run --locked -- build examples/26_smoke --dry-run --validate
cargo run --locked -- inspect /tmp/cobble-qa-26-smoke
cargo run --locked -- inspect /tmp/cobble-qa-26-smoke --json
cargo package --locked
cargo publish --dry-run --locked
```

`scripts/check_examples.sh` checks each example as an independent source entry.
Checking the entire `examples/` directory as one project should reject duplicate
function names across unrelated examples.

If web build inputs changed:

```bash
cd web
npm run test:wasm
npm run test:zip
npm run lint
npm run build:github
npm run test:e2e:run
npm run test:links
```

Optional real-server smoke:

```bash
COBBLE_MINECRAFT_EULA_ACCEPTED=1 scripts/test_minecraft_server.sh
```

When 0.7.1 tooling lands, add focused checks:

```bash
cargo run --locked -- fmt --check examples
cargo run --locked -- init --name /tmp/cobble-qa-init-resource --template resource-heavy
cargo run --locked -- build /tmp/cobble-qa-init-resource --validate -o /tmp/cobble-qa-init-resource-output
cargo run --locked -- init --list-templates
mkdir -p /tmp/cobble-qa-world/datapacks
cargo run --locked -- link /tmp/cobble-qa-init-resource --dry-run --datapacks /tmp/cobble-qa-world/datapacks --pack-name qa_resource
cargo run --locked -- link /tmp/cobble-qa-init-resource --datapacks /tmp/cobble-qa-world/datapacks --pack-name qa_resource
cargo run --locked -- link /tmp/cobble-qa-init-resource --status
cargo run --locked -- build /tmp/cobble-qa-init-resource -o /tmp/cobble-qa-world/datapacks/qa_resource
cargo run --locked -- clean /tmp/cobble-qa-init-resource --linked --dry-run
cargo run --locked -- doctor --json
scripts/qa_07_templates.sh
scripts/qa_07_link_clean_safety.sh
scripts/qa_07_watch_smoke.sh
```

Default QA must use temporary directories for `link` and must not write into a
real Minecraft save unless an explicit real-world QA flag or manual step is
used.

## Decision Backlog

These decisions should be made deliberately before implementation starts.

- Should Cobble support multiple Minecraft versions at once, or only one
  current target per release line?
- Before 0.8.0 helpers ship, what is the Minecraft target policy for helper
  compatibility and generated output?
- Should Cobble expose a plugin API, or only stable generated metadata for
  external tools?
- What metadata/config extension namespaces must be reserved before 1.0?
- Should `cobble link` persist link state only in `.cobble/` metadata, or also
  in `cobble.toml` profiles?
- Should linked-world workflows tail Minecraft logs, and should that be part of
  `watch --link` or a separate command?
- Should `cobble clean` require an explicit confirmation flag for all writes, or
  only for linked-world cleanup?
- Should workflow profiles ship in 0.7.3 or later 0.7.x after `fmt`, `watch`,
  `link`, and `clean` stabilize?
- Should resource-pack support live in Cobble core or a separate tool?
- Should stdlib versions be tied to Cobble versions or importable as explicit
  modules?
- What is the package taxonomy for source libraries, stdlib modules, helper
  packages, plugins, and resource asset packages?
- Should selector/NBT/vector ergonomics be new language syntax, stdlib helpers,
  or both?
- Should compile-time data import start with JSON only, or include YAML/CSV once
  schema validation is designed?
- Should the web compiler support complete project folders or remain focused on
  single-file demos plus generated virtual files?

## Current Priority Order

1. For 0.7.1, design and implement `cobble fmt --check` with deterministic
   formatting that preserves raw Minecraft commands and inline JSON.
2. For 0.7.1, harden `cobble watch` with debounce, output ignores, config
   reload, and previous-output preservation.
3. For 0.7.1, implement safe `cobble link` with explicit paths, dry-run, clear,
   marker checks, and `watch --link` integration.
4. For 0.7.1, add workflow safety/status commands: `clean`, `link --status`,
   `doctor --json`, and `init --list-templates`.
5. For 0.7.1, expand `cobble init` templates and add at least one realistic
   example data pack with generated-output snapshots.
6. For 0.7.1, design or prototype experimental editor metadata through
   existing parser, diagnostics, manifest, and source-map paths.
7. For 0.7.1, keep website and `/try` output aligned with CLI behavior through
   PR-gated web checks.
8. For 0.7.3, stabilize the 0.7.1 workflow surface based on real QA, issue
   reports, and manual template/link usage.
9. For 0.7.3, finalize 0.8.0 helper/resource scope, stdlib versioning, and
   Minecraft target policy before implementation starts.
10. For 0.8.0, design and implement the required stdlib v2 MVP and
    resource-authoring ergonomics
    with source-to-generated-output tests.
11. For 0.8.0, keep scoped resource-pack work limited to metadata, namespace
    layout, and generated JSON unless a separate pipeline design is accepted.
12. For 1.0.0, freeze compatibility policy, metadata schemas, CLI contracts,
    Minecraft target policy, docs, and release gates.
13. Defer plugin APIs, package management, full resource-pack pipelines,
    arbitrary Python/eval, and broad multi-target behavior until after the
    relevant 1.x design work exists.
