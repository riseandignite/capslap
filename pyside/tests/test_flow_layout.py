import pytest
from PySide6.QtWidgets import QPushButton, QWidget

from app.views.flow_layout import FlowLayout
from app.views.main_window import MainWindow


def _buttons(layout, n=6, width=80):
    host = QWidget()
    host.setLayout(layout)
    out = []
    for i in range(n):
        b = QPushButton(f"Button {i}")
        b.setFixedWidth(width)
        layout.addWidget(b)
        out.append(b)
    return host, out


def test_everything_fits_on_one_row_when_there_is_room(qtbot):
    layout = FlowLayout(spacing=6)
    host, buttons = _buttons(layout)
    qtbot.addWidget(host)
    host.resize(600, 100)
    host.show()
    assert len({b.y() for b in buttons}) == 1
    # One row is as tall as the items the layout itself measured. The size a
    # style gives a button differs between the layout's reading and the
    # widget's own on macOS, so the widget's number is not the reference.
    assert layout.heightForWidth(600) == layout.itemAt(0).sizeHint().height()


def test_buttons_wrap_to_a_new_row_instead_of_shrinking(qtbot):
    layout = FlowLayout(spacing=6)
    host, buttons = _buttons(layout)
    qtbot.addWidget(host)
    host.resize(270, 200)  # three 80 px buttons and their gaps
    host.show()
    assert len({b.y() for b in buttons}) == 2
    assert all(b.width() == 80 for b in buttons)
    assert layout.heightForWidth(270) > layout.heightForWidth(600)


def test_the_layout_reports_the_widest_item_as_its_minimum(qtbot):
    layout = FlowLayout(spacing=6)
    host, _ = _buttons(layout, n=3, width=120)
    qtbot.addWidget(host)
    assert layout.minimumSize().width() == 120


def test_items_can_be_taken_out_again(qtbot):
    layout = FlowLayout(spacing=6)
    host, buttons = _buttons(layout, n=2)
    qtbot.addWidget(host)
    item = layout.takeAt(0)
    assert item.widget() is buttons[0] and layout.count() == 1


@pytest.mark.parametrize("width", [1000, 1300])
def test_no_button_in_the_main_window_is_clipped(qtbot, width):
    window = MainWindow()
    qtbot.addWidget(window)
    window.resize(width, 700)
    window.show()
    qtbot.wait(50)
    layouts = [window.action_bar, window.caption_panel.header_layout]
    for layout in layouts:
        for i in range(layout.count()):
            w = layout.itemAt(i).widget()
            if w is None:
                continue
            # A deliberately fixed-width icon button is its own business.
            needs = min(w.sizeHint().width(), w.maximumWidth())
            assert w.width() >= needs, (
                f"{w.text() if hasattr(w, 'text') else w} is {w.width()} px wide "
                f"but needs {needs}"
            )
    window.close()
