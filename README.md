# my-calendar

Personal single-user calendar: Rust (axum + SQLite) server with an embedded,
installable dark-mode PWA. Syncs any `.ics` feed (e.g. Langara D2L) on an interval.

## Run with Docker

```sh
docker compose up -d --build        # http://localhost:8080
# or
docker build -t my-calendar .
docker run -d --name calendar --restart unless-stopped \
  -p 8080:8080 -v calendar-data:/app/data --memory 64m my-calendar
```

- Image: `FROM scratch`, a single static binary (about 8 MB). Idles at about 2 MiB RSS.
- All state is in `/app/data/calendar.db` (SQLite, WAL). Back up that volume.
- Config: `PORT` (default 8080) and `DATA_DIR`. Everything else (feed URLs,
  sync interval, colours) is set in the app under **Settings**.
- Add your D2L feed in Settings → Subscriptions → *Add .ics URL*. The token is
  stored only in the DB and is masked in the UI/API.

## Access over Tailscale

There is no login, so don't expose the port publicly. On the Docker host:

```sh
tailscale serve --bg 8080           # https://<host>.<tailnet>.ts.net
```

HTTPS is required for the PWA to install on your phone ("Add to Home Screen").

## Develop

```sh
cd server && cargo run -- --port 5002 --data-dir ../data
```

In debug builds the web files are read from `web/` on every request, so just edit and reload.
There is no frontend build step.
