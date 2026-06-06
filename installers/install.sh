#!/usr/bin/env bash
# Volt WebUI installer for Linux + macOS.
#
# Usage:
#   ./install.sh                     # per-user install to ~/.local
#   ./install.sh --system            # system-wide install to /opt/volt
#   ./install.sh --uninstall         # remove the install
#   ./install.sh --prefix /opt/volt  # custom prefix

set -euo pipefail

PRODUCT_NAME="Volt"
DISPLAY_NAME="Volt WebUI"
PUBLISHER="Volt Project"
APP_VERSION="0.7.1"
BINARY_NAME="webui"

# ----- Parse args -----
PREFIX="$HOME/.local"
SYSTEM=0
UNINSTALL=0
NO_DESKTOP=0
NO_PATH=0
SOURCE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --system) SYSTEM=1; PREFIX="/opt/volt"; shift;;
        --prefix) PREFIX="$2"; shift 2;;
        --uninstall) UNINSTALL=1; shift;;
        --no-desktop) NO_DESKTOP=1; shift;;
        --no-path) NO_PATH=1; shift;;
        --source-dir) SOURCE_DIR="$2"; shift 2;;
        -h|--help)
            echo "Usage: $0 [--system] [--prefix DIR] [--uninstall] [--no-desktop] [--no-path]"
            exit 0;;
        *) echo "Unknown arg: $1" >&2; exit 1;;
    esac
done

# ----- Helpers -----
status() { printf '\033[36m[Volt Installer]\033[0m %s\n' "$1"; }
ok()     { printf '\033[32m[Volt Installer]\033[0m %s\n' "$1"; }
warn()   { printf '\033[33m[Volt Installer]\033[0m %s\n' "$1"; }
err()    { printf '\033[31m[Volt Installer]\033[0m %s\n' "$1" >&2; }

require_root() {
    if [[ $SYSTEM -eq 1 && $EUID -ne 0 ]]; then
        err "System-wide install requires root. Re-run with sudo or omit --system."
        exit 1
    fi
}

# ----- Uninstall -----
if [[ $UNINSTALL -eq 1 ]]; then
    status "Uninstalling $DISPLAY_NAME from $PREFIX..."
    rm -rf "$PREFIX"
    rm -f "$HOME/.local/share/applications/volt.desktop"
    rm -f "$HOME/.local/share/icons/volt.png"
    rm -f "$HOME/Desktop/volt.desktop"
    ok "Uninstall complete."
    exit 0
fi

# ----- Pre-flight -----
require_root
source_binary="$SOURCE_DIR/$BINARY_NAME"
if [[ ! -f "$source_binary" ]]; then
    err "Source binary not found: $source_binary"
    err "Run this installer from the directory containing $BINARY_NAME"
    exit 1
fi

# ----- Install -----
status "Installing $DISPLAY_NAME v$APP_VERSION to $PREFIX"
install -d "$PREFIX/bin"
install -d "$PREFIX/share/volt"
install -d "$PREFIX/share/applications" 2>/dev/null || true
install -d "$PREFIX/share/icons" 2>/dev/null || true

# 1. Copy binary
install -m 0755 "$source_binary" "$PREFIX/bin/$BINARY_NAME"
status "  Installed $PREFIX/bin/$BINARY_NAME"

# 2. Copy docs
for doc in README.md LICENSE CHANGELOG.md AGENTS.md .env.example; do
    [[ -f "$SOURCE_DIR/$doc" ]] && cp "$SOURCE_DIR/$doc" "$PREFIX/share/volt/" || true
done

# 3. .desktop file
if [[ $NO_DESKTOP -eq 0 ]]; then
    cat > "$PREFIX/share/applications/volt.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=$DISPLAY_NAME
GenericName=AI Agent Harness
Comment=Volt WebUI - AI agent with full Postgres-backed harness
Exec=$PREFIX/bin/$BINARY_NAME
Icon=volt
Terminal=false
Categories=Development;Utility;
StartupNotify=true
Keywords=ai;agent;llm;chat;claude;groq;
EOF
    status "  Created .desktop file"

    # Also place one on the user's Desktop if XDG_DESKTOP_DIR is set
    if [[ -n "${XDG_DESKTOP_DIR:-}" && -d "$XDG_DESKTOP_DIR" ]]; then
        cp "$PREFIX/share/applications/volt.desktop" "$XDG_DESKTOP_DIR/" 2>/dev/null || true
        chmod +x "$XDG_DESKTOP_DIR/volt.desktop" 2>/dev/null || true
    fi
fi

# 4. PATH
if [[ $NO_PATH -eq 0 ]]; then
    bashrc="$HOME/.bashrc"
    [[ -f "$HOME/.zshrc" ]] && bashrc="$HOME/.zshrc"
    if ! grep -q "$PREFIX/bin" "$bashrc" 2>/dev/null; then
        {
            echo ""
            echo "# Added by Volt installer"
            echo "export PATH=\"$PREFIX/bin:\$PATH\""
        } >> "$bashrc"
        status "  Added $PREFIX/bin to PATH in $bashrc (restart shell or run: source $bashrc)"
    fi
fi

# 5. Uninstall script
cat > "$PREFIX/bin/volt-uninstall" <<EOF
#!/usr/bin/env bash
exec "$SOURCE_DIR/install.sh" --uninstall
EOF
chmod +x "$PREFIX/bin/volt-uninstall"

# 6. Desktop integration: register MIME + icon
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop_database "$PREFIX/share/applications" 2>/dev/null || true
fi
if command -v xdg-icon-resource >/dev/null 2>&1; then
    # We don't ship an icon; the .desktop file falls back to the binary's own icon
    xdg-icon-resource forceupdate 2>/dev/null || true
fi

ok ""
ok "Install complete!"
ok "  Binary:    $PREFIX/bin/$BINARY_NAME"
ok "  Desktop:   volt.desktop (in your application launcher)"
ok "  Uninstall: volt-uninstall  OR  $SOURCE_DIR/install.sh --uninstall"
echo ""
warn "Before first launch, ensure GROQ_API_KEY and DATABASE_URL are set."
warn "A template is at: $PREFIX/share/volt/.env.example"
