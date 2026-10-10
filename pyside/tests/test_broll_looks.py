import shutil
import subprocess
from pathlib import Path

import pytest

from app.services.broll import looks, reel
from app.services.broll.picks import Picks
from app.services.broll.assets import Asset

FFMPEG = shutil.which("ffmpeg")
needs_ffmpeg = pytest.mark.skipif(FFMPEG is None, reason="needs ffmpeg")


# --- the pure choices --------------------------------------------------------


def test_motions_cycle_so_neighbouring_shots_differ():
    seen = [looks.auto_motion(i) for i in range(len(looks.MOTIONS))]
    assert len(set(seen)) == len(looks.MOTIONS)
    assert looks.auto_motion(0) == looks.auto_motion(len(looks.MOTIONS))
    assert all(looks.auto_motion(i) != looks.auto_motion(i + 1) for i in range(12))


def test_every_motion_has_an_opposite_that_is_another_motion():
    for m in looks.MOTIONS:
        o = looks.opposite(m)
        assert o in looks.MOTIONS and o != m and looks.opposite(o) == m


def test_a_tall_picture_fills_the_frame_and_a_wide_or_square_one_is_fitted():
    assert looks.auto_layout((1080, 1920)) == "fill"
    assert looks.auto_layout((900, 1200)) == "fill"  # 3:4 loses a quarter, no more
    assert looks.auto_layout((1000, 1000)) == "blur"
    assert looks.auto_layout((1920, 1080)) == "blur"
    assert looks.auto_layout(None) == "blur"  # unknown: fit it, lose nothing


def test_zoompan_reaches_its_target_over_the_whole_shot():
    # 60 frames: the last one (on=59) is progress 1, whatever the length.
    zi = looks.zoompan("zoom_in", 60, 270, 480)
    assert "z='1+0.1500*min(on/59,1)'" in zi and "s=270x480" in zi
    zo = looks.zoompan("zoom_out", 60, 270, 480)
    assert "z='1.1500-0.1500*min(on/59,1)'" in zo
    pr = looks.zoompan("pan_right", 60, 270, 480)
    assert "z='1.2'" in pr and "x='(iw-iw/zoom)*min(on/59,1)'" in pr
    pl = looks.zoompan("pan_left", 60, 270, 480)
    assert "x='(iw-iw/zoom)*(1-min(on/59,1))'" in pl


def test_a_one_frame_shot_does_not_divide_by_zero():
    assert "on/1" in looks.zoompan("zoom_in", 1, 270, 480)


def test_every_motion_makes_a_filter():
    for m in looks.MOTIONS:
        assert looks.zoompan(m, 30, 270, 480).startswith("zoompan=")


# --- transitions -------------------------------------------------------------


def shot(a, b, name="a.jpg", **kw):
    return reel.Shot(a, b, Path(name), "image", f"credit {name}", **kw)


def test_the_first_shot_arrives_by_the_chosen_style_and_a_cut_style_is_all_cuts():
    s = reel.assign_transitions([shot(0, 4000), shot(4000, 8000)], "cut")
    assert [x.transition for x in s] == ["cut", "cut"]
    s = reel.assign_transitions([shot(0, 4000), shot(4000, 8000)], "fade")
    assert [x.transition for x in s] == ["fade", "fade"]


def test_mixed_style_varies_the_arrivals():
    shots = [shot(i * 4000, (i + 1) * 4000) for i in range(8)]
    kinds = [x.transition for x in reel.assign_transitions(shots, "mixed")]
    assert {"fade", "whip", "slide"} <= set(kinds)
    assert all(a != b for a, b in zip(kinds, kinds[1:]))


def test_a_transition_is_shorter_than_the_shots_it_joins():
    a, b = shot(0, 600), shot(600, 5000)
    assert reel.transition_seconds("fade", a, b) <= 0.4 * 0.6 + 1e-9
    assert reel.transition_seconds("whip", shot(0, 4000), shot(4000, 8000)) == 0.25
    assert reel.transition_seconds("cut", a, b) == 0.0


def test_only_adjacent_shots_overlap():
    assert reel.adjacent(shot(0, 4000), shot(4000, 8000))
    assert reel.adjacent(shot(0, 4000), shot(4030, 8000))  # a hair apart
    assert not reel.adjacent(shot(0, 4000), shot(6000, 8000))


def test_an_incoming_whip_slides_in_from_the_right_and_blurs():
    shots = reel.assign_transitions([shot(0, 4000), shot(4000, 8000)], "whip")
    expr = reel.overlay_x(shots, 1, width=1080)
    assert "1080*(1-" in expr and "clip((t-4)/0.25,0,1)" in expr
    assert reel.blur_enable(shots, 1) is not None


def test_an_outgoing_whip_leaves_to_the_left():
    shots = reel.assign_transitions([shot(0, 4000), shot(4000, 8000)], "whip")
    expr = reel.overlay_x(shots, 0, width=1080)
    assert "-1080*" in expr and "gte(t,4)" in expr


def test_a_slide_covers_the_old_shot_without_moving_it():
    shots = reel.assign_transitions([shot(0, 4000), shot(4000, 8000)], "slide")
    assert "gte" not in reel.overlay_x(
        shots, 0, width=1080
    )  # it is covered, not pushed
    assert "1080*(1-" in reel.overlay_x(shots, 1, width=1080)
    assert reel.blur_enable(shots, 1) is None


def test_the_old_shot_stays_through_the_transition_so_there_is_something_to_fade_over():
    shots = reel.assign_transitions([shot(0, 4000), shot(4000, 8000)], "fade")
    assert reel.extension(shots, 0) == 0.4
    assert reel.extension(shots, 1) == 0.0  # last shot, nothing follows
    assert (
        reel.extension(
            reel.assign_transitions([shot(0, 4000), shot(4000, 8000)], "cut"), 0
        )
        == 0.0
    )
    gap = reel.assign_transitions([shot(0, 3000), shot(5000, 8000)], "fade")
    assert reel.extension(gap, 0) == 0.0


# --- choosing and picking -----------------------------------------------------


def asset(title, w=1920, h=1080, kind="image"):
    return Asset(
        title=title,
        kind=kind,
        url=f"u/{title}",
        page_url=f"p/{title}",
        license="CC0",
        width=w,
        height=h,
    )


def proposals():
    return [
        {
            "startMs": 0,
            "endMs": 4000,
            "text": "t",
            "query": "q",
            "chosen": None,
            "candidates": [
                asset("wide.jpg").to_dict(),
                asset("tall.jpg", 900, 1600).to_dict(),
                asset("sq.jpg", 800, 800).to_dict(),
            ],
        },
        {
            "startMs": 4000,
            "endMs": 8000,
            "text": "u",
            "query": "r",
            "chosen": None,
            "candidates": [asset("tall.jpg", 900, 1600).to_dict()],
        },
    ]


def test_a_stack_takes_a_second_candidate():
    p = Picks(proposals())
    p.accept(0, 0, also=2)
    (choice,) = p.accepted()
    assert [a.title for a in choice.assets] == ["wide.jpg", "sq.jpg"]
    assert choice.layout == "stack"


def test_the_second_candidate_has_to_exist_and_differ():
    p = Picks(proposals())
    with pytest.raises(IndexError):
        p.accept(0, 0, also=9)
    with pytest.raises(ValueError):
        p.accept(0, 1, also=1)


def test_a_layout_can_be_forced_and_otherwise_follows_the_picture():
    p = Picks(proposals())
    p.accept(0, 0)  # wide
    p.accept(1, 0)  # tall
    wide, tall = p.accepted()
    assert wide.layout is None and tall.layout is None  # decided when the reel is built
    q = Picks(proposals())
    q.accept(0, 1, layout="blur")
    assert q.accepted()[0].layout == "blur"


def test_choices_survive_a_round_trip_through_json():
    p = Picks(proposals())
    p.accept(0, 0, also=1)
    q = Picks(p.data())
    assert [a.title for a in q.accepted()[0].assets] == ["wide.jpg", "tall.jpg"]


def test_shots_pick_layout_from_the_picture_and_cycle_the_motion(tmp_path):
    p = Picks(proposals())
    p.accept(0, 0)  # wide image: blurred fit
    p.accept(1, 0)  # tall image: fill
    files = {"u/wide.jpg": tmp_path / "w.jpg", "u/tall.jpg": tmp_path / "t.jpg"}
    a, b = reel.shots(p.accepted(), files, transition="fade")
    assert (a.layout, b.layout) == ("blur", "fill")
    assert a.motion != b.motion
    assert (a.transition, b.transition) == ("fade", "fade")


def test_a_stack_shot_has_two_parts_two_motions_and_both_credits(tmp_path):
    p = Picks(proposals())
    p.accept(0, 0, also=2)
    files = {"u/wide.jpg": tmp_path / "w.jpg", "u/sq.jpg": tmp_path / "s.jpg"}
    (s,) = reel.shots(p.accepted(), files, transition="cut")
    assert s.layout == "stack" and s.second is not None
    assert s.second.motion == looks.opposite(s.motion)
    assert len(s.credit_lines) == 2
    assert "wide.jpg" in s.credit_lines[0] and "sq.jpg" in s.credit_lines[1]


# --- the picture itself ------------------------------------------------------


def run(*a):
    subprocess.run([FFMPEG, "-v", "error", "-y", *a], check=True)


def solid(tmp_path, name, color, size="640x360"):
    out = tmp_path / name
    run("-f", "lavfi", "-i", f"color=c={color}:s={size}", "-frames:v", "1", str(out))
    return out


def tone(tmp_path, seconds):
    out = tmp_path / "ep.m4a"
    run("-f", "lavfi", "-i", f"sine=frequency=440:duration={seconds}", str(out))
    return out


def pixel(video, t, x, y, size=(180, 320)):
    """The RGB of one pixel of the frame at time t."""
    raw = subprocess.run(
        [
            FFMPEG,
            "-v",
            "error",
            "-ss",
            str(t),
            "-i",
            str(video),
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
    assert len(raw) == size[0] * size[1] * 3, "no frame at that time"
    i = (y * size[0] + x) * 3
    return tuple(raw[i : i + 3])


def reddish(c):
    return c[0] > 150 and c[1] < 90 and c[2] < 90


def bluish(c):
    return c[2] > 150 and c[0] < 90 and c[1] < 90


def render(tmp_path, shots, seconds, size=(180, 320), end_card_s=0):
    out = tmp_path / "reel.mp4"
    reel.render(
        tone(tmp_path, seconds),
        shots,
        out,
        work=tmp_path / "w",
        size=size,
        end_card_s=end_card_s,
        hw=False,
    )
    return out


@needs_ffmpeg
def test_a_stack_puts_one_picture_over_the_other(tmp_path):
    red = solid(tmp_path, "r.png", "red")
    blue = solid(tmp_path, "b.png", "blue")
    s = reel.Shot(
        0,
        2000,
        red,
        "image",
        "R",
        layout="stack",
        second=reel.Part(blue, "image", "B", None, "zoom_out"),
        motion="zoom_in",
    )
    out = render(tmp_path, [s], 2)
    assert reddish(pixel(out, 1.0, 90, 40))  # top half
    assert bluish(pixel(out, 1.0, 90, 200))  # bottom half


@needs_ffmpeg
def test_a_wide_picture_is_fitted_over_a_blurred_copy_not_cropped(tmp_path):
    # left third red, middle white, right third blue
    img = tmp_path / "wide.png"
    run(
        "-f",
        "lavfi",
        "-i",
        "color=c=white:s=900x300",
        "-vf",
        "drawbox=x=0:y=0:w=300:h=300:color=red:t=fill,drawbox=x=600:y=0:w=300:h=300:color=blue:t=fill",
        "-frames:v",
        "1",
        str(img),
    )
    f = render_one(tmp_path / "fill", img, "fill")
    b = render_one(tmp_path / "blur", img, "blur")
    # the centre row's left edge: fill has cropped the red away, the fit shows it
    assert not reddish(pixel(f, 1.0, 4, 160))
    assert reddish(pixel(b, 1.0, 4, 160))
    # above and below the fitted picture the blurred copy fills the frame: not black
    top = pixel(b, 1.0, 90, 8)
    assert sum(top) > 120


def render_one(folder, img, layout, motion="zoom_in", seconds=2):
    s = reel.Shot(0, seconds * 1000, img, "image", "c", layout=layout, motion=motion)
    folder.mkdir(exist_ok=True)
    out = folder / "reel.mp4"
    reel.render(
        tone(folder, seconds),
        [s],
        out,
        work=folder / "w",
        size=(180, 320),
        end_card_s=0,
        hw=False,
    )
    return out


@needs_ffmpeg
@pytest.mark.parametrize("motion", looks.MOTIONS)
def test_every_ken_burns_motion_renders(tmp_path, motion):
    img = solid(tmp_path, "a.png", "green")
    out = render_one(tmp_path / motion, img, "fill", motion=motion, seconds=1)
    assert out.stat().st_size > 0


@needs_ffmpeg
def test_a_pan_really_moves_the_picture(tmp_path):
    # left half red, right half blue: panning right shows more blue as it goes
    img = tmp_path / "rb.png"
    run(
        "-f",
        "lavfi",
        "-i",
        "color=c=blue:s=1600x1600",
        "-vf",
        "drawbox=x=0:y=0:w=800:h=1600:color=red:t=fill",
        "-frames:v",
        "1",
        str(img),
    )
    out = render_one(tmp_path / "p", img, "fill", motion="pan_right", seconds=3)
    early, late = pixel(out, 0.1, 90, 160), pixel(out, 2.8, 90, 160)
    assert late[2] > early[2]  # bluer as the view slides right


def two_colours(tmp_path, kind_a="red", kind_b="blue"):
    return solid(tmp_path, "r.png", kind_a), solid(tmp_path, "b.png", kind_b)


def two_shots(tmp_path, transition):
    a, b = two_colours(tmp_path)
    shots = [
        reel.Shot(0, 2000, a, "image", "A", layout="fill", motion="pan_right"),
        reel.Shot(2000, 4000, b, "image", "B", layout="fill", motion="pan_left"),
    ]
    return reel.assign_transitions(shots, transition)


@needs_ffmpeg
def test_a_cut_switches_at_once(tmp_path):
    out = render(tmp_path, two_shots(tmp_path, "cut"), 4)
    assert reddish(pixel(out, 1.9, 90, 160)) and bluish(pixel(out, 2.1, 90, 160))


@needs_ffmpeg
def test_a_crossfade_passes_through_a_mix(tmp_path):
    out = render(tmp_path, two_shots(tmp_path, "fade"), 4)
    assert reddish(pixel(out, 1.5, 90, 160)) and bluish(pixel(out, 2.6, 90, 160))
    mid = pixel(out, 2.2, 90, 160)  # 0.4 s fade starting at 2.0, halfway
    assert mid[0] > 40 and mid[2] > 40, mid


@needs_ffmpeg
def test_a_slide_brings_the_new_shot_in_over_the_old_one(tmp_path):
    out = render(tmp_path, two_shots(tmp_path, "slide"), 4)
    left, right = pixel(out, 2.17, 20, 160), pixel(out, 2.17, 160, 160)
    assert reddish(left) and bluish(
        right
    )  # mid-slide: old on the left, new coming in on the right
    assert bluish(pixel(out, 2.6, 20, 160))


@needs_ffmpeg
def test_a_whip_pushes_the_old_shot_out_to_the_left(tmp_path):
    out = render(tmp_path, two_shots(tmp_path, "whip"), 4)
    left, right = pixel(out, 2.125, 20, 160), pixel(out, 2.125, 160, 160)
    assert reddish(left) and bluish(right)
    assert bluish(pixel(out, 2.6, 20, 160)) and reddish(pixel(out, 1.8, 160, 160))


@needs_ffmpeg
def test_a_video_clip_can_be_stacked_and_blurred(tmp_path):
    clip = tmp_path / "c.webm"
    run("-f", "lavfi", "-i", "testsrc=s=320x240:d=1", "-c:v", "libvpx", str(clip))
    img = solid(tmp_path, "r.png", "red")
    s = reel.Shot(
        0,
        2000,
        clip,
        "video",
        "V",
        layout="stack",
        second=reel.Part(img, "image", "I", None, "zoom_in"),
        motion="zoom_in",
    )
    out = render(tmp_path, [s], 2)
    assert reddish(pixel(out, 1.0, 90, 200))  # the still is the bottom half
    b = reel.Shot(0, 2000, clip, "video", "V", layout="blur", size=(320, 240))
    out2 = tmp_path / "blur.mp4"
    reel.render(
        tone(tmp_path, 2),
        [b],
        out2,
        work=tmp_path / "w2",
        size=(180, 320),
        end_card_s=0,
        hw=False,
    )
    assert out2.stat().st_size > 0


@needs_ffmpeg
def test_credits_of_a_stack_are_two_lines_and_both_reach_the_end_card(tmp_path):
    red, blue = two_colours(tmp_path)
    s = reel.Shot(
        0,
        2000,
        red,
        "image",
        "Red by Ann, CC0",
        layout="stack",
        second=reel.Part(blue, "image", "Blue by Bob, CC BY", None, "zoom_out"),
    )
    cmd = reel.command(
        tone(tmp_path, 2),
        [s],
        tmp_path / "o.mp4",
        2.0,
        tmp_path / "w",
        size=(180, 320),
        end_card_s=3,
        hw=False,
    )
    assert cmd
    assert (tmp_path / "w" / "credit0.txt").read_text().count("\n") >= 1
    card = (tmp_path / "w" / "endcard.txt").read_text()
    assert "Red by Ann" in card and "Blue by Bob" in card


def loud_tone(folder, seconds):
    folder.mkdir(exist_ok=True)
    out = folder / "loud.m4a"
    run("-f", "lavfi", "-i", f"aevalsrc=0.95*sin(2*PI*440*t):d={seconds}", str(out))
    return out


@needs_ffmpeg
@pytest.mark.parametrize("layout", ["fill", "blur"])
def test_a_transparent_picture_does_not_let_the_waveform_show_through(tmp_path, layout):
    # A red square on a fully transparent ground, with white underneath the
    # transparency, as cut-out PNGs from Commons often have.
    logo = tmp_path / "logo.png"
    run(
        "-f",
        "lavfi",
        "-i",
        "color=c=white@0.0:s=640x360,format=rgba",
        "-vf",
        "drawbox=x=280:y=140:w=80:h=80:color=red@1:t=fill",
        "-frames:v",
        "1",
        str(logo),
    )
    s = reel.Shot(0, 2000, logo, "image", "c", layout=layout)
    out = tmp_path / "reel.mp4"
    reel.render(
        loud_tone(tmp_path, 2),
        [s],
        out,
        work=tmp_path / "w",
        size=(180, 320),
        end_card_s=0,
        hw=False,
    )
    # a row well away from the red square, where only the ground is
    row = [pixel(out, 1.0, x, 40) for x in range(0, 180, 3)]
    assert not any(sum(p) > 300 for p in row), "waveform lines show through the ground"
    assert max(max(p) for p in row) < 120


# --- keeping a big still from being decoded at full size for every frame ------


def big_photo(tmp_path, name="photo.jpg", size="4000x3000"):
    out = tmp_path / name
    run(
        "-f",
        "lavfi",
        "-i",
        f"testsrc2=s={size}:r=1",
        "-frames:v",
        "1",
        "-q:v",
        "3",
        str(out),
    )
    return out


def dims(path):
    raw = subprocess.run(
        [
            "ffprobe",
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height",
            "-of",
            "csv=p=0",
            str(path),
        ],
        capture_output=True,
        text=True,
        check=True,
    ).stdout.strip()
    w, h = raw.split(",")
    return int(w), int(h)


@needs_ffmpeg
def test_a_wide_photo_in_a_blurred_fit_is_shrunk_once_to_what_the_frame_can_show(
    tmp_path,
):
    photo = big_photo(tmp_path)
    s = reel.Shot(0, 2000, photo, "image", "c", layout="blur")
    (prepared,) = reel.prepare_stills([s], (1080, 1920), tmp_path / "w")
    w, h = dims(prepared.path)
    assert (w, h) == (2160, 1620)  # fits twice the frame's width, nothing more
    assert prepared.path != photo and prepared.alpha is False


@needs_ffmpeg
def test_a_picture_already_small_enough_is_left_alone(tmp_path):
    small = solid(tmp_path, "s.png", "red", "640x360")
    s = reel.Shot(0, 2000, small, "image", "c", layout="blur")
    (prepared,) = reel.prepare_stills([s], (1080, 1920), tmp_path / "w")
    assert prepared.path == small and prepared.alpha is False


@needs_ffmpeg
def test_a_picture_with_transparency_is_kept_as_png_and_marked(tmp_path):
    logo = tmp_path / "logo.png"
    run(
        "-f",
        "lavfi",
        "-i",
        "color=c=white@0.0:s=3000x2000,format=rgba",
        "-vf",
        "drawbox=x=1000:y=800:w=300:h=300:color=red@1:t=fill",
        "-frames:v",
        "1",
        str(logo),
    )
    s = reel.Shot(0, 2000, logo, "image", "c", layout="blur")
    (prepared,) = reel.prepare_stills([s], (1080, 1920), tmp_path / "w")
    assert prepared.alpha is True and prepared.path.suffix == ".png"
    assert dims(prepared.path)[0] <= 2160


@needs_ffmpeg
def test_a_fill_shrinks_only_when_the_cover_is_smaller_than_the_photo(tmp_path):
    tall = big_photo(tmp_path, "tall.jpg", "3000x4000")
    s = reel.Shot(0, 2000, tall, "image", "c", layout="fill")
    (prepared,) = reel.prepare_stills([s], (1080, 1920), tmp_path / "w")
    w, h = dims(prepared.path)
    assert w >= 2160 and h >= 3840 and (w, h) != (3000, 4000)  # covers 2x, no more
    wide = big_photo(tmp_path, "wide.jpg", "4000x3000")
    s2 = reel.Shot(0, 2000, wide, "image", "c", layout="fill")
    (kept,) = reel.prepare_stills([s2], (1080, 1920), tmp_path / "w2")
    assert kept.path == wide  # covering a tall frame needs more than it has


@needs_ffmpeg
def test_both_pictures_of_a_stack_are_prepared_for_a_half_frame(tmp_path):
    a, b = big_photo(tmp_path, "a.jpg"), big_photo(tmp_path, "b.jpg", "3000x3000")
    s = reel.Shot(
        0,
        2000,
        a,
        "image",
        "c",
        layout="stack",
        second=reel.Part(b, "image", "d", None, "zoom_out"),
    )
    (p,) = reel.prepare_stills([s], (1080, 1920), tmp_path / "w")
    assert dims(p.path)[0] <= 4000 and p.path != a
    assert p.second.path != b and dims(p.second.path)[0] <= 3000
    assert p.second.alpha is False


@needs_ffmpeg
def test_a_video_is_never_touched(tmp_path):
    clip = tmp_path / "c.webm"
    run("-f", "lavfi", "-i", "testsrc=s=320x240:d=1", "-c:v", "libvpx", str(clip))
    s = reel.Shot(0, 2000, clip, "video", "c", layout="blur")
    (kept,) = reel.prepare_stills([s], (1080, 1920), tmp_path / "w")
    assert kept.path == clip


def test_only_a_picture_with_transparency_is_flattened():
    opaque = looks.still_filters(
        "1:v", "o", "zoom_in", 30, 270, 480, "fill", "t", flatten=False
    )
    assert "premultiply" not in " ".join(opaque)
    alpha = looks.still_filters(
        "1:v", "o", "zoom_in", 30, 270, 480, "fill", "t", flatten=True
    )
    assert "premultiply" in " ".join(alpha)
    blur = looks.still_filters(
        "1:v", "o", "zoom_in", 30, 270, 480, "blur", "t", flatten=False
    )
    assert "premultiply" not in " ".join(blur)


def test_an_unknown_picture_is_flattened_to_be_safe():
    s = reel.Shot(0, 2000, Path("a.jpg"), "image", "c")
    assert s.alpha is True and s.parts[0].alpha is True


def test_the_alpha_formats_are_recognised():
    from app.services import media

    for fmt in ("rgba", "bgra", "argb", "ya8", "pal8", "yuva420p", "gbrap", "rgba64le"):
        assert media.alpha_format(fmt), fmt
    for fmt in ("yuvj420p", "yuv420p", "rgb24", "gray", "gray16le", "bgr24"):
        assert not media.alpha_format(fmt), fmt
    assert media.alpha_format(None)  # not knowing is treated as having it


# --- a still is decoded once, not once per frame ------------------------------


def test_zoompan_makes_all_the_frames_of_a_shot_from_one_picture():
    assert "d=60:" in looks.zoompan("zoom_in", 60, 270, 480)
    assert "d=1:" in looks.zoompan("zoom_in", 1, 270, 480)


def test_a_still_is_fed_in_once_and_not_looped(tmp_path):
    shots = [reel.Shot(0, 2000, tmp_path / "a.jpg", "image", "c")]
    cmd = reel.command(
        tmp_path / "ep.m4a",
        shots,
        tmp_path / "o.mp4",
        2.0,
        tmp_path / "w",
        size=(270, 480),
        end_card_s=0,
        hw=False,
    )
    assert "-loop" not in cmd, "looping re-decodes the file for every frame"
    graph = cmd[cmd.index("-filter_complex") + 1]
    assert "d=60:" in graph  # two seconds at 30 fps, all from the one frame


def test_a_transition_lengthens_the_still_not_its_decoding(tmp_path):
    shots = reel.assign_transitions(
        [
            shot(0, 2000, str(tmp_path / "a.jpg")),
            shot(2000, 4000, str(tmp_path / "b.jpg")),
        ],
        "fade",
    )
    cmd = reel.command(
        tmp_path / "ep.m4a",
        shots,
        tmp_path / "o.mp4",
        4.0,
        tmp_path / "w",
        size=(270, 480),
        end_card_s=0,
        hw=False,
    )
    graph = cmd[cmd.index("-filter_complex") + 1]
    assert "d=72:" in graph  # the first shot stays 0.4 s longer under the next one


@needs_ffmpeg
def test_a_zoom_in_really_grows_the_picture_over_the_shot(tmp_path):
    # a red square in the middle of a blue picture: the closer the zoom, the
    # more of the frame it covers
    img = tmp_path / "sq.png"
    run(
        "-f",
        "lavfi",
        "-i",
        "color=c=blue:s=800x800",
        "-vf",
        "drawbox=x=250:y=250:w=300:h=300:color=red:t=fill",
        "-frames:v",
        "1",
        str(img),
    )
    out = render_one(tmp_path / "z", img, "fill", motion="zoom_in", seconds=3)
    # the square spans x 30..150 of the 180 px frame at the start and 21..159 at the
    # end, so x=25 is off it at first and on it by the end
    assert bluish(pixel(out, 0.1, 25, 160)) and reddish(pixel(out, 2.9, 25, 160))


@needs_ffmpeg
def test_every_frame_of_a_looped_free_still_is_there(tmp_path):
    img = solid(tmp_path, "a.png", "green")
    out = render_one(tmp_path / "f", img, "fill", seconds=2)
    count = subprocess.run(
        [
            "ffprobe",
            "-v",
            "error",
            "-count_frames",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=nb_read_frames",
            "-of",
            "csv=p=0",
            str(out),
        ],
        capture_output=True,
        text=True,
        check=True,
    ).stdout.strip()
    assert 59 <= int(count) <= 61


# --- a crossfade pays for transparency only while it fades --------------------


def fade_graph(tmp_path, n=2, style="fade"):
    shots = reel.assign_transitions(
        [shot(i * 2000, (i + 1) * 2000, str(tmp_path / f"{i}.jpg")) for i in range(n)],
        style,
    )
    cmd = reel.command(
        tmp_path / "ep.m4a",
        shots,
        tmp_path / "o.mp4",
        2.0 * n,
        tmp_path / "w",
        size=(270, 480),
        end_card_s=0,
        hw=False,
    )
    return cmd[cmd.index("-filter_complex") + 1]


def test_only_the_first_part_of_a_fading_shot_has_an_alpha_channel(tmp_path):
    graph = fade_graph(tmp_path, n=3)
    assert graph.count("yuva420p") == 3  # one short head per shot, not the whole shot
    assert graph.count("fade=t=in") == 3
    # the rest of each shot starts where its fade ends: 0.4 s in
    for start in ("0.400", "2.400", "4.400"):
        assert f"trim=start={start}" in graph
        assert f"trim=end={start}" in graph


def test_the_rest_of_a_fading_shot_is_overlaid_opaque(tmp_path):
    graph = fade_graph(tmp_path)
    # the fade's own overlay covers just the fade; the rest covers what follows
    assert "enable='between(t,0.000,0.400)'" in graph
    assert (
        "enable='between(t,0.400,2.400)'" in graph
    )  # up to the end, plus the 0.4 s under the next


def test_other_transitions_still_use_a_single_opaque_stream(tmp_path):
    for style in ("cut", "slide", "whip"):
        graph = fade_graph(tmp_path, style=style)
        assert "yuva420p" not in graph and "trim=" not in graph, style


@needs_ffmpeg
def test_a_crossfade_has_no_gap_where_the_fade_hands_over_to_the_rest(tmp_path):
    out = render(tmp_path, two_shots(tmp_path, "fade"), 4)
    # the fade of the second shot runs 2.0 to 2.4 s; sample frames across the join
    for t in (2.30, 2.37, 2.40, 2.43, 2.50, 2.70):
        px = pixel(out, t, 90, 160)
        assert px[2] > 60, (
            t,
            px,
        )  # blue is there, never the red underneath showing alone


@needs_ffmpeg
def test_the_first_shot_fades_in_over_the_waveform(tmp_path):
    a, _ = two_colours(tmp_path)
    s = reel.assign_transitions([reel.Shot(0, 2000, a, "image", "A")], "fade")
    out = render(tmp_path, s, 2)
    early, late = pixel(out, 0.05, 90, 160), pixel(out, 1.0, 90, 160)
    assert reddish(late) and early[0] < late[0] - 60


@needs_ffmpeg
def test_a_mixed_reel_with_fades_whips_and_slides_renders_whole(tmp_path):
    imgs = [
        solid(tmp_path, f"{c}.png", c)
        for c in ("red", "blue", "green", "yellow", "white")
    ]
    shots = [
        reel.Shot(
            i * 1500, (i + 1) * 1500, p, "image", f"c{i}", motion=looks.auto_motion(i)
        )
        for i, p in enumerate(imgs)
    ]
    out = render(tmp_path, reel.assign_transitions(shots, "mixed"), 7.5)
    count = subprocess.run(
        [
            "ffprobe",
            "-v",
            "error",
            "-count_frames",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=nb_read_frames",
            "-of",
            "csv=p=0",
            str(out),
        ],
        capture_output=True,
        text=True,
        check=True,
    ).stdout.strip()
    assert 224 <= int(count) <= 226  # 7.5 s at 30 fps
