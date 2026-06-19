//! Web dashboard: router axum + security headers + bootstrap server.

pub mod api;
pub mod assets;
pub mod auth;
pub mod state;

use std::net::SocketAddr;

use anyhow::{Context, Result};
use axum::http::{HeaderName, HeaderValue};
use axum::routing::{get, post};
use axum::Router;
use axum::middleware;
use tower_http::set_header::SetResponseHeaderLayer;
use tracing::info;

use state::SharedState;

/// Header keamanan (OWASP). CSP membatasi semua sumber ke 'self'.
fn security_headers(router: Router) -> Router {
    let csp = "default-src 'self'; frame-ancestors 'none'; base-uri 'self'; \
               form-action 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; \
               script-src 'self'; connect-src 'self'";
    router
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("content-security-policy"),
            HeaderValue::from_static(csp),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("x-content-type-options"),
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("x-frame-options"),
            HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("referrer-policy"),
            HeaderValue::from_static("no-referrer"),
        ))
}

pub fn router(app: SharedState) -> Router {
    // Endpoint yang butuh sesi valid (+ CSRF untuk mutasi).
    let protected = Router::new()
        .route("/csrf", get(auth::csrf))
        .route("/logout", post(auth::logout))
        .route("/config", get(api::get_config).put(api::put_config))
        .route(
            "/scan",
            post(api::start_scan)
                .delete(api::stop_scan)
                .get(api::scan_status),
        )
        .route("/scans", get(api::list_scans))
        .route("/scans/:id", get(api::scan_detail))
        .route("/scans/:id/graph.svg", get(api::scan_graph_svg))
        .route("/scans/:id/graph.json", get(api::scan_graph_json))
        .route("/events", get(api::events))
        .layer(middleware::from_fn_with_state(
            app.clone(),
            auth::require_auth,
        ));

    let api = Router::new()
        .route("/login", post(auth::login))
        .merge(protected);

    let router = Router::new()
        .nest("/api", api)
        .fallback(assets::static_handler)
        .with_state(app);

    security_headers(router)
}

pub async fn serve(app: SharedState) -> Result<()> {
    let bind = app.server.bind.clone();
    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .with_context(|| format!("gagal bind ke {bind}"))?;
    info!("dashboard aktif di http://{bind}");
    axum::serve(
        listener,
        router(app).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .context("server berhenti dengan error")?;
    Ok(())
}
