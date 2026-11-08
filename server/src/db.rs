use rusqlite::Connection;

pub const PALETTE: [&str; 8] = [
    "#4fd1c5", "#f6ad55", "#f687b3", "#63b3ed", "#68d391", "#fc8181", "#ecc94b", "#b794f4",
];

pub fn open(path: &std::path::Path) -> rusqlite::Result<Connection> {
    let c = Connection::open(path)?;
    c.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON;
         PRAGMA cache_size=-512; PRAGMA temp_store=MEMORY;",
    )?;
    migrate(&c)?;
    Ok(c)
}

fn migrate(c: &Connection) -> rusqlite::Result<()> {
    let v: i64 = c.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if v < 1 {
        c.execute_batch(
            "BEGIN;
            CREATE TABLE calendars (
              id INTEGER PRIMARY KEY, name TEXT NOT NULL, color TEXT NOT NULL,
              kind TEXT NOT NULL CHECK(kind IN ('local','ics')),
              source_url TEXT, sync_interval_min INTEGER NOT NULL DEFAULT 30,
              color_by_course INTEGER NOT NULL DEFAULT 1,
              etag TEXT, last_modified_hdr TEXT,
              last_sync_at TEXT, last_sync_ok INTEGER, last_sync_status TEXT,
              enabled INTEGER NOT NULL DEFAULT 1
            );
            CREATE TABLE events (
              id INTEGER PRIMARY KEY,
              calendar_id INTEGER NOT NULL REFERENCES calendars(id) ON DELETE CASCADE,
              uid TEXT NOT NULL, title TEXT NOT NULL DEFAULT '',
              description TEXT NOT NULL DEFAULT '', location TEXT NOT NULL DEFAULT '', url TEXT,
              all_day INTEGER NOT NULL DEFAULT 0,
              start TEXT NOT NULL, end TEXT NOT NULL,
              kind TEXT NOT NULL DEFAULT 'event', course TEXT, rrule TEXT,
              sequence INTEGER NOT NULL DEFAULT 0, source_modified TEXT,
              removed_at TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
              UNIQUE(calendar_id, uid)
            );
            CREATE TABLE event_meta (
              event_id INTEGER PRIMARY KEY REFERENCES events(id) ON DELETE CASCADE,
              done INTEGER NOT NULL DEFAULT 0, hidden INTEGER NOT NULL DEFAULT 0,
              color_override TEXT, note TEXT
            );
            CREATE TABLE courses (
              code TEXT PRIMARY KEY, label TEXT NOT NULL, color TEXT NOT NULL,
              hidden INTEGER NOT NULL DEFAULT 0
            );
            INSERT INTO calendars (name, color, kind) VALUES ('Personal', '#a78bfa', 'local');
            PRAGMA user_version = 1;
            COMMIT;",
        )?;
    }
    Ok(())
}
