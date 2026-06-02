#!/bin/bash
# Downloads Minecraft server.jar and generates commands.json
# Requires: java (JDK 25+), wget/curl, python3

set -e

VERSION="${1:-26.1.2}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DATA_DIR="$SCRIPT_DIR/../data"
mkdir -p "$DATA_DIR"
WORK_DIR="$(mktemp -d "$DATA_DIR/.commands.XXXXXX")"

cleanup() {
    rm -rf "$WORK_DIR"
}
trap cleanup EXIT

echo "Fetching version manifest..."
MANIFEST_URL="https://piston-meta.mojang.com/mc/game/version_manifest_v2.json"
VERSION_JSON_URL=$(curl -fsSL "$MANIFEST_URL" | python3 -c "
import json, sys
manifest = json.load(sys.stdin)
for v in manifest['versions']:
    if v['id'] == '$VERSION':
        print(v['url'])
        break
")

if [ -z "$VERSION_JSON_URL" ]; then
    echo "Error: Version $VERSION not found in manifest"
    exit 1
fi

echo "Fetching version $VERSION info..."
SERVER_URL=$(curl -fsSL "$VERSION_JSON_URL" | python3 -c "
import json, sys
data = json.load(sys.stdin)
print(data['downloads']['server']['url'])
")

echo "Downloading server.jar..."
curl -fsSL -o "$WORK_DIR/server.jar" "$SERVER_URL"

echo "Generating commands.json..."
cd "$WORK_DIR"
java -DbundlerMainClass=net.minecraft.data.Main -jar server.jar --reports 2>/dev/null

COMMANDS_JSON=$(find "$WORK_DIR/generated" -path "*/reports/commands.json" -print -quit)
if [ -z "$COMMANDS_JSON" ]; then
    echo "Error: commands.json was not generated"
    exit 1
fi
cp "$COMMANDS_JSON" "$DATA_DIR/commands.json"

echo "commands.json generated at $DATA_DIR/commands.json"
