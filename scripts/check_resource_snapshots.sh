#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

echo "+ cargo test --locked --test generated_snapshots_test"
cargo test --locked --test generated_snapshots_test
