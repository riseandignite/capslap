"""Publishing to the review portal and importing what comes back, against a
stand-in server that speaks the portal's editor API."""

import json
import threading
from http.server import BaseHTTPRequestHandler, HTTPServer

import pytest

from app.models.captions import CaptionSegment
from app.portal_client import Portal, PortalError
from app.views.main_window import MainWindow

TOKEN = "admin-token-for-tests"


class FakePortal:
    def __init__(self):
        self.published: list[dict] = []
        self.uploads: dict[int, bytes] = {}
        self.captions: dict[int, dict] = {}
        portal = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass

            def _reply(self, code, body):
                data = json.dumps(body).encode()
                self.send_response(code)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)

            def _authorized(self):
                if self.headers.get("Authorization") != f"Bearer {TOKEN}":
                    self._reply(401, {"error": "Wrong or missing admin token."})
                    return False
                return True

            def _body(self):
                return self.rfile.read(int(self.headers.get("Content-Length", 0)))

            def do_POST(self):
                if not self._authorized():
                    return
                body = json.loads(self._body())
                portal.published.append(body)
                vid = len(portal.published)
                portal.captions[vid] = body["captions"]
                self._reply(
                    200,
                    {
                        "video": {
                            "id": vid,
                            "uploadUrl": f"/api/admin/videos/{vid}/file",
                        },
                        "seriesLink": "/s/tok",
                        "episodeLink": "/s/tok/e/1",
                    },
                )

            def do_PUT(self):
                if not self._authorized():
                    return
                vid = int(self.path.split("/")[-2])
                portal.uploads[vid] = self._body()
                self._reply(200, {"ok": True})

            def do_GET(self):
                if not self._authorized():
                    return
                if self.path == "/api/admin/series":
                    self._reply(200, [])
                    return
                vid = int(self.path.split("/")[-2])
                self._reply(200, portal.captions[vid])

        self.server = HTTPServer(("127.0.0.1", 0), Handler)
        self.url = f"http://127.0.0.1:{self.server.server_port}"
        threading.Thread(target=self.server.serve_forever, daemon=True).start()


@pytest.fixture
def fake_portal():
    portal = FakePortal()
    yield portal
    portal.server.shutdown()


def test_a_wrong_token_is_a_readable_error(fake_portal):
    with pytest.raises(PortalError, match="admin token"):
        Portal(fake_portal.url, "nope").series()
    with pytest.raises(PortalError, match="Can't reach"):
        Portal("http://127.0.0.1:9", TOKEN, timeout=2).series()


def test_publish_uploads_the_render_and_the_fixes_come_back(
    qtbot, tmp_path, fake_portal, no_modal_message_boxes
):
    from PySide6.QtCore import QSettings

    from app.models.project import VideoMetadata
    from app.views.portal_dialog import TOKEN_KEY, URL_KEY

    window = MainWindow()
    qtbot.addWidget(window)
    window.set_caption_segments([CaptionSegment(0, 2000, "Kylläpä on sää")])
    source = tmp_path / "talk.mp4"
    source.write_bytes(b"source")
    window.project.video = VideoMetadata(path=str(source), duration_sec=2.0)
    proof = tmp_path / "talk_9x16_proof720p.mp4"
    proof.write_bytes(b"\x00" * (3 * 1024 * 1024 + 17))

    QSettings().setValue(URL_KEY, fake_portal.url)
    QSettings().setValue(TOKEN_KEY, TOKEN)
    window.publish_to_portal(
        Portal(fake_portal.url, TOKEN),
        {
            "series": "Tekoälypodi",
            "episode": "Jakso 4",
            "label": "9:16 proof",
            "video": str(proof),
            "captions": True,
        },
    )
    qtbot.waitUntil(lambda: "Published" in window.status.currentMessage(), timeout=5000)
    (sent,) = fake_portal.published
    assert (sent["series"], sent["episode"], sent["label"]) == (
        "Tekoälypodi",
        "Jakso 4",
        "9:16 proof",
    )
    assert sent["captions"]["segments"][0]["text"] == "Kylläpä on sää"
    assert len(fake_portal.uploads[1]) == proof.stat().st_size
    assert f"{fake_portal.url}/s/tok/e/1" in window.status.currentMessage()

    # A client fixes a caption on the portal; the app fetches it for this video.
    fake_portal.captions[1]["segments"][0]["text"] = "Kyllä on sää"
    fake_portal.captions[1]["review"] = {"status": "changed", "reviewer": "Maija"}
    window._on_import_from_portal()
    qtbot.waitUntil(
        lambda: window.caption_panel.segments[0].text == "Kyllä on sää", timeout=5000
    )
    assert "Maija" in no_modal_message_boxes[-1][1]
    window.close()


def test_import_from_portal_needs_a_publish_first(
    qtbot, tmp_path, no_modal_message_boxes
):
    from app.models.project import VideoMetadata

    window = MainWindow()
    qtbot.addWidget(window)
    window.project.video = VideoMetadata(path=str(tmp_path / "other.mp4"))
    window._on_import_from_portal()
    kind, text = no_modal_message_boxes[-1]
    assert kind == "information" and "Publish this video" in text
    window.close()
