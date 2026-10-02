//! Hourly weather from Open-Meteo (free, no key), saved locally. Each
//! download replaces the hours it covers, so past hours keep the last
//! forecast made for them and the pages work offline.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use chrono::{DateTime, NaiveDateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::site;

const FORECAST_URL: &str = "https://api.open-meteo.com/v1/forecast";
const VARIABLES: &str = "weather_code,cloud_cover,precipitation,temperature_2m,\
                         wind_speed_10m,wind_direction_10m,wind_gusts_10m,visibility,is_day";
const REFRESH_EVERY: TimeDelta = TimeDelta::hours(3);
const CHECK_EVERY: Duration = Duration::from_secs(15 * 60);

/// One hour of weather at the camera point.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hour {
    /// WMO weather code: 0 clear … 3 overcast, 45 fog, 51+ drizzle,
    /// 61+ rain, 71+ snow, 80+ showers, 95+ thunderstorm.
    pub code: u8,
    /// Percent of the sky.
    pub cloud: Option<f64>,
    /// Millimetres in the hour.
    pub precipitation: Option<f64>,
    /// °C at 2 m.
    pub temperature: Option<f64>,
    /// km/h at 10 m.
    pub wind: Option<f64>,
    pub gusts: Option<f64>,
    /// Degrees the wind blows from.
    pub wind_from: Option<f64>,
    /// Metres.
    pub visibility: Option<f64>,
    pub is_day: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Saved {
    hours: BTreeMap<DateTime<Utc>, Hour>,
    fetched: Option<DateTime<Utc>>,
}

pub struct Store {
    http: reqwest::Client,
    file: PathBuf,
    saved: RwLock<Saved>,
}

impl Store {
    pub fn open(dir: PathBuf) -> Self {
        let file = dir.join("open-meteo.json");
        let saved = std::fs::read_to_string(&file)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Store {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(60))
                .build()
                .expect("HTTP client"),
            file,
            saved: RwLock::new(saved),
        }
    }

    /// Keeps the saved forecast fresh, forever.
    pub async fn run(self: Arc<Self>) {
        loop {
            let fetched = self.saved.read().unwrap().fetched;
            if fetched.is_none_or(|t| Utc::now() - t >= REFRESH_EVERY) {
                match self.fetch().await {
                    Ok(hours) => {
                        println!("Weather: {} hours from Open-Meteo", hours.len());
                        {
                            let mut s = self.saved.write().unwrap();
                            s.hours.extend(hours);
                            s.fetched = Some(Utc::now());
                        }
                        if let Err(e) = self.save().await {
                            eprintln!("cannot save {}: {e}", self.file.display());
                        }
                    }
                    Err(e) => eprintln!("weather: {e}"),
                }
            }
            tokio::time::sleep(CHECK_EVERY).await;
        }
    }

    async fn fetch(&self) -> Result<Vec<(DateTime<Utc>, Hour)>, String> {
        let lat = site::POINT.lat.to_string();
        let lon = site::POINT.lon.to_string();
        let body = self
            .http
            .get(FORECAST_URL)
            .query(&[
                ("latitude", lat.as_str()),
                ("longitude", lon.as_str()),
                ("hourly", VARIABLES),
                ("past_days", "7"),
                ("forecast_days", "16"),
                ("timezone", "UTC"),
            ])
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .map_err(|e| e.to_string())?
            .text()
            .await
            .map_err(|e| e.to_string())?;
        parse(&body)
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

    /// Saved hours from `start` (inclusive) to `end` (exclusive).
    pub fn hours(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> Vec<(DateTime<Utc>, Hour)> {
        self.saved
            .read()
            .unwrap()
            .hours
            .range(start..end)
            .map(|(t, h)| (*t, h.clone()))
            .collect()
    }
}

/// Open-Meteo's hourly arrays, one entry per hour; hours past the end of
/// the forecast come back as nulls and are skipped.
fn parse(body: &str) -> Result<Vec<(DateTime<Utc>, Hour)>, String> {
    let json: Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
    if json["error"] == true {
        return Err(json["reason"].as_str().unwrap_or("Open-Meteo error").to_string());
    }
    let hourly = &json["hourly"];
    let times = hourly["time"].as_array().ok_or("no hourly times")?;
    let at = |key: &str, i: usize| hourly[key][i].as_f64();
    let mut out = Vec::new();
    for (i, t) in times.iter().enumerate() {
        let Some(t) = t
            .as_str()
            .and_then(|t| NaiveDateTime::parse_from_str(t, "%Y-%m-%dT%H:%M").ok())
        else {
            continue;
        };
        let Some(code) = at("weather_code", i) else {
            continue;
        };
        out.push((
            t.and_utc(),
            Hour {
                code: code as u8,
                cloud: at("cloud_cover", i),
                precipitation: at("precipitation", i),
                temperature: at("temperature_2m", i),
                wind: at("wind_speed_10m", i),
                gusts: at("wind_gusts_10m", i),
                wind_from: at("wind_direction_10m", i),
                visibility: at("visibility", i),
                is_day: at("is_day", i) == Some(1.0),
            },
        ));
    }
    Ok(out)
}

/// The weather that best sums up the daylight hours: the most frequent
/// code, ties going to the worse weather.
pub fn daytime_code(hours: &[(DateTime<Utc>, Hour)]) -> Option<u8> {
    let mut counts: BTreeMap<u8, usize> = BTreeMap::new();
    for (_, h) in hours.iter().filter(|(_, h)| h.is_day) {
        *counts.entry(h.code).or_default() += 1;
    }
    counts
        .into_iter()
        .max_by_key(|&(code, n)| (n, code))
        .map(|(code, _)| code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    const BODY: &str = r#"{"hourly":{
        "time":["2026-10-02T09:00","2026-10-02T10:00","2026-10-17T23:00"],
        "weather_code":[51,3,null],"cloud_cover":[100,90,null],
        "precipitation":[0.3,0.0,null],"temperature_2m":[15.9,16.0,null],
        "wind_speed_10m":[30.2,30.6,null],"wind_direction_10m":[200,210,null],
        "wind_gusts_10m":[52.6,53.3,null],"visibility":[15580.0,12280.0,null],
        "is_day":[1,1,null]}}"#;

    #[test]
    fn parses_open_meteo() {
        let hours = parse(BODY).unwrap();
        assert_eq!(hours.len(), 2);
        let (t, h) = &hours[0];
        assert_eq!(*t, Utc.with_ymd_and_hms(2026, 10, 2, 9, 0, 0).unwrap());
        assert_eq!(h.code, 51);
        assert_eq!(h.precipitation, Some(0.3));
        assert_eq!(h.gusts, Some(52.6));
        assert!(h.is_day);
        assert!(parse(r#"{"error":true,"reason":"bad"}"#).is_err());
    }

    #[test]
    fn daytime_summary() {
        let hours = parse(BODY).unwrap();
        // One hour of drizzle, one overcast: the tie goes to drizzle.
        assert_eq!(daytime_code(&hours), Some(51));
        assert_eq!(daytime_code(&[]), None);
    }
}
