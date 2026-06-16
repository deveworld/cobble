#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"
shopt -s nullglob

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/cobble-qa-07-release.XXXXXX")"
cleanup() {
  if [[ "${COBBLE_QA_KEEP:-}" == "1" ]]; then
    echo "Keeping QA work directory: $work_dir"
  else
    rm -rf "$work_dir"
  fi
}
trap cleanup EXIT

run() {
  echo "+ $*"
  "$@"
}

json_ok() {
  local label="$1"
  local json="$2"
  JSON_PAYLOAD="$json" python3 - "$label" <<'PY'
import json
import os
import sys

label = sys.argv[1]
try:
    json.loads(os.environ["JSON_PAYLOAD"])
except json.JSONDecodeError as error:
    raise SystemExit(f"{label} did not emit valid JSON: {error}") from error
PY
}

assert_json_field() {
  local label="$1"
  local json="$2"
  local expression="$3"
  JSON_PAYLOAD="$json" python3 - "$label" "$expression" <<'PY'
import json
import os
import sys

label = sys.argv[1]
expression = sys.argv[2]
data = json.loads(os.environ["JSON_PAYLOAD"])
if not eval(expression, {}, {"data": data}):
    raise SystemExit(
        f"{label} failed JSON assertion: {expression}\n"
        f"{json.dumps(data, indent=2)}"
    )
PY
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
echo "== Examples and workflow scripts =="
run scripts/check_examples.sh
run scripts/qa_07_templates.sh
run scripts/qa_07_link_clean_safety.sh
run scripts/qa_07_watch_smoke.sh
run cargo run --locked -- fmt --check examples
fmt_diff_file="$work_dir/fmt-diff.cbl"
fmt_diff_expected="$work_dir/fmt-diff.expected"
printf 'def main():  \r\n  /say diff  \r\n' > "$fmt_diff_file"
cp "$fmt_diff_file" "$fmt_diff_expected"
set +e
fmt_diff_output="$(cargo run --locked --quiet -- fmt --diff "$fmt_diff_file" 2>"$work_dir/fmt-diff.err")"
fmt_diff_status=$?
set -e
if [[ "$fmt_diff_status" -eq 0 ]]; then
  echo "fmt --diff unexpectedly succeeded on unformatted input" >&2
  exit 1
fi
grep -q -- "--- " <<< "$fmt_diff_output"
grep -q -- "+++ " <<< "$fmt_diff_output"
grep -q -- "-def main():  " <<< "$fmt_diff_output"
grep -q -- "+def main():" <<< "$fmt_diff_output"
grep -q "file(s) differ from formatter output" "$work_dir/fmt-diff.err"
if ! cmp -s "$fmt_diff_expected" "$fmt_diff_file"; then
  echo "fmt --diff modified its input" >&2
  exit 1
fi

check_smoke_json="$(
  cargo run --locked --quiet -- check --json examples/26_smoke/src/main.cbl
)"
json_ok "check --json examples/26_smoke" "$check_smoke_json"
assert_json_field \
  "check --json examples/26_smoke" \
  "$check_smoke_json" \
  'data["schema_version"] == 1 and data["ok"] is True and data["error_count"] == 0'

check_symbols_json="$(
  cargo run --locked --quiet -- check --json --symbols examples/resource_authoring/src/main.cbl
)"
json_ok "check --json --symbols examples/resource_authoring" "$check_symbols_json"
assert_json_field \
  "check --json --symbols examples/resource_authoring" \
  "$check_symbols_json" \
  'data["schema_version"] == 1 and data["ok"] is True and len(data["experimental_symbols"]) > 0'

run cargo run --locked -- init --list-templates

echo
echo "== Template build samples =="
for template in resource-heavy game-mechanic web-demo; do
  project_dir="$work_dir/init-$template"
  output_dir="$work_dir/output-$template"
  run cargo run --locked -- init --name "$project_dir" --template "$template"
  run cargo run --locked -- build "$project_dir" -o "$output_dir"
done

echo
echo "== Validated example builds =="
run cargo run --locked -- build examples/26_smoke --validate -o "$work_dir/26-smoke"
run cargo run --locked -- build examples/26_feature_matrix --validate -o "$work_dir/26-feature-matrix"
run cargo run --locked -- build examples/resource_authoring --validate -o "$work_dir/resource-authoring"
run cargo run --locked -- build examples/inventory.cbl --validate -o "$work_dir/inventory"
run cargo run --locked -- build examples/26_smoke --dry-run --validate

echo
echo "== Validated full example gallery =="
all_examples_dir="$work_dir/all-examples"
mkdir -p "$all_examples_dir"
example_sources=(examples/*.cbl examples/*/src/main.cbl)
for example in "${example_sources[@]}"; do
  example_output="${example#examples/}"
  example_output="${example_output//\//__}"
  example_output="${example_output%.cbl}"
  run cargo run --locked --quiet -- build "$example" \
    --validate \
    -o "$all_examples_dir/$example_output"
done

echo
echo "== Doctor, inspect, link, and clean =="
run cargo run --locked -- doctor
doctor_json="$(cargo run --locked --quiet -- doctor --json)"
json_ok "doctor --json" "$doctor_json"
assert_json_field \
  "doctor --json" \
  "$doctor_json" \
  'data["schema_version"] == 1 and data["commands_json"]["status"] == "ok" and data["experimental_output"]["status"] == "not_configured"'
doctor_output_project="$work_dir/doctor-output"
run cargo run --locked --quiet -- init --name "$doctor_output_project" --template minimal
run cargo run --locked --quiet -- build "$doctor_output_project"
doctor_output_json="$(cargo run --locked --quiet -- doctor --json "$doctor_output_project" --commands-json data/commands.json)"
json_ok "doctor --json configured output" "$doctor_output_json"
assert_json_field \
  "doctor --json configured output" \
  "$doctor_output_json" \
  'data["experimental_output"]["status"] == "ok" and data["experimental_output"]["marker"]["present"] is True'
run cargo run --locked -- inspect "$work_dir/26-smoke"
inspect_json="$(cargo run --locked --quiet -- inspect "$work_dir/26-smoke" --json)"
json_ok "inspect --json" "$inspect_json"
assert_json_field \
  "inspect --json" \
  "$inspect_json" \
  'data["manifest"]["version"] == 1 and data["source_map_entries"] > 0'
run cargo run --locked -- clean --dry-run --output "$work_dir/26-smoke"
run cargo run --locked -- clean --output "$work_dir/26-smoke"

linked_project="$work_dir/linked"
linked_world="$work_dir/linked-world"
run cargo run --locked -- init --name "$linked_project" --template minimal
run cargo run --locked -- link "$linked_project" \
  --datapacks "$linked_world/datapacks" \
  --pack-name qa_linked
run cargo run --locked -- build "$linked_project/src" \
  -o "$linked_world/datapacks/qa_linked"
run cargo run --locked -- clean "$linked_project" --linked --dry-run
run cargo run --locked -- clean "$linked_project" --linked --yes

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
echo "0.7.1 release QA gate passed"
