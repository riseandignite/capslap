from app.models.captions import CaptionStyle
from app.services.preset_manager import PresetManager


def test_preset_manager_save_and_load(tmp_path):
    preset_file = tmp_path / "custom_presets.json"
    manager = PresetManager(storage_path=preset_file)

    assert manager.list_presets() == []

    style = CaptionStyle(
        font_name="Komika Axis",
        font_size=72,
        text_color="#00FFCC",
        highlight_color="#FF00FF",
        outline_color="#111111",
        karaoke=True,
        template_id="custom",
    )

    manager.save_preset("Herrasmieshakkerit", style)
    assert "Herrasmieshakkerit" in manager.list_presets()

    # Load back in a new manager instance
    manager2 = PresetManager(storage_path=preset_file)
    loaded_style = manager2.get_preset("Herrasmieshakkerit")
    assert loaded_style is not None
    assert loaded_style.font_name == "Komika Axis"
    assert loaded_style.font_size == 72
    assert loaded_style.text_color == "#00FFCC"
    assert loaded_style.highlight_color == "#FF00FF"
    assert loaded_style.karaoke is True


def test_preset_manager_delete_preset(tmp_path):
    preset_file = tmp_path / "custom_presets.json"
    manager = PresetManager(storage_path=preset_file)

    style = CaptionStyle(font_name="Roboto", font_size=50)
    manager.save_preset("Show A", style)
    assert len(manager.list_presets()) == 1

    deleted = manager.delete_preset("Show A")
    assert deleted is True
    assert manager.list_presets() == []
    assert manager.get_preset("Show A") is None


def test_preset_manager_default_style(tmp_path):
    preset_file = tmp_path / "custom_presets.json"
    manager = PresetManager(storage_path=preset_file)

    assert manager.get_default_style() is None

    style = CaptionStyle(font_name="THE BOLD FONT", font_size=80)
    manager.save_default_style(style)

    manager2 = PresetManager(storage_path=preset_file)
    default_style = manager2.get_default_style()
    assert default_style is not None
    assert default_style.font_name == "THE BOLD FONT"
    assert default_style.font_size == 80
