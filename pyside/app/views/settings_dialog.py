"""Transcription provider settings: local (offline) Whisper vs. OpenAI API.

Persisted with QSettings (org/app name set in app/main.py) so the choice
survives restarts. `get_transcription_params()` is the single place that
turns the persisted choice into the `model`/`apiKey` fragment the `transcribe`
RPC expects (see rust/src/types.rs::GenerateCaptionsParams and
rust/src/whisper.rs::transcribe_segments_with_temp, which treats
model == "whisper-1" as an explicit request for the OpenAI API and any other
model name as a local whisper.cpp model to run offline).
"""

from __future__ import annotations

from PySide6.QtCore import QSettings
from PySide6.QtWidgets import (
    QButtonGroup,
    QComboBox,
    QDialog,
    QDialogButtonBox,
    QFormLayout,
    QGroupBox,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QMessageBox,
    QPushButton,
    QRadioButton,
    QVBoxLayout,
    QWidget,
)

from app.core_client import CoreClient

LOCAL_MODELS = ["tiny", "base", "small", "medium", "large", "turbo"]
DEFAULT_LOCAL_MODEL = "tiny"

# ISO-639-1 codes Whisper's tokenizer recognizes, code -> display name.
# Both the whisper.cpp `-l` flag and the OpenAI API `language` form field
# accept these (see rust/src/whisper.rs).
WHISPER_LANGUAGES = {
    "en": "English",
    "zh": "Chinese",
    "de": "German",
    "es": "Spanish",
    "ru": "Russian",
    "ko": "Korean",
    "fr": "French",
    "ja": "Japanese",
    "pt": "Portuguese",
    "tr": "Turkish",
    "pl": "Polish",
    "ca": "Catalan",
    "nl": "Dutch",
    "ar": "Arabic",
    "sv": "Swedish",
    "it": "Italian",
    "id": "Indonesian",
    "hi": "Hindi",
    "fi": "Finnish",
    "vi": "Vietnamese",
    "he": "Hebrew",
    "uk": "Ukrainian",
    "el": "Greek",
    "ms": "Malay",
    "cs": "Czech",
    "ro": "Romanian",
    "da": "Danish",
    "hu": "Hungarian",
    "ta": "Tamil",
    "no": "Norwegian",
    "th": "Thai",
    "ur": "Urdu",
    "hr": "Croatian",
    "bg": "Bulgarian",
    "lt": "Lithuanian",
    "la": "Latin",
    "mi": "Maori",
    "ml": "Malayalam",
    "cy": "Welsh",
    "sk": "Slovak",
    "te": "Telugu",
    "fa": "Persian",
    "lv": "Latvian",
    "bn": "Bengali",
    "sr": "Serbian",
    "az": "Azerbaijani",
    "sl": "Slovenian",
    "kn": "Kannada",
    "et": "Estonian",
    "mk": "Macedonian",
    "eu": "Basque",
    "is": "Icelandic",
    "hy": "Armenian",
    "ne": "Nepali",
    "mn": "Mongolian",
    "bs": "Bosnian",
    "kk": "Kazakh",
    "sq": "Albanian",
    "sw": "Swahili",
    "gl": "Galician",
    "mr": "Marathi",
    "pa": "Punjabi",
    "si": "Sinhala",
    "km": "Khmer",
    "sn": "Shona",
    "yo": "Yoruba",
    "so": "Somali",
    "af": "Afrikaans",
    "ka": "Georgian",
    "be": "Belarusian",
    "gu": "Gujarati",
    "am": "Amharic",
    "lo": "Lao",
    "uz": "Uzbek",
    "ps": "Pashto",
    "mt": "Maltese",
    "tl": "Tagalog",
}
AUTO_DETECT = "auto"

_PROVIDER_KEY = "whisper/provider"
_MODEL_KEY = "whisper/model"
_API_KEY_KEY = "whisper/api_key"
_LANGUAGE_KEY = "whisper/language"


def get_transcription_params() -> dict:
    """Read the persisted provider choice as a params fragment to merge into
    the `transcribe` / `generateCaptions` RPC call (`model`, `apiKey`,
    `language`)."""
    settings = QSettings()
    language_code = settings.value(_LANGUAGE_KEY, AUTO_DETECT, type=str)
    language = (
        None if not language_code or language_code == AUTO_DETECT else language_code
    )

    provider = settings.value(_PROVIDER_KEY, "local")
    if provider == "openai":
        api_key = settings.value(_API_KEY_KEY, "", type=str)
        return {"model": "whisper-1", "apiKey": api_key or None, "language": language}
    model = settings.value(_MODEL_KEY, DEFAULT_LOCAL_MODEL, type=str)
    return {"model": model or DEFAULT_LOCAL_MODEL, "apiKey": None, "language": language}


class WhisperSettingsDialog(QDialog):
    """Lets the user pick local (offline) Whisper vs. the OpenAI API, choose
    a local model size, enter an OpenAI API key, and download local models."""

    def __init__(
        self, core_client: CoreClient | None = None, parent: QWidget | None = None
    ):
        super().__init__(parent)
        self.setWindowTitle("Transcription Settings")
        self.setMinimumWidth(440)
        self.core = core_client
        self.settings = QSettings()

        layout = QVBoxLayout(self)

        provider_box = QGroupBox("Transcription Provider")
        provider_layout = QVBoxLayout(provider_box)
        self.radio_local = QRadioButton(
            "Local Whisper (offline, runs on this Mac, free, private)"
        )
        self.radio_openai = QRadioButton(
            "OpenAI API (cloud, requires API key, usage costs money)"
        )
        self.provider_group = QButtonGroup(self)
        self.provider_group.addButton(self.radio_local)
        self.provider_group.addButton(self.radio_openai)
        provider_layout.addWidget(self.radio_local)
        provider_layout.addWidget(self.radio_openai)
        layout.addWidget(provider_box)

        form_box = QGroupBox("Options")
        form = QFormLayout(form_box)

        self.model_combo = QComboBox()
        self.model_combo.addItems(LOCAL_MODELS)
        form.addRow("Local model:", self.model_combo)

        self.language_combo = QComboBox()
        self.language_combo.addItem("Auto-detect", AUTO_DETECT)
        for code, name in sorted(WHISPER_LANGUAGES.items(), key=lambda kv: kv[1]):
            self.language_combo.addItem(name, code)
        self.language_combo.setToolTip(
            "Whisper's auto-detect guesses from the first ~30s of audio and can "
            "pick the wrong language. Force it here if you get captions in the "
            "wrong language."
        )
        form.addRow("Language:", self.language_combo)

        model_row = QHBoxLayout()
        self.model_status_lbl = QLabel("")
        self.model_status_lbl.setWordWrap(True)
        self.download_btn = QPushButton("Check / Download Model")
        self.download_btn.clicked.connect(self._on_download_clicked)
        model_row.addWidget(self.model_status_lbl, stretch=1)
        model_row.addWidget(self.download_btn)
        form.addRow("", model_row)

        self.api_key_edit = QLineEdit()
        self.api_key_edit.setEchoMode(QLineEdit.EchoMode.Password)
        self.api_key_edit.setPlaceholderText("sk-...")
        form.addRow("OpenAI API key:", self.api_key_edit)

        layout.addWidget(form_box)

        buttons = QDialogButtonBox(
            QDialogButtonBox.StandardButton.Ok | QDialogButtonBox.StandardButton.Cancel
        )
        buttons.accepted.connect(self._on_accept)
        buttons.rejected.connect(self.reject)
        layout.addWidget(buttons)

        self.radio_local.toggled.connect(self._update_enabled_state)
        self.radio_openai.toggled.connect(self._update_enabled_state)

        self._load_settings()
        self._update_enabled_state()

    def _load_settings(self) -> None:
        provider = self.settings.value(_PROVIDER_KEY, "local")
        if provider == "openai":
            self.radio_openai.setChecked(True)
        else:
            self.radio_local.setChecked(True)

        model = self.settings.value(_MODEL_KEY, DEFAULT_LOCAL_MODEL, type=str)
        idx = self.model_combo.findText(model)
        self.model_combo.setCurrentIndex(idx if idx >= 0 else 0)

        self.api_key_edit.setText(self.settings.value(_API_KEY_KEY, "", type=str))

        language_code = self.settings.value(_LANGUAGE_KEY, AUTO_DETECT, type=str)
        idx = self.language_combo.findData(language_code)
        self.language_combo.setCurrentIndex(idx if idx >= 0 else 0)

    def _update_enabled_state(self) -> None:
        is_local = self.radio_local.isChecked()
        self.model_combo.setEnabled(is_local)
        self.download_btn.setEnabled(is_local and self.core is not None)
        self.api_key_edit.setEnabled(not is_local)

    def _on_download_clicked(self) -> None:
        if not self.core:
            return
        model = self.model_combo.currentText()
        self.download_btn.setEnabled(False)
        self.model_status_lbl.setText(f"Checking {model} model...")

        check_fut = self.core.call("checkModelExists", model)

        def on_checked(f):
            try:
                exists = f.result()
            except (RuntimeError, ValueError, OSError) as err:
                self._set_status_async(f"Error: {err}")
                self._set_enabled_async(True)
                return
            if exists:
                self._set_status_async(f"'{model}' model is already downloaded.")
                self._set_enabled_async(True)
            else:
                self._set_status_async(f"Downloading '{model}' model...")
                dl_fut = self.core.call("downloadModel", {"model": model})
                dl_fut.add_done_callback(self._on_download_done)

        check_fut.add_done_callback(on_checked)

    def _on_download_done(self, f) -> None:
        try:
            f.result()
            model = self.model_combo.currentText()
            self._set_status_async(f"'{model}' model downloaded successfully.")
        except (RuntimeError, ValueError, OSError) as err:
            self._set_status_async(f"Download failed: {err}")
        self._set_enabled_async(True)

    def _set_status_async(self, text: str) -> None:
        from PySide6.QtCore import QTimer

        QTimer.singleShot(0, self, lambda: self.model_status_lbl.setText(text))

    def _set_enabled_async(self, enabled: bool) -> None:
        from PySide6.QtCore import QTimer

        QTimer.singleShot(0, self, lambda: self.download_btn.setEnabled(enabled))

    def _on_accept(self) -> None:
        if self.radio_openai.isChecked() and not self.api_key_edit.text().strip():
            QMessageBox.warning(
                self,
                "Transcription Settings",
                "Enter an OpenAI API key, or choose Local Whisper instead.",
            )
            return

        self.settings.setValue(
            _PROVIDER_KEY, "openai" if self.radio_openai.isChecked() else "local"
        )
        self.settings.setValue(_MODEL_KEY, self.model_combo.currentText())
        self.settings.setValue(_API_KEY_KEY, self.api_key_edit.text().strip())
        self.settings.setValue(_LANGUAGE_KEY, self.language_combo.currentData())
        self.accept()
