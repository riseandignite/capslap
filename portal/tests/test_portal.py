import json

import pytest
from fastapi.testclient import TestClient

from capslap_portal.app import Settings, create_app

ADMIN = "test-admin-token-123456"
AUTH = {"Authorization": f"Bearer {ADMIN}"}

CAPTIONS = {
    "segments": [
        {"startMs": 0, "endMs": 1500, "text": "Kylläpä on sää"},
        {"startMs": 1500, "endMs": 3000, "text": "tekoäly yhtiöt"},
    ],
    "video": {"name": "talk.mp4", "durationMs": 3000},
}


@pytest.fixture
def client(tmp_path):
    app = create_app(Settings(data_dir=tmp_path, admin_token=ADMIN))
    with TestClient(app) as c:
        yield c


def publish(client, series="Tekoälypodi", episode="Jakso 4", label="9:16 proof"):
    r = client.post(
        "/api/admin/publish",
        headers=AUTH,
        json={
            "series": series,
            "episode": episode,
            "label": label,
            "filename": "talk.mp4",
            "captions": CAPTIONS,
        },
    )
    assert r.status_code == 200, r.text
    out = r.json()
    up = client.put(out["video"]["uploadUrl"], headers=AUTH, content=b"\x00" * 5000)
    assert up.status_code == 200, up.text
    return out


def test_admin_needs_the_token(client):
    assert client.get("/api/admin/series").status_code == 401
    assert (
        client.get(
            "/api/admin/series", headers={"Authorization": "Bearer nope"}
        ).status_code
        == 401
    )
    assert client.get("/api/admin/series", headers=AUTH).json() == []


def test_publish_finds_or_makes_series_and_episode_by_title(client):
    a = publish(client)
    b = publish(client, series="tekoälypodi", episode="jakso 4", label="16:9 proof")
    c = publish(client, episode="Jakso 5")
    assert a["seriesLink"] == b["seriesLink"] == c["seriesLink"]
    assert a["episodeLink"] == b["episodeLink"] != c["episodeLink"]
    series = client.get("/api/admin/series", headers=AUTH).json()
    assert len(series) == 1 and len(series[0]["episodes"]) == 2


def test_the_secret_link_shows_the_series_and_nothing_else(client):
    mine = publish(client)
    other = publish(client, series="Toinen sarja")
    token = mine["seriesLink"].split("/")[-1]
    series = client.get(f"/api/s/{token}").json()
    assert series["title"] == "Tekoälypodi"
    assert [e["title"] for e in series["episodes"]] == ["Jakso 4"]
    assert series["episodes"][0]["videos"] == 1

    # Another series' episode and video are not reachable with this link.
    other_ep = other["episodeLink"].split("/")[-1]
    other_video = other["video"]["id"]
    assert client.get(f"/api/s/{token}/episodes/{other_ep}").status_code == 404
    assert client.get(f"/api/s/{token}/videos/{other_video}/file").status_code == 404
    assert client.get("/api/s/not-a-token").status_code == 404
    assert client.get("/s/not-a-token").status_code == 404
    assert client.get(mine["seriesLink"]).status_code == 200


def test_an_episode_plays_and_downloads_its_videos(client):
    out = publish(client)
    token = out["seriesLink"].split("/")[-1]
    ep = client.get(
        f"/api/s/{token}/episodes/{out['episodeLink'].split('/')[-1]}"
    ).json()
    (video,) = ep["videos"]
    assert video["label"] == "9:16 proof"
    whole = client.get(video["fileUrl"])
    assert whole.status_code == 200 and len(whole.content) == 5000
    # Players seek with ranges.
    part = client.get(video["fileUrl"], headers={"Range": "bytes=100-199"})
    assert part.status_code == 206 and len(part.content) == 100
    dl = client.get(video["downloadUrl"])
    assert dl.headers["content-disposition"].startswith("attachment")
    assert "Jakso%204%20-%20916%20proof.mp4" in dl.headers["content-disposition"]


def test_videos_still_uploading_are_not_shown(client):
    r = client.post(
        "/api/admin/publish",
        headers=AUTH,
        json={"series": "S", "episode": "E", "label": "x"},
    ).json()
    token = r["seriesLink"].split("/")[-1]
    ep = client.get(f"/api/s/{token}/episodes/{r['episodeLink'].split('/')[-1]}").json()
    assert ep["videos"] == []


def test_a_review_comes_back_as_a_captions_file_for_the_app(client):
    out = publish(client)
    token = out["seriesLink"].split("/")[-1]
    vid = out["video"]["id"]
    got = client.get(f"/api/s/{token}/videos/{vid}/captions").json()
    assert got == CAPTIONS

    reviewed = json.loads(json.dumps(CAPTIONS))
    reviewed["segments"][0]["text"] = "Kyllä on sää"
    reviewed["review"] = {
        "status": "changed",
        "reviewer": "Asiakas",
        "comments": [],
        "changes": [],
    }
    r = client.post(f"/api/s/{token}/videos/{vid}/review", content=json.dumps(reviewed))
    assert r.status_code == 200, r.text

    # The next reviewer continues from it.
    assert (
        client.get(f"/api/s/{token}/videos/{vid}/captions").json()["segments"][0][
            "text"
        ]
        == "Kyllä on sää"
    )

    detail = client.get(
        f"/api/admin/series/{client.get('/api/admin/series', headers=AUTH).json()[0]['id']}",
        headers=AUTH,
    ).json()
    v = detail["episodes"][0]["videos"][0]
    assert [c["source"] for c in v["captions"]] == ["editor", "review"]
    assert v["reviews"][0]["author"] == "Asiakas"

    dl = client.get(f"/api/admin/videos/{vid}/captions", headers=AUTH)
    assert dl.json()["review"]["reviewer"] == "Asiakas"
    assert "talk.mp4.reviewed.capslap.json" in dl.headers["content-disposition"]
    first = client.get(
        f"/api/admin/videos/{vid}/captions?version={v['captions'][0]['id']}",
        headers=AUTH,
    )
    assert first.json() == CAPTIONS


def test_a_review_of_other_captions_is_refused(client):
    out = publish(client)
    token = out["seriesLink"].split("/")[-1]
    vid = out["video"]["id"]
    bad = {"segments": CAPTIONS["segments"][:1]}
    assert (
        client.post(
            f"/api/s/{token}/videos/{vid}/review", content=json.dumps(bad)
        ).status_code
        == 409
    )
    assert (
        client.post(f"/api/s/{token}/videos/{vid}/review", content=b"{nope").status_code
        == 422
    )


def test_feedback_on_an_episode_and_a_moment_in_a_video(client):
    out = publish(client)
    token = out["seriesLink"].split("/")[-1]
    eid = out["episodeLink"].split("/")[-1]
    vid = out["video"]["id"]
    url = f"/api/s/{token}/episodes/{eid}/feedback"
    assert (
        client.post(url, json={"text": "Hyvä jakso!", "author": "Maija"}).status_code
        == 200
    )
    assert (
        client.post(
            url,
            json={
                "text": "Tässä kohtaa musiikki liian kovalla",
                "videoId": vid,
                "atMs": 42000,
            },
        ).status_code
        == 200
    )
    assert client.post(url, json={"text": ""}).status_code == 422
    ep = client.get(f"/api/s/{token}/episodes/{eid}").json()
    assert [(f["author"], f["videoId"], f["atMs"]) for f in ep["feedback"]] == [
        ("Maija", None, None),
        ("", vid, 42000),
    ]


def test_rotating_the_link_retires_the_old_one(client):
    out = publish(client)
    old = out["seriesLink"].split("/")[-1]
    sid = client.get("/api/admin/series", headers=AUTH).json()[0]["id"]
    new = client.post(f"/api/admin/series/{sid}/rotate", headers=AUTH).json()["token"]
    assert new != old
    assert client.get(f"/api/s/{old}").status_code == 404
    assert client.get(f"/api/s/{new}").status_code == 200


def test_deleting_an_episode_removes_its_files(client, tmp_path):
    out = publish(client)
    vid = out["video"]["id"]
    assert (tmp_path / "videos" / f"{vid}.mp4").exists()
    eid = out["episodeLink"].split("/")[-1]
    assert client.delete(f"/api/admin/episodes/{eid}", headers=AUTH).status_code == 200
    assert not (tmp_path / "videos" / f"{vid}.mp4").exists()


def test_pages_keep_the_link_out_of_referrers_and_search(client):
    r = client.get("/")
    assert r.text.startswith("Videoiden hyväksyntä")
    assert r.headers["referrer-policy"] == "no-referrer"
    assert "noindex" in r.headers["x-robots-tag"]
    assert client.get("/admin").status_code == 200
    assert client.get("/review/").status_code == 200


def test_post_texts_are_edited_approved_and_reapproved_after_a_change(client):
    out = publish(client)
    token = out["seriesLink"].split("/")[-1]
    eid = out["episodeLink"].split("/")[-1]
    vid = out["video"]["id"]
    base = f"/api/s/{token}/videos/{vid}/posts"

    # Every service by default, no texts yet.
    (video,) = client.get(f"/api/s/{token}/episodes/{eid}").json()["videos"]
    assert video["platforms"] == [
        "youtube",
        "tiktok",
        "instagram",
        "facebook",
        "linkedin",
        "x",
    ]
    assert video["posts"] == {}
    assert client.post(f"{base}/x/approval", json={"approved": True}).status_code == 409

    # The editor writes, the client approves.
    admin_url = f"/api/admin/videos/{vid}/posts/youtube"
    r = client.put(
        admin_url,
        headers=AUTH,
        json={"title": "Agentit", "body": "Jakso 4 #tekoäly", "author": "Olli"},
    )
    assert r.json()["updatedBy"] == "Olli" and r.json()["approvedBy"] is None
    r = client.post(
        f"{base}/youtube/approval", json={"approved": True, "author": "Maija"}
    )
    assert r.json()["approvedBy"] == "Maija"

    # Saving the same text keeps the approval; changing it clears it.
    client.put(
        admin_url, headers=AUTH, json={"title": "Agentit", "body": "Jakso 4 #tekoäly"}
    )
    post = client.get(f"/api/s/{token}/episodes/{eid}").json()["videos"][0]["posts"][
        "youtube"
    ]
    assert post["approvedBy"] == "Maija"
    client.put(
        f"{base}/youtube",
        json={"title": "Agentit!", "body": "Jakso 4", "author": "Maija"},
    )
    post = client.get(f"/api/s/{token}/episodes/{eid}").json()["videos"][0]["posts"][
        "youtube"
    ]
    assert (post["title"], post["updatedBy"], post["approvedBy"]) == (
        "Agentit!",
        "Maija",
        None,
    )

    assert client.put(f"{base}/myspace", json={"body": "x"}).status_code == 404


def test_the_editor_picks_the_services_a_video_goes_to(client):
    out = publish(client)
    token = out["seriesLink"].split("/")[-1]
    eid = out["episodeLink"].split("/")[-1]
    vid = out["video"]["id"]
    url = f"/api/admin/videos/{vid}/platforms"
    r = client.put(url, headers=AUTH, json={"platforms": ["x", "instagram"]})
    assert r.json()["platforms"] == ["instagram", "x"]  # in the fixed order
    assert (
        client.put(url, headers=AUTH, json={"platforms": ["myspace"]}).status_code
        == 422
    )
    (video,) = client.get(f"/api/s/{token}/episodes/{eid}").json()["videos"]
    assert video["platforms"] == ["instagram", "x"]
    # Clients edit only the services the video goes to.
    base = f"/api/s/{token}/videos/{vid}/posts"
    assert client.put(f"{base}/linkedin", json={"body": "x"}).status_code == 404
    assert client.put(f"{base}/x", json={"body": "x"}).status_code == 200


def test_an_old_database_gets_the_new_column(tmp_path):
    import sqlite3

    old = sqlite3.connect(tmp_path / "portal.sqlite3")
    old.executescript(
        "CREATE TABLE videos (id INTEGER PRIMARY KEY, episode_id INTEGER, label TEXT,"
        " filename TEXT, size INTEGER, uploaded INTEGER, created_at REAL);"
    )
    old.close()
    create_app(Settings(data_dir=tmp_path, admin_token=ADMIN))
    cols = {
        r[1]
        for r in sqlite3.connect(tmp_path / "portal.sqlite3").execute(
            "PRAGMA table_info(videos)"
        )
    }
    assert "platforms" in cols
