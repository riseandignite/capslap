"""Wikimedia Commons search (File: namespace) via the MediaWiki API."""

import html
import json
import re
from collections.abc import Callable
from typing import Any

from app.services.broll import licenses, net
from app.services.broll.assets import Asset

API = "https://commons.wikimedia.org/w/api.php"
SOURCE = "Wikimedia Commons"
# SVG is a diagram, not footage; the rest are what ffmpeg can scale to a frame.
_RASTER = {"image/jpeg", "image/png", "image/webp", "image/tiff"}
_VIDEO_PREFIX = "video/"

Fetch = Callable[[str, dict[str, str]], dict[str, Any]]


def _http(url: str, params: dict[str, str]) -> dict[str, Any]:
    return json.loads(net.get(url, params))


def _plain(markup: str) -> str:
    text = html.unescape(re.sub(r"<[^>]+>", "", markup)).strip()
    # Some uploads carry the same text twice with nothing between the copies.
    half, odd = divmod(len(text), 2)
    if not odd and half and text[:half] == text[half:]:
        return text[:half]
    return text


def _meta(info: dict[str, Any], key: str) -> str:
    return str(info.get("extmetadata", {}).get(key, {}).get("value", ""))


def search(query: str, limit: int = 20, fetch: Fetch = _http) -> list[Asset]:
    params = {
        "action": "query",
        "format": "json",
        "generator": "search",
        "gsrnamespace": "6",
        "gsrsearch": f"{query} filetype:bitmap|video",
        "gsrlimit": str(limit),
        "prop": "imageinfo",
        "iiprop": "url|mime|size|extmetadata",
        "iiurlwidth": "320",
    }
    pages = fetch(API, params).get("query", {}).get("pages", {})
    found: list[Asset] = []
    for page in pages.values():
        info = (page.get("imageinfo") or [{}])[0]
        mime = info.get("mime", "")
        is_video = mime.startswith(_VIDEO_PREFIX)
        if not is_video and mime not in _RASTER:
            continue
        lic = _meta(info, "LicenseShortName")
        if licenses.family(lic) is None:
            continue
        author = _plain(_meta(info, "Artist")) or None
        found.append(
            Asset(
                title=page.get("title", "").removeprefix("File:"),
                kind="video" if is_video else "image",
                url=info.get("url", ""),
                page_url=info.get("descriptionurl", ""),
                author=author,
                license=lic,
                license_url=_meta(info, "LicenseUrl") or None,
                source=SOURCE,
                thumb_url=info.get("thumburl") or None,
                width=int(info.get("width", 0)),
                height=int(info.get("height", 0)),
            )
        )
    return found
