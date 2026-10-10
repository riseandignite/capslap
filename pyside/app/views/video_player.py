import time

from PySide6.QtCore import Qt, QUrl, Signal
from PySide6.QtGui import QKeySequence, QShortcut
from PySide6.QtMultimedia import QAudioOutput, QMediaPlayer
from PySide6.QtWidgets import (
    QHBoxLayout,
    QLabel,
    QPushButton,
    QSlider,
    QVBoxLayout,
    QWidget,
)

from app.views.video_canvas import VideoCanvasWidget


class VideoPlayerWidget(QWidget):
    """
    Native hardware-accelerated video player with seek latency instrumentation
    and direct QVideoSink unified subtitle compositing.
    """

    seek_latency_measured = Signal(float)  # Latency in ms
    position_changed = Signal(int)  # Position in ms
    duration_changed = Signal(int)  # Duration in ms
    # "Set the trim's start / end here" — the window owns the trim, the player the buttons
    mark_in_requested = Signal()
    mark_out_requested = Signal()

    def __init__(self, parent: QWidget | None = None):
        super().__init__(parent)

        self._pending_seek_time: float | None = None
        self._pending_target_ms: int | None = None
        self._is_scrubbing = False
        # The trimmed part of the video. Stop returns to its start and playback stops at
        # its end; None means the video's own end.
        self._range_start_ms = 0
        self._range_end_ms: int | None = None
        self._frame_duration_ms = 1000 / 30

        self._setup_player()
        self._setup_ui()

    def _setup_player(self) -> None:
        self.media_player = QMediaPlayer(self)
        self.audio_output = QAudioOutput(self)
        self.media_player.setAudioOutput(self.audio_output)

        self.canvas = VideoCanvasWidget(self)
        self.overlay = self.canvas  # Alias for backward-compatible overlay access
        self.media_player.setVideoSink(self.canvas.sink)

        self.media_player.positionChanged.connect(self._on_position_changed)
        self.media_player.durationChanged.connect(self._on_duration_changed)

    def _setup_ui(self) -> None:
        layout = QVBoxLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.setSpacing(6)

        # Video Canvas with direct subtitle rendering
        layout.addWidget(self.canvas, stretch=1)

        # Controls Bar
        controls_layout = QHBoxLayout()
        controls_layout.setContentsMargins(8, 4, 8, 8)
        controls_layout.setSpacing(8)

        # Play/Pause button
        self.play_btn = QPushButton("Play")
        self.play_btn.setFixedWidth(65)
        self.play_btn.clicked.connect(self.toggle_play_pause)
        controls_layout.addWidget(self.play_btn)

        # Stop: back to the start of the trim, to check where playback begins
        self.stop_btn = QPushButton("Stop")
        self.stop_btn.setFixedWidth(55)
        self.stop_btn.setToolTip("Stop and go back to the start of the trim")
        self.stop_btn.clicked.connect(self.stop)
        controls_layout.addWidget(self.stop_btn)

        # Mark the trim's start / end at the playhead (also the I and O keys)
        self.mark_in_btn = QPushButton("I")
        self.mark_in_btn.setToolTip("Set the trim's start here (I)")
        self.mark_in_btn.clicked.connect(self.mark_in_requested)
        self.mark_out_btn = QPushButton("O")
        self.mark_out_btn.setToolTip("Set the trim's end here (O)")
        self.mark_out_btn.clicked.connect(self.mark_out_requested)
        for btn in (self.mark_in_btn, self.mark_out_btn):
            btn.setFixedWidth(32)
            # Clicking must not take focus, or Space would press the button
            # instead of playing and pausing.
            btn.setFocusPolicy(Qt.FocusPolicy.NoFocus)
            controls_layout.addWidget(btn)

        # Time label (current)
        self.time_lbl = QLabel("00:00")
        controls_layout.addWidget(self.time_lbl)

        # Timeline Slider (Scrubber)
        self.slider = QSlider(Qt.Orientation.Horizontal)
        self.slider.setRange(0, 0)
        self.slider.sliderPressed.connect(self._on_slider_pressed)
        self.slider.sliderMoved.connect(self._on_slider_moved)
        self.slider.sliderReleased.connect(self._on_slider_released)
        controls_layout.addWidget(self.slider, stretch=1)

        # Duration label
        self.duration_lbl = QLabel("00:00")
        controls_layout.addWidget(self.duration_lbl)

        # Playback speed button (e.g. 1.0x, 1.25x, 1.5x, 2.0x)
        self.playback_rate: float = 1.0
        self.speed_btn = QPushButton("1.0x")
        self.speed_btn.setFixedWidth(52)
        self.speed_btn.setToolTip(
            "Playback speed: click to cycle (1.0x, 1.25x, 1.5x, 2.0x)"
        )
        self.speed_btn.setStyleSheet("""
            QPushButton {
                background-color: #27272a;
                color: #e4e4e7;
                border: 1px solid #3f3f46;
                border-radius: 4px;
                font-size: 11px;
                font-weight: 600;
                padding: 2px 4px;
            }
            QPushButton:hover {
                background-color: #3f3f46;
                color: #ffffff;
            }
        """)
        self.speed_btn.clicked.connect(self._cycle_playback_speed)
        controls_layout.addWidget(self.speed_btn)

        # Seek latency indicator
        self.latency_lbl = QLabel("Seek: -- ms")
        self.latency_lbl.setStyleSheet("color: #888888; font-size: 11px;")
        controls_layout.addWidget(self.latency_lbl)

        layout.addLayout(controls_layout)

        # Keyboard shortcuts: + / - step one source frame.
        QShortcut(
            QKeySequence(Qt.Key.Key_Space), self, activated=self.toggle_play_pause
        )
        QShortcut(
            QKeySequence(Qt.Key.Key_Left),
            self,
            activated=lambda: self.seek_relative(-1000),
        )
        QShortcut(
            QKeySequence(Qt.Key.Key_Right),
            self,
            activated=lambda: self.seek_relative(1000),
        )
        QShortcut(
            QKeySequence(Qt.Key.Key_BracketRight),
            self,
            activated=self._increase_playback_speed,
        )
        QShortcut(
            QKeySequence(Qt.Key.Key_BracketLeft),
            self,
            activated=self._decrease_playback_speed,
        )
        QShortcut(
            QKeySequence(Qt.Key.Key_Plus), self, activated=lambda: self.step_frame(1)
        )
        QShortcut(
            QKeySequence(Qt.Key.Key_Minus), self, activated=lambda: self.step_frame(-1)
        )

    def set_frame_rate(self, fps: float) -> None:
        if fps > 0:
            self._frame_duration_ms = 1000.0 / fps

    def step_frame(self, direction: int) -> None:
        """Seek by exactly one frame at the loaded video's frame rate."""
        self.media_player.pause()
        self.seek_to_ms(
            round(self.media_player.position() + direction * self._frame_duration_ms)
        )

    def set_playback_rate(self, rate: float) -> None:
        """Set the media player playback rate (e.g. 1.0, 1.25, 1.5, 2.0)."""
        self.playback_rate = float(rate)
        self.media_player.setPlaybackRate(self.playback_rate)
        self.speed_btn.setText(f"{self.playback_rate}x")

    def _cycle_playback_speed(self) -> None:
        """Cycle through common playback speeds: 1.0x -> 1.25x -> 1.5x -> 2.0x -> 1.0x."""
        cycle_rates = [1.0, 1.25, 1.5, 2.0]
        try:
            cur_idx = cycle_rates.index(self.playback_rate)
            next_idx = (cur_idx + 1) % len(cycle_rates)
            self.set_playback_rate(cycle_rates[next_idx])
        except ValueError:
            self.set_playback_rate(1.0)

    def _increase_playback_speed(self) -> None:
        """Increase playback speed to next preset step."""
        presets = [0.75, 1.0, 1.25, 1.5, 1.75, 2.0]
        for p in presets:
            if p > self.playback_rate + 0.01:
                self.set_playback_rate(p)
                return

    def _decrease_playback_speed(self) -> None:
        """Decrease playback speed to previous preset step."""
        presets = [0.75, 1.0, 1.25, 1.5, 1.75, 2.0]
        for p in reversed(presets):
            if p < self.playback_rate - 0.01:
                self.set_playback_rate(p)
                return

    def load_video(self, file_path: str) -> None:
        url = QUrl.fromLocalFile(file_path)
        self.media_player.setSource(url)
        self.play_btn.setText("Play")

    def release(self) -> None:
        """Let go of the media before the widget goes away.

        The macOS backend keeps loading and seeking on its own timers; a player
        destroyed with a source still open left those firing into freed
        memory (a segfault in QTimerInfoList::activateTimers).
        """
        self.media_player.stop()
        self.media_player.setVideoSink(None)
        self.media_player.setSource(QUrl())

    def set_play_range(self, start_ms: int, end_ms: int | None) -> None:
        """The trimmed part of the video: Stop goes to its start, playback stops at its end."""
        self._range_start_ms = max(0, int(start_ms))
        self._range_end_ms = (
            None if end_ms is None else max(self._range_start_ms, int(end_ms))
        )

    def _at_or_past_range_end(self, pos_ms: int) -> bool:
        # Within one position update (~50 ms) of the end counts as there, or Play would
        # stop again at once after an auto-stop.
        return self._range_end_ms is not None and pos_ms >= self._range_end_ms - 50

    def toggle_play_pause(self) -> None:
        if self.media_player.playbackState() == QMediaPlayer.PlaybackState.PlayingState:
            self.media_player.pause()
            self.play_btn.setText("Play")
        else:
            # Past the trim's end there is nothing left to play: start the trim over.
            if self._at_or_past_range_end(self.media_player.position()):
                self.seek_to_ms(self._range_start_ms)
            self.media_player.play()
            self.play_btn.setText("Pause")

    def stop(self) -> None:
        """Pause and go back to the start of the trim."""
        self.media_player.pause()
        self.play_btn.setText("Play")
        self.seek_to_ms(self._range_start_ms)

    def seek_to_ms(self, pos_ms: int) -> None:
        self._pending_seek_time = time.perf_counter()
        self._pending_target_ms = pos_ms
        self.media_player.setPosition(pos_ms)

    def seek_relative(self, delta_ms: int) -> None:
        current = self.media_player.position()
        target = max(0, min(self.media_player.duration(), current + delta_ms))
        self.seek_to_ms(target)

    def _on_slider_pressed(self) -> None:
        self._is_scrubbing = True

    def _on_slider_moved(self, val: int) -> None:
        self.time_lbl.setText(self._format_time(val))
        self.seek_to_ms(val)

    def _on_slider_released(self) -> None:
        self._is_scrubbing = False
        self.seek_to_ms(self.slider.value())

    def _on_position_changed(self, pos_ms: int) -> None:
        # Playback stops at the trim's end and shows its last frame.
        if (
            not self._is_scrubbing
            and self._range_end_ms is not None
            and pos_ms >= self._range_end_ms
            and self.media_player.playbackState()
            == QMediaPlayer.PlaybackState.PlayingState
        ):
            self.media_player.pause()
            self.play_btn.setText("Play")
            if pos_ms > self._range_end_ms:
                self.seek_to_ms(self._range_end_ms)
                return
        self.canvas.current_pos_ms = pos_ms
        self.canvas.update()
        if not self._is_scrubbing:
            self.slider.setValue(pos_ms)
            self.time_lbl.setText(self._format_time(pos_ms))

        # Check seek latency if a seek was pending
        if self._pending_seek_time is not None:
            # Measure time from request to position response
            latency_ms = (time.perf_counter() - self._pending_seek_time) * 1000.0
            self._pending_seek_time = None
            self._pending_target_ms = None
            self.latency_lbl.setText(f"Seek: {latency_ms:.1f} ms")
            self.seek_latency_measured.emit(latency_ms)

        self.position_changed.emit(pos_ms)

    def _on_duration_changed(self, dur_ms: int) -> None:
        self.slider.setRange(0, dur_ms)
        self.duration_lbl.setText(self._format_time(dur_ms))
        self.duration_changed.emit(dur_ms)

    @staticmethod
    def _format_time(ms: int) -> str:
        s = ms // 1000
        m = s // 60
        s = s % 60
        return f"{m:02d}:{s:02d}"
