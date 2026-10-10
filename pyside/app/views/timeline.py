from PySide6.QtCore import QPoint, QRectF, Qt, Signal
from PySide6.QtGui import (
    QColor,
    QFont,
    QFontMetrics,
    QMouseEvent,
    QPainter,
    QPainterPath,
    QPen,
    QPolygonF,
)
from PySide6.QtWidgets import QWidget

from app.models.captions import CaptionSegment


class VisualTimelineWidget(QWidget):
    seek_requested = Signal(int)
    segment_selected = Signal(object)
    trim_range_changed = Signal(int, int)

    def __init__(self, parent: QWidget | None = None):
        super().__init__(parent)
        self.setFixedHeight(84)
        self.setMouseTracking(True)

        self.duration_ms: int = 0
        self.current_position_ms: int = 0
        self.segments: list[CaptionSegment] = []
        self.selected_segment: CaptionSegment | None = None
        self._is_scrubbing: bool = False
        self.trim_start_ms = 0
        self.trim_end_ms = 0
        self._trim_drag_handle: str | None = None
        self._trim_min_gap_ms = 100
        self.setToolTip(
            "Drag the blue trim handles to shorten the beginning or end,\n"
            "or press I / O to set the start / end at the playhead"
        )

    def set_duration(self, duration_ms: int) -> None:
        old_duration = self.duration_ms
        self.duration_ms = max(0, int(duration_ms))
        if old_duration == 0 or self.trim_end_ms == old_duration:
            self.trim_end_ms = self.duration_ms
        self.trim_start_ms = min(
            self.trim_start_ms, max(0, self.duration_ms - self._trim_min_gap_ms)
        )
        self.trim_end_ms = max(
            self.trim_start_ms + min(self._trim_min_gap_ms, self.duration_ms),
            min(self.trim_end_ms, self.duration_ms),
        )
        self.update()

    def set_trim_range(self, start_ms: int, end_ms: int) -> None:
        gap = min(self._trim_min_gap_ms, self.duration_ms)
        start = max(0, min(int(start_ms), max(0, self.duration_ms - gap)))
        end = max(start + gap, min(int(end_ms), self.duration_ms))
        self.trim_start_ms, self.trim_end_ms = start, end
        self.update()

    def set_trim_start_at(self, pos_ms: int) -> None:
        """Start the trim at `pos_ms` (the I key). Past the current end, the end goes back
        to the video's end rather than dragging along."""
        gap = min(self._trim_min_gap_ms, self.duration_ms)
        end = self.trim_end_ms if pos_ms < self.trim_end_ms - gap else self.duration_ms
        self.set_trim_range(pos_ms, end)
        self.trim_range_changed.emit(self.trim_start_ms, self.trim_end_ms)

    def set_trim_end_at(self, pos_ms: int) -> None:
        """End the trim at `pos_ms` (the O key). Before the current start, the start goes
        back to the video's start."""
        gap = min(self._trim_min_gap_ms, self.duration_ms)
        start = self.trim_start_ms if pos_ms > self.trim_start_ms + gap else 0
        self.set_trim_range(start, pos_ms)
        self.trim_range_changed.emit(self.trim_start_ms, self.trim_end_ms)

    def reset_trim_range(self) -> None:
        self.set_trim_range(0, self.duration_ms)

    def set_position(self, position_ms: int) -> None:
        if not self._is_scrubbing:
            self.current_position_ms = max(0, min(self.duration_ms, int(position_ms)))
            self.update()

    def set_segments(self, segments: list[CaptionSegment]) -> None:
        self.segments = list(segments)
        self.update()

    def select_segment(self, segment: CaptionSegment | None) -> None:
        self.selected_segment = segment
        self.update()

    def _ms_to_x(self, ms: int) -> float:
        if self.duration_ms <= 0:
            return 0.0
        return (ms / self.duration_ms) * max(0, self.width() - 1)

    def _x_to_ms(self, x: float) -> int:
        if self.width() <= 0 or self.duration_ms <= 0:
            return 0
        pct = max(0.0, min(1.0, x / max(1, self.width() - 1)))
        return int(pct * self.duration_ms)

    def _trim_handle_at(self, x: float) -> str | None:
        if self.duration_ms <= 0:
            return None
        distances = {
            "start": abs(x - self._ms_to_x(self.trim_start_ms)),
            "end": abs(x - self._ms_to_x(self.trim_end_ms)),
        }
        handle = min(distances, key=distances.get)
        return handle if distances[handle] <= 10 else None

    def mousePressEvent(self, event: QMouseEvent) -> None:
        if event.button() == Qt.MouseButton.LeftButton:
            self._trim_drag_handle = self._trim_handle_at(event.position().x())
            if self._trim_drag_handle:
                self.setCursor(Qt.CursorShape.SizeHorCursor)
                event.accept()
                return
            self._is_scrubbing = True
            target_ms = self._x_to_ms(event.position().x())
            self.current_position_ms = target_ms

            # Check if clicked inside a segment
            clicked_seg = None
            for seg in self.segments:
                if seg.start_ms <= target_ms <= seg.end_ms:
                    clicked_seg = seg
                    break

            self.selected_segment = clicked_seg
            if clicked_seg:
                self.segment_selected.emit(clicked_seg)

            self.seek_requested.emit(target_ms)
            self.update()
            event.accept()
        else:
            super().mousePressEvent(event)

    def mouseMoveEvent(self, event: QMouseEvent) -> None:
        if self._trim_drag_handle:
            target_ms = self._x_to_ms(event.position().x())
            gap = min(self._trim_min_gap_ms, self.duration_ms)
            if self._trim_drag_handle == "start":
                self.trim_start_ms = min(target_ms, self.trim_end_ms - gap)
            else:
                self.trim_end_ms = max(target_ms, self.trim_start_ms + gap)
            self.trim_range_changed.emit(self.trim_start_ms, self.trim_end_ms)
            self.update()
            event.accept()
        elif self._is_scrubbing:
            target_ms = self._x_to_ms(event.position().x())
            self.current_position_ms = target_ms
            self.seek_requested.emit(target_ms)
            self.update()
            event.accept()
        else:
            self.setCursor(
                Qt.CursorShape.SizeHorCursor
                if self._trim_handle_at(event.position().x())
                else Qt.CursorShape.ArrowCursor
            )
            super().mouseMoveEvent(event)

    def mouseReleaseEvent(self, event: QMouseEvent) -> None:
        if event.button() == Qt.MouseButton.LeftButton and self._trim_drag_handle:
            self._trim_drag_handle = None
            self.unsetCursor()
            event.accept()
            return
        if event.button() == Qt.MouseButton.LeftButton and self._is_scrubbing:
            self._is_scrubbing = False
            event.accept()
        else:
            super().mouseReleaseEvent(event)

    def paintEvent(self, _event) -> None:
        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing)
        painter.setRenderHint(QPainter.RenderHint.TextAntialiasing)

        w = self.width()
        h = self.height()
        if w <= 0 or h <= 0:
            return

        # Background
        painter.fillRect(0, 0, w, h, QColor(24, 24, 27))

        # Ruler section (top 20px)
        ruler_h = 20
        painter.fillRect(0, 0, w, ruler_h, QColor(18, 18, 20))
        painter.setPen(QColor(63, 63, 70))
        painter.drawLine(0, ruler_h, w, ruler_h)

        # Draw time ruler tick marks
        if self.duration_ms > 0:
            font = QFont("Helvetica Neue", 8)
            painter.setFont(font)
            painter.setPen(QColor(161, 161, 170))

            # Intervals: 1s ticks, 5s labels if duration < 60s, else 10s
            step_sec = 5 if self.duration_ms < 60_000 else 10
            total_sec = int(self.duration_ms / 1000)
            for sec in range(0, total_sec + 1, step_sec):
                x = self._ms_to_x(sec * 1000)
                painter.drawLine(int(x), ruler_h - 6, int(x), ruler_h)
                mins = sec // 60
                secs = sec % 60
                label = f"{mins:02d}:{secs:02d}"
                painter.drawText(
                    QRectF(x + 2, 2, 40, 14), Qt.AlignmentFlag.AlignLeft, label
                )

        # Caption track section (from y=24 to y=h-6)
        track_y = ruler_h + 4
        track_h = h - track_y - 4

        font_cue = QFont("Helvetica Neue", 10, QFont.Weight.Medium)
        metrics = QFontMetrics(font_cue)
        painter.setFont(font_cue)

        for seg in self.segments:
            x_start = self._ms_to_x(seg.start_ms)
            x_end = self._ms_to_x(seg.end_ms)
            seg_w = max(6.0, x_end - x_start)

            block_rect = QRectF(x_start, track_y, seg_w, track_h)
            path = QPainterPath()
            path.addRoundedRect(block_rect, 4, 4)

            is_selected = seg == self.selected_segment
            bg_color = (
                QColor(49, 46, 129, 220) if is_selected else QColor(39, 39, 42, 220)
            )
            border_color = QColor(129, 140, 248) if is_selected else QColor(82, 82, 91)

            painter.fillPath(path, bg_color)
            painter.setPen(QPen(border_color, 1.5 if is_selected else 1.0))
            painter.drawPath(path)

            # Draw text inside block with clipping/elision
            if seg_w > 20:
                text = seg.text.strip()
                elided = metrics.elidedText(
                    text, Qt.TextElideMode.ElideRight, int(seg_w - 8)
                )
                painter.setPen(
                    QColor(255, 255, 255) if is_selected else QColor(228, 228, 231)
                )
                text_rect = QRectF(
                    x_start + 4, track_y + (track_h - 16) / 2, seg_w - 8, 16
                )
                painter.drawText(
                    text_rect,
                    Qt.AlignmentFlag.AlignLeft | Qt.AlignmentFlag.AlignVCenter,
                    elided,
                )

        # Dim the portions that will be removed, leaving the selected range clear.
        if self.duration_ms > 0:
            start_x = self._ms_to_x(self.trim_start_ms)
            end_x = self._ms_to_x(self.trim_end_ms)
            painter.fillRect(QRectF(0, 0, start_x, h), QColor(0, 0, 0, 105))
            painter.fillRect(
                QRectF(end_x, 0, max(0.0, w - end_x), h), QColor(0, 0, 0, 105)
            )
            painter.setPen(QPen(QColor(96, 165, 250), 3))
            painter.drawLine(int(start_x), 0, int(start_x), h)
            painter.drawLine(int(end_x), 0, int(end_x), h)
            painter.setBrush(QColor(96, 165, 250))
            painter.setPen(Qt.PenStyle.NoPen)
            for handle_x in (start_x, end_x):
                painter.drawRoundedRect(QRectF(handle_x - 4, 14, 8, h - 20), 3, 3)

        # Playhead scrubber needle
        if self.duration_ms > 0:
            needle_x = self._ms_to_x(self.current_position_ms)
            needle_color = QColor(239, 68, 68)  # Red playhead

            # Vertical needle line
            painter.setPen(QPen(needle_color, 2))
            painter.drawLine(int(needle_x), 0, int(needle_x), h)

            # Triangular scrubber head at the top
            triangle = QPolygonF(
                [
                    QPoint(int(needle_x) - 6, 0),
                    QPoint(int(needle_x) + 6, 0),
                    QPoint(int(needle_x), 10),
                ]
            )
            painter.setBrush(needle_color)
            painter.setPen(Qt.PenStyle.NoPen)
            painter.drawPolygon(triangle)
