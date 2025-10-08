# CapSlap - AI Video Caption Generator

Automatically generate and burn captions into videos using AI transcription.

## Prerequisites

- **Rust**: https://rustup.rs/
- **Bun**: https://bun.sh/
- **FFmpeg** (auto-installed on macOS)

## Quick Start

1. **Clone the repository**

   ```bash
   git clone <repository-url>
   cd capslap
   ```

2. **Build Rust core**

   ```bash
   cd rust
   cargo build
   cd ..
   ```

3. **Install Electron dependencies**

   ```bash
   cd electron
   bun install
   ```

   FFmpeg will be automatically downloaded on macOS during `bun install`.

4. **Run the app**

   ```bash
   bun run dev
   ```

## Whisper Models

Local whisper models can be downloaded directly through the app UI, or manually:

```bash
mkdir -p rust/models

# Tiny model (fastest, 75 MB)
curl -L https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin \
  -o rust/models/ggml-tiny.bin

# Base model (recommended, 142 MB)
curl -L https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin \
  -o rust/models/ggml-base.bin
```

Alternatively, use OpenAI API (requires API key) without downloading models.

## Platform-Specific Notes

### macOS

FFmpeg is automatically downloaded during `bun install` via the postinstall script.
