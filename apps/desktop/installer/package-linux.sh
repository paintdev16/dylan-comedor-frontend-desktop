#!/usr/bin/env bash
set -euo pipefail

DYLAN_BIN=""
VELTRIX_BIN=""
OUTPUT_DIR=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dylan)
      DYLAN_BIN="$2"
      shift 2
      ;;
    --veltrix)
      VELTRIX_BIN="$2"
      shift 2
      ;;
    --output)
      OUTPUT_DIR="$2"
      shift 2
      ;;
    *)
      echo "Uso: $0 --dylan <binario> --veltrix <veltrix-linux-x86_64> --output <directorio>" >&2
      exit 1
      ;;
  esac
done

if [[ -z "$DYLAN_BIN" || -z "$VELTRIX_BIN" || -z "$OUTPUT_DIR" ]]; then
  echo "Uso: $0 --dylan <binario> --veltrix <veltrix-linux-x86_64> --output <directorio>" >&2
  exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"

VERSION="$(cargo metadata --manifest-path "$REPO_ROOT/Cargo.toml" --locked --no-deps --format-version 1 | python3 -c 'import json,sys; m=json.load(sys.stdin); print(next(p["version"] for p in m["packages"] if p["name"]=="dylan"))')"
if [[ ! "$VERSION" =~ ^([0-9]+\.[0-9]+\.[0-9]+|1\.0\.0-beta\.[1-9][0-9]*)$ ]]; then
  echo "Versión no admitida: $VERSION" >&2
  exit 1
fi

if [[ "$VERSION" == *-* ]]; then
  APP_CONFIG="$SCRIPT_DIR/app-linux.json"
else
  APP_CONFIG="$SCRIPT_DIR/app-linux-stable.json"
fi

VELTRIX_TAG="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["tag"])' "$SCRIPT_DIR/veltrix-release.json")"
EXPECTED_VELTRIX_SHA="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["linux_x86_64_sha256"])' "$SCRIPT_DIR/veltrix-release.json")"
ACTUAL_VELTRIX_SHA="$(sha256sum "$VELTRIX_BIN" | awk '{print $1}')"

if [[ "${ACTUAL_VELTRIX_SHA,,}" != "${EXPECTED_VELTRIX_SHA,,}" ]]; then
  echo "SHA-256 inválido para veltrix-linux-x86_64: esperado $EXPECTED_VELTRIX_SHA, obtenido $ACTUAL_VELTRIX_SHA" >&2
  exit 1
fi

rm -rf "$OUTPUT_DIR"
mkdir -p "$OUTPUT_DIR/inputs" "$OUTPUT_DIR/update-stage" "$OUTPUT_DIR/installer-stage/payload/.updater"

install -m 0755 "$DYLAN_BIN" "$OUTPUT_DIR/inputs/dylan"

# 1. Paquete de actualización Veltrix (.tar.gz con solo dylan y version.txt, sin symlinks ni .updater/)
install -m 0755 "$DYLAN_BIN" "$OUTPUT_DIR/update-stage/dylan"
printf '%s\n' "$VERSION" > "$OUTPUT_DIR/update-stage/version.txt"
chmod 0644 "$OUTPUT_DIR/update-stage/version.txt"

UPDATE_ARCHIVE_NAME="Dylan-Update-${VERSION}-linux-x86_64.tar.gz"
UPDATE_ARCHIVE_PATH="$OUTPUT_DIR/$UPDATE_ARCHIVE_NAME"
tar --sort=name --owner=0 --group=0 --numeric-owner -czf "$UPDATE_ARCHIVE_PATH" -C "$OUTPUT_DIR/update-stage" dylan version.txt

# 2. Paquete instalador inicial para usuarios finales (incluye install.sh + .updater/)
install -m 0755 "$SCRIPT_DIR/install-linux.sh" "$OUTPUT_DIR/installer-stage/install.sh"
install -m 0755 "$DYLAN_BIN" "$OUTPUT_DIR/installer-stage/payload/dylan"
printf '%s\n' "$VERSION" > "$OUTPUT_DIR/installer-stage/payload/version.txt"
chmod 0644 "$OUTPUT_DIR/installer-stage/payload/version.txt"
install -m 0755 "$VELTRIX_BIN" "$OUTPUT_DIR/installer-stage/payload/.updater/veltrix-${VELTRIX_TAG}"
install -m 0755 "$VELTRIX_BIN" "$OUTPUT_DIR/installer-stage/payload/.updater/veltrix"
install -m 0644 "$APP_CONFIG" "$OUTPUT_DIR/installer-stage/payload/.updater/app.json"
: > "$OUTPUT_DIR/installer-stage/payload/.updater/update.lock"
chmod 0644 "$OUTPUT_DIR/installer-stage/payload/.updater/update.lock"

INSTALLER_ARCHIVE_NAME="Dylan-Installer-${VERSION}-linux-x86_64.tar.gz"
INSTALLER_ARCHIVE_PATH="$OUTPUT_DIR/$INSTALLER_ARCHIVE_NAME"
tar --sort=name --owner=0 --group=0 --numeric-owner -czf "$INSTALLER_ARCHIVE_PATH" -C "$OUTPUT_DIR/installer-stage" install.sh payload

# 3. Manifiesto update-linux-x86_64.json
UPDATE_SIZE="$(stat -c%s "$UPDATE_ARCHIVE_PATH")"
UPDATE_SHA="$(sha256sum "$UPDATE_ARCHIVE_PATH" | awk '{print $1}')"
INSTALLED_SHA="$(sha256sum "$DYLAN_BIN" | awk '{print $1}')"

python3 - "$APP_CONFIG" "$VERSION" "$UPDATE_SIZE" "$UPDATE_ARCHIVE_NAME" "$UPDATE_SHA" "$INSTALLED_SHA" "$OUTPUT_DIR/update-linux-x86_64.json" <<'PY'
import json, sys

app_cfg_path, version, size_str, archive_name, sha256, installed_sha256, out_path = sys.argv[1:]
with open(app_cfg_path, "r", encoding="utf-8") as f:
    app = json.load(f)

manifest = {
    "app_id": app["app_id"],
    "version": version,
    "channel": app["channel"],
    "platform": "linux",
    "architecture": "x86_64",
    "size": int(size_str),
    "url": f"https://github.com/paintdev16/dylan-comedor-frontend-desktop/releases/download/v{version}/{archive_name}",
    "sha256": sha256.lower(),
    "installed_sha256": installed_sha256.lower(),
}

with open(out_path, "w", encoding="utf-8", newline="\n") as f:
    json.dump(manifest, f, indent=2)
    f.write("\n")
PY

rm -rf "$OUTPUT_DIR/update-stage" "$OUTPUT_DIR/installer-stage"
echo "Empaquetado Linux generado en: $OUTPUT_DIR"
