#!/usr/bin/env bash
set -e

REPO="mafly/routune"
COLOR_RESET="\033[0m"
COLOR_GREEN="\033[1;32m"
COLOR_CYAN="\033[1;36m"
COLOR_YELLOW="\033[1;33m"
COLOR_RED="\033[1;31m"

log_info() {
    printf "${COLOR_CYAN}[INFO]${COLOR_RESET} %s\n" "$1"
}

log_success() {
    printf "${COLOR_GREEN}[SUCCESS]${COLOR_RESET} %s\n" "$1"
}

log_warn() {
    printf "${COLOR_YELLOW}[WARN]${COLOR_RESET} %s\n" "$1"
}

log_error() {
    printf "${COLOR_RED}[ERROR]${COLOR_RESET} %s\n" "$1" >&2
}

show_help() {
    cat <<EOF
Routune Installer & Setup Script

Usage:
  ./install.sh [OPTIONS]
  curl -fsSL https://raw.githubusercontent.com/${REPO}/main/install.sh | bash

Options:
  --user         Install for current user only (default, no sudo needed)
  --system       Install system-wide (requires sudo)
  --uninstall    Remove all installed Routune binaries, services, and desktop files
  -h, --help     Show this help message
EOF
}

MODE="user"
UNINSTALL=false

for arg in "$@"; do
    case "$arg" in
        --system)
            MODE="system"
            ;;
        --user)
            MODE="user"
            ;;
        --uninstall)
            UNINSTALL=true
            ;;
        -h|--help)
            show_help
            exit 0
            ;;
        *)
            log_error "Unknown option: $arg"
            show_help
            exit 1
            ;;
    esac
done

# Setup installation paths
if [ "$MODE" = "system" ]; then
    BIN_DIR="/usr/local/bin"
    SERVICE_DIR="/etc/systemd/user"
    APP_DIR="/usr/share/applications"
    ICON_DIR="/usr/share/icons/hicolor/128x128/apps"
    SUDO="sudo"
else
    BIN_DIR="${HOME}/.local/bin"
    SERVICE_DIR="${HOME}/.config/systemd/user"
    APP_DIR="${HOME}/.local/share/applications"
    ICON_DIR="${HOME}/.local/share/icons/hicolor/128x128/apps"
    SUDO=""
fi

do_uninstall() {
    log_info "Uninstalling Routune..."

    if command -v systemctl >/dev/null 2>&1; then
        systemctl --user stop routune.service 2>/dev/null || true
        systemctl --user disable routune.service 2>/dev/null || true
    fi

    $SUDO rm -f "${BIN_DIR}/routune-daemon" "${BIN_DIR}/routune-gui"
    $SUDO rm -f "${SERVICE_DIR}/routune.service"
    $SUDO rm -f "${APP_DIR}/routune.desktop"
    $SUDO rm -f "${ICON_DIR}/routune.png"

    if command -v systemctl >/dev/null 2>&1; then
        systemctl --user daemon-reload 2>/dev/null || true
    fi

    if command -v update-desktop-database >/dev/null 2>&1; then
        update-desktop-database "${APP_DIR}" 2>/dev/null || true
    fi

    log_success "Routune successfully uninstalled."
    exit 0
}

if [ "$UNINSTALL" = true ]; then
    do_uninstall
fi

# Pre-flight system check
ARCH="$(uname -m)"
if [ "$ARCH" != "x86_64" ]; then
    log_error "Unsupported architecture: $ARCH. Routune currently provides binaries for x86_64."
    exit 1
fi

if ! command -v pipewire >/dev/null 2>&1; then
    log_warn "PipeWire was not found in your PATH."
    log_warn "Please ensure PipeWire and WirePlumber are installed for Routune to function."
fi

# Locate source directory or download from GitHub release
TEMP_DIR=""
cleanup() {
    if [ -n "$TEMP_DIR" ] && [ -d "$TEMP_DIR" ]; then
        rm -rf "$TEMP_DIR"
    fi
}
trap cleanup EXIT

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" 2>/dev/null && pwd || echo "")"
LOCAL_SOURCE=false

if [ -n "$SCRIPT_DIR" ] && [ -f "${SCRIPT_DIR}/target/release/routune-daemon" ] && [ -f "${SCRIPT_DIR}/target/release/routune-gui" ]; then
    SRC_DAEMON="${SCRIPT_DIR}/target/release/routune-daemon"
    SRC_GUI="${SCRIPT_DIR}/target/release/routune-gui"
    SRC_SERVICE="${SCRIPT_DIR}/packaging/routune.service"
    SRC_DESKTOP="${SCRIPT_DIR}/packaging/routune.desktop"
    SRC_ICON="${SCRIPT_DIR}/packaging/icons/routune.png"
    LOCAL_SOURCE=true
elif [ -n "$SCRIPT_DIR" ] && [ -f "${SCRIPT_DIR}/routune-daemon" ] && [ -f "${SCRIPT_DIR}/routune-gui" ]; then
    SRC_DAEMON="${SCRIPT_DIR}/routune-daemon"
    SRC_GUI="${SCRIPT_DIR}/routune-gui"
    SRC_SERVICE="${SCRIPT_DIR}/routune.service"
    SRC_DESKTOP="${SCRIPT_DIR}/routune.desktop"
    SRC_ICON="${SCRIPT_DIR}/routune.png"
    LOCAL_SOURCE=true
fi

if [ "$LOCAL_SOURCE" = false ]; then
    log_info "Fetching latest release archive from https://github.com/${REPO}..."
    TEMP_DIR="$(mktemp -d /tmp/routune-install-XXXXXX)"

    TARBALL_URL="https://github.com/${REPO}/releases/latest/download/routune-linux-x86_64.tar.gz"
    if ! curl -fsSL "$TARBALL_URL" -o "${TEMP_DIR}/routune.tar.gz"; then
        log_error "Failed to download release archive from $TARBALL_URL."
        log_error "Ensure a published release exists on https://github.com/${REPO}/releases."
        exit 1
    fi

    tar -xzf "${TEMP_DIR}/routune.tar.gz" -C "$TEMP_DIR"
    SRC_DAEMON="${TEMP_DIR}/routune-daemon"
    SRC_GUI="${TEMP_DIR}/routune-gui"
    SRC_SERVICE="${TEMP_DIR}/routune.service"
    SRC_DESKTOP="${TEMP_DIR}/routune.desktop"
    SRC_ICON="${TEMP_DIR}/routune.png"
fi

# Ensure target directories exist
$SUDO mkdir -p "$BIN_DIR" "$SERVICE_DIR" "$APP_DIR" "$ICON_DIR"

log_info "Installing binaries into ${BIN_DIR}..."
$SUDO install -m 755 "$SRC_DAEMON" "${BIN_DIR}/routune-daemon"
$SUDO install -m 755 "$SRC_GUI" "${BIN_DIR}/routune-gui"

# Install Desktop Entry and Icon
log_info "Installing desktop integration and icons..."
if [ -f "$SRC_ICON" ]; then
    $SUDO install -m 644 "$SRC_ICON" "${ICON_DIR}/routune.png"
fi

if [ -f "$SRC_DESKTOP" ]; then
    $SUDO install -m 644 "$SRC_DESKTOP" "${APP_DIR}/routune.desktop"
    if command -v update-desktop-database >/dev/null 2>&1; then
        update-desktop-database "$APP_DIR" 2>/dev/null || true
    fi
fi

# Install and configure systemd user service
log_info "Configuring systemd user service..."
if [ -f "$SRC_SERVICE" ]; then
    TMP_SVC="$(mktemp /tmp/routune.service.XXXXXX)"
    sed "s|%h/.local/bin/routune-daemon|${BIN_DIR}/routune-daemon|g" "$SRC_SERVICE" > "$TMP_SVC"
    $SUDO install -m 644 "$TMP_SVC" "${SERVICE_DIR}/routune.service"
    rm -f "$TMP_SVC"

    if command -v systemctl >/dev/null 2>&1; then
        log_info "Reloading systemd user daemon and starting routune.service..."
        systemctl --user daemon-reload || true
        systemctl --user enable --now routune.service || true
    fi
fi

# Verify user PATH
case ":$PATH:" in
    *":${BIN_DIR}:"*) ;;
    *)
        log_warn "${BIN_DIR} is not in your \$PATH."
        log_warn "Add 'export PATH=\"${BIN_DIR}:\$PATH\"' to your ~/.bashrc or ~/.zshrc."
        ;;
esac

log_success "Routune installed and started successfully!"
log_info "Run 'routune-gui' from terminal or launch 'Routune' from application menu."
