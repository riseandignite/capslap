import json

import pytest

from app.models.captions import CaptionSegment
from app.services.broll import commons, credits, licenses, topics
from app.services.broll.assets import Asset

# A trimmed real response shape from commons.wikimedia.org/w/api.php
# (action=query, generator=search, prop=imageinfo, iiprop=extmetadata).
COMMONS_REPLY = {
    "query": {
        "pages": {
            "1": {
                "title": "File:Helsinki Cathedral.jpg",
                "imageinfo": [
                    {
                        "url": "https://upload.wikimedia.org/a.jpg",
                        "descriptionurl": "https://commons.wikimedia.org/wiki/File:Helsinki_Cathedral.jpg",
                        "mime": "image/jpeg",
                        "width": 4000,
                        "height": 3000,
                        "extmetadata": {
                            "LicenseShortName": {"value": "CC BY-SA 4.0"},
                            "LicenseUrl": {
                                "value": "https://creativecommons.org/licenses/by-sa/4.0"
                            },
                            "Artist": {"value": '<a href="//x">Jane <b>Doe</b></a>'},
                        },
                    }
                ],
            },
            "2": {
                "title": "File:Tram.webm",
                "imageinfo": [
                    {
                        "url": "https://upload.wikimedia.org/t.webm",
                        "descriptionurl": "https://commons.wikimedia.org/wiki/File:Tram.webm",
                        "mime": "video/webm",
                        "width": 1920,
                        "height": 1080,
                        "extmetadata": {
                            "LicenseShortName": {"value": "Public domain"},
                            "Artist": {"value": "City of Helsinki"},
                        },
                    }
                ],
            },
            "3": {
                "title": "File:NoDerivs.jpg",
                "imageinfo": [
                    {
                        "url": "https://upload.wikimedia.org/n.jpg",
                        "descriptionurl": "https://commons.wikimedia.org/wiki/File:NoDerivs.jpg",
                        "mime": "image/jpeg",
                        "width": 800,
                        "height": 600,
                        "extmetadata": {
                            "LicenseShortName": {"value": "CC BY-ND 2.0"},
                            "Artist": {"value": "Someone"},
                        },
                    }
                ],
            },
            "4": {
                "title": "File:Diagram.svg",
                "imageinfo": [
                    {
                        "url": "https://upload.wikimedia.org/d.svg",
                        "descriptionurl": "https://commons.wikimedia.org/wiki/File:Diagram.svg",
                        "mime": "image/svg+xml",
                        "width": 100,
                        "height": 100,
                        "extmetadata": {"LicenseShortName": {"value": "CC0"}},
                    }
                ],
            },
        }
    }
}


@pytest.mark.parametrize(
    "name,family",
    [
        ("CC0", "cc0"),
        ("CC0 1.0", "cc0"),
        ("Public domain", "pd"),
        ("PD-old-70", "pd"),
        ("CC BY 2.0", "cc-by"),
        ("CC BY-SA 4.0", "cc-by-sa"),
        ("CC BY-NC 2.0", None),
        ("CC BY-ND 2.0", None),
        ("CC BY-NC-SA 3.0", None),
        ("All rights reserved", None),
        ("", None),
    ],
)
def test_license_family(name, family):
    assert licenses.family(name) == family


def test_commons_keeps_only_usable_licences_and_raster_or_video():
    calls = []

    def fetch(url, params):
        calls.append(params)
        return COMMONS_REPLY

    found = commons.search("helsinki", fetch=fetch)
    # ND is refused (cropping and pan/zoom modify the work), SVG is not footage.
    assert [a.title for a in found] == ["Helsinki Cathedral.jpg", "Tram.webm"]
    assert [a.kind for a in found] == ["image", "video"]
    first = found[0]
    assert first.author == "Jane Doe"  # markup stripped
    assert first.license == "CC BY-SA 4.0"
    assert first.source == "Wikimedia Commons"
    assert calls[0]["gsrsearch"].startswith("helsinki")
    assert calls[0]["gsrnamespace"] == "6"


def test_commons_unknown_author_is_kept_as_none():
    found = commons.search("x", fetch=lambda u, p: COMMONS_REPLY)
    assert found[1].author == "City of Helsinki"
    assert Asset.__dataclass_fields__["author"].default is None


def test_credit_line_names_title_author_licence_and_source():
    a = Asset(
        title="Helsinki Cathedral.jpg",
        kind="image",
        url="u",
        page_url="https://commons.wikimedia.org/wiki/File:Helsinki_Cathedral.jpg",
        author="Jane Doe",
        license="CC BY-SA 4.0",
        license_url="https://creativecommons.org/licenses/by-sa/4.0",
        source="Wikimedia Commons",
    )
    assert (
        credits.line(a)
        == "Helsinki Cathedral.jpg by Jane Doe, CC BY-SA 4.0, Wikimedia Commons"
    )
    assert credits.line(Asset(**{**a.__dict__, "author": None})).startswith(
        "Helsinki Cathedral.jpg, CC BY-SA 4.0"
    )


def test_description_text_lists_each_asset_once_with_links():
    a = Asset(
        title="T",
        kind="image",
        url="u",
        page_url="https://p/1",
        author="A",
        license="CC0",
        source="Wikimedia Commons",
    )
    text = credits.description([a, a])
    assert text.count("https://p/1") == 1
    assert text.splitlines()[0] == "Images and footage"


def seg(start, end, text):
    return CaptionSegment(start_ms=start, end_ms=end, text=text)


def test_windows_group_cues_up_to_the_target_length():
    segs = [seg(i * 3000, (i + 1) * 3000, f"cue {i}") for i in range(10)]
    wins = topics.windows(segs, target_ms=9000)
    assert [(w.start_ms, w.end_ms) for w in wins] == [
        (0, 9000),
        (9000, 18000),
        (18000, 27000),
        (27000, 30000),
    ]


def test_query_prefers_frequent_content_words_and_drops_stopwords():
    segs = [
        seg(0, 4000, "The tram goes past the cathedral and the tram stops"),
        seg(4000, 8000, "Then the tram turns toward the harbour"),
    ]
    (w,) = topics.windows(segs, target_ms=8000)
    words = w.query.split()
    assert words[0] == "tram"
    assert "the" not in words and "and" not in words
    assert len(words) <= 3


def test_finnish_stopwords_are_dropped():
    segs = [seg(0, 4000, "Ja sitten se ratikka meni sillan yli ja ratikka pysähtyi")]
    (w,) = topics.windows(segs, target_ms=4000)
    assert "ja" not in w.query.split()
    assert w.query.split()[0] == "ratikka"


def test_empty_transcript_gives_no_windows():
    assert topics.windows([], target_ms=8000) == []


def test_asset_round_trips_through_json():
    a = Asset(title="T", kind="video", url="u", page_url="p", license="CC0")
    assert Asset.from_dict(json.loads(json.dumps(a.to_dict()))) == a


def test_proposals_pair_each_window_with_its_candidates():
    from app.services.broll import suggest

    segs = [seg(0, 4000, "tram tram cathedral"), seg(4000, 20000, "harbour harbour")]
    asked = []

    def search(q, limit=20):
        asked.append(q)
        return [Asset(title=q, kind="image", url="u", page_url="p", license="CC0")]

    props = suggest.proposals(segs, search=search, target_ms=4000)
    assert asked == ["tram cathedral", "harbour"]
    assert [p["startMs"] for p in props] == [0, 4000]
    assert props[0]["candidates"][0]["title"] == "tram cathedral"
    assert props[0]["chosen"] is None  # a person picks; nothing is auto-accepted


def test_http_retries_on_429_and_honours_retry_after(monkeypatch):
    import io
    import urllib.error

    attempts, slept = [], []

    def urlopen(req, timeout):
        attempts.append(1)
        if len(attempts) < 3:
            raise urllib.error.HTTPError(
                req.full_url, 429, "Too Many", {"Retry-After": "2"}, io.BytesIO()
            )
        return io.BytesIO(b'{"ok": 1}')

    monkeypatch.setattr(commons.net.urllib.request, "urlopen", urlopen)
    monkeypatch.setattr(commons.net.time, "sleep", slept.append)
    assert commons._http("https://x", {"a": "b"}) == {"ok": 1}
    assert slept == [2, 2]


def test_http_gives_up_after_the_last_retry(monkeypatch):
    import io
    import urllib.error

    def urlopen(req, timeout):
        raise urllib.error.HTTPError(req.full_url, 429, "x", {}, io.BytesIO())

    monkeypatch.setattr(commons.net.urllib.request, "urlopen", urlopen)
    monkeypatch.setattr(commons.net.time, "sleep", lambda s: None)
    with pytest.raises(urllib.error.HTTPError):
        commons._http("https://x", {})


def test_doubled_author_markup_is_collapsed():
    # Commons repeats the text for some uploads: "<span>X</span><span>X</span>".
    assert commons._plain("<span>Unknown author</span><span>Unknown author</span>") == (
        "Unknown author"
    )
    assert commons._plain("<a>Ann</a> <a>Ann</a>") == "Ann Ann"  # two people, kept
