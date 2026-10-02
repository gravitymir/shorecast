# ShoreCast

**shore + forecast** — a shoot-planning tool for coastal filming.

You give it a camera point (latitude/longitude) and a date; it tells you
**where to stand, which way to look and when to press record**.

Built for long static-camera takes on the Irish coast — golden hours,
moonrises over the water, passing ships — where the shot is decided
hours before the camera arrives.

## What it will combine

- **Sun** — sunrise/sunset times, golden and blue hour windows,
  azimuths (does the sun set in your frame or behind you?)
- **Moon** — rise/set, phase, rising azimuth (moon path over water),
  culmination
- **Weather** — cloud cover, wind, precipitation, visibility,
  sea state / swell
- **Tides** — high/low water at the nearest station
- **Ships (AIS)** — tankers, cargo and cruise liners that will pass
  the point or transit the strait, and when
- **Plan** — a single answer: date, time window, camera bearing,
  what happens in frame

## Status

Early design stage. Data sources under evaluation; CLI sketch:

```
shorecast --point 51.7944,-8.2379 --date 2026-10-05
```

## Relatives

- [geoslate](https://github.com/gravitymir/geoslate) — titles and
  map-intro renderer; hosts the `camera-points/` registry of past
  shoots and OSM map extracts of the Irish coastline
- WalkLog — Android GPS logger that records where the camera stood

## License

MIT
