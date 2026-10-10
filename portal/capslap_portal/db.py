"""SQLite storage. One file, created on first start; videos live next to it."""

import secrets
import sqlite3
import time
from pathlib import Path

SCHEMA = """
CREATE TABLE IF NOT EXISTS series (
    id INTEGER PRIMARY KEY,
    title TEXT NOT NULL,
    token TEXT NOT NULL UNIQUE,
    created_at REAL NOT NULL
);
CREATE TABLE IF NOT EXISTS episodes (
    id INTEGER PRIMARY KEY,
    series_id INTEGER NOT NULL REFERENCES series(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    created_at REAL NOT NULL
);
CREATE TABLE IF NOT EXISTS videos (
    id INTEGER PRIMARY KEY,
    episode_id INTEGER NOT NULL REFERENCES episodes(id) ON DELETE CASCADE,
    label TEXT NOT NULL,
    filename TEXT NOT NULL,
    size INTEGER NOT NULL DEFAULT 0,
    uploaded INTEGER NOT NULL DEFAULT 0,
    created_at REAL NOT NULL
);
-- Every captions file a video has had: the editor's (source 'editor') and each
-- reviewer's corrections ('review'). The newest one is the video's captions.
CREATE TABLE IF NOT EXISTS captions (
    id INTEGER PRIMARY KEY,
    video_id INTEGER NOT NULL REFERENCES videos(id) ON DELETE CASCADE,
    source TEXT NOT NULL,
    author TEXT NOT NULL DEFAULT '',
    body TEXT NOT NULL,
    created_at REAL NOT NULL
);
-- What goes out with a video on each service: a title where the service has one,
-- the post text, who last changed it and who approved it.
CREATE TABLE IF NOT EXISTS posts (
    video_id INTEGER NOT NULL REFERENCES videos(id) ON DELETE CASCADE,
    platform TEXT NOT NULL,
    title TEXT NOT NULL DEFAULT '',
    body TEXT NOT NULL DEFAULT '',
    updated_by TEXT NOT NULL DEFAULT '',
    updated_at REAL,
    approved_by TEXT,
    approved_at REAL,
    PRIMARY KEY (video_id, platform)
);
CREATE TABLE IF NOT EXISTS feedback (
    id INTEGER PRIMARY KEY,
    episode_id INTEGER NOT NULL REFERENCES episodes(id) ON DELETE CASCADE,
    video_id INTEGER REFERENCES videos(id) ON DELETE CASCADE,
    at_ms INTEGER,
    author TEXT NOT NULL DEFAULT '',
    text TEXT NOT NULL,
    created_at REAL NOT NULL
);
"""


def new_token() -> str:
    """A series link's secret: 128 random bits, URL-safe."""
    return secrets.token_urlsafe(16)


def connect(path: Path) -> sqlite3.Connection:
    conn = sqlite3.connect(path, check_same_thread=False, isolation_level=None)
    conn.row_factory = sqlite3.Row
    conn.execute("PRAGMA foreign_keys = ON")
    conn.execute("PRAGMA journal_mode = WAL")
    conn.executescript(SCHEMA)
    return conn


def migrate(conn: sqlite3.Connection) -> None:
    """Bring a database made by an older portal up to date."""
    columns = {row["name"] for row in conn.execute("PRAGMA table_info(videos)")}
    if "platforms" not in columns:
        # JSON list of the services the video goes to; NULL means all of them.
        conn.execute("ALTER TABLE videos ADD COLUMN platforms TEXT")


def now() -> float:
    return time.time()
