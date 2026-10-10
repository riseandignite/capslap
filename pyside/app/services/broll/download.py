import hashlib
from pathlib import Path

from app.services.broll import net, sources
from app.services.broll.assets import Asset


def fetch(a: Asset, folder: Path) -> Path:
    """Fetch an asset into `folder`, once; the name comes from its URL."""
    a = sources.resolve(a)  # NASA and the Archive only name the file when asked
    folder.mkdir(parents=True, exist_ok=True)
    ext = Path(a.url.split("?")[0]).suffix or ".bin"
    dest = folder / f"{hashlib.sha1(a.url.encode()).hexdigest()[:16]}{ext}"
    if not dest.exists():
        tmp = dest.with_suffix(dest.suffix + ".part")
        tmp.write_bytes(net.get(a.url))
        tmp.replace(dest)
    return dest
