import shutil
import subprocess

import pytest

from app.services import media

FFMPEG = shutil.which("ffmpeg")


@pytest.mark.skipif(
    FFMPEG is None or shutil.which("ffprobe") is None, reason="needs ffmpeg"
)
def test_has_video_tells_audio_from_video(tmp_path):
    def make(name, *args):
        out = tmp_path / name
        subprocess.run([FFMPEG, "-v", "error", "-y", *args, str(out)], check=True)
        return str(out)

    wav = make("a.wav", "-f", "lavfi", "-i", "sine=duration=1")
    mp4 = make("v.mp4", "-f", "lavfi", "-i", "color=c=red:s=64x64:d=1")
    assert media.has_video(wav) is False
    assert media.has_video(mp4) is True


def test_has_video_is_none_for_a_file_that_cannot_be_read():
    assert media.has_video("/nonexistent/x.mp4") is None
