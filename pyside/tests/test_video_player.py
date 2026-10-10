from app.views.video_player import VideoPlayerWidget


def test_video_player_playback_speed_controls(qtbot):
    player = VideoPlayerWidget()
    qtbot.addWidget(player)

    # Initial state
    assert hasattr(player, "speed_btn")
    assert player.speed_btn.text() == "1.0x"
    assert player.playback_rate == 1.0
    assert player.media_player.playbackRate() == 1.0

    # Cycle speed: 1.0x -> 1.25x
    player.speed_btn.click()
    assert player.playback_rate == 1.25
    assert player.speed_btn.text() == "1.25x"
    assert player.media_player.playbackRate() == 1.25

    # Cycle speed: 1.25x -> 1.5x
    player.speed_btn.click()
    assert player.playback_rate == 1.5
    assert player.speed_btn.text() == "1.5x"
    assert player.media_player.playbackRate() == 1.5

    # Cycle speed: 1.5x -> 2.0x
    player.speed_btn.click()
    assert player.playback_rate == 2.0
    assert player.speed_btn.text() == "2.0x"
    assert player.media_player.playbackRate() == 2.0

    # Cycle speed: 2.0x -> 1.0x
    player.speed_btn.click()
    assert player.playback_rate == 1.0
    assert player.speed_btn.text() == "1.0x"
    assert player.media_player.playbackRate() == 1.0

    # Direct setter
    player.set_playback_rate(1.5)
    assert player.playback_rate == 1.5
    assert player.speed_btn.text() == "1.5x"
    assert player.media_player.playbackRate() == 1.5


class _FakeMediaPlayer:
    """Stands in for QMediaPlayer's transport, so playback can be driven without a video."""

    def __init__(self, player: VideoPlayerWidget):
        from PySide6.QtMultimedia import QMediaPlayer

        self._states = QMediaPlayer.PlaybackState
        self.state = self._states.StoppedState
        self.pos = 0
        self.seeks: list[int] = []
        mp = player.media_player
        mp.play = self._play
        mp.pause = self._pause
        mp.playbackState = lambda: self.state
        mp.position = lambda: self.pos
        mp.setPosition = self._set_position

    def _play(self):
        self.state = self._states.PlayingState

    def _pause(self):
        self.state = self._states.PausedState

    def _set_position(self, ms):
        self.pos = ms
        self.seeks.append(ms)


def test_stop_returns_to_trim_start(qtbot):
    player = VideoPlayerWidget()
    qtbot.addWidget(player)
    fake = _FakeMediaPlayer(player)
    player.set_play_range(4000, 9000)

    player.toggle_play_pause()
    fake.pos = 6000
    player.stop_btn.click()

    assert fake.state == fake._states.PausedState
    assert fake.seeks[-1] == 4000
    assert player.play_btn.text() == "Play"


def test_playback_stops_at_trim_end(qtbot):
    player = VideoPlayerWidget()
    qtbot.addWidget(player)
    fake = _FakeMediaPlayer(player)
    player.set_play_range(4000, 9000)
    player.toggle_play_pause()

    player._on_position_changed(8950)
    assert fake.state == fake._states.PlayingState

    player._on_position_changed(9040)
    assert fake.state == fake._states.PausedState
    assert fake.seeks[-1] == 9000
    assert player.play_btn.text() == "Play"


def test_play_from_trim_end_starts_the_trim_over(qtbot):
    player = VideoPlayerWidget()
    qtbot.addWidget(player)
    fake = _FakeMediaPlayer(player)
    player.set_play_range(4000, 9000)

    fake.pos = 9000
    player.toggle_play_pause()
    assert fake.seeks[-1] == 4000
    assert fake.state == fake._states.PlayingState


def test_play_before_trim_end_keeps_position(qtbot):
    player = VideoPlayerWidget()
    qtbot.addWidget(player)
    fake = _FakeMediaPlayer(player)
    player.set_play_range(4000, 9000)

    fake.pos = 1000
    player.toggle_play_pause()
    assert fake.seeks == []
    assert fake.state == fake._states.PlayingState


def test_no_trim_end_never_auto_stops(qtbot):
    player = VideoPlayerWidget()
    qtbot.addWidget(player)
    fake = _FakeMediaPlayer(player)
    player.set_play_range(0, None)
    player.toggle_play_pause()

    player._on_position_changed(60_000)
    assert fake.state == fake._states.PlayingState


def test_mark_buttons_ask_for_the_trim_and_leave_focus_alone(qtbot):
    from PySide6.QtCore import Qt

    player = VideoPlayerWidget()
    qtbot.addWidget(player)
    asked = []
    player.mark_in_requested.connect(lambda: asked.append("in"))
    player.mark_out_requested.connect(lambda: asked.append("out"))

    player.mark_in_btn.click()
    player.mark_out_btn.click()

    assert asked == ["in", "out"]
    for btn in (player.mark_in_btn, player.mark_out_btn):
        assert btn.focusPolicy() == Qt.FocusPolicy.NoFocus
        assert "(I)" in btn.toolTip() or "(O)" in btn.toolTip()
