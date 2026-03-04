#!/bin/sh
# NeoMind Installation Script
# Usage: curl -fsSL https://raw.githubusercontent.com/camthink-ai/NeoMind/main/scripts/install.sh | sh
#
# Environment variables:
#   VERSION        - Specific version to install (default: latest)
#   INSTALL_DIR    - Installation directory (default: /usr/local/bin)
#   DATA_DIR       - Data directory (default: /var/lib/neomind)
#   NO_SERVICE     - Skip service installation (default: false)

set -eu

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
BOLD='\033[1m'
NC='\033[0m'

# Configuration
REPO="camthink-ai/NeoMind"
VERSION="${VERSION:-}"
INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"
DATA_DIR="${DATA_DIR:-/var/lib/neomind}"
NO_SERVICE="${NO_SERVICE:-false}"

status() { echo "${BLUE}[INFO]${NC} $*"; }
success() { echo "${GREEN}[OK]${NC} $*"; }
warning() { echo "${YELLOW}[WARN]${NC} $*"; }
error() { echo "${RED}[ERROR]${NC} $*" >&2; exit 1; }

cleanup() {
    if [ -n "${TEMP_DIR:-}" ] && [ -d "$TEMP_DIR" ]; then
        rm -rf "$TEMP_DIR"
    fi
}
trap cleanup EXIT

available() { command -v "$1" >/dev/null 2>&1; }

require() {
    local MISSING=''
    for TOOL in "$@"; do
        if ! available "$TOOL"; then
            MISSING="$MISSING $TOOL"
        fi
    done
    if [ -n "$MISSING" ]; then
        error "Missing required tools:$MISSING. Please install them first."
    fi
}

get_os() {
    OS=$(uname -s)
    case "$OS" in
        Darwin) OS="darwin" ;;
        Linux) OS="linux" ;;
        *) error "Unsupported OS: $OS" ;;
    esac
}

get_arch() {
    ARCH=$(uname -m)
    case "$ARCH" in
        x86_64|amd64) ARCH="amd64" ;;
        aarch64|arm64) ARCH="arm64" ;;
        *) error "Unsupported architecture: $ARCH" ;;
    esac
}

get_latest_version() {
    status "Fetching latest version..."
    VERSION=$(curl -sfL https://api.github.com/repos/${REPO}/releases/latest | 
              grep '"tag_name":' | sed -E 's/.*"v([^"]+)".*/\1/')
    if [ -z "$VERSION" ]; then
        error "Failed to fetch latest version from GitHub"
    fi
}

detect_sudo() {
    if [ "$(id -u)" -ne 0 ]; then
        if available sudo; then
            SUDO="sudo"
        else
            error "This script requires root privileges. Please run with sudo or as root."
        fi
    else
        SUDO=""
    fi
}

install_linux() {
    status "Installing NeoMind on Linux..."
    
    # Create user if not exists
    if ! id -u neomind >/dev/null 2>&1; then
        status "Creating neomind user..."
        $SUDO useradd -r -s /bin/false -d "$DATA_DIR" neomind 2>/dev/null || true
    fi
    
    # Create directories
    status "Creating directories..."
    $SUDO mkdir -p "$INSTALL_DIR"
    $SUDO mkdir -p "$DATA_DIR"
    $SUDO chown -R neomind:neomind "$DATA_DIR"
    
    # Download and extract
    BINARY_FILE="neomind-server-linux-${ARCH}.tar.gz"
    DOWNLOAD_URL="https://github.com/${REPO}/releases/download/v${VERSION}/${BINARY_FILE}"
    
    status "Downloading NeoMind v${VERSION} for ${OS}/${ARCH}..."
    TEMP_DIR=$(mktemp -d)
    
    if ! curl -fSL --progress-bar "$DOWNLOAD_URL" -o "$TEMP_DIR/neomind.tar.gz"; then
        error "Failed to download from $DOWNLOAD_URL"
    fi
    
    status "Extracting..."
    tar xzf "$TEMP_DIR/neomind.tar.gz" -C "$TEMP_DIR"
    
    # Install binary
    status "Installing binary to $INSTALL_DIR..."
    $SUDO install -m 755 "$TEMP_DIR/neomind" "$INSTALL_DIR/neomind"
    
    # Install extension runner if present
    if [ -f "$TEMP_DIR/neomind-extension-runner" ]; then
        $SUDO install -m 755 "$TEMP_DIR/neomind-extension-runner" "$INSTALL_DIR/neomind-extension-runner"
        success "Extension runner installed"
    fi
    
    # Install systemd service
    if [ "$NO_SERVICE" != "true" ]; then
        status "Installing systemd service..."
        $SUDO tee /etc/systemd/system/neomind.service >/dev/null <<EOF
[Unit]
Description=NeoMind Edge AI Platform
Documentation=https://github.com/camthink-ai/NeoMind
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=neomind
Group=neomind
WorkingDirectory=${DATA_DIR}
ExecStart=${INSTALL_DIR}/neomind
Restart=always
RestartSec=3
TimeoutStopSec=30

# Environment
Environment=RUST_LOG=info
Environment=NEOMIND_DATA_DIR=${DATA_DIR}
Environment=NEOMIND_BIND_ADDR=0.0.0.0:9375

# Security hardening
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=${DATA_DIR}

[Install]
WantedBy=multi-user.target
EOF
        $SUDO systemctl daemon-reload
        success "Systemd service installed"
    fi
    
    success "Installation complete!"
}

install_darwin() {
    status "Installing NeoMind on macOS..."
    
    # Create directories
    mkdir -p "$INSTALL_DIR"
    mkdir -p "$DATA_DIR"
    
    # Download and extract
    BINARY_FILE="neomind-server-darwin-${ARCH}.tar.gz"
    DOWNLOAD_URL="https://github.com/${REPO}/releases/download/v${VERSION}/${BINARY_FILE}"
    
    status "Downloading NeoMind v${VERSION} for ${OS}/${ARCH}..."
    TEMP_DIR=$(mktemp -d)
    
    if ! curl -fSL --progress-bar "$DOWNLOAD_URL" -o "$TEMP_DIR/neomind.tar.gz"; then
        error "Failed to download from $DOWNLOAD_URL"
    fi
    
    status "Extracting..."
    tar xzf "$TEMP_DIR/neomind.tar.gz" -C "$TEMP_DIR"
    
    # Install binary
    status "Installing binary to $INSTALL_DIR..."
    install -m 755 "$TEMP_DIR/neomind" "$INSTALL_DIR/neomind"
    
    # Install extension runner if present
    if [ -f "$TEMP_DIR/neomind-extension-runner" ]; then
        install -m 755 "$TEMP_DIR/neomind-extension-runner" "$INSTALL_DIR/neomind-extension-runner"
        success "Extension runner installed"
    fi
    
    # Create launchd plist for macOS
    if [ "$NO_SERVICE" != "true" ]; then
        status "Installing launchd service..."
        PLIST_PATH="$HOME/Library/LaunchAgents/com.neomind.server.plist"
        mkdir -p "$(dirname "$PLIST_PATH")"
        
        cat > "$PLIST_PATH" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.neomind.server</string>
    <key>ProgramArguments</key>
    <array>
        <string>${INSTALL_DIR}/neomind</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>EnvironmentVariables</key>
    <dict>
        <key>RUST_LOG</key>
        <string>info</string>
        <key>NEOMIND_DATA_DIR</key>
        <string>${DATA_DIR}</string>
        <key>NEOMIND_BIND_ADDR</key>
        <string>0.0.0.0:9375</string>
    </dict>
    <key>StandardOutPath</key>
    <string>${DATA_DIR}/neomind.log</string>
    <key>StandardErrorPath</key>
    <string>${DATA_DIR}/neomind.log</string>
</dict>
</plist>
EOF
        success "Launchd service installed"
    fi
    
    success "Installation complete!"
}

print_post_install() {
    echo ""
    echo "${BOLD}═══════════════════════════════════════════════════════════${NC}"
    echo "${BOLD}  NeoMind v${VERSION} installed successfully!${NC}"
    echo "${BOLD}═══════════════════════════════════════════════════════════${NC}"
    echo ""
    echo "Binary location: ${INSTALL_DIR}/neomind"
    echo "Data directory:  ${DATA_DIR}"
    echo ""
    echo "${GREEN}✨ Web UI is embedded - no additional setup required!${NC}"
    echo ""
    
    if [ "$OS" = "linux" ]; then
        if [ "$NO_SERVICE" = "true" ]; then
            echo "To start NeoMind:"
            echo "  ${INSTALL_DIR}/neomind"
            echo ""
            echo "Then open: ${BOLD}http://localhost:9375${NC}"
        else
            echo "Starting NeoMind service..."
            $SUDO systemctl start neomind || true
            
            echo ""
            echo "Service commands:"
            echo "  Status:  ${SUDO} systemctl status neomind"
            echo "  Stop:    ${SUDO} systemctl stop neomind"
            echo "  Restart: ${SUDO} systemctl restart neomind"
            echo "  Logs:    ${SUDO} journalctl -u neomind -f"
            echo ""
            echo "Access the application:"
            echo "  Web UI:  ${BOLD}http://localhost:9375${NC}"
            echo "  API:     http://localhost:9375/api"
            echo "  Docs:    http://localhost:9375/api/docs"
        fi
    elif [ "$OS" = "darwin" ]; then
        if [ "$NO_SERVICE" = "true" ]; then
            echo "To start NeoMind:"
            echo "  ${INSTALL_DIR}/neomind"
            echo ""
            echo "Then open: ${BOLD}http://localhost:9375${NC}"
        else
            echo "Starting NeoMind service..."
            launchctl load ~/Library/LaunchAgents/com.neomind.server.plist 2>/dev/null || true
            
            echo ""
            echo "Service commands:"
            echo "  Stop:   launchctl unload ~/Library/LaunchAgents/com.neomind.server.plist"
            echo "  Start:  launchctl load ~/Library/LaunchAgents/com.neomind.server.plist"
            echo "  Logs:   tail -f ${DATA_DIR}/neomind.log"
            echo ""
            echo "Access the application:"
            echo "  Web UI:  ${BOLD}http://localhost:9375${NC}"
            echo "  API:     http://localhost:9375/api"
            echo "  Docs:    http://localhost:9375/api/docs"
        fi
    fi
    
    echo ""
    echo "Documentation: https://github.com/camthink-ai/NeoMind"
    echo ""
}

main() {
    echo ""
    echo "${BOLD}╔═══════════════════════════════════════════════════════════╗${NC}"
    echo "${BOLD}║           NeoMind Edge AI Platform Installer             ║${NC}"
    echo "${BOLD}╚═══════════════════════════════════════════════════════════╝${NC}"
    echo ""
    
    # Check dependencies
    require curl
    
    # Detect system
    get_os
    get_arch
    status "Detected: ${OS}/${ARCH}"
    
    # Get version
    if [ -z "$VERSION" ]; then
        get_latest_version
    fi
    status "Installing version: ${VERSION}"
    
    # Detect sudo for Linux
    if [ "$OS" = "linux" ]; then
        detect_sudo
    fi
    
    # Install
    case "$OS" in
        linux) install_linux ;;
        darwin) install_darwin ;;
    esac
    
    print_post_install
}

main "$@"