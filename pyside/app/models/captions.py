import json
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from app.models.project import VideoMetadata


@dataclass
class WordSpan:
    start_ms: int
    end_ms: int
    text: str
    glue_to_previous: bool = False

    def to_dict(self) -> dict[str, Any]:
        d: dict[str, Any] = {
            "startMs": self.start_ms,
            "endMs": self.end_ms,
            "text": self.text,
        }
        if self.glue_to_previous:
            d["glueToPrevious"] = True
        return d

    @classmethod
    def from_dict(cls, d: dict[str, Any]) -> "WordSpan":
        return cls(
            start_ms=int(d.get("startMs", 0)),
            end_ms=int(d.get("endMs", 0)),
            text=str(d.get("text", "")),
            glue_to_previous=bool(d.get("glueToPrevious", False)),
        )


@dataclass
class CaptionSegment:
    start_ms: int
    end_ms: int
    text: str
    words: list[WordSpan] = field(default_factory=list)

    def update_text(self, new_text: str) -> None:
        self.text = new_text
        tokens = new_text.split()
        if not tokens:
            self.words = []
            return

        if not self.words:
            # Populate words from scratch across segment duration
            duration = max(1, self.end_ms - self.start_ms)
            step = duration // len(tokens)
            new_words = []
            for i, tok in enumerate(tokens):
                w_start = self.start_ms + i * step
                w_end = (
                    self.start_ms + (i + 1) * step
                    if i < len(tokens) - 1
                    else self.end_ms
                )
                lead_space = " " if i > 0 else ""
                new_words.append(
                    WordSpan(
                        start_ms=w_start,
                        end_ms=w_end,
                        text=lead_space + tok.strip(),
                        glue_to_previous=False,
                    )
                )
            self.words = new_words
            return

        if len(tokens) == len(self.words):
            # Same word count: preserve exact timings, update token text
            new_words = []
            for i, tok in enumerate(tokens):
                old_w = self.words[i]
                lead_space = (
                    " "
                    if (
                        i > 0
                        and (old_w.text.startswith(" ") or not tok.startswith(" "))
                    )
                    else ""
                )
                clean_tok = tok.strip()
                new_words.append(
                    WordSpan(
                        start_ms=old_w.start_ms,
                        end_ms=old_w.end_ms,
                        text=lead_space + clean_tok,
                        glue_to_previous=(
                            old_w.glue_to_previous
                            if clean_tok == old_w.text.strip()
                            else False
                        ),
                    )
                )
            self.words = new_words
        else:
            # Word count changed: re-interpolate word timings across segment duration
            duration = max(1, self.end_ms - self.start_ms)
            step = duration // len(tokens)
            new_words = []
            for i, tok in enumerate(tokens):
                w_start = self.start_ms + i * step
                w_end = (
                    self.start_ms + (i + 1) * step
                    if i < len(tokens) - 1
                    else self.end_ms
                )
                lead_space = " " if i > 0 else ""
                new_words.append(
                    WordSpan(
                        start_ms=w_start,
                        end_ms=w_end,
                        text=lead_space + tok.strip(),
                        glue_to_previous=False,
                    )
                )
            self.words = new_words

    def to_dict(self) -> dict[str, Any]:
        d: dict[str, Any] = {
            "startMs": self.start_ms,
            "endMs": self.end_ms,
            "text": self.text,
        }
        if self.words:
            d["words"] = [w.to_dict() for w in self.words]
        return d

    @classmethod
    def from_dict(cls, d: dict[str, Any]) -> "CaptionSegment":
        words_data = d.get("words", [])
        words = [WordSpan.from_dict(w) for w in words_data]
        return cls(
            start_ms=int(d.get("startMs", 0)),
            end_ms=int(d.get("endMs", 0)),
            text=str(d.get("text", "")),
            words=words,
        )


def combine_separated_syllables(
    segments: list[CaptionSegment],
) -> list[CaptionSegment]:
    """
    Combines separated syllables into full words both within segments and across segment boundaries.
    Prevents words from being split in the middle.
    """
    if not segments:
        return []

    # 1. Process each segment internally: merge adjacent syllable tokens
    cleaned_segments: list[CaptionSegment] = []
    for seg in segments:
        new_words: list[WordSpan] = []
        for w in seg.words:
            w_clean = w.text.strip()
            if not w_clean:
                continue
            if not new_words:
                new_words.append(
                    WordSpan(
                        start_ms=w.start_ms,
                        end_ms=w.end_ms,
                        text=w_clean,
                        glue_to_previous=w.glue_to_previous,
                    )
                )
            else:
                prev = new_words[-1]
                prev_clean = prev.text.strip()
                should_glue = w.glue_to_previous or prev_clean.endswith("-")
                if should_glue:
                    combined_text = (
                        prev_clean.rstrip("-") + w_clean.lstrip()
                        if prev_clean.endswith("-")
                        else prev_clean + w_clean.lstrip()
                    )
                    prev.text = combined_text
                    prev.end_ms = max(prev.end_ms, w.end_ms)
                else:
                    new_words.append(
                        WordSpan(
                            start_ms=w.start_ms,
                            end_ms=w.end_ms,
                            text=w_clean,
                            glue_to_previous=False,
                        )
                    )

        text_parts = [w.text for w in new_words]
        new_text = " ".join(text_parts) if new_words else seg.text
        cleaned_segments.append(
            CaptionSegment(
                start_ms=seg.start_ms,
                end_ms=seg.end_ms,
                text=new_text,
                words=new_words,
            )
        )

    # 2. Process across segment boundaries:
    i = 0
    while i < len(cleaned_segments) - 1:
        seg_cur = cleaned_segments[i]
        seg_next = cleaned_segments[i + 1]

        if seg_cur.words and seg_next.words:
            first_next = seg_next.words[0]
            last_cur = seg_cur.words[-1]

            if first_next.glue_to_previous or last_cur.text.endswith("-"):
                merged_text = (
                    last_cur.text.rstrip("-") + first_next.text.lstrip()
                    if last_cur.text.endswith("-")
                    else last_cur.text + first_next.text.lstrip()
                )
                last_cur.text = merged_text
                last_cur.end_ms = max(last_cur.end_ms, first_next.end_ms)
                seg_cur.end_ms = max(seg_cur.end_ms, last_cur.end_ms)
                seg_cur.text = " ".join(w.text for w in seg_cur.words)

                seg_next.words.pop(0)
                if seg_next.words:
                    seg_next.start_ms = seg_next.words[0].start_ms
                    seg_next.text = " ".join(w.text for w in seg_next.words)
                else:
                    cleaned_segments.pop(i + 1)
                    continue
        i += 1

    return cleaned_segments


def _join_words(words: list[WordSpan]) -> str:
    """Join word texts, combining glue_to_previous words into their neighbour with no space."""
    parts: list[str] = []
    for w in words:
        clean = w.text.strip()
        if parts and w.glue_to_previous:
            parts[-1] += clean
        else:
            parts.append(clean)
    return " ".join(parts)


MIN_SEGMENT_MS = 100

# Defaults for grouping loose words into readable cues. Roughly what fits a
# portrait frame at the usual caption sizes — two lines' worth.
GROUP_MAX_WORDS = 6
GROUP_MAX_CHARS = 42
GROUP_GAP_MS = 600
SENTENCE_ENDINGS = (".", "!", "?", "…", ":")


def group_words_into_phrases(
    segments: list[CaptionSegment],
    max_words: int = GROUP_MAX_WORDS,
    max_chars: int = GROUP_MAX_CHARS,
    gap_ms: int = GROUP_GAP_MS,
) -> list[CaptionSegment]:
    """Merge word-per-cue transcripts into phrase-sized cues.

    Transcribing in karaoke mode used to emit one cue per word, and the
    renderer quietly stitched them back together. It no longer does — the cues
    are burned as authored — so this puts the stitching in the editor, where
    the result is visible and can be adjusted.
    """
    words: list[WordSpan] = []
    for seg in segments:
        for w in seg.words or []:
            text = w.text.strip()
            if not text:
                continue
            # A syllable marked as glued, or a piece left dangling by a
            # hyphen, belongs to the word before it — grouping them into one
            # cue is not enough, they have to become one word.
            if words and (w.glue_to_previous or words[-1].text.endswith("-")):
                words[-1].text = words[-1].text.rstrip("-") + text
                words[-1].end_ms = max(words[-1].end_ms, w.end_ms)
                continue
            words.append(
                WordSpan(
                    start_ms=w.start_ms,
                    end_ms=w.end_ms,
                    text=text,
                    glue_to_previous=False,
                )
            )
        if not (seg.words or []) and seg.text.strip():
            # No word timings: spread the cue's own text across its duration.
            tokens = seg.text.split()
            step = max(1, (seg.end_ms - seg.start_ms) // max(1, len(tokens)))
            for i, tok in enumerate(tokens):
                start = seg.start_ms + i * step
                end = seg.end_ms if i == len(tokens) - 1 else start + step
                words.append(WordSpan(start_ms=start, end_ms=end, text=tok))

    if not words:
        return list(segments)

    groups: list[list[WordSpan]] = []
    current: list[WordSpan] = []
    for word in words:
        if current:
            previous = current[-1]
            chars = len(" ".join(w.text for w in current)) + 1 + len(word.text)
            if (
                previous.text.endswith(SENTENCE_ENDINGS)
                or word.start_ms - previous.end_ms > gap_ms
                or len(current) >= max_words
                or chars > max_chars
            ):
                groups.append(current)
                current = []
        current.append(word)
    if current:
        groups.append(current)

    return [
        CaptionSegment(
            start_ms=group[0].start_ms,
            end_ms=group[-1].end_ms,
            text=" ".join(w.text for w in group),
            words=group,
        )
        for group in groups
    ]


def delete_segment(segments: list[CaptionSegment], index: int) -> None:
    """Remove a cue and hand its time to the previous one.

    The words are already gone (that is why the slot is being deleted), so the
    neighbour simply holds its last word longer instead of leaving dead air.
    Deleting the first cue stretches the following one backwards instead.
    """
    if not (0 <= index < len(segments)):
        return
    removed = segments.pop(index)

    if index > 0:
        prev_seg = segments[index - 1]
        if removed.end_ms > prev_seg.end_ms:
            prev_seg.end_ms = removed.end_ms
            if prev_seg.words:
                prev_seg.words[-1].end_ms = removed.end_ms
    elif segments:
        next_seg = segments[0]
        if removed.start_ms < next_seg.start_ms:
            next_seg.start_ms = removed.start_ms
            if next_seg.words:
                next_seg.words[0].start_ms = removed.start_ms


def set_segment_time(seg: CaptionSegment, start_ms: int, end_ms: int) -> None:
    """Move or stretch a cue, scaling its word timings to match.

    The burner takes cue times from the words, not from the segment, so word
    spans have to follow or a retimed cue would render at its old position.
    """
    start_ms = max(0, int(start_ms))
    end_ms = max(start_ms + MIN_SEGMENT_MS, int(end_ms))

    if seg.words:
        old_start = seg.words[0].start_ms
        old_end = seg.words[-1].end_ms
        old_span = old_end - old_start
        new_span = end_ms - start_ms
        if old_span > 0:
            scale = new_span / old_span
            for w in seg.words:
                w.start_ms = round(start_ms + (w.start_ms - old_start) * scale)
                w.end_ms = round(start_ms + (w.end_ms - old_start) * scale)
        else:
            # Zero-length source: spread the words evenly over the new range.
            step = new_span / len(seg.words)
            for i, w in enumerate(seg.words):
                w.start_ms = round(start_ms + i * step)
                w.end_ms = round(start_ms + (i + 1) * step)
        seg.words[0].start_ms = start_ms
        seg.words[-1].end_ms = end_ms

    seg.start_ms = start_ms
    seg.end_ms = end_ms


def clamped_segment_time(
    segments: list[CaptionSegment], index: int, start_ms: int, end_ms: int
) -> tuple[int, int]:
    """Keep a retimed cue inside its neighbours and at least MIN_SEGMENT_MS long."""
    if not (0 <= index < len(segments)):
        return (start_ms, end_ms)

    lower = segments[index - 1].end_ms if index > 0 else 0
    upper = segments[index + 1].start_ms if index + 1 < len(segments) else None

    start_ms = max(lower, int(start_ms))
    end_ms = int(end_ms)
    if upper is not None:
        end_ms = min(upper, end_ms)
    if end_ms - start_ms < MIN_SEGMENT_MS:
        # Whichever edge the user moved, keep the cue playable.
        if upper is not None and start_ms + MIN_SEGMENT_MS > upper:
            start_ms = max(lower, upper - MIN_SEGMENT_MS)
            end_ms = upper
        else:
            end_ms = start_ms + MIN_SEGMENT_MS
    return (start_ms, end_ms)


def shift_word_to_prev(segments: list[CaptionSegment], index: int) -> None:
    """Move the first word of segments[index] to the end of segments[index - 1]."""
    if index <= 0 or index >= len(segments):
        return
    prev_seg = segments[index - 1]
    cur_seg = segments[index]
    if not cur_seg.words:
        return

    word = cur_seg.words.pop(0)
    prev_seg.words.append(word)
    prev_seg.end_ms = word.end_ms
    prev_seg.text = _join_words(prev_seg.words)

    if cur_seg.words:
        cur_seg.start_ms = cur_seg.words[0].start_ms
        cur_seg.text = _join_words(cur_seg.words)
    else:
        cur_seg.start_ms = cur_seg.end_ms
        cur_seg.text = ""


def shift_word_to_next(segments: list[CaptionSegment], index: int) -> None:
    """Move the last word of segments[index] to the beginning of segments[index + 1]."""
    if index < 0 or index >= len(segments) - 1:
        return
    cur_seg = segments[index]
    next_seg = segments[index + 1]
    if not cur_seg.words:
        return

    word = cur_seg.words.pop()
    word.text = word.text.strip()
    next_seg.words.insert(0, word)
    next_seg.start_ms = word.start_ms
    next_seg.text = _join_words(next_seg.words)

    if cur_seg.words:
        cur_seg.end_ms = cur_seg.words[-1].end_ms
        cur_seg.text = _join_words(cur_seg.words)
    else:
        cur_seg.end_ms = cur_seg.start_ms
        cur_seg.text = ""


FINNISH_ORPHAN_WORDS: set[str] = {
    "ja",
    "tai",
    "vai",
    "sekä",
    "eli",
    "mutta",
    "vaan",
    "jotta",
    "koska",
    "kun",
    "jos",
    "vaikka",
    "kuin",
    "niin",
    "että",
}


def apply_orphan_rules(segments: list[CaptionSegment]) -> list[CaptionSegment]:
    """
    Apply orphan prevention rules across caption segments:
    1. Words like 'tai', 'ja' shouldn't be the last word in a caption, but should start a new caption.
    2. After a comma, do not leave just one single word at the end of a caption cue; break that single word off onto the next caption.
    """
    if len(segments) <= 1:
        return segments

    # Ensure all segments have words populated
    for s in segments:
        if not s.words and s.text.strip():
            s.update_text(s.text)

    changed = True
    passes = 0
    max_passes = 10

    while changed and passes < max_passes:
        changed = False
        passes += 1

        for i in range(len(segments) - 1):
            cur_seg = segments[i]

            if len(cur_seg.words) <= 1:
                continue

            last_word_text = cur_seg.words[-1].text.strip()
            clean_last = last_word_text.strip(".,!?:;-\"'\u201d\u201c\u2019").lower()

            # Rule 1: Trailing conjunction / orphan word
            is_orphan = clean_last in FINNISH_ORPHAN_WORDS

            # Rule 2: Post-comma single word
            # If the second-to-last word ends with a comma/semicolon, leaving a single trailing word
            has_trailing_single_after_comma = False
            if len(cur_seg.words) >= 2:
                sec_last_text = cur_seg.words[-2].text.strip()
                if sec_last_text.endswith(",") or sec_last_text.endswith(";"):
                    has_trailing_single_after_comma = True

            if is_orphan or has_trailing_single_after_comma:
                shift_word_to_next(segments, i)
                changed = True

    return segments


@dataclass
class PositionOverride:
    start_ms: int
    end_ms: int
    y_pct: float

    def to_dict(self) -> dict[str, Any]:
        return {
            "startMs": self.start_ms,
            "endMs": self.end_ms,
            "yPct": float(self.y_pct),
        }

    @classmethod
    def from_dict(cls, d: dict[str, Any]) -> "PositionOverride":
        return cls(
            start_ms=int(d.get("startMs", 0)),
            end_ms=int(d.get("endMs", 0)),
            y_pct=float(d.get("yPct", 80.0)),
        )


@dataclass
class CaptionStyle:
    template_id: str = "oneliner"
    font_name: str = "Montserrat Black"
    font_size: int = 65
    text_color: str = "#ffffff"
    highlight_color: str = "#ffff00"
    outline_color: str = "#000000"
    outline_width: int = 3
    karaoke: bool = True
    # Karaoke cues wrap onto two lines instead of being forced onto one, which
    # roughly doubles how many words fit per on-screen block at a given font
    # size — single-line karaoke at a large font in a narrow (portrait) frame
    # can be squeezed down to ~2 words per block, flashing by too fast to read.
    multiline: bool = True
    # Off by default: a semi-transparent box behind the text, matching the
    # in-app preview's pill. ASS can only draw a square-cornered box (no
    # rounded corners), so this is a close but not pixel-identical match to
    # the preview even when turned on.
    background_box: bool = False
    glow_effect: bool = True
    # Two lines flush to the same width, each line's font size chosen so it
    # fills that width — the block look where a short line renders large and a
    # long one small. The renderer measures the real font to size them.
    justify_lines: bool = False

    def to_dict(self) -> dict[str, Any]:
        return {
            "templateId": self.template_id,
            "fontName": self.font_name,
            "fontSize": self.font_size,
            "textColor": self.text_color,
            "highlightColor": self.highlight_color,
            "outlineColor": self.outline_color,
            "outlineWidth": self.outline_width,
            "karaoke": self.karaoke,
            "multiline": self.multiline,
            "backgroundBox": self.background_box,
            "glowEffect": self.glow_effect,
            "justifyLines": self.justify_lines,
        }

    @classmethod
    def from_dict(cls, d: dict[str, Any]) -> "CaptionStyle":
        return cls(
            template_id=str(d.get("templateId", "oneliner")),
            font_name=str(d.get("fontName", "Montserrat Black")),
            font_size=int(d.get("fontSize", 65)),
            text_color=str(d.get("textColor", "#ffffff")),
            highlight_color=str(d.get("highlightColor", "#ffff00")),
            outline_color=str(d.get("outlineColor", "#000000")),
            outline_width=int(d.get("outlineWidth", 3)),
            karaoke=bool(d.get("karaoke", True)),
            multiline=bool(d.get("multiline", True)),
            background_box=bool(d.get("backgroundBox", False)),
            glow_effect=bool(d.get("glowEffect", True)),
            justify_lines=bool(d.get("justifyLines", False)),
        )


STYLE_PRESETS: dict[str, CaptionStyle] = {
    "oneliner": CaptionStyle(
        template_id="oneliner",
        font_name="Montserrat Black",
        font_size=65,
        text_color="#ffffff",
        highlight_color="#ffff00",
        outline_color="#000000",
        outline_width=3,
        karaoke=False,
    ),
    "karaoke": CaptionStyle(
        template_id="karaoke",
        font_name="Komika Axis",
        font_size=65,
        text_color="#ffffff",
        highlight_color="#00f924",
        outline_color="#000000",
        outline_width=3,
        karaoke=True,
    ),
    "vibrant": CaptionStyle(
        template_id="vibrant",
        font_name="Roboto Bold",
        font_size=65,
        text_color="#eaeaea",
        highlight_color="#7ef1c5",
        outline_color="#000000",
        outline_width=3,
        karaoke=False,
    ),
    "storyteller": CaptionStyle(
        template_id="storyteller",
        font_name="Montserrat Black",
        font_size=65,
        text_color="#ffffff",
        highlight_color="#f59e0b",
        outline_color="#000000",
        outline_width=3,
        karaoke=False,
    ),
    "justified": CaptionStyle(
        template_id="justified",
        font_name="Montserrat Black",
        font_size=65,
        text_color="#ffffff",
        highlight_color="#eaff00",
        outline_color="#000000",
        outline_width=3,
        karaoke=False,
        justify_lines=True,
    ),
}


@dataclass
class CaptionsFile:
    segments: list[CaptionSegment] = field(default_factory=list)
    position_overrides: list[PositionOverride] = field(default_factory=list)
    style: CaptionStyle = field(default_factory=CaptionStyle)
    trim_start_ms: int | None = None
    trim_end_ms: int | None = None

    def to_dict(self) -> dict[str, Any]:
        data = {
            "segments": [s.to_dict() for s in self.segments],
            "positionOverrides": [o.to_dict() for o in self.position_overrides],
            "style": self.style.to_dict(),
        }
        if self.trim_start_ms is not None:
            data["trimStartMs"] = self.trim_start_ms
        if self.trim_end_ms is not None:
            data["trimEndMs"] = self.trim_end_ms
        return data

    @classmethod
    def from_dict(cls, d: dict[str, Any]) -> "CaptionsFile":
        segments_raw = d.get("segments", [])
        segments = [CaptionSegment.from_dict(s) for s in segments_raw]
        overrides_raw = d.get("positionOverrides", [])
        overrides = [PositionOverride.from_dict(o) for o in overrides_raw]
        style_raw = d.get("style", {})
        style = CaptionStyle.from_dict(style_raw) if style_raw else CaptionStyle()
        return cls(
            segments=segments,
            position_overrides=overrides,
            style=style,
            trim_start_ms=(
                int(d["trimStartMs"]) if d.get("trimStartMs") is not None else None
            ),
            trim_end_ms=(
                int(d["trimEndMs"]) if d.get("trimEndMs") is not None else None
            ),
        )

    @classmethod
    def from_json_str(cls, json_str: str) -> "CaptionsFile":
        data = json.loads(json_str)
        if isinstance(data, list):
            # Legacy format: bare array of segments
            segments = [CaptionSegment.from_dict(s) for s in data]
            return cls(segments=segments, position_overrides=[])
        elif isinstance(data, dict):
            return cls.from_dict(data)
        return cls()

    def to_json_str(self, indent: int = 2) -> str:
        return json.dumps(self.to_dict(), indent=indent)


@dataclass
class ProjectState:
    video: VideoMetadata | None = None
    sidecar_path: str | None = None
    segments: list[CaptionSegment] = field(default_factory=list)
    position_overrides: list[PositionOverride] = field(default_factory=list)
    style: CaptionStyle = field(default_factory=CaptionStyle)
    is_dirty: bool = False
    trim_start_ms: int | None = None
    trim_end_ms: int | None = None

    @property
    def video_path(self) -> str | None:
        return self.video.path if self.video else None

    def get_default_sidecar_path(self) -> str | None:
        return self.sidecar_path

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
        self.trim_start_ms = None
        self.trim_end_ms = None
        self.is_dirty = False

    def get_active_segment(self, timestamp_ms: int) -> CaptionSegment | None:
        for seg in self.segments:
            if seg.start_ms <= timestamp_ms <= seg.end_ms:
                return seg
        return None

    def get_anchor_y_for_segment(
        self, seg: CaptionSegment, default_y: float = 80.0
    ) -> float:
        midpoint = (seg.start_ms + seg.end_ms) // 2
        for ov in self.position_overrides:
            if ov.start_ms <= midpoint <= ov.end_ms:
                return ov.y_pct
        return default_y

    def set_segment_position_override(self, seg: CaptionSegment, y_pct: float) -> None:
        midpoint = (seg.start_ms + seg.end_ms) // 2
        # Remove any existing override covering this midpoint
        self.position_overrides = [
            ov
            for ov in self.position_overrides
            if not (ov.start_ms <= midpoint <= ov.end_ms)
        ]
        # Add new override covering this segment span
        self.position_overrides.append(
            PositionOverride(
                start_ms=seg.start_ms,
                end_ms=seg.end_ms,
                y_pct=round(y_pct, 1),
            )
        )
        self.is_dirty = True

    def apply_position_to_all(self, y_pct: float) -> None:
        """Replace every position override with a single one spanning the
        whole timeline, so every caption sits at `y_pct`.

        Unlike per-segment overrides (matched by a segment's midpoint falling
        inside a stored [start_ms, end_ms] range), a single all-covering
        override can't be missed when segments later get re-chunked — e.g.
        re-transcribing, or toggling karaoke word-splitting — which otherwise
        leaves some of the new segments outside their old override's range
        and stuck back at the style's default position.
        """
        self.position_overrides = [
            PositionOverride(start_ms=0, end_ms=10**9, y_pct=round(y_pct, 1))
        ]
        self.is_dirty = True

    def load_sidecar(self, path: str | Path | None = None) -> bool:
        target = (
            Path(path)
            if path
            else (Path(self.sidecar_path) if self.sidecar_path else None)
        )
        if not target or not target.exists():
            return False
        try:
            content = target.read_text(encoding="utf-8")
            cf = CaptionsFile.from_json_str(content)
            self.segments = cf.segments
            self.position_overrides = cf.position_overrides
            self.style = cf.style
            self.trim_start_ms = cf.trim_start_ms
            self.trim_end_ms = cf.trim_end_ms
            self.is_dirty = False
            return True
        except (OSError, json.JSONDecodeError):
            return False

    def save_sidecar(self, path: str | Path | None = None) -> bool:
        target = (
            Path(path)
            if path
            else (Path(self.sidecar_path) if self.sidecar_path else None)
        )
        if not target:
            return False
        try:
            cf = CaptionsFile(
                segments=self.segments,
                position_overrides=self.position_overrides,
                style=self.style,
                trim_start_ms=self.trim_start_ms,
                trim_end_ms=self.trim_end_ms,
            )
            target.write_text(cf.to_json_str(), encoding="utf-8")
            self.is_dirty = False
            return True
        except OSError:
            return False
