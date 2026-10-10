# Caption review page

A static page for clients to check and correct captions made in PyCapSlap. No
build step, no dependencies, no server: open `index.html` from any static host
(GitHub Pages publishes it from `main`, see `.github/workflows/review-page.yml`).

- **In:** the video and a `.capslap.json` (from *Client Review → Export for
  Review…* in the app, or any sidecar). Pick them, drop them on the page, or link
  them: `?video=<url>&captions=<url>` (the captions URL has to allow CORS).
- **Out:** `<video>.reviewed.capslap.json` — the same sidecar with the edited
  captions, plus `review: { status, reviewer, reviewedAt, comments, changes }`.
  Import it in the app with *Client Review → Import Reviewed Captions…*.

**From the review portal** (`portal/`), the page is opened as
`/review/?video=…&captions=…&submit=…&back=…`: *Send to the editor* posts the
reviewed file to `submit` (same site only) and a link leads `back` to the episode.

**Password lock.** The page is public, so the app locks the file it exports:
`{ format: 'capslap-locked', kdf: 'PBKDF2-SHA256', iterations, salt, iv, data }`,
where `data` is the captions file sealed with AES-256-GCM. The page asks for the
password, keeps the draft in `localStorage` locked with the same key, and locks the
file it downloads with it. `lock.js` holds this; `app/models/review.py` does the
same in the app, and `testdata/locked.capslap.json` (password
`k7mq-x2fp-9tza-hw4c`) is opened by both test suites. The video is not locked.

Captions are edited, never added or removed, so the file lines up with the
project cue by cue. `core.js` holds the logic and is tested with
`node --test review/*.test.js`; `review.js` is the page.
