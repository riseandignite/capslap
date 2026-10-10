import logging
import os
import shutil
import sys
from pathlib import Path

logger = logging.getLogger(__name__)

# Ensure pyside root is in sys.path when executed directly as `python app/main.py`
pyside_dir = Path(__file__).resolve().parent.parent
if str(pyside_dir) not in sys.path:
    sys.path.insert(0, str(pyside_dir))

from PySide6.QtGui import QIcon
from PySide6.QtWidgets import QApplication

from app.views.main_window import MainWindow


def ensure_macos_app_name() -> bool:
    """Ensure the process name shown in macOS Dock, Menu Bar, and Cmd+Tab is PyCapSlap.

    On macOS, unbundled processes display the executable binary basename in the
    Task Switcher (Cmd+Tab) and Dock. By ensuring a Mach-O executable named 'PyCapSlap'
    exists and executing it, macOS registers the process under the name 'PyCapSlap'.
    """
    if sys.platform != "darwin":
        return False

    if "PYTEST_CURRENT_TEST" in os.environ:
        return False

    if (
        os.environ.get("PYCAPSLAP_REEXECED") == "1"
        or os.environ.get("PYCAPSLAP_NO_REEXEC") == "1"
    ):
        return False

    if os.path.basename(sys.executable) == "PyCapSlap":
        return False

    try:
        real_py = os.path.realpath(sys.executable)
        real_bin_dir = os.path.dirname(real_py)
        toolchain_bin = os.path.join(real_bin_dir, "PyCapSlap")

        if not os.path.exists(toolchain_bin):
            try:
                os.link(real_py, toolchain_bin)
            except OSError:
                shutil.copy2(real_py, toolchain_bin)
                os.chmod(toolchain_bin, 0o755)

        # On macOS (APFS/HFS+), filesystems are usually case-insensitive.
        # Placing 'PyCapSlap' in .venv/bin would collide with the 'pycapslap' script.
        # Instead, we place the symlink in .venv/MacOS/PyCapSlap, where Python
        # discovers .venv/pyvenv.cfg automatically (via grandparent directory).
        venv_root = Path(sys.prefix)
        venv_macos = venv_root / "MacOS"
        venv_macos.mkdir(parents=True, exist_ok=True)
        venv_pycapslap = venv_macos / "PyCapSlap"
        if not venv_pycapslap.exists():
            try:
                os.symlink(toolchain_bin, str(venv_pycapslap))
            except OSError:
                pass

        target_bin = str(venv_pycapslap) if venv_pycapslap.exists() else toolchain_bin
        main_script = str(Path(__file__).resolve())
        args = [target_bin, main_script] + sys.argv[1:]
        env = os.environ.copy()
        env["PYCAPSLAP_REEXECED"] = "1"
        if str(pyside_dir) not in env.get("PYTHONPATH", ""):
            existing_pythonpath = env.get("PYTHONPATH")
            env["PYTHONPATH"] = (
                f"{pyside_dir}:{existing_pythonpath}"
                if existing_pythonpath
                else str(pyside_dir)
            )
        if sys.prefix != sys.base_prefix:
            env["VIRTUAL_ENV"] = sys.prefix
        os.execve(target_bin, args, env)
        return True
    except Exception as e:
        logger.warning("Could not set macOS process name to PyCapSlap: %s", e)
        return False


def main():
    ensure_macos_app_name()

    app = QApplication(sys.argv)
    app.setApplicationName("PyCapSlap")
    app.setApplicationDisplayName("PyCapSlap")
    app.setOrganizationName("PyCapSlap")

    icon_path = Path(__file__).resolve().parent / "resources" / "icon.png"
    if icon_path.exists():
        app.setWindowIcon(QIcon(str(icon_path)))

    window = MainWindow()
    window.show()

    # If a video was passed as CLI argument, load it immediately
    if len(sys.argv) > 1 and Path(sys.argv[1]).exists():
        window.load_video(str(Path(sys.argv[1]).resolve()))

    sys.exit(app.exec())


if __name__ == "__main__":
    main()
