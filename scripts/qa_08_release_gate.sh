#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

run() {
  echo "+ $*"
  "$@"
}

package_args=(--locked)
if [[ "${COBBLE_QA_ALLOW_DIRTY:-}" == "1" ]]; then
  package_args+=(--allow-dirty)
fi

echo "== Core Rust gate =="
run cargo fmt --check
run git diff --check
run cargo check --locked
run cargo test --locked
run cargo clippy --locked --all-targets -- -D warnings
run cargo run --locked -- --version

echo
echo "== Examples and focused 0.8 QA =="
run scripts/check_examples.sh
run scripts/qa_08_stdlib_v2.sh
run scripts/qa_08_resource_authoring.sh
run scripts/qa_08_unrolling.sh
run scripts/qa_08_resource_pack.sh
run scripts/check_resource_snapshots.sh
run scripts/check_resource_schemas.sh

echo
echo "== 0.8 validated build matrix =="
work_dir="$(mktemp -d "${TMPDIR:-/tmp}/cobble-qa-08-release.XXXXXX")"
cleanup() {
  if [[ "${COBBLE_QA_KEEP:-}" == "1" ]]; then
    echo "Keeping QA work directory: $work_dir"
  else
    rm -rf "$work_dir"
  fi
}
trap cleanup EXIT
run cargo run --locked -- build examples/stdlib_v2 --validate -o "$work_dir/stdlib-v2"
run cargo run --locked -- build examples/resource_authoring --validate -o "$work_dir/resource-authoring"
run cargo run --locked -- build examples/unrolling --validate -o "$work_dir/unrolling"
run cargo run --locked -- build examples/resource_pack --experimental-resource-pack --validate -o "$work_dir/resource-pack"
run cargo run --locked -- inspect "$work_dir/stdlib-v2" --json
run cargo run --locked -- inspect "$work_dir/resource-authoring" --json
run cargo run --locked -- inspect "$work_dir/unrolling" --json
run cargo run --locked -- inspect "$work_dir/resource-pack" --json

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

echo
echo "0.8.0 release QA gate passed"
