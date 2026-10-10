"""The Internet Archive: video with a Creative Commons licence or marked public domain."""

import re
import urllib.parse
from collections.abc import Callable
from dataclasses import replace
from typing import Any

from app.services.broll import licenses, net
from app.services.broll.assets import Asset

SEARCH_API = "https://archive.org/advancedsearch.php"
SOURCE = "Internet Archive"
# The biggest file taken without trouble: the originals of long films run to
# gigabytes, and a reel uses a few seconds of one. A derived MP4 of a single
# film measured 127 MB at 320x240 (Prelinger), so the cap sits well under that.
MAX_BYTES = 80 * 1024 * 1024

# Only the licences a reel may use are asked for. Searching them all and
# dropping the rest afterwards leaves nothing when the most downloaded films of
# a subject happen to be NC or ND, as they often are. The slashes are escaped
# for the Archive's query parser.
LICENSED = (
    "(licenseurl:(*publicdomain*) "
    "OR licenseurl:(*creativecommons.org\\/licenses\\/by\\/*) "
    "OR licenseurl:(*creativecommons.org\\/licenses\\/by-sa\\/*))"
)

Fetch = Callable[..., Any]


def license_name(url: str) -> str:
    """The licence's usual name from its Creative Commons URL, "" if it is not one."""
    if "publicdomain/zero" in url:
        return "CC0"
    # "publicdomain/mark/1.0/", and the older "licenses/publicdomain/"
    if "publicdomain/mark" in url or "/licenses/publicdomain" in url:
        return "Public domain"
    m = re.search(r"/licenses/([a-z-]+)/(\d\.\d)", url)
    if not m:
        return ""
    return f"CC {m.group(1).upper()} {m.group(2)}"


def _words(query: str) -> str:
    return " ".join(re.findall(r"\w+", query))


def search(query: str, limit: int = 20, fetch: Fetch = net.get_json) -> list[Asset]:
    q = f"({_words(query)}) AND mediatype:(movies) AND {LICENSED}"
    params = {
        "q": q,
        "fl[]": ["identifier", "title", "creator", "licenseurl"],
        "sort[]": ["downloads desc"],
        "rows": str(limit),
        "page": "1",
        "output": "json",
    }
    found: list[Asset] = []
    for d in fetch(SEARCH_API, params).get("response", {}).get("docs", []):
        name = license_name(str(d.get("licenseurl", "")))
        if licenses.family(name) is None or not d.get("identifier"):
            continue
        ident = d["identifier"]
        creator = d.get("creator")
        if isinstance(creator, list):
            creator = ", ".join(creator)
        found.append(
            Asset(
                title=str(d.get("title") or ident),
                kind="video",
                url=f"https://archive.org/metadata/{ident}",
                page_url=f"https://archive.org/details/{ident}",
                author=creator or None,
                license=name,
                license_url=d.get("licenseurl"),
                source=SOURCE,
                thumb_url=f"https://archive.org/services/img/{ident}",
                resolver="archive",
            )
        )
    return found


def resolve(asset: Asset, fetch: Fetch = net.get_json) -> Asset:
    """Find the video file: the biggest MP4 that is not enormous."""
    ident = asset.url.rstrip("/").rsplit("/", 1)[-1]
    files = [
        f
        for f in fetch(asset.url).get("files", [])
        if str(f.get("name", "")).lower().endswith(".mp4")
    ]
    if not files:
        raise ValueError(f"{asset.title}: the Internet Archive has no MP4 of it")

    def size(f: dict[str, Any]) -> int:
        try:
            return int(f.get("size") or 0)
        except ValueError:
            return 0

    fitting = [f for f in files if size(f) <= MAX_BYTES]
    best = max(fitting, key=size) if fitting else min(files, key=size)
    name = urllib.parse.quote(best["name"], safe="")
    return replace(
        asset,
        url=f"https://archive.org/download/{ident}/{name}",
        width=int(best.get("width") or 0),
        height=int(best.get("height") or 0),
        resolver=None,
    )
