"""`capslap-portal`: serve the portal. Configured by environment variables:

PORTAL_ADMIN_TOKEN   secret for the admin page and the desktop app (required)
PORTAL_DATA          where the database and the videos go (default ./data)
PORTAL_HOST/PORT     where to listen (default 127.0.0.1:8080; put HTTPS in front)
PORTAL_MAX_UPLOAD_MB largest video accepted (default 8192)
"""

import os

import uvicorn

from capslap_portal.app import Settings, create_app


def main() -> None:
    app = create_app(Settings.from_env())
    uvicorn.run(
        app,
        host=os.environ.get("PORTAL_HOST", "127.0.0.1"),
        port=int(os.environ.get("PORTAL_PORT", "8080")),
        proxy_headers=True,
    )


if __name__ == "__main__":
    main()
