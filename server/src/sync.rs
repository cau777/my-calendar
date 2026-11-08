use crate::ics::{self, When};
use crate::{now, AppState};
use rusqlite::{params, OptionalExtension};
use std::collections::HashSet;

pub fn when_str(w: &When) -> String {
    match w {
        When::Date(d) => d.format("%Y-%m-%d").to_string(),
        When::Time(t) => t.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
    }
}

fn classify(summary: &str, start: &When, end: &When) -> (String, &'static str) {
    let s = summary.trim();
    for (suf, kind) in [("- Due", "due"), ("- Available", "available")] {
        if let Some(t) = s.strip_suffix(suf) {
            return (t.trim().to_string(), kind);
        }
    }
    let kind = if matches!(start, When::Time(_)) && start == end { "due" } else { "event" };
    (s.to_string(), kind)
}

pub async fn fetch(
    http: &reqwest::Client,
    url: &str,
    etag: Option<&str>,
    lm: Option<&str>,
) -> Result<Option<(String, Option<String>, Option<String>)>, String> {
    let mut req = http.get(url);
    if let Some(e) = etag {
        req = req.header("If-None-Match", e);
    }
    if let Some(l) = lm {
        req = req.header("If-Modified-Since", l);
    }
    let res = req.send().await.map_err(|e| format!("fetch failed: {e}"))?;
    if res.status() == reqwest::StatusCode::NOT_MODIFIED {
        return Ok(None);
    }
    if !res.status().is_success() {
        return Err(format!("HTTP {}", res.status()));
    }
    let h = |n: &str| res.headers().get(n).and_then(|v| v.to_str().ok()).map(String::from);
    let (etag, lm) = (h("etag"), h("last-modified"));
    let body = res.text().await.map_err(|e| format!("read failed: {e}"))?;
    if !body.contains("BEGIN:VCALENDAR") {
        return Err("not an iCalendar feed".into());
    }
    Ok(Some((body, etag, lm)))
}

/// Sync one ICS calendar. Returns a human-readable status.
pub async fn sync_calendar(st: &AppState, id: i64) -> Result<String, String> {
    let row: Option<(String, Option<String>, Option<String>)> = {
        let db = st.db.lock().unwrap();
        db.query_row(
            "SELECT source_url, etag, last_modified_hdr FROM calendars WHERE id=? AND kind='ics'",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?
    };
    let Some((url, etag, lm)) = row else { return Err("no such subscription".into()) };
    let result = fetch(&st.http, &url, etag.as_deref(), lm.as_deref()).await;
    let db = st.db.lock().unwrap();
    let outcome = match result {
        Ok(None) => Ok("Up to date".to_string()),
        Ok(Some((body, etag, lm))) => apply(&db, id, &body).map(|s| {
            let _ = db.execute(
                "UPDATE calendars SET etag=?, last_modified_hdr=? WHERE id=?",
                params![etag, lm, id],
            );
            s
        }),
        Err(e) => Err(e),
    };
    let (ok, msg) = match &outcome {
        Ok(m) => (true, m.clone()),
        Err(e) => (false, e.clone()),
    };
    let _ = db.execute(
        "UPDATE calendars SET last_sync_at=?, last_sync_ok=?, last_sync_status=? WHERE id=?",
        params![now(), ok, msg, id],
    );
    outcome
}

fn apply(db: &rusqlite::Connection, cal_id: i64, body: &str) -> Result<String, String> {
    let cal = ics::parse(body);
    let e = |e: rusqlite::Error| e.to_string();
    let tx = db.unchecked_transaction().map_err(e)?;
    let (mut added, mut updated, mut removed) = (0, 0, 0);
    let mut seen = HashSet::new();
    let ts = now();
    for ev in &cal.events {
        if ev.uid.is_empty() || !seen.insert(ev.uid.clone()) {
            continue;
        }
        let all_day = matches!(ev.start, When::Date(_));
        let end = match (&ev.start, &ev.end) {
            (When::Date(d), None) => When::Date(*d + chrono::Duration::days(1)),
            (s, None) => s.clone(),
            (_, Some(e)) => e.clone(),
        };
        let (title, kind) = classify(&ev.summary, &ev.start, &end);
        let course = ics::course_of(&ev.location);
        if let Some((code, label)) = &course {
            let n: i64 = tx.query_row("SELECT count(*) FROM courses", [], |r| r.get(0)).map_err(e)?;
            tx.execute(
                "INSERT OR IGNORE INTO courses (code, label, color) VALUES (?,?,?)",
                params![code, label, crate::db::PALETTE[n as usize % 8]],
            )
            .map_err(e)?;
        }
        let existing: Option<(i64, String, Option<String>, Option<String>)> = tx
            .query_row(
                "SELECT sequence, coalesce(source_modified,''), removed_at, start FROM events WHERE calendar_id=? AND uid=?",
                params![cal_id, ev.uid],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()
            .map_err(e)?;
        if let Some((seq, lm, rm, start)) = &existing {
            if *seq == ev.sequence
                && *lm == ev.last_modified
                && rm.is_none()
                && start.as_deref() == Some(when_str(&ev.start).as_str())
            {
                continue;
            }
        }
        tx.execute(
            "INSERT INTO events (calendar_id, uid, title, description, location, url, all_day, start, end,
               kind, course, rrule, sequence, source_modified, created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?15)
             ON CONFLICT(calendar_id, uid) DO UPDATE SET title=?3, description=?4, location=?5, url=?6,
               all_day=?7, start=?8, end=?9, kind=?10, course=?11, rrule=?12, sequence=?13,
               source_modified=?14, updated_at=?15, removed_at=NULL",
            params![
                cal_id, ev.uid, title, ev.description, ev.location, ics::first_url(&ev.description),
                all_day, when_str(&ev.start), when_str(&end), kind, course.map(|c| c.0), ev.rrule,
                ev.sequence, ev.last_modified, ts
            ],
        )
        .map_err(e)?;
        if existing.is_some() { updated += 1 } else { added += 1 }
    }
    {
        let mut q = tx
            .prepare("SELECT id, uid FROM events WHERE calendar_id=? AND removed_at IS NULL")
            .map_err(e)?;
        let gone: Vec<i64> = q
            .query_map([cal_id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))
            .map_err(e)?
            .filter_map(Result::ok)
            .filter(|(_, u)| !seen.contains(u))
            .map(|(id, _)| id)
            .collect();
        for id in gone {
            tx.execute("UPDATE events SET removed_at=? WHERE id=?", params![ts, id]).map_err(e)?;
            removed += 1;
        }
    }
    tx.commit().map_err(e)?;
    Ok(format!("{added} new · {updated} updated · {removed} removed"))
}

/// Background loop: every minute, sync calendars whose interval has elapsed.
pub async fn run(st: AppState) {
    let mut tick = tokio::time::interval(std::time::Duration::from_secs(60));
    loop {
        tick.tick().await;
        let due: Vec<i64> = {
            let db = st.db.lock().unwrap();
            let mut q = db
                .prepare(
                    "SELECT id FROM calendars WHERE kind='ics' AND enabled=1 AND (last_sync_at IS NULL OR
                     strftime('%s','now') - strftime('%s', last_sync_at) >= sync_interval_min*60
                       * (CASE WHEN last_sync_ok=0 THEN 2 ELSE 1 END))",
                )
                .unwrap();
            q.query_map([], |r| r.get(0)).unwrap().filter_map(Result::ok).collect()
        };
        for id in due {
            match sync_calendar(&st, id).await {
                Ok(m) => println!("sync calendar {id}: {m}"),
                Err(m) => eprintln!("sync calendar {id} failed: {m}"),
            }
        }
    }
}
