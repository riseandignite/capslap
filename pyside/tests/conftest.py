import shutil
import subprocess
from pathlib import Path

import pytest
from PySide6.QtCore import QCoreApplication, QSettings
from PySide6.QtWidgets import QMessageBox

REPO_ROOT = Path(__file__).resolve().parents[2]
SAMPLE_VIDEO = REPO_ROOT / "rust" / "bin" / "test_input.mp4"


@pytest.fixture(autouse=True)
def isolated_settings(tmp_path):
    """Each test gets empty QSettings, never the user's own."""
    QCoreApplication.setOrganizationName("PyCapSlapTests")
    QCoreApplication.setApplicationName("PyCapSlapTests")
    QSettings.setDefaultFormat(QSettings.Format.IniFormat)
    QSettings.setPath(
        QSettings.Format.IniFormat, QSettings.Scope.UserScope, str(tmp_path)
    )
    yield


@pytest.fixture(autouse=True)
def no_modal_message_boxes(monkeypatch):
    """Record message boxes instead of showing them.

    A modal QMessageBox waits for a click that never comes in a test run. The
    window opens one whenever the core reports an error, e.g. when a test loads
    a placeholder file that is not a real video, so the test hung — on whichever
    test happened to be running when the core's answer arrived.
    """
    shown: list[tuple[str, str]] = []

    def record(kind):
        def box(parent, title, text, *args, **kwargs):
            shown.append((kind, str(text)))
            return QMessageBox.StandardButton.Ok

        return staticmethod(box)

    for kind in ("warning", "critical", "information"):
        monkeypatch.setattr(QMessageBox, kind, record(kind))
    return shown


def _ffmpeg() -> str | None:
    bundled = REPO_ROOT / "rust" / "bin" / "ffmpeg"
    if bundled.exists():
        return str(bundled)
    return shutil.which("ffmpeg")


@pytest.fixture(scope="session", autouse=True)
def sample_video():
    """The 1080p sample the core and window tests open.

    It is git-ignored, so a fresh checkout (CI included) has none; make a short
    one with ffmpeg when it is missing. Without ffmpeg the tests that need it
    fail on the missing file, as they would anyway.
    """
    ffmpeg = _ffmpeg()
    if SAMPLE_VIDEO.exists() or not ffmpeg:
        return SAMPLE_VIDEO
    SAMPLE_VIDEO.parent.mkdir(parents=True, exist_ok=True)
    # Not every ffmpeg build has libx264; the bundled macOS one may not.
    for codec in ("libx264", "h264_videotoolbox", "mpeg4"):
        result = subprocess.run(
            [
                ffmpeg,
                "-y",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=1920x1080:rate=30:duration=5",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=5",
                "-c:v",
                codec,
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                "-shortest",
                str(SAMPLE_VIDEO),
            ],
            capture_output=True,
            timeout=120,
        )
        if result.returncode == 0:
            break
        SAMPLE_VIDEO.unlink(missing_ok=True)
    return SAMPLE_VIDEO
