"""The portal's web app: a client side reached by a series' secret link, and an
editor side (admin page and the PyCapSlap app) behind one admin token.

Clients:  /s/<token>              the series' episodes
          /s/<token>/e/<id>       an episode's videos, caption check, feedback
Editor:   /admin                  series, links, feedback, reviewed captions
          /api/admin/...          the same for the desktop app (Bearer token)
"""

import hmac
import json
import os
import re
import sqlite3
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from fastapi import Depends, FastAPI, HTTPException, Request
from fastapi.responses import FileResponse, JSONResponse, PlainTextResponse, Response
from fastapi.staticfiles import StaticFiles
from pydantic import BaseModel, Field

from capslap_portal import db

STATIC = Path(__file__).parent / "static"
# The caption review page from this repo; the Docker image copies it next to the app.
DEFAULT_REVIEW_DIR = Path(__file__).resolve().parents[2] / "review"


@dataclass
class Settings:
    data_dir: Path
    admin_token: str
    review_dir: Path = DEFAULT_REVIEW_DIR
    max_upload_bytes: int = 8 * 1024**3

    @classmethod
    def from_env(cls) -> "Settings":
        token = os.environ.get("PORTAL_ADMIN_TOKEN", "")
        if len(token) < 16:
            raise SystemExit("Set PORTAL_ADMIN_TOKEN to a secret of 16+ characters.")
        return cls(
            data_dir=Path(os.environ.get("PORTAL_DATA", "data")),
            admin_token=token,
            review_dir=Path(os.environ.get("PORTAL_REVIEW_DIR", DEFAULT_REVIEW_DIR)),
            max_upload_bytes=int(os.environ.get("PORTAL_MAX_UPLOAD_MB", "8192"))
            * 1024**2,
        )


# ── Request bodies ───────────────────────────────────────────────────────────


class Title(BaseModel):
    title: str = Field(min_length=1, max_length=200)


class NewVideo(BaseModel):
    label: str = Field(min_length=1, max_length=200)
    filename: str = Field(default="video.mp4", max_length=300)
    captions: dict[str, Any] | None = None


class Publish(NewVideo):
    series: str = Field(min_length=1, max_length=200)
    episode: str = Field(min_length=1, max_length=200)


# The services a video can be posted to. Their limits live with the editor that
# checks them (static/platforms.js); the server only knows the names.
PLATFORMS = ("youtube", "tiktok", "instagram", "facebook", "linkedin", "x")


class PostText(BaseModel):
    title: str = Field(default="", max_length=1000)
    body: str = Field(default="", max_length=70_000)
    author: str = Field(default="", max_length=100)


class Approval(BaseModel):
    approved: bool
    author: str = Field(default="", max_length=100)


class Platforms(BaseModel):
    platforms: list[str]


class Feedback(BaseModel):
    text: str = Field(min_length=1, max_length=10_000)
    author: str = Field(default="", max_length=100)
    video_id: int | None = Field(default=None, alias="videoId")
    at_ms: int | None = Field(default=None, alias="atMs", ge=0)


# ── Helpers ──────────────────────────────────────────────────────────────────


def _row(row: sqlite3.Row | None, what: str) -> sqlite3.Row:
    if row is None:
        raise HTTPException(404, f"No such {what}.")
    return row


def _check_captions(data: Any) -> str:
    if not isinstance(data, dict) or not isinstance(data.get("segments"), list):
        raise HTTPException(422, "Not a captions file (no segments).")
    return json.dumps(data, ensure_ascii=False)


def _safe_name(text: str) -> str:
    name = re.sub(r"[^\w\-. ]+", "", text, flags=re.UNICODE).strip(" .")
    return name[:120] or "video"


def create_app(settings: Settings) -> FastAPI:
    settings.data_dir.mkdir(parents=True, exist_ok=True)
    videos_dir = settings.data_dir / "videos"
    videos_dir.mkdir(exist_ok=True)
    db_path = settings.data_dir / "portal.sqlite3"
    first = db.connect(db_path)  # creates the schema
    db.migrate(first)
    first.close()

    app = FastAPI(title="Videoiden hyväksyntä", docs_url=None, redoc_url=None)

    @app.middleware("http")
    async def private_headers(request: Request, call_next):
        response = await call_next(request)
        # The link is the key: keep it out of Referer headers and search engines.
        response.headers["Referrer-Policy"] = "no-referrer"
        response.headers["X-Robots-Tag"] = "noindex, nofollow"
        response.headers["X-Content-Type-Options"] = "nosniff"
        return response

    def conn():
        c = db.connect(db_path)
        try:
            yield c
        finally:
            c.close()

    def admin(request: Request) -> None:
        given = request.headers.get("authorization", "").removeprefix("Bearer ")
        if not hmac.compare_digest(given.encode(), settings.admin_token.encode()):
            raise HTTPException(401, "Wrong or missing admin token.")

    def series_by_token(token: str, c: sqlite3.Connection) -> sqlite3.Row:
        return _row(
            c.execute("SELECT * FROM series WHERE token=?", (token,)).fetchone(),
            "series",
        )

    def video_path(video_id: int) -> Path:
        return videos_dir / f"{video_id}.mp4"

    # ── What both sides see ───────────────────────────────────────────────

    def captions_of(c, video_id: int) -> list[dict]:
        rows = c.execute(
            "SELECT id, source, author, created_at FROM captions WHERE video_id=? ORDER BY id",
            (video_id,),
        ).fetchall()
        return [
            {
                "id": r["id"],
                "source": r["source"],
                "author": r["author"],
                "createdAt": r["created_at"],
            }
            for r in rows
        ]

    def video_json(c, v: sqlite3.Row, base: str) -> dict:
        versions = captions_of(c, v["id"])
        out = {
            "id": v["id"],
            "label": v["label"],
            "size": v["size"],
            "ready": bool(v["uploaded"]),
            "createdAt": v["created_at"],
            "fileUrl": f"{base}/videos/{v['id']}/file",
            "downloadUrl": f"{base}/videos/{v['id']}/file?download=1",
            "captionsUrl": f"{base}/videos/{v['id']}/captions" if versions else None,
            "reviews": [x for x in versions if x["source"] == "review"],
            "platforms": platforms_of(v),
            "posts": posts_of(c, v["id"]),
        }
        return out

    def platforms_of(v: sqlite3.Row) -> list[str]:
        chosen = json.loads(v["platforms"]) if v["platforms"] else list(PLATFORMS)
        return [p for p in PLATFORMS if p in chosen]

    def posts_of(c, video_id: int) -> dict[str, dict]:
        rows = c.execute("SELECT * FROM posts WHERE video_id=?", (video_id,))
        return {
            r["platform"]: {
                "title": r["title"],
                "body": r["body"],
                "updatedBy": r["updated_by"],
                "updatedAt": r["updated_at"],
                "approvedBy": r["approved_by"],
                "approvedAt": r["approved_at"],
            }
            for r in rows
        }

    def check_platform(platform: str) -> None:
        if platform not in PLATFORMS:
            raise HTTPException(404, f"No such service: {platform}.")

    def save_post(c, video_id: int, platform: str, text: PostText) -> dict:
        """Store a post text; a changed text needs approving again."""
        check_platform(platform)
        old = c.execute(
            "SELECT title, body FROM posts WHERE video_id=? AND platform=?",
            (video_id, platform),
        ).fetchone()
        changed = old is None or (old["title"], old["body"]) != (text.title, text.body)
        if old is None:
            c.execute(
                "INSERT INTO posts (video_id, platform) VALUES (?, ?)",
                (video_id, platform),
            )
        if changed:
            c.execute(
                """UPDATE posts SET title=?, body=?, updated_by=?, updated_at=?,
                   approved_by=NULL, approved_at=NULL
                   WHERE video_id=? AND platform=?""",
                (
                    text.title,
                    text.body,
                    text.author.strip(),
                    db.now(),
                    video_id,
                    platform,
                ),
            )
        return posts_of(c, video_id)[platform]

    def approve_post(c, video_id: int, platform: str, approval: Approval) -> dict:
        check_platform(platform)
        if (
            c.execute(
                "SELECT 1 FROM posts WHERE video_id=? AND platform=?",
                (video_id, platform),
            ).fetchone()
            is None
        ):
            raise HTTPException(409, "There is no text to approve yet.")
        c.execute(
            "UPDATE posts SET approved_by=?, approved_at=? WHERE video_id=? AND platform=?",
            (
                approval.author.strip() if approval.approved else None,
                db.now() if approval.approved else None,
                video_id,
                platform,
            ),
        )
        return posts_of(c, video_id)[platform]

    def feedback_of(c, episode_id: int) -> list[dict]:
        rows = c.execute(
            "SELECT * FROM feedback WHERE episode_id=? ORDER BY id", (episode_id,)
        ).fetchall()
        return [
            {
                "id": r["id"],
                "videoId": r["video_id"],
                "atMs": r["at_ms"],
                "author": r["author"],
                "text": r["text"],
                "createdAt": r["created_at"],
            }
            for r in rows
        ]

    def episode_json(c, e: sqlite3.Row, base: str) -> dict:
        videos = c.execute(
            "SELECT * FROM videos WHERE episode_id=? ORDER BY id", (e["id"],)
        ).fetchall()
        return {
            "id": e["id"],
            "title": e["title"],
            "createdAt": e["created_at"],
            "videos": [video_json(c, v, base) for v in videos],
            "feedback": feedback_of(c, e["id"]),
        }

    def episodes_summary(c, series_id: int) -> list[dict]:
        rows = c.execute(
            """SELECT e.id, e.title, e.created_at,
                      (SELECT COUNT(*) FROM videos v WHERE v.episode_id=e.id AND v.uploaded) AS videos,
                      (SELECT COUNT(*) FROM feedback f WHERE f.episode_id=e.id) AS feedback
               FROM episodes e WHERE e.series_id=? ORDER BY e.id DESC""",
            (series_id,),
        ).fetchall()
        return [
            {
                "id": r["id"],
                "title": r["title"],
                "createdAt": r["created_at"],
                "videos": r["videos"],
                "feedback": r["feedback"],
            }
            for r in rows
        ]

    def serve_video(v: sqlite3.Row, episode_title: str, download: bool) -> Response:
        path = video_path(v["id"])
        if not v["uploaded"] or not path.exists():
            raise HTTPException(404, "The video is not uploaded yet.")
        name = f"{_safe_name(episode_title)} - {_safe_name(v['label'])}.mp4"
        return FileResponse(
            path,
            media_type="video/mp4",
            filename=name,
            content_disposition_type="attachment" if download else "inline",
        )

    def latest_captions(c, video_id: int) -> sqlite3.Row:
        return _row(
            c.execute(
                "SELECT * FROM captions WHERE video_id=? ORDER BY id DESC LIMIT 1",
                (video_id,),
            ).fetchone(),
            "captions",
        )

    # ── Client API: the series token is the only key ──────────────────────

    def client_video(token: str, video_id: int, c) -> tuple[sqlite3.Row, sqlite3.Row]:
        s = series_by_token(token, c)
        v = _row(
            c.execute(
                """SELECT v.*, e.title AS episode_title FROM videos v
                   JOIN episodes e ON e.id=v.episode_id
                   WHERE v.id=? AND e.series_id=?""",
                (video_id, s["id"]),
            ).fetchone(),
            "video",
        )
        return s, v

    @app.get("/api/s/{token}")
    def client_series(token: str, c=Depends(conn)):
        s = series_by_token(token, c)
        return {"title": s["title"], "episodes": episodes_summary(c, s["id"])}

    @app.get("/api/s/{token}/episodes/{episode_id}")
    def client_episode(token: str, episode_id: int, c=Depends(conn)):
        s = series_by_token(token, c)
        e = _row(
            c.execute(
                "SELECT * FROM episodes WHERE id=? AND series_id=?",
                (episode_id, s["id"]),
            ).fetchone(),
            "episode",
        )
        out = episode_json(c, e, f"/api/s/{token}")
        out["videos"] = [v for v in out["videos"] if v["ready"]]
        out["series"] = s["title"]
        return out

    @app.get("/api/s/{token}/videos/{video_id}/file")
    def client_file(token: str, video_id: int, download: bool = False, c=Depends(conn)):
        _, v = client_video(token, video_id, c)
        return serve_video(v, v["episode_title"], download)

    @app.get("/api/s/{token}/videos/{video_id}/captions")
    def client_captions(token: str, video_id: int, c=Depends(conn)):
        client_video(token, video_id, c)
        return Response(
            latest_captions(c, video_id)["body"], media_type="application/json"
        )

    @app.post("/api/s/{token}/videos/{video_id}/review")
    async def client_review(
        token: str, video_id: int, request: Request, c=Depends(conn)
    ):
        client_video(token, video_id, c)
        try:
            data = json.loads(await request.body())
        except ValueError as err:
            raise HTTPException(422, "Not JSON.") from err
        body = _check_captions(data)
        current = json.loads(latest_captions(c, video_id)["body"])
        if len(data["segments"]) != len(current.get("segments", [])):
            raise HTTPException(
                409, "These captions were made from a different version."
            )
        author = str((data.get("review") or {}).get("reviewer", ""))[:100]
        c.execute(
            "INSERT INTO captions (video_id, source, author, body, created_at) VALUES (?,?,?,?,?)",
            (video_id, "review", author, body, db.now()),
        )
        return {"ok": True}

    @app.put("/api/s/{token}/videos/{video_id}/posts/{platform}")
    def client_post(
        token: str, video_id: int, platform: str, text: PostText, c=Depends(conn)
    ):
        _, v = client_video(token, video_id, c)
        if platform not in platforms_of(v):
            raise HTTPException(404, "This video doesn't go to that service.")
        return save_post(c, video_id, platform, text)

    @app.post("/api/s/{token}/videos/{video_id}/posts/{platform}/approval")
    def client_approve(
        token: str, video_id: int, platform: str, approval: Approval, c=Depends(conn)
    ):
        client_video(token, video_id, c)
        return approve_post(c, video_id, platform, approval)

    @app.post("/api/s/{token}/episodes/{episode_id}/feedback")
    def client_feedback(token: str, episode_id: int, fb: Feedback, c=Depends(conn)):
        s = series_by_token(token, c)
        _row(
            c.execute(
                "SELECT id FROM episodes WHERE id=? AND series_id=?",
                (episode_id, s["id"]),
            ).fetchone(),
            "episode",
        )
        if fb.video_id is not None:
            _row(
                c.execute(
                    "SELECT id FROM videos WHERE id=? AND episode_id=?",
                    (fb.video_id, episode_id),
                ).fetchone(),
                "video",
            )
        cur = c.execute(
            "INSERT INTO feedback (episode_id, video_id, at_ms, author, text, created_at) VALUES (?,?,?,?,?,?)",
            (
                episode_id,
                fb.video_id,
                fb.at_ms,
                fb.author.strip(),
                fb.text.strip(),
                db.now(),
            ),
        )
        return {"id": cur.lastrowid}

    # ── Editor API ────────────────────────────────────────────────────────

    A = [Depends(admin)]

    def series_json(c, s: sqlite3.Row) -> dict:
        return {
            "id": s["id"],
            "title": s["title"],
            "token": s["token"],
            "link": f"/s/{s['token']}",
            "createdAt": s["created_at"],
        }

    def create_series(c, title: str) -> sqlite3.Row:
        cur = c.execute(
            "INSERT INTO series (title, token, created_at) VALUES (?,?,?)",
            (title.strip(), db.new_token(), db.now()),
        )
        return c.execute("SELECT * FROM series WHERE id=?", (cur.lastrowid,)).fetchone()

    def create_episode(c, series_id: int, title: str) -> sqlite3.Row:
        cur = c.execute(
            "INSERT INTO episodes (series_id, title, created_at) VALUES (?,?,?)",
            (series_id, title.strip(), db.now()),
        )
        return c.execute(
            "SELECT * FROM episodes WHERE id=?", (cur.lastrowid,)
        ).fetchone()

    def create_video(c, episode_id: int, body: NewVideo) -> sqlite3.Row:
        cur = c.execute(
            "INSERT INTO videos (episode_id, label, filename, created_at) VALUES (?,?,?,?)",
            (episode_id, body.label.strip(), body.filename, db.now()),
        )
        if body.captions is not None:
            c.execute(
                "INSERT INTO captions (video_id, source, body, created_at) VALUES (?,?,?,?)",
                (cur.lastrowid, "editor", _check_captions(body.captions), db.now()),
            )
        return c.execute("SELECT * FROM videos WHERE id=?", (cur.lastrowid,)).fetchone()

    def admin_video_json(c, v) -> dict:
        out = video_json(c, v, "/api/admin")
        out["uploadUrl"] = f"/api/admin/videos/{v['id']}/file"
        out["captions"] = captions_of(c, v["id"])
        return out

    @app.get("/api/admin/series", dependencies=A)
    def admin_series(c=Depends(conn)):
        rows = c.execute(
            "SELECT * FROM series ORDER BY title COLLATE NOCASE"
        ).fetchall()
        return [
            series_json(c, s) | {"episodes": episodes_summary(c, s["id"])} for s in rows
        ]

    @app.post("/api/admin/series", dependencies=A)
    def admin_new_series(body: Title, c=Depends(conn)):
        return series_json(c, create_series(c, body.title))

    @app.get("/api/admin/series/{series_id}", dependencies=A)
    def admin_one_series(series_id: int, c=Depends(conn)):
        s = _row(
            c.execute("SELECT * FROM series WHERE id=?", (series_id,)).fetchone(),
            "series",
        )
        episodes = c.execute(
            "SELECT * FROM episodes WHERE series_id=? ORDER BY id DESC", (series_id,)
        ).fetchall()
        out = series_json(c, s)
        out["episodes"] = []
        for e in episodes:
            ej = episode_json(c, e, "/api/admin")
            ej["videos"] = [
                admin_video_json(c, v)
                for v in c.execute(
                    "SELECT * FROM videos WHERE episode_id=? ORDER BY id", (e["id"],)
                )
            ]
            out["episodes"].append(ej)
        return out

    @app.patch("/api/admin/series/{series_id}", dependencies=A)
    def admin_rename_series(series_id: int, body: Title, c=Depends(conn)):
        c.execute(
            "UPDATE series SET title=? WHERE id=?", (body.title.strip(), series_id)
        )
        return admin_one_series(series_id, c)

    @app.post("/api/admin/series/{series_id}/rotate", dependencies=A)
    def admin_rotate(series_id: int, c=Depends(conn)):
        """A new secret link; the old one stops working."""
        _row(
            c.execute("SELECT id FROM series WHERE id=?", (series_id,)).fetchone(),
            "series",
        )
        c.execute("UPDATE series SET token=? WHERE id=?", (db.new_token(), series_id))
        return series_json(
            c, c.execute("SELECT * FROM series WHERE id=?", (series_id,)).fetchone()
        )

    def remove_files(c, where: str, arg: int) -> None:
        for (vid,) in c.execute(
            f"SELECT v.id FROM videos v JOIN episodes e ON e.id=v.episode_id WHERE {where}",
            (arg,),
        ):
            video_path(vid).unlink(missing_ok=True)

    @app.delete("/api/admin/series/{series_id}", dependencies=A)
    def admin_delete_series(series_id: int, c=Depends(conn)):
        remove_files(c, "e.series_id=?", series_id)
        c.execute("DELETE FROM series WHERE id=?", (series_id,))
        return {"ok": True}

    @app.post("/api/admin/series/{series_id}/episodes", dependencies=A)
    def admin_new_episode(series_id: int, body: Title, c=Depends(conn)):
        _row(
            c.execute("SELECT id FROM series WHERE id=?", (series_id,)).fetchone(),
            "series",
        )
        return episode_json(c, create_episode(c, series_id, body.title), "/api/admin")

    @app.patch("/api/admin/episodes/{episode_id}", dependencies=A)
    def admin_rename_episode(episode_id: int, body: Title, c=Depends(conn)):
        c.execute(
            "UPDATE episodes SET title=? WHERE id=?", (body.title.strip(), episode_id)
        )
        return {"ok": True}

    @app.delete("/api/admin/episodes/{episode_id}", dependencies=A)
    def admin_delete_episode(episode_id: int, c=Depends(conn)):
        remove_files(c, "e.id=?", episode_id)
        c.execute("DELETE FROM episodes WHERE id=?", (episode_id,))
        return {"ok": True}

    @app.post("/api/admin/episodes/{episode_id}/videos", dependencies=A)
    def admin_new_video(episode_id: int, body: NewVideo, c=Depends(conn)):
        _row(
            c.execute("SELECT id FROM episodes WHERE id=?", (episode_id,)).fetchone(),
            "episode",
        )
        return admin_video_json(c, create_video(c, episode_id, body))

    @app.post("/api/admin/publish", dependencies=A)
    def admin_publish(body: Publish, c=Depends(conn)):
        """One call for the desktop app: series and episode by title, made when new."""
        s = c.execute(
            "SELECT * FROM series WHERE title=? COLLATE NOCASE", (body.series.strip(),)
        ).fetchone() or create_series(c, body.series)
        e = c.execute(
            "SELECT * FROM episodes WHERE series_id=? AND title=? COLLATE NOCASE",
            (s["id"], body.episode.strip()),
        ).fetchone() or create_episode(c, s["id"], body.episode)
        v = create_video(c, e["id"], body)
        return {
            "video": admin_video_json(c, v),
            "seriesLink": f"/s/{s['token']}",
            "episodeLink": f"/s/{s['token']}/e/{e['id']}",
        }

    @app.put("/api/admin/videos/{video_id}/posts/{platform}", dependencies=A)
    def admin_post(video_id: int, platform: str, text: PostText, c=Depends(conn)):
        _row(
            c.execute("SELECT id FROM videos WHERE id=?", (video_id,)).fetchone(),
            "video",
        )
        return save_post(c, video_id, platform, text)

    @app.post("/api/admin/videos/{video_id}/posts/{platform}/approval", dependencies=A)
    def admin_approve(
        video_id: int, platform: str, approval: Approval, c=Depends(conn)
    ):
        _row(
            c.execute("SELECT id FROM videos WHERE id=?", (video_id,)).fetchone(),
            "video",
        )
        return approve_post(c, video_id, platform, approval)

    @app.put("/api/admin/videos/{video_id}/platforms", dependencies=A)
    def admin_platforms(video_id: int, body: Platforms, c=Depends(conn)):
        """Which services the video goes to; the others' texts are kept, just hidden."""
        _row(
            c.execute("SELECT id FROM videos WHERE id=?", (video_id,)).fetchone(),
            "video",
        )
        unknown = [p for p in body.platforms if p not in PLATFORMS]
        if unknown:
            raise HTTPException(422, f"No such service: {', '.join(unknown)}.")
        c.execute(
            "UPDATE videos SET platforms=? WHERE id=?",
            (json.dumps([p for p in PLATFORMS if p in body.platforms]), video_id),
        )
        v = c.execute("SELECT * FROM videos WHERE id=?", (video_id,)).fetchone()
        return {"platforms": platforms_of(v)}

    @app.put("/api/admin/videos/{video_id}/file", dependencies=A)
    async def admin_upload(video_id: int, request: Request, c=Depends(conn)):
        _row(
            c.execute("SELECT id FROM videos WHERE id=?", (video_id,)).fetchone(),
            "video",
        )
        final = video_path(video_id)
        partial = final.with_suffix(".part")
        size = 0
        try:
            with partial.open("wb") as out:
                async for chunk in request.stream():
                    size += len(chunk)
                    if size > settings.max_upload_bytes:
                        raise HTTPException(413, "The video is too big.")
                    out.write(chunk)
        except BaseException:
            partial.unlink(missing_ok=True)
            raise
        if not size:
            partial.unlink(missing_ok=True)
            raise HTTPException(422, "Empty upload.")
        partial.replace(final)
        c.execute("UPDATE videos SET size=?, uploaded=1 WHERE id=?", (size, video_id))
        return {"ok": True, "size": size}

    @app.get("/api/admin/videos/{video_id}/file", dependencies=A)
    def admin_file(video_id: int, download: bool = False, c=Depends(conn)):
        v = _row(
            c.execute(
                "SELECT v.*, e.title AS episode_title FROM videos v JOIN episodes e ON e.id=v.episode_id WHERE v.id=?",
                (video_id,),
            ).fetchone(),
            "video",
        )
        return serve_video(v, v["episode_title"], download)

    @app.put("/api/admin/videos/{video_id}/captions", dependencies=A)
    async def admin_put_captions(video_id: int, request: Request, c=Depends(conn)):
        _row(
            c.execute("SELECT id FROM videos WHERE id=?", (video_id,)).fetchone(),
            "video",
        )
        try:
            data = json.loads(await request.body())
        except ValueError as err:
            raise HTTPException(422, "Not JSON.") from err
        c.execute(
            "INSERT INTO captions (video_id, source, body, created_at) VALUES (?,?,?,?)",
            (video_id, "editor", _check_captions(data), db.now()),
        )
        return {"ok": True}

    @app.get("/api/admin/videos/{video_id}/captions", dependencies=A)
    def admin_get_captions(video_id: int, version: int | None = None, c=Depends(conn)):
        """The newest captions (or one version) as a file PyCapSlap imports as is."""
        v = _row(
            c.execute("SELECT * FROM videos WHERE id=?", (video_id,)).fetchone(),
            "video",
        )
        if version is None:
            row = latest_captions(c, video_id)
        else:
            row = _row(
                c.execute(
                    "SELECT * FROM captions WHERE id=? AND video_id=?",
                    (version, video_id),
                ).fetchone(),
                "captions version",
            )
        name = Path(v["filename"]).name or "video.mp4"
        kind = "reviewed" if row["source"] == "review" else "review"
        return Response(
            row["body"],
            media_type="application/json",
            headers={
                "Content-Disposition": f'attachment; filename="{_safe_name(name)}.{kind}.capslap.json"'
            },
        )

    @app.delete("/api/admin/videos/{video_id}", dependencies=A)
    def admin_delete_video(video_id: int, c=Depends(conn)):
        video_path(video_id).unlink(missing_ok=True)
        c.execute("DELETE FROM videos WHERE id=?", (video_id,))
        return {"ok": True}

    @app.delete("/api/admin/feedback/{feedback_id}", dependencies=A)
    def admin_delete_feedback(feedback_id: int, c=Depends(conn)):
        c.execute("DELETE FROM feedback WHERE id=?", (feedback_id,))
        return {"ok": True}

    # ── Pages ─────────────────────────────────────────────────────────────

    @app.get("/", include_in_schema=False)
    def home():
        return PlainTextResponse(
            "Videoiden hyväksyntä. Käytä linkkiä, jonka sait. / Use the link you were sent."
        )

    @app.get("/s/{token}", include_in_schema=False)
    @app.get("/s/{token}/e/{episode_id}", include_in_schema=False)
    def client_page(token: str, episode_id: int | None = None, c=Depends(conn)):
        if c.execute("SELECT 1 FROM series WHERE token=?", (token,)).fetchone() is None:
            return PlainTextResponse(
                "This link doesn't work (any more). Ask for a new one.", 404
            )
        return FileResponse(STATIC / "client.html")

    @app.get("/admin", include_in_schema=False)
    def admin_page():
        return FileResponse(STATIC / "admin.html")

    @app.exception_handler(HTTPException)
    async def errors(_request: Request, exc: HTTPException):
        return JSONResponse({"error": exc.detail}, status_code=exc.status_code)

    app.mount("/static", StaticFiles(directory=STATIC), name="static")
    if settings.review_dir.is_dir():
        app.mount(
            "/review",
            StaticFiles(directory=settings.review_dir, html=True),
            name="review",
        )
    return app
