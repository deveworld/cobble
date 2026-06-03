# QA And Release Checklist

This document records repeatable Cobble QA commands that should not depend on
chat history.

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

`scripts/check_examples.sh` checks each standalone example independently.
Running `cobble check examples` treats the whole gallery as one project and is
expected to reject duplicate names such as `init` or `tick` across unrelated
examples.

## Web Gate

Run these when `web/`, the WASM wrapper, the compiler transcript, or generated
web assets changed:

```bash
cd web
npm run test:wasm
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

GitHub Actions runs the Rust package subset on pushes and pull requests:
`cargo fmt --check`, `cargo test --locked`, `cargo clippy --locked
--all-targets -- -D warnings`, `cargo package --locked`, and `cargo publish
--dry-run --locked`. CI also runs a docs-only markdown link check on pushes and
pull requests. The additional example, validation, doctor, inspect, full web,
and optional server commands above are manual release gates.

The GitHub Pages workflow runs the web gate on pull requests that touch the
compiler, WASM wrapper, or `web/` sources. It only uploads and deploys Pages
artifacts on `main` pushes or manual dispatches.

## Command Tree Live E2E

Default tests use local fixtures. Before a final release candidate, also verify
the live Mojang manifest/server-jar path in a temporary directory:

```bash
REPO=/path/to/cobble
rm -rf /tmp/cobble-command-tree-e2e
mkdir -p /tmp/cobble-command-tree-e2e
cd /tmp/cobble-command-tree-e2e
cargo run --locked --manifest-path "$REPO/Cargo.toml" -- build "$REPO/examples/inventory.cbl" --validate --output output
cargo run --locked --manifest-path "$REPO/Cargo.toml" -- doctor --commands-json data/commands.json
```

The pass condition is that validation succeeds, `data/commands.json` is
generated, and `doctor` reports a target match for the supported Minecraft
version.

## Optional Server Gate

The server smoke requires Java, network/cache access, and EULA acceptance:

```bash
COBBLE_MINECRAFT_EULA_ACCEPTED=1 scripts/test_minecraft_server.sh
```

If this gate is skipped for a release candidate, record that in the release
notes.
