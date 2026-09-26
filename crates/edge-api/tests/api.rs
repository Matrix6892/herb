use std::net::SocketAddr;

use axum::body::Body;
use axum::extract::connect_info::MockConnectInfo;
use axum::http::{Request, StatusCode, header};
use catalog_store::SqliteStore;
use edge_api::{AppState, Config, MAX_BODY_BYTES, REQUESTS_PER_MINUTE, router, session_hash};
use tower::ServiceExt;

// 2026-09-26 12:00 UTC
fn clock() -> i64 {
    20_722 * 86_400 + 12 * 3600
}

fn app(header_name: Option<&str>) -> (axum::Router, std::sync::Arc<AppState>) {
    let state = AppState::with_clock(
        SqliteStore::open_in_memory().unwrap(),
        Config {
            client_ip_header: header_name.map(str::to_owned),
            session_salt: "salt".into(),
            allowed_origin: None,
        },
        clock,
    );
    let app = router(state.clone()).layer(MockConnectInfo(SocketAddr::from(([203, 0, 113, 7], 5000))));
    (app, state)
}

fn post(path: &str, body: &str) -> Request<Body> {
    Request::post(path)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(header::USER_AGENT, "TestAgent/1.0")
        .body(Body::from(body.to_owned()))
        .unwrap()
}

#[tokio::test]
async fn click_is_stored_and_answers_204_without_body() {
    let (app, state) = app(None);
    let resp = app
        .oneshot(post("/api/click", "product=900002&preset=balance&page=%2Fc%2Fmagnesium"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
    assert!(body.is_empty());
    assert_eq!(state.with_store(|s| s.count_clicks().unwrap()), 1);
}

#[tokio::test]
async fn bad_input_is_rejected_without_storing() {
    let (app, state) = app(None);
    for body in [
        "product=abc&preset=balance&page=%2F",
        "product=1&preset=commission&page=%2F",
        "product=1&preset=balance&page=https%3A%2F%2Fevil.example",
        "product=1&preset=balance",
    ] {
        let resp = app.clone().oneshot(post("/api/click", body)).await.unwrap();
        assert!(resp.status().is_client_error(), "{body}: {}", resp.status());
    }
    for body in [
        "field=price&text=&page_url=%2F",
        "field=secret&text=x&page_url=%2F",
        "product=x&field=price&text=x&page_url=%2F",
    ] {
        let resp = app.clone().oneshot(post("/api/report", body)).await.unwrap();
        assert!(resp.status().is_client_error(), "{body}: {}", resp.status());
    }
    assert_eq!(state.with_store(|s| s.count_clicks().unwrap() + s.count_reports().unwrap()), 0);
}

#[tokio::test]
async fn report_is_stored() {
    let (app, state) = app(None);
    let resp = app
        .oneshot(post(
            "/api/report",
            "product=900002&field=label&text=The+dose+is+250+mg&page_url=%2Fpr%2Fx%2F900002",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    assert_eq!(state.with_store(|s| s.count_reports().unwrap()), 1);
}

#[tokio::test]
async fn bodies_over_4_kb_are_refused() {
    let (app, _) = app(None);
    let text = "x".repeat(MAX_BODY_BYTES);
    let resp = app
        .oneshot(post("/api/report", &format!("field=other&text={text}&page_url=%2F")))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn sixty_requests_a_minute_per_address() {
    let (app, _) = app(Some("CF-Connecting-IP"));
    let send = |ip: &'static str| {
        let mut req = post("/api/click", "product=1&preset=&page=%2F");
        req.headers_mut().insert("CF-Connecting-IP", ip.parse().unwrap());
        app.clone().oneshot(req)
    };
    for _ in 0..REQUESTS_PER_MINUTE {
        assert_eq!(send("198.51.100.1").await.unwrap().status(), StatusCode::NO_CONTENT);
    }
    assert_eq!(send("198.51.100.1").await.unwrap().status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(send("198.51.100.2").await.unwrap().status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn only_the_two_post_routes_exist() {
    let (app, _) = app(None);
    let resp = app
        .clone()
        .oneshot(Request::get("/api/click").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
    let resp = app.oneshot(Request::get("/").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[test]
fn session_hash_changes_daily_and_hides_the_agent() {
    let a = session_hash("s", "Agent", "2026-09-26");
    assert_eq!(a.len(), 16);
    assert_ne!(a, session_hash("s", "Agent", "2026-09-27"));
    assert_eq!(a, session_hash("s", "Agent", "2026-09-26"));
    assert!(!a.contains("Agent"));
}
