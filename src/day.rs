//! Everything the day page draws, as JSON for the browser.

use chrono::{DateTime, Days, NaiveDate, TimeDelta, Utc};
use serde::Serialize;

use crate::astro::{self, Body, EventKind, compass};
use crate::sailing::{Kind, Session};
use crate::ships::Passage;
use crate::site;
use crate::tides::{self, Extreme, Series};
use crate::weather::{self, Hour};

/// One local calendar day at the camera point, as a UTC window.
pub struct Window {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

impl Window {
    pub fn new(date: NaiveDate) -> Self {
        let next = date.checked_add_days(Days::new(1)).unwrap_or(date);
        Window { start: local_midnight(date), end: local_midnight(next) }
    }

    fn contains(&self, t: DateTime<Utc>) -> bool {
        self.start <= t && t < self.end
    }
}

fn local_midnight(date: NaiveDate) -> DateTime<Utc> {
    date.and_hms_opt(0, 0, 0)
        .expect("midnight exists")
        .and_local_timezone(site::TZ)
        .earliest()
        .expect("clocks never skip midnight in Europe/Dublin")
        .with_timezone(&Utc)
}

#[derive(Serialize)]
pub struct Site {
    name: &'static str,
    lat: f64,
    lon: f64,
    tz: &'static str,
    now: DateTime<Utc>,
}

pub fn site(now: DateTime<Utc>) -> Site {
    Site {
        name: site::NAME,
        lat: site::POINT.lat,
        lon: site::POINT.lon,
        tz: site::TZ.name(),
        now,
    }
}

#[derive(Serialize)]
pub struct Day {
    date: NaiveDate,
    site: Site,
    /// Local midnight to the next local midnight (23–25 hours).
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    sun: Sky,
    moon: Moon,
    tides: Tides,
    /// Ships passing the harbour mouth, by time.
    ships: Vec<Passage>,
    /// Yacht club racing and training on the water.
    sailing: Vec<Session>,
    weather: Vec<WeatherHour>,
}

#[derive(Serialize)]
struct WeatherHour {
    time: DateTime<Utc>,
    #[serde(flatten)]
    hour: Hour,
}

/// When a body is up, and its rises and sets.
#[derive(Serialize)]
struct Sky {
    up: Vec<(DateTime<Utc>, DateTime<Utc>)>,
    events: Vec<SkyEvent>,
}

#[derive(Serialize)]
struct SkyEvent {
    #[serde(flatten)]
    event: astro::Event,
    compass: &'static str,
}

#[derive(Serialize)]
struct Moon {
    #[serde(flatten)]
    sky: Sky,
    /// Illuminated fraction at local noon, 0..1.
    illuminated: f64,
    waxing: bool,
    phase: &'static str,
    emoji: &'static str,
}

#[derive(Serialize)]
struct Tides {
    station: &'static str,
    /// Why there is no series, if there is none.
    error: Option<String>,
    /// 5-minute water levels: [time, metres above chart datum].
    series: Series,
    /// Level at each local hour 00..23, if predicted.
    hourly: Vec<Option<f64>>,
    extremes: Vec<TideExtreme>,
}

#[derive(Serialize)]
struct TideExtreme {
    kind: Extreme,
    time: DateTime<Utc>,
    level: f64,
}

fn sky(body: Body, day: &Window) -> Sky {
    let track = astro::track(body, site::POINT, day.start, day.end);
    Sky {
        up: track.up,
        events: track
            .events
            .into_iter()
            .map(|event| SkyEvent { compass: compass(event.azimuth), event })
            .collect(),
    }
}

fn tides(day: &Window, station: &'static str, series: Result<Series, String>) -> Tides {
    let (series, error) = match series {
        Ok(series) => (series, None),
        Err(err) => (Series::new(), Some(err)),
    };
    let hourly = (0..24)
        .map(|h| tides::level_at(&series, day.start + TimeDelta::hours(h)))
        .collect();
    let extremes = tides::extremes(&series)
        .into_iter()
        .filter(|(_, t, _)| day.contains(*t))
        .map(|(kind, time, level)| TideExtreme { kind, time, level })
        .collect();
    Tides { station, error, series, hourly, extremes }
}

pub fn build(
    date: NaiveDate,
    now: DateTime<Utc>,
    station: &'static str,
    series: Result<Series, String>,
    ships: Vec<Passage>,
    sailing: Vec<Session>,
    weather: Vec<(DateTime<Utc>, Hour)>,
) -> Day {
    let day = Window::new(date);
    let noon = day.start + (day.end - day.start) / 2;
    let phase = astro::moon_phase(noon);
    Day {
        date,
        site: site(now),
        start: day.start,
        end: day.end,
        sun: sky(Body::Sun, &day),
        moon: Moon {
            sky: sky(Body::Moon, &day),
            illuminated: phase.illuminated,
            waxing: phase.waxing,
            phase: phase.name(),
            emoji: phase.emoji(),
        },
        tides: tides(&day, station, series),
        ships,
        sailing,
        weather: weather
            .into_iter()
            .map(|(time, hour)| WeatherHour { time, hour })
            .collect(),
    }
}

/// A day in a few numbers, for its cell in the month calendar.
#[derive(Serialize)]
pub struct Summary {
    date: NaiveDate,
    sunrise: Option<DateTime<Utc>>,
    sunset: Option<DateTime<Utc>>,
    moonrise: Option<DateTime<Utc>>,
    moonset: Option<DateTime<Utc>>,
    /// Low waters: (time, metres).
    lows: Vec<(DateTime<Utc>, f64)>,
    /// Cruise liners calling that day (known months ahead).
    cruises: usize,
    /// Passages in and out by every other ship; None past the end of the
    /// port schedule, where they are not known yet.
    others: Option<usize>,
    /// Yacht club races and regattas that day.
    racing: Vec<String>,
    /// WMO code summing up the daylight hours, if forecast.
    weather: Option<u8>,
}

/// `extremes` may cover more than the day; only the day's low waters count.
pub fn summary(
    date: NaiveDate,
    extremes: &[(Extreme, DateTime<Utc>, f64)],
    cruises: usize,
    others: Option<usize>,
    sailing: &[Session],
    weather: &[(DateTime<Utc>, Hour)],
) -> Summary {
    let day = Window::new(date);
    let first = |track: &astro::Track, kind| {
        track.events.iter().find(|e| e.kind == kind).map(|e| e.time)
    };
    let sun = astro::track(Body::Sun, site::POINT, day.start, day.end);
    let moon = astro::track(Body::Moon, site::POINT, day.start, day.end);
    Summary {
        date,
        sunrise: first(&sun, EventKind::Rise),
        sunset: first(&sun, EventKind::Set),
        moonrise: first(&moon, EventKind::Rise),
        moonset: first(&moon, EventKind::Set),
        lows: extremes
            .iter()
            .filter(|(kind, t, _)| *kind == Extreme::Low && day.contains(*t))
            .map(|&(_, t, level)| (t, level))
            .collect(),
        cruises,
        others,
        racing: sailing
            .iter()
            .filter(|s| s.kind == Kind::Racing)
            .map(|s| s.title.clone())
            .collect(),
        weather: weather::daytime_code(weather),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use serde_json::Value;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn day_window_follows_irish_time() {
        // Summer time: local midnight is 23:00 UTC the day before.
        let w = Window::new(d(2026, 10, 2));
        assert_eq!(w.start, Utc.with_ymd_and_hms(2026, 10, 1, 23, 0, 0).unwrap());
        assert_eq!(w.end, Utc.with_ymd_and_hms(2026, 10, 2, 23, 0, 0).unwrap());
        // Clocks go back on 25 October 2026: that day is 25 hours long.
        let w = Window::new(d(2026, 10, 25));
        assert_eq!((w.end - w.start).num_hours(), 25);
    }

    #[test]
    fn day_json() {
        let date = d(2026, 10, 2);
        let window = Window::new(date);
        let series: Series = (0..=288)
            .map(|i| {
                let t = window.start + TimeDelta::minutes(5 * i);
                let level = 2.0 + 1.5 * (i as f64 / 288.0 * 4.0 * std::f64::consts::PI).sin();
                (t, level)
            })
            .collect();
        let day = build(date, window.start, "Crosshaven_MODELLED", Ok(series), Vec::new(), Vec::new(), Vec::new());
        let json: Value = serde_json::to_value(&day).unwrap();

        assert_eq!(json["date"], "2026-10-02");
        assert_eq!(json["site"]["tz"], "Europe/Dublin");
        assert_eq!(json["start"], "2026-10-01T23:00:00Z");

        let sun = &json["sun"]["events"];
        assert_eq!(sun[0]["kind"], "rise");
        assert_eq!(sun[0]["compass"], "E");
        assert!(sun[0]["time"].as_str().unwrap().starts_with("2026-10-02T06:3"));
        assert_eq!(sun[1]["kind"], "set");
        assert_eq!(json["sun"]["up"].as_array().unwrap().len(), 1);

        assert_eq!(json["moon"]["phase"], "Waning gibbous");
        assert!(json["moon"]["events"].as_array().is_some());

        let tides = &json["tides"];
        assert!(tides["error"].is_null());
        assert_eq!(tides["hourly"].as_array().unwrap().len(), 24);
        assert_eq!(tides["hourly"][0], 2.0);
        assert_eq!(tides["extremes"][0]["kind"], "high");
        assert_eq!(tides["series"][0][1], 2.0);
    }

    #[test]
    fn summary_of_a_day() {
        let date = d(2026, 10, 2);
        let w = Window::new(date);
        let at = |h| w.start + TimeDelta::hours(h);
        let extremes = [
            (Extreme::Low, at(-2), 0.5),
            (Extreme::Low, at(3), 0.64),
            (Extreme::High, at(9), 3.58),
            (Extreme::Low, at(16), 0.76),
        ];
        let s = serde_json::to_value(summary(date, &extremes, 1, Some(12), &[], &[])).unwrap();
        assert_eq!(s["cruises"], 1);
        assert_eq!(s["date"], "2026-10-02");
        assert_eq!(s["others"], 12);
        assert_eq!(s["lows"].as_array().unwrap().len(), 2);
        assert_eq!(s["lows"][0][1], 0.64);
        assert!(s["sunrise"].as_str().unwrap().starts_with("2026-10-02T06:3"));
        assert!(s["moonrise"].is_string() && s["moonset"].is_string());
        assert!(s["weather"].is_null());
    }

    #[test]
    fn tide_error_is_reported() {
        let day = build(d(2026, 10, 2), Utc::now(), "Crosshaven_MODELLED", Err("boom".into()), Vec::new(), Vec::new(), Vec::new());
        let json: Value = serde_json::to_value(&day).unwrap();
        assert_eq!(json["tides"]["error"], "boom");
        assert_eq!(json["tides"]["series"].as_array().unwrap().len(), 0);
    }
}
