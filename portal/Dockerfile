# Build from the repository root, so the caption review page comes along:
#   docker build -f portal/Dockerfile -t capslap-portal .
FROM python:3.12-slim

COPY --from=ghcr.io/astral-sh/uv:0.8 /uv /usr/local/bin/uv
WORKDIR /app/portal
COPY portal/pyproject.toml portal/uv.lock portal/README.md ./
RUN uv sync --frozen --no-dev --no-install-project
COPY portal/capslap_portal ./capslap_portal
RUN uv sync --frozen --no-dev
COPY review /app/review

ENV PORTAL_DATA=/data \
    PORTAL_REVIEW_DIR=/app/review \
    PORTAL_HOST=0.0.0.0 \
    PORTAL_PORT=8080
RUN mkdir /data && chown 1000:1000 /data
VOLUME /data
EXPOSE 8080
USER 1000:1000
CMD ["/app/portal/.venv/bin/capslap-portal"]
