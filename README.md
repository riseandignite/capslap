# Prerequisites

1. Install FFmpeg and ensure it's in your PATH

   - macOS: `brew install ffmpeg`
   - Windows: Download from https://ffmpeg.org/

2. Install Rust: https://rustup.rs/

3. Install Bun: https://bun.sh/

# Setup Steps

1. Clone the repository
2. Build Rust backend: `cd rust && cargo build`
3. Install Electron dependencies: `cd electron && bun install`
4. Run the app: `bun run dev`
5. Get an OpenAI API key and enter it in the app's settings

# Additional Notes

- An OpenAI API key is required for transcription functionality
- The app uses OpenAI's Whisper API for audio-to-text conversion
- FFmpeg is used for video processing and subtitle rendering
