# Contributing

## Good first contribution: Windows / Linux packaged builds

`.github/workflows/release.yml` currently only builds and releases a macOS
(Apple Silicon) `PyCapSlap.app`. Windows and Linux aren't built at all. If
you want to take this on:

**What exists already:**
- `rust/bin-win/whisper-cli.exe` is committed — Windows has a head start on
  the whisper.cpp binary. Linux has nothing bundled yet.
- `scripts/download-binaries.sh` has `download_windows()`/`download_linux()`
  functions already stubbed in, but unused and untested — the macOS path
  (`download_macos()`) is the one that's actually been exercised.
- `pyside/pycapslap.spec` (PyInstaller) bundles `rust/target/release/core`,
  `rust/bin/` (ffmpeg/ffprobe/whisper-cli + their runtime libs) and
  `rust/src/fonts/` — the same shape should carry over per platform, but the
  macOS-specific bits (`.icns` icon, `BUNDLE(...)` producing a `.app`) need
  a Windows (`.ico`, plain onedir/onefile `.exe`) and Linux (AppImage or
  plain dir, no bundle icon convention) equivalent.
- `core_client.py`/`fonts.py` already check `sys.frozen`/`sys._MEIPASS` for
  a packaged build, and the launcher sets `WHISPER_CLI_PATH`/`FFPROBE_PATH`/
  `CAPSLAP_FONTS_DIR`/`FFMPEG_PATH` explicitly rather than relying on the
  Rust core's own path-guessing — that part should already work cross-platform
  as long as the packaging script points those env vars at the right bundled
  files.

**What's missing, per platform:**
- **A real hardware video encoder path.** The Rust side currently assumes
  Apple VideoToolbox (see `rust/src/video.rs`, `HardwareEncoder`) — Windows
  needs an NVENC/QSV/AMF path, Linux needs VAAPI (or fall back to software
  x264, which works everywhere but is much slower).
- **A genuinely architecture-native ffmpeg + ffprobe** for that OS,
  statically built or with all its shared-library dependencies resolvable
  (this exact issue broke the very first macOS release, so verify by
  *running* the downloaded/bundled binary, not just checking its file type).
- A `download_windows()`/`download_linux()` rewrite mirroring whatever fix
  `download_macos()` ended up needing (see CHANGELOG v26.09.13.1 →
  v26.09.13.2: the first macOS release attempt bundled an x86_64-only
  ffmpeg build that silently doesn't run on Apple Silicon without Rosetta —
  the *build* succeeded because PyInstaller just copies files without
  executing them, so this class of bug won't show up until someone
  actually launches the packaged app. **Always smoke-test the packaged
  binaries themselves** — running them, not just checking their file type
  — before calling a release build done. See the `release.yml` build job's
  own "Smoke-test the bundled core binary" step for the pattern).
- CI: extend the `release.yml` build matrix, and ideally `ci.yml` too if
  there's a Windows/Linux-only code path worth testing.

## Tests that hung under pytest-qt (solved)

`test_main_window.py` used to hang, on a different test from run to run, and
two tests were skipped for it. The cause was a modal `QMessageBox`: the window
opens one whenever the core reports an error (loading the placeholder "video"
a test writes makes first-frame extraction fail), and in a test run nobody
clicks it. Which test froze depended on when the core's answer arrived.
`tests/conftest.py` now records message boxes instead of showing them (the
`no_modal_message_boxes` fixture returns what would have been shown), so a
new dialog in the app can't freeze the suite either.

The sample video the core and window tests open, `rust/bin/test_input.mp4`, is
git-ignored; the `sample_video` fixture makes a 5-second 1080p clip with ffmpeg
(the bundled one, else the one on `PATH`) when it is missing.

## Local development

See the root `README.md` and `pyside/README.md` for running the app,
tests, and linters locally.

### Optional: Finnish spell check

`brew install libvoikko` turns on the caption table's spell checking. The
Python wrapper (`libvoikko`) is a normal dependency, but it is a ctypes
shim: without the native library and its dictionary, `SpellChecker`
reports no errors and the UI behaves as if the feature did not exist.
`PYCAPSLAP_DISABLE_SPELLCHECK=1` forces that state, and
`PYCAPSLAP_VOIKKO_LIB_PATH` points at the native library if it lives
somewhere the usual Homebrew/MacPorts paths don't cover.

### Where caption layout lives

**One layout engine, in Rust.** `rust/src/captions.rs` decides cue
boundaries, line breaks, per-line font size, uppercasing, which word is
highlighted and the vertical anchor, measuring text against the bundled
font files (`rust/src/text_metrics.rs`, `rust/src/justify.rs`).

The editor does **not** lay captions out. It asks for the layout over the
`previewLayout` RPC and draws what comes back
(`pyside/app/views/video_canvas.py`). Please keep it that way: the
preview and the export used to be separate implementations, and they
drifted into disagreeing about nearly every visual property (see
CHANGELOG, Unreleased). If a caption needs to look different, change it
in `captions.rs` — both sides follow from there. The one thing the canvas
still decides for itself is a stand-in layout for before the core
answers, and it mirrors the renderer's own font-size formula.
