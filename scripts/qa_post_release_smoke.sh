#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

run() {
  echo "+ $*"
  "$@"
}

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "Missing required command: $1" >&2
    exit 1
  fi
}

trim_trailing_slash() {
  local value="$1"
  while [[ "$value" == */ ]]; do
    value="${value%/}"
  done
  printf '%s\n' "$value"
}

require_cmd cargo
require_cmd curl

version="$(
  awk -F '"' '/^version = / {
    print $2
    exit
  }' Cargo.toml
)"
version="${COBBLE_POST_RELEASE_VERSION:-$version}"
crate_name="${COBBLE_POST_RELEASE_CRATE:-cobble-lang}"
bin_name="${COBBLE_POST_RELEASE_BIN:-cobble}"
release_tag="${COBBLE_POST_RELEASE_TAG:-v$version}"
site_url="$(trim_trailing_slash "${COBBLE_POST_RELEASE_SITE_URL:-https://deveworld.github.io/cobble/}")"

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/cobble-post-release.XXXXXX")"
cleanup() {
  if [[ "${COBBLE_QA_KEEP:-}" == "1" ]]; then
    echo "Keeping QA work directory: $work_dir"
  else
    rm -rf "$work_dir"
  fi
}
trap cleanup EXIT

echo "== Post-release metadata =="
echo "Version: $version"
echo "Crate: $crate_name"
echo "Release tag: $release_tag"
echo "Site: $site_url"

if [[ "${COBBLE_QA_SKIP_GITHUB:-}" != "1" ]]; then
  require_cmd gh
  release_json="$(
    gh release view "$release_tag" \
      --json tagName,name,isDraft,isPrerelease,publishedAt,url
  )"
  echo "$release_json"
  if [[ "$release_json" != *'"isDraft":false'* ]]; then
    echo "GitHub release $release_tag is still a draft" >&2
    exit 1
  fi
  if [[ "$release_json" != *'"isPrerelease":false'* ]]; then
    echo "GitHub release $release_tag is still marked prerelease" >&2
    exit 1
  fi
else
  echo "Skipping GitHub release check because COBBLE_QA_SKIP_GITHUB=1."
fi

echo
echo "== crates.io install smoke =="
run cargo info "${crate_name}@${version}"
install_root="$work_dir/install"
run cargo install "$crate_name" --version "$version" --locked --root "$install_root"
installed_bin="$install_root/bin/$bin_name"
actual_version="$("$installed_bin" --version)"
echo "$actual_version"
if [[ "$actual_version" != "$bin_name $version" ]]; then
  echo "Installed binary version mismatch: expected '$bin_name $version'" >&2
  exit 1
fi

smoke_project="$work_dir/smoke"
smoke_output="$work_dir/output"
mkdir -p "$smoke_project"
cat > "$smoke_project/main.cbl" <<'CBL'
from stdlib import datapack

datapack.function_tag("minecraft:load", ["post_release:main"])

def main():
    for i in range(3):
        /say post release {i}
CBL

run "$installed_bin" build "$smoke_project/main.cbl" \
  --namespace post_release \
  --output "$smoke_output" \
  --validate \
  --commands-json "$repo_root/data/commands.json"

generated_function="$smoke_output/data/post_release/function/main.mcfunction"
if [[ ! -f "$generated_function" ]]; then
  echo "Missing generated smoke function: $generated_function" >&2
  exit 1
fi
grep -F "say post release 0" "$generated_function" >/dev/null
grep -F "say post release 1" "$generated_function" >/dev/null
grep -F "say post release 2" "$generated_function" >/dev/null

echo
echo "== GitHub Pages smoke =="
if [[ "${COBBLE_QA_SKIP_WEB:-}" == "1" ]]; then
  echo "Skipping deployed web smoke because COBBLE_QA_SKIP_WEB=1."
else
  home_html="$work_dir/home.html"
  try_html="$work_dir/try.html"
  wasm_file="$work_dir/cobble_web_wasm_bg.wasm"

  run curl -fsSL "$site_url/?verify=$version" -o "$home_html"
  run curl -fsSL "$site_url/try/?verify=$version" -o "$try_html"
  run curl -fsSL "$site_url/wasm/cobble_web_wasm_bg.wasm?verify=$version" -o "$wasm_file"

  normalized_home_html="$work_dir/home.normalized.html"
  perl -0pe 's/<!-- -->//g' "$home_html" > "$normalized_home_html"
  if ! grep -F "$version stable" "$normalized_home_html" >/dev/null; then
    echo "Deployed home page does not advertise $version stable" >&2
    exit 1
  fi

  wasm_magic="$(dd if="$wasm_file" bs=4 count=1 2>/dev/null | od -An -tx1 | tr -d ' \n')"
  if [[ "$wasm_magic" != "0061736d" ]]; then
    echo "Downloaded WebAssembly file has unexpected magic bytes: $wasm_magic" >&2
    exit 1
  fi
fi

echo
echo "Post-release smoke passed for Cobble $version"
