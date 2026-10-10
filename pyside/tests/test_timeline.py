from PySide6.QtCore import QPoint, Qt
from PySide6.QtWidgets import QWidget

from app.models.captions import CaptionSegment
from app.views.timeline import VisualTimelineWidget


def test_timeline_initial_state(qtbot):
    parent = QWidget()
    timeline = VisualTimelineWidget(parent)
    qtbot.addWidget(timeline)

    assert timeline.duration_ms == 0
    assert timeline.current_position_ms == 0
    assert timeline.segments == []
    assert timeline.selected_segment is None


def test_timeline_seek_on_click(qtbot):
    timeline = VisualTimelineWidget()
    timeline.resize(1000, 80)
    timeline.set_duration(10000)  # 10.0 seconds
    qtbot.addWidget(timeline)
    timeline.show()

    emitted_seeks = []
    timeline.seek_requested.connect(emitted_seeks.append)

    # Click in the middle (x = 500 out of 1000 -> 5000 ms)
    qtbot.mousePress(timeline, Qt.MouseButton.LeftButton, pos=QPoint(500, 40))
    qtbot.mouseRelease(timeline, Qt.MouseButton.LeftButton, pos=QPoint(500, 40))

    assert len(emitted_seeks) > 0
    # Should seek to approx 5000 ms (+/- 100ms)
    assert 4900 <= emitted_seeks[-1] <= 5100


def test_timeline_select_segment(qtbot):
    timeline = VisualTimelineWidget()
    timeline.resize(1000, 80)
    timeline.set_duration(10000)  # 10.0 seconds

    seg1 = CaptionSegment(start_ms=1000, end_ms=3000, text="First segment")
    seg2 = CaptionSegment(start_ms=5000, end_ms=8000, text="Second segment")
    timeline.set_segments([seg1, seg2])
    qtbot.addWidget(timeline)
    timeline.show()

    selected = []
    timeline.segment_selected.connect(selected.append)

    # Click inside seg1 (start: 100px, end: 300px -> click at x = 200)
    qtbot.mousePress(timeline, Qt.MouseButton.LeftButton, pos=QPoint(200, 40))
    qtbot.mouseRelease(timeline, Qt.MouseButton.LeftButton, pos=QPoint(200, 40))

    assert len(selected) > 0
    assert selected[-1] == seg1
    assert timeline.selected_segment == seg1


def _trimmed_timeline(qtbot, start_ms=2000, end_ms=8000):
    timeline = VisualTimelineWidget()
    qtbot.addWidget(timeline)
    timeline.set_duration(10000)
    timeline.set_trim_range(start_ms, end_ms)
    emitted = []
    timeline.trim_range_changed.connect(lambda s, e: emitted.append((s, e)))
    return timeline, emitted


def test_set_trim_start_at_playhead(qtbot):
    timeline, emitted = _trimmed_timeline(qtbot)
    timeline.set_trim_start_at(3500)
    assert (timeline.trim_start_ms, timeline.trim_end_ms) == (3500, 8000)
    assert emitted == [(3500, 8000)]


def test_set_trim_end_at_playhead(qtbot):
    timeline, emitted = _trimmed_timeline(qtbot)
    timeline.set_trim_end_at(6200)
    assert (timeline.trim_start_ms, timeline.trim_end_ms) == (2000, 6200)
    assert emitted == [(2000, 6200)]


def test_trim_start_past_the_end_releases_the_end(qtbot):
    timeline, emitted = _trimmed_timeline(qtbot)
    timeline.set_trim_start_at(9000)
    assert (timeline.trim_start_ms, timeline.trim_end_ms) == (9000, 10000)
    assert emitted == [(9000, 10000)]


def test_trim_end_before_the_start_releases_the_start(qtbot):
    timeline, emitted = _trimmed_timeline(qtbot)
    timeline.set_trim_end_at(1000)
    assert (timeline.trim_start_ms, timeline.trim_end_ms) == (0, 1000)
    assert emitted == [(0, 1000)]
