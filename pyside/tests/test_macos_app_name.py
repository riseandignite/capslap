from unittest.mock import MagicMock, patch

from app.main import ensure_macos_app_name


def test_ensure_macos_app_name_noop_on_non_darwin():
    with patch("sys.platform", "linux"):
        reexeced = ensure_macos_app_name()
        assert reexeced is False


def test_ensure_macos_app_name_noop_if_already_pycapslap():
    with patch("sys.platform", "darwin"), patch("sys.executable", "/path/to/PyCapSlap"):
        reexeced = ensure_macos_app_name()
        assert reexeced is False


def test_ensure_macos_app_name_noop_if_reexec_flag_set(monkeypatch):
    monkeypatch.setenv("PYCAPSLAP_REEXECED", "1")
    with patch("sys.platform", "darwin"), patch("sys.executable", "/path/to/python3"):
        reexeced = ensure_macos_app_name()
        assert reexeced is False


def test_ensure_macos_app_name_reexecs(monkeypatch, tmp_path):
    monkeypatch.delenv("PYCAPSLAP_REEXECED", raising=False)
    monkeypatch.delenv("PYTEST_CURRENT_TEST", raising=False)

    fake_py = tmp_path / "python3"
    fake_py.write_text("#!/bin/sh\n")

    mock_execve = MagicMock()
    with (
        patch("sys.platform", "darwin"),
        patch("sys.executable", str(fake_py)),
        patch("os.execve", mock_execve),
    ):
        reexeced = ensure_macos_app_name()
        assert reexeced is True
        assert mock_execve.called
        called_binary, called_args, called_env = mock_execve.call_args[0]
        assert "PyCapSlap" in called_binary
        assert called_env.get("PYCAPSLAP_REEXECED") == "1"
