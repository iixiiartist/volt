#!/usr/bin/env bash
# Build a portable Volt distribution archive.
#
# Outputs:
#   installers/volt-windows.zip       (webui.exe + install.ps1 + docs)
#   installers/volt-linux-x86_64.tar.gz  (webui + install.sh + docs)
#   installers/volt-macos-universal.tar.gz (webui + install.sh + docs)
#
# Usage:
#   ./build-dist.sh            # build for current platform
#   ./build-dist.sh --all      # build for all platforms (requires cross toolchains)

set -euo pipefail

cd "$(dirname "$0")/.."
ROOT="$(pwd)"
DIST="$ROOT/installers"
mkdir -p "$DIST"

# ----- Detect / select target -----
TARGET="${1:-current}"
case "$TARGET" in
    current|--all|"") TARGET="current" ;;
    windows) BUILD_WINDOWS=1 ;;
    linux)   BUILD_LINUX=1 ;;
    macos)   BUILD_MACOS=1 ;;
    --all)   BUILD_WINDOWS=1; BUILD_LINUX=1; BUILD_MACOS=1 ;;
    *) echo "Unknown target: $TARGET" >&2; exit 1 ;;
esac

# Always build current platform by default
case "$(uname -s)" in
    MINGW*|MSYS*|CYGWIN*) BUILD_WINDOWS=1 ;;
    Linux*)               BUILD_LINUX=1 ;;
    Darwin*)              BUILD_MACOS=1 ;;
esac

build_for() {
    local triple="$1"
    local out_name="$2"
    echo "=== Building $out_name ($triple) ==="
    cargo build --release --features webui --bin webui --target "$triple" || {
        echo "Cross build for $triple failed. Skipping." >&2
        return 1
    }
}

# ----- Build current platform release -----
echo "=== Building release binary (current platform) ==="
cargo build --release --features webui --bin webui

# ----- Bundle Windows -----
if [[ -n "${BUILD_WINDOWS:-}" ]] && [[ -f "target/release/webui.exe" ]]; then
    echo "=== Bundling Windows distribution ==="
    rm -rf "$DIST/windows" && mkdir -p "$DIST/windows"
    cp "target/release/webui.exe" "$DIST/windows/"
    for doc in README.md LICENSE CHANGELOG.md AGENTS.md .env.example; do
        [[ -f "$doc" ]] && cp "$doc" "$DIST/windows/"
    done
    cp "installers/install.ps1" "$DIST/windows/"
    # Create a single-file "self-install" .cmd wrapper that pulls webui.exe
    # out of the zip and runs the installer. Requires the user to keep
    # the zip intact.
    cat > "$DIST/windows/Install Volt.cmd" <<'EOF'
@echo off
echo Extracting Volt installer...
powershell -Command "Expand-Archive -Path '%~dp0volt-windows.zip' -DestinationPath '%TEMP%\volt-installer' -Force"
powershell -ExecutionPolicy Bypass -File '%TEMP%\volt-installer\install.ps1'
EOF
    # Zip everything
    cd "$DIST/windows"
    powershell -Command "Compress-Archive -Path '*' -DestinationPath '../volt-windows.zip' -Force"
    cd "$DIST"
    rm -rf "$DIST/windows"
    echo "  -> $DIST/volt-windows.zip"
fi

# ----- Bundle Linux -----
if [[ -n "${BUILD_LINUX:-}" ]] && [[ -f "target/release/webui" ]]; then
    echo "=== Bundling Linux distribution ==="
    rm -rf "$DIST/linux" && mkdir -p "$DIST/linux"
    cp "target/release/webui" "$DIST/linux/"
    for doc in README.md LICENSE CHANGELOG.md AGENTS.md .env.example; do
        [[ -f "$doc" ]] && cp "$doc" "$DIST/linux/"
    done
    cp "installers/install.sh" "$DIST/linux/"
    chmod +x "$DIST/linux/install.sh"
    cd "$DIST/linux"
    tar czf "../volt-linux-x86_64.tar.gz" .
    cd "$DIST"
    rm -rf "$DIST/linux"
    echo "  -> $DIST/volt-linux-x86_64.tar.gz"
fi

# ----- Bundle macOS -----
if [[ -n "${BUILD_MACOS:-}" ]] && [[ -f "target/release/webui" ]]; then
    echo "=== Bundling macOS distribution ==="
    rm -rf "$DIST/macos" && mkdir -p "$DIST/macos"
    cp "target/release/webui" "$DIST/macos/"
    for doc in README.md LICENSE CHANGELOG.md AGENTS.md .env.example; do
        [[ -f "$doc" ]] && cp "$doc" "$DIST/macos/"
    done
    cp "installers/install.sh" "$DIST/macos/"
    chmod +x "$DIST/macos/install.sh"
    cd "$DIST/macos"
    tar czf "../volt-macos-universal.tar.gz" .
    cd "$DIST"
    rm -rf "$DIST/macos"
    echo "  -> $DIST/volt-macos-universal.tar.gz"
fi

echo ""
echo "Done. Archives are in $DIST/"
ls -la "$DIST"/*.zip "$DIST"/*.tar.gz 2>/dev/null || true
