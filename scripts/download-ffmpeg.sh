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

    # Builds come from ffmpeg.martin-riedl.de: static (system frameworks only,
    # so they are safe to bundle), published for both architectures, and built
    # --enable-libass, which CapSlap needs to burn subtitles.
    #
    # The previous source, evermeet.cx, only publishes x86_64. Apple Silicon
    # machines therefore got a binary the app refuses to run, silently fell back
    # to whatever ffmpeg was on PATH, and failed to burn captions if that build
    # had no libass.
    if [[ "$ARCH" == "arm64" ]]; then
        ARCH_NAME="arm64"
    else
        ARCH_NAME="amd64"
    fi

    BASE_URL="https://ffmpeg.martin-riedl.de/redirect/latest/macos/$ARCH_NAME/release"
    FFMPEG_URL="$BASE_URL/ffmpeg.zip"
    FFPROBE_URL="$BASE_URL/ffprobe.zip"
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

# Does this binary run on this machine? Checked against the Mach-O header rather
# than by executing it, because Rosetta will happily run an x86_64 build that
# the app itself skips — which is how a wrong-architecture download went
# unnoticed before.
runs_natively() {
    local bin="$1"
    [[ -x "$bin" ]] || return 1

    local archs
    if archs="$(lipo -archs "$bin" 2>/dev/null)" && [[ -n "$archs" ]]; then
        tr ' ' '\n' <<<"$archs" | grep -qx "$ARCH"
    else
        # lipo is an Xcode shim and needs the developer tools. `file` ships with
        # macOS itself and names every architecture in thin and universal
        # binaries alike; -L so a symlinked binary is inspected, not the link.
        file -bL "$bin" | grep -qw "$ARCH"
    fi
}

# Can this binary burn subtitles? Without libass the ass filter is missing, and
# every export fails no matter which subtitle format we hand it.
has_libass() {
    "$1" -hide_banner -filters 2>/dev/null | grep -qE "^ [A-Z.]+ ass +V->V"
}

# Install one binary, then prove it is usable before accepting it.
install_binary() {
    local name="$1" url="$2" needs_libass="$3"
    local target="$BIN_DIR/$name"

    if runs_natively "$target" && { [[ "$needs_libass" != "yes" ]] || has_libass "$target"; }; then
        echo -e "${GREEN}  ✓ $name already installed and usable${NC}"
        return 0
    fi

    if [[ -e "$target" ]]; then
        echo -e "${YELLOW}  Replacing unusable $name (wrong architecture or built without libass)${NC}"
        rm -f "$target"
    fi

    # Each step is checked by hand: errexit is disabled inside a function whose
    # result is tested by the caller, so a failed download would otherwise fall
    # through to unzip and report a misleading reason.
    echo "  Downloading $name..."
    if ! curl -fL -o "$BIN_DIR/$name.zip" "$url" --progress-bar; then
        echo -e "${RED}  ✗ Could not download $name from $url${NC}"
        rm -f "$BIN_DIR/$name.zip"
        return 1
    fi

    echo "  Extracting $name..."
    if ! unzip -oq "$BIN_DIR/$name.zip" -d "$BIN_DIR/"; then
        echo -e "${RED}  ✗ Could not extract $name${NC}"
        rm -f "$BIN_DIR/$name.zip"
        return 1
    fi
    rm -f "$BIN_DIR/$name.zip"
    chmod +x "$target"

    if ! runs_natively "$target"; then
        echo -e "${RED}  ✗ Downloaded $name is not built for $ARCH${NC}"
        return 1
    fi
    if [[ "$needs_libass" == "yes" ]] && ! has_libass "$target"; then
        echo -e "${RED}  ✗ Downloaded $name was built without libass${NC}"
        return 1
    fi

    echo -e "${GREEN}  ✓ $name installed${NC}"
}

if ! install_binary ffmpeg "$FFMPEG_URL" yes || ! install_binary ffprobe "$FFPROBE_URL" no; then
    echo ""
    echo -e "${RED}FFmpeg setup failed.${NC} CapSlap cannot burn captions without an"
    echo -e "${ARCH} ffmpeg built with libass. To provide one yourself:"
    echo ""
    echo "  brew install ffmpeg-full"
    echo "  FULL=\$(brew --prefix ffmpeg-full)"
    echo "  ln -sf \"\$FULL/bin/ffmpeg\"  $BIN_DIR/ffmpeg"
    echo "  ln -sf \"\$FULL/bin/ffprobe\" $BIN_DIR/ffprobe"
    echo ""
    echo -e "${YELLOW}Note:${NC} the plain 'ffmpeg' formula is a slim build with no libass."
    exit 1
fi

echo ""
echo -e "${GREEN}======================================${NC}"
echo -e "${GREEN}  Installation Complete!${NC}"
echo -e "${GREEN}======================================${NC}"
echo ""
"$BIN_DIR/ffmpeg" -version | head -n 1
echo -e "${GREEN}Subtitle burning (libass): available${NC}"
echo ""
