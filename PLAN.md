# Cobble 0.6.2 Implementation Plan

## Status

- Planning date: 2026-06-02
- Base release: `0.6.1`
- Current development version: `0.6.2-a0`
- Target release: `0.6.2`
- Minecraft target: Java Edition `26.1.2`
- Data pack format: `101.1`

## Theme

Cobble 0.6.2 is a stabilization patch for the 0.6.1 release line. It should
not expand the language or standard library unless a change is required to fix a
release-blocking correctness issue.

The release theme is:

> Turn the post-0.6.1 QA fixes into a clean, reproducible patch release:
> stronger command validation, safer command-tree handling, modern examples,
> clearer release docs, and tighter deployment hygiene.

0.6.2 should be treated as a patch release even though Cobble is still
pre-1.0. The main rule is: no broad new feature work until 0.6.2 is stable.

## Release Goals

1. Preserve the published `v0.6.1` tag and crate as immutable release history.
2. Keep the current development line on a prerelease version until the full
   0.6.2 release gate passes.
3. Promote `0.6.2-a0` to `0.6.2` only after local, package, web, and optional
   server QA are complete.
4. Fix correctness gaps discovered during 0.6.1 QA without changing the
   supported Minecraft version or pack format.
5. Make default command-tree validation fail loudly when a stale local
   `data/commands.json` is present.
6. Keep GitHub Pages deployment scoped to actual web demo changes.
7. Ensure examples and docs match Minecraft Java Edition 26.1.2 syntax.
8. Keep crates.io and GitHub release notes aligned with the exact version being
   published.

## Scope Policy

0.6.2 may include:

- Bug fixes found during 0.6.1 release QA.
- Validator false-positive reductions for known Minecraft 26.1.2 parser shapes.
- Example updates needed because stricter validation now rejects old syntax.
- Documentation, README, changelog, and release workflow cleanup.
- Packaging, CI, and deployment hygiene changes.

0.6.2 should not include:

- New language syntax.
- New stdlib modules beyond bug-fix-level adjustments.
- Support for another Minecraft version.
- A new data pack format target.
- Major parser, transpiler, or project layout rewrites.
- Making real-server tests part of default `cargo test`.

If a candidate task needs broad design work, defer it to 0.7.0 or a later
feature release.

## Workstreams

### 1. Versioning And Release Hygiene

#### Current State

- `v0.6.1` points at the published 0.6.1 release commit.
- QA fixes landed after `v0.6.1`, so they must not be folded into 0.6.1.
- The current branch is versioned as `0.6.2-a0`.
- Cargo/crates.io require SemVer-compatible prerelease identifiers, so the
  development version is `0.6.2-a0`, not raw `0.6.2a0`.

#### Tasks

- Keep `Cargo.toml`, `Cargo.lock`, README, docs, and changelog on
  `0.6.2-a0` while QA is incomplete.
- Do not move the `v0.6.1` tag.
- Do not publish a stable `0.6.2` crate until the full release gate passes.
- When ready, bump `0.6.2-a0` to `0.6.2` in one focused release commit.
- Tag stable release as `v0.6.2`.
- If publishing an alpha crate is needed, use:
  - `cargo publish --dry-run`
  - `cargo publish`
  - install test with `cargo install cobble-lang --version 0.6.2-a0`
- For stable release, verify `cargo install cobble-lang` resolves to `0.6.2`
  only after the stable crate is published.

#### Acceptance Criteria

- `cobble --version` prints the exact intended version.
- `cargo package --locked` packages the exact intended version.
- GitHub release title, tag, changelog, README, and crates.io version all match.
- No force-push is needed after stable tagging.

### 2. Command Validation Hardening

#### Current State

0.6.2-a0 already includes fixes for several 0.6.1 QA findings:

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

0.6.2-a0 strengthens the Rust validation path and the shell setup script:

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
- No docs page describes 0.6.2-a0 changes as stable 0.6.2 before promotion.

### 5. Web Demo And GitHub Pages

#### Current State

- The web demo is already deployed at <https://deveworld.github.io/cobble/>.
- Push-triggered Pages deployment now runs only for `web/**` changes.
- `workflow_dispatch` remains available for manual deployment.

#### Tasks

- Keep Pages deployment path filter to `web/**` only.
- Use manual `workflow_dispatch` when a docs-only release needs a redeploy.
- Re-run local web checks after any web change:
  - `npm run lint`
  - `npm run build:github`
- Verify the live preview image returns `200` after any asset change.
- Keep the base path assumption documented in web deployment notes if needed.

#### Acceptance Criteria

- README-only, docs-only, and Cargo-only pushes do not trigger Pages.
- Web changes trigger Pages automatically.
- Manual dispatch can deploy when explicitly requested.
- The live demo loads the WASM bundle and preview image under `/cobble/`.

### 6. Optional Real-Server QA

#### Current State

- `tests/minecraft_server_test.rs` is ignored by default because it needs Java,
  network/Purpur jar access, and explicit Minecraft EULA acceptance.
- `scripts/test_minecraft_server.sh` guards execution with
  `COBBLE_MINECRAFT_EULA_ACCEPTED=1`.

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

Run this matrix before promoting `0.6.2-a0` to stable `0.6.2`.

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
cargo package --locked
```

### Web Required Only If `web/**` Changed

```bash
cd web
npm run lint
npm run build:github
```

### Optional

```bash
COBBLE_MINECRAFT_EULA_ACCEPTED=1 scripts/test_minecraft_server.sh
```

## Promotion Plan

1. Complete the QA matrix.
2. Resolve any findings with focused commits.
3. Update `CHANGELOG.md` from `0.6.2-a0` to `0.6.2`.
4. Bump Cargo/doc versions from `0.6.2-a0` to `0.6.2`.
5. Run required QA again on the clean stable release commit.
6. Run `cargo publish --dry-run`.
7. Commit the stable version bump.
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

- Should `0.6.2-a0` be published to crates.io, or should it remain a GitHub-only
  prerelease marker until stable 0.6.2?
- Should the command-tree SHA-1 be documented publicly, or is the actionable
  error message enough?
- Should we add a dedicated release checklist file separate from `PLAN.md` for
  future patch releases?

## Completion Checklist

- [ ] Keep `v0.6.1` unchanged.
- [ ] Keep development version as `0.6.2-a0` until release QA passes.
- [ ] Confirm Pages workflow only auto-runs on `web/**` changes.
- [ ] Run required QA matrix.
- [ ] Run optional real-server QA or document why it was skipped.
- [ ] Decide whether to publish `0.6.2-a0`.
- [ ] Promote to stable `0.6.2`.
- [ ] Publish crate and GitHub Release.
