import json
import logging
import os
import subprocess
import sys
import threading
import uuid
from concurrent.futures import Future
from pathlib import Path
from typing import Any

from PySide6.QtCore import QObject, Signal

logger = logging.getLogger("capslap.core_client")


class CoreClient(QObject):
    """
    Asynchronous JSON-RPC bridge to the long-lived Rust `core` executable.
    Emits Qt signals on the main thread for progress and logging.
    """

    # Signals: (id: str, status: str, progress: float 0.0..1.0)
    progress = Signal(str, str, float)
    # Signals: (id: str, message: str)
    log = Signal(str, str)
    # Signal: (exit_code: int)
    core_exited = Signal(int)

    def __init__(self, binary_path: str | None = None, parent: QObject | None = None):
        super().__init__(parent)
        self.binary_path = binary_path or self._resolve_binary_path()
        self.proc: subprocess.Popen | None = None
        self._pending: dict[str, Future] = {}
        self._lock = threading.Lock()
        self._write_lock = threading.Lock()
        self._reader_thread: threading.Thread | None = None
        self._running = False

        self._start_process()

    def _resolve_binary_path(self) -> str:
        possible_paths = []

        # A PyInstaller-frozen build extracts/unpacks its bundled data next
        # to sys._MEIPASS (onefile) or next to the executable (onedir) —
        # either way, `rust/...` was bundled at that root by pycapslap.spec.
        frozen_root = getattr(sys, "_MEIPASS", None)
        if frozen_root:
            possible_paths.append(
                Path(frozen_root) / "rust" / "target" / "release" / "core"
            )
        if getattr(sys, "frozen", False):
            possible_paths.append(
                Path(sys.executable).resolve().parent
                / "rust"
                / "target"
                / "release"
                / "core"
            )

        # Dev checkout: locate project root relative to pyside/app/core_client.py
        current_dir = Path(__file__).resolve().parent
        project_root = current_dir.parent.parent
        possible_paths += [
            project_root / "rust" / "target" / "release" / "core",
            project_root / "rust" / "target" / "debug" / "core",
            project_root / "rust" / "bin" / "core",
        ]

        if os.name == "nt":
            possible_paths = [p.with_suffix(".exe") for p in possible_paths]

        for p in possible_paths:
            if p.exists() and os.access(p, os.X_OK):
                return str(p)

        raise FileNotFoundError(
            f"Rust core executable not found. Looked in: {[str(p) for p in possible_paths]}"
        )

    def _start_process(self) -> None:
        logger.info(f"Starting Rust core from: {self.binary_path}")
        working_dir = os.path.dirname(self.binary_path)

        env = dict(os.environ)
        # Ensure FFMPEG_PATH and rust/bin are in environment
        rust_bin = Path(self.binary_path).resolve().parent.parent.parent / "bin"
        ffmpeg_candidates = [
            rust_bin / ("ffmpeg.exe" if os.name == "nt" else "ffmpeg"),
            Path("/opt/homebrew/opt/ffmpeg-full/bin/ffmpeg"),
            Path("/opt/homebrew/bin/ffmpeg"),
            Path("/usr/local/bin/ffmpeg"),
        ]
        ffmpeg_path = None
        for candidate in ffmpeg_candidates:
            if candidate.exists() and os.access(candidate, os.X_OK):
                ffmpeg_path = str(candidate)
                break

        if ffmpeg_path:
            env["FFMPEG_PATH"] = ffmpeg_path
            env["PATH"] = (
                f"{os.path.dirname(ffmpeg_path)}{os.pathsep}{env.get('PATH', '')}"
            )

        # Point the core straight at rust/bin's ffprobe/whisper-cli and
        # rust/src/fonts explicitly, rather than relying on its own
        # exe-relative guessing — which assumes a specific bundle layout
        # (core and a sibling bin/) that a packaged build may not match.
        # This is the one thing this launcher actually knows for certain.
        ffprobe_candidate = rust_bin / ("ffprobe.exe" if os.name == "nt" else "ffprobe")
        if ffprobe_candidate.exists() and os.access(ffprobe_candidate, os.X_OK):
            env["FFPROBE_PATH"] = str(ffprobe_candidate)

        if os.name == "nt":
            whisper_candidates = [rust_bin / "whisper-cli.exe"]
        else:
            import platform

            arch = "arm64" if platform.machine() == "arm64" else "x64"
            whisper_candidates = [
                rust_bin / f"whisper-cli-macos-{arch}",
                rust_bin / "whisper-cli",
            ]
        for candidate in whisper_candidates:
            if candidate.exists() and os.access(candidate, os.X_OK):
                env["WHISPER_CLI_PATH"] = str(candidate)
                break

        fonts_dir = rust_bin.parent / "src" / "fonts"
        if fonts_dir.is_dir():
            env["CAPSLAP_FONTS_DIR"] = str(fonts_dir)

        self.proc = subprocess.Popen(
            [self.binary_path],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            cwd=working_dir,
            env=env,
            bufsize=0,
        )
        self._running = True
        self._reader_thread = threading.Thread(target=self._stdout_reader, daemon=True)
        self._reader_thread.start()

    def _stdout_reader(self) -> None:
        assert self.proc and self.proc.stdout
        while self._running:
            line_bytes = self.proc.stdout.readline()
            if not line_bytes:
                break
            line = line_bytes.decode("utf-8", errors="replace").strip()
            if not line:
                continue

            try:
                msg = json.loads(line)
            except json.JSONDecodeError:
                logger.warning(f"Failed to parse JSON from Rust core: {line[:200]}")
                continue

            event = msg.get("event")
            req_id = msg.get("id")

            if event == "progress":
                status = str(msg.get("status", ""))
                try:
                    val = float(msg.get("progress", 0.0))
                except (ValueError, TypeError):
                    val = 0.0
                self.progress.emit(str(req_id), status, val)
            elif event == "log":
                message = msg.get("message", "")
                self.log.emit(str(req_id), message)
            elif "result" in msg and req_id:
                with self._lock:
                    fut = self._pending.pop(req_id, None)
                if fut and not fut.done():
                    fut.set_result(msg["result"])
            elif "error" in msg and req_id:
                err = msg.get("error")
                with self._lock:
                    fut = self._pending.pop(req_id, None)
                if fut and not fut.done():
                    fut.set_exception(RuntimeError(f"Rust core error: {err}"))

        exit_code = self.proc.poll() if self.proc else -1
        try:
            if self._running:
                self.core_exited.emit(exit_code or 0)
        except RuntimeError:
            pass
        self._fail_all_pending(RuntimeError(f"Rust core exited with code {exit_code}"))

    def _fail_all_pending(self, exc: Exception) -> None:
        with self._lock:
            for fut in self._pending.values():
                if not fut.done():
                    fut.set_exception(exc)
            self._pending.clear()

    def call(self, method: str, params: Any, request_id: str | None = None) -> Future:
        """
        Send JSON-RPC request to Rust core. Returns a Future resolved when Rust responds.
        """
        if not self.proc or self.proc.poll() is not None:
            fut: Future = Future()
            fut.set_exception(RuntimeError("Rust core process is not running"))
            return fut

        req_id = request_id or str(uuid.uuid4())
        payload = json.dumps({"id": req_id, "method": method, "params": params}) + "\n"

        fut = Future()
        with self._lock:
            self._pending[req_id] = fut

        try:
            with self._write_lock:
                assert self.proc.stdin
                self.proc.stdin.write(payload.encode("utf-8"))
                self.proc.stdin.flush()
        except (OSError, ValueError) as e:
            with self._lock:
                self._pending.pop(req_id, None)
            fut.set_exception(e)

        return fut

    def cancel(self, target_id: str) -> Future:
        return self.call("cancel", target_id)

    def close(self) -> None:
        self._running = False
        if self.proc and self.proc.poll() is None:
            try:
                self.proc.terminate()
                self.proc.wait(timeout=1.0)
            except (subprocess.TimeoutExpired, OSError):
                try:
                    self.proc.kill()
                except OSError:
                    pass
