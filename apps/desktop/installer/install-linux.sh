#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PAYLOAD_DIR="$SCRIPT_DIR/payload"

if [[ ! -d "$PAYLOAD_DIR" || ! -f "$PAYLOAD_DIR/dylan" || ! -f "$PAYLOAD_DIR/.updater/app.json" ]]; then
  echo "Error: paquete de instalación incompleto junto a install.sh" >&2
  exit 1
fi

if [[ "$(id -u)" -eq 0 ]]; then
  echo "Error: ejecuta install.sh con tu usuario normal (sin sudo) para mantener la instalación por usuario de Veltrix." >&2
  exit 1
fi

APP_CHANNEL="$(grep -o '"channel"[[:space:]]*:[[:space:]]*"[^"]*"' "$PAYLOAD_DIR/.updater/app.json" | head -n1 | sed 's/.*"channel"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/')"
if [[ "$APP_CHANNEL" == "beta" ]]; then
  INSTALL_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/dylan-beta"
  BIN_NAME="dylan-beta"
  APP_NAME="Dylan Test"
  DESKTOP_FILE="${XDG_DATA_HOME:-$HOME/.local/share}/applications/dylan-beta.desktop"
else
  INSTALL_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/dylan"
  BIN_NAME="dylan"
  APP_NAME="Comedor Dylan"
  DESKTOP_FILE="${XDG_DATA_HOME:-$HOME/.local/share}/applications/dylan.desktop"
fi

BIN_DIR="$HOME/.local/bin"

umask 022
mkdir -p "$INSTALL_DIR/.updater" "$BIN_DIR" "$(dirname "$DESKTOP_FILE")"
chmod 0755 "$INSTALL_DIR" "$INSTALL_DIR/.updater"

install -m 0755 "$PAYLOAD_DIR/dylan" "$INSTALL_DIR/dylan"
install -m 0644 "$PAYLOAD_DIR/version.txt" "$INSTALL_DIR/version.txt"
install -m 0644 "$PAYLOAD_DIR/.updater/app.json" "$INSTALL_DIR/.updater/app.json"

for veltrix_bin in "$PAYLOAD_DIR/.updater"/veltrix*; do
  base="$(basename "$veltrix_bin")"
  if [[ "$base" == "veltrix" || "$base" == veltrix-* ]]; then
    install -m 0755 "$veltrix_bin" "$INSTALL_DIR/.updater/$base"
  fi
done

if [[ ! -f "$INSTALL_DIR/.updater/update.lock" ]]; then
  : > "$INSTALL_DIR/.updater/update.lock"
fi
chmod 0644 "$INSTALL_DIR/.updater/update.lock"

cat > "$BIN_DIR/$BIN_NAME" <<EOF
#!/usr/bin/env bash
exec "$INSTALL_DIR/dylan" "\$@"
EOF
chmod 0755 "$BIN_DIR/$BIN_NAME"

cat > "$DESKTOP_FILE" <<EOF
[Desktop Entry]
Type=Application
Name=$APP_NAME
Comment=Sistema de gestión del comedor Dylan
Exec=$INSTALL_DIR/dylan
Terminal=false
Categories=Office;Utility;
EOF
chmod 0644 "$DESKTOP_FILE"

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$(dirname "$DESKTOP_FILE")" >/dev/null 2>&1 || true
fi

VERSION="$(cat "$INSTALL_DIR/version.txt")"
echo "$APP_NAME v$VERSION instalado correctamente en: $INSTALL_DIR"
echo "Ejecutable: $INSTALL_DIR/dylan"
echo "Acceso directo: $DESKTOP_FILE"
