import json

import pytest
from PySide6.QtCore import QBuffer, QByteArray, QIODevice, QItemSelectionModel, Qt
from PySide6.QtGui import QColor, QImage

from app.services.broll import picks as picks_mod
from app.services.broll import pipeline
from app.services.broll.assets import Asset
from app.views.broll_dialog import BrollDialog


def png() -> bytes:
    img = QImage(8, 8, QImage.Format.Format_RGB32)
    img.fill(QColor("red"))
    data = QByteArray()
    buf = QBuffer(data)
    buf.open(QIODevice.OpenModeFlag.WriteOnly)
    img.save(buf, "PNG")
    return bytes(data)


def asset(title, kind="image"):
    return Asset(
        title=title,
        kind=kind,
        url=f"https://x/{title}",
        page_url=f"https://p/{title}",
        license="CC0",
        thumb_url=f"https://t/{title}",
        source="Wikimedia Commons",
    )


def proposals():
    return [
        {
            "startMs": 0,
            "endMs": 4000,
            "text": "tram",
            "query": "tram",
            "candidates": [
                asset("A.jpg").to_dict(),
                asset("B.jpg").to_dict(),
                asset("C.jpg").to_dict(),
            ],
            "chosen": None,
        },
        {
            "startMs": 4000,
            "endMs": 9000,
            "text": "none",
            "query": "none",
            "candidates": [],
            "chosen": None,
        },
    ]


@pytest.fixture
def dialog(qtbot):
    d = BrollDialog(picks_mod.Picks(proposals()), load_thumb=lambda url: png())
    qtbot.addWidget(d)
    return d


def test_lists_every_window_and_its_candidates(dialog):
    assert dialog.windows.count() == 2
    dialog.windows.setCurrentRow(0)
    assert dialog.candidates.count() == 3
    dialog.windows.setCurrentRow(1)
    assert dialog.candidates.count() == 0


def test_accepting_needs_a_selected_candidate(dialog):
    dialog.windows.setCurrentRow(0)
    assert not dialog.accept_btn.isEnabled()
    dialog.candidates.setCurrentRow(1)
    assert dialog.accept_btn.isEnabled()
    dialog.accept_btn.click()
    assert dialog.picks.status[0] == "accepted"
    assert dialog.picks.accepted()[0].assets[0].title == "B.jpg"


def test_reject_marks_the_window_and_moves_on(dialog):
    dialog.windows.setCurrentRow(0)
    dialog.reject_btn.click()
    assert dialog.picks.status == ["rejected", "pending"]
    assert dialog.windows.currentRow() == 1  # on to the next one that waits


def test_the_build_button_waits_for_an_accepted_window(dialog):
    assert not dialog.build_btn.isEnabled()
    dialog.windows.setCurrentRow(0)
    dialog.candidates.setCurrentRow(0)
    dialog.accept_btn.click()
    assert dialog.build_btn.isEnabled()


def test_thumbnails_arrive_in_the_background(dialog, qtbot):
    dialog.windows.setCurrentRow(0)
    qtbot.waitUntil(lambda: not dialog.candidates.item(0).icon().isNull(), timeout=3000)


def test_a_failing_thumbnail_leaves_the_card_without_a_picture(qtbot):
    def boom(url):
        raise OSError("offline")

    d = BrollDialog(picks_mod.Picks(proposals()), load_thumb=boom)
    qtbot.addWidget(d)
    d.windows.setCurrentRow(0)
    assert d.candidates.count() == 3
    assert d.candidates.item(0).icon().isNull()


def test_window_rows_show_the_status(dialog):
    dialog.windows.setCurrentRow(0)
    dialog.candidates.setCurrentRow(0)
    dialog.accept_btn.click()
    assert "✓" in dialog.windows.item(0).text()
    assert dialog.windows.item(0).data(Qt.ItemDataRole.UserRole) == 0


def select(dialog, *rows):
    dialog.candidates.clearSelection()
    for r in rows:
        dialog.candidates.item(r).setSelected(True)
    dialog.candidates.setCurrentRow(rows[0], QItemSelectionModel.SelectionFlag.NoUpdate)
    dialog._sync()


def test_two_selected_candidates_are_accepted_as_a_stack(dialog):
    dialog.windows.setCurrentRow(0)
    select(dialog, 0, 2)
    assert dialog.accept_btn.isEnabled()
    dialog.accept_btn.click()
    (choice,) = dialog.picks.accepted()
    assert [a.title for a in choice.assets] == ["A.jpg", "C.jpg"]
    assert choice.layout == "stack"


def test_three_selected_candidates_cannot_be_accepted(dialog):
    dialog.windows.setCurrentRow(0)
    select(dialog, 0, 1, 2)
    assert not dialog.accept_btn.isEnabled()


def test_a_layout_is_chosen_for_a_single_picture_and_not_for_a_stack(dialog):
    dialog.windows.setCurrentRow(0)
    select(dialog, 1)
    assert dialog.layout_combo.isEnabled()
    dialog.layout_combo.setCurrentIndex(dialog.layout_combo.findData("blur"))
    dialog.accept_btn.click()
    assert dialog.picks.accepted()[0].layout == "blur"
    dialog.windows.setCurrentRow(0)
    select(dialog, 0, 1)
    assert not dialog.layout_combo.isEnabled()


def test_auto_layout_is_left_to_the_picture(dialog):
    dialog.windows.setCurrentRow(0)
    select(dialog, 1)
    dialog.accept_btn.click()
    assert dialog.picks.accepted()[0].layout is None


def test_coming_back_to_an_accepted_window_shows_what_was_picked(dialog):
    dialog.windows.setCurrentRow(0)
    select(dialog, 0, 2)
    dialog.accept_btn.click()
    dialog.windows.setCurrentRow(1)
    dialog.windows.setCurrentRow(0)
    chosen = sorted(i.row() for i in dialog.candidates.selectedIndexes())
    assert chosen == [0, 2]


def test_the_transition_style_is_chosen_for_the_whole_reel(dialog):
    assert dialog.transition == "mixed"
    dialog.transition_combo.setCurrentIndex(dialog.transition_combo.findData("whip"))
    assert dialog.transition == "whip"
    assert {
        dialog.transition_combo.itemData(i)
        for i in range(dialog.transition_combo.count())
    } == {
        "mixed",
        "fade",
        "whip",
        "slide",
        "cut",
    }


# --- the pipeline behind the menu item ---------------------------------------


def test_build_downloads_each_accepted_asset_once_and_renders(tmp_path):
    p = picks_mod.Picks(proposals())
    p.accept(0, 0)
    fetched, rendered = [], {}

    def fetch(a, folder):
        fetched.append(a.title)
        folder.mkdir(parents=True, exist_ok=True)
        path = folder / a.title
        path.write_bytes(b"x")
        return path

    def render(audio, shots, out, work, **kw):
        rendered.update(audio=audio, shots=shots, out=out, kw=kw)
        out.write_bytes(b"mp4")
        return out

    sidecar = tmp_path / "ep.m4a.capslap.json"
    sidecar.write_text(json.dumps({"segments": []}))
    out = tmp_path / "ep.reel.mp4"
    pipeline.build(
        tmp_path / "ep.m4a", p, out, fetch=fetch, render=render, sidecar=sidecar
    )
    assert fetched == ["A.jpg"]
    assert rendered["shots"][0].credit.startswith("A.jpg")
    # the captions follow the reel, so the app can caption it as it is
    assert (tmp_path / "ep.reel.mp4.capslap.json").read_text() == sidecar.read_text()
    # credits text for the episode description sits beside it
    txt = (tmp_path / "ep.reel.credits.txt").read_text()
    assert txt.splitlines()[0] == "Images and footage" and "https://p/A.jpg" in txt


def test_build_with_nothing_accepted_is_an_error(tmp_path):
    with pytest.raises(pipeline.NothingAccepted):
        pipeline.build(
            tmp_path / "a.m4a",
            picks_mod.Picks(proposals()),
            tmp_path / "o.mp4",
            fetch=lambda a, f: f,
            render=lambda *a, **k: None,
        )
