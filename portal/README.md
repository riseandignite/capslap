# Videoiden hyväksyntä

The PyCapSlap review portal.

A small server where clients watch the renders of their episodes, fix the
captions and leave feedback, and where the editor collects both. It runs next
to PyCapSlap: the app publishes a render with its captions, and imports what
comes back.

- **Series and episodes.** Every series has one secret link
  (`/s/<token>`). Whoever has it sees that series' episodes and nothing else.
  A new link can be made at any time; the old one stops working.
- **An episode** lists its videos (the small proof renders PyCapSlap makes,
  good for phones and for sharing), each with a player and a download button.
- **Captions.** *Check captions* opens the caption review page on the video.
  The client fixes wording and timing and presses *Send*; the corrected file is
  stored as a new version. The editor downloads it from the admin page (or
  the app fetches it) and imports it into PyCapSlap as is.
- **Post texts.** Each video carries the text it goes out with on every
  service it is meant for (the editor ticks YouTube, TikTok, Instagram,
  Facebook, LinkedIn and X per video). The editor writes them on the admin
  page, the client edits and approves them next to the video; a changed text
  needs approving again. Each is checked against the service's rules: length
  (X counted the way X counts, links as 23), YouTube's 100-character title and
  15-hashtag cap, Instagram's five hashtags, TikTok's 2200 for scheduling
  tools, and the part shown before "more". The limits are in
  `static/platforms.js`; the services change them now and then.
- **Feedback.** Free-form notes on the episode, optionally tied to a video and
  a moment in it.
- **Admin** (`/admin`, with the admin token): series, their links, episodes,
  videos, every captions version and all feedback.

## Running it

```sh
cd portal
PORTAL_ADMIN_TOKEN=$(openssl rand -hex 24) uv run capslap-portal
```

| Variable | Default | |
|---|---|---|
| `PORTAL_ADMIN_TOKEN` | (required) | Secret for `/admin` and for the app |
| `PORTAL_DATA` | `./data` | Database and videos |
| `PORTAL_HOST` / `PORTAL_PORT` | `127.0.0.1` / `8080` | Where to listen |
| `PORTAL_MAX_UPLOAD_MB` | `8192` | Largest video accepted |

It speaks plain HTTP; put it behind HTTPS.

The production portal already runs at https://thumbs.peliteoria.fi, installed
from the infra repository (`registry.json`: `capslap-portal`). It uses the
shared Caddy of that host, a rootless Podman Quadlet on `127.0.0.1:10190`, and
a bind mount at `/opt/stacks/capslap-portal/data` that the host's borg backup
covers. Updates go through `infra/capslap-portal/update.sh`. Do not install a
second copy next to it with the steps below.

The steps below are for other hosts, standalone. The `podman/` folder runs the
portal with Podman as systemd user services, with its own Caddy in front (it
gets the certificate itself). Rootless, so nothing needs root except two
one-time system settings.

```sh
# 1. Once, as root: let a normal user listen on 80 and 443 …
echo 'net.ipv4.ip_unprivileged_port_start=80' | sudo tee /etc/sysctl.d/90-unprivileged-ports.conf
sudo sysctl --system
#    … and keep that user's services running when nobody is logged in.
sudo loginctl enable-linger "$USER"
#    The firewall (and, in a cloud, its ingress rules) must let 80 and 443 in.

# 2. From the repository root: build the image.
podman build -f portal/Dockerfile -t localhost/capslap-portal:latest .

# 3. Put the units and settings where Podman and systemd look for them.
mkdir -p ~/.config/containers/systemd ~/.config/capslap-portal
cp portal/podman/*.container portal/podman/*.volume portal/podman/*.network \
   ~/.config/containers/systemd/
cp portal/podman/Caddyfile ~/.config/capslap-portal/
cp portal/podman/portal.env.example ~/.config/capslap-portal/portal.env
chmod 600 ~/.config/capslap-portal/portal.env
$EDITOR ~/.config/capslap-portal/portal.env     # token (openssl rand -hex 24) and domain

# 4. Start.
systemctl --user daemon-reload
systemctl --user start capslap-portal capslap-caddy
```

The domain's DNS record must already point at the machine when Caddy starts,
or it cannot get the certificate. Look at what is happening with
`journalctl --user -u capslap-portal -u capslap-caddy`. After a new build of the
image: `systemctl --user restart capslap-portal`.

The services start again by themselves after a reboot. `portal/compose.yaml`
does the same job with `podman compose` if that suits better.

Back up `PORTAL_DATA` (under Podman the volume `capslap-portal-data`; see
`podman volume inspect`): `portal.sqlite3` holds everything but the videos.

## Tests

```sh
cd portal && uv run pytest
```
