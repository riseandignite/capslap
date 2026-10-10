from app.fonts import get_categorized_fonts, init_app_fonts


def test_init_app_fonts(qapp):
    fonts = init_app_fonts()
    assert isinstance(fonts, list)
    assert len(fonts) > 0
    # Must contain key app fonts
    assert any("Montserrat" in f for f in fonts)
    assert any("Komika" in f for f in fonts)


def test_get_categorized_fonts(qapp):
    categories = get_categorized_fonts()
    assert isinstance(categories, dict)
    # Check expected categories
    assert "Modern / Sans" in categories
    assert "Display / Impact" in categories
    assert "Fun / Comic" in categories
    assert "Serif / Elegant" in categories
    assert "Handwritten / Script" in categories

    # Check key fonts exist in proper categories
    assert any("Montserrat" in f for f in categories["Modern / Sans"])
    assert any("Komika" in f for f in categories["Fun / Comic"])
    assert any("Permanent Marker" in f for f in categories["Handwritten / Script"])
