#!/bin/bash

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${GREEN}======================================${NC}"
echo -e "${GREEN}  CapSlap FFmpeg Auto-Downloader${NC}"
echo -e "${GREEN}======================================${NC}"
echo ""

# Determine platform and architecture
OS="$(uname -s)"
ARCH="$(uname -m)"

BIN_DIR="$(dirname "$0")/../rust/bin"
mkdir -p "$BIN_DIR"

# Detect platform
if [[ "$OS" == "Darwin" ]]; then
    PLATFORM="macos"
    if [[ "$ARCH" == "arm64" ]]; then
        ARCH_NAME="arm64"
        FFMPEG_URL="https://evermeet.cx/ffmpeg/getrelease/ffmpeg/zip"
        FFPROBE_URL="https://evermeet.cx/ffmpeg/getrelease/ffprobe/zip"
    else
        ARCH_NAME="x64"
        FFMPEG_URL="https://evermeet.cx/ffmpeg/getrelease/ffmpeg/zip"
        FFPROBE_URL="https://evermeet.cx/ffmpeg/getrelease/ffprobe/zip"
    fi
elif [[ "$OS" == "Linux" ]]; then
    PLATFORM="linux"
    ARCH_NAME="x64"
    echo -e "${YELLOW}Linux detected. Please install ffmpeg manually:${NC}"
    echo "  sudo apt install ffmpeg  # Ubuntu/Debian"
    echo "  sudo dnf install ffmpeg  # Fedora"
    exit 0
elif [[ "$OS" == MINGW* ]] || [[ "$OS" == MSYS* ]]; then
    PLATFORM="windows"
    ARCH_NAME="x64"
    echo -e "${YELLOW}Windows detected. Please download ffmpeg manually from:${NC}"
    echo "  https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip"
    exit 0
else
    echo -e "${RED}Unsupported platform: $OS${NC}"
    exit 1
fi

echo -e "${YELLOW}Platform:${NC} $PLATFORM $ARCH_NAME"
echo ""

# Check if ffmpeg already exists
if [[ -f "$BIN_DIR/ffmpeg" ]] && [[ -f "$BIN_DIR/ffprobe" ]]; then
    echo -e "${GREEN}✓ FFmpeg and FFprobe already installed!${NC}"
    "$BIN_DIR/ffmpeg" -version | head -n 1
    exit 0
fi

# Download for macOS
if [[ "$PLATFORM" == "macos" ]]; then
    echo -e "${YELLOW}Downloading FFmpeg for macOS...${NC}"

    # Download ffmpeg
    if [[ ! -f "$BIN_DIR/ffmpeg" ]]; then
        echo "  Downloading ffmpeg..."
        curl -L -o "$BIN_DIR/ffmpeg.zip" "$FFMPEG_URL" --progress-bar
        echo "  Extracting ffmpeg..."
        unzip -q "$BIN_DIR/ffmpeg.zip" -d "$BIN_DIR/"
        rm "$BIN_DIR/ffmpeg.zip"
        chmod +x "$BIN_DIR/ffmpeg"
        echo -e "${GREEN}  ✓ ffmpeg installed${NC}"
    fi

    # Download ffprobe
    if [[ ! -f "$BIN_DIR/ffprobe" ]]; then
        echo "  Downloading ffprobe..."
        curl -L -o "$BIN_DIR/ffprobe.zip" "$FFPROBE_URL" --progress-bar
        echo "  Extracting ffprobe..."
        unzip -q "$BIN_DIR/ffprobe.zip" -d "$BIN_DIR/"
        rm "$BIN_DIR/ffprobe.zip"
        chmod +x "$BIN_DIR/ffprobe"
        echo -e "${GREEN}  ✓ ffprobe installed${NC}"
    fi
fi

echo ""
echo -e "${GREEN}======================================${NC}"
echo -e "${GREEN}  Installation Complete!${NC}"
echo -e "${GREEN}======================================${NC}"
echo ""
"$BIN_DIR/ffmpeg" -version | head -n 1
echo ""
