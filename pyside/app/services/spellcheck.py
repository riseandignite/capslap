"""Optional Finnish spell checking through libvoikko.

Both the Python wrapper and the native library are optional. When either is
missing the checker reports no errors at all, so the app behaves exactly as
before for anyone who has not run `brew install libvoikko`.
"""

import os
import re
import sys
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from app.models.captions import CaptionSegment

# Letters only: digits, timecodes and punctuation are never spell checked.
WORD_RE = re.compile(r"[^\W\d_]+(?:['’\-][^\W\d_]+)*", re.UNICODE)

# Homebrew (arm64 and x86_64) and MacPorts install libvoikko outside the
# default dyld search path, so try those before giving up.
_MAC_LIBRARY_PATHS = ("/opt/homebrew/lib", "/usr/local/lib", "/opt/local/lib")
_LINUX_LIBRARY_PATHS = ("/usr/lib", "/usr/local/lib")


def _library_search_paths() -> tuple[str, ...]:
    override = os.environ.get("PYCAPSLAP_VOIKKO_LIB_PATH")
    paths: tuple[str, ...] = (override,) if override else ()
    if sys.platform == "darwin":
        return paths + _MAC_LIBRARY_PATHS
    if sys.platform.startswith("linux"):
        return paths + _LINUX_LIBRARY_PATHS
    return paths


def _native_library_name() -> str:
    if sys.platform == "darwin":
        return "libvoikko.1.dylib"
    if os.name == "nt":
        return "libvoikko-1.dll"
    return "libvoikko.so.1"


def _native_library_loadable(directory: str | None) -> bool:
    import ctypes

    name = _native_library_name()
    target = os.path.join(directory, name) if directory else name
    try:
        ctypes.CDLL(target)
        return True
    except OSError:
        return False


class SpellChecker:
    """Thin wrapper over Voikko that degrades to a no-op when unavailable."""

    def __init__(self, language: str = "fi") -> None:
        self._voikko = None
        self._cache: dict[str, bool] = {}
        self._load(language)

    def _load(self, language: str) -> None:
        if os.environ.get("PYCAPSLAP_DISABLE_SPELLCHECK"):
            return
        try:
            from libvoikko import Voikko
        except ImportError:
            return

        # Probe with ctypes first: constructing Voikko without a loadable
        # native library raises from __init__ and then throws again from
        # __del__, which spams stderr.
        for lib_path in (None, *_library_search_paths()):
            if not _native_library_loadable(lib_path):
                continue
            try:
                if lib_path is not None:
                    Voikko.setLibrarySearchPath(lib_path)
                self._voikko = Voikko(language)
                return
            except Exception:
                self._voikko = None

    @property
    def available(self) -> bool:
        return self._voikko is not None

    def is_correct(self, word: str) -> bool:
        if self._voikko is None or not word:
            return True
        cached = self._cache.get(word)
        if cached is not None:
            return cached
        try:
            ok = bool(self._voikko.spell(word))
        except Exception:
            ok = True
        self._cache[word] = ok
        return ok

    def suggest(self, word: str) -> list[str]:
        if self._voikko is None or not word:
            return []
        try:
            return list(self._voikko.suggest(word))
        except Exception:
            return []

    def check_text(self, text: str) -> list[tuple[int, int, str]]:
        """Return (start, end, word) for every misspelled word in `text`."""
        if self._voikko is None or not text:
            return []
        spans: list[tuple[int, int, str]] = []
        for m in WORD_RE.finditer(text):
            word = m.group()
            if not self.is_correct(word):
                spans.append((m.start(), m.end(), word))
        return spans

    def misspelled_spans(self, segment: "CaptionSegment") -> list[tuple[int, int, str]]:
        """Misspelled words of a caption, ignoring deliberately split syllables.

        A cue may start with a syllable glued to the previous cue ("pa" of
        "kaup|pa") or end with a hyphenated fragment. Those are not typos, so
        the first and last word are skipped when they look like a split word.
        """
        text = segment.text
        spans = self.check_text(text)
        if not spans:
            return []

        words = getattr(segment, "words", None) or []
        skip_first = bool(words) and words[0].glue_to_previous
        skip_last = text.rstrip().endswith("-")

        if skip_first and spans and spans[0][0] == (len(text) - len(text.lstrip())):
            spans = spans[1:]
        if skip_last and spans and spans[-1][1] >= len(text.rstrip().rstrip("-")):
            spans = spans[:-1]
        return spans


_shared: SpellChecker | None = None


def get_shared_checker() -> SpellChecker:
    """One Voikko instance per process; loading the dictionary is not cheap."""
    global _shared
    if _shared is None:
        _shared = SpellChecker()
    return _shared
