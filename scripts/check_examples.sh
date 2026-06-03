#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

shopt -s nullglob
examples=(examples/*.cbl examples/*/src/main.cbl)

for example in "${examples[@]}"; do
  echo "Checking ${example}"
  cargo run --locked --quiet -- check "$example"
done
