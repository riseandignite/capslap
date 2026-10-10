from app.services.broll.assets import Asset


def line(a: Asset) -> str:
    """The on-screen credit: title by author, licence, source."""
    head = f"{a.title} by {a.author}" if a.author else a.title
    return ", ".join(p for p in (head, a.license, a.source) if p)


def description(assets: list[Asset]) -> str:
    """Plain text for the episode description, where links can be copied."""
    out = ["Images and footage"]
    seen: set[str] = set()
    for a in assets:
        if a.page_url in seen:
            continue
        seen.add(a.page_url)
        out.append(f"{line(a)}: {a.page_url}")
    return "\n".join(out)
