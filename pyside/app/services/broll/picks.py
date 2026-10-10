"""A person's decisions over the proposals: accept one candidate (or two, to
stack), or reject the window."""

from dataclasses import dataclass
from typing import Any

from app.services.broll.assets import Asset


@dataclass(frozen=True)
class Choice:
    window: dict[str, Any]
    assets: tuple[Asset, ...]  # one, or two for a stack
    layout: str | None  # "fill", "blur" or "stack"; None leaves it to the picture


class Picks:
    def __init__(self, proposals: list[dict[str, Any]]):
        self._p = proposals
        self.status: list[str] = [
            w.get("status") or ("pending" if w.get("chosen") is None else "accepted")
            for w in proposals
        ]

    def accept(
        self,
        window: int,
        candidate: int,
        also: int | None = None,
        layout: str | None = None,
    ) -> None:
        cands = self._p[window]["candidates"]
        for index in (candidate, also):
            if index is not None and not 0 <= index < len(cands):
                raise IndexError(f"window {window} has no candidate {index}")
        if also == candidate:
            raise ValueError("a stack needs two different candidates")
        w = self._p[window]
        w["chosen"] = candidate
        w["also"] = also
        w["layout"] = "stack" if also is not None else layout
        self.status[window] = "accepted"

    def reject(self, window: int) -> None:
        w = self._p[window]
        w["chosen"] = None
        w["also"] = None
        w["layout"] = None
        self.status[window] = "rejected"

    def pending(self) -> list[int]:
        return [i for i, s in enumerate(self.status) if s == "pending"]

    def accepted(self) -> list[Choice]:
        out = []
        for w, s in zip(self._p, self.status, strict=True):
            if s != "accepted":
                continue
            picked = [w["chosen"]] + ([w["also"]] if w.get("also") is not None else [])
            out.append(
                Choice(
                    w,
                    tuple(Asset.from_dict(w["candidates"][i]) for i in picked),
                    w.get("layout"),
                )
            )
        return out

    def data(self) -> list[dict[str, Any]]:
        return [{**w, "status": s} for w, s in zip(self._p, self.status, strict=True)]
