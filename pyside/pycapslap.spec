# PyInstaller spec for PyCapSlap.
#
# Bundles the PySide6 app together with the Rust core binary and its own
# runtime dependencies (ffmpeg/ffprobe/whisper-cli + libs, bundled fonts) so
# the result runs standalone on a machine with none of this repo, `uv`, or
# a Rust toolchain installed. core_client.py and fonts.py know to look for
# these same relative paths (rust/target/release/core, rust/bin/,
# rust/src/fonts) under `sys._MEIPASS` / next to the executable when frozen.
#
# Build (from the pyside/ directory, after `cargo build --release` in
# rust/ has produced rust/target/release/core):
#   uv tool run pyinstaller pycapslap.spec --noconfirm
#
# Output: dist/PyCapSlap.app (macOS) — a plain onedir bundle is used
# (not --onefile) so startup doesn't pay a self-extraction cost every launch.

import sys
from pathlib import Path

block_cipher = None

repo_root = Path(SPECPATH).resolve().parent
pyside_dir = repo_root / "pyside"
rust_dir = repo_root / "rust"

# (source, destination-relative-to-bundle-root) pairs. Destinations mirror
# the dev checkout's own layout so nothing in core_client.py/fonts.py needs
# a frozen-specific path beyond checking sys._MEIPASS / sys.executable.
datas = [
    (str(rust_dir / "src" / "fonts"), "rust/src/fonts"),
]
binaries = [
    (str(rust_dir / "target" / "release" / "core"), "rust/target/release"),
]

bin_dir = rust_dir / "bin"
if bin_dir.exists():
    for entry in bin_dir.iterdir():
        if entry.name.endswith((".bak", ".capslap.json")) or entry.name == "test_input.mp4":
            continue  # build leftovers / fixtures, not runtime dependencies
        dest = f"rust/bin/{entry.relative_to(bin_dir)}"
        if entry.is_dir():
            datas.append((str(entry), dest))
        else:
            # ffmpeg/ffprobe are sometimes a symlink into a local Homebrew
            # install on the machine that builds this — resolve to the real
            # file so the bundle carries actual binary content, not a
            # symlink pointing at a path that won't exist elsewhere.
            binaries.append((str(entry.resolve()), "rust/bin"))

a = Analysis(
    [str(pyside_dir / "app" / "main.py")],
    pathex=[str(pyside_dir)],
    binaries=binaries,
    datas=datas,
    hiddenimports=[],
    hookspath=[],
    hooksconfig={},
    runtime_hooks=[],
    excludes=[],
    noarchive=False,
    cipher=block_cipher,
)
pyz = PYZ(a.pure, a.zipped_data, cipher=block_cipher)

exe = EXE(
    pyz,
    a.scripts,
    [],
    exclude_binaries=True,
    name="PyCapSlap",
    debug=False,
    strip=False,
    upx=False,
    console=False,
    icon=str(pyside_dir / "app" / "resources" / "icon.icns")
    if sys.platform == "darwin"
    else str(pyside_dir / "app" / "resources" / "icon.ico"),
)

coll = COLLECT(
    exe,
    a.binaries,
    a.zipfiles,
    a.datas,
    strip=False,
    upx=False,
    name="PyCapSlap",
)

if sys.platform == "darwin":
    app = BUNDLE(
        coll,
        name="PyCapSlap.app",
        icon=str(pyside_dir / "app" / "resources" / "icon.icns"),
        bundle_identifier="com.pycapslap.app",
        info_plist={
            "CFBundleName": "PyCapSlap",
            "CFBundleDisplayName": "PyCapSlap",
            "NSHighResolutionCapable": True,
            "LSMinimumSystemVersion": "12.0",
        },
    )
