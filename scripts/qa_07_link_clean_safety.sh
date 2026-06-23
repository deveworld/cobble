#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/cobble-qa-07-link-clean.XXXXXX")"
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

expect_fail() {
  local log_file="$1"
  shift
  echo "+ $*  # expected failure"
  if "$@" >"$log_file" 2>&1; then
    echo "Expected command to fail, but it succeeded: $*" >&2
    cat "$log_file" >&2
    exit 1
  fi
}

assert_json_field() {
  local json="$1"
  local expression="$2"
  ASSERT_JSON="$json" python3 - "$expression" <<'PY'
import json
import os
import sys

expression = sys.argv[1]
data = json.loads(os.environ["ASSERT_JSON"])
if not eval(expression, {}, {"data": data}):
    raise SystemExit(f"JSON assertion failed: {expression}\n{json.dumps(data, indent=2)}")
PY
}

project_dir="$work_dir/linked_pack"
world_dir="$work_dir/world"
datapacks_dir="$world_dir/datapacks"
pack_name="qa_linked"
pack_dir="$datapacks_dir/$pack_name"
namespace="linked_pack"

run cargo run --locked --quiet -- init --name "$project_dir" --template minimal

run cargo run --locked --quiet -- link "$project_dir" \
  --datapacks "$datapacks_dir" \
  --pack-name "$pack_name" \
  --dry-run
if [[ -e "$project_dir/.cobble/link_state.json" ]]; then
  echo "link --dry-run wrote link_state.json" >&2
  exit 1
fi

run cargo run --locked --quiet -- link "$project_dir" \
  --datapacks "$datapacks_dir" \
  --pack-name "$pack_name"

status_output="$(cargo run --locked --quiet -- link "$project_dir" --status)"
printf '%s\n' "$status_output"
grep -q "Marker: missing" <<< "$status_output"

doctor_missing="$(cargo run --locked --quiet -- doctor --json "$project_dir" --commands-json data/commands.json)"
assert_json_field "$doctor_missing" 'data["experimental_link"]["status"] == "warning"'
assert_json_field "$doctor_missing" 'data["experimental_link"]["marker"]["present"] is False'

mkdir -p "$pack_dir"
printf 'keep me\n' > "$pack_dir/important.txt"
unmarked_log="$work_dir/unmarked-watch.log"
expect_fail "$unmarked_log" cargo run --locked --quiet -- watch "$project_dir/src" --link
grep -q "Refusing to build linked output" "$unmarked_log"
test -f "$pack_dir/important.txt"
rm -rf "$pack_dir"

outside_pack_dir="$work_dir/outside/$pack_name"
mkdir -p "$outside_pack_dir"
printf 'keep me\n' > "$outside_pack_dir/important.txt"
python3 - "$project_dir/.cobble/link_state.json" "$outside_pack_dir" <<'PY'
import json
import sys
from pathlib import Path

state_path = Path(sys.argv[1])
state = json.loads(state_path.read_text())
state["pack_path"] = sys.argv[2]
state_path.write_text(json.dumps(state, indent=2) + "\n")
PY
tampered_status="$(cargo run --locked --quiet -- link "$project_dir" --status)"
printf '%s\n' "$tampered_status"
grep -q "Link state: invalid" <<< "$tampered_status"
grep -q "outside target datapacks directory" <<< "$tampered_status"
doctor_tampered="$(cargo run --locked --quiet -- doctor --json "$project_dir" --commands-json data/commands.json)"
assert_json_field "$doctor_tampered" 'data["status"] == "error"'
assert_json_field "$doctor_tampered" 'data["experimental_link"]["status"] == "error"'
tampered_clean_log="$work_dir/tampered-clean.log"
expect_fail "$tampered_clean_log" cargo run --locked --quiet -- clean "$project_dir" --linked --yes
grep -q "outside target datapacks directory" "$tampered_clean_log"
test -f "$outside_pack_dir/important.txt"
tampered_log="$work_dir/tampered-link-state.log"
expect_fail "$tampered_log" cargo run --locked --quiet -- watch "$project_dir/src" --link
grep -q "outside target datapacks directory" "$tampered_log"

run cargo run --locked --quiet -- link "$project_dir" \
  --datapacks "$datapacks_dir" \
  --pack-name "$pack_name"

mkdir -p "$pack_dir/.cobble" "$pack_dir/data/other_pack/function"
printf '{}\n' > "$pack_dir/pack.mcmeta"
cat > "$pack_dir/.cobble/build_manifest.json" <<'JSON'
{
  "version": 1,
  "cobble_version": "0.7.2",
  "namespace": "other_pack"
}
JSON
mismatched_status="$(cargo run --locked --quiet -- link "$project_dir" --status)"
printf '%s\n' "$mismatched_status"
grep -q "Marker: invalid" <<< "$mismatched_status"
grep -q 'marker namespace `other_pack`' <<< "$mismatched_status"
mismatched_log="$work_dir/mismatched-marker.log"
expect_fail "$mismatched_log" cargo run --locked --quiet -- watch "$project_dir/src" --link
grep -q 'marker namespace `other_pack`' "$mismatched_log"
rm -rf "$pack_dir"

mkdir -p "$pack_dir/.cobble" "$pack_dir/data/$namespace/function"
printf '{}\n' > "$pack_dir/pack.mcmeta"
printf 'keep me\n' > "$pack_dir/SENTINEL_DO_NOT_DELETE.txt"
cat > "$pack_dir/.cobble/build_manifest.json" <<JSON
{
  "version": 1,
  "cobble_version": "0.7.2",
  "namespace": "$namespace",
  "generated_namespaces": ["$namespace"]
}
JSON
forged_status="$(cargo run --locked --quiet -- link "$project_dir" --status)"
printf '%s\n' "$forged_status"
grep -q "Marker: invalid" <<< "$forged_status"
grep -q "missing project_id" <<< "$forged_status"
doctor_forged="$(cargo run --locked --quiet -- doctor --json "$project_dir" --commands-json data/commands.json)"
assert_json_field "$doctor_forged" 'data["status"] == "error"'
assert_json_field "$doctor_forged" 'data["experimental_link"]["status"] == "error"'
forged_watch_log="$work_dir/forged-marker-watch.log"
expect_fail "$forged_watch_log" cargo run --locked --quiet -- watch "$project_dir/src" --link
grep -q "missing project_id" "$forged_watch_log"
forged_clean_log="$work_dir/forged-marker-clean.log"
expect_fail "$forged_clean_log" cargo run --locked --quiet -- clean "$project_dir" --linked --yes
grep -q "missing project_id" "$forged_clean_log"
test -f "$pack_dir/SENTINEL_DO_NOT_DELETE.txt"
rm -rf "$pack_dir"

run cargo run --locked --quiet -- build "$project_dir/src" --validate -o "$pack_dir"

status_output="$(cargo run --locked --quiet -- link "$project_dir" --status)"
printf '%s\n' "$status_output"
grep -q "Marker: present" <<< "$status_output"

doctor_present="$(cargo run --locked --quiet -- doctor --json "$project_dir" --commands-json data/commands.json)"
assert_json_field "$doctor_present" 'data["experimental_link"]["status"] == "ok"'
assert_json_field "$doctor_present" 'data["experimental_link"]["marker"]["present"] is True'

previous_function="$work_dir/previous-main.mcfunction"
cp "$pack_dir/data/$namespace/function/main.mcfunction" "$previous_function"
printf 'def main():\n    /titel @a actionbar bad\n' > "$project_dir/src/main.cbl"
validation_log="$work_dir/failed-validation.log"
expect_fail "$validation_log" cargo run --locked --quiet -- build "$project_dir/src" --validate -o "$pack_dir"
grep -q "validation error" "$validation_log"
cmp "$previous_function" "$pack_dir/data/$namespace/function/main.mcfunction"
test -f "$pack_dir/.cobble/build_manifest.json"

run cargo run --locked --quiet -- clean "$project_dir" --linked --dry-run
unconfirmed_log="$work_dir/unconfirmed-clean.log"
expect_fail "$unconfirmed_log" cargo run --locked --quiet -- clean "$project_dir" --linked
grep -q "requires --yes" "$unconfirmed_log"
run cargo run --locked --quiet -- clean "$project_dir" --linked --yes
if [[ -e "$pack_dir" ]]; then
  echo "clean --linked --yes did not remove $pack_dir" >&2
  exit 1
fi

unmarked_output="$work_dir/unmarked-output"
mkdir -p "$unmarked_output"
printf 'keep me\n' > "$unmarked_output/important.txt"
unmarked_clean_log="$work_dir/unmarked-clean.log"
expect_fail "$unmarked_clean_log" cargo run --locked --quiet -- clean --output "$unmarked_output"
grep -q "Refusing to clean" "$unmarked_clean_log"
test -f "$unmarked_output/important.txt"

if command -v ln >/dev/null 2>&1; then
  real_target="$work_dir/real-datapacks"
  symlink_target="$work_dir/symlink-datapacks"
  mkdir -p "$real_target"
  ln -s "$real_target" "$symlink_target"
  symlink_link_log="$work_dir/symlink-link.log"
  expect_fail "$symlink_link_log" cargo run --locked --quiet -- link "$project_dir" \
    --datapacks "$symlink_target" \
    --pack-name symlink_pack
  grep -q "Refusing to link through symlink" "$symlink_link_log"

  build_symlink_log="$work_dir/symlink-build.log"
  expect_fail "$build_symlink_log" cargo run --locked --quiet -- build "$project_dir/src" \
    -o "$symlink_target/build_pack"
  grep -q "Refusing to build through symlink" "$build_symlink_log"
  test ! -e "$real_target/build_pack"

  build_descendant_output="$work_dir/build-symlink-descendant"
  build_descendant_target="$work_dir/build-symlink-descendant-target"
  mkdir -p "$build_descendant_output" "$build_descendant_target"
  printf 'keep me\n' > "$build_descendant_target/important.txt"
  ln -s "$build_descendant_target" "$build_descendant_output/data"
  build_descendant_log="$work_dir/symlink-descendant-build.log"
  expect_fail "$build_descendant_log" cargo run --locked --quiet -- build "$project_dir/src" \
    -o "$build_descendant_output"
  grep -q "Refusing to build through symlink" "$build_descendant_log"
  test -f "$build_descendant_target/important.txt"
  test ! -e "$build_descendant_target/$namespace"

  symlink_output="$work_dir/symlink-output"
  ln -s "$unmarked_output" "$symlink_output"
  symlink_clean_log="$work_dir/symlink-clean.log"
  expect_fail "$symlink_clean_log" cargo run --locked --quiet -- clean --output "$symlink_output"
  grep -q "Refusing to clean through symlink" "$symlink_clean_log"

  real_clean_parent="$work_dir/real-clean-parent"
  symlink_clean_parent="$work_dir/symlink-clean-parent"
  marked_output="$real_clean_parent/output"
  mkdir -p "$marked_output/.cobble" "$marked_output/data/$namespace/function"
  printf '{}\n' > "$marked_output/pack.mcmeta"
  cat > "$marked_output/.cobble/build_manifest.json" <<JSON
{
  "version": 1,
  "cobble_version": "0.7.2",
  "namespace": "$namespace",
  "generated_namespaces": ["$namespace"]
}
JSON
  ln -s "$real_clean_parent" "$symlink_clean_parent"
  symlink_parent_clean_log="$work_dir/symlink-parent-clean.log"
  expect_fail "$symlink_parent_clean_log" cargo run --locked --quiet -- clean --output "$symlink_clean_parent/output"
  grep -q "Refusing to clean through symlink" "$symlink_parent_clean_log"
  test -f "$marked_output/pack.mcmeta"

  clean_descendant_target="$work_dir/clean-symlink-descendant-target"
  mkdir -p "$clean_descendant_target"
  printf 'keep me\n' > "$clean_descendant_target/important.txt"
  ln -s "$clean_descendant_target" "$marked_output/data/$namespace/function/leak"
  symlink_descendant_clean_log="$work_dir/symlink-descendant-clean.log"
  expect_fail "$symlink_descendant_clean_log" cargo run --locked --quiet -- clean --output "$marked_output"
  grep -q "Refusing to clean through symlink" "$symlink_descendant_clean_log"
  test -f "$clean_descendant_target/important.txt"
fi

echo
echo "Link and clean safety QA passed"
