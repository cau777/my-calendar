mod db;
mod ics;
mod recur;
mod sync;

use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::{get, post, put},
    Json, Router,
};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Mutex<Connection>>,
    pub http: reqwest::Client,
}

pub fn now() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

type ApiResult<T = Json<Value>> = Result<T, (StatusCode, String)>;
fn bad<E: std::fmt::Display>(e: E) -> (StatusCode, String) {
    (StatusCode::BAD_REQUEST, e.to_string())
}
fn ise<E: std::fmt::Display>(e: E) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

#[derive(rust_embed::Embed)]
#[folder = "../web"]
struct Web;

fn main() {
    let mut port = std::env::var("PORT").unwrap_or_else(|_| "8080".into());
    let mut data = std::env::var("DATA_DIR").unwrap_or_else(|_| "data".into());
    let args: Vec<String> = std::env::args().collect();
    for w in args.windows(2) {
        match w[0].as_str() {
            "--port" => port = w[1].clone(),
            "--data-dir" => data = w[1].clone(),
            _ => {}
        }
    }
    std::fs::create_dir_all(&data).expect("create data dir");
    let conn = db::open(&std::path::Path::new(&data).join("calendar.db")).expect("open db");
    let st = AppState {
        db: Arc::new(Mutex::new(conn)),
        http: reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .user_agent("my-calendar/0.1")
            .build()
            .unwrap(),
    };
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async move {
        tokio::spawn(sync::run(st.clone()));
        let app = Router::new()
            .route("/api/events", get(list_events).post(create_event))
            .route("/api/events/:id", put(update_event).patch(update_event).delete(delete_event))
            .route("/api/events/:id/meta", put(put_meta))
            .route("/api/calendars", get(list_calendars).post(create_calendar))
            .route("/api/calendars/test", post(test_feed))
            .route("/api/calendars/:id", put(update_calendar).patch(update_calendar).delete(delete_calendar))
            .route("/api/calendars/:id/sync", post(sync_now))
            .route("/api/courses", get(list_courses))
            .route("/api/courses/:code", put(update_course).patch(update_course))
            .route("/api/health", get(|| async { "ok" }))
            .fallback(static_file)
            .with_state(st);
        let addr = format!("0.0.0.0:{port}");
        let l = tokio::net::TcpListener::bind(&addr).await.expect("bind");
        println!("my-calendar listening on http://{addr} (data: {data})");
        axum::serve(l, app)
            .with_graceful_shutdown(async {
                let _ = tokio::signal::ctrl_c().await;
            })
            .await
            .unwrap();
    });
}

async fn static_file(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let (file, name) = match Web::get(path) {
        Some(f) if !path.is_empty() => (f, path),
        _ if path.starts_with("api/") => return StatusCode::NOT_FOUND.into_response(),
        _ => (Web::get("index.html").unwrap(), "index.html"),
    };
    let mime = mime_guess::from_path(name).first_or_octet_stream();
    let cache = if name == "sw.js" || name.ends_with(".html") { "no-cache" } else { "public, max-age=300" };
    ([(header::CONTENT_TYPE, mime.as_ref().to_string()), (header::CACHE_CONTROL, cache.into())], file.data)
        .into_response()
}

// ---------- events ----------

#[derive(Deserialize)]
struct RangeQ {
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    tz: Option<String>,
}

struct Ev {
    v: Value,
    all_day: bool,
    start: String,
    end: String,
    rrule: Option<String>,
}

const EV_SQL: &str = "SELECT e.id, e.calendar_id, c.name, c.kind, e.title, e.description, e.location, e.url,
    e.all_day, e.start, e.end, e.kind, e.course, co.label, e.rrule,
    coalesce(m.done,0), coalesce(m.hidden,0), m.color_override, m.note,
    CASE WHEN m.color_override IS NOT NULL THEN m.color_override
         WHEN c.color_by_course=1 AND co.color IS NOT NULL THEN co.color ELSE c.color END
  FROM events e JOIN calendars c ON c.id=e.calendar_id
  LEFT JOIN event_meta m ON m.event_id=e.id
  LEFT JOIN courses co ON co.code=e.course
  WHERE e.removed_at IS NULL AND c.enabled=1";

fn ev_row(r: &Row) -> rusqlite::Result<Ev> {
    let all_day: bool = r.get(8)?;
    let start: String = r.get(9)?;
    let end: String = r.get(10)?;
    let rrule: Option<String> = r.get(14)?;
    Ok(Ev {
        v: json!({
            "id": r.get::<_, i64>(0)?, "calendar_id": r.get::<_, i64>(1)?, "calendar": r.get::<_, String>(2)?,
            "readonly": r.get::<_, String>(3)? == "ics",
            "title": r.get::<_, String>(4)?, "description": r.get::<_, String>(5)?,
            "location": r.get::<_, String>(6)?, "url": r.get::<_, Option<String>>(7)?,
            "all_day": all_day, "start": start, "end": end, "kind": r.get::<_, String>(11)?,
            "course": r.get::<_, Option<String>>(12)?, "course_label": r.get::<_, Option<String>>(13)?,
            "rrule": rrule, "done": r.get::<_, bool>(15)?, "hidden": r.get::<_, bool>(16)?,
            "color_override": r.get::<_, Option<String>>(17)?, "note": r.get::<_, Option<String>>(18)?,
            "color": r.get::<_, String>(19)?,
        }),
        all_day,
        start,
        end,
        rrule,
    })
}

fn parse_dt(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s).ok().map(|d| d.with_timezone(&Utc))
}
fn parse_d(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}

async fn list_events(State(st): State<AppState>, Query(q): Query<RangeQ>) -> ApiResult {
    let tz: chrono_tz::Tz = q.tz.as_deref().and_then(|t| t.parse().ok()).unwrap_or(chrono_tz::UTC);
    let db = st.db.lock().unwrap();
    let sql = format!(
        "{EV_SQL} AND coalesce(m.hidden,0)=0 AND coalesce(co.hidden,0)=0 AND e.start < ?1 AND (e.rrule IS NOT NULL OR e.end >= ?2)"
    );
    // Dates sort before same-day datetimes, so widen bounds a day for all-day rows.
    let hi = (q.to + Duration::days(1)).format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let lo = (q.from - Duration::days(1)).format("%Y-%m-%d").to_string();
    let mut stmt = db.prepare(&sql).map_err(ise)?;
    let rows: Vec<Ev> = stmt.query_map(params![hi, lo], ev_row).map_err(ise)?.filter_map(Result::ok).collect();
    // date range in viewer tz for all-day items
    let (dfrom, dto) = (q.from.with_timezone(&tz).date_naive(), q.to.with_timezone(&tz).date_naive());
    let mut out = Vec::new();
    for e in rows {
        let rule = e.rrule.as_deref().and_then(recur::parse);
        if e.all_day {
            let (Some(s), Some(en)) = (parse_d(&e.start), parse_d(&e.end)) else { continue };
            let len = en - s;
            let starts = match &rule {
                Some(r) => recur::dates(r, s, dto),
                None => vec![s],
            };
            for d in starts {
                if d <= dto && d + len > dfrom {
                    let mut v = e.v.clone();
                    v["start"] = json!(d.to_string());
                    v["end"] = json!((d + len).to_string());
                    out.push(v);
                }
            }
        } else {
            let (Some(s), Some(en)) = (parse_dt(&e.start), parse_dt(&e.end)) else { continue };
            let inst = match &rule {
                Some(r) => recur::expand_timed(r, s, en, tz, q.from, q.to),
                None if s < q.to && (en > q.from || s >= q.from) => vec![(s, en)],
                None => vec![],
            };
            for (a, b) in inst {
                let mut v = e.v.clone();
                v["start"] = json!(a.format("%Y-%m-%dT%H:%M:%SZ").to_string());
                v["end"] = json!(b.format("%Y-%m-%dT%H:%M:%SZ").to_string());
                out.push(v);
            }
        }
    }
    out.sort_by(|a, b| a["start"].as_str().cmp(&b["start"].as_str()));
    Ok(Json(json!(out)))
}

#[derive(Deserialize)]
struct EventIn {
    calendar_id: Option<i64>,
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    location: String,
    #[serde(default)]
    all_day: bool,
    start: String,
    end: String,
    rrule: Option<String>,
    color: Option<String>,
}

fn validate(e: &EventIn) -> Result<(String, String), (StatusCode, String)> {
    if e.title.trim().is_empty() {
        return Err(bad("title required"));
    }
    if let Some(r) = &e.rrule {
        recur::parse(r).ok_or_else(|| bad("unsupported rrule"))?;
    }
    if e.all_day {
        let s = parse_d(&e.start).ok_or_else(|| bad("start must be YYYY-MM-DD"))?;
        let en = parse_d(&e.end).ok_or_else(|| bad("end must be YYYY-MM-DD"))?;
        let en = if en <= s { s + Duration::days(1) } else { en };
        Ok((s.to_string(), en.to_string()))
    } else {
        let s = parse_dt(&e.start).ok_or_else(|| bad("start must be RFC3339"))?;
        let en = parse_dt(&e.end).ok_or_else(|| bad("end must be RFC3339"))?;
        let en = en.max(s);
        let f = |d: DateTime<Utc>| d.format("%Y-%m-%dT%H:%M:%SZ").to_string();
        Ok((f(s), f(en)))
    }
}

fn set_color(db: &Connection, id: i64, color: &Option<String>) -> rusqlite::Result<()> {
    db.execute(
        "INSERT INTO event_meta (event_id, color_override) VALUES (?1, ?2)
         ON CONFLICT(event_id) DO UPDATE SET color_override=?2",
        params![id, color],
    )?;
    Ok(())
}

fn get_event(db: &Connection, id: i64) -> ApiResult {
    let sql = format!("{EV_SQL} AND e.id=?");
    db.query_row(&sql, [id], ev_row).optional().map_err(ise)?
        .map(|e| Json(e.v))
        .ok_or((StatusCode::NOT_FOUND, "not found".into()))
}

async fn create_event(State(st): State<AppState>, Json(e): Json<EventIn>) -> ApiResult {
    let (s, en) = validate(&e)?;
    let db = st.db.lock().unwrap();
    let cal: i64 = match e.calendar_id {
        Some(c) => c,
        None => db.query_row("SELECT id FROM calendars WHERE kind='local' ORDER BY id LIMIT 1", [], |r| r.get(0)).map_err(ise)?,
    };
    let kind: Option<String> = db.query_row("SELECT kind FROM calendars WHERE id=?", [cal], |r| r.get(0)).optional().map_err(ise)?;
    if kind.as_deref() != Some("local") {
        return Err(bad("calendar is not editable"));
    }
    let uid = format!("{}-{}@my-calendar", Utc::now().timestamp_nanos_opt().unwrap_or(0), std::process::id());
    let ts = now();
    db.execute(
        "INSERT INTO events (calendar_id, uid, title, description, location, all_day, start, end, rrule, created_at, updated_at)
         VALUES (?,?,?,?,?,?,?,?,?,?,?)",
        params![cal, uid, e.title.trim(), e.description, e.location, e.all_day, s, en, e.rrule, ts, ts],
    ).map_err(ise)?;
    let id = db.last_insert_rowid();
    if e.color.is_some() {
        set_color(&db, id, &e.color).map_err(ise)?;
    }
    get_event(&db, id)
}

fn ensure_local(db: &Connection, id: i64) -> Result<(), (StatusCode, String)> {
    let k: Option<String> = db
        .query_row("SELECT c.kind FROM events e JOIN calendars c ON c.id=e.calendar_id WHERE e.id=?", [id], |r| r.get(0))
        .optional().map_err(ise)?;
    match k.as_deref() {
        None => Err((StatusCode::NOT_FOUND, "not found".into())),
        Some("local") => Ok(()),
        _ => Err((StatusCode::FORBIDDEN, "subscribed events are read-only".into())),
    }
}

async fn update_event(State(st): State<AppState>, Path(id): Path<i64>, Json(e): Json<EventIn>) -> ApiResult {
    let (s, en) = validate(&e)?;
    let db = st.db.lock().unwrap();
    ensure_local(&db, id)?;
    db.execute(
        "UPDATE events SET title=?, description=?, location=?, all_day=?, start=?, end=?, rrule=?, updated_at=?,
           calendar_id=coalesce((SELECT id FROM calendars WHERE id=? AND kind='local'), calendar_id) WHERE id=?",
        params![e.title.trim(), e.description, e.location, e.all_day, s, en, e.rrule, now(), e.calendar_id, id],
    ).map_err(ise)?;
    set_color(&db, id, &e.color).map_err(ise)?;
    get_event(&db, id)
}

async fn delete_event(State(st): State<AppState>, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    let db = st.db.lock().unwrap();
    ensure_local(&db, id)?;
    db.execute("DELETE FROM events WHERE id=?", [id]).map_err(ise)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct MetaIn {
    done: Option<bool>,
    hidden: Option<bool>,
    color_override: Option<Option<String>>,
    note: Option<Option<String>>,
}

async fn put_meta(State(st): State<AppState>, Path(id): Path<i64>, Json(m): Json<MetaIn>) -> ApiResult {
    let db = st.db.lock().unwrap();
    db.execute("INSERT OR IGNORE INTO event_meta (event_id) SELECT id FROM events WHERE id=?", [id]).map_err(ise)?;
    if let Some(v) = m.done { db.execute("UPDATE event_meta SET done=? WHERE event_id=?", params![v, id]).map_err(ise)?; }
    if let Some(v) = m.hidden { db.execute("UPDATE event_meta SET hidden=? WHERE event_id=?", params![v, id]).map_err(ise)?; }
    if let Some(v) = m.color_override { db.execute("UPDATE event_meta SET color_override=? WHERE event_id=?", params![v, id]).map_err(ise)?; }
    if let Some(v) = m.note {
        let v = v.filter(|s| !s.trim().is_empty());
        db.execute("UPDATE event_meta SET note=? WHERE event_id=?", params![v, id]).map_err(ise)?;
    }
    get_event(&db, id)
}

// ---------- calendars ----------

/// Hide secrets in feed URLs: keep the first 4 chars of each query value.
fn mask(url: &str) -> String {
    let Some((base, q)) = url.split_once('?') else { return url.to_string() };
    let parts: Vec<String> = q
        .split('&')
        .map(|kv| match kv.split_once('=') {
            Some((k, v)) if v.len() > 4 => format!("{k}={}{}", &v[..4], "•".repeat(10)),
            _ => kv.to_string(),
        })
        .collect();
    format!("{base}?{}", parts.join("&"))
}

async fn list_calendars(State(st): State<AppState>) -> ApiResult {
    let db = st.db.lock().unwrap();
    let mut q = db.prepare(
        "SELECT c.id, c.name, c.color, c.kind, c.source_url, c.sync_interval_min, c.color_by_course,
           c.last_sync_at, c.last_sync_ok, c.last_sync_status, c.enabled,
           (SELECT count(*) FROM events e WHERE e.calendar_id=c.id AND e.removed_at IS NULL)
         FROM calendars c ORDER BY c.kind DESC, c.id",
    ).map_err(ise)?;
    let rows: Vec<Value> = q.query_map([], |r| {
        Ok(json!({
            "id": r.get::<_, i64>(0)?, "name": r.get::<_, String>(1)?, "color": r.get::<_, String>(2)?,
            "kind": r.get::<_, String>(3)?, "source_url": r.get::<_, Option<String>>(4)?.map(|u| mask(&u)),
            "sync_interval_min": r.get::<_, i64>(5)?, "color_by_course": r.get::<_, bool>(6)?,
            "last_sync_at": r.get::<_, Option<String>>(7)?, "last_sync_ok": r.get::<_, Option<bool>>(8)?,
            "last_sync_status": r.get::<_, Option<String>>(9)?, "enabled": r.get::<_, bool>(10)?,
            "event_count": r.get::<_, i64>(11)?,
        }))
    }).map_err(ise)?.filter_map(Result::ok).collect();
    Ok(Json(json!(rows)))
}

#[derive(Deserialize)]
struct CalIn {
    name: Option<String>,
    color: Option<String>,
    kind: Option<String>,
    source_url: Option<String>,
    sync_interval_min: Option<i64>,
    color_by_course: Option<bool>,
    enabled: Option<bool>,
}

fn real_url(u: &Option<String>) -> Option<String> {
    u.as_ref().map(|s| s.trim().to_string()).filter(|s| !s.is_empty() && !s.contains('•'))
        .map(|s| s.replacen("webcal://", "https://", 1))
}

async fn create_calendar(State(st): State<AppState>, Json(c): Json<CalIn>) -> ApiResult {
    let kind = c.kind.clone().unwrap_or_else(|| if c.source_url.is_some() { "ics".into() } else { "local".into() });
    let url = real_url(&c.source_url);
    if kind == "ics" && url.is_none() {
        return Err(bad("source_url required"));
    }
    let id = {
        let db = st.db.lock().unwrap();
        db.execute(
            "INSERT INTO calendars (name, color, kind, source_url, sync_interval_min, color_by_course) VALUES (?,?,?,?,?,?)",
            params![
                c.name.unwrap_or_else(|| "Calendar".into()), c.color.unwrap_or_else(|| "#8ea2ff".into()), kind,
                url, c.sync_interval_min.unwrap_or(30).clamp(5, 1440), c.color_by_course.unwrap_or(true)
            ],
        ).map_err(bad)?;
        db.last_insert_rowid()
    };
    if kind == "ics" {
        let _ = sync::sync_calendar(&st, id).await;
    }
    Ok(Json(json!({ "id": id })))
}

async fn update_calendar(State(st): State<AppState>, Path(id): Path<i64>, Json(c): Json<CalIn>) -> ApiResult {
    let url = real_url(&c.source_url);
    {
        let db = st.db.lock().unwrap();
        let n = db.execute(
            "UPDATE calendars SET name=coalesce(?,name), color=coalesce(?,color), sync_interval_min=coalesce(?,sync_interval_min),
               color_by_course=coalesce(?,color_by_course), enabled=coalesce(?,enabled) WHERE id=?",
            params![c.name, c.color, c.sync_interval_min.map(|v| v.clamp(5, 1440)), c.color_by_course, c.enabled, id],
        ).map_err(ise)?;
        if n == 0 {
            return Err((StatusCode::NOT_FOUND, "not found".into()));
        }
        if url.is_some() {
            db.execute("UPDATE calendars SET source_url=?, etag=NULL, last_modified_hdr=NULL WHERE id=? AND kind='ics'", params![url, id]).map_err(ise)?;
        }
    }
    if url.is_some() {
        let _ = sync::sync_calendar(&st, id).await;
    }
    Ok(Json(json!({ "id": id })))
}

async fn delete_calendar(State(st): State<AppState>, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    let db = st.db.lock().unwrap();
    let locals: i64 = db.query_row("SELECT count(*) FROM calendars WHERE kind='local' AND id<>?", [id], |r| r.get(0)).map_err(ise)?;
    let kind: Option<String> = db.query_row("SELECT kind FROM calendars WHERE id=?", [id], |r| r.get(0)).optional().map_err(ise)?;
    if kind.as_deref() == Some("local") && locals == 0 {
        return Err(bad("keep at least one local calendar"));
    }
    db.execute("DELETE FROM calendars WHERE id=?", [id]).map_err(ise)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn sync_now(State(st): State<AppState>, Path(id): Path<i64>) -> ApiResult {
    match sync::sync_calendar(&st, id).await {
        Ok(m) => Ok(Json(json!({ "ok": true, "status": m }))),
        Err(m) => Ok(Json(json!({ "ok": false, "status": m }))),
    }
}

#[derive(Deserialize)]
struct TestIn {
    url: Option<String>,
    id: Option<i64>,
}

async fn test_feed(State(st): State<AppState>, Json(t): Json<TestIn>) -> ApiResult {
    let url = match (real_url(&t.url), t.id) {
        (Some(u), _) => u,
        (None, Some(id)) => {
            let db = st.db.lock().unwrap();
            db.query_row("SELECT source_url FROM calendars WHERE id=?", [id], |r| r.get::<_, Option<String>>(0))
                .optional().map_err(ise)?.flatten().ok_or_else(|| bad("no url"))?
        }
        _ => return Err(bad("url required")),
    };
    match sync::fetch(&st.http, &url, None, None).await {
        Ok(Some((body, _, _))) => {
            let cal = ics::parse(&body);
            let mut courses: Vec<String> = cal.events.iter().filter_map(|e| ics::course_of(&e.location).map(|c| c.0)).collect();
            courses.sort();
            courses.dedup();
            Ok(Json(json!({ "ok": true, "name": cal.name, "events": cal.events.len(), "courses": courses.len() })))
        }
        Ok(None) => Ok(Json(json!({ "ok": true }))),
        Err(e) => Ok(Json(json!({ "ok": false, "error": e }))),
    }
}

// ---------- courses ----------

async fn list_courses(State(st): State<AppState>) -> ApiResult {
    let db = st.db.lock().unwrap();
    let mut q = db.prepare("SELECT code, label, color, hidden FROM courses ORDER BY code").map_err(ise)?;
    let rows: Vec<Value> = q.query_map([], |r| Ok(json!({
        "code": r.get::<_, String>(0)?, "label": r.get::<_, String>(1)?,
        "color": r.get::<_, String>(2)?, "hidden": r.get::<_, bool>(3)?,
    }))).map_err(ise)?.filter_map(Result::ok).collect();
    Ok(Json(json!(rows)))
}

#[derive(Deserialize)]
struct CourseIn {
    label: Option<String>,
    color: Option<String>,
    hidden: Option<bool>,
}

async fn update_course(State(st): State<AppState>, Path(code): Path<String>, Json(c): Json<CourseIn>) -> ApiResult {
    let db = st.db.lock().unwrap();
    db.execute(
        "UPDATE courses SET label=coalesce(?,label), color=coalesce(?,color), hidden=coalesce(?,hidden) WHERE code=?",
        params![c.label, c.color, c.hidden, code],
    ).map_err(ise)?;
    Ok(Json(json!({ "ok": true })))
}
