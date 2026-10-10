"""The Podman units, the Caddyfile and the portal's own defaults have to agree.

Nothing here runs Podman; it checks the names and ports the pieces use to find
each other, which is where a copy of one file drifts from another and the
deployment fails with nothing wrong in either file alone."""

import re
from pathlib import Path

PODMAN = Path(__file__).resolve().parents[1] / "podman"


def read(name: str) -> str:
    return (PODMAN / name).read_text()


def setting(text: str, key: str) -> str:
    m = re.search(rf"^{key}=(.+)$", text, re.MULTILINE)
    assert m, f"{key} is missing"
    return m.group(1).strip()


def test_caddy_proxies_to_the_portal_container_by_name_and_port():
    portal = read("capslap-portal.container")
    name = setting(portal, "ContainerName")
    assert f"reverse_proxy {name}:8080" in read("Caddyfile")


def test_the_portal_listens_on_the_port_caddy_uses():
    # PORTAL_PORT defaults to 8080 and the image sets it so; the Caddyfile
    # points at 8080, so the unit must not override it.
    dockerfile = (PODMAN.parent / "Dockerfile").read_text()
    assert "PORTAL_PORT=8080" in dockerfile
    assert "PORTAL_PORT" not in read("capslap-portal.container")


def test_both_containers_share_the_network_the_units_define():
    network = setting(read("capslap.network"), "NetworkName")
    for unit in ("capslap-portal.container", "capslap-caddy.container"):
        assert setting(read(unit), "Network") == f"{network}.network"


def test_the_volumes_the_units_mount_are_defined():
    for unit, volume in (
        ("capslap-portal.container", "capslap-portal-data"),
        ("capslap-caddy.container", "capslap-caddy-data"),
    ):
        assert f"Volume={volume}.volume:" in read(unit)
        assert setting(read(f"{volume}.volume"), "VolumeName") == volume


def test_the_portal_keeps_its_data_where_the_image_keeps_it():
    assert "Volume=capslap-portal-data.volume:/data" in read("capslap-portal.container")


def test_the_env_example_names_what_the_units_and_caddyfile_read():
    example = read("portal.env.example")
    assert re.search(r"^PORTAL_ADMIN_TOKEN=", example, re.MULTILINE)
    assert re.search(r"^PORTAL_DOMAIN=", example, re.MULTILINE)
    assert "{$PORTAL_DOMAIN}" in read("Caddyfile")


def test_caddy_waits_for_the_portal_and_both_start_at_login():
    caddy = read("capslap-caddy.container")
    assert "Requires=capslap-portal.service" in caddy
    for unit in ("capslap-portal.container", "capslap-caddy.container"):
        assert "WantedBy=default.target" in read(unit)


def test_the_units_read_the_same_env_file_the_readme_tells_to_create():
    for unit in ("capslap-portal.container", "capslap-caddy.container"):
        assert (
            setting(read(unit), "EnvironmentFile")
            == "%h/.config/capslap-portal/portal.env"
        )
