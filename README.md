# my-calendar

A small, self-hosted, single-user calendar. It's a Rust server (axum + SQLite) with an
embedded, installable dark-mode PWA, built mobile-first but usable on desktop too.
It subscribes to any `.ics` feed (e.g. a D2L/LMS calendar) and keeps it in sync.

- Agenda, month and week views; repeating events; offline reading
- Synced events are read-only, but you can mark them done, hide them, add a note or recolour them
- One static binary in a `scratch` image (about 8 MB, ~2 MiB RAM idle)

## Run

```sh
docker run -d --name calendar --restart unless-stopped \
  -p 8080:8080 -v calendar-data:/app/data ghcr.io/cau777/my-calendar:latest
```

Open http://localhost:8080, then add a feed under **Settings → Add .ics URL**.
Feed URLs are stored only in the database (`/app/data`) and are masked in the UI and API.

Options: `PORT` (default `8080`), `DATA_DIR` (default `/app/data`).

> There is no login. Keep it on a private network (e.g. `tailscale serve --bg 8080`),
> not on the public internet. Installing the PWA on a phone requires HTTPS.

## Develop

```sh
cd server && cargo run -- --port 5002 --data-dir ../data   # serves web/ live in debug builds
cargo test
```

## Release

Pushing a `v*` tag builds the image and publishes it to `ghcr.io/cau777/my-calendar` (see `.github/workflows/docker.yml`).
