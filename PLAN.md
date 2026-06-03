# Cobble Roadmap

## Status

- Planning date: 2026-06-02
- Current stable release: `0.6.3`
- Active release candidate: `0.7.0-rc.1`
- Current Minecraft target: Java Edition `26.1.2`
- Current data pack format: `101.1`
- Package name: `cobble-lang`
- CLI command: `cobble`
- Website: <https://deveworld.github.io/cobble/>
- Browser compiler: <https://deveworld.github.io/cobble/try/>

This roadmap records the completed 0.6.3 stabilization plan and the active
0.7.0 language-specification and diagnostics plan. Historical release details
belong in `CHANGELOG.md`; this file tracks what Cobble should become after
0.6.3.

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

The following projects were reviewed as comparable or adjacent tools:

- `beet` and `bolt`: strongest reference for pack APIs, plugin pipelines,
  Python-like language semantics, diagnostics, and ecosystem maturity.
- `jmc`: strongest reference for high-level language ergonomics, exact output
  testing, pack-version feature gates, and built-in command helpers.
- `cbscript`: strongest reference for real-world data pack authoring features,
  compile-time helpers, selector/vector/NBT conveniences, and large examples.
- `MC-Language`: useful reference for simple package, event, and macro syntax.
- `CakeLang`: useful mostly as an early module/static-analysis concept.

Cobble should borrow engineering lessons, not copy the whole shape of any one
project. The best strategic position is:

- More current and easier to install than older transpilers.
- More compiler-focused and standalone than `beet`.
- Simpler and more transparent than a broad scripting runtime.
- Better validated against current Minecraft than most small language projects.

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

### 0.6.3: Stabilization And Test Depth

Theme: make the 0.6.x line more trustworthy before adding larger language
surface.

#### Goals

- Improve regression coverage around generated output.
- Convert recent release QA procedures into repeatable tests where practical.
- Harden command-tree cache behavior and diagnostics.
- Keep public behavior compatible with 0.6.2.

#### Candidate Work

- Add exact output-tree snapshot tests for representative fixtures:
  - `examples/26_smoke`
  - `examples/26_feature_matrix`
  - `examples/inventory.cbl`
  - at least one stdlib-heavy project
  - at least one explicit resource-heavy project
- Add CLI output tests for stable behavior:
  - `cobble doctor`
  - `cobble build --dry-run`
  - `cobble inspect`
  - validation failure output
- Add command-tree cache tests:
  - missing default `data/commands.json`
  - stale default `data/commands.json`
  - custom `--commands-json`
  - local server jar override
  - environment URL override with a fixture server when practical
- Improve duplicate-resource diagnostics:
  - show both declaration locations when source spans are available
  - distinguish exact duplicate, invalid overwrite, and invalid tag duplicate
    cases
  - keep function-tag merging deterministic
- Add web WASM E2E coverage:
  - compile default sample
  - inspect generated function output
  - inspect generated resources
  - verify ZIP output contains merged tag files
- Add lightweight docs and link checks for README, docs, website routes, and
  release pages.
- Keep the ignored real-server smoke documented and easy to run.

#### Acceptance Criteria

- `cargo fmt --check` passes.
- `cargo test --locked` passes.
- `cargo clippy --locked --all-targets -- -D warnings` passes.
- `scripts/check_examples.sh` passes.
- `cargo run --locked -- build examples/26_smoke --validate` passes.
- `cargo run --locked -- build examples/26_feature_matrix --validate` passes.
- `cargo package --locked` passes before publishing.
- If web build inputs changed, `npm run lint` and `npm run build:github` pass
  in `web/`.
- If web compiler, ZIP, routes, or docs changed, `npm run test:wasm`,
  `npm run test:zip`, `npm run test:e2e`, and `npm run test:links` pass in
  `web/`.
- Rust tests and package dry-run are wired into GitHub Actions.
- Any skipped real-server QA is recorded in release notes.

#### Non-Goals

- No new parser fundamentals.
- No new Minecraft target version.
- No plugin system.
- No package manager.

#### Detailed 0.6.3 Plan

0.6.3 should be treated as a stabilization release, not a feature release. The
main deliverable is confidence: users should be able to trust that generated
files, CLI summaries, validation behavior, and the web compiler stay stable
across small changes.

##### 1. Snapshot Testing For Generated Packs

The highest-priority work is exact output testing. Cobble already validates many
behaviors through unit and integration tests, but 0.6.3 should make generated
data pack trees harder to accidentally change.

Tasks:

- Add a snapshot fixture helper that:
  - builds a fixture into a temporary output directory,
  - normalizes unstable paths,
  - reads generated `pack.mcmeta`, `data/**`, and `.cobble/**`,
  - compares the result to committed snapshots.
- Start with focused fixture snapshots:
  - `examples/26_smoke`
  - `examples/26_feature_matrix`
  - `examples/inventory.cbl`
  - one small resource-only fixture
  - one fixture that merges stdlib event tags with explicit function tags
- Snapshot generated metadata separately:
  - `.cobble/source_map.json`
  - `.cobble/build_manifest.json`
  - validation summary when `--validate` is used
- Normalize fields that should not be path-dependent.
- Keep snapshots reviewable; avoid one huge combined file if per-file snapshots
  are easier to inspect.

Acceptance criteria:

- Snapshot tests fail when generated functions, tags, resources, source maps, or
  build manifests change unexpectedly.
- Snapshot update flow is documented.
- Existing examples continue to build and validate.

##### 2. CLI Output Regression Tests

The 0.6.2 release added useful CLI commands. 0.6.3 should lock down the stable
parts of their output so future changes do not quietly degrade workflow UX.

Tasks:

- Add tests for `cobble doctor`:
  - no project config,
  - valid project config,
  - missing command tree,
  - valid command tree fingerprint when fixture data is available.
- Add tests for `cobble build --dry-run`:
  - does not leave final output behind,
  - prints generated counts,
  - works with `--validate`,
  - rejects incompatible options such as `--zip`.
- Add tests for `cobble inspect`:
  - human output,
  - `--json` output,
  - missing manifest error,
  - malformed manifest error.
- Add tests for validation diagnostics:
  - invalid command path,
  - caret context,
  - generated command source-map context when available.

Acceptance criteria:

- CLI tests assert stable semantic content, not fragile whitespace.
- `--quiet` remains script-friendly.
- User-facing error messages stay actionable.

##### 3. Command-Tree Cache And Download Safety

0.6.2 made command-tree generation more reliable. 0.6.3 should make that path
easier to test without depending on live Mojang endpoints in the normal test
suite.

Tasks:

- Add a local fixture path for command-tree generation tests where possible.
- Test default command-tree behavior:
  - missing default tree,
  - stale default tree,
  - correct default tree,
  - custom `--commands-json` bypassing the bundled fingerprint requirement.
- Test environment overrides:
  - `COBBLE_COMMANDS_JSON_URL` through a local fixture HTTP server if practical,
  - `COBBLE_MINECRAFT_SERVER_JAR` using a cached or fixture jar only if safe,
  - failure messages when Java or `curl` is unavailable.
- Keep live network tests outside the default test suite.
- Document manual E2E command-tree generation QA for release candidates.

Acceptance criteria:

- Default tests do not require network access.
- Stale bundled command data fails loudly.
- Custom command-tree paths remain supported.
- Manual live E2E command-tree QA has a clear command sequence.

##### 4. Resource Conflict Diagnostics

Cobble now supports more generated JSON resources. 0.6.3 should improve the
quality of errors around duplicate or conflicting resources before larger
stdlib/resource work begins.

Tasks:

- Audit all resource insertion paths:
  - function tags,
  - block/item/entity tags,
  - predicates,
  - advancements,
  - loot tables,
  - recipes,
  - item modifiers,
  - dialogs.
- Classify conflicts:
  - valid merge,
  - exact duplicate,
  - invalid overwrite,
  - invalid duplicate tag declaration,
  - same path from different source declarations.
- Add source-span propagation where currently missing.
- Show both declaration locations when available.
- Add tests for duplicate resources across imported files.
- Ensure web WASM output uses the same conflict behavior as the CLI.

Acceptance criteria:

- Function-tag merging remains deterministic.
- Invalid duplicate resources produce clear diagnostics.
- Generated manifest resource counts remain correct after merges.
- Browser ZIP output and CLI output agree for merged resources.

##### 5. Web Compiler Parity Tests

The browser compiler is now a first-class user surface. 0.6.3 should test it as
part of the product instead of relying only on manual inspection.

Tasks:

- Add a web E2E smoke test for `/try`:
  - loads the WASM bundle,
  - compiles the default sample,
  - shows generated functions,
  - shows generated resources,
  - shows build manifest/source map output.
- Add a direct WASM test where practical:
  - default sample compiles,
  - duplicate tag paths merge,
  - generated manifest version matches root `cobble-lang`,
  - diagnostics return structured errors.
- Verify ZIP output:
  - includes `pack.mcmeta`,
  - includes `data/**`,
  - excludes internal-only files unless intentionally exposed,
  - does not duplicate merged tag paths.
- Keep mobile/desktop screenshot QA manual unless the test can be stable.

Acceptance criteria:

- `npm run lint` passes.
- `npm run build:github` passes.
- Browser smoke confirms non-empty generated output.
- Web and CLI generated output are equivalent for selected samples.

##### 6. Documentation And Release Hygiene

0.6.3 should reduce ambiguity around release procedures and docs maintenance.

Tasks:

- Add or update a release checklist document if `PLAN.md` becomes too crowded.
- Keep `CHANGELOG.md` as the source of historical release facts.
- Keep `PLAN.md` focused on future work.
- Add link checks for:
  - README,
  - docs,
  - website routes,
  - GitHub release links where practical.
- Clarify optional real-server QA:
  - EULA requirement,
  - Java requirement,
  - network/cache behavior,
  - what counts as a pass.
- Finalize release QA notes before release.

Acceptance criteria:

- Docs do not describe unreleased work as already shipped.
- README, website, docs, crates.io metadata, and release notes agree.
- Release procedure is repeatable without relying on chat history.

##### 7. Suggested Implementation Order

1. Add snapshot test harness and one small fixture.
2. Add snapshots for the main 26.1.2 examples.
3. Add resource conflict diagnostics and duplicate-resource tests.
4. Add CLI output regression tests.
5. Add command-tree cache tests with local fixtures.
6. Add web WASM parity tests.
7. Update docs and release checklist.
8. Run the full release QA matrix.
9. Decide whether remaining work belongs in 0.6.3 or moves to 0.7.0.

##### 8. 0.6.3 Release Gate

Required:

```bash
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo run --locked -- --version
scripts/check_examples.sh
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

Required if web build inputs changed:

```bash
cd web
npm run test:wasm
npm run test:zip
npm run lint
npm run build:github
npm run test:e2e:run
npm run test:links
```

Required after publishing to crates.io:

```bash
cargo install cobble-lang --version 0.6.3 --locked --root /tmp/cobble-install-qa --force
/tmp/cobble-install-qa/bin/cobble --version
```

Optional but preferred before release:

```bash
COBBLE_MINECRAFT_EULA_ACCEPTED=1 scripts/test_minecraft_server.sh
```

##### 9. Deferral Rules

Move work to 0.7.0 if it requires:

- parser grammar changes,
- new language syntax,
- a new Minecraft target version,
- broad stdlib redesign,
- plugin architecture,
- package/dependency management,
- editor protocol design beyond small diagnostics experiments.

### 0.7.0: Language Spec And Diagnostics

Theme: make Cobble's language rules explicit and improve compiler feedback.

#### Goals

- Publish a usable language specification.
- Separate parsing, semantic analysis, and transpilation responsibilities more
  clearly where this improves correctness.
- Make unsupported Python-like constructs fail with precise diagnostics.
- Improve source spans across imports and generated commands.

#### Candidate Work

- Rewrite `docs/language.md` as a spec rather than only a feature tour:
  - lexical rules
  - indentation rules
  - expression support
  - statement support
  - imports and module resolution
  - macro parameters and command interpolation
  - resource declarations
  - unsupported constructs
- Add a semantic analysis pass for:
  - undefined variables
  - unsupported assignment shapes
  - function call limitations
  - type mismatches that can be caught before command generation
  - unreachable or no-op constructs when detectable
- Improve diagnostics:
  - consistent source snippets
  - import-chain context
  - generated-command context for validation failures
  - suggestions for common syntax mistakes
- Define module/import behavior:
  - project root resolution
  - relative imports
  - import cycles
  - duplicate symbols
  - stdlib import naming
- Stabilize metadata schema shape for:
  - `.cobble/source_map.json`
  - `.cobble/build_manifest.json`
  - future editor tooling
- Expand negative tests for unsupported language constructs.

#### Detailed 0.7.0 Plan

0.7.0 should clarify what Cobble is, not make it look more like Python by
accident. The release should draw a clean line between supported Cobble syntax,
compile-time conveniences, raw Minecraft command passthrough, and intentionally
unsupported Python-like constructs.

##### Implemented So Far In 0.7.0-rc.1

- Promoted the active 0.7.0 line to release candidate `0.7.0-rc.1`.
- Added `docs/language-support.md` as the implementation-facing language
  support matrix.
- Clarified `docs/language.md` around Cobble-vs-Python compatibility, raw
  command placeholders, unsupported constructs, and shared diagnostics.
- Added parser-level language surface regression tests for representative
  supported and unsupported constructs.
- Added a shared source diagnostic API for early language-surface errors.
- Wired the early diagnostic pass into `cobble check`, `cobble build`, and the
  web WASM compiler.
- Added regression coverage for unsupported Python-like constructs including
  default parameters, varargs, decorators, compound assignment, comprehensions,
  dotted/relative imports, import aliases, wildcard imports,
  comma-separated module imports, `break`, `continue`, `raise`, `assert`,
  `del`, `nonlocal`, `for ... else`, and non-identifier assignment targets.
- Added source-aware diagnostics for duplicate function parameter names.
- Added source-aware diagnostics for common missing block-colon,
  unexpected/inconsistent indentation, unclosed delimiter, unmatched delimiter,
  and unterminated string syntax mistakes before parser fallback.
- Added the first semantic preflight checks for duplicate function definitions,
  unsupported `return` statements, and non-math function calls used as
  assignment values.
- Added same-file user function call argument-count diagnostics, including
  forward calls, while leaving module/stdlib call validation on the existing
  transpiler paths.
- Added same-file user function argument diagnostics for nested function-call
  expressions such as `user_func(other_func())`.
- Added source-aware diagnostics for standalone expressions that would
  otherwise be no-ops.
- Added undefined-variable diagnostics for variable-dependent expressions such
  as assignment RHS values, control-flow conditions, and standalone
  helper/function call arguments.
- Added source-aware diagnostics for undefined, invalid, or unclosed raw
  command `{name}` placeholders while preserving JSON/NBT braces and `{{name}}`
  literals.
- Added source-aware type mismatch diagnostics for clearly inferred variable
  reassignments before transpilation.
- Added source-aware datapack helper argument diagnostics for non-object JSON
  resources and invalid tag value arrays in single-line and multi-line calls.
- Added source-aware datapack resource ID diagnostics for invalid literal names
  and tag values such as `minecraft/load`, uppercase paths, invalid namespaces,
  and invalid path separators.
- Added CLI import-tree preflight for missing imports, circular imports, and
  imported-file language diagnostics before transpilation.
- Added CLI import-tree diagnostics for `from module import item` names that do
  not exist in the imported file.
- Added import-wide function diagnostics for duplicate function definitions,
  duplicate selector/entity symbols, and calls to imported functions with the
  wrong argument count.
- Added multi-root CLI preflight so directory/project builds reject duplicate
  functions and duplicate named symbols across independent root files.
- Added browser compiler diagnostics for non-stdlib imports in single-file WASM
  compilation.
- Hardened semantic diagnostics to skip multi-line docstring bodies after
  parser-level docstring handling.
- Fixed CRLF-aware diagnostic byte offsets so rendered snippets stay aligned on
  Windows-authored files.
- Added source-line snippets and caret markers to file-level language diagnostic
  formatting used by build failures.
- Added `cobble check --json` for machine-readable file summaries and
  structured diagnostics while preserving non-zero exits on failed checks.
- Added metadata schema documentation and schema-shape regression coverage for
  `.cobble/build_manifest.json` and `.cobble/source_map.json`.
- Added structured WASM diagnostic details for the web compiler response while
  preserving legacy formatted diagnostic strings for compatibility.
- Added `/try` browser E2E coverage for invalid Cobble source and structured
  diagnostic rendering.
- Added a `/try` diagnostic sample that demonstrates raw command placeholder
  errors.

##### 1. Language Specification Audit

Tasks:

- Inventory the parser and current tests into a support matrix:
  - literals: numbers, strings, booleans, arrays, maps, SNBT-like objects,
  - expressions: arithmetic, comparisons, boolean operators, unary operators,
    power/modulo, indexing where supported,
  - statements: assignments, constants, globals, functions, imports,
    `if`/`elif`/`else`, `while`, `for`, `match`, raw commands,
  - command interpolation: `{param}` macros, selector aliases, JSON values,
  - compile-time declarations: `datapack.*`, stdlib imports, event listeners.
- Split `docs/language.md` into clear sections:
  - supported grammar,
  - semantic rules,
  - command interpolation rules,
  - resource declaration rules,
  - unsupported Python features,
  - examples with generated output where useful.
- Add a short compatibility note that Cobble is Python-like but not Python.

Acceptance criteria:

- Every documented language feature has at least one parser/transpiler test or
  example snapshot.
- Every explicitly unsupported feature has a documented reason or alternative.
- The language reference avoids vague terms such as "Python-style" without
  defining the exact supported behavior.

##### 2. Semantic Diagnostics Pass

Tasks:

- Add a lightweight semantic-analysis layer before transpilation where it can
  catch errors without changing generated output.
- Start with high-signal checks:
  - undefined variables in expressions,
  - assignment to unsupported targets,
  - unsupported function-call positions,
  - duplicate function names in one module,
  - unsupported `return` values,
  - type mismatches already inferred later in transpilation,
  - non-object JSON passed to resource helpers,
  - impossible or no-op constructs when detection is straightforward.
- Preserve existing transpiler behavior while moving diagnostic ownership to the
  semantic pass gradually.
- Make diagnostics source-aware across imported files.

Acceptance criteria:

- Semantic errors produce source file, line, column, and actionable message.
- Existing successful examples compile to the same generated output unless an
  intentional diagnostic catches a previously accepted bug.
- Negative tests cover each new semantic diagnostic.

##### 3. Diagnostic Rendering And Import Context

Tasks:

- Standardize error message shape across parser, semantic, transpiler, and
  validator failures.
- Include import-chain context when an imported file fails.
- Improve suggestions for common mistakes:
  - using `minecraft/load` instead of `minecraft:load`,
  - uppercase resource paths,
  - unsupported Python syntax,
  - accidental string/scoreboard type mismatch,
  - invalid function macro placeholders.
- Keep generated-command validation diagnostics tied to source-map entries.
- Make `--quiet` and JSON-oriented workflows remain script-friendly.

Acceptance criteria:

- CLI regression tests cover representative diagnostics from each compiler
  stage.
- Error messages are stable enough for users to understand, but tests avoid
  asserting brittle decorative formatting.
- Import failures identify both importing file and failed target.

##### 4. Parser And AST Boundary Cleanup

Tasks:

- Document parser ownership for syntax only; do not let parser behavior imply
  semantic support by accident.
- Review AST variants for unsupported or ambiguous constructs.
- Add helper APIs for span/source ownership so semantic checks and transpiler
  diagnostics do not duplicate line lookup logic.
- Keep parser grammar changes minimal unless a diagnostic requires it.

Acceptance criteria:

- Parser tests describe syntax acceptance separately from semantic acceptance.
- Source span collection is shared by parser/semantic/transpiler paths where
  practical.
- No broad grammar refactor lands without snapshot and integration coverage.

##### 5. Metadata Schema Stabilization

Tasks:

- Document `.cobble/source_map.json` schema:
  - version,
  - generated path,
  - generated line,
  - command text,
  - source location,
  - generated command kind.
- Document `.cobble/build_manifest.json` schema:
  - package/Minecraft/pack-format metadata,
  - source input metadata,
  - generated counts,
  - resource entries,
  - validation summary.
- Add schema-focused snapshot or JSON-shape tests that tolerate count changes
  where appropriate but catch accidental field removal.
- Decide which fields are stable for 0.x and which remain experimental.

Acceptance criteria:

- Docs and snapshots agree on metadata fields.
- `cobble inspect --json` remains compatible with documented manifest fields.
- Any schema version bump has a migration note.

##### 6. Web And Docs Parity

Tasks:

- Keep `/try` examples aligned with the 0.7.0 language reference.
- Add at least one web sample that demonstrates a new or clarified diagnostic.
- Ensure WASM structured diagnostics expose enough data for the web UI to show
  file/line context.
- Keep web build/test gates in sync with compiler changes.

Acceptance criteria:

- `npm run test:web` passes after language/diagnostic changes.
- The web compiler and CLI agree on accepted source and key diagnostics for
  selected fixtures.
- The web response exposes both formatted diagnostic strings and structured
  file/line/column/kind/message/help details.
- README links remain small and current.

##### 7. Suggested Implementation Order

1. Add a language support matrix test/documentation inventory.
2. Rewrite `docs/language.md` around supported syntax and semantics.
3. Add negative fixtures for unsupported Python-like constructs.
4. Introduce the first semantic-analysis checks with source-aware diagnostics.
5. Move duplicate/simple semantic errors out of transpiler paths where safe.
6. Standardize diagnostic rendering and import-chain context.
7. Document source-map and build-manifest schemas.
8. Add web parity tests for structured diagnostics.
9. Run full Rust, web, command-tree, package, and optional server gates.

##### 8. 0.7.0 Release Gate

Required:

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

Use `scripts/check_examples.sh` instead of `cobble check examples` because the
example gallery contains unrelated projects that intentionally reuse names such
as `init`, `tick`, and `hello`.
Run `cargo package --locked` and `cargo publish --dry-run --locked` from a clean
working tree for the final release. During alpha development, use
`--allow-dirty` only to verify packaging before the release commit exists.

Required if language docs, WASM compiler, web examples, or routes changed:

```bash
cd web
npm run test:web
```

Optional but preferred before final release:

```bash
COBBLE_MINECRAFT_EULA_ACCEPTED=1 scripts/test_minecraft_server.sh
```

##### 9. Deferral Rules

Move work out of 0.7.0 if it requires:

- new Minecraft target version support,
- broad stdlib v2 redesign,
- formatting or LSP implementation beyond diagnostic data prep,
- plugin/package manager architecture,
- runtime return values or hidden execution framework,
- resource-pack authoring outside schema documentation.

#### Acceptance Criteria

- The language docs match tested behavior.
- Unsupported constructs have explicit tests and error messages.
- Import diagnostics identify the importing file and failed target.
- Validation errors can be traced back to source where possible.

#### Non-Goals

- No full Python compatibility promise.
- No runtime return values from Minecraft functions unless a clear data model is
  designed first.
- No hidden scoreboard framework that users cannot inspect.

### 0.8.0: Tooling And Authoring Workflow

Theme: make Cobble pleasant to use in larger projects.

#### Goals

- Add first-class formatting and editor-oriented tooling.
- Improve project templates and watch workflows.
- Make local iteration against a Minecraft world easier without forcing it into
  default tests.

#### Candidate Work

- Add `cobble fmt`:
  - stable indentation
  - command-line preservation where possible
  - string and JSON preservation rules
  - formatter tests against examples
- Add minimal LSP or editor tooling:
  - syntax diagnostics
  - go-to source for generated metadata when practical
  - completion for stdlib modules and common resource helper names
  - document symbols for functions and resources
- Improve `cobble watch`:
  - clearer rebuild summaries
  - validation mode
  - debounce behavior
  - graceful error recovery
- Consider `cobble link`:
  - write output into a selected world's `datapacks` directory
  - never overwrite unrelated packs without an explicit project marker
  - pair well with `watch`
- Expand `cobble init` templates:
  - minimal
  - stdlib
  - validation
  - resource-heavy
  - game-mechanic starter
  - web-demo sample
- Add more example projects that feel like real data packs, not only feature
  tests.

#### Acceptance Criteria

- Formatting is deterministic.
- Editor tooling uses the same parser behavior as the CLI.
- Watch/link operations avoid destructive writes outside generated pack paths.
- Larger examples build and validate cleanly.

#### Non-Goals

- No remote package imports.
- No editor feature that requires a separate parser implementation.
- No automatic world modification without explicit user opt-in.

### 0.9.0: Stdlib V2 And Resource Authoring

Theme: make Cobble useful for real data pack authors beyond basic functions.

#### Goals

- Expand the standard library while keeping helpers thin and transparent.
- Improve resource declaration ergonomics.
- Add high-value abstractions seen in mature datapack languages.

#### Candidate Work

- Design stdlib v2 with versioned docs:
  - text components
  - scoreboards
  - storage
  - schedules
  - bossbars
  - teams
  - entities
  - selectors
  - NBT paths
  - item components
  - predicates
  - randomization
- Add selector and target helpers inspired by real authoring needs:
  - reusable selector aliases
  - safer selector interpolation
  - common entity filters
  - validation-aware selector fragments where possible
- Add NBT and vector convenience helpers:
  - keep output transparent
  - avoid inventing a hidden runtime
  - validate generated commands when possible
- Improve resource authoring:
  - better tag merging
  - resource path suggestions
  - typed JSON resource validation for supported kinds
  - deterministic resource ordering
- Consider resource-pack support only if it can be scoped:
  - pack metadata
  - assets namespace layout
  - generated JSON models or language files only where Cobble has a clear API

#### Acceptance Criteria

- Every helper has generated output tests.
- Helper output validates against the target command tree when it emits
  commands.
- Docs show both Cobble source and generated command/resource output.
- Stdlib additions do not require users to trust invisible runtime behavior.

#### Non-Goals

- No broad asset pipeline unless a separate design is accepted.
- No full `beet`-style pack SDK replacement.
- No helper that cannot explain its generated output.

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

## Future 1.x Ideas

These are intentionally deferred until the core language and workflow are more
stable.

- Plugin API for build steps or generated-pack inspection.
- Interop with `beet` or other pack tooling.
- Package manager or dependency model for Cobble libraries.
- Multi-target Minecraft support.
- Rich resource-pack authoring.
- Advanced compile-time macros.
- Generated documentation for Cobble projects.
- Larger example gallery and template catalog.

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

## Decision Backlog

These decisions should be made deliberately before implementation starts.

- Should Cobble support multiple Minecraft versions at once, or only one
  current target per release line?
- Should Cobble expose a plugin API, or only stable generated metadata for
  external tools?
- Should `cobble link` write directly into world folders, and how should it
  prevent accidental overwrites?
- Should resource-pack support live in Cobble core or a separate tool?
- Should stdlib versions be tied to Cobble versions or importable as explicit
  modules?
- Should the web compiler support complete project folders or remain focused on
  single-file demos plus generated virtual files?

## Current Priority Order

1. Expand the language support matrix into the full language reference rewrite.
2. Add source-aware semantic diagnostics before expanding syntax.
3. Keep the website and `/try` demo aligned with the CLI through PR-gated web
   checks.
4. Expand real-world examples before adding large new syntax.
5. Defer plugin/package-manager work until the language and metadata contracts
   are stable.
