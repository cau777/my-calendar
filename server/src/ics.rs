//! Minimal iCalendar (RFC 5545) reader: enough for D2L-style feeds.
use chrono::{DateTime, NaiveDate, NaiveDateTime, TimeZone, Utc};

#[derive(Debug, Clone, PartialEq)]
pub enum When {
    Date(NaiveDate),
    Time(DateTime<Utc>),
}

#[derive(Debug, Clone)]
pub struct IcsEvent {
    pub uid: String,
    pub summary: String,
    pub description: String,
    pub location: String,
    pub start: When,
    pub end: Option<When>,
    pub sequence: i64,
    pub last_modified: String,
    pub rrule: Option<String>,
}

pub struct IcsCalendar {
    pub name: Option<String>,
    pub events: Vec<IcsEvent>,
}

fn unfold(src: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in src.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if (line.starts_with(' ') || line.starts_with('\t')) && !out.is_empty() {
            out.last_mut().unwrap().push_str(&line[1..]);
        } else if !line.is_empty() {
            out.push(line.to_string());
        }
    }
    out
}

fn unescape(v: &str) -> String {
    let mut s = String::with_capacity(v.len());
    let mut it = v.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('n') | Some('N') => s.push('\n'),
                Some(o) => s.push(o),
                None => {}
            }
        } else {
            s.push(c);
        }
    }
    s
}

/// Split "NAME;P=1;Q=2:value" into (NAME, params, value).
fn split_prop(line: &str) -> Option<(String, Vec<(String, String)>, &str)> {
    let mut in_q = false;
    let mut colon = None;
    for (i, c) in line.char_indices() {
        match c {
            '"' => in_q = !in_q,
            ':' if !in_q => {
                colon = Some(i);
                break;
            }
            _ => {}
        }
    }
    let colon = colon?;
    let (head, val) = (&line[..colon], &line[colon + 1..]);
    let mut parts = head.split(';');
    let name = parts.next()?.to_ascii_uppercase();
    let params = parts
        .filter_map(|p| p.split_once('='))
        .map(|(k, v)| (k.to_ascii_uppercase(), v.trim_matches('"').to_string()))
        .collect();
    Some((name, params, val))
}

pub fn parse_when(params: &[(String, String)], v: &str) -> Option<When> {
    let v = v.trim();
    let is_date = params.iter().any(|(k, x)| k == "VALUE" && x == "DATE") || v.len() == 8;
    if is_date {
        return NaiveDate::parse_from_str(&v[..8.min(v.len())], "%Y%m%d").ok().map(When::Date);
    }
    if let Some(z) = v.strip_suffix('Z') {
        let n = NaiveDateTime::parse_from_str(z, "%Y%m%dT%H%M%S").ok()?;
        return Some(When::Time(Utc.from_utc_datetime(&n)));
    }
    let n = NaiveDateTime::parse_from_str(v, "%Y%m%dT%H%M%S").ok()?;
    let tz = params
        .iter()
        .find(|(k, _)| k == "TZID")
        .and_then(|(_, t)| t.parse::<chrono_tz::Tz>().ok());
    Some(When::Time(match tz {
        Some(tz) => tz.from_local_datetime(&n).earliest()?.with_timezone(&Utc),
        None => Utc.from_utc_datetime(&n), // floating: treat as UTC
    }))
}

pub fn parse(src: &str) -> IcsCalendar {
    let mut cal = IcsCalendar { name: None, events: Vec::new() };
    let mut cur: Option<(Option<IcsEvent>, Vec<(String, Vec<(String, String)>, String)>)> = None;
    let mut depth_other = 0;
    for line in unfold(src) {
        let Some((name, params, val)) = split_prop(&line) else { continue };
        match (name.as_str(), val) {
            ("BEGIN", "VEVENT") => cur = Some((None, Vec::new())),
            ("END", "VEVENT") => {
                if let Some((_, props)) = cur.take() {
                    if let Some(e) = build(props) {
                        cal.events.push(e);
                    }
                }
            }
            ("BEGIN", _) if cur.is_some() => depth_other += 1, // VALARM etc.
            ("END", _) if cur.is_some() => depth_other -= 1,
            ("X-WR-CALNAME", v) if cur.is_none() => cal.name = Some(unescape(v)),
            _ => {
                if let Some((_, props)) = cur.as_mut() {
                    if depth_other == 0 {
                        props.push((name, params, val.to_string()));
                    }
                }
            }
        }
    }
    cal
}

fn build(props: Vec<(String, Vec<(String, String)>, String)>) -> Option<IcsEvent> {
    let get = |n: &str| props.iter().find(|p| p.0 == n);
    let txt = |n: &str| get(n).map(|p| unescape(&p.2)).unwrap_or_default();
    let (_, sp, sv) = get("DTSTART")?;
    let start = parse_when(sp, sv)?;
    let end = get("DTEND").and_then(|(_, p, v)| parse_when(p, v));
    Some(IcsEvent {
        uid: txt("UID"),
        summary: txt("SUMMARY"),
        description: txt("DESCRIPTION"),
        location: txt("LOCATION"),
        start,
        end,
        sequence: txt("SEQUENCE").parse().unwrap_or(0),
        last_modified: txt("LAST-MODIFIED"),
        rrule: get("RRULE").map(|p| p.2.clone()),
    })
}

/// "CPSC-2190-M01 - Theoretical Founds" -> ("CPSC-2190", "Theoretical Founds")
pub fn course_of(location: &str) -> Option<(String, String)> {
    let (code, rest) = location.split_once(" - ").unwrap_or((location, ""));
    let mut p = code.split('-');
    let subj = p.next()?;
    let num = p.next()?;
    if (2..=5).contains(&subj.len())
        && subj.chars().all(|c| c.is_ascii_uppercase())
        && num.len() == 4
        && num.chars().all(|c| c.is_ascii_digit())
    {
        Some((format!("{subj}-{num}"), rest.trim().to_string()))
    } else {
        None
    }
}

pub fn first_url(text: &str) -> Option<String> {
    // prefer the "View event - <url>" link D2L adds
    let pick = text.find("View event - ").map(|i| i + 13).or_else(|| text.find("https://"))?;
    let s = &text[pick..];
    let end = s.find(|c: char| c.is_whitespace()).unwrap_or(s.len());
    let u = &s[..end];
    u.starts_with("http").then(|| u.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_feed() {
        let src = "BEGIN:VCALENDAR\r\nX-WR-CALNAME:All Courses\r\nBEGIN:VEVENT\r\nSUMMARY:Quiz 1 - Due\r\nLOCATION:CPSC-2190-M01 - Theory\r\nDESCRIPTION:a\\nView event - htt\r\n\tps://x.y/z\\n\r\nDTSTART:20260916T233000Z\r\nDTEND:20260916T233000Z\r\nUID:1@x\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nSUMMARY:Exam\r\nDTSTART;VALUE=DATE:20250212\r\nUID:2@x\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let c = parse(src);
        assert_eq!(c.name.as_deref(), Some("All Courses"));
        assert_eq!(c.events.len(), 2);
        assert_eq!(first_url(&c.events[0].description).as_deref(), Some("https://x.y/z"));
        assert_eq!(course_of(&c.events[0].location).unwrap().0, "CPSC-2190");
        assert!(matches!(c.events[1].start, When::Date(_)));
    }
}
