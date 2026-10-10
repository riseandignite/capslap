import sys
from pathlib import Path
from typing import Any

from PySide6.QtGui import QFontDatabase

_LOADED = False
_AVAILABLE_FONTS: list[str] = []


def _fonts_dir() -> Path:
    # A PyInstaller-frozen build bundled rust/src/fonts at the same relative
    # path, next to sys._MEIPASS (onefile) or the executable (onedir).
    frozen_root = getattr(sys, "_MEIPASS", None)
    if frozen_root:
        return Path(frozen_root) / "rust" / "src" / "fonts"
    if getattr(sys, "frozen", False):
        return Path(sys.executable).resolve().parent / "rust" / "src" / "fonts"
    # Dev checkout: relative to pyside/app/fonts.py
    return Path(__file__).resolve().parents[2] / "rust" / "src" / "fonts"


def init_app_fonts() -> list[str]:
    """
    Load bundled fonts from rust/src/fonts into QFontDatabase.
    Returns prioritized list of available font family names.
    """
    global _LOADED, _AVAILABLE_FONTS
    if _LOADED:
        return _AVAILABLE_FONTS

    fonts_dir = _fonts_dir()
    found_families = set()
    if fonts_dir.exists():
        for font_path in sorted(fonts_dir.glob("*.ttf")):
            fid = QFontDatabase.addApplicationFont(str(font_path))
            if fid >= 0:
                for family in QFontDatabase.applicationFontFamilies(fid):
                    found_families.add(family)

    preferred_order = [
        "Montserrat",
        "Komika Axis",
        "Roboto",
        "Poppins",
        "Kanit",
        "THE BOLD FONT",
        "Bebas Neue",
        "Luckiest Guy",
        "Permanent Marker",
        "Bangers",
        "Fredoka",
        "Lilita One",
        "Lato",
        "Raleway",
        "Oswald",
        "Cinzel",
        "Caveat Brush",
        "Amatic SC",
        "Chewy",
        "Patrick Hand",
        "Playfair Display",
    ]
    ordered = [f for f in preferred_order if f in found_families]
    for f in sorted(found_families):
        if f not in ordered:
            ordered.append(f)

    _AVAILABLE_FONTS = ordered
    _LOADED = True
    return _AVAILABLE_FONTS


FONT_CATEGORIES: list[dict[str, Any]] = [
    {
        "name": "Modern / Sans",
        "fonts": [
            "Montserrat",
            "Roboto",
            "Open Sans",
            "Lato",
            "Raleway",
            "Kanit",
            "Poppins",
            "Work Sans",
        ],
    },
    {
        "name": "Display / Impact",
        "fonts": [
            "THE BOLD FONT",
            "Bebas Neue",
            "Anton",
            "Lilita One",
            "Oswald",
            "Bangers",
        ],
    },
    {
        "name": "Fun / Comic",
        "fonts": [
            "Komika Axis",
            "Comic Neue",
            "Fredoka",
            "Chewy",
            "Luckiest Guy",
        ],
    },
    {
        "name": "Serif / Elegant",
        "fonts": [
            "Playfair Display",
            "Merriweather",
            "Lora",
            "Cinzel",
            "Bodoni Moda",
        ],
    },
    {
        "name": "Handwritten / Script",
        "fonts": [
            "Permanent Marker",
            "Patrick Hand",
            "Amatic SC",
            "Caveat Brush",
            "Pacifico",
        ],
    },
]


def get_categorized_fonts() -> dict[str, list[str]]:
    """
    Return available fonts organized hierarchically by category.
    """
    avail = init_app_fonts()
    categorized: dict[str, list[str]] = {}
    assigned: set[str] = set()

    for cat in FONT_CATEGORIES:
        cat_name = cat["name"]
        cat_fonts = []
        for target in cat["fonts"]:
            # Find any available font matching target (case-insensitive)
            target_lower = target.lower().replace(" ", "")
            for f in avail:
                f_norm = f.lower().replace(" ", "")
                if target_lower in f_norm or f_norm in target_lower:
                    if f not in cat_fonts:
                        cat_fonts.append(f)
                        assigned.add(f)
        categorized[cat_name] = cat_fonts

    # Collect any unassigned available fonts into "Other"
    other_fonts = [f for f in avail if f not in assigned]
    if other_fonts:
        categorized["Other"] = other_fonts

    return categorized
