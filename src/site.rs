//! The camera point the calendar plans for.

use chrono_tz::Tz;

use crate::astro::Point;

pub const NAME: &str = "Roches Point";
pub const POINT: Point = Point { lat: 51.7944, lon: -8.2379 };
/// Times on the pages are local to the camera point.
pub const TZ: Tz = chrono_tz::Europe::Dublin;
