"""Which licences a reel may use.

Accepted: CC0, public domain, CC BY, CC BY-SA. Refused: anything NC (a podcast
may earn money) and anything ND (cropping to 9:16 and pan/zoom modify the
work). Unrecognised names are refused, not guessed at.
"""

import re


def family(name: str) -> str | None:
    n = name.strip().lower()
    if not n:
        return None
    if re.search(r"\b(nc|nd)\b", n) or "noncommercial" in n or "all rights" in n:
        return None
    if n.startswith("cc0") or n.startswith("cc-zero"):
        return "cc0"
    if n.startswith("public domain") or n.startswith("pd") or n == "pdm":
        return "pd"
    m = re.match(r"cc[ -]by(?:[ -](sa))?\b", n)
    if m:
        return "cc-by-sa" if m.group(1) else "cc-by"
    return None
