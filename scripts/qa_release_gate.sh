#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

VERSION="$(grep -m1 '^version' Cargo.toml | cut -d '"' -f 2)"

run() {
  echo "+ $*"
  "$@"
}

package_args=(--locked)
if [[ "${COBBLE_QA_ALLOW_DIRTY:-}" == "1" ]]; then
  package_args+=(--allow-dirty)
fi

# Tests that validate against the real command tree skip when it is missing.
# Require it here so the gate runs them.
if [[ ! -f data/commands.json ]]; then
  echo "data/commands.json is missing. Generate it with scripts/setup_commands_json.sh so the validated build tests run." >&2
  exit 1
fi

echo "== Core Rust gate =="
run cargo fmt --check
run git diff --check
run cargo check --locked
run cargo test --locked
run cargo clippy --locked --all-targets -- -D warnings
run cargo run --locked -- --version
version_output="$(cargo run --locked --quiet -- --version)"
if [[ "$version_output" != *"$VERSION"* ]]; then
  echo "Expected cobble --version to report $VERSION, got: $version_output" >&2
  exit 1
fi
run node scripts/check_web_metadata.mjs

echo
echo "== Examples, security, snapshots, and schemas =="
run scripts/check_examples.sh
run scripts/qa_watch_smoke.sh
run scripts/qa_security_regressions.sh
run scripts/check_resource_snapshots.sh
run scripts/check_resource_schemas.sh

echo
echo "== Web gate =="
(
  cd web
  run npm run test:web
)

echo
echo "== Optional Minecraft server gate =="
if [[ "${COBBLE_MINECRAFT_EULA_ACCEPTED:-}" == "1" ]]; then
  run scripts/test_minecraft_server.sh
else
  echo "Skipping Minecraft server gate; set COBBLE_MINECRAFT_EULA_ACCEPTED=1 after accepting the Minecraft EULA to include it."
fi

echo
echo "== Package dry-runs =="
run cargo package "${package_args[@]}"
run cargo publish --dry-run "${package_args[@]}"
package_version="$(cargo metadata --locked --no-deps --format-version 1 | python3 -c 'import json,sys; data=json.load(sys.stdin); print(data["packages"][0]["version"])')"
if [[ "$package_version" != "$VERSION" ]]; then
  echo "Expected package metadata version $VERSION, got: $package_version" >&2
  exit 1
fi

echo
echo "Release QA gate passed for $VERSION"
