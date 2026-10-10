import re
import shutil
import subprocess


def has_video(path: str) -> bool | None:
    """Does `path` have a video stream (cover art counts)? None if it cannot be told."""
    ffprobe = shutil.which("ffprobe")
    if not ffprobe:
        return None
    try:
        out = subprocess.run(
            [
                ffprobe,
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=codec_type",
                "-of",
                "csv=p=0",
                path,
            ],
            capture_output=True,
            text=True,
            timeout=10,
            check=True,
        ).stdout
    except (OSError, subprocess.SubprocessError):
        return None
    return "video" in out


# Pixel formats that carry transparency, and the palette format, which can.
_ALPHA = re.compile(
    r"^(rgba|bgra|argb|abgr|ya8|ya16|pal8|gbrap|yuva|ayuv|rgba64|bgra64|argb64|abgr64)"
)


def alpha_format(pix_fmt: str | None) -> bool:
    """Might a picture of this pixel format be transparent? Not knowing counts as yes."""
    return True if pix_fmt is None else bool(_ALPHA.match(pix_fmt))


def image_info(path: str) -> tuple[int, int, str] | None:
    """(width, height, pixel format) of the first video stream, None if unreadable."""
    ffprobe = shutil.which("ffprobe")
    if not ffprobe:
        return None
    try:
        out = subprocess.run(
            [
                ffprobe,
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=width,height,pix_fmt",
                "-of",
                "csv=p=0",
                str(path),
            ],
            capture_output=True,
            text=True,
            timeout=15,
            check=True,
        ).stdout.strip()
        w, h, fmt = out.split(",")
        return int(w), int(h), fmt
    except (OSError, subprocess.SubprocessError, ValueError):
        return None


def size(path: str) -> tuple[int, int] | None:
    """(width, height) of a picture or clip, None if it cannot be read."""
    info = image_info(str(path))
    return (info[0], info[1]) if info else None
