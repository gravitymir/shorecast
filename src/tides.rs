//! Tide height predictions from the Marine Institute (Ireland) ERDDAP
//! server: 5-minute water levels in metres above chart datum (LAT).

use std::path::PathBuf;
use std::time::Duration;

use chrono::{DateTime, Datelike, NaiveDate, TimeDelta, Utc};

use crate::astro::Point;

const ERDDAP: &str = "https://erddap.marine.ie/erddap/tabledap/IMI-TidePrediction.csv";

/// A saved month is downloaded again once it is this old.
const REFRESH_AFTER: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// What ERDDAP sends before the rows; also what an empty month is saved as.
const CSV_HEADER: &str = "time,Water_Level
UTC,metres
";

/// Marine Institute prediction stations: (ID, latitude, longitude).
const STATIONS: &[(&str, f64, f64)] = &[
    ("Achill_Island_MODELLED", 53.9522, -10.1016),
    ("Aranmore", 54.9896, -8.49562),
    ("Arklow", 52.79205, -6.145231),
    ("Ballycotton", 51.82776, -8.0007),
    ("Ballyglass", 54.253, -9.89),
    ("Bray_Harbour_MODELLED", 53.2191, -6.0901),
    ("Buncranna", 55.12662, -7.464125),
    ("Carrigaholt_MODELLED", 52.5965, -9.6812),
    ("Castletownbere", 51.6496, -9.9034),
    ("Clare_Island_MODELLED", 53.8019, -9.9443),
    ("Crosshaven_MODELLED", 51.7794, -8.2411),
    ("Dingle", 52.13924, -10.27732),
    ("Dublin_Port", 53.34574, -6.22166),
    ("Dungarvan_MODELLED", 52.0672, -7.5521),
    ("Dunmore", 52.14754, -6.99166),
    ("Fenit", 52.27129, -9.8644),
    ("Galway", 53.26895, -9.04796),
    ("Howth", 53.39148, -6.0683),
    ("Inishmore", 53.126, -9.66),
    ("Killary_Harbour_MODELLED", 53.6316, -9.9016),
    ("Killybegs", 54.6364, -8.3949),
    ("Kilrush", 52.63191, -9.50208),
    ("Kinsale_MODELLED", 51.6777, -8.446),
    ("Lahinch_MODELLED", 52.911, -9.3899),
    ("Letterfrack_MODELLED", 53.582, -10.0388),
    ("Malin_Head", 55.37168, -7.33432),
    ("Port_Oriel", 53.79899, -6.221713),
    ("Ringaskiddy", 51.84, -8.304),
    ("Roonagh", 53.76235, -9.90442),
    ("Rossaveel", 53.26693, -9.562056),
    ("Rosslare", 52.2546, -6.334861),
    ("Skerries", 53.585, -6.108117),
    ("Sligo", 54.3046, -8.5689),
    ("Tom_Clarke_Bridge", 53.34623, -6.227383),
    ("Tory_Island_MODELLED", 55.2508, -8.1962),
    ("Union_Hall", 51.559, -9.1335),
    ("Wexford", 52.33852, -6.4589),
    ("Wicklow_MODELLED", 52.9889, -6.0127),
];

pub type Series = Vec<(DateTime<Utc>, f64)>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Extreme {
    High,
    Low,
}

/// The prediction station closest to `p`.
pub fn nearest_station(p: Point) -> &'static str {
    let dist = |lat: f64, lon: f64| {
        let dx = (lon - p.lon) * p.lat.to_radians().cos();
        let dy = lat - p.lat;
        dx * dx + dy * dy
    };
    STATIONS
        .iter()
        .min_by(|a, b| dist(a.1, a.2).total_cmp(&dist(b.1, b.2)))
        .map(|s| s.0)
        .expect("station list is not empty")
}

/// Downloads predictions a month at a time and saves each month as a CSV
/// file under `dir/<station>/<YYYY-MM>.csv`; the server is asked again
/// only when a saved month is older than [`REFRESH_AFTER`].
pub struct Client {
    http: reqwest::Client,
    base_url: String,
    dir: PathBuf,
}

impl Client {
    pub fn new(dir: PathBuf) -> Self {
        Client {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(60))
                .build()
                .expect("HTTP client"),
            base_url: ERDDAP.to_string(),
            dir,
        }
    }

    /// Water levels at `station` from `start` to `end` inclusive.
    pub async fn heights(
        &self,
        station: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Series, String> {
        let mut out = Series::new();
        for month in months_between(start, end) {
            let series = self.month(station, month).await?;
            out.extend(series.into_iter().filter(|(t, _)| start <= *t && *t <= end));
        }
        Ok(out)
    }

    /// Saves every month the server has predictions for (about 230 KB a
    /// month, three years ahead) so pages never wait for a download.
    pub async fn prefetch(&self, station: &str) -> Result<(), String> {
        let url = format!(
            "{}?time&stationID=%22{station}%22&orderByMinMax(%22time%22)",
            self.base_url
        );
        let body = self
            .http
            .get(&url)
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .map_err(|e| e.to_string())?
            .text()
            .await
            .map_err(|e| e.to_string())?;
        let times: Vec<DateTime<Utc>> = body
            .lines()
            .skip(2)
            .filter_map(|line| DateTime::parse_from_rfc3339(line.trim()).ok())
            .map(|t| t.with_timezone(&Utc))
            .collect();
        let (Some(&first), Some(&last)) = (times.first(), times.last()) else {
            return Err(format!("no prediction range for {station}"));
        };

        let months = months_between(first, last);
        let mut failed = 0;
        for &month in &months {
            if let Err(e) = self.month(station, month).await {
                eprintln!("tides {station} {}: {e}", month.format("%Y-%m"));
                failed += 1;
            }
        }
        println!(
            "Tides {station}: {} of {} months saved ({} – {})",
            months.len() - failed,
            months.len(),
            first.format("%Y-%m"),
            last.format("%Y-%m"),
        );
        Ok(())
    }

    /// One UTC month at `station`, from disk when the saved copy is fresh.
    async fn month(&self, station: &str, month: NaiveDate) -> Result<Series, String> {
        let path = self
            .dir
            .join(station)
            .join(format!("{}.csv", month.format("%Y-%m")));
        let saved = tokio::fs::read_to_string(&path).await.ok();
        let fresh = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| age < REFRESH_AFTER);
        if let Some(body) = &saved
            && fresh
        {
            return parse_csv(body);
        }

        match self.download(station, month).await {
            Ok(body) => {
                let series = parse_csv(&body)?;
                if let Err(e) = save(&path, &body).await {
                    eprintln!("cannot save {}: {e}", path.display());
                }
                Ok(series)
            }
            // Offline or the server is down: an old copy beats nothing.
            Err(err) => match saved {
                Some(body) => parse_csv(&body),
                None => Err(err),
            },
        }
    }

    async fn download(&self, station: &str, month: NaiveDate) -> Result<String, String> {
        let next = month
            .checked_add_months(chrono::Months::new(1))
            .ok_or("date out of range")?;
        let url = format!(
            "{}?time,Water_Level&stationID=%22{station}%22&time%3E={month}T00:00:00Z&time%3C{next}T00:00:00Z",
            self.base_url,
        );
        let response = self.http.get(&url).send().await.map_err(|e| e.to_string())?;
        let status = response.status();
        let body = response.text().await.map_err(|e| e.to_string())?;
        if status.is_success() {
            Ok(body)
        } else if body.contains("Your query produced no matching results") {
            Ok(CSV_HEADER.to_string())
        } else {
            Err(format!("Marine Institute server answered {status}"))
        }
    }
}

/// Writes through a temporary file so a half-written month is never read.
async fn save(path: &std::path::Path, body: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        tokio::fs::create_dir_all(dir).await?;
    }
    let tmp = path.with_extension("csv.tmp");
    tokio::fs::write(&tmp, body).await?;
    tokio::fs::rename(&tmp, path).await
}

/// First days of the UTC months that `start..=end` touches.
fn months_between(start: DateTime<Utc>, end: DateTime<Utc>) -> Vec<NaiveDate> {
    let first = |t: DateTime<Utc>| NaiveDate::from_ymd_opt(t.year(), t.month(), 1).unwrap();
    let mut out = vec![first(start)];
    while let Some(next) = out.last().unwrap().checked_add_months(chrono::Months::new(1))
        && next <= end.date_naive()
    {
        out.push(next);
    }
    out
}

/// Parses ERDDAP CSV: a header row, a units row, then `time,level` rows.
fn parse_csv(body: &str) -> Result<Series, String> {
    body.lines()
        .skip(2)
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let (time, level) = line
                .split_once(',')
                .ok_or_else(|| format!("unexpected row {line:?}"))?;
            let time = DateTime::parse_from_rfc3339(time)
                .map_err(|e| format!("bad time {time:?}: {e}"))?
                .with_timezone(&Utc);
            let level = level
                .trim()
                .parse()
                .map_err(|_| format!("bad level {level:?}"))?;
            Ok((time, level))
        })
        .collect()
}

/// Level at exactly `t`, or the closest sample within 10 minutes.
pub fn level_at(series: &Series, t: DateTime<Utc>) -> Option<f64> {
    series
        .iter()
        .min_by_key(|(s, _)| (*s - t).num_seconds().abs())
        .filter(|(s, _)| (*s - t).abs() <= TimeDelta::minutes(10))
        .map(|&(_, level)| level)
}

/// High and low waters: turning points of the series.
pub fn extremes(series: &Series) -> Vec<(Extreme, DateTime<Utc>, f64)> {
    let mut out = Vec::new();
    let mut i = 1;
    while i + 1 < series.len() {
        let prev = series[i - 1].1;
        let here = series[i].1;
        // Skip over flat tops/bottoms, then compare both sides.
        let mut j = i;
        while j + 1 < series.len() && series[j + 1].1 == here {
            j += 1;
        }
        if j + 1 < series.len() {
            let next = series[j + 1].1;
            let mid = series[(i + j) / 2].0;
            if here > prev && here > next {
                out.push((Extreme::High, mid, here));
            } else if here < prev && here < next {
                out.push((Extreme::Low, mid, here));
            }
        }
        i = j + 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn t(h: u32, m: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 2, h, m, 0).unwrap()
    }

    #[test]
    fn roches_point_uses_crosshaven() {
        assert_eq!(
            nearest_station(Point { lat: 51.7944, lon: -8.2379 }),
            "Crosshaven_MODELLED"
        );
        assert_eq!(nearest_station(Point { lat: 53.35, lon: -6.2 }), "Dublin_Port");
    }

    #[test]
    fn day_windows_can_span_two_months() {
        let start = Utc.with_ymd_and_hms(2026, 10, 31, 23, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2026, 11, 1, 23, 0, 0).unwrap();
        let first = |m| NaiveDate::from_ymd_opt(2026, m, 1).unwrap();
        assert_eq!(months_between(start, end), vec![first(10), first(11)]);
        assert_eq!(months_between(t(0, 0), t(23, 0)), vec![first(10)]);
    }

    #[tokio::test]
    async fn reads_saved_months_without_the_network() {
        let dir = std::env::temp_dir().join(format!("shorecast-test-{}", std::process::id()));
        let station_dir = dir.join("Crosshaven_MODELLED");
        std::fs::create_dir_all(&station_dir).unwrap();
        std::fs::write(
            station_dir.join("2026-10.csv"),
            format!("{CSV_HEADER}2026-10-02T00:00:00Z,1.56
2026-10-03T00:00:00Z,2.0
"),
        )
        .unwrap();

        let mut client = Client::new(dir.clone());
        client.base_url = "http://127.0.0.1:9/unreachable".into();
        let got = client.heights("Crosshaven_MODELLED", t(0, 0), t(23, 0)).await;
        // November is neither saved nor downloadable.
        let missing = client
            .heights("Crosshaven_MODELLED", t(0, 0), Utc.with_ymd_and_hms(2026, 11, 2, 0, 0, 0).unwrap())
            .await;
        std::fs::remove_dir_all(&dir).unwrap();

        assert_eq!(got.unwrap(), vec![(t(0, 0), 1.56)]);
        assert!(missing.is_err());
    }

    #[test]
    fn parses_erddap_csv() {
        let body = "time,Water_Level\nUTC,metres\n\
                    2026-10-02T00:00:00Z,1.56\n2026-10-02T00:05:00Z,1.51\n";
        assert_eq!(parse_csv(body).unwrap(), vec![(t(0, 0), 1.56), (t(0, 5), 1.51)]);
        assert!(parse_csv("time,Water_Level\nUTC,metres\nnonsense\n").is_err());
    }

    #[test]
    fn finds_level_at_time() {
        let s = vec![(t(0, 0), 1.0), (t(0, 5), 2.0)];
        assert_eq!(level_at(&s, t(0, 5)), Some(2.0));
        assert_eq!(level_at(&s, t(0, 1)), Some(1.0));
        assert_eq!(level_at(&s, t(1, 0)), None);
    }

    #[test]
    fn finds_highs_and_lows() {
        let s = vec![
            (t(0, 0), 1.0),
            (t(0, 5), 2.0),
            (t(0, 10), 3.0),
            (t(0, 15), 3.0),
            (t(0, 20), 2.0),
            (t(0, 25), 0.5),
            (t(0, 30), 1.0),
        ];
        assert_eq!(
            extremes(&s),
            vec![(Extreme::High, t(0, 10), 3.0), (Extreme::Low, t(0, 25), 0.5)]
        );
    }
}
