from dataclasses import dataclass, field
from typing import Any


@dataclass
class VideoMetadata:
    path: str
    width: int = 0
    height: int = 0
    duration_sec: float = 0.0
    fps: float = 0.0
    codec: str = ""
    is_hdr: bool = False

    @property
    def aspect_ratio_str(self) -> str:
        if self.width <= 0 or self.height <= 0:
            return "Unknown"
        ratio = self.width / self.height
        if abs(ratio - 16 / 9) < 0.05:
            return "16:9"
        elif abs(ratio - 9 / 16) < 0.05:
            return "9:16"
        elif abs(ratio - 1.0) < 0.05:
            return "1:1"
        elif abs(ratio - 4 / 5) < 0.05:
            return "4:5"
        return f"{self.width}:{self.height}"


@dataclass
class ProjectState:
    video: VideoMetadata | None = None
    sidecar_path: str | None = None
    segments: list[dict[str, Any]] = field(default_factory=list)
    position_overrides: list[dict[str, Any]] = field(default_factory=list)
    is_dirty: bool = False

    def load_video(self, path: str, probe_dict: dict[str, Any]) -> None:
        self.video = VideoMetadata(
            path=path,
            width=probe_dict.get("width") or 0,
            height=probe_dict.get("height") or 0,
            duration_sec=probe_dict.get("duration") or 0.0,
            fps=probe_dict.get("fps") or 0.0,
            codec=probe_dict.get("codec") or "",
            is_hdr=probe_dict.get("isHdr") or False,
        )
        self.sidecar_path = f"{path}.capslap.json"
        self.is_dirty = False
