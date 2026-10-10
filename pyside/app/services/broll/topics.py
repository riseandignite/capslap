"""Split a transcript into windows and name a search query for each."""

import re
from collections import Counter
from dataclasses import dataclass

from app.models.captions import CaptionSegment

_STOP = set(
    """a an the and or but if then than so to of in on at by for with from as is are
    was were be been it its this that these those i you he she we they me him her us
    them my your his our their not no do does did have has had will would can could
    just also very about into over past toward there here what when which who how
    goes go going get got one like
    ja tai mutta jos niin että se ne ei on oli ovat ole olen olet sitten kun kuin
    myös vain sen sitä siinä tämä tuo nämä nuo minä sinä hän me te he yli kanssa
    jo vielä mitä mikä kuka joka jotka tässä""".split()
)
_WORD = re.compile(r"[^\W\d_]{3,}", re.UNICODE)


@dataclass(frozen=True)
class Window:
    start_ms: int
    end_ms: int
    text: str
    query: str


def _query(text: str, words: int) -> str:
    counts = Counter(w for w in _WORD.findall(text.lower()) if w not in _STOP)
    first_seen = {
        w: i for i, w in enumerate(dict.fromkeys(_WORD.findall(text.lower())))
    }
    ranked = sorted(counts, key=lambda w: (-counts[w], first_seen[w]))
    return " ".join(ranked[:words])


def windows(
    segments: list[CaptionSegment], target_ms: int = 8000, words: int = 3
) -> list[Window]:
    out: list[Window] = []
    group: list[CaptionSegment] = []

    def flush() -> None:
        if not group:
            return
        text = " ".join(s.text for s in group)
        out.append(
            Window(group[0].start_ms, group[-1].end_ms, text, _query(text, words))
        )
        group.clear()

    for s in segments:
        if group and s.end_ms - group[0].start_ms > target_ms:
            flush()
        group.append(s)
    flush()
    return out
