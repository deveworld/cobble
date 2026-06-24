#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/cobble-qa-08-resource-schemas.XXXXXX")"
cleanup() {
  if [[ "${COBBLE_QA_KEEP:-}" == "1" ]]; then
    echo "Keeping QA work directory: $work_dir"
  else
    rm -rf "$work_dir"
  fi
}
trap cleanup EXIT

echo "+ cargo run --locked --quiet -- build examples/resource_authoring -o $work_dir/resource-authoring"
cargo run --locked --quiet -- build examples/resource_authoring -o "$work_dir/resource-authoring"

python3 - "$work_dir/resource-authoring" <<'PY'
import json
import re
import sys
from pathlib import Path

root = Path(sys.argv[1])
resource_id = re.compile(r"^[a-z0-9_.-]+:[a-z0-9_./-]+$")
tag_roots = [
    "tags/function",
    "tags/block",
    "tags/item",
    "tags/entity_type",
]

for namespace_dir in (root / "data").iterdir():
    if not namespace_dir.is_dir():
        continue
    for tag_root in tag_roots:
        for path in (namespace_dir / tag_root).glob("**/*.json"):
            data = json.loads(path.read_text(encoding="utf-8"))
            values = data.get("values")
            if not isinstance(values, list):
                raise SystemExit(f"{path} has non-array values: {values!r}")
            for value in values:
                if not isinstance(value, str):
                    raise SystemExit(f"{path} has non-string tag value: {value!r}")
                if not resource_id.match(value):
                    raise SystemExit(f"{path} has invalid resource ID: {value!r}")

manifest = json.loads((root / ".cobble" / "build_manifest.json").read_text(encoding="utf-8"))
for resource in manifest["resources"]:
    for field in ("namespace", "path"):
        value = resource[field]
        if ".." in value or "\\" in value:
            raise SystemExit(f"unsafe manifest resource {field}: {value!r}")

print("resource schema checks passed")
PY
