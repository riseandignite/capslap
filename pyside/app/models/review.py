"""Client review: sending captions out and taking the corrections back in.

The review page (review/ at the repo root) opens a sidecar, lets a client fix the
text and times and sends back the same file with a `review` section. The file
stays a valid sidecar; importing it here applies only the captions, cue by cue,
so the local style, positions and word timings stay authoritative.
"""

import base64
import json
import os
import secrets
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from cryptography.exceptions import InvalidTag
from cryptography.hazmat.primitives import hashes
from cryptography.hazmat.primitives.ciphers.aead import AESGCM
from cryptography.hazmat.primitives.kdf.pbkdf2 import PBKDF2HMAC

from app.models.captions import (
    CaptionsFile,
    CaptionSegment,
    ProjectState,
    set_segment_time,
)

REVIEW_FORMAT = "capslap-review"

# Password-locked files: the whole captions file, AES-256-GCM with a key from
# PBKDF2-SHA256. The review page reads and writes the same (review/lock.js).
LOCKED_FORMAT = "capslap-locked"
LOCK_ITERATIONS = 600_000
# No look-alikes (0/o, 1/l/i), so a password read out over the phone survives.
_PASSWORD_ALPHABET = "abcdefghjkmnpqrstuvwxyz23456789"


class WrongPassword(ValueError):
    """The password doesn't open the locked file."""

    def __init__(self) -> None:
        super().__init__("Wrong password.")


def new_review_password() -> str:
    """Something like `k7mq-x2fp-9tza-hw4c`: easy to send, about 79 bits."""
    groups = (
        "".join(secrets.choice(_PASSWORD_ALPHABET) for _ in range(4)) for _ in range(4)
    )
    return "-".join(groups)


def is_locked(data: Any) -> bool:
    return isinstance(data, dict) and data.get("format") == LOCKED_FORMAT


def _key(password: str, salt: bytes, iterations: int) -> bytes:
    kdf = PBKDF2HMAC(
        algorithm=hashes.SHA256(), length=32, salt=salt, iterations=iterations
    )
    return kdf.derive(password.encode("utf-8"))


def lock_file(
    data: dict[str, Any], password: str, iterations: int = LOCK_ITERATIONS
) -> dict[str, Any]:
    """`data` locked with `password`, as a file the review page can open."""
    salt, iv = os.urandom(16), os.urandom(12)
    plain = json.dumps(data, ensure_ascii=False).encode("utf-8")
    sealed = AESGCM(_key(password, salt, iterations)).encrypt(iv, plain, None)

    def b64(raw: bytes) -> str:
        return base64.b64encode(raw).decode("ascii")

    return {
        "format": LOCKED_FORMAT,
        "formatVersion": 1,
        "kdf": "PBKDF2-SHA256",
        "iterations": iterations,
        "salt": b64(salt),
        "iv": b64(iv),
        "data": b64(sealed),
    }


def unlock_file(locked: dict[str, Any], password: str) -> dict[str, Any]:
    """The captions file inside `locked`. Raises WrongPassword."""
    try:
        key = _key(
            password, base64.b64decode(locked["salt"]), int(locked["iterations"])
        )
        plain = AESGCM(key).decrypt(
            base64.b64decode(locked["iv"]), base64.b64decode(locked["data"]), None
        )
    except (KeyError, TypeError, ValueError) as err:
        raise ValueError(f"damaged locked file ({err})") from err
    except InvalidTag as err:
        raise WrongPassword() from err
    data = json.loads(plain.decode("utf-8"))
    if not isinstance(data, dict):
        raise ValueError("not a captions file")
    return data


def review_export_dict(project: ProjectState) -> dict[str, Any]:
    """The file to send to a client: the sidecar plus which video it belongs to."""
    data = CaptionsFile(
        segments=project.segments,
        position_overrides=project.position_overrides,
        style=project.style,
    ).to_dict()
    if project.video:
        data["video"] = {
            "name": Path(project.video.path).name,
            "durationMs": int(round(project.video.duration_sec * 1000)),
        }
    return data


@dataclass
class ReviewResult:
    status: str = ""
    reviewer: str = ""
    text_changes: int = 0
    time_changes: int = 0
    # (caption number from 1, the caption's text, the comment)
    comments: list[tuple[int, str, str]] = field(default_factory=list)

    @property
    def changed(self) -> bool:
        return bool(self.text_changes or self.time_changes)

    def summary(self) -> str:
        who = f" from {self.reviewer}" if self.reviewer else ""
        if not self.changed and not self.comments:
            return f"Review{who}: approved, no changes."
        parts = []
        if self.text_changes:
            parts.append(f"{self.text_changes} text change(s)")
        if self.time_changes:
            parts.append(f"{self.time_changes} timing change(s)")
        if self.comments:
            parts.append(f"{len(self.comments)} comment(s)")
        return f"Review{who}: " + ", ".join(parts) + "."


class ReviewMismatch(ValueError):
    """The reviewed file doesn't match the captions it would replace."""


def apply_review(segments: list[CaptionSegment], data: dict[str, Any]) -> ReviewResult:
    """Apply a reviewed file's captions to `segments`, in place.

    The review page edits captions but never adds or removes one, so the files
    line up cue by cue; a different count means it was made from other captions
    and nothing is changed.
    """
    reviewed = CaptionsFile.from_dict(data).segments
    if len(reviewed) != len(segments):
        raise ReviewMismatch(
            f"The reviewed file has {len(reviewed)} captions, the project "
            f"{len(segments)}. It was made from different captions."
        )
    review = data.get("review") or {}
    result = ReviewResult(
        status=str(review.get("status", "")),
        reviewer=str(review.get("reviewer", "")).strip(),
    )
    for seg, new in zip(segments, reviewed):
        if (new.start_ms, new.end_ms) != (seg.start_ms, seg.end_ms):
            set_segment_time(seg, new.start_ms, new.end_ms)
            result.time_changes += 1
        if new.text != seg.text:
            # Rebuilds the word timings from ours, not the page's approximation.
            seg.update_text(new.text)
            result.text_changes += 1
    for comment in review.get("comments") or []:
        index = int(comment.get("index", -1))
        if 0 <= index < len(segments):
            text = str(comment.get("text", "")).strip()
            if text:
                result.comments.append((index + 1, segments[index].text, text))
    return result
