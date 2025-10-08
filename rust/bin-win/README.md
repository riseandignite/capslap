# Windows Binaries

This directory contains Windows-specific binaries for whisper.cpp.

## Contents

- `whisper-cli.exe` - Whisper CLI executable for Windows x64
- `lib/libinternal/` - Required DLL libraries:
  - `whisper.dll` - Main whisper library
  - `ggml.dll` - GGML base library
  - `ggml-base.dll` - GGML base implementation
  - `ggml-cpu.dll` - CPU-optimized GGML
  - `SDL2.dll` - SDL2 dependency

## Source

These binaries are from [whisper.cpp v1.7.6](https://github.com/ggerganov/whisper.cpp/releases/tag/v1.7.6)

## Usage

During Windows build, electron-builder copies these files to `app/Resources/bin-win/`
