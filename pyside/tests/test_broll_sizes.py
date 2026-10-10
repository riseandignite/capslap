"""Footage whose size is not known until it is downloaded (NASA, the Internet
Archive): the reel reads it from the file and lays the shot out by it."""

import shutil
import subprocess

import pytest

from app.services import media
from app.services.broll import looks, pipeline, reel
from app.services.broll import picks as picks_mod
from app.services.broll.assets import Asset

FFMPEG = shutil.which("ffmpeg")
needs_ffmpeg = pytest.mark.skipif(
    FFMPEG is None or shutil.which("ffprobe") is None, reason="needs ffmpeg"
)


def run(*a):
    subprocess.run([FFMPEG, "-v", "error", "-y", *a], check=True)


def clip(tmp_path, name, size, seconds=1):
    out = tmp_path / name
    run(
        "-f", "lavfi", "-i", f"testsrc=s={size}:d={seconds}", "-c:v", "libvpx", str(out)
    )
    return out


def asset(title, url=None, kind="video", w=0, h=0):
    return Asset(
        title=title,
        kind=kind,
        url=url or f"https://x/{title}",
        page_url=f"p/{title}",
        license="Public domain",
        width=w,
        height=h,
    )


def one_window_each(*assets):
    props = []
    for i, a in enumerate(assets):
        props.append(
            {
                "startMs": i * 3000,
                "endMs": (i + 1) * 3000,
                "text": "t",
                "query": "q",
                "candidates": [a.to_dict()],
                "chosen": None,
            }
        )
    p = picks_mod.Picks(props)
    for i in range(len(assets)):
        p.accept(i, 0)
    return p


# --- reading the size from the file -------------------------------------------


@needs_ffmpeg
def test_media_size_reads_a_clip_and_a_picture(tmp_path):
    png = tmp_path / "a.png"
    run("-f", "lavfi", "-i", "color=c=red:s=64x48", "-frames:v", "1", str(png))
    assert media.size(clip(tmp_path, "c.webm", "320x240")) == (320, 240)
    assert media.size(png) == (64, 48)


def test_media_size_of_something_unreadable_is_none(tmp_path):
    junk = tmp_path / "junk.mp4"
    junk.write_bytes(b"not a video")
    assert media.size(junk) is None
    assert media.size(tmp_path / "missing.mp4") is None


# --- what the layout does with an unknown size --------------------------------


def test_an_unknown_size_is_fitted_so_nothing_is_cropped_away():
    assert looks.auto_layout(None) == "blur"
    assert looks.auto_layout((0, 0)) == "blur"


# --- shots: sizes from outside, files by URL ----------------------------------


def test_a_size_read_from_the_file_decides_the_layout(tmp_path):
    p = one_window_each(asset("Old film"), asset("Phone video"))
    files = {a.url: tmp_path / a.title for ch in p.accepted() for a in ch.assets}
    sizes = {"https://x/Old film": (320, 240), "https://x/Phone video": (270, 480)}
    wide, tall = reel.shots(p.accepted(), files, "cut", sizes)
    assert (wide.layout, wide.size) == ("blur", (320, 240))
    assert (tall.layout, tall.size) == ("fill", (270, 480))


def test_a_size_the_source_declared_is_kept_when_none_is_read(tmp_path):
    p = one_window_each(asset("Declared", w=1080, h=1920))
    (ch,) = p.accepted()
    (s,) = reel.shots([ch], {ch.assets[0].url: tmp_path / "d"}, "cut", {})
    assert (s.layout, s.size) == ("fill", (1080, 1920))


def test_two_pictures_with_the_same_title_do_not_share_a_file(tmp_path):
    a = asset("Mars", url="https://nasa/Mars")
    b = asset("Mars", url="https://commons/Mars")
    p = one_window_each(a, b)
    files = {a.url: tmp_path / "from_nasa.mp4", b.url: tmp_path / "from_commons.mp4"}
    first, second = reel.shots(p.accepted(), files, "cut")
    assert first.path.name == "from_nasa.mp4" and second.path.name == "from_commons.mp4"


# --- the pipeline reads the sizes after the download --------------------------


@needs_ffmpeg
def test_build_reads_the_size_of_footage_that_arrived_without_one(tmp_path):
    old = clip(tmp_path, "old.webm", "320x240")
    phone = clip(tmp_path, "phone.webm", "270x480")
    files = {"https://x/Old film": old, "https://x/Phone video": phone}
    p = one_window_each(asset("Old film"), asset("Phone video"))
    seen = {}

    def render(audio, shots, out, work, **kw):
        seen["shots"] = shots
        out.write_bytes(b"mp4")
        return out

    pipeline.build(
        tmp_path / "ep.m4a",
        p,
        tmp_path / "ep.reel.mp4",
        fetch=lambda a, folder: files[a.url],
        render=render,
    )
    wide, tall = seen["shots"]
    assert (wide.size, wide.layout) == ((320, 240), "blur")
    assert (tall.size, tall.layout) == ((270, 480), "fill")


def test_build_does_not_look_at_files_whose_size_is_known(tmp_path, monkeypatch):
    p = one_window_each(asset("Declared", w=1920, h=1080))
    looked = []
    monkeypatch.setattr(pipeline.media, "size", lambda path: looked.append(path))
    pipeline.build(
        tmp_path / "ep.m4a",
        p,
        tmp_path / "o.mp4",
        fetch=lambda a, folder: tmp_path / "d.mp4",
        render=lambda *a, **k: (tmp_path / "o.mp4").write_bytes(b"x"),
    )
    assert looked == []


@needs_ffmpeg
def test_an_old_four_by_three_film_is_fitted_not_cropped_in_the_finished_reel(tmp_path):
    # Left third red, middle white, right third blue: a crop would lose the red.
    film = tmp_path / "film.webm"
    run(
        "-f",
        "lavfi",
        "-i",
        "color=c=white:s=320x240:d=2",
        "-vf",
        "drawbox=x=0:y=0:w=107:h=240:color=red:t=fill,"
        "drawbox=x=213:y=0:w=107:h=240:color=blue:t=fill",
        "-c:v",
        "libvpx",
        str(film),
    )
    tone = tmp_path / "ep.m4a"
    run("-f", "lavfi", "-i", "sine=frequency=440:duration=2", str(tone))
    p = one_window_each(asset("Film"))
    out = tmp_path / "ep.reel.mp4"
    pipeline.build(
        tone,
        p,
        out,
        fetch=lambda a, folder: film,
        size=(180, 320),
        end_card_s=0,
        transition="cut",
    )
    raw = subprocess.run(
        [
            FFMPEG,
            "-v",
            "error",
            "-ss",
            "1.0",
            "-i",
            str(out),
            "-frames:v",
            "1",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgb24",
            "-",
        ],
        capture_output=True,
        check=True,
    ).stdout
    px = lambda x, y: tuple(raw[(y * 180 + x) * 3 : (y * 180 + x) * 3 + 3])  # noqa: E731
    left_edge = px(4, 160)  # the middle row, at the very left of the frame
    assert left_edge[0] > 150 and left_edge[1] < 90, left_edge  # red is there: fitted
    top = px(90, 6)
    assert sum(top) > 90, top  # a blurred copy fills above the picture
