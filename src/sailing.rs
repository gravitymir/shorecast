//! Sailing events from the Royal Cork Yacht Club calendar (Crosshaven,
//! next to Roches Point), saved locally. The club's calendar mixes racing
//! and training with dinners and bank holidays; only time on the water is
//! kept, sorted into racing, training and cruising.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use chrono::{DateTime, NaiveDate, NaiveDateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The Events Calendar (WordPress plugin) REST API on royalcork.com.
const EVENTS_URL: &str = "https://www.royalcork.com/wp-json/tribe/events/v1/events";
const REFRESH_EVERY: TimeDelta = TimeDelta::days(1);
const CHECK_EVERY: Duration = Duration::from_secs(60 * 60);
/// How far ahead the calendar is asked for.
const AHEAD: TimeDelta = TimeDelta::days(540);
/// The first download goes back to here; later ones re-read a month back.
const SINCE: (i32, u32, u32) = (2025, 1, 1);

/// One event as the club lists it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub title: String,
    pub url: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    /// The club gives no times, only the date.
    pub all_day: bool,
    /// Boat classes and the like ("Keelboats", "National18", "General").
    pub categories: Vec<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Saved {
    events: BTreeMap<u64, Event>,
    fetched: Option<DateTime<Utc>>,
}

pub struct Store {
    http: reqwest::Client,
    file: PathBuf,
    saved: RwLock<Saved>,
}

impl Store {
    pub fn open(dir: PathBuf) -> Self {
        let file = dir.join("royal-cork.json");
        let saved = std::fs::read_to_string(&file)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Store {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(60))
                .user_agent("ShoreCast (coastal shoot planner)")
                .build()
                .expect("HTTP client"),
            file,
            saved: RwLock::new(saved),
        }
    }

    /// Keeps the saved calendar fresh, forever.
    pub async fn run(self: Arc<Self>) {
        loop {
            let fetched = self.saved.read().unwrap().fetched;
            if fetched.is_none_or(|t| Utc::now() - t >= REFRESH_EVERY) {
                let now = Utc::now();
                let from = match fetched {
                    Some(t) => (t - TimeDelta::days(31)).date_naive(),
                    None => NaiveDate::from_ymd_opt(SINCE.0, SINCE.1, SINCE.2).unwrap(),
                };
                match self.fetch(from, (now + AHEAD).date_naive()).await {
                    Ok(events) => {
                        println!("Sailing: {} events from Royal Cork Yacht Club", events.len());
                        {
                            let mut s = self.saved.write().unwrap();
                            s.events.extend(events);
                            s.fetched = Some(now);
                        }
                        if let Err(e) = self.save().await {
                            eprintln!("cannot save {}: {e}", self.file.display());
                        }
                    }
                    Err(e) => eprintln!("sailing: {e}"),
                }
            }
            tokio::time::sleep(CHECK_EVERY).await;
        }
    }

    /// Every event between the two dates, 50 to a page.
    async fn fetch(&self, from: NaiveDate, to: NaiveDate) -> Result<Vec<(u64, Event)>, String> {
        let (from, to) = (from.to_string(), to.to_string());
        let mut out = Vec::new();
        let mut page = 1;
        loop {
            let page_s = page.to_string();
            let body = self
                .http
                .get(EVENTS_URL)
                .query(&[
                    ("start_date", from.as_str()),
                    ("end_date", to.as_str()),
                    ("per_page", "50"),
                    ("page", page_s.as_str()),
                ])
                .send()
                .await
                .and_then(|r| r.error_for_status())
                .map_err(|e| e.to_string())?
                .text()
                .await
                .map_err(|e| e.to_string())?;
            let json: Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
            out.extend(json["events"].as_array().into_iter().flatten().filter_map(event));
            let pages = json["total_pages"].as_u64().unwrap_or(1);
            if page >= pages {
                return Ok(out);
            }
            page += 1;
        }
    }

    async fn save(&self) -> std::io::Result<()> {
        let json = serde_json::to_string(&*self.saved.read().unwrap())?;
        if let Some(dir) = self.file.parent() {
            tokio::fs::create_dir_all(dir).await?;
        }
        let tmp = self.file.with_extension("json.tmp");
        tokio::fs::write(&tmp, json).await?;
        tokio::fs::rename(&tmp, &self.file).await
    }

    /// Time on the water between `start` and `end`.
    pub fn sessions(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> Vec<Session> {
        let s = self.saved.read().unwrap();
        sessions(s.events.values(), start, end)
    }
}

fn event(e: &Value) -> Option<(u64, Event)> {
    let utc = |key: &str| {
        NaiveDateTime::parse_from_str(e[key].as_str()?, "%Y-%m-%d %H:%M:%S")
            .ok()
            .map(|t| t.and_utc())
    };
    let ev = Event {
        title: decode_html(e["title"].as_str()?),
        url: e["url"].as_str().unwrap_or_default().to_string(),
        start: utc("utc_start_date")?,
        end: utc("utc_end_date")?,
        all_day: e["all_day"].as_bool().unwrap_or(false),
        categories: e["categories"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|c| c["name"].as_str().map(decode_html))
            .collect(),
    };
    Some((e["id"].as_u64()?, ev))
}

/// WordPress titles come HTML-escaped ("Junior &#038; Youth", "&#8211;").
fn decode_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let Some(semi) = rest.find(';').filter(|&i| i <= 10) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..semi];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            _ => entity
                .strip_prefix("#x")
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| entity.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// Races, regattas, championships, leagues: the fleets worth filming.
    Racing,
    /// Courses, camps, club sailing: small boats near the club.
    Training,
    /// Club cruises, usually heading away along the coast.
    Cruising,
}

/// Club life ashore, never on the water.
const ASHORE: &[&str] = &[
    "dinner", "lunch", "supper", "party", "market", "holiday", "agm", "mothers day",
    "fathers day", "christmas", "new year", "easter", "communion", "confirmation", "coffee",
    "talk", "presentation", "gathering", "culture night", "jumble", "bbq", "welcome",
    "safety", "open day", "darkness into light", "st patrick", "st brigid", "good friday",
    "c.a.d.s", "scora",
];

const RACING: &[&str] = &[
    "race", "racing", "regatta", "week", "league", "championship", "nationals", "munsters",
    "southerns", "worlds", "cup", "traveller", "cock of the north", "series",
];

const TRAINING: &[&str] = &["sailing", "cadet", "camp", "pathway", "course", "clinic"];

/// Racing, training or cruising, or None for anything ashore.
pub fn kind(e: &Event) -> Option<Kind> {
    let title = e.title.to_lowercase();
    if ASHORE.iter().any(|w| title.contains(w)) {
        return None;
    }
    let cats: Vec<String> = e.categories.iter().map(|c| c.to_lowercase()).collect();
    if title.contains("cruise") || cats.iter().any(|c| c == "cruising") {
        Some(Kind::Cruising)
    } else if RACING.iter().any(|w| title.contains(w)) {
        Some(Kind::Racing)
    } else if TRAINING.iter().any(|w| title.contains(w)) {
        Some(Kind::Training)
    } else if cats.iter().any(|c| !matches!(c.as_str(), "all" | "general" | "bar & catering")) {
        // Filed under a boat class (1720s, Optimist, Topper…): a fleet event.
        Some(Kind::Racing)
    } else {
        None
    }
}

/// An event on the water, for the day page.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Session {
    pub title: String,
    pub kind: Kind,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub all_day: bool,
    pub url: String,
    pub categories: Vec<String>,
}

fn sessions<'a>(
    events: impl Iterator<Item = &'a Event>,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Vec<Session> {
    let mut out: Vec<Session> = events
        .filter(|e| e.start < end && start < e.end)
        .filter_map(|e| {
            Some(Session {
                kind: kind(e)?,
                title: e.title.clone(),
                start: e.start,
                end: e.end,
                all_day: e.all_day,
                url: e.url.clone(),
                categories: e.categories.clone(),
            })
        })
        .collect();
    // The club sometimes lists the same session twice.
    out.sort_by(|a, b| (a.start, &a.title).cmp(&(b.start, &b.title)));
    out.dedup_by(|a, b| a.title == b.title && a.start == b.start);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use serde_json::json;

    fn ev(title: &str, cats: &[&str]) -> Event {
        Event {
            title: title.into(),
            url: String::new(),
            start: Utc.with_ymd_and_hms(2026, 7, 5, 23, 0, 0).unwrap(),
            end: Utc.with_ymd_and_hms(2026, 7, 6, 22, 59, 59).unwrap(),
            all_day: true,
            categories: cats.iter().map(|c| c.to_string()).collect(),
        }
    }

    #[test]
    fn parses_api_event() {
        let e = json!({
            "id": 71986, "title": "Junior &#038; Youth &#8211; Pathway", "url": "u",
            "all_day": true, "utc_start_date": "2026-07-05 23:00:00",
            "utc_end_date": "2026-07-06 22:59:59",
            "categories": [{"name": "Bar &amp; Catering"}]
        });
        let (id, e) = event(&e).unwrap();
        assert_eq!(id, 71986);
        assert_eq!(e.title, "Junior & Youth – Pathway");
        assert_eq!(e.start, Utc.with_ymd_and_hms(2026, 7, 5, 23, 0, 0).unwrap());
        assert_eq!(e.categories, ["Bar & Catering"]);
    }

    #[test]
    fn decodes_entities() {
        assert_eq!(decode_html("N18&#8217;s &amp; &#x41; & co"), "N18’s & A & co");
        assert_eq!(decode_html("no entities"), "no entities");
    }

    #[test]
    fn sorts_events_by_kind() {
        assert_eq!(kind(&ev("CORK WEEK 2026", &[])), Some(Kind::Racing));
        assert_eq!(kind(&ev("1720 Southerns", &["1720s"])), Some(Kind::Racing));
        assert_eq!(kind(&ev("Optimist Nationals", &["Optimist"])), Some(Kind::Racing));
        assert_eq!(kind(&ev("At Home Regatta", &["All"])), Some(Kind::Racing));
        assert_eq!(kind(&ev("Topper Traveller Event", &["Topper"])), Some(Kind::Racing));
        assert_eq!(kind(&ev("Rankin Worlds", &["All"])), Some(Kind::Racing));
        assert_eq!(kind(&ev("Junior Sailing Courses", &[])), Some(Kind::Training));
        assert_eq!(kind(&ev("Cadet Club May 2026", &[])), Some(Kind::Training));
        assert_eq!(kind(&ev("Royal Cork Shannon Cruise", &["Cruising"])), Some(Kind::Cruising));
        assert_eq!(kind(&ev("N18's Laying Up Dinner", &["National18"])), None);
        assert_eq!(kind(&ev("Yacht Safety Course", &["General"])), None);
        assert_eq!(kind(&ev("End of Season Party", &["All"])), None);
        assert_eq!(kind(&ev("Bank Holiday", &["All"])), None);
        assert_eq!(kind(&ev("History talk", &["General"])), None);
    }

    #[test]
    fn sessions_for_a_day() {
        let events = [
            ev("CORK WEEK 2026", &[]),
            ev("CORK WEEK 2026", &[]),
            ev("ladies Day Lunch", &["All"]),
        ];
        let day = |d| Utc.with_ymd_and_hms(2026, 7, d, 23, 0, 0).unwrap();
        let got = sessions(events.iter(), day(5), day(6));
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].kind, Kind::Racing);
        assert!(sessions(events.iter(), day(6), day(7)).is_empty());
    }
}
