mod astro;
mod day;
mod sailing;
mod ships;
mod site;
mod tides;
mod weather;
mod web;

use std::net::{Ipv4Addr, SocketAddr};

/// Port when the `PORT` environment variable is not set.
const DEFAULT_PORT: u16 = 6001;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let port = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(DEFAULT_PORT);
    // Listen on every interface so the calendar is reachable from other devices.
    web::serve(SocketAddr::from((Ipv4Addr::UNSPECIFIED, port))).await
}
