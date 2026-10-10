#!/usr/bin/env python3
"""Create a macOS application bundle for PyCapSlap: dist/PyCapSlap.app."""

import plistlib
import shutil
import stat
from pathlib import Path


def create_app_bundle(output_dir: Path | None = None) -> Path:
    repo_root = Path(__file__).resolve().parents[1]
    if output_dir is None:
        output_dir = repo_root / "dist"
    output_dir.mkdir(parents=True, exist_ok=True)

    app_bundle = output_dir / "PyCapSlap.app"
    contents_dir = app_bundle / "Contents"
    macos_dir = contents_dir / "MacOS"
    resources_dir = contents_dir / "Resources"

    macos_dir.mkdir(parents=True, exist_ok=True)
    resources_dir.mkdir(parents=True, exist_ok=True)

    # 1. Copy icon.icns
    icns_src = repo_root / "pyside" / "app" / "resources" / "icon.icns"
    if icns_src.exists():
        shutil.copy2(icns_src, resources_dir / "icon.icns")

    # 2. Write Info.plist
    info_plist = {
        "CFBundleName": "PyCapSlap",
        "CFBundleDisplayName": "PyCapSlap",
        "CFBundleIdentifier": "com.pycapslap.app",
        "CFBundleVersion": "0.1.0",
        "CFBundleShortVersionString": "0.1.0",
        "CFBundlePackageType": "APPL",
        "CFBundleExecutable": "PyCapSlap",
        "CFBundleIconFile": "icon.icns",
        "NSHighResolutionCapable": True,
        "LSMinimumSystemVersion": "12.0",
    }
    with open(contents_dir / "Info.plist", "wb") as f:
        plistlib.dump(info_plist, f)

    # 3. Create launcher script in Contents/MacOS/PyCapSlap
    launcher_path = macos_dir / "PyCapSlap"
    launcher_script = f"""#!/bin/bash
DIR="$( cd "$( dirname "${{BASH_SOURCE[0]}}" )" && pwd )"
REPO_ROOT="{repo_root}"
cd "$REPO_ROOT"
if command -v uv >/dev/null 2>&1; then
    exec uv run pycapslap "$@"
else
    exec "$REPO_ROOT/.venv/MacOS/PyCapSlap" "$REPO_ROOT/pyside/app/main.py" "$@"
fi
"""
    launcher_path.write_text(launcher_script, encoding="utf-8")
    launcher_path.chmod(
        launcher_path.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH
    )

    return app_bundle


if __name__ == "__main__":
    bundle = create_app_bundle()
    print(f"Created macOS App Bundle: {bundle}")
