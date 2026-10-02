# ShoreCast

**shore + forecast** — a shoot-planning tool for coastal filming.

A local web app that shows, day by day, everything that decides a shot
from one camera point: sun, moon, weather, tides, ships and yachts.
Built for long static-camera takes on the Irish coast — golden hours,
moonrises over the water, passing ships — where the shot is decided
hours before the camera arrives.

The camera point is **Roches Point** at the mouth of Cork Harbour
(51.7944, −8.2379); all times are Irish time (Europe/Dublin).

## Running

```
cargo build --release
target/release/shorecast            # http://localhost:6001
```

No flags. The server listens on every interface (`0.0.0.0:6001`), so
the calendar opens from a phone on the same network too; set `PORT` to
use another port. On first start it downloads what it needs into
`data/` and keeps it fresh in the background; afterwards it works
offline from the saved copies.

## Pages

### Month calendar — `/` and `/month/YYYY/M`

Six Monday-first weeks. Each day cell is the day in miniature:

| Line | Meaning |
|---|---|
| weather icon (top right) | daytime weather, from the hourly forecast |
| 🛳️ **N** ⛵ M | cruise liners calling (gold) · passages in and out by other ships; ⛵ is left out where the port schedule does not reach yet |
| 🏁 Cork Week 2026 | yacht club racing that day |
| ☀️ 07:35–19:08 | sunrise – sunset |
| 🌙 ↑22:02 ↓15:21 | moonrise, moonset |
| 🌊 03:25 · 15:55 | low waters |

### Day — `/day/YYYY-MM-DD`

A big clock (time at the camera point) and one 24-hour strip per
source, 00:00 on the left, 24:00 on the right, with a red line at the
current moment that moves by itself:

- **Sun** — yellow while the sun is up; sunrise/sunset times and
  azimuths, length of daylight. Hours are written on the strip.
- **Moon** — silver while the moon is up; phase and illumination,
  moonrise/moonset and their azimuths.
- **Weather** — one coloured block per hour with its icon (clear, cloud,
  rain, fog, thunder…), and below it the hour, °C and wind km/h. Hover a
  column for details: wind direction and gusts, cloud, rain, visibility.
- **Tides** — the water level curve with each high and low water
  labelled on it, and the level at every hour below.
- **Ships** — a label per arrival and departure, coloured by kind of
  ship; ▲ under the label means arriving, ▼ leaving. Cruise liners are
  gold. Hover for the full card; a table below lists them all.

  | Kind | Colour |
  |---|---|
  | 🛳️ cruise | gold |
  | ⛴️ ferry | red |
  | 📦 container | yellow |
  | 🛢️ tanker | purple |
  | 🚢 cargo / bulk | green |
  | 🎣 fishing | blue |
  | ⚓ navy | light blue |
  | 🚤 other | white |

  Times mean different things: for an arrival it is when the pilot
  boards off the harbour mouth (the ship is about to pass the point);
  for a departure it is when the ship leaves its berth (it passes the
  point later, depending on the berth); for cruise calls from the
  cruise schedule it is the time at the berth in Cobh.
- **Sailing** — graphite bars for yacht club racing 🏁, training 🎓
  and club cruises 🧭; click one for the club's page.
- **Plan** — still to come: a single answer — time window, camera
  bearing, what happens in frame.

## Data sources

Everything that cannot be computed is downloaded, saved under `data/`
(not in git) and refreshed in the background. If a source is down, the
saved copy is used.

| What | Source | Refreshed | Saved as |
|---|---|---|---|
| Sun, moon | computed (Astronomical Almanac low-precision formulas, within a minute or two) | — | — |
| Tides | [Marine Institute](https://erddap.marine.ie/) ERDDAP, 5-minute predictions at the nearest station (Crosshaven), metres above chart datum; available to the end of 2028 | each month file every 30 days; all months fetched at start | `data/tides/<station>/YYYY-MM.csv` (~230 KB a month) |
| Weather | [Open-Meteo](https://open-meteo.com/) hourly forecast, 16 days ahead and 7 back | every 3 hours | `data/weather/open-meteo.json` |
| Ships | [Port of Cork](https://www.portofcork.ie/) shipping schedule (the open ArcGIS service behind its dashboard): planned arrivals and departures about a week ahead, plus a log of movements back to January 2025 with actual times | schedule every 3 hours, log daily | `data/ships/ships.json` |
| Cruise liners | Port of Cork [cruise schedule](https://www.portofcork.ie/cruise-schedule/) page: the whole season | weekly | `data/ships/ships.json` |
| Sailing | [Royal Cork Yacht Club](https://www.royalcork.com/events/) calendar (The Events Calendar REST API); dinners, holidays and other club life ashore are filtered out | daily | `data/sailing/royal-cork.json` |

## How it is built

- **Server** — Rust (axum, tokio). It computes the astronomy, keeps the
  downloaded data and serves it as JSON:
  - `GET /api/site` — camera point, time zone, server time
  - `GET /api/day/YYYY-MM-DD` — everything for one day
  - `GET /api/summary?from=YYYY-MM-DD&to=YYYY-MM-DD` — day summaries
    for the calendar (up to 62 days)
- **Pages** — plain HTML, CSS and JavaScript in `web/`, read from disk
  on every request: edit them and refresh the browser, no rebuild
  needed. Only changes to the Rust code need `cargo build`.

| File | Does |
|---|---|
| `src/main.rs` | starts the server on `PORT` (default 6001) |
| `src/web.rs` | routes, page files, JSON endpoints |
| `src/site.rs` | the camera point and its time zone |
| `src/astro.rs` | sun and moon positions, rise/set, moon phase |
| `src/tides.rs` | tide download, monthly files, highs and lows |
| `src/weather.rs` | Open-Meteo download and saved hours |
| `src/ships.rs` | port schedule, movement log, cruise page, ship kinds |
| `src/sailing.rs` | yacht club calendar, racing/training/cruising filter |
| `src/day.rs` | the day and summary JSON |
| `web/month.*`, `web/day.*` | the two pages |
| `web/common.js` | shared helpers, weather icons and colours |
| `web/style.css` | dark theme and layout |

```
cargo test
```

## Ideas

- Per-berth delay so departures show when they pass the camera point
- Golden and blue hour on the sun strip
- Ship photos in the ship card
- More sources: ferry timetables beyond a week, other harbour sailing
  clubs, live AIS for actual passing times

## Relatives

- [geoslate](https://github.com/gravitymir/geoslate) — titles and
  map-intro renderer; hosts the `camera-points/` registry of past
  shoots and OSM map extracts of the Irish coastline
- WalkLog — Android GPS logger that records where the camera stood

## License

MIT
