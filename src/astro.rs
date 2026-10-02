//! Sun and Moon positions from the low-precision formulas of the
//! Astronomical Almanac (Sun ~0.01°, Moon ~0.3°): good to a minute or two
//! for rise and set times, which is all a shoot plan needs.

use chrono::{DateTime, TimeDelta, Utc};
use serde::Serialize;

/// A point on the Earth in decimal degrees (east longitude positive).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub lat: f64,
    pub lon: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Body {
    Sun,
    Moon,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EventKind {
    Rise,
    Set,
}

/// A rise or set: when, and the compass bearing (degrees from north).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Event {
    pub kind: EventKind,
    pub time: DateTime<Utc>,
    pub azimuth: f64,
}

/// When a body is above the horizon within a time window.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct Track {
    pub up: Vec<(DateTime<Utc>, DateTime<Utc>)>,
    pub events: Vec<Event>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MoonPhase {
    /// Illuminated fraction of the disc, 0..1.
    pub illuminated: f64,
    pub waxing: bool,
}

/// Equatorial position: right ascension and declination (radians),
/// plus the altitude (degrees) at which the body's upper limb touches the
/// horizon, refraction included.
struct Equatorial {
    ra: f64,
    dec: f64,
    horizon: f64,
}

/// Days since J2000.0 (2000-01-01 12:00 TT, ignoring the TT-UT offset).
fn days_since_j2000(t: DateTime<Utc>) -> f64 {
    let unix = t.timestamp() as f64 + f64::from(t.timestamp_subsec_millis()) / 1000.0;
    unix / 86_400.0 + 2_440_587.5 - 2_451_545.0
}

fn sin_d(x: f64) -> f64 {
    x.to_radians().sin()
}

fn cos_d(x: f64) -> f64 {
    x.to_radians().cos()
}

fn obliquity(d: f64) -> f64 {
    23.439 - 0.000_000_4 * d
}

/// Ecliptic longitude of the Sun in degrees.
fn sun_longitude(d: f64) -> f64 {
    let l = 280.460 + 0.985_647_4 * d;
    let g = 357.528 + 0.985_600_3 * d;
    (l + 1.915 * sin_d(g) + 0.020 * sin_d(2.0 * g)).rem_euclid(360.0)
}

/// Ecliptic longitude, latitude (degrees) and horizontal parallax
/// (degrees) of the Moon.
fn moon_ecliptic(d: f64) -> (f64, f64, f64) {
    let t = d / 36_525.0;
    let lon = 218.32 + 481_267.881 * t
        + 6.29 * sin_d(135.0 + 477_198.87 * t)
        - 1.27 * sin_d(259.3 - 413_335.36 * t)
        + 0.66 * sin_d(235.7 + 890_534.22 * t)
        + 0.21 * sin_d(269.9 + 954_397.74 * t)
        - 0.19 * sin_d(357.5 + 35_999.05 * t)
        - 0.11 * sin_d(186.5 + 966_404.03 * t);
    let lat = 5.13 * sin_d(93.3 + 483_202.02 * t)
        + 0.28 * sin_d(228.2 + 960_400.89 * t)
        - 0.28 * sin_d(318.3 + 6_003.15 * t)
        - 0.17 * sin_d(217.6 - 407_332.21 * t);
    let parallax = 0.9508
        + 0.0518 * cos_d(135.0 + 477_198.87 * t)
        + 0.0095 * cos_d(259.3 - 413_335.36 * t)
        + 0.0078 * cos_d(235.7 + 890_534.22 * t)
        + 0.0028 * cos_d(269.9 + 954_397.74 * t);
    (lon.rem_euclid(360.0), lat, parallax)
}

fn ecliptic_to_equatorial(lon: f64, lat: f64, eps: f64) -> (f64, f64) {
    let (lon, lat, eps) = (lon.to_radians(), lat.to_radians(), eps.to_radians());
    let ra = (lon.sin() * eps.cos() - lat.tan() * eps.sin()).atan2(lon.cos());
    let dec = (lat.sin() * eps.cos() + lat.cos() * eps.sin() * lon.sin()).asin();
    (ra, dec)
}

fn equatorial(body: Body, d: f64) -> Equatorial {
    let eps = obliquity(d);
    match body {
        Body::Sun => {
            let (ra, dec) = ecliptic_to_equatorial(sun_longitude(d), 0.0, eps);
            Equatorial { ra, dec, horizon: -0.833 }
        }
        Body::Moon => {
            let (lon, lat, parallax) = moon_ecliptic(d);
            let (ra, dec) = ecliptic_to_equatorial(lon, lat, eps);
            // Meeus, Astronomical Algorithms ch. 15: geocentric altitude of
            // moonrise, accounting for parallax, semidiameter and refraction.
            Equatorial { ra, dec, horizon: 0.7275 * parallax - 0.5667 }
        }
    }
}

/// Altitude above the horizon and azimuth from north, both in degrees.
pub fn position(body: Body, t: DateTime<Utc>, p: Point) -> (f64, f64) {
    let (alt, az, _) = position_with_horizon(body, t, p);
    (alt, az)
}

fn position_with_horizon(body: Body, t: DateTime<Utc>, p: Point) -> (f64, f64, f64) {
    let d = days_since_j2000(t);
    let eq = equatorial(body, d);
    let gmst = 280.460_618_37 + 360.985_647_366_29 * d;
    let ha = (gmst + p.lon).to_radians() - eq.ra;
    let lat = p.lat.to_radians();

    let alt = (lat.sin() * eq.dec.sin() + lat.cos() * eq.dec.cos() * ha.cos()).asin();
    let az = ha.sin().atan2(ha.cos() * lat.sin() - eq.dec.tan() * lat.cos());
    (alt.to_degrees(), (az.to_degrees() + 180.0).rem_euclid(360.0), eq.horizon)
}

/// Height above the rise/set threshold; positive while the body is up.
fn clearance(body: Body, t: DateTime<Utc>, p: Point) -> f64 {
    let (alt, _, horizon) = position_with_horizon(body, t, p);
    alt - horizon
}

/// When `body` is above the horizon between `start` and `end`, sampled
/// once a minute with crossings interpolated to the second.
pub fn track(body: Body, p: Point, start: DateTime<Utc>, end: DateTime<Utc>) -> Track {
    let step = TimeDelta::minutes(1);
    let mut out = Track::default();

    let mut t0 = start;
    let mut c0 = clearance(body, t0, p);
    let mut up_since = (c0 > 0.0).then_some(start);

    while t0 < end {
        let t1 = (t0 + step).min(end);
        let c1 = clearance(body, t1, p);
        if (c0 > 0.0) != (c1 > 0.0) {
            let span = (t1 - t0).num_milliseconds() as f64;
            let at = t0 + TimeDelta::milliseconds((span * c0 / (c0 - c1)).round() as i64);
            let azimuth = position(body, at, p).1;
            if c1 > 0.0 {
                out.events.push(Event { kind: EventKind::Rise, time: at, azimuth });
                up_since = Some(at);
            } else {
                out.events.push(Event { kind: EventKind::Set, time: at, azimuth });
                if let Some(since) = up_since.take() {
                    out.up.push((since, at));
                }
            }
        }
        t0 = t1;
        c0 = c1;
    }
    if let Some(since) = up_since {
        out.up.push((since, end));
    }
    out
}

pub fn moon_phase(t: DateTime<Utc>) -> MoonPhase {
    let d = days_since_j2000(t);
    let (moon_lon, moon_lat, _) = moon_ecliptic(d);
    let elongation = (moon_lon - sun_longitude(d)).rem_euclid(360.0);
    // Cosine of the Sun-Earth-Moon angle; the phase angle is its supplement.
    let cos_psi = cos_d(moon_lat) * cos_d(elongation);
    MoonPhase {
        illuminated: (1.0 - cos_psi) / 2.0,
        waxing: elongation < 180.0,
    }
}

impl MoonPhase {
    pub fn name(self) -> &'static str {
        match (self.illuminated, self.waxing) {
            (k, _) if k < 0.03 => "New moon",
            (k, _) if k > 0.97 => "Full moon",
            (k, true) if k < 0.47 => "Waxing crescent",
            (k, true) if k <= 0.53 => "First quarter",
            (_, true) => "Waxing gibbous",
            (k, false) if k < 0.47 => "Waning crescent",
            (k, false) if k <= 0.53 => "Last quarter",
            (_, false) => "Waning gibbous",
        }
    }

    pub fn emoji(self) -> &'static str {
        match self.name() {
            "New moon" => "🌑",
            "Waxing crescent" => "🌒",
            "First quarter" => "🌓",
            "Waxing gibbous" => "🌔",
            "Full moon" => "🌕",
            "Waning gibbous" => "🌖",
            "Last quarter" => "🌗",
            _ => "🌘",
        }
    }
}

/// 16-point compass name for an azimuth in degrees.
pub fn compass(azimuth: f64) -> &'static str {
    const NAMES: [&str; 16] = [
        "N", "NNE", "NE", "ENE", "E", "ESE", "SE", "SSE", "S", "SSW", "SW", "WSW", "W", "WNW",
        "NW", "NNW",
    ];
    NAMES[((azimuth / 22.5).round() as usize) % 16]
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    const LONDON: Point = Point { lat: 51.5074, lon: -0.1278 };

    fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap()
    }

    fn assert_near(actual: DateTime<Utc>, expected: DateTime<Utc>, minutes: i64) {
        let diff = (actual - expected).num_seconds().abs();
        assert!(diff <= minutes * 60, "{actual} is {diff}s from {expected}");
    }

    #[test]
    fn london_midsummer_sunrise_and_sunset() {
        // Published: sunrise 04:43 BST, sunset 21:21 BST on 21 June 2024.
        let tr = track(Body::Sun, LONDON, utc(2024, 6, 20, 23, 0), utc(2024, 6, 21, 23, 0));
        assert_eq!(tr.events.len(), 2);
        assert_eq!(tr.events[0].kind, EventKind::Rise);
        assert_near(tr.events[0].time, utc(2024, 6, 21, 3, 43), 2);
        assert_eq!(tr.events[1].kind, EventKind::Set);
        assert_near(tr.events[1].time, utc(2024, 6, 21, 20, 21), 2);
        assert_eq!(tr.up, vec![(tr.events[0].time, tr.events[1].time)]);
        // Midsummer sun rises in the north-east and sets in the north-west.
        assert!((45.0..55.0).contains(&tr.events[0].azimuth));
        assert!((305.0..315.0).contains(&tr.events[1].azimuth));
    }

    #[test]
    fn noon_sun_is_due_south() {
        let (alt, az) = position(Body::Sun, utc(2024, 6, 21, 12, 2), LONDON);
        assert!((61.0..63.0).contains(&alt), "alt {alt}");
        assert!((175.0..185.0).contains(&az), "az {az}");
    }

    #[test]
    fn polar_night_has_no_sunrise() {
        let svalbard = Point { lat: 78.22, lon: 15.65 };
        let tr = track(Body::Sun, svalbard, utc(2024, 12, 21, 0, 0), utc(2024, 12, 22, 0, 0));
        assert!(tr.up.is_empty() && tr.events.is_empty());
    }

    #[test]
    fn moon_phases() {
        // Full moon 25 Jan 2024 17:54 UTC, new moon 11 Jan 2024 11:57 UTC,
        // first quarter 18 Jan 2024 03:52 UTC.
        let full = moon_phase(utc(2024, 1, 25, 17, 54));
        assert!(full.illuminated > 0.99, "{full:?}");
        let new = moon_phase(utc(2024, 1, 11, 11, 57));
        assert!(new.illuminated < 0.01, "{new:?}");
        let quarter = moon_phase(utc(2024, 1, 18, 3, 52));
        assert!((0.45..0.55).contains(&quarter.illuminated), "{quarter:?}");
        assert!(quarter.waxing);
        assert_eq!(quarter.name(), "First quarter");
    }

    #[test]
    fn moon_rises_and_sets_about_once_a_day() {
        let tr = track(Body::Moon, LONDON, utc(2024, 1, 1, 0, 0), utc(2024, 1, 31, 0, 0));
        let rises = tr.events.iter().filter(|e| e.kind == EventKind::Rise).count();
        // The Moon rises ~50 minutes later each day: 29 rises in 30 days.
        assert!((28..=30).contains(&rises), "{rises} rises");
    }

    #[test]
    fn compass_points() {
        assert_eq!(compass(0.0), "N");
        assert_eq!(compass(359.0), "N");
        assert_eq!(compass(90.0), "E");
        assert_eq!(compass(250.0), "WSW");
    }
}
