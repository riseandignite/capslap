from dataclasses import asdict, dataclass
from typing import Any


@dataclass(frozen=True)
class Asset:
    """One piece of footage or one still, with what a credit needs."""

    title: str
    kind: str  # "image" | "video"
    url: str
    page_url: str
    author: str | None = None
    license: str = ""
    license_url: str | None = None
    source: str = ""
    width: int = 0
    height: int = 0
    thumb_url: str | None = None
    # Set while `url` only leads to the file (a metadata page), not to the file
    # itself: the name of the source module that finishes the job at download.
    resolver: str | None = None

    def to_dict(self) -> dict[str, Any]:
        return asdict(self)

    @classmethod
    def from_dict(cls, d: dict[str, Any]) -> "Asset":
        return cls(**{k: d[k] for k in cls.__dataclass_fields__ if k in d})
