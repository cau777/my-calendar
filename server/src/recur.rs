//! Small RRULE subset: FREQ=DAILY|WEEKLY|MONTHLY|YEARLY, INTERVAL, COUNT, UNTIL, BYDAY (weekly).
//! Expansion happens in the viewer's time zone so "every Monday 7:00" survives DST.
use chrono::{DateTime, Datelike, Duration, NaiveDate, TimeZone, Utc, Weekday};
use chrono_tz::Tz;

pub struct Rule {
    freq: String,
    interval: i64,
    count: Option<usize>,
    until: Option<NaiveDate>,
    byday: Vec<Weekday>,
}

pub fn parse(s: &str) -> Option<Rule> {
    let mut r = Rule { freq: String::new(), interval: 1, count: None, until: None, byday: vec![] };
    for part in s.trim_start_matches("RRULE:").split(';') {
        let (k, v) = part.split_once('=')?;
        match k {
            "FREQ" => r.freq = v.to_string(),
            "INTERVAL" => r.interval = v.parse().ok().filter(|n| *n > 0)?,
            "COUNT" => r.count = v.parse().ok(),
            "UNTIL" => r.until = NaiveDate::parse_from_str(&v[..8.min(v.len())], "%Y%m%d").ok(),
            "BYDAY" => {
                r.byday = v
                    .split(',')
                    .filter_map(|d| match &d[d.len().saturating_sub(2)..] {
                        "MO" => Some(Weekday::Mon), "TU" => Some(Weekday::Tue),
                        "WE" => Some(Weekday::Wed), "TH" => Some(Weekday::Thu),
                        "FR" => Some(Weekday::Fri), "SA" => Some(Weekday::Sat),
                        "SU" => Some(Weekday::Sun), _ => None,
                    })
                    .collect()
            }
            _ => {}
        }
    }
    matches!(r.freq.as_str(), "DAILY" | "WEEKLY" | "MONTHLY" | "YEARLY").then_some(r)
}

fn add_months(d: NaiveDate, m: i64) -> Option<NaiveDate> {
    let total = d.year() as i64 * 12 + d.month0() as i64 + m;
    NaiveDate::from_ymd_opt((total / 12) as i32, (total % 12) as u32 + 1, d.day())
}

/// Occurrence start dates (local) for a series starting on `start`, up to `last` inclusive.
pub fn dates(rule: &Rule, start: NaiveDate, last: NaiveDate) -> Vec<NaiveDate> {
    let mut out = Vec::new();
    let stop = rule.until.map_or(last, |u| u.min(last));
    let max = rule.count.unwrap_or(usize::MAX);
    let mut n = 0usize;
    if rule.freq == "WEEKLY" && !rule.byday.is_empty() {
        let week0 = start - Duration::days(start.weekday().num_days_from_sunday() as i64);
        let mut d = start;
        while d <= stop && n < max {
            let wk = (d - week0).num_days() / 7;
            if wk % rule.interval == 0 && rule.byday.contains(&d.weekday()) {
                out.push(d);
                n += 1;
            }
            d += Duration::days(1);
        }
        return out;
    }
    let mut i = 0i64;
    loop {
        let d = match rule.freq.as_str() {
            "DAILY" => Some(start + Duration::days(i * rule.interval)),
            "WEEKLY" => Some(start + Duration::days(7 * i * rule.interval)),
            "MONTHLY" => add_months(start, i * rule.interval),
            _ => add_months(start, 12 * i * rule.interval),
        };
        i += 1;
        if i > 20000 { break; }
        let Some(d) = d else { continue }; // e.g. Feb 31: skip
        if d > stop || n >= max { break; }
        out.push(d);
        n += 1;
    }
    out
}

/// Expand a timed event: returns (start, end) UTC pairs overlapping [from, to).
pub fn expand_timed(
    rule: &Rule, start: DateTime<Utc>, end: DateTime<Utc>, tz: Tz, from: DateTime<Utc>, to: DateTime<Utc>,
) -> Vec<(DateTime<Utc>, DateTime<Utc>)> {
    let dur = end - start;
    let ls = start.with_timezone(&tz);
    let last = to.with_timezone(&tz).date_naive();
    dates(rule, ls.date_naive(), last)
        .into_iter()
        .filter_map(|d| tz.from_local_datetime(&d.and_time(ls.time())).earliest())
        .map(|s| (s.with_timezone(&Utc), s.with_timezone(&Utc) + dur))
        .filter(|(s, e)| *s < to && (*e > from || *s >= from))
        .collect()
}
