"""How a picture sits in the 9:16 frame and how a still moves.

Pure functions that return ffmpeg filter text; the reel stitches them together.
"""

FPS = 30
# A zoom starts or ends this much closer than the whole picture.
KB_ZOOM = 1.15
# A pan shows 1/1.2 of the picture and slides across the rest: it needs room
# to move, and 20 % is visible without cropping the subject away.
KB_PAN_ZOOM = 1.2
# At most this (width over height) and a picture fills the 9:16 frame; a
# wide or square one is fitted over a blurred copy of itself. A 3:4 picture
# (0.75) loses a quarter of its width to a 9:16 crop, which still reads as
# the same picture; a square one loses almost half.
FILL_ASPECT_LIMIT = 0.75
# The blurred backdrop is drawn this many times smaller and scaled back up:
# the blur costs a sixteenth as much and comes out smoother.
BLUR_SHRINK = 4
BLUR_SIGMA = 5

# Transparent ground goes dark, like the reel's own background, rather than
# letting the waveform behind it show: colour times alpha, then the alpha goes.
# Cut-out PNGs from Commons often carry it, with whatever colour under it.
FLATTEN = "format=gbrap,premultiply=inplace=1,format=rgb24"

MOTIONS = ("zoom_in", "pan_right", "zoom_out", "pan_left", "pan_down", "pan_up")
_OPPOSITE = {
    "zoom_in": "zoom_out",
    "zoom_out": "zoom_in",
    "pan_right": "pan_left",
    "pan_left": "pan_right",
    "pan_down": "pan_up",
    "pan_up": "pan_down",
}


def auto_motion(index: int) -> str:
    """Neighbouring shots never move the same way."""
    return MOTIONS[index % len(MOTIONS)]


def opposite(motion: str) -> str:
    return _OPPOSITE[motion]


def auto_layout(size: tuple[int, int] | None) -> str:
    # Not knowing the shape, fit it: a fit loses nothing, a crop may lose the subject.
    if not size or size[0] <= 0 or size[1] <= 0:
        return "blur"
    return "fill" if size[0] / size[1] <= FILL_ASPECT_LIMIT else "blur"


def even(v: float) -> int:
    return max(2, int(v) // 2 * 2)


def zoompan(motion: str, frames: int, w: int, h: int, fps: int = FPS) -> str:
    """The Ken Burns filter for a still of `frames` frames, out at w x h.

    One input picture makes all `frames` frames (`d`), so the file is decoded
    and scaled once rather than once per frame, which cost three quarters of a
    render. Progress runs from 0 on the first frame to 1 on the last, so the
    move ends exactly when the shot does, whatever its length."""
    p = f"min(on/{max(frames - 1, 1)},1)"
    a = KB_ZOOM - 1
    centre_x, centre_y = "iw/2-(iw/zoom/2)", "ih/2-(ih/zoom/2)"
    z, x, y = "1", centre_x, centre_y
    if motion == "zoom_in":
        z = f"1+{a:.4f}*{p}"
    elif motion == "zoom_out":
        z = f"{KB_ZOOM:.4f}-{a:.4f}*{p}"
    else:
        z = f"{KB_PAN_ZOOM}"
        if motion == "pan_right":
            x = f"(iw-iw/zoom)*{p}"
        elif motion == "pan_left":
            x = f"(iw-iw/zoom)*(1-{p})"
        elif motion == "pan_down":
            y = f"(ih-ih/zoom)*{p}"
        elif motion == "pan_up":
            y = f"(ih-ih/zoom)*(1-{p})"
        else:
            raise ValueError(f"unknown motion {motion!r}")
    return f"zoompan=z='{z}':x='{x}':y='{y}':d={max(frames, 1)}:s={w}x{h}:fps={fps}"


def _backdrop(tag: str, w: int, h: int, out_w: int, out_h: int) -> str:
    """Filters from `[<tag>a]` to a blurred, dimmed cover of w x h, scaled to out."""
    sw, sh = even(w / BLUR_SHRINK), even(h / BLUR_SHRINK)
    return (
        f"[{tag}a]scale={sw}:{sh}:force_original_aspect_ratio=increase,"
        f"crop={sw}:{sh},gblur=sigma={BLUR_SIGMA},scale={out_w}:{out_h},"
        f"eq=brightness=-0.1[{tag}bg]"
    )


def still_filters(
    src: str,
    out: str,
    motion: str,
    frames: int,
    w: int,
    h: int,
    layout: str,
    tag: str,
    flatten: bool = True,
) -> list[str]:
    """A still, looped to `frames` frames, as a w x h picture moving by `motion`.

    Worked at twice the size so the zoom and pan have pixels to move over."""
    w2, h2 = 2 * w, 2 * h
    move = zoompan(motion, frames, w, h)
    # Only a picture that may be transparent pays for the flattening: it costs
    # about 40 % of a render on 12-megapixel JPEGs, which have no alpha.
    pre = f"{FLATTEN}," if flatten else ""
    if layout == "blur":
        return [
            f"[{src}]{pre}split[{tag}a][{tag}b]",
            _backdrop(tag, w, h, w2, h2),
            f"[{tag}b]scale={w2}:{h2}:force_original_aspect_ratio=decrease[{tag}fg]",
            f"[{tag}bg][{tag}fg]overlay=(W-w)/2:(H-h)/2,{move}[{out}]",
        ]
    return [
        f"[{src}]{pre}scale={w2}:{h2}:force_original_aspect_ratio=increase,"
        f"crop={w2}:{h2},{move}[{out}]"
    ]


def video_filters(
    src: str,
    out: str,
    w: int,
    h: int,
    layout: str,
    tag: str,
    gpu_scale: tuple[int, int] | None = None,
) -> list[str]:
    """A clip as a w x h picture. `gpu_scale` is the size to hand VideoToolbox's
    `scale_vt` first (see reel.cover_dims); frames come back to the CPU after it."""
    if layout == "blur":
        return [
            f"[{src}]split[{tag}a][{tag}b]",
            _backdrop(tag, w, h, w, h),
            f"[{tag}b]scale={w}:{h}:force_original_aspect_ratio=decrease[{tag}fg]",
            f"[{tag}bg][{tag}fg]overlay=(W-w)/2:(H-h)/2,fps={FPS}[{out}]",
        ]
    if gpu_scale:
        sw, sh = gpu_scale
        return [
            f"[{src}]scale_vt=w={sw}:h={sh},hwdownload,format=nv12,"
            f"crop={w}:{h},fps={FPS}[{out}]"
        ]
    return [
        f"[{src}]scale={w}:{h}:force_original_aspect_ratio=increase,"
        f"crop={w}:{h},fps={FPS}[{out}]"
    ]
