#!/bin/bash
# NeoMind Server Installation Script
# Usage: curl -fsSL https://github.com/camthink-ai/NeoMind/releases/download/v0.5.8/install.sh | bash

set -e

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Configuration
VERSION="${VERSION:-0.5.8}"
REPO="camthink-ai/NeoMind"
BINARY_NAME="neomind"
INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"
DATA_DIR="${DATA_DIR:-/var/lib/neomind}"
SERVICE_NAME="neomind"

# Detect architecture
ARCH=$(uname -m)
case $ARCH in
    x86_64)
        BINARY_ARCH="amd64"
        ;;
    aarch64|arm64)
        BINARY_ARCH="arm64"
        ;;
    *)
        echo -e "${RED}Unsupported architecture: $ARCH${NC}"
        exit 1
        ;;
esac

# Detect OS
OS=$(uname -s)
case $OS in
    Linux)
        BINARY_OS="linux"
        ;;
    *)
        echo -e "${RED}Unsupported OS: $OS${NC}"
        exit 1
        ;;
esac

BINARY_FILE="neomind-server-${BINARY_OS}-${BINARY_ARCH}"
DOWNLOAD_URL="https://github.com/${REPO}/releases/download/v${VERSION}/${BINARY_FILE}.tar.gz"

echo -e "${GREEN}NeoMind Server Installer${NC}"
echo "Version: ${VERSION}"
echo "Architecture: ${BINARY_ARCH}"
echo ""

# Check if running as root
if [ "$EUID" -ne 0 ]; then
    echo -e "${YELLOW}Note: This script requires root privileges for system-wide installation${NC}"
    echo "Please run with sudo or set INSTALL_DIR to a user-writable location"
    echo ""
fi

# Create data directory
echo "Creating data directory at ${DATA_DIR}..."
sudo mkdir -p "${DATA_DIR}"
sudo chown -R $USER:${USER} "${DATA_DIR}" 2>/dev/null || true

# Download binary
echo "Downloading ${DOWNLOAD_URL}..."
TMP_DIR=$(mktemp -d)
cd "$TMP_DIR"

curl -fsSL "${DOWNLOAD_URL}" -o neomind.tar.gz || {
    echo -e "${RED}Failed to download binary${NC}"
    echo "Please check the version and try again"
    rm -rf "$TMP_DIR"
    exit 1
}

# Extract
echo "Extracting..."
tar xzf neomind.tar.gz

# Install binary
echo "Installing binary to ${INSTALL_DIR}..."
sudo install -m 755 "${BINARY_NAME}" "${INSTALL_DIR}/neomind" || {
    echo -e "${YELLOW}Could not install to ${INSTALL_DIR}, trying /usr/bin${NC}"
    sudo install -m 755 "${BINARY_NAME}" /usr/bin/neomind
}

# Cleanup
rm -rf "$TMP_DIR"

# Create systemd service
echo "Creating systemd service..."
cat /tmp/neomind.service <<EOF
[Unit]
Description=NeoMind Server
After=network.target

[Service]
Type=simple
User=neomind
Group=neomind
WorkingDirectory=${DATA_DIR}
ExecStart=${INSTALL_DIR}/neomind
Restart=always
RestartSec=5
Environment=RUST_LOG=info
Environment=NEOMIND_DATA_DIR=${DATA_DIR}

# Security
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=${DATA_DIR}

[Install]
WantedBy=multi-user.target
EOF

# Create user
if ! id -u neomind &>/dev/null; then
    echo "Creating neomind user..."
    sudo useradd -r -s /bin/false -d ${DATA_DIR} neomind || true
fi

# Install service
sudo mv /tmp/neomind.service /etc/systemd/system/neomind.service
sudo systemctl daemon-reload

echo ""
echo -e "${GREEN}Installation complete!${NC}"
echo ""
echo "To start the service:"
echo "  sudo systemctl start neomind"
echo ""
echo "To enable on boot:"
echo "  sudo systemctl enable neomind"
echo ""
echo "To check status:"
echo "  sudo systemctl status neomind"
echo ""
echo "View logs:"
echo "  sudo journalctl -u neomind -f"
echo ""
echo "The API will be available at: http://localhost:9375"
