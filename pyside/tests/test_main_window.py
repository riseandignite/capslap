from pathlib import Path

import pytest

from app.models.captions import CaptionSegment
from app.views.main_window import MainWindow


def _make_video_file(path: Path) -> str:
    path.write_bytes(b"dummy")
    return str(path)


def test_main_window_init(qtbot):
    window = MainWindow()
    qtbot.addWidget(window)
    window.show()

    assert window.windowTitle().startswith("PyCapSlap")
    assert window.player is not None
    assert window.core is not None
    assert window.overlay is not None
    assert window.timeline is not None
    assert window.caption_panel is not None
    assert window.save_btn is not None
    assert window.render_btn is not None
    assert "Render" in window.render_btn.text()
    window.close()


def test_main_window_load_1080p_video(qtbot):
    repo_root = Path(__file__).resolve().parents[2]
    sample = str((repo_root / "rust" / "bin" / "test_input.mp4").resolve())
    assert Path(sample).exists(), "Sample 1080p video must exist"

    window = MainWindow()
    qtbot.addWidget(window)
    window.show()

    window.load_video(sample)
    qtbot.waitUntil(lambda: window.thumb_btn.isEnabled(), timeout=5000)
    assert window.render_btn.isEnabled()
    assert window.save_btn.isEnabled()
    assert "test_input.mp4" in window.meta_lbl.text()

    # Test seek
    window.player.seek_to_ms(2000)
    assert window.player.slider.value() >= 0

    window.close()


def test_main_window_caption_sync(qtbot):
    window = MainWindow()
    qtbot.addWidget(window)
    window.show()

    seg1 = CaptionSegment(start_ms=1000, end_ms=3000, text="Synchronized cue")
    window.set_caption_segments([seg1])

    assert len(window.timeline.segments) == 1
    assert len(window.caption_panel.segments) == 1
    assert window.caption_panel.cue_table.rowCount() == 1

    # Simulate playhead moving inside seg1 (pos = 2000)
    window._on_position_changed(2000)
    assert window.overlay.current_segment == seg1

    # Simulate moving outside seg1 (pos = 4000)
    window._on_position_changed(4000)
    assert window.overlay.current_segment is None

    # Test dragging overlay anchor
    window.overlay.set_segment(seg1, 80.0)
    window._on_overlay_anchor_changed(45.0)
    assert window.project.get_anchor_y_for_segment(seg1) == 45.0
    assert window.caption_panel.slider_anchor.value() == 45

    window.close()


def test_main_window_save_action(qtbot, tmp_path):
    window = MainWindow()
    qtbot.addWidget(window)

    temp_video = tmp_path / "dummy.mp4"
    _make_video_file(temp_video)

    window.load_video(str(temp_video))
    seg = CaptionSegment(start_ms=0, end_ms=1000, text="Saved test")
    window.set_caption_segments([seg])

    window.save_btn.click()

    expected_sidecar = tmp_path / "dummy.mp4.capslap.json"
    assert expected_sidecar.exists()
    assert "Saved test" in expected_sidecar.read_text()
    window.close()


def test_main_window_style_selection_and_sidecar(qtbot, tmp_path):
    window = MainWindow()
    qtbot.addWidget(window)

    temp_video = tmp_path / "styled.mp4"
    _make_video_file(temp_video)

    window.load_video(str(temp_video))

    # Switch template to karaoke
    idx = window.caption_panel.combo_template.findData("karaoke")
    window.caption_panel.combo_template.setCurrentIndex(idx)

    assert window.project.style.karaoke is True
    assert window.player.canvas.style.karaoke is True

    # Save and verify style in sidecar
    window.save_btn.click()
    sidecar = tmp_path / "styled.mp4.capslap.json"
    assert sidecar.exists()
    content = sidecar.read_text()
    assert "karaoke" in content
    window.close()


def test_main_window_clean_sidebar_layout(qtbot):
    window = MainWindow()
    qtbot.addWidget(window)
    window.show()

    # The right sidebar widget should contain caption_panel as its primary child
    # and should NOT contain meta_box, thumb_box, or perf_box in its visible layout
    right_widget = window.caption_panel.parentWidget()
    assert right_widget is not None
    layout = right_widget.layout()
    assert layout is not None

    # Verify only caption_panel is added to the layout
    layout_widgets = [
        layout.itemAt(i).widget()
        for i in range(layout.count())
        if layout.itemAt(i).widget() is not None
    ]
    assert layout_widgets == [window.caption_panel]

    # Check that style section in caption panel starts collapsed
    assert not window.caption_panel.style_content.isVisible()
    window.close()


def test_main_window_core_progress_signal(qtbot):
    window = MainWindow()
    qtbot.addWidget(window)
    window.show()

    # Normal progress event
    window._on_core_progress("op-render-12345", "Exporting...", 0.45)
    assert "45%" in window.status.currentMessage()
    assert "Exporting..." in window.status.currentMessage()

    # Inverted arguments safeguard
    window._on_core_progress("op-render-12345", 0.75, "Encoding...")
    assert "75%" in window.status.currentMessage()
    assert "Encoding..." in window.status.currentMessage()

    # String / invalid progress safeguard
    window._on_core_progress("op-render-12345", "Analyzing frames", "invalid")
    assert "Analyzing frames" in window.status.currentMessage()

    # Active render button text update
    window.render_btn.setEnabled(False)
    window.render_btn.setText("Rendering...")
    window._on_core_progress("op-render-12345", "Exporting...", 0.88)
    assert "88%" in window.render_btn.text()

    window.close()


def test_main_window_progress_bar_and_empty_segments_on_load(qtbot, tmp_path):
    window = MainWindow()
    qtbot.addWidget(window)
    window.show()

    assert hasattr(window, "progress_bar")
    assert not window.progress_bar.isVisible()

    temp_video = tmp_path / "fresh_video.mp4"
    _make_video_file(temp_video)

    window.load_video(str(temp_video))
    # Fresh video with no sidecar must start with clean empty segments!
    assert window.project.segments == []
    assert window.caption_panel.segments == []
    window.close()


def test_main_window_transcribe_params(qtbot, tmp_path):
    temp_video = tmp_path / "video.mp4"
    _make_video_file(temp_video)

    called_method = None
    called_params = None

    from PySide6.QtCore import QObject, Signal

    class MockCore(QObject):
        progress = Signal(str, str, float)

        def call(self, method, params):
            nonlocal called_method, called_params
            called_method = method
            called_params = params
            from concurrent.futures import Future

            f = Future()
            f.set_result({"transcription": {"segments": []}})
            return f

        def close(self):
            pass

    window = MainWindow(core_client=MockCore())
    qtbot.addWidget(window)
    window.load_video(str(temp_video))

    window._on_transcribe_requested()
    assert called_method == "transcribe"
    assert "exportFormats" in called_params
    assert isinstance(called_params["exportFormats"], list)
    window.close()


def test_preview_layout_reaches_canvas_from_reader_thread(qtbot):
    """The core answers on its own reader thread, not the GUI thread.

    The layout has to be handed over with a thread hop that actually fires
    there; when it doesn't, the canvas silently keeps drawing its stand-in
    layout — one flowing line, no karaoke highlight — no matter what the
    style panel says.
    """
    import threading
    import time
    from concurrent.futures import Future

    from PySide6.QtCore import QObject, Signal

    cue = {
        "startMs": 0,
        "endMs": 2000,
        "lines": [
            {"words": [{"text": "JA", "isHighlighted": True}], "fontSizePx": 44},
            {"words": [{"text": "SITTEN", "isHighlighted": False}], "fontSizePx": 52},
        ],
        "yPct": 88.0,
        "anchor": "bottom",
    }

    class MockCore(QObject):
        progress = Signal(str, str, float)
        proc = None  # the telemetry timer looks for one

        def call(self, method, params):
            f: Future = Future()
            if method == "previewLayout":
                # Answer with a beat's delay so the caller has attached its
                # callback first: a Future that is already done runs the
                # callback inline on the GUI thread, and the test would pass
                # without ever crossing a thread boundary.
                def answer():
                    time.sleep(0.05)
                    f.set_result({"cues": [cue]})

                threading.Thread(target=answer, daemon=True).start()
            else:
                f.set_result({})
            return f

        def close(self):
            pass

    window = MainWindow(core_client=MockCore())
    qtbot.addWidget(window)
    window.set_caption_segments(
        [CaptionSegment(start_ms=0, end_ms=2000, text="Ja sitten")]
    )
    # Only the explicit call below should reach the mock; the debounced
    # refresh set_caption_segments queued would answer on the GUI thread.
    window._layout_timer.stop()
    window._request_preview_layout()

    qtbot.waitUntil(lambda: window._preview_cues == [cue], timeout=2000)
    assert window.overlay.layout_cue == cue
    window.close()


def _mock_core_with_layer(cue, layer_png):
    """A core that answers previewLayout with `cue` and hands back `layer_png`."""
    import threading
    import time
    from concurrent.futures import Future

    from PySide6.QtCore import QObject, Signal

    calls: list[tuple[str, dict]] = []

    class MockCore(QObject):
        progress = Signal(str, str, float)
        proc = None

        def call(self, method, params):
            calls.append((method, params))
            f: Future = Future()
            if method == "previewLayout":

                def answer():
                    time.sleep(0.05)
                    f.set_result({"cues": [cue]})

                threading.Thread(target=answer, daemon=True).start()
            elif method == "generatePreviewFrame":

                def answer_frame():
                    time.sleep(0.05)
                    f.set_result({"imageData": "data:image/png;base64," + layer_png})

                threading.Thread(target=answer_frame, daemon=True).start()
            else:
                f.set_result({})
            return f

        def close(self):
            pass

    return MockCore(), calls


def test_caption_layer_is_fetched_and_cached_per_cue(qtbot):
    """The preview shows libass's pixels, and asks for each cue only once."""
    import base64

    from PySide6.QtCore import QBuffer, QIODevice
    from PySide6.QtGui import QColor, QImage

    img = QImage(8, 8, QImage.Format.Format_ARGB32)
    img.fill(QColor(0, 255, 0, 255))
    buf = QBuffer()
    buf.open(QIODevice.OpenModeFlag.WriteOnly)
    img.save(buf, "PNG")
    png_b64 = base64.b64encode(bytes(buf.data())).decode()

    cue = {
        "startMs": 0,
        "endMs": 2000,
        "lines": [{"words": [{"text": "JA", "isHighlighted": True}], "fontSizePx": 44}],
        "yPct": 88.0,
        "anchor": "bottom",
    }
    core, calls = _mock_core_with_layer(cue, png_b64)

    window = MainWindow(core_client=core)
    qtbot.addWidget(window)
    from app.models.captions import VideoMetadata

    window.project.video = VideoMetadata(
        path="/tmp/does-not-need-to-exist.mp4", width=1080, height=1920
    )
    window.set_caption_segments(
        [CaptionSegment(start_ms=0, end_ms=2000, text="Ja sitten")]
    )
    window._layout_timer.stop()
    window._request_preview_layout()

    qtbot.waitUntil(lambda: window.overlay.caption_layer is not None, timeout=3000)

    frame_calls = [c for c in calls if c[0] == "generatePreviewFrame"]
    assert len(frame_calls) == 1
    params = frame_calls[0][1]
    assert params["renderMode"] == "captions"
    assert params["exportFormat"] == "source"
    # Rendered past the karaoke pop, inside the cue.
    assert cue["startMs"] <= params["timestampMs"] < cue["endMs"]

    # Same cue again: served from cache, no second render.
    window._apply_preview_layout()
    assert len([c for c in calls if c[0] == "generatePreviewFrame"]) == 1

    # A style change invalidates every rendered cue.
    window._invalidate_caption_layers()
    assert window.overlay.caption_layer is None
    assert window._layer_cache == {}
    window.close()


def test_block_prefetch_renders_every_karaoke_window_once(qtbot):
    """One window on screen pulls in the whole block, a few renders at a time.

    Karaoke shows a caption one word at a time; without this, playback would
    fall back to the painted approximation for every window the user has not
    stopped on.
    """
    import base64

    from PySide6.QtCore import QBuffer, QIODevice
    from PySide6.QtGui import QColor, QImage

    img = QImage(4, 4, QImage.Format.Format_ARGB32)
    img.fill(QColor(0, 255, 0, 255))
    buf = QBuffer()
    buf.open(QIODevice.OpenModeFlag.WriteOnly)
    img.save(buf, "PNG")
    png_b64 = base64.b64encode(bytes(buf.data())).decode()

    words = ["JA", "SITTEN", "ON", "ERIKSEEN", "KANNETTAVA", "TIETOKONE"]
    cues = [
        {
            "startMs": i * 300,
            "endMs": (i + 1) * 300,
            "lines": [
                {
                    "words": [{"text": w, "isHighlighted": w == word} for w in words],
                    "fontSizePx": 44,
                }
            ],
            "yPct": 88.0,
            "anchor": "bottom",
            "groupStartMs": 0,
            "groupEndMs": 1800,
        }
        for i, word in enumerate(words)
    ]
    core, calls = _mock_core_with_layer(cues[0], png_b64)

    window = MainWindow(core_client=core)
    qtbot.addWidget(window)
    from app.models.captions import VideoMetadata

    window.project.video = VideoMetadata(path="/tmp/nope.mp4", width=1080, height=1920)
    window.project.segments = [
        CaptionSegment(start_ms=0, end_ms=1800, text=" ".join(words))
    ]
    window._preview_cues = cues
    window._layout_timer.stop()

    window._sync_caption_layer(cues[0])
    # Never more than a few ffmpeg pairs at once.
    assert len(window._layer_pending) <= 3

    def rendered():
        return {c[1]["timestampMs"] for c in calls if c[0] == "generatePreviewFrame"}

    qtbot.waitUntil(lambda: len(window._layer_cache) == len(cues), timeout=5000)
    # Every window rendered, each exactly once.
    frame_calls = [c for c in calls if c[0] == "generatePreviewFrame"]
    assert len(frame_calls) == len(cues)
    assert len(rendered()) == len(cues)

    # Coming back to a window in the block costs nothing.
    window._sync_caption_layer(cues[3])
    assert window.overlay.caption_layer is not None
    assert len([c for c in calls if c[0] == "generatePreviewFrame"]) == len(cues)
    window.close()


def test_export_format_drives_layout_layer_and_render(qtbot):
    """One canvas for the preview and the burn, chosen in the editor."""
    from PySide6.QtCore import QObject, Signal

    calls: list[tuple[str, dict]] = []

    class MockCore(QObject):
        progress = Signal(str, str, float)
        proc = None

        def call(self, method, params):
            from concurrent.futures import Future

            calls.append((method, params))
            f: Future = Future()
            f.set_result({"cues": [], "frameWidth": 1920, "frameHeight": 3414})
            return f

        def close(self):
            pass

    window = MainWindow(core_client=MockCore())
    qtbot.addWidget(window)
    from app.models.captions import VideoMetadata

    window.project.video = VideoMetadata(path="/tmp/nope.mp4", width=1920, height=1080)
    window.project.segments = [CaptionSegment(start_ms=0, end_ms=2000, text="Ja")]

    index = window.format_combo.findData("9:16")
    assert index >= 0
    window.format_combo.setCurrentIndex(index)
    window._layout_timer.stop()
    window._request_preview_layout()

    layout_calls = [c for c in calls if c[0] == "previewLayout"]
    assert layout_calls[-1][1]["exportFormat"] == "9:16"
    # The renderer's answer decides the canvas the editor draws.
    qtbot.waitUntil(lambda: window._canvas_size is not None, timeout=2000)
    assert window._canvas_size == (1920, 3414)
    assert window.player.canvas.export_canvas_size == (1920, 3414)

    # And the layer for a cue is rendered on that same canvas.
    window._request_caption_layer({"startMs": 0, "endMs": 2000})
    frame_calls = [c for c in calls if c[0] == "generatePreviewFrame"]
    assert frame_calls[-1][1]["exportFormat"] == "9:16"
    window.close()


def test_i_and_o_set_the_trim_at_the_playhead(qtbot):
    from PySide6.QtGui import QShortcut

    window = MainWindow()
    qtbot.addWidget(window)
    window._on_duration_changed(10000)

    keys = {s.key().toString() for s in window.findChildren(QShortcut)}
    assert {"I", "O"} <= keys

    window.player.media_player.position = lambda: 2500
    window._set_trim_start_here()
    window.player.media_player.position = lambda: 7400
    window._set_trim_end_here()

    assert (window.timeline.trim_start_ms, window.timeline.trim_end_ms) == (2500, 7400)
    # The player follows, so Stop and playback use the new trim.
    assert (window.player._range_start_ms, window.player._range_end_ms) == (2500, 7400)
    assert "00:07.4" in window.status.currentMessage()
    window.close()


def test_i_and_o_buttons_set_the_trim_at_the_playhead(qtbot):
    window = MainWindow()
    qtbot.addWidget(window)
    window._on_duration_changed(10000)

    window.player.media_player.position = lambda: 1500
    window.player.mark_in_btn.click()
    window.player.media_player.position = lambda: 6000
    window.player.mark_out_btn.click()

    assert (window.timeline.trim_start_ms, window.timeline.trim_end_ms) == (1500, 6000)
    assert (window.player._range_start_ms, window.player._range_end_ms) == (1500, 6000)
    window.close()


def test_import_review_applies_fixes_and_shows_comments(
    qtbot, tmp_path, no_modal_message_boxes
):
    import json

    window = MainWindow()
    qtbot.addWidget(window)
    window.set_caption_segments(
        [
            CaptionSegment(0, 2000, "Kylläpä on sää"),
            CaptionSegment(2000, 4000, "tekoäly yhtiöt"),
        ]
    )
    reviewed = {
        "segments": [
            {"startMs": 0, "endMs": 2000, "text": "Kyllä on sää"},
            {"startMs": 2000, "endMs": 4000, "text": "tekoäly yhtiöt"},
        ],
        "review": {
            "status": "changed",
            "reviewer": "Asiakas",
            "comments": [{"index": 1, "text": "Yhdyssana?"}],
        },
    }
    path = tmp_path / "talk.mp4.reviewed.capslap.json"
    path.write_text(json.dumps(reviewed), encoding="utf-8")

    window.import_review_file(str(path))

    assert [s.text for s in window.caption_panel.segments] == [
        "Kyllä on sää",
        "tekoäly yhtiöt",
    ]
    assert window.project.is_dirty
    kind, text = no_modal_message_boxes[-1]
    assert kind == "information"
    assert "1 text change(s)" in text and "Yhdyssana?" in text
    window.close()


def test_import_review_of_other_captions_changes_nothing(
    qtbot, tmp_path, no_modal_message_boxes
):
    import json

    window = MainWindow()
    qtbot.addWidget(window)
    window.set_caption_segments([CaptionSegment(0, 2000, "Yksi")])
    path = tmp_path / "other.json"
    path.write_text(
        json.dumps({"segments": [{"startMs": 0, "endMs": 1, "text": "a"}] * 2}),
        encoding="utf-8",
    )

    window.import_review_file(str(path))

    assert [s.text for s in window.caption_panel.segments] == ["Yksi"]
    assert no_modal_message_boxes[-1][0] == "warning"
    window.close()


def test_export_locks_the_file_and_the_reply_opens_with_the_same_password(
    qtbot, tmp_path, monkeypatch, no_modal_message_boxes
):
    import json

    from app.models.project import VideoMetadata
    from app.models.review import is_locked, lock_file, unlock_file

    window = MainWindow()
    qtbot.addWidget(window)
    window.set_caption_segments([CaptionSegment(0, 2000, "Kylläpä on sää")])
    window.project.video = VideoMetadata(
        path=str(tmp_path / "talk.mp4"), width=1920, height=1080, duration_sec=2.0
    )
    out = tmp_path / "talk.mp4.review.capslap.json"
    monkeypatch.setattr(
        "app.views.main_window.QFileDialog.getSaveFileName",
        lambda *a, **k: (str(out), ""),
    )
    monkeypatch.setattr(window, "_ask_export_password", lambda: "k7mq-x2fp")

    window._on_export_for_review()

    locked = json.loads(out.read_text(encoding="utf-8"))
    assert is_locked(locked)
    sent = unlock_file(locked, "k7mq-x2fp")
    assert sent["segments"][0]["text"] == "Kylläpä on sää"

    # The client's reply comes back locked with the same password: no prompt.
    sent["segments"][0]["text"] = "Kyllä on sää"
    reply = tmp_path / "talk.mp4.reviewed.capslap.json"
    reply.write_text(json.dumps(lock_file(sent, "k7mq-x2fp", iterations=1000)))
    monkeypatch.setattr(
        window,
        "_ask_import_password",
        lambda wrong: pytest.fail("asked for a password it already had"),
    )
    window.import_review_file(str(reply))
    assert window.caption_panel.segments[0].text == "Kyllä on sää"
    window.close()


def test_import_asks_again_after_a_wrong_password(
    qtbot, tmp_path, monkeypatch, no_modal_message_boxes
):
    import json

    from app.models.review import lock_file

    window = MainWindow()
    qtbot.addWidget(window)
    window.set_caption_segments([CaptionSegment(0, 2000, "Yksi")])
    reply = tmp_path / "reply.json"
    reply.write_text(
        json.dumps(
            lock_file(
                {"segments": [{"startMs": 0, "endMs": 2000, "text": "Kaksi"}]},
                "oikea",
                iterations=1000,
            )
        )
    )
    answers = iter(["väärä", "oikea"])
    asked = []

    def ask(wrong):
        asked.append(wrong)
        return next(answers)

    monkeypatch.setattr(window, "_ask_import_password", ask)
    window.import_review_file(str(reply))
    assert asked == [False, True]
    assert window.caption_panel.segments[0].text == "Kaksi"

    # Cancelling the prompt leaves the captions alone.
    monkeypatch.setattr(window, "_ask_import_password", lambda wrong: None)
    window.set_caption_segments([CaptionSegment(0, 2000, "Yksi")])
    window.import_review_file(str(reply))
    assert window.caption_panel.segments[0].text == "Yksi"
    window.close()


def test_render_asks_for_the_chosen_proof_copy(qtbot, tmp_path):
    from PySide6.QtCore import QObject, QUrl, Signal

    calls: list[tuple[str, dict]] = []

    class MockCore(QObject):
        progress = Signal(str, str, float)
        proc = None

        def call(self, method, params):
            from concurrent.futures import Future

            calls.append((method, params))
            f: Future = Future()
            if method == "burn":
                f.set_result(
                    [
                        {
                            "captionedVideo": "/out/talk_16x9.mp4",
                            "proofVideo": "/out/talk_16x9_proof720p.mp4",
                        }
                    ]
                )
            else:
                f.set_result({"cues": [], "frameWidth": 1920, "frameHeight": 1080})
            return f

        def close(self):
            pass

    window = MainWindow(core_client=MockCore())
    qtbot.addWidget(window)
    video = tmp_path / "talk.mp4"
    video.write_bytes(b"x")
    window.player.media_player.source = lambda: QUrl.fromLocalFile(str(video))
    window.set_caption_segments([CaptionSegment(0, 2000, "Ja")])

    # 720p is the default: the proof copy is on unless turned off.
    assert window.proof_combo.currentData() == 720
    window._on_render_video_requested()
    burn = [p for m, p in calls if m == "burn"][-1]
    assert burn["proofShortSide"] == 720
    qtbot.waitUntil(lambda: "proof" in window.status.currentMessage(), timeout=2000)
    assert "talk_16x9_proof720p.mp4" in window.status.currentMessage()

    window.proof_combo.setCurrentIndex(window.proof_combo.findData(0))
    window._on_render_video_requested()
    burn = [p for m, p in calls if m == "burn"][-1]
    assert burn["proofShortSide"] is None
    # No intro or outro chosen: none asked for.
    assert (burn["introVideo"], burn["outroVideo"]) == (None, None)

    # Chosen clips go with every render, and outlive the window.
    window.set_bumper("outro", "/clips/listen-now.mp4")
    window._on_render_video_requested()
    burn = [p for m, p in calls if m == "burn"][-1]
    assert (burn["introVideo"], burn["outroVideo"]) == (None, "/clips/listen-now.mp4")
    assert window.bumpers_btn.text() == "+ outro"
    window.close()

    again = MainWindow(core_client=MockCore())
    qtbot.addWidget(again)
    assert again.bumper_path("outro") == "/clips/listen-now.mp4"
    again._fill_bumpers_menu()
    labels = [a.text() for a in again.bumpers_menu.actions() if a.text()]
    assert "Outro: listen-now.mp4" in labels and "No Outro" in labels
    assert "No Intro" not in labels
    again.set_bumper("outro", "")
    assert again.bumpers_btn.text() == "No intro/outro"
    again.close()


def test_core_replies_reach_the_gui_thread(qtbot):
    """Work handed over from the core's reader thread runs on the GUI thread, later."""
    import threading

    from PySide6.QtCore import QThread

    window = MainWindow()
    qtbot.addWidget(window)
    ran_on = []

    def from_reader():
        window._in_gui(lambda: ran_on.append(QThread.currentThread()))

    worker = threading.Thread(target=from_reader)
    worker.start()
    worker.join()
    assert ran_on == []  # queued, not run on the reader thread
    qtbot.waitUntil(lambda: bool(ran_on), timeout=2000)
    assert ran_on[0] == window.thread()
    window.close()


def test_the_logo_goes_into_the_render_unless_turned_off_for_the_video(qtbot, tmp_path):
    from PySide6.QtGui import QColor, QImage

    from app.models.project import VideoMetadata

    logo = tmp_path / "logo.png"
    image = QImage(40, 20, QImage.Format.Format_ARGB32)
    image.fill(QColor(255, 0, 0))
    image.save(str(logo))

    window = MainWindow()
    qtbot.addWidget(window)
    assert window.watermark_params() is None
    assert window.bug_btn.text() == "No logo"

    window.set_bug_path(str(logo))
    window.set_bug_style("corner", "bottom-left")
    window.set_bug_style("sizePct", 8.0)
    assert window.watermark_params() == {
        "path": str(logo),
        "corner": "bottom-left",
        "sizePct": 8.0,
        "opacity": 0.85,
    }
    assert window.player.canvas.watermark is not None
    assert window.bug_btn.text() == "+ logo"

    # This video has its own bug: off for it, still on for others.
    window.project.video = VideoMetadata(path=str(tmp_path / "has-bug.mp4"))
    window.set_bug_on_this_video(False)
    assert window.watermark_params() is None
    assert window.player.canvas.watermark is None
    assert window.bug_btn.text() == "Logo off"
    window.project.video = VideoMetadata(path=str(tmp_path / "plain.mp4"))
    window._apply_bug()
    assert window.watermark_params() is not None

    window._fill_bug_menu()
    labels = [a.text() for a in window.bug_menu.actions()]
    assert "Logo: logo.png" in labels and "Put it on this video" in labels
    window.close()


def test_broll_button_waits_for_a_transcript(qtbot, no_modal_message_boxes):
    window = MainWindow()
    qtbot.addWidget(window)
    assert not window.broll_btn.isEnabled()
    window._on_broll()
    assert no_modal_message_boxes[-1][0] == "information"
    window.close()


def test_broll_flow_searches_asks_and_builds(qtbot, tmp_path, monkeypatch):
    from app.services.broll.assets import Asset
    from app.views import main_window as mw

    found = [
        {
            "startMs": 0,
            "endMs": 2000,
            "text": "tram",
            "query": "tram",
            "candidates": [
                Asset(
                    title="A.jpg", kind="image", url="u", page_url="p", license="CC0"
                ).to_dict()
            ],
            "chosen": None,
        }
    ]
    built = {}

    class Chooser:
        transition = "whip"

        def __init__(self, picks, parent=None):
            picks.accept(0, 0)

        def exec(self):
            return mw.QDialog.DialogCode.Accepted

    monkeypatch.setattr(mw.broll_suggest, "proposals", lambda segs: found)
    monkeypatch.setattr(mw, "BrollDialog", Chooser)
    monkeypatch.setattr(
        mw.broll_pipeline,
        "build",
        lambda audio, picks, out, sidecar=None, transition=None: built.update(
            audio=audio, out=out, n=len(picks.accepted()), transition=transition
        ),
    )
    window = MainWindow()
    qtbot.addWidget(window)
    window.project.video = type("V", (), {"path": str(tmp_path / "ep.m4a")})()
    window.set_caption_segments([CaptionSegment(0, 2000, "tram")])
    (tmp_path / "ep.m4a").write_bytes(b"x")
    window.project.sidecar_path = str(tmp_path / "ep.m4a.capslap.json")

    window._on_broll()
    qtbot.waitUntil(lambda: bool(built), timeout=3000)
    assert built["n"] == 1
    assert built["out"].name == "ep.reel.mp4"
    assert built["transition"] == "whip"  # the dialog's choice reaches the build
    window.close()


def test_the_open_dialog_offers_audio_files():
    from app.views.main_window import OPEN_FILES

    assert "*.m4a" in OPEN_FILES and "*.mp3" in OPEN_FILES and "*.mp4" in OPEN_FILES


def test_rendering_an_audio_file_says_what_to_do_instead(
    qtbot, tmp_path, monkeypatch, no_modal_message_boxes
):
    from app.views import main_window as mw

    audio = tmp_path / "ep.wav"
    audio.write_bytes(b"x")
    monkeypatch.setattr(mw.media, "has_video", lambda p: False)
    window = MainWindow()
    qtbot.addWidget(window)
    window.set_caption_segments([CaptionSegment(0, 2000, "tram")])
    monkeypatch.setattr(
        window.player.media_player.__class__,
        "source",
        lambda self: mw.QUrl.fromLocalFile(str(audio)),
    )
    window._on_render_video_requested()
    kind, text = no_modal_message_boxes[-1]
    assert kind == "information"
    assert "audio" in text.lower() and "B-roll Reel" in text
    window.close()


def _transcribing_window(qtbot, monkeypatch, tmp_path, duration_ms, marks=None):
    from concurrent.futures import Future

    from app.views import main_window as mw

    audio = tmp_path / "ep.wav"
    audio.write_bytes(b"x")
    window = MainWindow()
    qtbot.addWidget(window)
    monkeypatch.setattr(
        window.player.media_player.__class__,
        "source",
        lambda self: mw.QUrl.fromLocalFile(str(audio)),
    )
    window.timeline.set_duration(duration_ms)
    if marks:
        window.timeline.set_trim_range(*marks)
    sent = []
    pending = Future()

    def call(method, params, *a, **k):
        sent.append((method, params))
        return pending

    monkeypatch.setattr(window.core, "call", call)
    return window, sent, pending


def test_transcribing_a_long_recording_without_marks_asks_for_a_range(
    qtbot, monkeypatch, tmp_path, no_modal_message_boxes
):
    window, sent, _ = _transcribing_window(qtbot, monkeypatch, tmp_path, 47 * 60_000)
    window._on_transcribe_requested()
    assert sent == []  # nothing is sent to whisper
    kind, text = no_modal_message_boxes[-1]
    assert kind == "information" and "47:00" in text and "press I" in text
    window.close()


def test_transcribing_between_the_marks_sends_only_that_range(
    qtbot, monkeypatch, tmp_path
):
    window, sent, _ = _transcribing_window(
        qtbot, monkeypatch, tmp_path, 47 * 60_000, marks=(600_000, 900_000)
    )
    window._on_transcribe_requested()
    ((method, params),) = sent
    assert method == "transcribe"
    assert (params["transcribeStartMs"], params["transcribeEndMs"]) == (
        600_000,
        900_000,
    )
    window.close()


def test_a_short_recording_is_sent_whole(qtbot, monkeypatch, tmp_path):
    window, sent, _ = _transcribing_window(qtbot, monkeypatch, tmp_path, 3 * 60_000)
    window._on_transcribe_requested()
    ((_, params),) = sent
    assert "transcribeStartMs" not in params and "transcribeEndMs" not in params
    window.close()


def test_a_transcribed_range_lands_in_place_among_the_existing_cues(
    qtbot, monkeypatch, tmp_path
):
    window, sent, pending = _transcribing_window(
        qtbot, monkeypatch, tmp_path, 47 * 60_000, marks=(600_000, 900_000)
    )
    window.set_caption_segments(
        [
            CaptionSegment(1_000, 3_000, "before"),
            CaptionSegment(650_000, 652_000, "old inside"),
            CaptionSegment(1_200_000, 1_203_000, "after"),
        ]
    )
    window._on_transcribe_requested()
    # whisper counts from the start of the part it was given
    pending.set_result(
        {
            "transcription": {
                "segments": [
                    {"startMs": 5_000, "endMs": 8_000, "text": "new one", "words": []},
                    {"startMs": 8_000, "endMs": 11_000, "text": "new two", "words": []},
                ]
            }
        }
    )
    qtbot.waitUntil(
        lambda: (
            [s.text for s in window.caption_panel.segments]
            == ["before", "new one", "new two", "after"]
        ),
        timeout=3000,
    )
    starts = [s.start_ms for s in window.caption_panel.segments]
    assert starts == [1_000, 605_000, 608_000, 1_200_000]
    window.close()
