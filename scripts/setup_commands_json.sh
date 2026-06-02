#!/bin/bash
# Downloads Minecraft server.jar and generates commands.json
# Requires: java (JDK 25+), curl, python3, sha1sum or shasum

set -e

VERSION="${1:-26.1.2}"
PINNED_2612_SERVER_SHA1="97ccd4c0ed3f81bbb7bfacddd1090b0c56f9bc51"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DATA_DIR="$SCRIPT_DIR/../data"
mkdir -p "$DATA_DIR"
WORK_DIR="$(mktemp -d "$DATA_DIR/.commands.XXXXXX")"

cleanup() {
    rm -rf "$WORK_DIR"
}
trap cleanup EXIT

sha1_file() {
    if command -v sha1sum >/dev/null 2>&1; then
        sha1sum "$1" | awk '{print $1}'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 1 "$1" | awk '{print $1}'
    else
        echo "Error: sha1sum or shasum is required to verify server.jar" >&2
        return 1
    fi
}

verify_server_jar() {
    local expected="$1"
    local actual
    actual="$(sha1_file "$WORK_DIR/server.jar")"
    if [ "$actual" != "$expected" ]; then
        echo "Error: server.jar SHA-1 mismatch: expected $expected, got $actual" >&2
        exit 1
    fi
}

if [ -n "${COBBLE_COMMANDS_JSON_URL:-}" ]; then
    echo "Downloading commands.json from COBBLE_COMMANDS_JSON_URL..."
    curl -fsSL --retry 3 -o "$DATA_DIR/commands.json" "$COBBLE_COMMANDS_JSON_URL"
    echo "commands.json downloaded at $DATA_DIR/commands.json"
    exit 0
fi

echo "Fetching version manifest..."
MANIFEST_URLS=(
    "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json"
    "https://launchermeta.mojang.com/mc/game/version_manifest_v2.json"
    "https://piston-meta.mojang.com/mc/game/version_manifest.json"
    "https://launchermeta.mojang.com/mc/game/version_manifest.json"
)
VERSION_JSON_URL=""
for MANIFEST_URL in "${MANIFEST_URLS[@]}"; do
    if VERSION_JSON_URL=$(curl -fsSL --retry 3 "$MANIFEST_URL" | python3 -c "
import json, sys
manifest = json.load(sys.stdin)
for v in manifest['versions']:
    if v['id'] == '$VERSION':
        print(v['url'])
        break
"); then
        if [ -n "$VERSION_JSON_URL" ]; then
            break
        fi
    fi
done

SERVER_URL="${COBBLE_MINECRAFT_SERVER_URL:-}"
SERVER_SHA1="${COBBLE_MINECRAFT_SERVER_SHA1:-}"
if [ -z "$VERSION_JSON_URL" ] && [ -z "$SERVER_URL" ] && [ "$VERSION" = "26.1.2" ]; then
    echo "Warning: manifest download failed; using pinned 26.1.2 server.jar URL."
    SERVER_URL="https://piston-data.mojang.com/v1/objects/97ccd4c0ed3f81bbb7bfacddd1090b0c56f9bc51/server.jar"
    SERVER_SHA1="$PINNED_2612_SERVER_SHA1"
fi

if [ -z "$VERSION_JSON_URL" ] && [ -z "$SERVER_URL" ]; then
    echo "Error: Version $VERSION not found in manifest"
    exit 1
fi

if [ -z "$SERVER_URL" ]; then
    echo "Fetching version $VERSION info..."
    VERSION_INFO=$(curl -fsSL --retry 3 "$VERSION_JSON_URL")
    SERVER_URL=$(printf '%s' "$VERSION_INFO" | python3 -c "
import json, sys
data = json.load(sys.stdin)
print(data['downloads']['server']['url'])
")
    SERVER_SHA1=$(printf '%s' "$VERSION_INFO" | python3 -c "
import json, sys
data = json.load(sys.stdin)
print(data['downloads']['server']['sha1'])
")
fi

echo "Downloading server.jar..."
if [ -n "${COBBLE_MINECRAFT_SERVER_JAR:-}" ]; then
    cp "$COBBLE_MINECRAFT_SERVER_JAR" "$WORK_DIR/server.jar"
    if [ -z "$SERVER_SHA1" ] && [ "$VERSION" = "26.1.2" ]; then
        SERVER_SHA1="$PINNED_2612_SERVER_SHA1"
    fi
else
    curl -fsSL --retry 3 -o "$WORK_DIR/server.jar" "$SERVER_URL"
    if [ -z "$SERVER_SHA1" ] && [ "$VERSION" = "26.1.2" ]; then
        SERVER_SHA1="$PINNED_2612_SERVER_SHA1"
    fi
fi

if [ -z "$SERVER_SHA1" ]; then
    echo "Error: no expected SHA-1 is available for server.jar" >&2
    exit 1
fi
verify_server_jar "$SERVER_SHA1"

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
