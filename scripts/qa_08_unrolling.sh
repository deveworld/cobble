#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/cobble-qa-08-unrolling.XXXXXX")"
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

source_file="$work_dir/unroll.cbl"
cat >"$source_file" <<'CBL'
def main():
    for i in range(3):
        /say range {i}
    for n in range(1, 6, 2):
        /say stepped {n}
    for label in ["north", "south"]:
        /say array {label}
CBL

echo "== literal range and array loops unroll =="
run cargo run --locked --quiet -- build "$source_file" -o "$work_dir/out"
main_file="$work_dir/out/data/cobble/function/main.mcfunction"
grep -q "say range 0" "$main_file"
grep -q "say range 2" "$main_file"
grep -q "say stepped 5" "$main_file"
grep -q "say array north" "$main_file"
if find "$work_dir/out/data/cobble/function" -type f -name 'loop_*' | grep -q .; then
  echo "unrolling produced legacy loop helper functions" >&2
  exit 1
fi
cargo run --locked --quiet -- inspect "$work_dir/out" --json >"$work_dir/inspect.json"
assert_json_file \
  "unrolling manifest" \
  "$work_dir/inspect.json" \
  'data["manifest"]["unrolled_loops"] == 3'
assert_json_file \
  "unrolling source map" \
  "$work_dir/out/.cobble/source_map.json" \
  'sum(1 for entry in data["entries"] if entry["kind"] == "Unrolled") == 8'

echo
echo "== expansion warning threshold =="
warning_source="$work_dir/warning.cbl"
cat >"$warning_source" <<'CBL'
def main():
    for i in range(257):
        /say big {i}
CBL
run cargo run --locked --quiet -- build "$warning_source" -o "$work_dir/warning-out" 2>"$work_dir/warning.err"
grep -q "unroll-large-expansion" "$work_dir/warning.err"

echo
echo "== invalid unrolling inputs fail =="
limit_source="$work_dir/limit.cbl"
cat >"$limit_source" <<'CBL'
def main():
    for i in range(1025):
        /say too many {i}
CBL
set +e
cargo run --locked --quiet -- build "$limit_source" -o "$work_dir/limit-out" >"$work_dir/limit.out" 2>"$work_dir/limit.err"
limit_status=$?
set -e
if [[ "$limit_status" -eq 0 ]]; then
  echo "range(1025) unexpectedly succeeded" >&2
  exit 1
fi
grep -q "unroll-limit-exceeded" "$work_dir/limit.err"

nested_source="$work_dir/nested-limit.cbl"
cat >"$nested_source" <<'CBL'
def main():
    for i in range(64):
        for j in range(64):
            for k in range(64):
                /say nested {i} {j} {k}
CBL
set +e
cargo run --locked --quiet -- build "$nested_source" -o "$work_dir/nested-limit-out" >"$work_dir/nested-limit.out" 2>"$work_dir/nested-limit.err"
nested_status=$?
set -e
if [[ "$nested_status" -eq 0 ]]; then
  echo "nested aggregate unrolling unexpectedly succeeded" >&2
  exit 1
fi
grep -q "nested unrolling" "$work_dir/nested-limit.err"

command_limit_source="$work_dir/command-limit.cbl"
{
  echo "def main():"
  echo "    for i in range(1024):"
  for index in $(seq 0 64); do
    echo "        /say command $index {i}"
  done
} >"$command_limit_source"
set +e
cargo run --locked --quiet -- build "$command_limit_source" -o "$work_dir/command-limit-out" >"$work_dir/command-limit.out" 2>"$work_dir/command-limit.err"
command_limit_status=$?
set -e
if [[ "$command_limit_status" -eq 0 ]]; then
  echo "command-heavy unrolling unexpectedly succeeded" >&2
  exit 1
fi
grep -q "unrolling generated" "$work_dir/command-limit.err"

nonliteral_source="$work_dir/nonliteral.cbl"
cat >"$nonliteral_source" <<'CBL'
values = [1, 2]

def main():
    for i in values:
        /say nonliteral {i}
CBL
set +e
cargo run --locked --quiet -- build "$nonliteral_source" -o "$work_dir/nonliteral-out" >"$work_dir/nonliteral.out" 2>"$work_dir/nonliteral.err"
nonliteral_status=$?
set -e
if [[ "$nonliteral_status" -eq 0 ]]; then
  echo "non-literal iterable unexpectedly succeeded" >&2
  exit 1
fi
grep -q "unroll-non-literal" "$work_dir/nonliteral.err"

echo
echo "== unrolling example validates =="
run cargo run --locked --quiet -- build examples/unrolling --validate -o "$work_dir/example"

echo
echo "0.8 unrolling QA passed"
