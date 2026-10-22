# Testing And Releases

This page lists the checks to run when changing Cobble itself.

## Test Suite

```bash
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
```

Most runtime-behavior tests compile a pack and run it in the command simulator
in `tests/common/mcsim.rs`, which executes the generated functions and checks
scores and storage.

Generated pack snapshots live in `tests/snapshots/`. After an intended change
to generated output, update and re-run them:

```bash
INSTA_UPDATE=always cargo test --locked --test generated_snapshots_test
cargo test --locked --test generated_snapshots_test
```

Review the changed `.snap` files before committing, and check that local
paths and package versions are redacted.

## Release Gate

The release gate runs everything below except the live command-tree check and
the post-release smoke:

```bash
scripts/qa_10_release_gate.sh
```

It runs the Rust checks, the example and fixture checks, the focused QA
scripts, validated builds of the release fixtures, `doctor`/`inspect`/
`migrate` JSON checks, the web gate, the server gate when the EULA variable is
set, and `cargo package` and `cargo publish --dry-run`. Run it on a clean
worktree for a release. `COBBLE_QA_ALLOW_DIRTY=1` lets it run against
uncommitted changes for a rehearsal.

`examples/` holds the user-facing example packs. The release fixtures that the
gates build and snapshot live in `tests/fixtures/projects/`.
`scripts/check_examples.sh` checks each example and fixture on its own;
`cobble check examples` would treat the whole folder as one project and report
duplicate names such as `on_load`.

## Focused Scripts

Each script creates temporary projects under `/tmp` and deletes them on
success. Set `COBBLE_QA_KEEP=1` to keep them.

| Script | Covers |
| --- | --- |
| `qa_07_templates.sh` | Every `cobble init` template through `fmt --check`, `check`, `build --validate`, `inspect`, and cleanup. |
| `qa_07_link_clean_safety.sh` | `link`, `doctor --json`, `watch --link`, and `clean --linked` against unmarked, tampered, forged, and symlinked outputs. |
| `qa_07_watch_smoke.sh` | A bounded `watch --link --validate` run: a failed rebuild keeps the last pack, a later edit recovers, output noise does not trigger a rebuild. |
| `qa_security_regressions.sh` | Output path safety, link and clean ownership, execute guard translation, and unroll limits. Run after touching any of these. |
| `qa_08_stdlib_v2.sh` | Per-module stdlib imports, `missing-stdlib-module`, the `version = 1` warning, and the manifest stdlib fields. |
| `qa_08_resource_authoring.sh` | Tag merging, dedup, and ordering, schema diagnostics, and path suggestions. |
| `qa_08_unrolling.sh` | Literal loop unrolling, its limits and diagnostics, and the `Unrolled` source-map kind. |
| `qa_08_resource_pack.sh` | The resource-pack opt-in, generated assets, ZIP contents, and manifest counts. |
| `check_resource_snapshots.sh` | Regenerates the resource fixture snapshots and fails on unintended diffs. |
| `check_resource_schemas.sh` | Validates generated tag JSON against the tag schema. |

## Web Gate

Run this when `web/`, the WASM wrapper, or the documentation changed:

```bash
cd web
npm run test:web
```

It runs the WASM tests and `cargo check`, the ZIP helper test, the metadata
check and typecheck, the GitHub Pages build, the Playwright tests, and the link
check. Install Chromium once with `npx playwright install chromium`. The build
includes this documentation, so it also needs mdBook (see
[Documentation](#documentation)).

`web/lib/compilerMetadata.ts` is generated from `Cargo.toml` and
`src/pack_format.rs`. After a version or target change, regenerate it with
`npm --prefix web run sync:metadata`; the web gate fails when it is stale.

## Documentation

This documentation is an [mdBook](https://rust-lang.github.io/mdBook/) book.
`docs/book.toml` configures it, `docs/src/SUMMARY.md` lists the pages, and
`docs/theme/cobble.css` holds the theme overrides. The Pages workflow builds it
with mdBook 0.5.4. Install the same version with Cargo, or download a binary
from the [mdBook releases](https://github.com/rust-lang/mdBook/releases):

```bash
cargo install mdbook --version 0.5.4 --locked
```

`mdbook serve` previews the book at <http://localhost:3000> and rebuilds it on
every save:

```bash
mdbook serve docs
```

`npm run build` in `web/` runs `mdbook build` and writes the book to
`web/public/docs/`, so the site serves it under `/docs/`. The link check in the
web gate covers every page in `docs/src/`, including links to headings.

## Command Tree Check

`--validate` needs `data/commands.json`, which Cobble generates by downloading
the Minecraft server jar and running `java --reports`. Tests that need the tree
skip when `data/commands.json` is missing, so `cargo test` also runs on a fresh
clone without Java. CI also sets `COBBLE_COMMANDS_JSON_AUTO_DOWNLOAD=0`, so a
`--validate` run without the tree fails instead of downloading the jar. Before a
release, check the live download path from an empty directory:

```bash
REPO=/path/to/cobble
mkdir -p /tmp/cobble-command-tree-e2e && cd /tmp/cobble-command-tree-e2e
cargo run --locked --manifest-path "$REPO/Cargo.toml" -- build "$REPO/tests/fixtures/projects/inventory.cbl" --validate --output output
cargo run --locked --manifest-path "$REPO/Cargo.toml" -- doctor --commands-json data/commands.json
```

Validation should pass, and `doctor` should report a target match.

## Server Gate

The server gate runs a Purpur 26.3 server. It needs Java, network or cache
access, and EULA acceptance:

```bash
COBBLE_MINECRAFT_EULA_ACCEPTED=1 scripts/test_minecraft_server.sh
```

It loads a smoke pack and the `showcase` fixture and runs
`/function cobble_showcase:selftest`, which prints `PASS <check>` or
`FAIL <check> expected <x> got <y>` for each check. The gate fails on any
`FAIL` line or when the pass count does not match. It also loads two packs
built from the same source under different namespaces and checks that their
scores stay separate, and that after `/reload` a module variable resets while
a `persist` variable keeps its value.

If a release skips this gate, say so in the release notes.

## After A Release

After publishing to crates.io, creating the GitHub release, and letting GitHub
Pages deploy:

```bash
scripts/qa_post_release_smoke.sh
```

It installs the released crate into a temporary Cargo root, builds and
validates a smoke pack with it, checks that the GitHub release is published,
and checks that the website, `/try/`, the WASM file, and the docs are served.
`COBBLE_POST_RELEASE_VERSION`, `COBBLE_POST_RELEASE_SITE_URL`,
`COBBLE_QA_SKIP_GITHUB=1`, and `COBBLE_QA_SKIP_WEB=1` override its defaults.

## CI

GitHub Actions runs `cargo fmt --check`, `cargo test`, `cargo clippy`,
`cargo package`, and `cargo publish --dry-run` on every push and pull request,
plus the markdown link check. The Pages workflow runs the web gate on pull
requests that touch the compiler, the web app, or the docs, and deploys on
pushes to `main`.
