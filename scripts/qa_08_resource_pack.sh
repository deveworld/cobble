#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/cobble-qa-08-resource-pack.XXXXXX")"
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

assert_json_field() {
  local label="$1"
  local file="$2"
  local expression="$3"
  python3 - "$label" "$file" "$expression" <<'PY'
import json
import sys

label = sys.argv[1]
path = sys.argv[2]
expression = sys.argv[3]
with open(path, encoding="utf-8") as handle:
    data = json.load(handle)
if not eval(expression, {}, {"data": data}):
    raise SystemExit(
        f"{label} failed JSON assertion: {expression}\n"
        f"{json.dumps(data, indent=2)}"
    )
PY
}

source_file="$work_dir/resource_pack.cbl"
cat >"$source_file" <<'CBL'
from stdlib import resource_pack

resource_pack.item_model("qa:test_item", {"parent": "minecraft:item/generated"})
resource_pack.block_model("test_block", {"parent": "minecraft:block/cube_all"})
resource_pack.lang("qa:en_us", {"item.qa.test_item": "QA Test Item"})

def main():
    /say resource pack
CBL

echo "== resource_pack.* refuses to run without opt-in =="
set +e
cargo run --locked --quiet -- build "$source_file" -o "$work_dir/no-opt-in" >"$work_dir/no-opt-in.out" 2>"$work_dir/no-opt-in.err"
status=$?
set -e
if [[ "$status" -eq 0 ]]; then
  echo "resource_pack build unexpectedly succeeded without opt-in" >&2
  exit 1
fi
grep -q "resource_pack.* requires --experimental-resource-pack" "$work_dir/no-opt-in.err"

echo
echo "== CLI opt-in writes assets and ZIP entries =="
run cargo run --locked --quiet -- build "$source_file" \
  --experimental-resource-pack \
  --zip \
  -o "$work_dir/with-flag"
test -f "$work_dir/with-flag/assets/qa/models/item/test_item.json"
test -f "$work_dir/with-flag/assets/cobble/models/block/test_block.json"
test -f "$work_dir/with-flag/assets/qa/lang/en_us.json"
python3 - "$work_dir/cobble.zip" <<'PY'
import sys
import zipfile

with zipfile.ZipFile(sys.argv[1]) as archive:
    names = set(archive.namelist())
required = {
    "pack.mcmeta",
    "data/cobble/function/main.mcfunction",
    "assets/qa/models/item/test_item.json",
    "assets/cobble/models/block/test_block.json",
    "assets/qa/lang/en_us.json",
}
missing = sorted(required - names)
if missing:
    raise SystemExit(f"ZIP missing expected entries: {missing}")
PY

inspect_json="$work_dir/inspect.json"
echo "+ cargo run --locked --quiet -- inspect $work_dir/with-flag --json"
cargo run --locked --quiet -- inspect "$work_dir/with-flag" --json >"$inspect_json"
assert_json_field \
  "inspect resource-pack manifest" \
  "$inspect_json" \
  '"resource_pack" in data["manifest"]["experimental_features"] and data["manifest"]["generated"]["resource_pack_models"] == 2 and data["manifest"]["generated"]["resource_pack_langs"] == 1'

echo
echo "== Config opt-in works without CLI flag =="
run cargo run --locked --quiet -- build examples/resource_pack -o "$work_dir/config-opt-in"
test -f "$work_dir/config-opt-in/assets/cobble_resource_pack/models/item/custom_sword.json"
test -f "$work_dir/config-opt-in/assets/cobble_resource_pack/models/block/display_block.json"
test -f "$work_dir/config-opt-in/assets/cobble_resource_pack/lang/en_us.json"

echo
echo "0.8 resource-pack QA passed"
