#!/usr/bin/env bash
# Runs the ignored Minecraft server integration test used for release QA.
#
# Required:
#   COBBLE_MINECRAFT_EULA_ACCEPTED=1
#
# Optional:
#   COBBLE_PURPUR_JAR=/path/to/purpur.jar
#   COBBLE_PURPUR_SERVER_CACHE_DIR=/path/to/runtime-cache

set -euo pipefail

if [[ "${COBBLE_MINECRAFT_EULA_ACCEPTED:-}" != "1" ]]; then
  cat >&2 <<'EOF'
Refusing to run the Minecraft server test.

Set COBBLE_MINECRAFT_EULA_ACCEPTED=1 only after you have read and accepted
the Minecraft EULA for this local test server.
EOF
  exit 1
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

if [[ ! -f data/commands.json ]]; then
  echo "data/commands.json not found; generating the 26.1.2 command tree first."
  scripts/setup_commands_json.sh 26.1.2
fi

cargo test --test minecraft_server_test -- --ignored --nocapture
