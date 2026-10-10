"""Where to publish a render on the review portal: series, episode, label, file."""

from pathlib import Path

from PySide6.QtCore import QSettings
from PySide6.QtWidgets import (
    QCheckBox,
    QComboBox,
    QDialog,
    QDialogButtonBox,
    QFileDialog,
    QFormLayout,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QPushButton,
    QWidget,
)

from app.portal_client import Portal, PortalError

URL_KEY = "portal/url"
TOKEN_KEY = "portal/token"
SERIES_KEY = "portal/last_series"


def portal_from_settings() -> Portal | None:
    settings = QSettings()
    url = str(settings.value(URL_KEY, "", type=str) or "")
    token = str(settings.value(TOKEN_KEY, "", type=str) or "")
    return Portal(url, token) if url and token else None


class PublishDialog(QDialog):
    def __init__(
        self,
        video_path: str,
        label: str,
        episode: str,
        parent: QWidget | None = None,
    ):
        super().__init__(parent)
        self.setWindowTitle("Publish to Portal")
        self.setMinimumWidth(520)
        settings = QSettings()
        self._series: list[dict] = []

        self.url = QLineEdit(str(settings.value(URL_KEY, "", type=str) or ""))
        self.url.setPlaceholderText("https://review.example.com")
        self.token = QLineEdit(str(settings.value(TOKEN_KEY, "", type=str) or ""))
        self.token.setEchoMode(QLineEdit.EchoMode.Password)
        self.token.setPlaceholderText("PORTAL_ADMIN_TOKEN")
        connect = QPushButton("Connect")
        connect.clicked.connect(self.load_series)
        server = QHBoxLayout()
        server.addWidget(self.token, 1)
        server.addWidget(connect)

        self.series = QComboBox()
        self.series.setEditable(True)
        self.series.setEditText(str(settings.value(SERIES_KEY, "", type=str) or ""))
        self.series.currentTextChanged.connect(self._fill_episodes)
        self.episode = QComboBox()
        self.episode.setEditable(True)
        self.episode.setEditText(episode)
        self.label = QLineEdit(label)

        self.video = QLineEdit(video_path)
        browse = QPushButton("Choose…")
        browse.clicked.connect(self._choose_video)
        video_row = QHBoxLayout()
        video_row.addWidget(self.video, 1)
        video_row.addWidget(browse)

        self.with_captions = QCheckBox("Send the captions too, for the client to check")
        self.with_captions.setChecked(True)
        self.status = QLabel()
        self.status.setWordWrap(True)

        form = QFormLayout(self)
        form.addRow("Portal:", self.url)
        form.addRow("Admin token:", server)
        form.addRow("Series:", self.series)
        form.addRow("Episode:", self.episode)
        form.addRow("Label:", self.label)
        form.addRow("Video:", video_row)
        form.addRow("", self.with_captions)
        form.addRow(self.status)
        buttons = QDialogButtonBox(
            QDialogButtonBox.StandardButton.Ok | QDialogButtonBox.StandardButton.Cancel
        )
        buttons.button(QDialogButtonBox.StandardButton.Ok).setText("Publish")
        buttons.accepted.connect(self._accept)
        buttons.rejected.connect(self.reject)
        form.addRow(buttons)

        if self.url.text() and self.token.text():
            self.load_series()

    def portal(self) -> Portal:
        return Portal(self.url.text().strip(), self.token.text().strip(), timeout=10)

    def load_series(self) -> None:
        typed_series, typed_episode = (
            self.series.currentText(),
            self.episode.currentText(),
        )
        try:
            self._series = self.portal().series()
        except PortalError as err:
            self.status.setText(str(err))
            return
        self.series.blockSignals(True)
        self.series.clear()
        self.series.addItems([s["title"] for s in self._series])
        self.series.setEditText(typed_series)
        self.series.blockSignals(False)
        self._fill_episodes(typed_series)
        self.episode.setEditText(typed_episode)
        self.status.setText(
            f"Connected: {len(self._series)} series. Pick one or type a new name."
        )

    def _fill_episodes(self, series_title: str) -> None:
        typed = self.episode.currentText()
        self.episode.clear()
        for s in self._series:
            if s["title"].casefold() == series_title.strip().casefold():
                self.episode.addItems([e["title"] for e in s["episodes"]])
        self.episode.setEditText(typed)

    def _choose_video(self) -> None:
        path, _ = QFileDialog.getOpenFileName(
            self, "Video to Publish", self.video.text(), "Video (*.mp4)"
        )
        if path:
            self.video.setText(path)

    def values(self) -> dict:
        return {
            "series": self.series.currentText().strip(),
            "episode": self.episode.currentText().strip(),
            "label": self.label.text().strip(),
            "video": self.video.text().strip(),
            "captions": self.with_captions.isChecked(),
        }

    def _accept(self) -> None:
        v = self.values()
        missing = [name for name in ("series", "episode", "label") if not v[name]]
        if not self.url.text().strip() or not self.token.text().strip():
            missing.insert(0, "portal and admin token")
        if missing:
            self.status.setText("Fill in: " + ", ".join(missing) + ".")
            return
        if not Path(v["video"]).is_file():
            self.status.setText("Choose the video file to publish.")
            return
        settings = QSettings()
        settings.setValue(URL_KEY, self.url.text().strip())
        settings.setValue(TOKEN_KEY, self.token.text().strip())
        settings.setValue(SERIES_KEY, v["series"])
        self.accept()
