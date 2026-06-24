#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/cobble-qa-08-resource-authoring.XXXXXX")"
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

json_assert() {
  local label="$1"
  local file="$2"
  local expression="$3"
  python3 - "$label" "$file" "$expression" <<'PY'
import json
import sys

label, path, expression = sys.argv[1:4]
with open(path, encoding="utf-8") as handle:
    data = json.load(handle)
if not eval(expression, {}, {"data": data}):
    raise SystemExit(
        f"{label} failed JSON assertion: {expression}\n"
        f"{json.dumps(data, indent=2)}"
    )
PY
}

echo "== resource_authoring example validates and reports resources =="
run cargo run --locked --quiet -- build examples/resource_authoring --validate -o "$work_dir/example"
cargo run --locked --quiet -- inspect "$work_dir/example" --json >"$work_dir/example.inspect.json"
json_assert \
  "resource authoring manifest" \
  "$work_dir/example.inspect.json" \
  'data["manifest"]["generated"]["json_resources"] >= 8 and any(resource["kind"] == "function_tag" for resource in data["manifest"]["resources"]) and any(resource["kind"] == "predicate" for resource in data["manifest"]["resources"])'

echo
echo "== typed tags merge, dedup, and sort deterministically =="
merge_source="$work_dir/merge.cbl"
cat >"$merge_source" <<'CBL'
from stdlib import datapack, event

datapack.function_tag("minecraft:load", ["qa:extra", "qa:load"])
datapack.function_tag("minecraft:load", ["qa:load", "qa:setup"])
datapack.block_tag("mineable/test", ["minecraft:stone"])
datapack.block_tag("mineable/test", ["minecraft:deepslate", "minecraft:stone"])
datapack.item_tag("rewards", ["minecraft:emerald", "minecraft:diamond"])
datapack.item_tag("rewards", ["minecraft:diamond"])
datapack.entity_type_tag("targets", ["minecraft:skeleton", "minecraft:zombie"])

def load():
    /say load

def setup():
    /say setup

def extra():
    /say extra

stdlib.addEventListener(event.LOAD, load)
CBL
run cargo run --locked --quiet -- build "$merge_source" -o "$work_dir/merge"
json_assert \
  "merged load tag" \
  "$work_dir/merge/data/minecraft/tags/function/load.json" \
  'data["values"] == ["cobble:load", "qa:extra", "qa:load", "qa:setup"]'
json_assert \
  "deduped block tag" \
  "$work_dir/merge/data/cobble/tags/block/mineable/test.json" \
  'data["values"] == ["minecraft:deepslate", "minecraft:stone"]'
json_assert \
  "deduped item tag" \
  "$work_dir/merge/data/cobble/tags/item/rewards.json" \
  'data["values"] == ["minecraft:diamond", "minecraft:emerald"]'
json_assert \
  "entity type tag" \
  "$work_dir/merge/data/cobble/tags/entity_type/targets.json" \
  'data["values"] == ["minecraft:skeleton", "minecraft:zombie"]'

echo
echo "== pass-through duplicate overwrite is refused =="
duplicate_source="$work_dir/duplicate.cbl"
cat >"$duplicate_source" <<'CBL'
from stdlib import datapack

datapack.predicate("dup", {"condition": "minecraft:random_chance", "chance": 1})
datapack.predicate("dup", {"condition": "minecraft:random_chance", "chance": 0})

def main():
    /say duplicate
CBL
set +e
cargo run --locked --quiet -- build "$duplicate_source" -o "$work_dir/duplicate-out" >"$work_dir/duplicate.out" 2>"$work_dir/duplicate.err"
duplicate_status=$?
set -e
if [[ "$duplicate_status" -eq 0 ]]; then
  echo "conflicting duplicate resource unexpectedly succeeded" >&2
  exit 1
fi
grep -q "Duplicate data pack resource" "$work_dir/duplicate.err"

echo
echo "== resource diagnostics include path suggestions and schema errors =="
slash_source="$work_dir/slash.cbl"
cat >"$slash_source" <<'CBL'
from stdlib import datapack

datapack.function_tag("minecraft/load", ["qa:main"])

def main():
    /say slash
CBL
set +e
cargo run --locked --quiet -- check "$slash_source" >"$work_dir/slash.out" 2>"$work_dir/slash.err"
slash_status=$?
set -e
if [[ "$slash_status" -eq 0 ]]; then
  echo "slash-separated namespace unexpectedly passed check" >&2
  exit 1
fi
grep -q "Use 'minecraft:load' instead of a slash-separated namespace prefix" "$work_dir/slash.err"

bad_values_source="$work_dir/bad-values.cbl"
cat >"$bad_values_source" <<'CBL'
from stdlib import datapack

datapack.item_tag("bad", "minecraft:stone")

def main():
    /say bad values
CBL
set +e
cargo run --locked --quiet -- check "$bad_values_source" >"$work_dir/bad-values.out" 2>"$work_dir/bad-values.err"
bad_values_status=$?
set -e
if [[ "$bad_values_status" -eq 0 ]]; then
  echo "non-array tag values unexpectedly passed check" >&2
  exit 1
fi
grep -q "datapack-resource-argument" "$work_dir/bad-values.err"

echo
echo "0.8 resource-authoring QA passed"
