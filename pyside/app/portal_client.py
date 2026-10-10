"""Talking to the review portal (portal/ in this repo) as its editor.

Plain urllib: a handful of JSON calls and one streamed upload, run from a
worker thread by the window.
"""

import json
import os
import urllib.error
import urllib.request
from collections.abc import Callable
from typing import Any

CHUNK = 1024 * 1024


class PortalError(RuntimeError):
    pass


class Portal:
    def __init__(self, url: str, token: str, timeout: float = 30.0):
        self.url = url.rstrip("/")
        self.token = token
        self.timeout = timeout

    def _request(
        self,
        method: str,
        path: str,
        body: Any = None,
        data: Any = None,
        headers: dict | None = None,
    ):
        all_headers = {"Authorization": f"Bearer {self.token}", **(headers or {})}
        if body is not None:
            data = json.dumps(body).encode("utf-8")
            all_headers["Content-Type"] = "application/json"
        request = urllib.request.Request(
            self.url + path, data=data, method=method, headers=all_headers
        )
        try:
            return urllib.request.urlopen(request, timeout=self.timeout)
        except urllib.error.HTTPError as err:
            try:
                reason = json.load(err).get("error") or err.reason
            except ValueError:
                reason = err.reason
            if err.code == 401:
                reason = "the portal refused the admin token"
            raise PortalError(f"Portal: {reason} ({err.code})") from err
        except (urllib.error.URLError, OSError) as err:
            raise PortalError(f"Can't reach the portal at {self.url}: {err}") from err

    def _json(self, method: str, path: str, body: Any = None) -> Any:
        with self._request(method, path, body=body) as response:
            return json.load(response)

    def series(self) -> list[dict]:
        """Every series with its episodes (id, title), for picking where to publish."""
        return self._json("GET", "/api/admin/series")

    def publish(
        self,
        series: str,
        episode: str,
        label: str,
        filename: str,
        captions: dict | None,
    ) -> dict:
        """Make the video's place (series and episode made when new); upload next."""
        return self._json(
            "POST",
            "/api/admin/publish",
            {
                "series": series,
                "episode": episode,
                "label": label,
                "filename": filename,
                "captions": captions,
            },
        )

    def upload(
        self,
        upload_url: str,
        path: str,
        progress: Callable[[float], None] | None = None,
    ) -> None:
        size = os.path.getsize(path)

        def chunks():
            sent = 0
            with open(path, "rb") as f:
                while block := f.read(CHUNK):
                    sent += len(block)
                    if progress:
                        progress(sent / size if size else 1.0)
                    yield block

        # Uploads of a big render take longer than any JSON call.
        saved, self.timeout = self.timeout, max(self.timeout, 600.0)
        try:
            with self._request(
                "PUT",
                upload_url,
                data=chunks(),
                headers={
                    "Content-Type": "video/mp4",
                    "Content-Length": str(size),
                },
            ):
                pass
        finally:
            self.timeout = saved

    def latest_captions(self, video_id: int) -> dict:
        """The video's newest captions: the client's corrections, if any came."""
        return self._json("GET", f"/api/admin/videos/{video_id}/captions")
