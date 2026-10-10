#!/bin/bash
set -e

# Download platform-specific FFmpeg and whisper-cli binaries for CI builds
# Usage: TARGET_OS=macOS|Windows|Linux bash scripts/download-binaries.sh

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"

# FFmpeg versions
FFMPEG_VERSION="7.1"

# Whisper.cpp release
WHISPER_VERSION="v1.7.4"

download_macos() {
    echo "Downloading macOS binaries..."

    BIN_DIR="$ROOT_DIR/rust/bin"
    mkdir -p "$BIN_DIR/lib"

    # ffmpeg/ffprobe are .gitignored (too large / machine-specific to
    # commit), so they're always missing on a fresh checkout.
    if [[ -f "$BIN_DIR/ffmpeg" && -f "$BIN_DIR/ffprobe" ]]; then
        echo "ffmpeg/ffprobe already present, skipping download"
    else
        # evermeet.cx's static builds are x86_64-only. On Apple Silicon that
        # only runs under Rosetta 2, which isn't installed on GitHub's arm64
        # runners (or guaranteed on an end user's Mac) — binary_runnable()'s
        # arch check correctly refuses to use it there, so ffmpeg silently
        # "isn't found" despite the file existing. Homebrew's `ffmpeg-full`
        # (homebrew-core, keg-only — not `ffmpeg`, which lacks libass) is
        # native for whatever architecture Homebrew itself is running as,
        # and has libass (subtitle burning) built in.
        echo "Installing FFmpeg via Homebrew (arch-native, has libass)..."
        brew list ffmpeg-full &>/dev/null || brew install ffmpeg-full
        FFMPEG_PREFIX="$(brew --prefix ffmpeg-full)"
        cp -L "$FFMPEG_PREFIX/bin/ffmpeg" "$BIN_DIR/ffmpeg"
        cp -L "$FFMPEG_PREFIX/bin/ffprobe" "$BIN_DIR/ffprobe"
        chmod +x "$BIN_DIR/ffmpeg" "$BIN_DIR/ffprobe"
    fi

    # whisper-cli-macos-arm64 (and its runtime dylibs under bin/lib/) ARE
    # committed to git — built locally without the CoreML requirement the
    # upstream v1.7.4 release asset has, which otherwise fails at runtime
    # whenever the matching CoreML-converted model isn't also present.
    # Never overwrite that with the older release asset if it's already
    # there; only fetch it as a fallback for an architecture we haven't
    # built for ourselves.
    ARCH=$(uname -m)
    if [[ "$ARCH" == "arm64" ]]; then
        WHISPER_ASSET="whisper-cli-macos-arm64"
    else
        WHISPER_ASSET="whisper-cli-macos-x64"
    fi

    if [[ -f "$BIN_DIR/$WHISPER_ASSET" ]]; then
        echo "$WHISPER_ASSET already present (committed build), skipping download"
    else
        echo "Downloading whisper-cli ($WHISPER_ASSET)..."
        curl -L "https://github.com/ggerganov/whisper.cpp/releases/download/${WHISPER_VERSION}/${WHISPER_ASSET}" \
            -o "$BIN_DIR/${WHISPER_ASSET}"
        chmod +x "$BIN_DIR/${WHISPER_ASSET}"
    fi

    echo "macOS binaries ready"
}

download_windows() {
    echo "Downloading Windows binaries..."
    
    BIN_DIR="$ROOT_DIR/rust/bin-win"
    mkdir -p "$BIN_DIR/lib"
    
    # Check if already exists
    if [[ -f "$BIN_DIR/ffmpeg.exe" && -f "$BIN_DIR/whisper-cli.exe" ]]; then
        echo "Windows binaries already exist, skipping download"
        return
    fi
    
    # Download FFmpeg from gyan.dev (Windows builds with all codecs)
    echo "Downloading FFmpeg..."
    FFMPEG_URL="https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-gpl.zip"
    curl -L "$FFMPEG_URL" -o /tmp/ffmpeg-win.zip
    unzip -o /tmp/ffmpeg-win.zip -d /tmp/ffmpeg-win
    
    # Find and copy binaries
    find /tmp/ffmpeg-win -name "ffmpeg.exe" -exec cp {} "$BIN_DIR/" \;
    find /tmp/ffmpeg-win -name "ffprobe.exe" -exec cp {} "$BIN_DIR/" \;
    
    # Download whisper.cpp Windows build
    echo "Downloading whisper-cli..."
    curl -L "https://github.com/ggerganov/whisper.cpp/releases/download/${WHISPER_VERSION}/whisper-cli-win64.exe" \
        -o "$BIN_DIR/whisper-cli.exe"
    
    echo "Windows binaries downloaded successfully"
}

download_linux() {
    echo "Downloading Linux binaries..."
    
    BIN_DIR="$ROOT_DIR/rust/bin-linux"
    mkdir -p "$BIN_DIR/lib"
    
    # Check if already exists
    if [[ -f "$BIN_DIR/ffmpeg" && -f "$BIN_DIR/whisper-cli" ]]; then
        echo "Linux binaries already exist, skipping download"
        return
    fi
    
    # Download FFmpeg static build for Linux
    echo "Downloading FFmpeg..."
    # Use John Van Sickle's reliable static builds for Linux
    FFMPEG_URL="https://johnvansickle.com/ffmpeg/releases/ffmpeg-release-amd64-static.tar.xz"
    curl -L "$FFMPEG_URL" -o /tmp/ffmpeg-linux.tar.xz
    
    mkdir -p /tmp/ffmpeg-linux
    tar -xJf /tmp/ffmpeg-linux.tar.xz -C /tmp/ffmpeg-linux --strip-components=1
    
    cp /tmp/ffmpeg-linux/ffmpeg "$BIN_DIR/"
    cp /tmp/ffmpeg-linux/ffprobe "$BIN_DIR/"
    chmod +x "$BIN_DIR/ffmpeg" "$BIN_DIR/ffprobe"
    
    # Download whisper.cpp Linux build
    echo "Downloading whisper-cli..."
    curl -L "https://github.com/ggerganov/whisper.cpp/releases/download/${WHISPER_VERSION}/whisper-cli-linux-x64" \
        -o "$BIN_DIR/whisper-cli"
    chmod +x "$BIN_DIR/whisper-cli"
    
    echo "Linux binaries downloaded successfully"
}

# Detect OS if not specified
if [[ -z "$TARGET_OS" ]]; then
    case "$(uname -s)" in
        Darwin) TARGET_OS="macOS" ;;
        Linux) TARGET_OS="Linux" ;;
        MINGW*|MSYS*|CYGWIN*) TARGET_OS="Windows" ;;
        *) echo "Unknown OS"; exit 1 ;;
    esac
fi

echo "Target OS: $TARGET_OS"

case "$TARGET_OS" in
    macOS|Darwin)
        download_macos
        ;;
    Windows)
        download_windows
        ;;
    Linux)
        download_linux
        ;;
    *)
        echo "Unknown TARGET_OS: $TARGET_OS"
        exit 1
        ;;
esac

echo "Done!"
