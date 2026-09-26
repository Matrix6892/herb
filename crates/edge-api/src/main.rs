//! `edge-api`, configured from the environment (secrets and paths live in a
//! file outside the repository, spec §9):
//!
//! - `VITRINA_DB` — SQLite file, default `var/vitrina.db`
//! - `VITRINA_API_ADDR` — listen address, default `127.0.0.1:8081`
//! - `VITRINA_CLIENT_IP_HEADER` — e.g. `CF-Connecting-IP` behind a CDN
//! - `VITRINA_SESSION_SALT` — optional salt for `session_hash`
//! - `VITRINA_ALLOWED_ORIGIN` — only if pages and API are on different origins

use std::net::SocketAddr;
use std::path::PathBuf;

use axum::http::HeaderValue;
use catalog_store::SqliteStore;
use edge_api::{AppState, Config, router};

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db = PathBuf::from(env("VITRINA_DB").unwrap_or_else(|| "var/vitrina.db".into()));
    let addr: SocketAddr = env("VITRINA_API_ADDR").unwrap_or_else(|| "127.0.0.1:8081".into()).parse()?;
    let allowed_origin = env("VITRINA_ALLOWED_ORIGIN").map(|o| HeaderValue::from_str(&o)).transpose()?;
    if let Some(parent) = db.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let store = SqliteStore::open(&db)?;
    let state = AppState::new(
        store,
        Config {
            client_ip_header: env("VITRINA_CLIENT_IP_HEADER"),
            session_salt: env("VITRINA_SESSION_SALT").unwrap_or_default(),
            allowed_origin,
        },
    );
    let listener = tokio::net::TcpListener::bind(addr).await?;
    eprintln!("edge-api: listening on {addr}, database {}", db.display());
    axum::serve(listener, router(state).into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
