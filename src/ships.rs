//! Ship movements in and out of Cork Harbour, saved locally so pages work
//! offline and the sources are asked only now and then.
//!
//! Sources, both from the Port of Cork Company:
//! - the shipping schedule behind the dashboard on portofcork.ie, an ArcGIS
//!   feature service: planned arrivals and departures about a week ahead
//!   (layer 1, "status") and a log of movements a year back (layer 2, "log");
//! - the cruise schedule page: the season's cruise calls, months ahead.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use chrono::{DateTime, Datelike, Months, NaiveDate, NaiveDateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::site;

const MOVEMENTS_URL: &str = "https://utility.arcgis.com/usrsvcs/servers/1c876bed756644a1b7916b0107d01cd8/rest/services/geoiot/port-of-cork-movements/FeatureServer";
const CRUISES_URL: &str = "https://www.portofcork.ie/cruise-schedule/";

const SCHEDULE_EVERY: TimeDelta = TimeDelta::hours(3);
const LOG_EVERY: TimeDelta = TimeDelta::days(1);
const CRUISES_EVERY: TimeDelta = TimeDelta::days(7);
/// How often the background task looks at what is due.
const CHECK_EVERY: Duration = Duration::from_secs(15 * 60);
/// The first log download goes back this far.
const LOG_SINCE: (i32, u32) = (2025, 1);

const FIELDS: &str = "VISIT_NO,VESSEL,VESSEL_TYPE,VESSEL_LOA,IMO,MOVE_TYPE,MOVEMENT_STATUS,\
                      FROM_LOC,TO_LOC,FROM_NAME,TO_NAME,SRT,ATA,ATD";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    In,
    Out,
}

/// One arrival or departure from the port schedule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Movement {
    pub vessel: String,
    pub vessel_type: String,
    pub length_m: Option<f64>,
    pub imo: Option<u64>,
    pub direction: Direction,
    pub status: String,
    /// Berths or pilot stations ("PS ROCHES POINT", "RO-RO BERTH").
    pub from: String,
    pub to: String,
    /// Previous port for arrivals, next port for departures.
    pub port: String,
    /// Arrivals: pilot boarding off the harbour mouth. Departures: leaving the berth.
    pub scheduled: DateTime<Utc>,
    /// The same moment as it actually happened, once it has.
    pub actual: Option<DateTime<Utc>>,
}

/// One call from the cruise schedule page; times are at the berth.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CruiseCall {
    pub vessel: String,
    pub berth: String,
    pub line: String,
    pub pax: Option<u32>,
    pub imo: Option<u64>,
    pub arrival: DateTime<Utc>,
    pub departure: DateTime<Utc>,
}

/// Everything ever downloaded, as saved in `ships.json`.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Saved {
    movements: BTreeMap<String, Movement>,
    cruises: BTreeMap<String, CruiseCall>,
    schedule_fetched: Option<DateTime<Utc>>,
    log_fetched: Option<DateTime<Utc>>,
    cruises_fetched: Option<DateTime<Utc>>,
}

pub struct Store {
    http: reqwest::Client,
    file: PathBuf,
    saved: RwLock<Saved>,
}

impl Store {
    /// Opens the saved data in `dir`, if any.
    pub fn open(dir: PathBuf) -> Self {
        let file = dir.join("ships.json");
        let saved = std::fs::read_to_string(&file)
            .ok()
            .and_then(|s| match serde_json::from_str(&s) {
                Ok(saved) => Some(saved),
                Err(e) => {
                    eprintln!("ignoring unreadable {}: {e}", file.display());
                    None
                }
            })
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

    /// Keeps the saved data fresh, forever.
    pub async fn run(self: Arc<Self>) {
        loop {
            self.refresh_due().await;
            tokio::time::sleep(CHECK_EVERY).await;
        }
    }

    async fn refresh_due(&self) {
        let now = Utc::now();
        let due = |last: Option<DateTime<Utc>>, every| last.is_none_or(|t| now - t >= every);
        let (schedule, log, cruises, log_from) = {
            let s = self.saved.read().unwrap();
            let log_from = match s.log_fetched {
                Some(t) => (t - TimeDelta::days(7)).date_naive(),
                None => NaiveDate::from_ymd_opt(LOG_SINCE.0, LOG_SINCE.1, 1).unwrap(),
            };
            (
                due(s.schedule_fetched, SCHEDULE_EVERY),
                due(s.log_fetched, LOG_EVERY),
                due(s.cruises_fetched, CRUISES_EVERY),
                log_from,
            )
        };
        let mut changed = false;

        if log {
            match self.fetch_log(log_from, now).await {
                Ok(rows) => {
                    let n = self.merge(rows);
                    self.saved.write().unwrap().log_fetched = Some(now);
                    println!("Ships: movement log since {log_from}: {n} movements");
                    changed = true;
                }
                Err(e) => eprintln!("ships: movement log: {e}"),
            }
        }
        if schedule {
            match self.fetch_layer(1, "1=1").await {
                Ok(rows) => {
                    let n = self.merge(rows);
                    self.saved.write().unwrap().schedule_fetched = Some(now);
                    println!("Ships: schedule: {n} movements");
                    changed = true;
                }
                Err(e) => eprintln!("ships: schedule: {e}"),
            }
        }
        if cruises {
            match self.fetch_cruises().await {
                Ok(calls) => {
                    let mut s = self.saved.write().unwrap();
                    println!("Ships: cruise schedule: {} calls", calls.len());
                    s.cruises.extend(calls);
                    s.cruises_fetched = Some(now);
                    changed = true;
                }
                Err(e) => eprintln!("ships: cruise schedule: {e}"),
            }
        }

        if changed && let Err(e) = self.save().await {
            eprintln!("cannot save {}: {e}", self.file.display());
        }
    }

    /// Adds movements, letting later stages (in progress, completed)
    /// win over earlier ones. Returns how many rows were usable.
    fn merge(&self, rows: Vec<(String, Movement)>) -> usize {
        let mut s = self.saved.write().unwrap();
        let n = rows.len();
        for (key, m) in rows {
            match s.movements.get(&key) {
                Some(old) if stage(&old.status) > stage(&m.status) => {}
                _ => {
                    s.movements.insert(key, m);
                }
            }
        }
        n
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

    /// The log a month at a time: the service returns at most 10 000 rows
    /// per query, and a month is around 1 200.
    async fn fetch_log(
        &self,
        from: NaiveDate,
        until: DateTime<Utc>,
    ) -> Result<Vec<(String, Movement)>, String> {
        let mut month = NaiveDate::from_ymd_opt(from.year(), from.month(), 1).unwrap();
        let last = until.date_naive() + TimeDelta::days(31);
        let mut out = Vec::new();
        while month <= last {
            let next = month + Months::new(1);
            let filter = format!(
                "SRT >= timestamp '{month} 00:00:00' AND SRT < timestamp '{next} 00:00:00'"
            );
            out.extend(self.fetch_layer(2, &filter).await?);
            month = next;
        }
        Ok(out)
    }

    async fn fetch_layer(&self, layer: u32, filter: &str) -> Result<Vec<(String, Movement)>, String> {
        let filter = format!("({filter}) AND MOVE_TYPE IN ('ARRIVAL','DEPARTURE')");
        let url = format!("{MOVEMENTS_URL}/{layer}/query");
        let body = self
            .http
            .get(&url)
            .query(&[
                ("where", filter.as_str()),
                ("outFields", FIELDS),
                ("returnGeometry", "false"),
                ("f", "json"),
            ])
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .map_err(|e| e.to_string())?
            .text()
            .await
            .map_err(|e| e.to_string())?;
        let json: Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
        if let Some(err) = json.get("error") {
            return Err(err.to_string());
        }
        if json["exceededTransferLimit"] == true {
            eprintln!("ships: layer {layer} cut short for {filter}");
        }
        Ok(json["features"]
            .as_array()
            .map(|f| f.iter().filter_map(|f| movement(&f["attributes"])).collect())
            .unwrap_or_default())
    }

    async fn fetch_cruises(&self) -> Result<Vec<(String, CruiseCall)>, String> {
        let html = self
            .http
            .get(CRUISES_URL)
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .map_err(|e| e.to_string())?
            .text()
            .await
            .map_err(|e| e.to_string())?;
        let calls = parse_cruises(&html);
        if calls.is_empty() {
            return Err("no calls found on the page; has its layout changed?".into());
        }
        Ok(calls)
    }

    /// The last scheduled movement: the port schedule says nothing beyond it.
    pub fn known_until(&self) -> Option<DateTime<Utc>> {
        let s = self.saved.read().unwrap();
        s.movements.values().map(|m| m.scheduled).max()
    }

    /// Ships passing the harbour mouth between `start` and `end`.
    pub fn passages(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> Vec<Passage> {
        let s = self.saved.read().unwrap();
        passages(&s.movements, &s.cruises, start, end)
    }
}

/// Later stages of a movement replace earlier ones.
fn stage(status: &str) -> u8 {
    match status {
        "COMPLETED" => 3,
        "INPROGRESS" => 2,
        _ => 1,
    }
}

fn text(a: &Value, key: &str) -> Option<String> {
    match &a[key] {
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn number(a: &Value, key: &str) -> Option<f64> {
    match &a[key] {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn time(a: &Value, key: &str) -> Option<DateTime<Utc>> {
    DateTime::from_timestamp_millis(a[key].as_i64()?)
}

/// A movement from a feature's attributes, keyed like the service's own
/// ID ("2601416_DEPARTURE").
fn movement(a: &Value) -> Option<(String, Movement)> {
    let visit = text(a, "VISIT_NO")?;
    let kind = text(a, "MOVE_TYPE")?;
    let (direction, actual_field, port_field) = match kind.as_str() {
        "ARRIVAL" => (Direction::In, "ATA", "FROM_NAME"),
        "DEPARTURE" => (Direction::Out, "ATD", "TO_NAME"),
        _ => return None,
    };
    let status = text(a, "MOVEMENT_STATUS").unwrap_or_default();
    // Planned movements sometimes carry estimates in ATA/ATD.
    let actual = if stage(&status) > 1 { time(a, actual_field) } else { None };
    let m = Movement {
        vessel: text(a, "VESSEL").unwrap_or_else(|| "Unnamed vessel".into()),
        vessel_type: text(a, "VESSEL_TYPE").unwrap_or_default(),
        length_m: number(a, "VESSEL_LOA"),
        imo: number(a, "IMO").filter(|n| *n > 0.0).map(|n| n as u64),
        direction,
        status,
        from: text(a, "FROM_LOC").unwrap_or_default(),
        to: text(a, "TO_LOC").unwrap_or_default(),
        port: text(a, port_field).unwrap_or_default(),
        scheduled: time(a, "SRT")?,
        actual,
    };
    Some((format!("{visit}_{kind}"), m))
}

/// Cruise calls from the schedule page's tables: arrival date, vessel,
/// berth, arrival, departure, line, passengers, agent, IMO link.
fn parse_cruises(html: &str) -> Vec<(String, CruiseCall)> {
    let html = strip_comments(html);
    let mut out = Vec::new();
    for row in html.split("<tr").skip(1) {
        let cells: Vec<String> = row.split("<td").skip(1).map(cell_text).collect();
        if cells.len() < 9 || cells[1].to_lowercase().contains("test") {
            continue;
        }
        let (Some(arrival), Some(departure)) = (local_time(&cells[3]), local_time(&cells[4])) else {
            continue;
        };
        let imo = row
            .split_once("imo:")
            .map(|(_, rest)| rest.chars().take_while(char::is_ascii_digit).collect::<String>())
            .and_then(|digits| digits.parse().ok());
        let call = CruiseCall {
            vessel: cells[1].clone(),
            berth: cells[2].clone(),
            line: cells[5].clone(),
            pax: cells[6].parse().ok(),
            imo,
            arrival,
            departure,
        };
        out.push((format!("{}_{}", arrival.format("%Y-%m-%d"), call.vessel), call));
    }
    out
}

fn strip_comments(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(start) = rest.find("<!--") {
        out.push_str(&rest[..start]);
        rest = rest[start..].find("-->").map_or("", |end| &rest[start + end + 3..]);
    }
    out.push_str(rest);
    out
}

/// The text of a `<td ...>...</td>` fragment, tags removed.
fn cell_text(fragment: &str) -> String {
    let inner = fragment.split_once('>').map_or("", |(_, rest)| rest);
    let inner = inner.split("</td").next().unwrap_or("");
    let mut text = String::new();
    let mut in_tag = false;
    for c in inner.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => text.push(c),
            _ => {}
        }
    }
    let text = text
        .replace("&amp;", "&")
        .replace("&#039;", "'")
        .replace("&#39;", "'")
        .replace("&quot;", "\"")
        .replace("&nbsp;", " ");
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// "13/04/2026 09:00" at the camera point's local time.
fn local_time(s: &str) -> Option<DateTime<Utc>> {
    let t = NaiveDateTime::parse_from_str(s, "%d/%m/%Y %H:%M").ok()?;
    Some(t.and_local_timezone(site::TZ).earliest()?.with_timezone(&Utc))
}

/// A ship passing the camera, for the day page.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Passage {
    /// Actual time if known, else scheduled.
    pub time: DateTime<Utc>,
    pub scheduled: DateTime<Utc>,
    pub actual: bool,
    pub vessel: String,
    pub vessel_type: String,
    /// cruise, ferry, container, tanker, bulk, cargo, navy, fishing, other
    pub category: &'static str,
    pub length_m: Option<f64>,
    pub direction: Direction,
    pub from: String,
    pub to: String,
    /// Previous or next port (for cruises: the cruise line).
    pub port: String,
    pub status: String,
    /// "port" for the shipping schedule, "cruise" for the cruise page.
    pub source: &'static str,
}

fn category(vessel_type: &str) -> &'static str {
    let t = vessel_type.to_uppercase();
    if t.contains("CRUISE") {
        "cruise"
    } else if t.contains("PASSENGER") || t.contains("FERRY") {
        // "RO-RO CARGO SHIP" without passengers is a freighter, not a ferry.
        "ferry"
    } else if t.contains("CONTAINER") {
        "container"
    } else if t.contains("TANK") || t.contains("CHEMICAL") || t.contains("GAS") {
        "tanker"
    } else if t.contains("CARGO") || t.contains("VEHICLES CARRIER") {
        "cargo"
    } else if t.contains("BULK") || t.contains("CARRIER") {
        // Cement, aggregates and stone carriers are bulkers too.
        "bulk"
    } else if t.contains("NAVY") || t.contains("NAVAL") || t.contains("PATROL") {
        "navy"
    } else if t.contains("FISH") {
        "fishing"
    } else {
        "other"
    }
}

fn passages(
    movements: &BTreeMap<String, Movement>,
    cruises: &BTreeMap<String, CruiseCall>,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Vec<Passage> {
    let within = |t: DateTime<Utc>| start <= t && t < end;
    let mut out: Vec<Passage> = movements
        .values()
        .filter(|m| !m.status.contains("CANCEL"))
        .map(|m| Passage {
            time: m.actual.unwrap_or(m.scheduled),
            scheduled: m.scheduled,
            actual: m.actual.is_some(),
            vessel: m.vessel.clone(),
            vessel_type: m.vessel_type.clone(),
            category: category(&m.vessel_type),
            length_m: m.length_m,
            direction: m.direction,
            from: m.from.clone(),
            to: m.to.clone(),
            port: m.port.clone(),
            status: m.status.clone(),
            source: "port",
        })
        .filter(|p| within(p.time))
        .collect();

    // Cruise calls the shipping schedule does not cover yet.
    for c in cruises.values() {
        let same_ship = |m: &Movement| match (m.imo, c.imo) {
            (Some(a), Some(b)) => a == b,
            _ => m.vessel.eq_ignore_ascii_case(&c.vessel),
        };
        let covered = movements.values().any(|m| {
            same_ship(m) && (m.scheduled - c.arrival).abs() < TimeDelta::hours(24)
        });
        if covered {
            continue;
        }
        for (t, direction, from, to) in [
            (c.arrival, Direction::In, "Sea", c.berth.as_str()),
            (c.departure, Direction::Out, c.berth.as_str(), "Sea"),
        ] {
            if within(t) {
                out.push(Passage {
                    time: t,
                    scheduled: t,
                    actual: false,
                    vessel: c.vessel.clone(),
                    vessel_type: "CRUISE SHIP".into(),
                    category: "cruise",
                    length_m: None,
                    direction,
                    from: from.into(),
                    to: to.into(),
                    port: c.line.clone(),
                    status: "SCHEDULED".into(),
                    source: "cruise",
                });
            }
        }
    }
    out.sort_by_key(|p| p.time);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use serde_json::json;

    fn utc(d: u32, h: u32, m: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, d, h, m, 0).unwrap()
    }

    #[test]
    fn parses_movement_attributes() {
        let a = json!({
            "VISIT_NO": "2601416", "VESSEL": "MYKONOS SEAS", "VESSEL_TYPE": "BULK CARRIER",
            "VESSEL_LOA": "189.99", "IMO": "9491240", "MOVE_TYPE": "DEPARTURE",
            "MOVEMENT_STATUS": "APPROVED", "FROM_LOC": "RINGASKIDDY DEEPWATER BERTH",
            "TO_LOC": "PS 2 VESSELS >130M LOA", "FROM_NAME": "San Lorenzo", "TO_NAME": "Dublin",
            "SRT": 1791565200000_i64, "ATA": null, "ATD": 1791566200000_i64
        });
        let (key, m) = movement(&a).unwrap();
        assert_eq!(key, "2601416_DEPARTURE");
        assert_eq!(m.direction, Direction::Out);
        assert_eq!(m.length_m, Some(189.99));
        assert_eq!(m.imo, Some(9491240));
        assert_eq!(m.port, "Dublin");
        assert_eq!(m.scheduled, utc(9, 17, 0));
        // An ATD on a movement still only approved is an estimate.
        assert_eq!(m.actual, None);

        let shift = json!({"VISIT_NO": "1", "MOVE_TYPE": "SHIFT", "SRT": 0});
        assert!(movement(&shift).is_none());
    }

    #[test]
    fn later_stages_win() {
        let store = Store::open(std::env::temp_dir().join("shorecast-no-such-dir"));
        let m = |status: &str, hour| Movement {
            vessel: "X".into(),
            vessel_type: String::new(),
            length_m: None,
            imo: None,
            direction: Direction::In,
            status: status.into(),
            from: String::new(),
            to: String::new(),
            port: String::new(),
            scheduled: utc(2, hour, 0),
            actual: None,
        };
        store.merge(vec![("k".into(), m("COMPLETED", 10)), ("k".into(), m("APPROVED", 11))]);
        assert_eq!(store.saved.read().unwrap().movements["k"].status, "COMPLETED");
        store.merge(vec![("j".into(), m("APPROVED", 10)), ("j".into(), m("APPROVED", 12))]);
        assert_eq!(store.saved.read().unwrap().movements["j"].scheduled, utc(2, 12, 0));
    }

    const CRUISE_HTML: &str = r#"
        <table><thead><tr><td colspan="9"><h1>October 2026</h1></td></tr>
        <tr><td>ARRIVAL DATE</td><td>VESSEL</td><td>BERTH</td><td>Arrival to Berth</td>
        <td>Departure</td><td>LINE</td><td>PAX</td><td>AGENT</td><td>IMO</td></tr></thead>
        <tbody><tr>
          <!-- <td></td> -->
          <td>
          14/10/2026        </td>
          <td>Ambition</td><td>Cobh Cruise Terminal</td>
          <td>14/10/2026 13:00</td><td>14/10/2026 19:00</td>
          <td>Ambassador Cruise Line</td><td>1904</td><td>DSG</td>
          <td><a href="https://www.marinetraffic.com/en/ais/details/ships/imo:9171292">Vessel Information</a></td>
        </tr><tr>
          <td>02/10/2026</td><td>Test Vessel</td><td>Cobh Cruise Terminal</td>
          <td>02/10/2026 08:00</td><td>02/10/2026 12:00</td><td>Test Booking Line</td><td>646</td><td></td><td></td>
        </tr></tbody></table>"#;

    #[test]
    fn parses_cruise_page() {
        let calls = parse_cruises(CRUISE_HTML);
        assert_eq!(calls.len(), 1);
        let (key, c) = &calls[0];
        assert_eq!(key, "2026-10-14_Ambition");
        assert_eq!(c.line, "Ambassador Cruise Line");
        assert_eq!(c.pax, Some(1904));
        assert_eq!(c.imo, Some(9171292));
        // 13:00 Irish summer time.
        assert_eq!(c.arrival, utc(14, 12, 0));
        assert_eq!(c.departure, utc(14, 18, 0));
    }

    #[test]
    fn categories() {
        assert_eq!(category("PASSENGER/RO-RO SHIP (VEHICLES)"), "ferry");
        assert_eq!(category("RO-RO CARGO SHIP"), "cargo");
        assert_eq!(category("VEHICLES CARRIER"), "cargo");
        assert_eq!(category("CEMENT CARRIER"), "bulk");
        assert_eq!(category("PATROL VESSEL"), "navy");
        assert_eq!(category("TUG"), "other");
        assert_eq!(category("CONTAINER SHIP (FULLY CELLULAR)"), "container");
        assert_eq!(category("CHEMICAL/PRODUCTS TANKER"), "tanker");
        assert_eq!(category("CRUDE/OIL PRODUCTS TANKER"), "tanker");
        assert_eq!(category("BULK CARRIER"), "bulk");
        assert_eq!(category("GENERAL CARGO SHIP"), "cargo");
        assert_eq!(category("PASSENGER (CRUISE) SHIP"), "cruise");
        assert_eq!(category("NAVY SHIP"), "navy");
        assert_eq!(category("FISH CATCHING"), "fishing");
    }

    #[test]
    fn passages_for_a_day() {
        let movement = |vessel: &str, imo, status: &str, scheduled, actual| Movement {
            vessel: vessel.into(),
            vessel_type: "PASSENGER/RO-RO SHIP".into(),
            length_m: Some(203.3),
            imo,
            direction: Direction::In,
            status: status.into(),
            from: "PS 2".into(),
            to: "RO-RO BERTH".into(),
            port: "Roscoff".into(),
            scheduled,
            actual,
        };
        let movements = BTreeMap::from([
            ("a".into(), movement("LATE", None, "COMPLETED", utc(2, 10, 0), Some(utc(2, 11, 0)))),
            ("b".into(), movement("GONE", None, "CANCELLED", utc(2, 9, 0), None)),
            ("c".into(), movement("TOMORROW", None, "APPROVED", utc(3, 9, 0), None)),
            ("d".into(), movement("AMBITION", Some(9171292), "APPROVED", utc(14, 11, 0), None)),
        ]);
        let cruise = |vessel: &str, imo, day| CruiseCall {
            vessel: vessel.into(),
            berth: "Cobh Cruise Terminal".into(),
            line: "Line".into(),
            pax: None,
            imo,
            arrival: utc(day, 8, 0),
            departure: utc(day, 17, 0),
        };
        let cruises = BTreeMap::from([
            ("x".into(), cruise("Big Ship", None, 2)),
            ("y".into(), cruise("Ambition", Some(9171292), 14)),
        ]);

        let day = passages(&movements, &cruises, utc(2, 0, 0), utc(3, 0, 0));
        let names: Vec<_> = day.iter().map(|p| (p.vessel.as_str(), p.direction)).collect();
        assert_eq!(
            names,
            [("Big Ship", Direction::In), ("LATE", Direction::In), ("Big Ship", Direction::Out)]
        );
        assert!(day[1].actual);
        assert_eq!(day[1].time, utc(2, 11, 0));
        assert_eq!(day[0].source, "cruise");

        // The port schedule already has Ambition: no duplicate from the cruise page.
        let day = passages(&movements, &cruises, utc(14, 0, 0), utc(15, 0, 0));
        assert_eq!(day.len(), 1);
        assert_eq!(day[0].source, "port");
    }
}
