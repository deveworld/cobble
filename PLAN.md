# Cobble 0.6.2 Implementation Plan

## Status

- Planning date: 2026-06-02
- Base release: `0.6.1`
- Current release candidate version: `0.6.2`
- Target release: `0.6.2`
- Minecraft target: Java Edition `26.1.2`
- Data pack format: `101.1`

## Theme

Cobble 0.6.2 should be wider than a narrow patch, but still smaller and more
controlled than a 0.7.0 release. It should use the post-0.6.1 QA findings as a
base and expand into practical workflow improvements that make Cobble easier to
trust in real data pack projects.

The release theme is:

> Make Cobble 0.6.2 the first "release-quality workflow" build: stronger
> command validation, safer command-tree handling, better project feedback,
> clearer generated metadata, a more useful web demo, modern examples, and
> repeatable release QA.

0.6.2 may include non-breaking user-facing improvements because Cobble is still
pre-1.0. The main rule is: every addition must be small, testable, documented,
and directly useful to authors building Minecraft Java Edition 26.1.2 data
packs.

## Release Goals

1. Preserve the published `v0.6.1` tag and crate as immutable release history.
2. Use a prerelease version during active development, then promote to stable
   only after the full 0.6.2 release gate passes.
3. Promote the working tree to stable `0.6.2` only after local, package, web,
   and optional server QA are complete.
4. Fix correctness gaps discovered during 0.6.1 QA without changing the
   supported Minecraft version or pack format.
5. Add a small set of workflow improvements that reduce friction in normal
   project development.
6. Improve generated source metadata and build feedback so users can understand
   what Cobble emitted.
7. Make default command-tree validation fail loudly when a stale local
   `data/commands.json` is present.
8. Keep GitHub Pages deployment scoped to actual web demo build inputs.
9. Ensure examples and docs match Minecraft Java Edition 26.1.2 syntax.
10. Keep crates.io and GitHub release notes aligned with the exact version being
   published.

## Scope Policy

0.6.2 may include:

- Bug fixes found during 0.6.1 release QA.
- Validator false-positive reductions for known Minecraft 26.1.2 parser shapes.
- Example updates needed because stricter validation now rejects old syntax.
- Documentation, README, changelog, and release workflow cleanup.
- Packaging, CI, and deployment hygiene changes.
- Small CLI/project workflow improvements.
- Additional source-map or generated-manifest metadata.
- Focused stdlib helper additions only when they remove common raw-command
  boilerplate and validate cleanly.
- Web demo improvements that help evaluate Cobble output.

0.6.2 should not include:

- New language syntax that changes parser fundamentals.
- Large new stdlib modules.
- Support for another Minecraft version.
- A new data pack format target.
- Major parser, transpiler, or project layout rewrites.
- Making real-server tests part of default `cargo test`.
- Plugin systems, package managers, remote imports, or LSP support.

If a candidate task needs broad design work, defer it to 0.7.0 or a later
feature release.

## Workstreams

### 1. Versioning And Release Hygiene

#### Current State

- `v0.6.1` points at the published 0.6.1 release commit.
- QA fixes landed after `v0.6.1`, so they must not be folded into 0.6.1.
- The current branch is versioned as stable `0.6.2` after prerelease QA.
- The earlier development build used a Cargo-compatible prerelease identifier.

#### Tasks

- Keep `Cargo.toml`, `Cargo.lock`, README, docs, and changelog on the exact
  version being tested.
- Do not move the `v0.6.1` tag.
- Do not publish a stable `0.6.2` crate until the full release gate passes.
- Stable version bump from the development prerelease to `0.6.2` is prepared in
  this working tree and passed dirty-worktree release QA. A final clean-worktree
  `cargo package --locked` run is still required after the release commit.
- Tag stable release as `v0.6.2`.
- For stable release, verify `cargo install cobble-lang` resolves to `0.6.2`
  only after the stable crate is published.

#### Acceptance Criteria

- `cobble --version` prints the exact intended version.
- `cargo package --locked` packages the exact intended version.
- GitHub release title, tag, changelog, README, and crates.io version all match.
- No force-push is needed after stable tagging.

### 2. Command Validation Hardening

#### Current State

0.6.2 already includes fixes for several 0.6.1 QA findings:

- Nested NBT/JSON delimiter parsing now tracks delimiter type with a stack.
- Legacy item-stack NBT after item IDs is rejected for modern 26.1.2 item
  stacks.
- `minecraft:scoreboard_slot`, `minecraft:swizzle`, `minecraft:item_slot`, and
  `minecraft:item_slots` now have focused parsers instead of generic word
  acceptance.
- Default `data/commands.json` is checked against a known 26.1.2 SHA-1.

#### Tasks

- Re-audit current focused parsers against commands in `data/commands.json`:
  - scoreboard display slots,
  - execute swizzles,
  - entity/block item slots,
  - item stack components,
  - JSON text components,
  - NBT compound/path arguments.
- Add regression tests for any false positives found during audit.
- Keep unknown parser fallback behavior conservative enough to avoid blocking
  unsupported but valid 26.1.2 commands.
- Confirm stricter item-stack validation does not break macro-heavy generated
  commands.
- Keep error position reporting intact for newly rejected cases.

#### Acceptance Criteria

- Known invalid commands fail:
  - mismatched nested NBT delimiters,
  - invalid scoreboard display slot,
  - duplicate or unknown execute swizzle axes,
  - invalid item slot,
  - legacy item-stack NBT.
- Existing valid fixtures still validate cleanly.
- `examples/26_smoke` validates with 52 commands checked.
- `examples/26_feature_matrix` validates with 282 commands checked.
- `examples/inventory.cbl` validates with modern item components.

### 3. Command Tree Generation And Cache Safety

#### Current State

0.6.2 strengthens the Rust validation path and the shell setup script:

- Rust auto-generation already tries multiple manifest hosts and falls back to a
  pinned 26.1.2 server jar URL.
- `scripts/setup_commands_json.sh` now continues past version-detail fetch
  failures instead of stopping after the first manifest URL.
- The default `data/commands.json` path is fingerprint-checked.

#### Tasks

- Re-run the default auto-generation path after temporarily moving local
  `data/commands.json` aside.
- Re-run generation with `COBBLE_MINECRAFT_SERVER_JAR` using a local server jar.
- Re-run generation with `COBBLE_COMMANDS_JSON_URL` using a known-good fixture
  only if a safe URL is available.
- Verify stale default command tree behavior by intentionally providing a wrong
  `data/commands.json`.
- Decide whether the default command-tree SHA should be documented in
  `docs/cli.md` or kept as an internal implementation detail.

#### Acceptance Criteria

- Missing default `data/commands.json` is generated automatically.
- Wrong default `data/commands.json` fails with an actionable message.
- Custom `--commands-json` paths remain allowed without forcing the default
  fingerprint.
- Script and Rust fallback behavior are consistent.

### 4. Examples And Documentation

#### Current State

- README is now shorter and focused on install, demo, quick start, commands,
  configuration, and docs.
- README preview image is shown near the top.
- The preview image is compressed to a smaller JPEG.
- GitHub Discussions link was removed because Discussions is disabled.
- `examples/inventory.cbl` uses modern 26.1.2 item components.

#### Tasks

- Re-check README for version, install, demo, and warning accuracy.
- Keep detailed language content in `docs/language.md`, not README.
- Re-check all examples for legacy item NBT or outdated command syntax.
- Clarify that real-server QA is optional and EULA-gated.
- Ensure docs do not promise full data pack spec coverage.
- Update changelog when additional 0.6.2 fixes are made.

#### Acceptance Criteria

- `cargo run --locked -- check examples` passes for all example files.
- README links resolve or intentionally point to local repo paths.
- No docs page advertises unsupported Minecraft versions.
- No docs page describes unreleased changes as already published before
  promotion.

### 5. Project Workflow And CLI UX

#### Motivation

0.6.1 made project creation and validation more capable, but the normal loop
can still be clearer. 0.6.2 should improve the feedback users get when they
initialize, build, validate, and inspect a project.

#### Candidate Tasks

- [x] Add `cobble doctor` to report:
  - Cobble version,
  - target Minecraft version,
  - pack format,
  - whether Java is available,
  - whether `curl` is available,
  - whether default `data/commands.json` exists and matches the expected SHA-1,
  - whether the current directory has a valid `cobble.toml`.
- [x] Add `cobble build --dry-run` to parse, resolve imports, compute output plan,
  and optionally validate generated commands without writing final output.
- [x] Add clearer build summaries:
  - source files compiled,
  - generated function count,
  - generated resource count,
  - macro command count,
  - validation command count,
  - ZIP output path when enabled.
- [x] Improve `cobble init` post-create output with exact next commands:
  - `cd <project>`,
  - `cobble build --dry-run`,
  - `cobble build --validate`.
- [x] Add `--quiet` or refine current verbosity if logs are noisy in scripts.
- Add tests for CLI output where behavior is stable enough to assert.

#### Acceptance Criteria

- New CLI behavior is covered by focused tests.
- Existing commands keep backward-compatible defaults.
- Build output remains readable in CI logs.
- `cobble doctor` or equivalent does not require network access by default.

### 6. Source Maps And Generated Metadata

#### Motivation

Generated packs now include `.cobble/source_map.json`, but 0.6.2 can make the
metadata more useful for debugging and tooling without changing Cobble syntax.

#### Candidate Tasks

- [x] Add `.cobble/build_manifest.json` with:
  - Cobble version,
  - Minecraft target,
  - pack format,
  - namespace,
  - source root,
  - entry points,
  - generated function/resource counts,
  - validation summary when validation ran.
- [x] Add generated resource entries to metadata where useful:
  - tags,
  - predicates,
  - advancements,
  - loot tables,
  - recipes,
  - item modifiers,
  - dialogs.
- [x] Improve source-map path stability:
  - prefer project-relative paths where possible,
  - avoid leaking absolute paths when a relative source path is known,
  - keep existing validation checks for generated paths outside the data pack.
- [x] Add a `cobble inspect <output>` command only if it can be implemented as a
  small manifest/source-map reader.
- [x] Document the metadata files as internal but stable enough for debugging.

#### Acceptance Criteria

- Metadata is deterministic across clean builds from the same project path.
- Source-map validation still catches stale or missing command entries.
- Generated metadata does not include unnecessary absolute paths.
- Metadata tests cover imported files and generated resources.

### 7. Stdlib And Data Pack Resource Polish

#### Motivation

0.6.1 expanded stdlib v1.1 and resource declarations. 0.6.2 can add small
quality-of-life helpers and safety checks where they are clearly bounded.

#### Candidate Tasks

- [x] Add focused text helpers if they map directly to stable JSON text components:
  - `text.plain`,
  - `text.colored`,
  - `text.score`,
  - `text.selector`.
- [x] Add scoreboard display convenience helpers only if they reuse existing command
  generation and validation paths.
- [x] Add storage helper coverage for common list/object operations if gaps remain
  after 0.6.1.
- [x] Add resource name normalization diagnostics:
  - reject accidental uppercase paths,
  - point to the invalid character,
  - suggest `namespace:path` form when users include slashes that look like a
    namespace.
- Add duplicate-resource diagnostics with both declaration locations if source
  locations are available.
- Keep all helpers thin; do not add a framework that hides Minecraft commands.

#### Acceptance Criteria

- Every new helper has a direct generated-command/resource fixture.
- Generated helper output validates against the 26.1.2 command tree when it
  emits commands.
- Resource diagnostics include enough context for users to fix the declaration.
- No helper requires runtime state that Cobble cannot explain in generated
  output.

### 8. Web Demo And GitHub Pages

#### Current State

- The web demo is already deployed at <https://deveworld.github.io/cobble/>.
- Push-triggered Pages deployment runs only when web demo build inputs change:
  `web/**`, `src/**`, `Cargo.toml`, `Cargo.lock`, or the Pages workflow.
- `workflow_dispatch` remains available for manual deployment.

#### Tasks

- Keep Pages deployment path filter limited to files that can affect the static
  web demo build.
- Use manual `workflow_dispatch` when a docs-only release needs a redeploy.
- Re-run local web checks after any web change:
  - `npm run lint`
  - `npm run build:github`
- Verify the live preview image returns `200` after any asset change.
- Keep the base path assumption documented in web deployment notes if needed.

#### Candidate Enhancements

- [x] Add tabs for generated outputs:
  - `.mcfunction`,
  - `pack.mcmeta`,
  - tags/resources,
  - source map,
  - build manifest if added.
- [x] Add copy/download buttons for each generated file.
- [x] Add a ZIP download if it can be done without pulling in heavy dependencies.
- [x] Add sample selector:
  - hello world,
  - stdlib events,
  - inventory components,
  - resource declarations,
  - validation-focused example.
- [x] Surface parser/transpiler errors in a compact diagnostics panel.
- [x] Split the website root from the compiler route so `/` explains Cobble and
  `/try` hosts the browser compiler.
- [x] Keep the page usable on mobile and desktop.
  - Checked local headless browser screenshots and sub-agent review across
    320/360/390 mobile widths, 768/820/1024 tablet widths, and
    1280/1440/1920 desktop widths.

#### Acceptance Criteria

- README-only and docs-only pushes do not trigger Pages.
- Web changes and Rust compiler changes that affect the WASM demo trigger Pages
  automatically.
- Manual dispatch can deploy when explicitly requested.
- The live demo loads the WASM bundle and preview image under `/cobble/`.
- `npm run lint` passes.
- `npm run build:github` passes.
- Headless browser smoke verifies the live editor renders non-empty generated
  output.
- Asset paths work under `/cobble/`.

### 9. Optional Real-Server QA

#### Current State

- `tests/minecraft_server_test.rs` is ignored by default because it needs Java,
  network/Purpur jar access, and explicit Minecraft EULA acceptance.
- `scripts/test_minecraft_server.sh` guards execution with
  `COBBLE_MINECRAFT_EULA_ACCEPTED=1`.
- 2026-06-02 pre-commit QA: `COBBLE_MINECRAFT_EULA_ACCEPTED=1
  scripts/test_minecraft_server.sh` passed. Purpur downloaded, the generated
  data pack loaded, smoke commands ran, and the server shut down cleanly.
- 2026-06-02 stable pre-commit QA reran
  `COBBLE_MINECRAFT_EULA_ACCEPTED=1 scripts/test_minecraft_server.sh` after
  the working tree was promoted to `0.6.2`; the generated data pack loaded and
  the server smoke passed.

#### Tasks

- Run the real-server smoke test before stable 0.6.2 if EULA acceptance is
  available:

```bash
COBBLE_MINECRAFT_EULA_ACCEPTED=1 scripts/test_minecraft_server.sh
```

- Prefer cached jar/runtime paths for repeatability:

```bash
COBBLE_MINECRAFT_EULA_ACCEPTED=1 \
COBBLE_PURPUR_JAR=/path/to/purpur.jar \
scripts/test_minecraft_server.sh
```

- If the test is not run, record the reason in the release notes.
- Do not remove `#[ignore]`; this test should remain opt-in.

#### Acceptance Criteria

- Server starts successfully.
- Generated data pack loads.
- Load and tick functions run without command errors.
- Server shuts down cleanly.
- If skipped, the release notes state that real-server QA was not run.

## QA Matrix

Run this matrix before committing, tagging, and publishing stable `0.6.2`.

### Required

```bash
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo run --locked -- --version
cargo run --locked -- check examples
cargo run --locked -- build examples/26_smoke --validate -o /tmp/cobble-qa-26-smoke
cargo run --locked -- build examples/26_feature_matrix --validate -o /tmp/cobble-qa-26-feature-matrix
cargo run --locked -- build examples/inventory.cbl --validate -o /tmp/cobble-qa-inventory
cargo run --locked -- doctor
cargo run --locked -- build examples/26_smoke --dry-run --validate
cargo run --locked -- inspect /tmp/cobble-qa-26-smoke
cargo run --locked -- inspect /tmp/cobble-qa-26-smoke --json
cargo package --locked
```

During active pre-commit QA with a dirty worktree, use
`cargo package --locked --allow-dirty` to verify package contents, then rerun
`cargo package --locked` after the release commit is clean.

### Required If CLI Or Metadata Changes

```bash
cargo run --locked -- init --name /tmp/cobble-qa-init --template validation
cargo run --locked -- build /tmp/cobble-qa-init --validate -o /tmp/cobble-qa-init-out
```

### Required If Web Build Inputs Changed

```bash
cd web
npm run lint
npm run build:github
```

### Optional

```bash
COBBLE_MINECRAFT_EULA_ACCEPTED=1 scripts/test_minecraft_server.sh
```

### Results From 2026-06-02 Stable Pre-Commit QA

Passed:

- `cargo fmt --check`
- `git diff --check`
- `cargo test --locked`
- `cargo clippy --locked --all-targets -- -D warnings`
- `cargo run --locked -- --version` prints `cobble 0.6.2`
- `cargo run --locked -- check examples`
- `cargo run --locked -- build examples/26_smoke --validate -o /tmp/cobble-qa-26-smoke`
- `cargo run --locked -- build examples/26_feature_matrix --validate -o /tmp/cobble-qa-26-feature-matrix`
- `cargo run --locked -- build examples/inventory.cbl --validate -o /tmp/cobble-qa-inventory`
- `cargo run --locked -- init --name /tmp/cobble-qa-init --template validation`
- `cargo run --locked -- build /tmp/cobble-qa-init --validate -o /tmp/cobble-qa-init-out`
- `cargo run --locked -- doctor`
- `cargo run --locked -- build examples/26_smoke --dry-run --validate`
- `cargo run --locked -- inspect /tmp/cobble-qa-26-smoke`
- `cargo run --locked -- inspect /tmp/cobble-qa-26-smoke --json`
- `cargo package --locked --allow-dirty`
- `cargo publish --dry-run --locked --allow-dirty`
- `npm run lint`
- `npm run build:github`
- Static GitHub Pages smoke for `/cobble/`, `/cobble/try/`,
  `/cobble/cobble-workshop.jpg`, `/cobble/icon.svg`, and
  `/cobble/wasm/cobble_web_wasm_bg.wasm`.
- Headless Chrome `/try` smoke confirmed non-empty generated mcfunction output.
- Headless Chrome screenshots checked at 390px, 1280px, and 1440px.
- `COBBLE_MINECRAFT_EULA_ACCEPTED=1 scripts/test_minecraft_server.sh`

Notes:

- Sub-agent review found duplicate `.cobble/build_manifest.json` function-tag
  entries when stdlib event tags merged with explicit `datapack.function_tag`
  declarations. This was fixed and covered by
  `cargo test --locked --test datapack_resources_test`.
- After the sub-agent fix, `cargo test --locked`,
  `cargo clippy --locked --all-targets -- -D warnings`,
  `cargo package --locked --allow-dirty`,
  `cargo publish --dry-run --locked --allow-dirty`, `npm run lint`,
  `npm run build:github`, static `/cobble/try/` WASM smoke, `cargo fmt --check`,
  `git diff --check`, and the opt-in Purpur server smoke passed again.
- Final WASM direct-call QA found that `.cobble/build_manifest.json` generated
  by the web demo reported the internal `cobble-web-wasm` helper crate version
  instead of the root `cobble-lang` version. The web WASM build now injects the
  root Cargo version through `COBBLE_LANG_VERSION`, and direct-call QA confirmed
  manifest `cobble_version: "0.6.2"`.
- After the WASM version fix, the stable release gate was rerun: `cargo fmt --check`,
  `git diff --check`, `cargo test --locked`,
  `cargo clippy --locked --all-targets -- -D warnings`, CLI example
  check/build/dry-run/inspect smoke, `npm run lint`, `npm run build:github`,
  static `/cobble/try/` smoke, web WASM direct-call manifest QA,
  `cargo package --locked --allow-dirty`,
  `cargo publish --dry-run --locked --allow-dirty`, and the opt-in Purpur
  server smoke all passed.
- Final sub-agent review found a web `/try` parity issue where stdlib function
  tags and explicit `datapack.function_tag` declarations could produce duplicate
  file entries for the same tag path in the browser file list and ZIP download.
  The web WASM materializer now merges duplicate tag paths in memory, and a
  direct WASM smoke confirmed one `data/minecraft/tags/function/load.json` file
  with merged values.
- Final sub-agent review also noted that the Pages workflow was too narrow now
  that the web WASM compiler depends on root Rust sources. The workflow path
  filter now includes `web/**`, `src/**`, `Cargo.toml`, `Cargo.lock`, and the
  workflow file itself while still avoiding README-only and docs-only deploys.
- `cargo package --locked` still needs one final clean-worktree run after the
  stable `0.6.2` release commit is prepared.

## Promotion Plan

1. Complete the QA matrix.
2. Resolve any findings with focused commits.
3. Make sure every implemented 0.6.2 feature has a changelog entry.
4. Decide whether remaining candidate tasks are deferred to 0.7.0.
5. Run required QA again on the stable `0.6.2` working tree.
6. Run `cargo publish --dry-run`.
7. Commit the stable version bump and implementation changes.
8. Tag `v0.6.2`.
9. Push branch and tag.
10. Publish to crates.io.
11. Create GitHub Release `Cobble 0.6.2`.
12. Verify install:

```bash
cargo install cobble-lang --version 0.6.2
cobble --version
```

## Open Questions

- Should the command-tree SHA-1 be documented publicly, or is the actionable
  error message enough?
- Should we add a dedicated release checklist file separate from `PLAN.md` for
  future patch releases?
- Should build metadata be considered internal-only, or can downstream tools
  rely on `.cobble/build_manifest.json` once introduced?

## Completion Checklist

- [x] Keep `v0.6.1` unchanged.
- [x] Keep a prerelease development version until release QA passes.
- [x] Confirm Pages workflow only auto-runs when web build inputs change.
- [x] Select final 0.6.2 feature workstreams from the candidate list.
- [x] Implement selected workflow/metadata/web/doc improvements.
- [x] Add focused test or smoke coverage for selected implementation tasks.
- [x] Run required QA matrix.
- [x] Run optional real-server QA or document why it was skipped.
- [x] Decide to publish stable `0.6.2` instead of a prerelease crate.
- [x] Promote working tree to stable `0.6.2`.
- [ ] Publish crate and GitHub Release.
