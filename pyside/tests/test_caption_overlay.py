from PySide6.QtCore import QPoint, Qt
from PySide6.QtWidgets import QWidget

from app.models.captions import CaptionSegment
from app.views.caption_overlay import CaptionOverlayWidget


def test_caption_overlay_initial_state(qtbot):
    parent = QWidget()
    parent.resize(800, 600)
    overlay = CaptionOverlayWidget(parent)
    qtbot.addWidget(parent)

    assert overlay.current_segment is None
    assert overlay.anchor_y_pct == 80.0
    assert overlay.is_dragging is False


def test_caption_overlay_set_segment(qtbot):
    parent = QWidget()
    parent.resize(800, 600)
    overlay = CaptionOverlayWidget(parent)
    qtbot.addWidget(parent)

    seg = CaptionSegment(start_ms=1000, end_ms=2000, text="Hello world")
    overlay.set_segment(seg, anchor_y_pct=70.0)

    assert overlay.current_segment == seg
    assert overlay.anchor_y_pct == 70.0


def test_caption_overlay_mouse_drag(qtbot):
    parent = QWidget()
    parent.resize(800, 600)
    overlay = CaptionOverlayWidget(parent)
    qtbot.addWidget(parent)
    parent.show()

    seg = CaptionSegment(start_ms=1000, end_ms=2000, text="Drag me")
    overlay.set_segment(seg, anchor_y_pct=80.0)

    emitted_anchors = []
    overlay.anchor_changed.connect(emitted_anchors.append)

    # Click near 80% height (y = 480) and drag to 50% height (y = 300)
    start_pos = QPoint(400, 480)
    drag_pos = QPoint(400, 300)

    with qtbot.wait_signals([overlay.anchor_changed]):
        qtbot.mousePress(overlay, Qt.MouseButton.LeftButton, pos=start_pos)
        qtbot.mouseMove(overlay, pos=drag_pos)
        qtbot.mouseRelease(overlay, Qt.MouseButton.LeftButton, pos=drag_pos)

    assert len(emitted_anchors) > 0
    # Last emitted anchor should be ~50.0% (300 / 600 * 100)
    final_anchor = emitted_anchors[-1]
    assert 48.0 <= final_anchor <= 52.0
