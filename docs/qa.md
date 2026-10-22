# QA And Release Checklist

This document records repeatable Cobble QA commands.

## Snapshot Tests

Generated data pack snapshots live under `tests/snapshots/`.

Review normal snapshot behavior:

```bash
cargo test --locked --test generated_snapshots_test
```

Update snapshots after an intentional generated-output change:

```bash
INSTA_UPDATE=always cargo test --locked --test generated_snapshots_test
cargo test --locked --test generated_snapshots_test
```

Before accepting snapshot updates, inspect the changed `.snap` files and check
that unstable paths, local cache paths, and package versions are redacted.

## Rust Release Gate

Run the package and publish dry-runs from a clean working tree for a final
release. During pre-commit stabilization work, `--allow-dirty` is acceptable
only as an interim check.

```bash
cargo fmt --check
git diff --check
cargo check --locked
cargo test --locked
cargo test --locked commands::watch
cargo clippy --locked --all-targets -- -D warnings
cargo run --locked -- --version
scripts/check_examples.sh
scripts/qa_07_templates.sh
scripts/qa_07_link_clean_safety.sh
scripts/qa_07_watch_smoke.sh
cargo run --locked -- fmt --check examples
cargo run --locked -- fmt --check tests/fixtures/projects
cargo run --locked -- check --json tests/fixtures/projects/26_smoke/src/main.cbl
cargo run --locked -- check --json --symbols tests/fixtures/projects/resource_authoring/src/main.cbl
cargo run --locked -- init --list-templates
cargo run --locked -- init --name /tmp/cobble-qa-init-resource --template resource-heavy
cargo run --locked -- init --name /tmp/cobble-qa-init-game --template game-mechanic
cargo run --locked -- init --name /tmp/cobble-qa-init-web --template web-demo
cargo run --locked -- build /tmp/cobble-qa-init-resource -o /tmp/cobble-qa-init-resource-output
cargo run --locked -- build /tmp/cobble-qa-init-game -o /tmp/cobble-qa-init-game-output
cargo run --locked -- build /tmp/cobble-qa-init-web -o /tmp/cobble-qa-init-web-output
cargo run --locked -- build tests/fixtures/projects/26_smoke --validate -o /tmp/cobble-qa-26-smoke
cargo run --locked -- build tests/fixtures/projects/26_feature_matrix --validate -o /tmp/cobble-qa-26-feature-matrix
cargo run --locked -- build tests/fixtures/projects/stdlib_v3 --validate -o /tmp/cobble-qa-stdlib-v3
cargo run --locked -- build tests/fixtures/projects/resource_authoring --validate -o /tmp/cobble-qa-resource-authoring
cargo run --locked -- build tests/fixtures/projects/inventory.cbl --validate -o /tmp/cobble-qa-inventory
cargo run --locked -- doctor
cargo run --locked -- doctor --json
rm -rf /tmp/cobble-qa-linked /tmp/cobble-qa-linked-world
cargo run --locked -- init --name /tmp/cobble-qa-linked --template minimal
cargo run --locked -- link /tmp/cobble-qa-linked --datapacks /tmp/cobble-qa-linked-world/datapacks --pack-name qa_linked
cargo run --locked -- build /tmp/cobble-qa-linked/src -o /tmp/cobble-qa-linked-world/datapacks/qa_linked
cargo run --locked -- clean /tmp/cobble-qa-linked --linked --dry-run
cargo run --locked -- clean /tmp/cobble-qa-linked --linked --yes
cargo run --locked -- build tests/fixtures/projects/26_smoke --dry-run --validate
cargo run --locked -- inspect /tmp/cobble-qa-26-smoke
cargo run --locked -- inspect /tmp/cobble-qa-26-smoke --json
cargo run --locked -- clean --dry-run --output /tmp/cobble-qa-26-smoke
cargo run --locked -- clean --output /tmp/cobble-qa-26-smoke
cargo package --locked
cargo publish --dry-run --locked
```

The aggregate release gate runs the Rust gate, example checks, focused workflow
QA scripts, representative validated builds, the full example gallery as
validated builds, doctor/inspect JSON checks, linked cleanup, the full web gate,
optional server smoke when EULA acceptance is provided, and package dry-runs:

```bash
scripts/qa_10_release_gate.sh
```

For final release verification, run it from a clean working tree. During
pre-commit stabilization, this interim form allows Cargo package dry-runs to run
against local changes:

```bash
COBBLE_QA_ALLOW_DIRTY=1 scripts/qa_10_release_gate.sh
```

`examples/` holds the user-facing example packs. The release fixtures the
gates build and snapshot (`26_smoke`, `26_feature_matrix`,
`resource_authoring`, `stdlib_v2`, `stdlib_v3`, `resource_pack`,
`unrolling`, `showcase`, and `inventory.cbl`) live in
`tests/fixtures/projects/`.

`scripts/check_examples.sh` checks each standalone example and fixture
independently. Running `cobble check examples` treats the whole gallery as one
project and is expected to reject duplicate names such as `on_load` or
`on_tick` across unrelated examples.

## Web Compiler Metadata

The web home page reads `web/lib/compilerMetadata.ts`, which is generated from
`Cargo.toml` and `src/pack_format.rs`. Regenerate and check it with:

```bash
npm --prefix web run sync:metadata
npm --prefix web run check:metadata
```

`npm --prefix web run lint` checks the committed metadata before building the
WASM package, so version, Minecraft target, and pack-format drift fails during
the web gate.

## Focused 0.7.3 Workflow QA

These scripts run the workflow-specific checks added for the 0.7.3 line. They
create temporary projects and linked world directories under `/tmp` by default
and delete them on success. Set `COBBLE_QA_KEEP=1` to keep their temporary
directories for debugging.

```bash
scripts/qa_07_templates.sh
scripts/qa_07_link_clean_safety.sh
scripts/qa_07_watch_smoke.sh
```

Coverage:

- `qa_07_templates.sh` initializes every template, runs `fmt --check`, `check`,
  `build --validate`, `inspect`, `inspect --json`, and marked-output cleanup.
  Formatter regression tests also cover BOM/CRLF normalization, multiline
  docstring preservation, trailing comments, and no-write behavior when one
  file in a formatted directory fails diagnostics.
- `qa_07_link_clean_safety.sh` covers link dry-run, link state, `link --status`,
  `doctor --json` link marker states, unmarked linked-output refusal, tampered
  link-state refusal across `link --status`, `doctor --json`, `watch --link`,
  and `clean --linked`, mismatched marker namespace refusal, namespace-only
  forged marker refusal, validated rebuild failure preserving the previous
  linked pack, linked cleanup confirmation, unmarked cleanup refusal, build
  symlink-output refusal, and link/watch/clean symlink output, symlink-parent,
  symlink-descendant, or symlinked link-state refusal.
- `qa_07_watch_smoke.sh` runs a bounded linked `watch --validate`, verifies the
  initial build, confirms a failed validated rebuild preserves the previous
  linked pack, confirms a later valid edit recovers, writes generated-output
  noise, and confirms it does not trigger another rebuild.

## Security Regression Gate

Run the focused security regression gate after touching output paths, namespace
validation, link/clean/watch safety, execute guard translation, command
validation staging, compile-time unrolling, or the WASM compiler wrapper:

```bash
scripts/qa_security_regressions.sh
```

Coverage:

- Build output safety: namespace traversal, ZIP path escape, validated
  staging-and-replace ownership, unowned output preservation, existing file
  output refusal, and symlink parent/descendant refusal.
- Link and clean safety: linked output ownership markers, tampered link state,
  namespace/project identity mismatch, unmarked cleanup refusal, and symlinked
  link/clean/watch paths.
- Execute guard translation: raw Python-style `if`/`unless`, `and`/`or`,
  `!=`, and out-of-range integer boundary conditions.
- Expansion budget safety: per-loop, nested aggregate, generated-command, and
  WASM compile-path unrolling limits.

## Watch Smoke

Run this manually before a workflow-focused release candidate:

```bash
rm -rf /tmp/cobble-watch-smoke /tmp/cobble-watch-smoke-output
cargo run --locked -- init --name /tmp/cobble-watch-smoke --template validation
cargo run --locked -- link /tmp/cobble-watch-smoke --datapacks /tmp/cobble-watch-smoke-world/datapacks
cargo run --locked -- watch /tmp/cobble-watch-smoke/src --link --validate
```

While watch is running, edit `/tmp/cobble-watch-smoke/src/main.cbl` and confirm
that one save burst produces one rebuild. Then edit the generated output under
`/tmp/cobble-watch-smoke-world/datapacks/` and confirm it does not trigger another
rebuild. Stop watch with Ctrl+C.

## Web Gate

Run these when `web/`, the WASM wrapper, the compiler transcript, or generated
web assets changed:

```bash
cd web
npm run test:wasm
cargo check --manifest-path wasm/Cargo.toml --locked
npm run test:zip
npm run lint
npm run build:github
npm run test:e2e:run
npm run test:links
```

For local browser E2E setup, run `npx playwright install chromium` once if
Chromium is not already installed. `npm run test:e2e` is a convenience command
that runs `build:github` before `test:e2e:run`.

The full web gate can also be run as:

```bash
cd web
npm run test:web
```

`npm run test:web` includes the WASM unit tests, WASM `cargo check`, data-pack
ZIP test, typecheck/build, Playwright E2E, and markdown/export link check.

GitHub Actions runs the Rust package subset on pushes and pull requests:
`cargo fmt --check`, `cargo test --locked`, `cargo clippy --locked
--all-targets -- -D warnings`, `cargo package --locked`, and `cargo publish
--dry-run --locked`. CI also runs a docs-only markdown link check on pushes and
pull requests. The additional example, validation, doctor, inspect, full web,
and optional server commands above are manual release gates.

The GitHub Pages workflow runs the web gate on pull requests that touch the
compiler, WASM wrapper, or `web/` sources. It only uploads and deploys Pages
artifacts on `main` pushes or manual dispatches.

## Post-Release Verification

After publishing crates.io, creating the GitHub release, and letting GitHub
Pages deploy, run the networked post-release smoke:

```bash
scripts/qa_post_release_smoke.sh
```

The script installs the released crate into a temporary Cargo root, verifies
`cobble --version`, builds and validates a generated smoke pack with the
installed binary, checks that the GitHub release is not draft or prerelease, and
confirms the deployed home page, `/try/` page, and WebAssembly asset are
available. It reads the version from `Cargo.toml` by default.

Useful overrides:

```bash
COBBLE_POST_RELEASE_VERSION=0.9.0 scripts/qa_post_release_smoke.sh
COBBLE_POST_RELEASE_SITE_URL=https://deveworld.github.io/cobble scripts/qa_post_release_smoke.sh
COBBLE_QA_SKIP_GITHUB=1 scripts/qa_post_release_smoke.sh
COBBLE_QA_SKIP_WEB=1 scripts/qa_post_release_smoke.sh
```

## Command Tree Live E2E

Default tests use local fixtures. Before a final release candidate, also verify
the live Mojang manifest/server-jar path in a temporary directory:

```bash
REPO=/path/to/cobble
rm -rf /tmp/cobble-command-tree-e2e
mkdir -p /tmp/cobble-command-tree-e2e
cd /tmp/cobble-command-tree-e2e
cargo run --locked --manifest-path "$REPO/Cargo.toml" -- build "$REPO/tests/fixtures/projects/inventory.cbl" --validate --output output
cargo run --locked --manifest-path "$REPO/Cargo.toml" -- doctor --commands-json data/commands.json
```

The pass condition is that validation succeeds, `data/commands.json` is
generated, and `doctor` reports a target match for the supported Minecraft
version.

Generating `data/commands.json` — whether through this E2E flow,
`scripts/setup_commands_json.sh`, or the automatic download during
`build --validate` — requires network access, `curl`, and Java: Cobble
downloads a Mojang server jar and runs `java --reports` to extract the
command tree. Because `data/commands.json` is gitignored, the GitHub Actions
Rust job sets `COBBLE_COMMANDS_JSON_AUTO_DOWNLOAD=0` so tests that pass
`--validate` skip; unset it (or pre-generate the file) when validation coverage
is wanted in CI.

## Optional Server Gate

The server smoke requires Java, network/cache access, and EULA acceptance:

```bash
COBBLE_MINECRAFT_EULA_ACCEPTED=1 scripts/test_minecraft_server.sh
```

It loads a generated smoke pack and the `showcase` fixture, then runs
`/function cobble_showcase:selftest`. The self-test prints `PASS <check>` or
`FAIL <check> expected <x> got <y>` for each check; the gate fails on any
`FAIL` line, a nonzero `failed` score, or a `passed` score that differs from
the number of `PASS` lines.

It also loads two packs built from the same source under different namespaces
and step sizes, and checks that their `round` and `visits` scores stay
separate. After `/reload`, `round` must be back at 0 and the `persist`
variable `visits` must keep its value.

If this gate is skipped for a release candidate, record that in the release
notes.

## 0.8.0 QA Gate

The 0.8.0 line adds stdlib v2 opt-in, resource authoring ergonomics,
compile-time unrolling, and experimental resource-pack support. Run the
existing 0.7.x gates plus the focused 0.8.0 checks below.

### Focused 0.8.0 Workflow QA

```bash
scripts/qa_08_stdlib_v2.sh
scripts/qa_08_resource_authoring.sh
scripts/qa_08_unrolling.sh
scripts/qa_08_resource_pack.sh
scripts/check_resource_snapshots.sh
scripts/check_resource_schemas.sh
```

Coverage:

- `qa_08_stdlib_v2.sh` covers per-module opt-in (`from stdlib import text`,
  `from stdlib import text, score`), `import stdlib` full activation,
  module-gating errors (diagnostic kind `missing-stdlib-module`) for
  unimported modules,
  `[stdlib] version = 1` deprecation warning, and build manifest
  `stdlib_version`/`active_stdlib_modules` fields.
- `qa_08_resource_authoring.sh` covers tag auto-merge for
  `function_tag`/`block_tag`/`item_tag`/`entity_type_tag`, dedup of
  identical values, deterministic sort order, `replace` warning, typed
  schema violation diagnostics, passthrough resource overwrite refusal,
  and path suggestions for slash/uppercase mistakes.
- `qa_08_unrolling.sh` covers literal `range(n)`, `range(start, stop,
  step)`, and literal array unrolling, the 1024 limit, the 256 expansion
  warning, aggregate nested/command output limits, `unroll-non-literal`
  for non-literal iterables, source-map `Unrolled` kind mapping, and
  manifest `unrolled_loops` count.
- `qa_08_resource_pack.sh` covers `--experimental-resource-pack` opt-in,
  refusal without the flag, `resource_pack.item_model`/
  `block_model`/`lang` generation, unified `data/`+`assets/` output,
  ZIP inclusion of `assets/`, manifest `experimental_features` and
  `resource_pack_models`/`resource_pack_langs` counts, and `inspect --json`
  asset reporting.
- `check_resource_snapshots.sh` regenerates resource snapshots for
  `tests/fixtures/projects/stdlib_v2`, `tests/fixtures/projects/resource_authoring`,
  `tests/fixtures/projects/resource_pack`, and `tests/fixtures/projects/unrolling` and fails on
  unintended diffs.
- `check_resource_schemas.sh` validates typed tag JSON against the Cobble
  tag schema and rejects non-array `values`, non-string entries, and
  invalid resource IDs.

### 0.8.0 Validated Build Matrix

```bash
cargo run --locked -- build tests/fixtures/projects/stdlib_v2 --validate -o /tmp/cobble-qa-stdlib-v2
cargo run --locked -- build tests/fixtures/projects/resource_authoring --validate -o /tmp/cobble-qa-resource-authoring
cargo run --locked -- build tests/fixtures/projects/unrolling --validate -o /tmp/cobble-qa-unrolling
cargo run --locked -- build tests/fixtures/projects/resource_pack --experimental-resource-pack --validate -o /tmp/cobble-qa-resource-pack
cargo run --locked -- inspect /tmp/cobble-qa-stdlib-v2 --json
cargo run --locked -- inspect /tmp/cobble-qa-resource-authoring --json
cargo run --locked -- inspect /tmp/cobble-qa-unrolling --json
cargo run --locked -- inspect /tmp/cobble-qa-resource-pack --json
```

## 0.9.0 QA Gate

The 0.9.0 line widens Cobble into an authoring-platform beta. It keeps the
0.8 gates and adds resource-pack beta checks, schema-versioned tooling JSON,
experimental plugin/migration checks, security regressions, stdlib v3 value
helpers, template/watch workflow smoke tests, and the browser ZIP/export gate.

### Focused 0.9.0 Workflow QA

```bash
scripts/qa_security_regressions.sh
scripts/qa_07_templates.sh
scripts/qa_07_watch_smoke.sh
scripts/qa_08_stdlib_v2.sh
scripts/qa_08_resource_authoring.sh
scripts/qa_08_unrolling.sh
scripts/qa_08_resource_pack.sh
scripts/check_resource_snapshots.sh
scripts/check_resource_schemas.sh
```

The aggregate release gate (`scripts/qa_10_release_gate.sh`) also runs these
0.9 checks:

- `cobble inspect --json` reports `schema_version`, `ok`, `status`,
  `manifest`, and `source_map_entries`.
- `cobble check --json --symbols --experimental-plugins` keeps schema version
  1, reports the diagnostics-only experimental plugin host, and validates the
  read-only plugin manifest draft.
- Declarative plugin rules such as `example_lints.no_tellraw` and
  `example_lints.no_raw_op` can emit experimental diagnostics without running
  project plugin code.
- `cobble check --json --experimental-python-compat` reports the
  diagnostics-only Python compatibility surface and observed supported
  constructs without changing compile semantics.
- `cobble migrate --json` reports the experimental 0.8 to 0.9 dry-run schema,
  source-location review hints, and config before/after summaries without
  rewriting files.
- `cobble migrate --json --apply` performs only supported config-only changes,
  writes a timestamped config backup, and preserves source files.
- Stdlib v3 value helpers generate storage commands, teleport commands, and
  item modifier JSON resources without registering load or tick functions.
- `tests/fixtures/projects/stdlib_v3` validates as a checked-in fixture for storage path,
  item component, selector, position, entity teleport, and schedule helpers.

The 0.9 validated build matrix adds the stdlib v3 fixture to the 0.8 matrix:

```bash
cargo run --locked -- build tests/fixtures/projects/stdlib_v3 --validate -o /tmp/cobble-qa-stdlib-v3
cargo run --locked -- inspect /tmp/cobble-qa-stdlib-v3 --json
```

## 0.10.0 QA Gate

The 0.10.0 line moves the single supported target to Minecraft Java Edition
26.3 (pack format 121.0) and ships the runtime-semantics fixes listed in the
changelog. It keeps every 0.9 check and adds:

- The pinned 26.3 command tree (`data/commands.json`, generated from the 26.3
  server jar) for `--validate` and `cobble validate`, including its permission
  levels and 26.3 argument types.
- `cargo test --locked --test fix_runtime_semantics_test`, which executes the
  generated packs in the command simulator (`tests/common/mcsim.rs`) and checks
  their runtime results.
- `cobble migrate --json` reports the 0.9 to 0.10 dry-run schema by default;
  `--apply` updates `project.pack_format` to `121.0`.
- The optional server gate runs Purpur 26.3 (build 2652).

### 0.10.0 Aggregate Release Gate

```bash
scripts/qa_10_release_gate.sh
```

This composes the Rust, focused, tooling, validated-build, web, optional
server, and package gates above against the 26.3 target. Use
`COBBLE_QA_ALLOW_DIRTY=1` only for local release candidate rehearsal before the
final clean-tree run. After publishing and deploying 0.10.x, run
`scripts/qa_post_release_smoke.sh` to verify the released crate and deployed
web demo.
