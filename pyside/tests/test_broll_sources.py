import json

import pytest

from app.services.broll import archive, download, nasa, net, openverse, sources
from app.services.broll.assets import Asset

# --- Openverse ----------------------------------------------------------------

OPENVERSE = {
    "results": [
        {
            "title": "Tram in snow",
            "creator": "Ann",
            "url": "https://live.staticflickr.com/a.jpg",
            "thumbnail": "https://api.openverse.org/v1/images/1/thumb/",
            "foreign_landing_url": "https://www.flickr.com/photos/x/1",
            "license": "by",
            "license_version": "2.0",
            "license_url": "https://creativecommons.org/licenses/by/2.0/",
            "provider": "flickr",
            "width": 4000,
            "height": 3000,
        },
        {
            "title": "Harbour",
            "creator": None,
            "url": "https://x/h.png",
            "foreign_landing_url": "https://x/h",
            "license": "cc0",
            "license_version": "1.0",
            "provider": "wikimedia",
            "width": None,
            "height": None,
        },
        {  # an NC picture that slipped through must not
            "title": "Nope",
            "url": "https://x/n.jpg",
            "foreign_landing_url": "https://x/n",
            "license": "by-nc",
            "license_version": "2.0",
            "provider": "flickr",
        },
        {"title": "No file", "license": "by", "license_version": "4.0"},
    ]
}


def test_openverse_asks_only_for_licences_that_allow_commercial_use_and_changes():
    seen = {}

    def fetch(url, params):
        seen.update(params, _url=url)
        return OPENVERSE

    openverse.search("tram", limit=5, fetch=fetch)
    assert seen["_url"] == "https://api.openverse.org/v1/images/"
    assert seen["q"] == "tram" and seen["page_size"] == "5"
    assert seen["license_type"] == "commercial,modification"


def test_openverse_results_become_credited_assets():
    found = openverse.search("tram", fetch=lambda u, p: OPENVERSE)
    assert [a.title for a in found] == [
        "Tram in snow",
        "Harbour",
    ]  # NC and fileless dropped
    first, second = found
    assert first.license == "CC BY 2.0"
    assert first.author == "Ann" and first.kind == "image"
    assert first.source == "Flickr via Openverse"
    assert first.page_url == "https://www.flickr.com/photos/x/1"
    assert (first.width, first.height) == (4000, 3000)
    assert first.thumb_url.endswith("/thumb/")
    assert second.license == "CC0" and second.author is None and second.width == 0


# --- Internet Archive ---------------------------------------------------------

ARCHIVE_SEARCH = {
    "response": {
        "docs": [
            {
                "identifier": "tram1920",
                "title": "Tram, 1920",
                "creator": ["City Film Office", "Someone"],
                "licenseurl": "http://creativecommons.org/publicdomain/mark/1.0/",
            },
            {
                "identifier": "harbour",
                "title": "Harbour",
                "creator": "Bo",
                "licenseurl": "https://creativecommons.org/licenses/by/4.0/",
            },
            {
                "identifier": "nc",
                "title": "Not for money",
                "licenseurl": "https://creativecommons.org/licenses/by-nc/4.0/",
            },
        ]
    }
}


@pytest.mark.parametrize(
    "url,name",
    [
        ("http://creativecommons.org/publicdomain/zero/1.0/", "CC0"),
        ("http://creativecommons.org/publicdomain/mark/1.0/", "Public domain"),
        # the old public-domain dedication, still on much of the Prelinger archive
        ("http://creativecommons.org/licenses/publicdomain/", "Public domain"),
        ("https://creativecommons.org/licenses/by/4.0/", "CC BY 4.0"),
        ("https://creativecommons.org/licenses/by-sa/3.0/", "CC BY-SA 3.0"),
        ("https://creativecommons.org/licenses/by-nc/4.0/", "CC BY-NC 4.0"),
        ("https://creativecommons.org/licenses/by-nd/2.0/", "CC BY-ND 2.0"),
        ("https://example.com/terms", ""),
    ],
)
def test_archive_licence_names_come_from_the_licence_url(url, name):
    assert archive.license_name(url) == name


def test_archive_searches_videos_that_carry_a_creative_commons_licence():
    seen = {}

    def fetch(url, params):
        seen.update(params, _url=url)
        return ARCHIVE_SEARCH

    archive.search("tram", limit=7, fetch=fetch)
    assert seen["_url"] == "https://archive.org/advancedsearch.php"
    assert "mediatype:(movies)" in seen["q"]
    # only the licences a reel may use are asked for, the NC and ND ones never
    assert "publicdomain" in seen["q"] and "licenses\\/by\\/" in seen["q"]
    assert "licenses\\/by-sa\\/" in seen["q"] and "by-nc" not in seen["q"]
    assert "tram" in seen["q"] and seen["rows"] == "7" and seen["output"] == "json"


def test_archive_results_wait_to_be_resolved_and_drop_unusable_licences():
    found = archive.search("tram", fetch=lambda u, p: ARCHIVE_SEARCH)
    assert [a.title for a in found] == ["Tram, 1920", "Harbour"]
    a = found[0]
    assert a.kind == "video" and a.license == "Public domain"
    assert a.author == "City Film Office, Someone"
    assert a.page_url == "https://archive.org/details/tram1920"
    assert a.thumb_url == "https://archive.org/services/img/tram1920"
    assert a.resolver == "archive" and a.url == "https://archive.org/metadata/tram1920"
    assert a.source == "Internet Archive"


ARCHIVE_META = {
    "files": [
        {
            "name": "tram.ia.mp4",
            "format": "h.264 IA",
            "size": "9000000",
            "width": "320",
            "height": "240",
        },
        {"name": "tram_512kb.mp4", "format": "512Kb MPEG4", "size": "3000000"},
        {
            "name": "tram.mp4",
            "format": "h.264",
            "size": "60000000",
            "width": "640",
            "height": "480",
        },
        {
            "name": "tram_huge.mp4",
            "format": "h.264",
            "size": "900000000",
            "width": "1920",
            "height": "1080",
        },
        {"name": "tram.ogv", "format": "Ogg Video", "size": "5000000"},
        {"name": "notes.txt", "format": "Text", "size": "10"},
    ]
}


def test_archive_resolves_to_the_biggest_mp4_that_is_not_enormous():
    a = archive.search("tram", fetch=lambda u, p: ARCHIVE_SEARCH)[0]
    r = archive.resolve(a, fetch=lambda url, params=None: ARCHIVE_META)
    assert r.url == "https://archive.org/download/tram1920/tram.mp4"
    assert (r.width, r.height) == (640, 480)
    assert r.resolver is None


def test_archive_without_any_mp4_is_an_error():
    a = archive.search("tram", fetch=lambda u, p: ARCHIVE_SEARCH)[0]
    with pytest.raises(ValueError):
        archive.resolve(
            a, fetch=lambda url, params=None: {"files": [{"name": "a.ogv"}]}
        )


def test_archive_file_names_are_quoted_in_the_download_url():
    a = archive.search("tram", fetch=lambda u, p: ARCHIVE_SEARCH)[0]
    meta = {
        "files": [{"name": "Tram film (1920).mp4", "format": "h.264", "size": "100"}]
    }
    r = archive.resolve(a, fetch=lambda url, params=None: meta)
    assert r.url.endswith("/tram1920/Tram%20film%20%281920%29.mp4")


# --- NASA ---------------------------------------------------------------------

NASA_SEARCH = {
    "collection": {
        "items": [
            {
                "href": "https://images-assets.nasa.gov/image/PIA1/collection.json",
                "data": [
                    {
                        "nasa_id": "PIA1",
                        "title": "Mars at dawn",
                        "media_type": "image",
                        "photographer": "J. Smith",
                        "center": "JPL",
                    }
                ],
                "links": [
                    {
                        "href": "https://images-assets.nasa.gov/image/PIA1/PIA1~thumb.jpg",
                        "render": "image",
                    }
                ],
            },
            {
                "href": "https://images-assets.nasa.gov/video/V1/collection.json",
                "data": [
                    {
                        "nasa_id": "V1",
                        "title": "Launch",
                        "media_type": "video",
                        "center": "KSC",
                    }
                ],
                "links": [
                    {
                        "href": "https://images-assets.nasa.gov/video/V1/V1~thumb.jpg",
                        "render": "image",
                    }
                ],
            },
            {"data": [{"nasa_id": "A1", "title": "Sound", "media_type": "audio"}]},
        ]
    }
}


def test_nasa_searches_pictures_and_video():
    seen = {}

    def fetch(url, params):
        seen.update(params, _url=url)
        return NASA_SEARCH

    nasa.search("mars", limit=6, fetch=fetch)
    assert seen["_url"] == "https://images-api.nasa.gov/search"
    assert seen["q"] == "mars" and seen["media_type"] == "image,video"
    assert seen["page_size"] == "6"


def test_nasa_results_are_public_domain_and_credit_the_photographer_or_centre():
    found = nasa.search("mars", fetch=lambda u, p: NASA_SEARCH)
    assert [a.title for a in found] == ["Mars at dawn", "Launch"]  # audio dropped
    img, vid = found
    assert (img.kind, vid.kind) == ("image", "video")
    assert (
        img.license == "Public domain" and img.source == "NASA Image and Video Library"
    )
    assert img.author == "J. Smith" and vid.author == "KSC"
    assert img.page_url == "https://images.nasa.gov/details/PIA1"
    assert img.thumb_url.endswith("PIA1~thumb.jpg")
    assert img.resolver == "nasa" and img.url.endswith("/collection.json")


def test_nasa_resolves_to_a_good_sized_file():
    img, vid = nasa.search("mars", fetch=lambda u, p: NASA_SEARCH)
    files = [
        "https://x/PIA1~orig.tif",
        "https://x/PIA1~orig.jpg",
        "https://x/PIA1~large.jpg",
        "https://x/PIA1~medium.jpg",
        "https://x/PIA1~thumb.jpg",
        "https://x/PIA1.json",
    ]
    assert (
        nasa.resolve(img, fetch=lambda url, params=None: files).url
        == "https://x/PIA1~large.jpg"
    )
    vids = [
        "https://x/V1~orig.mp4",
        "https://x/V1~mobile.mp4",
        "https://x/V1~medium.mp4",
        "https://x/V1.srt",
    ]
    assert (
        nasa.resolve(vid, fetch=lambda url, params=None: vids).url
        == "https://x/V1~medium.mp4"
    )
    assert (
        nasa.resolve(vid, fetch=lambda url, params=None: ["https://x/V1~orig.mp4"]).url
        == "https://x/V1~orig.mp4"
    )


def test_nasa_without_a_usable_file_is_an_error():
    img, _ = nasa.search("mars", fetch=lambda u, p: NASA_SEARCH)
    with pytest.raises(ValueError):
        nasa.resolve(img, fetch=lambda url, params=None: ["https://x/readme.txt"])


# --- putting them together ----------------------------------------------------


def a(title, url=None, source="s"):
    return Asset(
        title=title,
        kind="image",
        url=url or f"u/{title}",
        page_url=f"p/{title}",
        license="CC0",
        source=source,
    )


def test_results_from_every_source_are_interleaved_so_all_show_up(monkeypatch):
    fake = {
        "commons": lambda q, limit: [a("c1"), a("c2"), a("c3")],
        "openverse": lambda q, limit: [a("o1"), a("o2")],
        "archive": lambda q, limit: [a("i1")],
        "nasa": lambda q, limit: [a("n1")],
    }
    monkeypatch.setattr(sources, "SEARCHES", fake)
    titles = [x.title for x in sources.search("q", limit=20)]
    assert titles == ["c1", "o1", "i1", "n1", "c2", "o2", "c3"]


def test_the_limit_applies_to_the_merged_list(monkeypatch):
    fake = {
        "commons": lambda q, limit: [a(f"c{i}") for i in range(10)],
        "openverse": lambda q, limit: [a(f"o{i}") for i in range(10)],
    }
    monkeypatch.setattr(sources, "SEARCHES", fake)
    assert len(sources.search("q", limit=6)) == 6


def test_a_source_that_fails_does_not_stop_the_others(monkeypatch, caplog):
    def boom(q, limit):
        raise OSError("offline")

    monkeypatch.setattr(
        sources, "SEARCHES", {"commons": boom, "nasa": lambda q, limit: [a("n1")]}
    )
    with caplog.at_level("WARNING"):
        assert [x.title for x in sources.search("q")] == ["n1"]
    assert "commons" in caplog.text


def test_the_same_picture_from_two_sources_is_listed_once(monkeypatch):
    same = a("x", url="https://same/file.jpg")
    other = a("y", url="https://same/file.jpg")
    monkeypatch.setattr(
        sources,
        "SEARCHES",
        {"commons": lambda q, limit: [same], "openverse": lambda q, limit: [other]},
    )
    assert len(sources.search("q")) == 1


def test_only_the_named_sources_are_searched(monkeypatch):
    calls = []
    fake = {
        "commons": lambda q, limit: calls.append("commons") or [],
        "nasa": lambda q, limit: calls.append("nasa") or [],
    }
    monkeypatch.setattr(sources, "SEARCHES", fake)
    sources.search("q", only=("nasa",))
    assert calls == ["nasa"]


def test_a_resolver_names_the_module_that_finishes_the_asset(monkeypatch):
    seen = []
    monkeypatch.setattr(
        sources, "RESOLVERS", {"nasa": lambda asset: seen.append(asset) or a("done")}
    )
    pending = Asset(title="t", kind="image", url="u", page_url="p", resolver="nasa")
    assert sources.resolve(pending).title == "done"
    plain = a("plain")
    assert sources.resolve(plain) is plain  # nothing to do


def test_downloading_resolves_first_and_names_the_file_from_the_real_url(
    tmp_path, monkeypatch
):
    pending = Asset(
        title="t",
        kind="image",
        url="https://meta/collection.json",
        page_url="p",
        resolver="nasa",
    )
    real = a("t", url="https://files/PIA1~large.jpg")
    monkeypatch.setattr(download.sources, "resolve", lambda asset: real)
    monkeypatch.setattr(download.net, "get", lambda url, params=None: b"jpegdata")
    path = download.fetch(pending, tmp_path)
    assert path.suffix == ".jpg" and path.read_bytes() == b"jpegdata"


def test_get_json_parses_what_get_returns(monkeypatch):
    monkeypatch.setattr(
        net, "get", lambda url, params=None: json.dumps({"a": 1}).encode()
    )
    assert net.get_json("https://x", {"q": "1"}) == {"a": 1}
