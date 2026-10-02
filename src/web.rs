//! HTTP server: JSON for the computed data, and the page files from the
//! `web/` folder, read from disk on every request so they can be edited
//! without rebuilding.

use std::net::SocketAddr;
use std::path::{Component, Path as FsPath, PathBuf};
use std::sync::Arc;

use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::get;
use chrono::{NaiveDate, Utc};

use crate::{day, sailing, ships, site, tides, weather};

struct App {
    web: PathBuf,
    tides: tides::Client,
    ships: Arc<ships::Store>,
    weather: Arc<weather::Store>,
    sailing: Arc<sailing::Store>,
}

type AppState = State<Arc<App>>;

pub async fn serve(addr: SocketAddr) -> std::io::Result<()> {
    let web = find_web_dir().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "cannot find the web/ folder")
    })?;
    // Saved data sits next to web/, in data/.
    let data = web.parent().unwrap_or(&web).join("data");
    println!("Serving pages from {}", web.display());
    println!("Saving downloaded data in {}", data.display());

    let ships = Arc::new(ships::Store::open(data.join("ships")));
    tokio::spawn(Arc::clone(&ships).run());
    let weather = Arc::new(weather::Store::open(data.join("weather")));
    tokio::spawn(Arc::clone(&weather).run());
    let sailing = Arc::new(sailing::Store::open(data.join("sailing")));
    tokio::spawn(Arc::clone(&sailing).run());
    let state = Arc::new(App {
        web,
        tides: tides::Client::new(data.join("tides")),
        ships,
        weather,
        sailing,
    });
    let background = Arc::clone(&state);
    tokio::spawn(async move {
        let station = tides::nearest_station(site::POINT);
        if let Err(e) = background.tides.prefetch(station).await {
            eprintln!("tides {station}: cannot list predictions: {e}");
        }
    });

    let app = Router::new()
        .route("/", get(month_page))
        .route("/month/{year}/{month}", get(month_page))
        .route("/day/{date}", get(day_page))
        .route("/static/{*path}", get(static_file))
        .route("/api/site", get(site_json))
        .route("/api/day/{date}", get(day_json))
        .route("/api/summary", get(summary_json))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!("ShoreCast listening on http://{}", listener.local_addr()?);
    println!("Open http://localhost:{} in a browser", addr.port());
    axum::serve(listener, app).await
}

/// `web/` next to the executable or one of its parents (so
/// `target/release/shorecast.exe` finds the project's folder), else in the
/// current directory.
fn find_web_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok();
    let from_exe = exe.iter().flat_map(|e| e.ancestors().skip(1));
    let cwd = std::env::current_dir().ok();
    from_exe
        .chain(cwd.as_deref())
        .map(|dir| dir.join("web"))
        .find(|dir| dir.join("day.html").is_file())
}

async fn send_file(web: &FsPath, rel: &str) -> Response {
    let rel = FsPath::new(rel);
    // Only plain names below web/: no "..", no drive letters.
    if !rel.components().all(|c| matches!(c, Component::Normal(_))) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let content_type = match rel.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        _ => "application/octet-stream",
    };
    match tokio::fs::read(web.join(rel)).await {
        Ok(bytes) => (
            [(header::CONTENT_TYPE, content_type), (header::CACHE_CONTROL, "no-store")],
            bytes,
        )
            .into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn month_page(State(app): AppState) -> Response {
    send_file(&app.web, "month.html").await
}

async fn day_page(State(app): AppState) -> Response {
    send_file(&app.web, "day.html").await
}

async fn static_file(State(app): AppState, Path(path): Path<String>) -> Response {
    send_file(&app.web, &path).await
}

async fn site_json() -> Json<day::Site> {
    Json(day::site(Utc::now()))
}

async fn day_json(State(app): AppState, Path(date): Path<String>) -> Response {
    let Ok(date) = NaiveDate::parse_from_str(&date, "%Y-%m-%d") else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let window = day::Window::new(date);
    let station = tides::nearest_station(site::POINT);
    let series = app.tides.heights(station, window.start, window.end).await;
    let ships = app.ships.passages(window.start, window.end);
    let sailing = app.sailing.sessions(window.start, window.end);
    let weather = app.weather.hours(window.start, window.end);
    Json(day::build(date, Utc::now(), station, series, ships, sailing, weather)).into_response()
}

#[derive(serde::Deserialize)]
struct Range {
    from: NaiveDate,
    to: NaiveDate,
}

/// Per-day summaries for the month calendar, `from` to `to` inclusive.
async fn summary_json(State(app): AppState, Query(range): Query<Range>) -> Response {
    if range.to < range.from || (range.to - range.from).num_days() > 62 {
        return (StatusCode::BAD_REQUEST, "from..to must be at most 62 days").into_response();
    }
    let start = day::Window::new(range.from).start;
    let end = day::Window::new(range.to).end;
    let station = tides::nearest_station(site::POINT);
    // Missing tides only leave the low waters out of the cells.
    let series = app.tides.heights(station, start, end).await.unwrap_or_default();
    let extremes = tides::extremes(&series);
    let known_until = app.ships.known_until();
    let days: Vec<day::Summary> = range
        .from
        .iter_days()
        .take_while(|d| *d <= range.to)
        .map(|date| {
            let w = day::Window::new(date);
            let passages = app.ships.passages(w.start, w.end);
            let (liners, others): (Vec<_>, Vec<_>) =
                passages.iter().partition(|p| p.category == "cruise");
            let mut liners: Vec<&str> = liners.iter().map(|p| p.vessel.as_str()).collect();
            liners.sort_unstable();
            liners.dedup();
            // Past the port schedule only cruise calls are known.
            let known = known_until.is_some_and(|t| w.start <= t);
            let others = known.then_some(others.len());
            let sailing = app.sailing.sessions(w.start, w.end);
            let weather = app.weather.hours(w.start, w.end);
            day::summary(date, &extremes, liners.len(), others, &sailing, &weather)
        })
        .collect();
    Json(days).into_response()
}
