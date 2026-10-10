from app.models.captions import CaptionStyle, STYLE_PRESETS


def test_caption_style_defaults():
    style = CaptionStyle()
    assert style.template_id == "oneliner"
    assert "Montserrat" in style.font_name
    assert style.font_size == 65
    assert style.text_color == "#ffffff"
    assert style.highlight_color == "#ffff00"
    assert style.karaoke is True


def test_caption_style_presets():
    assert "oneliner" in STYLE_PRESETS
    assert "karaoke" in STYLE_PRESETS
    assert "vibrant" in STYLE_PRESETS
    assert "storyteller" in STYLE_PRESETS

    karaoke_style = STYLE_PRESETS["karaoke"]
    assert karaoke_style.karaoke is True
    assert "Komika" in karaoke_style.font_name


def test_caption_style_roundtrip():
    orig = CaptionStyle(
        template_id="custom",
        font_name="Bangers",
        font_size=72,
        text_color="#123456",
        highlight_color="#abcdef",
        outline_color="#000000",
        outline_width=4,
        karaoke=True,
    )
    d = orig.to_dict()
    restored = CaptionStyle.from_dict(d)
    assert restored.font_name == "Bangers"
    assert restored.font_size == 72
    assert restored.karaoke is True
