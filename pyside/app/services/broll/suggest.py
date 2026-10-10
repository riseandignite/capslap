"""Transcript in, b-roll proposals out.

    python -m app.services.broll.suggest episode.capslap.json -o broll.json

Each proposal is a time window, the query used and its candidates. `chosen`
stays null: a person fills it in, because a candidate that matches the words
is often the wrong picture.
"""

import argparse
import json
from collections.abc import Callable
from pathlib import Path
from typing import Any

from app.models.captions import CaptionSegment, CaptionsFile
from app.services.broll import sources, topics
from app.services.broll.assets import Asset

Search = Callable[..., list[Asset]]


def proposals(
    segments: list[CaptionSegment],
    search: Search = sources.search,
    target_ms: int = 8000,
    limit: int = 20,
) -> list[dict[str, Any]]:
    out = []
    for w in topics.windows(segments, target_ms=target_ms):
        found = search(w.query, limit=limit) if w.query else []
        out.append(
            {
                "startMs": w.start_ms,
                "endMs": w.end_ms,
                "text": w.text,
                "query": w.query,
                "candidates": [a.to_dict() for a in found],
                "chosen": None,
            }
        )
    return out


def main(argv: list[str] | None = None) -> None:
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("captions", type=Path, help=".capslap.json sidecar")
    p.add_argument("-o", "--output", type=Path, required=True)
    p.add_argument("--window", type=float, default=8.0, help="seconds per window")
    args = p.parse_args(argv)
    cf = CaptionsFile.from_json_str(args.captions.read_text(encoding="utf-8"))
    result = proposals(cf.segments, target_ms=int(args.window * 1000))
    args.output.write_text(json.dumps(result, indent=2, ensure_ascii=False), "utf-8")


if __name__ == "__main__":
    main()
