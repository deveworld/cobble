#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/cobble-qa-07-templates.XXXXXX")"
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

templates="$(
  cargo run --locked --quiet -- init --list-templates \
    | awk '/^  / {print $1}'
)"

if [[ -z "$templates" ]]; then
  echo "No templates found from cobble init --list-templates" >&2
  exit 1
fi

while IFS= read -r template; do
  [[ -z "$template" ]] && continue
  project_dir="$work_dir/projects/$template"
  output_dir="$work_dir/outputs/$template"

  echo
  echo "== Template: $template =="
  run cargo run --locked --quiet -- init --name "$project_dir" --template "$template"
  run cargo run --locked --quiet -- fmt --check "$project_dir/src"
  run cargo run --locked --quiet -- check "$project_dir/src"
  run cargo run --locked --quiet -- build "$project_dir" --validate -o "$output_dir"
  run cargo run --locked --quiet -- inspect "$output_dir"
  run cargo run --locked --quiet -- inspect "$output_dir" --json >/dev/null
  run cargo run --locked --quiet -- clean --output "$output_dir"
done <<< "$templates"

echo
echo "Template QA passed for: $(tr '\n' ' ' <<< "$templates" | sed 's/[[:space:]]*$//')"
