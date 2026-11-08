# my-calendar — rough plan

Personal, single-user calendar. Rust + SQLite server, installable PWA client,
mobile-first, dark only. Auto-syncs the Langara D2L iCal feed.

Designs: open `docs/designs/index.html` in a browser.

---

## 1. What the D2L feed actually looks like (checked 2026-11-08)

- Plain iCalendar, ~10 KB, 20 `VEVENT`s, calendar name "All Courses - Langara College".
- **No `RRULE`s**: every event is a single instance.
- **Times are UTC** (`...Z`). Deadlines like `20261120T065959Z` are 11:59 PM Pacific,
  so we must render in local time and never store "naive" times.
- **All-day items** use `DTSTART;VALUE=DATE` (older MATH-1171 quizzes/exams).
- **Deadlines** have `DTSTART == DTEND` (zero-length). Treat these as *due items*
  (show a checkbox, not a time block).
- `LOCATION` holds the course: `CPSC-2190-M01 - Theoretical Founds of Comp Sci`.
  We can parse the course code (`CPSC-2190`) and colour-code automatically.
- `SUMMARY` ends in `- Due` / `- Available`, so we can classify kind = due | available | event.
- `DESCRIPTION` contains D2L links (`View event - https://…`), so we pull out the
  first link for an "Open in D2L" button.
- Stable `UID` + `SEQUENCE` + `LAST-MODIFIED` make upserts easy.

> The feed URL contains a personal token. It is entered in the app's
> **Settings → Subscriptions** screen and stored in SQLite (`calendars.source_url`),
> never in the repo or env. The API only ever returns it masked
> (`…feed.ics?token=abcd••••`), and `data/` is git-ignored.

---

## 2. Architecture

```
┌──────────── phone / desktop ────────────┐
│ PWA (Svelte + Vite, service worker)      │
│  • app shell cached offline              │
│  • events cached in IndexedDB            │
└───────────────┬─────────────────────────┘
                │ JSON over HTTPS
┌───────────────▼─────────────────────────┐
│ Rust binary (axum + tokio)               │
│  • /api/*  REST                          │
│  • static PWA assets (embedded)          │
│  • sync worker: every 30 min → fetch ICS │
│  • SQLite (sqlx, WAL mode)  data/cal.db  │
└──────────────────────────────────────────┘
```

**One binary, one DB file.** The built PWA is embedded with `rust-embed`, so
deploying = copy the binary; all config (feed URLs, sync interval, colours) lives in the DB.
The only CLI/env options are `--port` and `--data-dir`.

### Server crates
| Purpose | Crate |
|---|---|
| HTTP | `axum`, `tower-http` (compression, CORS in dev) |
| DB | `sqlx` (sqlite, migrations, compile-time checked queries) |
| ICS parse | `icalendar` (parser feature) or `ical` |
| Recurrence (local events) | `rrule` |
| Time | `chrono` + `chrono-tz` |
| Fetch | `reqwest` (rustls) |
| Static | `rust-embed` |
| CLI / logs | `clap`, `tracing` |

### Client
- **Svelte 5 + Vite + TypeScript**, `vite-plugin-pwa` (Workbox) for manifest + SW.
  Small bundle, no UI framework; hand-written CSS with design tokens.
- Date math: `date-fns` (+ `date-fns-tz` only if needed; browser `Intl` covers most).
- Routes: `/` Agenda (default) · `/month` · `/week` · `/settings`.
- Gestures: swipe left/right to change period, bottom sheets for detail/edit.
- Offline: read everything offline; create/edit queues in an IndexedDB outbox and
  flushes when back online (v2 — v1 can just disable the save button offline).

---

## 3. Data model (SQLite)

```sql
calendars (
  id INTEGER PK, name TEXT, color TEXT,
  kind TEXT CHECK(kind IN ('local','ics')),
  source_url TEXT NULL, sync_interval_min INTEGER DEFAULT 30,
  etag TEXT NULL, last_modified TEXT NULL,
  last_sync_at TEXT NULL, last_sync_status TEXT NULL, enabled INTEGER DEFAULT 1
)

events (
  id INTEGER PK, calendar_id INTEGER FK,
  uid TEXT,                       -- ICS UID, or generated UUID for local
  title TEXT, description TEXT, location TEXT, url TEXT,
  start_utc TEXT, end_utc TEXT,   -- RFC3339 UTC; NULL for all-day
  start_date TEXT, end_date TEXT, -- YYYY-MM-DD for all-day (end exclusive)
  all_day INTEGER, kind TEXT,     -- 'event' | 'due' | 'available'
  course TEXT NULL,               -- 'CPSC-2190' parsed from LOCATION
  rrule TEXT NULL,                -- local events only
  sequence INTEGER, source_modified TEXT,
  removed_at TEXT NULL,           -- soft-delete when it disappears from feed
  created_at TEXT, updated_at TEXT,
  UNIQUE(calendar_id, uid)
)

-- user-owned overlay on any event, so a sync never clobbers it
event_meta (
  event_id INTEGER PK FK, done INTEGER DEFAULT 0, hidden INTEGER DEFAULT 0,
  color_override TEXT NULL, note TEXT NULL
)

courses (code TEXT PK, label TEXT, color TEXT)   -- auto-filled, user-editable
```

Subscribed events are **read-only**. Personal tweaks (done, hide, note, colour)
live in `event_meta`.

---

## 4. Sync

1. A tokio interval task (default 30 min) plus `POST /api/calendars/:id/sync` for "Sync now".
2. `GET` with `If-None-Match` / `If-Modified-Since` → on 304, just update `last_sync_at`.
3. Parse → normalise (UTC / all-day / kind / course / first URL).
4. In one transaction: upsert by `(calendar_id, uid)` (skip if `sequence` and
   `LAST-MODIFIED` unchanged); soft-delete rows whose UID vanished.
5. Record status + counts (`+2 new · 1 updated · 0 removed`) for the Settings screen.
6. Failures back off exponentially (max 6 h) and show a warning dot in the UI.
   Old data stays visible.

Generic enough that any other `.ics` URL can be added later (e.g. a work calendar).

---

## 5. API (JSON)

```
GET    /api/events?from=…&to=…        expanded instances in range (rrules expanded server-side)
GET    /api/events/upcoming?limit=50  agenda feed
POST   /api/events                    create (local calendars only)
PATCH  /api/events/:id                update  (local only)
DELETE /api/events/:id
PUT    /api/events/:id/meta           done / hidden / note / color (any event)
GET    /api/calendars                 incl. sync status
POST   /api/calendars                 add local or ics calendar
PATCH  /api/calendars/:id
POST   /api/calendars/:id/sync
GET    /api/courses   PATCH /api/courses/:code
GET    /api/export.ics                (nice-to-have: subscribe from phone's native calendar)
```

---

## 6. "No login" — a caveat

No login is fine **as long as the server isn't openly on the internet**, because
anyone who finds the URL could read and edit everything. Options, in order of
simplicity:

1. **Tailscale** on the VM + phone: private HTTPS URL, zero auth code. *(recommended)*
2. Caddy reverse proxy with HTTP basic auth (one shared password, browser remembers it).
3. Cloudflare Tunnel + Access.

Note: PWA install and service workers require **HTTPS** (except on `localhost`),
so one of the above is needed anyway for the phone.

---

## 7. Milestones

| # | Scope | Done when |
|---|---|---|
| M0 | Repo scaffold: `server/` cargo workspace, `web/` Vite app, `justfile`, `.gitignore` (incl. `data/`) | `just dev` runs both |
| M1 | DB schema + migrations, ICS sync worker, `GET /api/events` | D2L events in SQLite, visible via curl |
| M2 | Agenda + Month views, event detail sheet, course colours | Usable read-only on phone |
| M3 | Create/edit/delete local events, recurrence, done checkbox | Daily-driver |
| M4 | PWA: manifest, icons, SW, offline cache, install prompt | Installs to home screen, opens offline |
| M5 | Week/3-day timeline, swipe nav, settings screen | Full design shipped |
| M6 | Deploy: systemd or Docker + Tailscale/Caddy, SQLite backup cron | Running 24/7 |
| later | Web push reminders for due items, `.ics` export, outbox offline edits, search | — |

---

## 8. Decisions (2026-11-08)

- Implementation deviations from the sketch above (to keep the container minimal):
  `rusqlite` instead of `sqlx`, a hand-rolled ICS parser and RRULE subset instead of crates,
  and a **no-build vanilla JS** PWA instead of Svelte/Vite. Offline reads use the
  service worker's cache of API responses (no IndexedDB).
- Deliverable is a Docker image (`FROM scratch`, static musl binary).

- Week starts on **Sunday**.
- Times display in the **phone's time zone** (`Intl` on the client; server stays UTC).
- **Past-term courses stay visible** (they can still be hidden one at a time in Settings).
- **No push notifications** in v1.
- Access via **Tailscale** only: no auth layer, and HTTPS comes from `tailscale serve`.
- The D2L feed URL is an **in-app setting** (Settings → Subscriptions → Add .ics URL),
  not an env var. On first run the app has no subscriptions; Settings shows an empty state.
  "Test URL" fetches and parses the feed before saving and shows the calendar name + event count.
