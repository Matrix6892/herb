//! edge-api (spec §8): `POST /api/click` and `POST /api/report`.
//!
//! Bodies are form-encoded (what `navigator.sendBeacon` and a plain HTML form
//! send), at most 4 KB. Every accepted request answers 204 with an empty
//! body; this service never serves HTML. The client IP is used only for the
//! in-memory rate limit and is never stored. `session_hash` is computed here
//! from the user agent and the UTC date, so it changes daily and needs no
//! cookie.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::Router;
use axum::extract::connect_info::MockConnectInfo;
use axum::extract::{ConnectInfo, DefaultBodyLimit, Form, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use catalog_core::{IherbId, Preset, UtcTimestamp};
use catalog_store::{ClickEvent, ErrorReport, EventSink, SqliteStore};
use serde::Deserialize;
use sha2::{Digest, Sha256};

pub const MAX_BODY_BYTES: usize = 4 * 1024;
pub const REQUESTS_PER_MINUTE: u32 = 60;
const REPORT_FIELDS: [&str; 4] = ["price", "label", "rating", "other"];

pub struct Config {
    /// Header carrying the client address when behind a proxy or CDN, such
    /// as `CF-Connecting-IP`. Without it the socket peer is used.
    pub client_ip_header: Option<String>,
    /// Mixed into `session_hash`; optional.
    pub session_salt: String,
    /// Sent as `Access-Control-Allow-Origin` when the API is on another
    /// origin than the pages.
    pub allowed_origin: Option<HeaderValue>,
}

struct RateLimiter {
    window_start: Instant,
    counts: HashMap<IpAddr, u32>,
}

impl RateLimiter {
    /// Fixed one-minute windows; the whole table is dropped at each new
    /// window, so addresses live in memory for a minute at most.
    fn allow(&mut self, ip: IpAddr, now: Instant) -> bool {
        if now.duration_since(self.window_start) >= Duration::from_secs(60) {
            self.window_start = now;
            self.counts.clear();
        }
        let n = self.counts.entry(ip).or_insert(0);
        *n += 1;
        *n <= REQUESTS_PER_MINUTE
    }
}

pub struct AppState {
    store: Mutex<SqliteStore>,
    limiter: Mutex<RateLimiter>,
    config: Config,
    clock: fn() -> i64,
}

fn system_clock() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

impl AppState {
    pub fn new(store: SqliteStore, config: Config) -> Arc<AppState> {
        AppState::with_clock(store, config, system_clock)
    }

    pub fn with_clock(store: SqliteStore, config: Config, clock: fn() -> i64) -> Arc<AppState> {
        Arc::new(AppState {
            store: Mutex::new(store),
            limiter: Mutex::new(RateLimiter {
                window_start: Instant::now(),
                counts: HashMap::new(),
            }),
            config,
            clock,
        })
    }

    pub fn with_store<T>(&self, f: impl FnOnce(&SqliteStore) -> T) -> T {
        f(&self.store.lock().unwrap_or_else(std::sync::PoisonError::into_inner))
    }
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/click", post(click))
        .route("/api/report", post(report))
        .layer(middleware::from_fn_with_state(state.clone(), rate_limit))
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .with_state(state)
}

fn client_ip(state: &AppState, headers: &HeaderMap, peer: Option<SocketAddr>) -> Option<IpAddr> {
    if let Some(name) = &state.config.client_ip_header {
        return headers.get(name.as_str())?.to_str().ok()?.split(',').next()?.trim().parse().ok();
    }
    peer.map(|p| p.ip())
}

async fn rate_limit(State(state): State<Arc<AppState>>, request: Request, next: Next) -> Response {
    let ext = request.extensions();
    let peer = ext
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0)
        .or_else(|| ext.get::<MockConnectInfo<SocketAddr>>().map(|c| c.0));
    let Some(ip) = client_ip(&state, request.headers(), peer) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let allowed = state
        .limiter
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .allow(ip, Instant::now());
    if !allowed {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    }
    let mut response = next.run(request).await;
    if let Some(origin) = &state.config.allowed_origin {
        response.headers_mut().insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin.clone());
    }
    response
}

/// `sha256(salt ‖ user agent ‖ date)`, first 16 hex digits.
pub fn session_hash(salt: &str, user_agent: &str, date: &str) -> String {
    let mut h = Sha256::new();
    for part in [salt, user_agent, date] {
        h.update(part.as_bytes());
        h.update([0]);
    }
    hex::encode(h.finalize())[..16].to_owned()
}

fn valid_page(page: &str) -> bool {
    page.starts_with('/') && !page.starts_with("//") && page.len() <= 300 && !page.chars().any(char::is_control)
}

#[derive(Deserialize)]
pub struct ClickForm {
    product: String,
    #[serde(default)]
    preset: String,
    page: String,
}

async fn click(State(state): State<Arc<AppState>>, headers: HeaderMap, Form(form): Form<ClickForm>) -> StatusCode {
    let Ok(product) = IherbId::new(form.product) else {
        return StatusCode::BAD_REQUEST;
    };
    if !(form.preset.is_empty() || Preset::from_key(&form.preset).is_some()) || !valid_page(&form.page) {
        return StatusCode::BAD_REQUEST;
    }
    let ts = UtcTimestamp::from_unix((state.clock)());
    let date = ts.date().map(|d| d.to_string()).unwrap_or_default();
    let ua = headers.get(header::USER_AGENT).and_then(|v| v.to_str().ok()).unwrap_or("");
    let event = ClickEvent {
        product,
        preset: form.preset,
        page: form.page,
        ts,
        session_hash: session_hash(&state.config.session_salt, ua, &date),
    };
    store_blocking(state, move |s| s.insert_click(&event)).await
}

#[derive(Deserialize)]
pub struct ReportForm {
    #[serde(default)]
    product: String,
    field: String,
    text: String,
    page_url: String,
}

async fn report(State(state): State<Arc<AppState>>, Form(form): Form<ReportForm>) -> StatusCode {
    let product = match form.product.as_str() {
        "" => None,
        p => match IherbId::new(p) {
            Ok(id) => Some(id),
            Err(_) => return StatusCode::BAD_REQUEST,
        },
    };
    let text = form.text.trim().to_owned();
    if !REPORT_FIELDS.contains(&form.field.as_str()) || text.is_empty() || text.chars().count() > 2000 || !valid_page(&form.page_url) {
        return StatusCode::BAD_REQUEST;
    }
    let report = ErrorReport {
        product,
        field: form.field,
        text,
        created_at: UtcTimestamp::from_unix((state.clock)()),
        page_url: form.page_url,
    };
    store_blocking(state, move |s| s.insert_report(&report)).await
}

async fn store_blocking(
    state: Arc<AppState>,
    f: impl FnOnce(&SqliteStore) -> Result<(), catalog_store::StoreError> + Send + 'static,
) -> StatusCode {
    let result = tokio::task::spawn_blocking(move || state.with_store(f)).await;
    match result {
        Ok(Ok(())) => StatusCode::NO_CONTENT,
        Ok(Err(e)) => {
            eprintln!("edge-api: store error: {e}");
            StatusCode::SERVICE_UNAVAILABLE
        }
        Err(e) => {
            eprintln!("edge-api: task error: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}
