"""NASA Image and Video Library: public domain pictures and footage."""

from collections.abc import Callable
from dataclasses import replace
from typing import Any

from app.services.broll import net
from app.services.broll.assets import Asset

API = "https://images-api.nasa.gov/search"
SOURCE = "NASA Image and Video Library"
# Which file to take, best first: big enough for a phone frame, not the
# multi-megabyte originals.
_IMAGE = ("~large.jpg", "~medium.jpg", "~orig.jpg", "~small.jpg")
_VIDEO = ("~medium.mp4", "~mobile.mp4", "~large.mp4", "~orig.mp4")

Fetch = Callable[..., Any]


def search(query: str, limit: int = 20, fetch: Fetch = net.get_json) -> list[Asset]:
    params = {"q": query, "media_type": "image,video", "page_size": str(limit)}
    found: list[Asset] = []
    for item in fetch(API, params).get("collection", {}).get("items", []):
        data = (item.get("data") or [{}])[0]
        kind = data.get("media_type")
        if kind not in ("image", "video") or not item.get("href"):
            continue
        thumbs = [
            link["href"]
            for link in item.get("links", [])
            if link.get("render") == "image"
        ]
        ident = data.get("nasa_id", "")
        found.append(
            Asset(
                title=str(data.get("title") or ident),
                kind=kind,
                url=item["href"],
                page_url=f"https://images.nasa.gov/details/{ident}",
                author=data.get("photographer")
                or data.get("secondary_creator")
                or data.get("center")
                or "NASA",
                license="Public domain",
                source=SOURCE,
                thumb_url=thumbs[0] if thumbs else None,
                resolver="nasa",
            )
        )
    return found


def resolve(asset: Asset, fetch: Fetch = net.get_json) -> Asset:
    """The collection lists every file of the item; take a good one."""
    urls = [str(u) for u in fetch(asset.url)]
    for suffix in _IMAGE if asset.kind == "image" else _VIDEO:
        for u in urls:
            if u.lower().endswith(suffix):
                return replace(
                    asset, url=u.replace("http://", "https://"), resolver=None
                )
    raise ValueError(f"{asset.title}: no usable file in NASA's listing")
