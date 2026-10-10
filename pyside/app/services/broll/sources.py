"""All the sources at once: search them side by side, merge, resolve at download."""

import logging
from collections.abc import Callable
from concurrent.futures import ThreadPoolExecutor

from app.services.broll import archive, commons, nasa, openverse
from app.services.broll.assets import Asset

log = logging.getLogger("capslap.broll")

# name -> search(query, limit); the order is the order in the picker
SEARCHES: dict[str, Callable[[str, int], list[Asset]]] = {
    "commons": commons.search,
    "openverse": openverse.search,
    "archive": archive.search,
    "nasa": nasa.search,
}
# Sources whose search results only point at the file.
RESOLVERS: dict[str, Callable[[Asset], Asset]] = {
    "archive": archive.resolve,
    "nasa": nasa.resolve,
}


def search(
    query: str, limit: int = 20, only: tuple[str, ...] | None = None
) -> list[Asset]:
    """Every source's candidates taken in turn, so each one shows in the picker."""
    names = [n for n in SEARCHES if only is None or n in only]
    if not names:
        return []
    per_source = max(6, limit // 2)

    def one(name: str) -> list[Asset]:
        try:
            return SEARCHES[name](query, per_source)
        except Exception as err:  # one source being down must not cost the rest
            log.warning("b-roll source %s failed for %r: %s", name, query, err)
            return []

    with ThreadPoolExecutor(max_workers=len(names)) as pool:
        lists = list(pool.map(one, names))

    merged: list[Asset] = []
    seen: set[str] = set()
    for rank in range(max((len(x) for x in lists), default=0)):
        for found in lists:
            if rank < len(found) and found[rank].url not in seen:
                seen.add(found[rank].url)
                merged.append(found[rank])
    return merged[:limit]


def resolve(asset: Asset) -> Asset:
    """The asset with a URL that leads to the file itself."""
    if not asset.resolver:
        return asset
    return RESOLVERS[asset.resolver](asset)
