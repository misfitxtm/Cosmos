#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Builds Cosmos and installs it into your home directory. It never uses sudo.
#   ./install.sh                 build and install (command line + window)
#   ./install.sh --cli-only      skip the window, so GTK is not needed
#   ./install.sh --prefix DIR    install under DIR instead of ~/.local
#   ./install.sh --uninstall     remove what this script installed
set -euo pipefail

die() { echo "install.sh: $*" >&2; exit 1; }

PREFIX="${HOME:-}/.local"
CLI_ONLY=0
UNINSTALL=0
while [ $# -gt 0 ]; do
    case "$1" in
        --prefix) [ $# -ge 2 ] || die "--prefix needs a directory"; PREFIX=$2; shift 2 ;;
        --cli-only) CLI_ONLY=1; shift ;;
        --uninstall) UNINSTALL=1; shift ;;
        -h|--help) sed -n '3,7p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) die "unknown option: $1 (try --help)" ;;
    esac
done

[ "$(id -u)" -ne 0 ] || die "do not run this as root; it installs into your home directory"
[ -n "${HOME:-}" ] || die "HOME is not set"
# The prefix is pasted into the desktop file, so only plain path characters are allowed
[[ $PREFIX =~ ^/[A-Za-z0-9._/-]+$ ]] || die "--prefix must be an absolute path using only letters, digits and . _ / -"

cd "$(dirname "$(readlink -f "$0")")"
[ -f Cargo.toml ] && [ -d crates/cosmos-core ] || die "run this from a Cosmos checkout"

BIN="$PREFIX/bin"
ICONS="$PREFIX/share/icons/hicolor/scalable/apps"
APPS="$PREFIX/share/applications"
APP_ID=io.github.Misfitxtm.Cosmos
DESKTOP=$APP_ID.desktop

refresh_caches() {
    command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database -q "$APPS" 2>/dev/null || true
}

if [ "$UNINSTALL" -eq 1 ]; then
    rm -f "$BIN/cosmos" "$BIN/cosmos-gtk" "$ICONS/$APP_ID.svg" "$APPS/$DESKTOP"
    refresh_caches
    echo "Removed Cosmos from $PREFIX."
    echo "Your settings in ~/.config/cosmos and any themes you saved were left alone."
    exit 0
fi

command -v cargo >/dev/null 2>&1 || die "cargo was not found. On Fedora: sudo dnf install cargo gcc"
PACKAGES=(-p cosmos-cli)
if [ "$CLI_ONLY" -eq 0 ]; then
    pkg-config --exists gtk4 2>/dev/null \
        || die "the GTK 4 development files were not found. On Fedora: sudo dnf install gtk4-devel (or use --cli-only)"
    PACKAGES+=(-p cosmos-ui)
fi

echo "Building (this takes a minute the first time)..."
cargo build --release --locked "${PACKAGES[@]}"

install -Dm755 target/release/cosmos "$BIN/cosmos"
if [ "$CLI_ONLY" -eq 0 ]; then
    install -Dm755 target/release/cosmos-gtk "$BIN/cosmos-gtk"
    install -Dm644 crates/cosmos-cli/assets/cosmos.svg "$ICONS/$APP_ID.svg"
    # The menu entry gets the full path, because menus often start apps without ~/.local/bin on PATH
    tmp=$(mktemp)
    trap 'rm -f "$tmp"' EXIT
    sed "s|@BINDIR@|$BIN|g" "data/$DESKTOP" > "$tmp"
    install -Dm644 "$tmp" "$APPS/$DESKTOP"
    refresh_caches
fi

echo "Installed to $PREFIX."
case ":$PATH:" in
    *":$BIN:"*) ;;
    *) echo "Note: $BIN is not on your PATH, so add it to run 'cosmos' from a terminal." ;;
esac
echo "Try: cosmos docs/demo-wallpaper.png"
