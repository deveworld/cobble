#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/cobble-qa-08-stdlib-v2.XXXXXX")"
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

assert_json_file() {
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

echo "== from stdlib import activates only listed modules =="
limited_source="$work_dir/limited.cbl"
cat >"$limited_source" <<'CBL'
from stdlib import text, score

def main():
    text.tellraw("@a", "limited")
    score.set("points", 1)
CBL
run cargo run --locked --quiet -- build "$limited_source" -o "$work_dir/limited"
cargo run --locked --quiet -- inspect "$work_dir/limited" --json >"$work_dir/limited.inspect.json"
assert_json_file \
  "limited stdlib modules" \
  "$work_dir/limited.inspect.json" \
  'data["manifest"]["stdlib_version"] == 2 and data["manifest"]["active_stdlib_modules"] == ["score", "text"]'

echo
echo "== import stdlib activates all modules =="
all_source="$work_dir/all.cbl"
cat >"$all_source" <<'CBL'
import stdlib

def main():
    text.tellraw("@a", "all")
    score.set("points", 2)
    storage.set("state", {"ready": True})
CBL
run cargo run --locked --quiet -- build "$all_source" -o "$work_dir/all"
cargo run --locked --quiet -- inspect "$work_dir/all" --json >"$work_dir/all.inspect.json"
assert_json_file \
  "import stdlib modules" \
  "$work_dir/all.inspect.json" \
  '"text" in data["manifest"]["active_stdlib_modules"] and "score" in data["manifest"]["active_stdlib_modules"] and "storage" in data["manifest"]["active_stdlib_modules"] and "datapack" in data["manifest"]["active_stdlib_modules"]'

echo
echo "== unimported module calls fail =="
missing_source="$work_dir/missing.cbl"
cat >"$missing_source" <<'CBL'
from stdlib import text

def main():
    text.tellraw("@a", "missing")
    score.set("points", 3)
CBL
set +e
cargo run --locked --quiet -- build "$missing_source" -o "$work_dir/missing" >"$work_dir/missing.out" 2>"$work_dir/missing.err"
missing_status=$?
set -e
if [[ "$missing_status" -eq 0 ]]; then
  echo "build unexpectedly succeeded for unimported score module" >&2
  exit 1
fi
grep -q "module 'score' not imported" "$work_dir/missing.err"

echo
echo "== [stdlib] version = 1 keeps compatibility and warns =="
v1_project="$work_dir/v1-project"
mkdir -p "$v1_project/src"
cat >"$v1_project/cobble.toml" <<'TOML'
[project]
name = "qa_stdlib_v1"
description = "stdlib v1 compatibility"
namespace = "qa_stdlib_v1"
pack_format = "101.1"

[build]
source = "src"
output = "output"

[stdlib]
version = 1
TOML
cat >"$v1_project/src/main.cbl" <<'CBL'
def main():
    text.tellraw("@a", "v1")
    score.set("points", 4)
CBL
run cargo run --locked --quiet -- build "$v1_project" -o "$work_dir/v1-output" 2>"$work_dir/v1.err"
grep -q "version = 1 is deprecated" "$work_dir/v1.err"
cargo run --locked --quiet -- inspect "$work_dir/v1-output" --json >"$work_dir/v1.inspect.json"
assert_json_file \
  "stdlib v1 manifest" \
  "$work_dir/v1.inspect.json" \
  'data["manifest"]["stdlib_version"] == 1 and "text" in data["manifest"]["active_stdlib_modules"] and "score" in data["manifest"]["active_stdlib_modules"]'

echo
echo "== stdlib_v2 example builds =="
run cargo run --locked --quiet -- build examples/stdlib_v2 --validate -o "$work_dir/example"
cargo run --locked --quiet -- inspect "$work_dir/example" --json >"$work_dir/example.inspect.json"
assert_json_file \
  "stdlib_v2 example manifest" \
  "$work_dir/example.inspect.json" \
  'data["manifest"]["stdlib_version"] == 2 and data["manifest"]["active_stdlib_modules"] == ["datapack", "event", "score", "storage", "text"]'

echo
echo "0.8 stdlib v2 QA passed"
