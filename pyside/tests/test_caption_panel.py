from app.models.captions import CaptionSegment, WordSpan
from app.views.caption_panel import CaptionPanelWidget


def test_caption_panel_initial_state(qtbot):
    panel = CaptionPanelWidget()
    qtbot.addWidget(panel)

    assert panel.segments == []
    assert panel.selected_segment is None
    assert panel.active_anchor_pct == 80.0


def test_caption_panel_load_segments(qtbot):
    panel = CaptionPanelWidget()
    qtbot.addWidget(panel)

    seg1 = CaptionSegment(start_ms=1000, end_ms=2000, text="First cue")
    seg2 = CaptionSegment(start_ms=2500, end_ms=4000, text="Second cue")
    panel.set_segments([seg1, seg2])

    assert len(panel.segments) == 2
    assert panel.cue_table.rowCount() == 2
    assert panel.cue_table.item(0, 2).text() == "First cue"
    assert panel.cue_table.item(1, 2).text() == "Second cue"


def test_caption_panel_preset_position_buttons(qtbot):
    panel = CaptionPanelWidget()
    qtbot.addWidget(panel)

    seg = CaptionSegment(start_ms=1000, end_ms=2000, text="Position test")
    panel.set_segments([seg])
    panel.select_segment(seg, anchor_y_pct=80.0)

    emitted_overrides = []
    panel.position_override_changed.connect(
        lambda s, y: emitted_overrides.append((s, y))
    )

    # Click 'Top' button (15%)
    panel.btn_top.click()
    assert len(emitted_overrides) == 1
    assert emitted_overrides[-1] == (seg, 15.0)
    assert panel.slider_anchor.value() == 15

    # Click 'Middle' button (50%)
    panel.btn_middle.click()
    assert len(emitted_overrides) == 2
    assert emitted_overrides[-1] == (seg, 50.0)
    assert panel.slider_anchor.value() == 50

    # Click 'Bottom' button (80%)
    panel.btn_bottom.click()
    assert len(emitted_overrides) == 3
    assert emitted_overrides[-1] == (seg, 80.0)
    assert panel.slider_anchor.value() == 80


def test_caption_panel_style_selection(qtbot):
    panel = CaptionPanelWidget()
    qtbot.addWidget(panel)

    emitted_styles = []
    panel.style_changed.connect(emitted_styles.append)

    # Change template preset to Karaoke
    idx = panel.combo_template.findData("karaoke")
    assert idx >= 0
    panel.combo_template.setCurrentIndex(idx)

    assert len(emitted_styles) > 0
    current_style = panel.get_current_style()
    assert current_style.karaoke is True
    assert current_style.template_id == "karaoke"


def test_caption_panel_style_collapsible(qtbot):
    panel = CaptionPanelWidget()
    qtbot.addWidget(panel)
    panel.show()

    # Style content should be collapsed by default
    assert hasattr(panel, "btn_toggle_style")
    assert hasattr(panel, "style_content")
    assert not panel.style_content.isVisible()
    assert "▶" in panel.btn_toggle_style.text()

    # Click toggle to expand
    panel.btn_toggle_style.click()
    assert panel.style_content.isVisible()
    assert "▼" in panel.btn_toggle_style.text()

    # Click toggle to collapse again
    panel.btn_toggle_style.click()
    assert not panel.style_content.isVisible()
    assert "▶" in panel.btn_toggle_style.text()


def test_caption_panel_edit_cell_syncs_segment(qtbot):
    panel = CaptionPanelWidget()
    qtbot.addWidget(panel)

    seg = CaptionSegment(
        start_ms=0,
        end_ms=2000,
        text="Initial text",
        words=[
            WordSpan(start_ms=0, end_ms=1000, text="Initial"),
            WordSpan(start_ms=1000, end_ms=2000, text=" text"),
        ],
    )
    panel.set_segments([seg])

    emitted_updates = []
    panel.segment_updated.connect(emitted_updates.append)

    # Edit text in table item
    item = panel.cue_table.item(0, 2)
    item.setText("Replaced caption here")

    assert len(emitted_updates) == 1
    assert seg.text == "Replaced caption here"
    assert len(seg.words) == 3
    assert seg.words[0].text == "Replaced"
    assert seg.words[-1].text == " here"

    # Verify commit_active_editor clears selection / finishes editing
    panel.commit_active_editor()
    assert panel.cue_table.currentItem() is None


def test_caption_panel_commit_active_editor_with_open_lineedit(qtbot):
    from PySide6.QtWidgets import QLineEdit

    panel = CaptionPanelWidget()
    qtbot.addWidget(panel)
    panel.show()

    seg = CaptionSegment(
        start_ms=0,
        end_ms=2000,
        text="Original text",
        words=[
            WordSpan(start_ms=0, end_ms=1000, text="Original"),
            WordSpan(start_ms=1000, end_ms=2000, text=" text"),
        ],
    )
    panel.set_segments([seg])

    item = panel.cue_table.item(0, 2)
    panel.cue_table.editItem(item)

    editor = None
    for child in panel.cue_table.viewport().children():
        if isinstance(child, QLineEdit):
            editor = child
            break

    assert editor is not None
    editor.setText("Editor edited text")

    panel.commit_active_editor()
    assert seg.text == "Editor edited text"
    assert panel.cue_table.item(0, 2).text() == "Editor edited text"


def test_caption_panel_fix_orphans_button(qtbot):
    panel = CaptionPanelWidget()
    qtbot.addWidget(panel)

    seg1 = CaptionSegment(start_ms=0, end_ms=2000, text="Tämä on lause ja")
    seg2 = CaptionSegment(start_ms=2000, end_ms=4000, text="toinen tässä")
    panel.set_segments([seg1, seg2])

    assert hasattr(panel, "btn_fix_orphans")
    panel.btn_fix_orphans.click()

    assert panel.segments[0].text == "Tämä on lause"
    assert panel.segments[1].text == "ja toinen tässä"


def test_caption_panel_hierarchical_font_menu(qtbot):
    panel = CaptionPanelWidget()
    qtbot.addWidget(panel)

    assert hasattr(panel, "btn_font")
    assert hasattr(panel, "font_menu")
    actions = panel.font_menu.actions()
    assert len(actions) > 0

    emitted = []
    panel.style_changed.connect(emitted.append)
    panel.set_font_name("Komika Axis")
    assert panel.current_style.font_name == "Komika Axis"
    assert "Komika Axis" in panel.btn_font.text()
    assert len(emitted) > 0


def test_caption_panel_custom_preset_saving(qtbot, tmp_path, monkeypatch):
    preset_file = tmp_path / "panel_presets.json"
    from PySide6.QtWidgets import QInputDialog
    from app.services.preset_manager import PresetManager

    manager = PresetManager(storage_path=preset_file)

    panel = CaptionPanelWidget(preset_manager=manager)
    qtbot.addWidget(panel)

    panel.set_font_name("Komika Axis")
    panel.slider_font_size.setValue(77)

    monkeypatch.setattr(
        QInputDialog, "getText", lambda *args, **kwargs: ("Podcast Show", True)
    )

    panel.btn_save_preset.click()

    assert "Podcast Show" in manager.list_presets()
    idx = panel.combo_template.findData("Podcast Show")
    assert idx >= 0


def test_caption_panel_combine_syllables_button(qtbot):
    panel = CaptionPanelWidget()
    qtbot.addWidget(panel)

    seg1 = CaptionSegment(
        start_ms=0,
        end_ms=1000,
        text="kaup",
        words=[WordSpan(0, 1000, "kaup")],
    )
    seg2 = CaptionSegment(
        start_ms=1000,
        end_ms=2000,
        text="pa kes kus",
        words=[
            WordSpan(1000, 1500, "pa", glue_to_previous=True),
            WordSpan(1500, 2000, "kes kus"),
        ],
    )
    panel.set_segments([seg1, seg2])

    emitted = []
    panel.segments_updated.connect(emitted.append)

    panel.btn_combine_syllables.click()
    assert len(emitted) == 1
    assert len(panel.segments) == 2
    assert panel.segments[0].text == "kauppa"
    assert panel.segments[1].text == "kes kus"


def test_caption_panel_shift_start_and_end(qtbot):
    panel = CaptionPanelWidget()
    qtbot.addWidget(panel)

    seg1 = CaptionSegment(
        start_ms=0,
        end_ms=1000,
        text="First",
        words=[WordSpan(0, 1000, "First")],
    )
    seg2 = CaptionSegment(
        start_ms=1000,
        end_ms=2500,
        text="Second Third",
        words=[WordSpan(1000, 1800, "Second"), WordSpan(1800, 2500, "Third")],
    )
    panel.set_segments([seg1, seg2])

    # Select second segment
    panel.cue_table.selectRow(1)

    # Shift first word ("Second") to previous segment
    panel.btn_shift_prev.click()
    assert panel.segments[0].text == "First Second"
    assert panel.segments[1].text == "Third"

    # Shift it back
    panel.cue_table.selectRow(0)
    panel.btn_shift_next.click()
    assert panel.segments[0].text == "First"
    assert panel.segments[1].text == "Second Third"


def test_caption_panel_delete_empty_slot(qtbot):
    panel = CaptionPanelWidget()
    qtbot.addWidget(panel)

    seg1 = CaptionSegment(
        start_ms=0,
        end_ms=1000,
        text="First",
        words=[WordSpan(0, 1000, "First")],
    )
    seg2 = CaptionSegment(
        start_ms=1000,
        end_ms=1800,
        text="Second",
        words=[WordSpan(1000, 1800, "Second")],
    )
    panel.set_segments([seg1, seg2])

    # A populated slot cannot be deleted through this button.
    panel.cue_table.selectRow(1)
    assert not panel.btn_delete_empty.isEnabled()

    # Shift the only word of seg2 away, leaving it empty.
    panel.btn_shift_prev.click()
    assert panel.segments[1].text == ""
    assert panel.btn_delete_empty.isEnabled()

    emitted = []
    panel.segments_updated.connect(emitted.append)
    panel.btn_delete_empty.click()

    assert len(emitted) == 1
    assert len(panel.segments) == 1
    assert panel.segments[0].text == "First Second"


def test_caption_panel_safe_area_toggles(qtbot):
    panel = CaptionPanelWidget()
    qtbot.addWidget(panel)

    assert hasattr(panel, "btn_safe_tiktok")
    assert hasattr(panel, "btn_safe_reels")
    assert hasattr(panel, "btn_safe_shorts")

    # All three are on by default, so captions don't overlap platform UI or
    # (via the lower-half auto-dodge floor) faces out of the box.
    assert panel.active_safe_platforms == {"tiktok", "reels", "shorts"}
    assert panel.btn_safe_tiktok.isChecked()
    assert panel.btn_safe_reels.isChecked()
    assert panel.btn_safe_shorts.isChecked()

    emitted = []
    panel.safe_platforms_changed.connect(emitted.append)

    panel.btn_safe_tiktok.click()
    assert "tiktok" not in panel.active_safe_platforms
    assert emitted[-1] == {"reels", "shorts"}

    panel.btn_safe_reels.click()
    assert "reels" not in panel.active_safe_platforms
    assert emitted[-1] == {"shorts"}

    panel.btn_safe_tiktok.click()
    assert "tiktok" in panel.active_safe_platforms
    assert emitted[-1] == {"tiktok", "shorts"}


class _FakeChecker:
    """Stands in for Voikko so the test does not need the native library."""

    available = True

    def __init__(self, bad_words: set[str]):
        self.bad_words = bad_words

    def misspelled_spans(self, segment):
        spans = []
        pos = 0
        for token in segment.text.split(" "):
            if token in self.bad_words:
                spans.append((pos, pos + len(token), token))
            pos += len(token) + 1
        return spans

    def suggest(self, word):
        return ["kauppa", "kaappi"]


def test_caption_panel_spell_spans_and_replacement(qtbot):
    panel = CaptionPanelWidget()
    qtbot.addWidget(panel)
    panel.spell_checker = _FakeChecker({"kaupppa"})

    seg = CaptionSegment(
        start_ms=0,
        end_ms=1000,
        text="Tämä on kaupppa",
        words=[
            WordSpan(0, 300, "Tämä"),
            WordSpan(300, 600, "on"),
            WordSpan(600, 1000, "kaupppa"),
        ],
    )
    panel.set_segments([seg])

    spans = panel.spell_spans_for_row(0)
    assert spans == [(8, 15, "kaupppa")]

    emitted = []
    panel.segments_updated.connect(emitted.append)
    panel._replace_word(0, 8, 15, "kauppa")

    assert panel.segments[0].text == "Tämä on kauppa"
    assert len(emitted) == 1
    # The corrected row is re-checked, not served from the stale cache.
    assert panel.spell_spans_for_row(0) == []


def test_caption_panel_spell_check_disabled_without_voikko(qtbot):
    panel = CaptionPanelWidget()
    qtbot.addWidget(panel)

    class _Unavailable:
        available = False

    panel.spell_checker = _Unavailable()
    seg = CaptionSegment(
        start_ms=0, end_ms=1000, text="kaupppa", words=[WordSpan(0, 1000, "kaupppa")]
    )
    panel.set_segments([seg])

    assert panel.spell_spans_for_row(0) == []


def test_caption_panel_edit_timecode_cell(qtbot):
    panel = CaptionPanelWidget()
    qtbot.addWidget(panel)

    seg1 = CaptionSegment(
        start_ms=0, end_ms=1000, text="First", words=[WordSpan(0, 1000, "First")]
    )
    seg2 = CaptionSegment(
        start_ms=2000,
        end_ms=3000,
        text="Second",
        words=[WordSpan(2000, 3000, "Second")],
    )
    panel.set_segments([seg1, seg2])

    emitted = []
    panel.segments_updated.connect(emitted.append)

    # Retype the second cue's start as mm:ss.t
    panel.cue_table.item(1, 0).setText("00:01.5")
    assert panel.segments[1].start_ms == 1500
    assert panel.segments[1].words[0].start_ms == 1500
    assert len(emitted) == 1

    # Gibberish is rejected and the cell snaps back to the cue's own time.
    panel.cue_table.item(1, 0).setText("not a time")
    assert panel.segments[1].start_ms == 1500
    assert panel.cue_table.item(1, 0).text() == "00:01.5"

    # A start dragged before the previous cue's end is clamped to it.
    panel.cue_table.item(1, 0).setText("00:00.2")
    assert panel.segments[1].start_ms == 1000


def test_caption_panel_nudge_selected_cue(qtbot):
    panel = CaptionPanelWidget()
    qtbot.addWidget(panel)

    seg = CaptionSegment(
        start_ms=1000,
        end_ms=2000,
        text="Nudge me",
        words=[WordSpan(1000, 1500, "Nudge"), WordSpan(1500, 2000, "me")],
    )
    panel.set_segments([seg])
    panel.cue_table.selectRow(0)

    panel.nudge_selected_cue(100, "end")
    assert panel.segments[0].end_ms == 2100
    assert panel.segments[0].words[-1].end_ms == 2100

    panel.nudge_selected_cue(-100, "start")
    assert panel.segments[0].start_ms == 900

    panel.nudge_selected_cue(100, "both")
    assert (panel.segments[0].start_ms, panel.segments[0].end_ms) == (1000, 2200)


def test_caption_panel_delete_empty_slot_stretches_previous_cue(qtbot):
    panel = CaptionPanelWidget()
    qtbot.addWidget(panel)

    seg1 = CaptionSegment(
        start_ms=0, end_ms=1000, text="First", words=[WordSpan(0, 1000, "First")]
    )
    seg2 = CaptionSegment(
        start_ms=1000,
        end_ms=1800,
        text="Second",
        words=[WordSpan(1000, 1800, "Second")],
    )
    panel.set_segments([seg1, seg2])

    panel.cue_table.selectRow(1)
    panel.btn_shift_prev.click()  # empties the second cue
    panel.btn_delete_empty.click()

    assert len(panel.segments) == 1
    # The freed time goes to the cue that absorbed the words.
    assert panel.segments[0].end_ms == 1800
