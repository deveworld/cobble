# Testing And Releases

These checks are for changes to Cobble itself.

## Test Suite

```bash
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
```

Runtime tests compile packs and execute their generated commands in
`tests/common/mcsim.rs`. `examples/` contains user-facing projects;
`tests/fixtures/projects/` contains release fixtures.

## Differential Test

Random programs are compared with Python using scoreboard integer rules.
The test needs `python3`; CI sets `COBBLE_DIFF_REQUIRE_PYTHON=1` to require it.
After changing expression or control-flow compilation, run more seeds:

```bash
COBBLE_DIFF_SEED=1000 COBBLE_DIFF_CASES=5000 cargo test --locked --test differential_test
```

Failures report a seed and a reduced reproducer. Defaults are seed `1` and
300 cases. Set `COBBLE_DIFF_SHRINK=0` to keep the original failing program.

Tests are grouped by feature. Extend an existing case when it already covers
the failure; add a separate test for a distinct behavior or boundary. Prefer
executed results over temporary register names or helper numbering.

Snapshots cover small, stable export contracts: resource-only output,
function-tag merging, and validated manifest metadata. Full example packs
are built, validated, and inspected by `example_fixtures_test`; their generated
runtime helpers are not checked into snapshots.

For intended export-format changes, update snapshots and review the diff:

```bash
INSTA_UPDATE=always cargo test --locked --test generated_snapshots_test
cargo test --locked --test generated_snapshots_test
```

## Release Gate

```bash
scripts/qa_release_gate.sh
```

Run from a clean worktree with `data/commands.json` available. The gate covers
Rust, examples, snapshots, schemas, web checks, packaging, and a publish dry
run. Set `COBBLE_QA_ALLOW_DIRTY=1` for a local rehearsal. The server gate runs
when `COBBLE_MINECRAFT_EULA_ACCEPTED=1` is set.

## Focused Scripts

| Script | Use after changing |
| --- | --- |
| `scripts/check_examples.sh` | Examples or templates; checks each project separately. |
| `scripts/qa_watch_smoke.sh` | Watch behavior and recovery after failed builds. |
| `scripts/qa_security_regressions.sh` | Output ownership, symlinks, guards, or unroll limits. |
| `scripts/check_resource_snapshots.sh` | Generated resources. |
| `scripts/check_resource_schemas.sh` | Tag JSON schemas. |

## Web Gate

```bash
cd web
npm run test:web
```

This runs WASM and ZIP tests, metadata and type checks, the site build,
Playwright tests, and link checks. It needs Chromium
(`npx playwright install chromium`) and mdBook.

After a package version or Minecraft target change, run
`npm --prefix web run sync:metadata`.

## Documentation

The book uses mdBook `0.5.4`. Pages are listed in `docs/src/SUMMARY.md`.
To build and check links without rebuilding the compiler:

```bash
mdbook build docs
npm --prefix web run test:docs-links
```

Keep guide pages focused on tasks and examples. Put contracts needed by
integrations in the reference; keep fix histories in the changelog.

## Command Tree Check

Generate `data/commands.json` with `scripts/setup_commands_json.sh 26.3`.
Tests that need it skip if it is missing. CI sets
`COBBLE_COMMANDS_JSON_AUTO_DOWNLOAD=0` to prevent unexpected downloads.
Before release, also test `cobble build --validate` from an empty directory
and cache to verify automatic generation. See
[Validation Setup](../reference/cli.md#validation-setup).

## Server Gate

After accepting Minecraft's EULA:

```bash
COBBLE_MINECRAFT_EULA_ACCEPTED=1 scripts/test_minecraft_server.sh
```

The gate loads smoke and showcase packs, checks their self-tests, and verifies
namespace isolation and persistence across reloads. Note any skipped server
gate in the release notes.

## After A Release

```bash
scripts/qa_post_release_smoke.sh
```

This installs the published crate, builds a pack, and checks the GitHub release
and deployed site. CI runs Rust and documentation checks on pushes and pull
requests; the Pages workflow runs the web gate and deploys pushes to `main`.
