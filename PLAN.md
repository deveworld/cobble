# Cobble Roadmap

## Status

- Last updated: 2026-10-22
- Current stable release: `0.9.0`
- Next release: `0.10.0` (committed, not yet released)
- Planned after that: `0.11.0`, then `1.0.0`
- Active pre-release: none
- Current Minecraft target: Java Edition `26.3`
- Current data pack format: `121.0`
- Package name: `cobble-lang`
- CLI command: `cobble`
- Website: <https://deveworld.github.io/cobble/>
- Browser compiler: <https://deveworld.github.io/cobble/try/>

This roadmap tracks what Cobble should become next. Historical release details
belong in `CHANGELOG.md`; the detailed 0.7.1 to 0.8.0 plans that used to live
here are in git history (`git show f43ef08:PLAN.md`).

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

- `0.7.1` to `0.9.0`: released; see Shipped Releases below.
- `0.10.0`: Minecraft 26.3 target and runtime-semantics fixes. Committed,
  waiting for the release steps.
- `0.11.0`: pack isolation and persistent state, the last planned change to
  generated output before 1.0 locks it.
- `1.0.0`: long-term compatibility contract for language behavior, CLI
  behavior, metadata schemas, and release gates.
- Future 1.x: multi-target support, editor integration, plugin and tooling
  interop, packages, and richer resource-pack pipelines after the core
  compatibility story is stable.

### 0.x Compatibility Rule

Until 1.0, a minor release (`0.N.0`) may change source behavior or generated
output when it ships migration notes (`docs/migration-0.N.md`) and a
`cobble migrate` report for the change. Patch releases (`0.N.x`) preserve the
source language and generated output except for documented bug fixes.

Rules for every 0.x release:

- no hidden runtime behavior,
- no required `cobble.toml` change without a `cobble migrate` path,
- new JSON or metadata surfaces must include a `schema_version`,
- new JSON or metadata fields are experimental unless explicitly listed as
  stable,
- experimental fields must be namespaced or labeled so 1.0 can reserve forward
  compatibility without breaking early users.

### Shipped Releases

- `0.7.1` (2026-06-16): workflow tooling: `fmt`, template discovery, link
  state, linked cleanup, `doctor --json`, and experimental
  `check --json --symbols`.
- `0.7.2` (2026-06-23): security and workflow hardening for `clean` and
  `link --status`.
- `0.7.3` (2026-06-24): security hardening for link state and `watch --link`.
- `0.8.0` (2026-06-24): stdlib v2 module opt-in and data-pack resource
  authoring.
- `0.9.0` (2026-06-27): authoring-platform beta: resource authoring v2,
  resource-pack beta, stdlib v3, CLI and tooling contracts, and experimental
  plugins, Python-compatibility reports, and migrations
  (`docs/09-plan.md`).

Carried forward from those plans and still open:

- Workflow profiles (`[profiles.<name>]`, `build --profile`,
  `watch --profile`). Profiles must never change language semantics, parser
  behavior, stdlib version, or Minecraft target.

### 0.10.0: Minecraft 26.3

Status: committed on `main`, not yet released. Contents are in the
`[0.10.0]` section of `CHANGELOG.md`; the upgrade flow is in
`docs/migration-0.10.md`.

- Single supported target moves to Minecraft Java Edition 26.3 (pack format
  `121.0`).
- Runtime-semantics fixes found with the command simulator, the showcase
  self-test, and the practical examples.
- `examples/` now holds practical packs (`lobby`, `parkour`, `magic_items`);
  release fixtures live in `tests/fixtures/projects/`.
- The optional Purpur server gate runs the `showcase` fixture's in-game
  self-test.

Remaining release steps:

- Run the 0.10.0 aggregate gate (`docs/qa.md`) and the server gate.
- Set the release date in `CHANGELOG.md`.
- Push `main` (this deploys the website), tag `v0.10.0`, publish
  `cobble-lang` to crates.io, and write the GitHub release.
- Check the website and `/try` against the released CLI.

### 0.11.0: Pack Isolation And Persistent State

Status: planned.

Theme: fix the two generated-output behaviors that writing real packs exposed,
while generated output can still change before 1.0 locks it. Both showed up in
ordinary pack code while building `examples/lobby`, `examples/parkour`, and
`examples/magic_items`.

#### Goals

1. Per-pack runtime state.
   - Every Cobble pack keeps numeric variables as fake players on one shared
     `temp` objective. String variables already live in per-namespace storage
     (`<ns>:global`), so only scores collide. Two Cobble packs on one server
     that both use a module variable named `round` overwrite each other.
   - Give each pack a namespace-derived objective, and move internal helper
     scores to the same rule.
   - Raw commands read values into Cobble variables with
     `store result score <var> temp:`, and the docs and examples teach that
     pattern. Pick a spelling that survives the rename before implementing:
     keep `temp` as a source-level name the compiler maps to the pack
     objective, or add an explicit placeholder.
2. Persistent module variables.
   - Module-level variables are reset on every load (`/reload` or a server
     start), so records and settings have to live in hand-written objectives;
     all three practical examples do this.
   - Add an opt-in declaration whose initializer only runs when the score or
     storage value is unset (syntax candidate: `persist best_time = 0`).
   - Plain module variables keep the documented reset-on-load behavior.
3. Migration.
   - `docs/migration-0.11.md` and a `cobble migrate` report that lists raw
     references to `temp`.
   - Document what happens to values already stored under `temp` in existing
     worlds.

Candidates only if they stay small:

- Selector alias refinement (`@Spawn[distance=..1]`).
- A warning for references to undeclared tags in the pack's own namespace.
- String constants.

#### Acceptance Criteria

- The server gate runs two Cobble packs that use the same module variable
  names, and neither disturbs the other.
- A persistent variable keeps its value across `/reload` in the server gate.
- Generated snapshot diffs are reviewed, and the examples and showcase
  self-test pass on Purpur after the change.

#### Non-Goals

- No hidden runtime framework or automatic state the author cannot see in the
  generated output.
- No 1.0 contract work beyond what these changes need.

### 1.0.0: Stable Cobble

Theme: make Cobble stable enough for projects that expect compatibility.

Starts after 0.11.0, so the compatibility audit covers per-pack runtime
state and persistent variables instead of locking the shared `temp`
objective into the contract.

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

The 0.x line already ships stable releases. 1.0.0 means something narrower
and stricter: Cobble should publish a compatibility policy and then follow it.
The release is successful when users know which changes can happen in patch
releases, which changes require minor releases, and which changes would be
breaking.

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
cargo run --locked -- check --json tests/fixtures/projects/26_smoke/src/main.cbl
cargo run --locked -- build tests/fixtures/projects/26_smoke --validate -o /tmp/cobble-qa-26-smoke
cargo run --locked -- build tests/fixtures/projects/26_feature_matrix --validate -o /tmp/cobble-qa-26-feature-matrix
cargo run --locked -- build tests/fixtures/projects/inventory.cbl --validate -o /tmp/cobble-qa-inventory
cargo run --locked -- doctor
cargo run --locked -- doctor --json
cargo run --locked -- inspect /tmp/cobble-qa-26-smoke
cargo run --locked -- inspect /tmp/cobble-qa-26-smoke --json
cargo install --locked --path . --root /tmp/cobble-qa-install
/tmp/cobble-qa-install/bin/cobble --version
/tmp/cobble-qa-install/bin/cobble check tests/fixtures/projects/26_smoke/src/main.cbl
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
- Namespace conflict diagnostics for resources and functions across packages
  (runtime score isolation lands in 0.11.0).
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
cargo run --locked -- check --json tests/fixtures/projects/26_smoke/src/main.cbl
cargo run --locked -- build tests/fixtures/projects/26_smoke --validate -o /tmp/cobble-qa-26-smoke
cargo run --locked -- build tests/fixtures/projects/26_feature_matrix --validate -o /tmp/cobble-qa-26-feature-matrix
cargo run --locked -- build tests/fixtures/projects/inventory.cbl --validate -o /tmp/cobble-qa-inventory
cargo run --locked -- doctor
cargo run --locked -- build tests/fixtures/projects/26_smoke --dry-run --validate
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

The server smoke also runs the `showcase` fixture's in-game self-test and fails
on any failed check.

Workflow tooling checks:

```bash
cargo run --locked -- fmt --check examples
cargo run --locked -- fmt --check tests/fixtures/projects
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

For 0.11.0:

- What is the per-pack objective name, and how do raw commands refer to it
  (keep `temp` as a mapped source name, or add a placeholder)?
- What is the persistent-variable syntax, and does it cover string variables
  as well as scores?
- Does `cobble migrate --apply` rewrite raw `temp` references, or only report
  them?

For 1.0.0:

- Should Cobble support multiple Minecraft versions at once, or only one
  current target per release line?
- What metadata/config extension namespaces must be reserved before 1.0?
- Does the experimental plugin contract graduate to stable metadata only, or
  to a plugin API?
- Should linked-world workflows tail Minecraft logs, and should that be part of
  `watch --link` or a separate command?
- When do workflow profiles ship, and which fields do they cover?

Later:

- What is the package taxonomy for source libraries, stdlib modules, helper
  packages, plugins, and resource asset packages?
- Should selector/NBT/vector ergonomics grow as new syntax, stdlib helpers, or
  both?
- Should compile-time data import start with JSON only, or include YAML/CSV once
  schema validation is designed?
- Should the web compiler support complete project folders or remain focused on
  single-file demos plus generated virtual files?

Settled by shipped releases:

- Link state lives in project-local `.cobble/link_state.json` (0.7.1, hardened
  in 0.7.3).
- `cobble clean` requires `--yes` for real linked cleanup (0.7.1).
- Stdlib versions are selected with `[stdlib] version` and per-module
  `from stdlib import ...` opt-in (0.8.0).
- Resource-pack support lives in Cobble core as a beta workflow (0.9.0).
- Plugins start as an experimental, diagnostics-only declarative host (0.9.0).

## Current Priority Order

1. Release 0.10.0: run the aggregate and server gates, set the release date,
   push, tag, publish to crates.io, and check the website and `/try`.
2. Decide the 0.11.0 design questions in the Decision Backlog.
3. Implement per-pack runtime state and persistent variables with snapshot,
   simulator, and server-gate tests, plus migration notes.
4. Release 0.11.0 and re-verify the practical examples on a real server.
5. Start the 1.0.0 compatibility audit (Phase 0) on top of 0.11.0.
6. Keep plugin APIs, package management, full resource-pack pipelines,
   arbitrary Python/eval, and multi-target behavior deferred until the
   relevant 1.x design work exists.
