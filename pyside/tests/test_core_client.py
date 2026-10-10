from app.core_client import CoreClient


def test_core_client_check_model_exists(qapp):
    client = CoreClient()
    try:
        fut = client.call("checkModelExists", "tiny")
        result = fut.result(timeout=5.0)
        assert isinstance(result, bool)
    finally:
        client.close()


def test_core_client_extract_first_frame(qapp):
    from pathlib import Path

    repo_root = Path(__file__).resolve().parents[2]
    sample = str((repo_root / "rust" / "bin" / "test_input.mp4").resolve())
    client = CoreClient()
    try:
        fut = client.call("extractFirstFrame", {"videoPath": sample})
        result = fut.result(timeout=10.0)
        assert "imageData" in result
        data = result["imageData"]
        assert data.startswith("data:image/")
    finally:
        client.close()


def test_core_client_burn_captions(qapp, tmp_path):
    from pathlib import Path

    repo_root = Path(__file__).resolve().parents[2]
    sample = str((repo_root / "rust" / "bin" / "test_input.mp4").resolve())
    client = CoreClient()

    progress_events = []
    client.progress.connect(
        lambda req_id, status, val: progress_events.append((req_id, status, val))
    )

    try:
        params = {
            "inputVideo": sample,
            "segments": [{"startMs": 0, "endMs": 1500, "text": "Burned caption test"}],
            "exportFormats": ["16:9"],
            "karaoke": False,
            "fontName": "Montserrat",
            "fontSize": 48,
            "textColor": "#FFFFFF",
            "highlightWordColor": "#00FF00",
            "outlineColor": "#000000",
            "positionOverrides": [],
        }
        fut = client.call("burn", params)
        res = fut.result(timeout=30.0)
        assert isinstance(res, list) and len(res) > 0
        output_file = res[0].get("captionedVideo", "")
        assert output_file and Path(output_file).exists()
        qapp.processEvents()
    finally:
        client.close()
        if "output_file" in locals() and output_file:
            Path(output_file).unlink(missing_ok=True)
