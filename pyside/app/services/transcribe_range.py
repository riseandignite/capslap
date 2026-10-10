"""Transcribing a part of a recording rather than all of it.

Whisper over a 47-minute episode keeps every core busy for a long time, so a
long recording is transcribed a part at a time: between the trim marks (I and O),
and the cues land among the ones already there.
"""

from dataclasses import dataclass, replace
from typing import Literal

from app.models.captions import CaptionSegment

# Longer than this and Transcribe asks for a range instead of starting on the
# whole file. Ten minutes of speech is a few minutes of whisper on a laptop.
LONG_RECORDING_MS = 10 * 60 * 1000
# The timeline's end mark sits at the exact length until someone moves it; a
# mark this close to the end has not been moved.
_UNMOVED_MS = 100


@dataclass(frozen=True)
class Plan:
    kind: Literal["whole", "range", "ask"]
    start_ms: int = 0
    end_ms: int = 0
    duration_ms: int = 0


def plan(duration_ms: int, trim_start_ms: int, trim_end_ms: int) -> Plan:
    if duration_ms <= 0:
        return Plan("whole")
    marked = trim_start_ms > _UNMOVED_MS or trim_end_ms < duration_ms - _UNMOVED_MS
    if marked:
        return Plan("range", trim_start_ms, trim_end_ms)
    if duration_ms > LONG_RECORDING_MS:
        return Plan("ask", duration_ms=duration_ms)
    return Plan("whole")


def shifted(segments: list[CaptionSegment], offset_ms: int) -> list[CaptionSegment]:
    """Copies of the cues moved `offset_ms` later, words included."""
    return [
        CaptionSegment(
            start_ms=s.start_ms + offset_ms,
            end_ms=s.end_ms + offset_ms,
            text=s.text,
            words=[
                replace(w, start_ms=w.start_ms + offset_ms, end_ms=w.end_ms + offset_ms)
                for w in s.words
            ],
        )
        for s in segments
    ]


def merged(
    existing: list[CaptionSegment],
    new: list[CaptionSegment],
    start_ms: int,
    end_ms: int,
) -> list[CaptionSegment]:
    """The cues outside [start, end) kept, the range's own replaced by `new`.

    A cue that begins before the range and runs into it is part of the range:
    it was transcribed with it and would double up with the new ones."""
    kept = [s for s in existing if s.end_ms <= start_ms or s.start_ms >= end_ms]
    return sorted(kept + new, key=lambda s: s.start_ms)


def clock(ms: int) -> str:
    s = ms // 1000
    h, m = divmod(s // 60, 60)
    return f"{h}:{m:02d}:{s % 60:02d}" if h else f"{m}:{s % 60:02d}"


def ask_text(duration_ms: int) -> str:
    return (
        f"This recording is {clock(duration_ms)} long, and transcribing all of it "
        "at once keeps the computer busy for a long time.\n\n"
        "Mark the part to transcribe first: play to where it starts and press I, "
        "then to where it ends and press O. Transcribe then covers just that "
        "part, and a later one can be added beside it."
    )
