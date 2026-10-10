"""Openverse: Creative Commons pictures from Flickr, Wikimedia and others."""

from collections.abc import Callable
from typing import Any

from app.services.broll import licenses, net
from app.services.broll.assets import Asset

API = "https://api.openverse.org/v1/images/"
# Openverse's own filters, which together keep CC0, public domain, CC BY and
# CC BY-SA: allowed to earn money from, and to crop and move.
LICENSE_TYPE = "commercial,modification"
_NAMES = {"by": "CC BY", "by-sa": "CC BY-SA", "cc0": "CC0", "pdm": "Public domain"}

Fetch = Callable[[str, dict[str, Any]], Any]


def _license(r: dict[str, Any]) -> str:
    code = str(r.get("license", "")).lower()
    base = _NAMES.get(code, f"CC {code.upper()}" if code else "")
    if code in ("cc0", "pdm") or not r.get("license_version"):
        return base
    return f"{base} {r['license_version']}"


def search(query: str, limit: int = 20, fetch: Fetch = net.get_json) -> list[Asset]:
    params = {"q": query, "page_size": str(limit), "license_type": LICENSE_TYPE}
    found: list[Asset] = []
    for r in fetch(API, params).get("results", []):
        name = _license(r)
        # The filter above already did this; trust it, but not blindly.
        if not r.get("url") or licenses.family(name) is None:
            continue
        provider = str(r.get("provider") or "").replace("_", " ").title()
        found.append(
            Asset(
                title=r.get("title") or "Untitled",
                kind="image",
                url=r["url"],
                page_url=r.get("foreign_landing_url") or r["url"],
                author=r.get("creator") or None,
                license=name,
                license_url=r.get("license_url"),
                source=f"{provider} via Openverse" if provider else "Openverse",
                width=int(r.get("width") or 0),
                height=int(r.get("height") or 0),
                thumb_url=r.get("thumbnail"),
            )
        )
    return found
