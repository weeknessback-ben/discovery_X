//! Handler JSON API: config, kontrol scan, riwayat, dan SSE event.

use std::convert::Infallible;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::header::CONTENT_TYPE;
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures::Stream;
use serde::Deserialize;
use serde_json::json;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::sync::broadcast::error::RecvError;
use url::Url;

use super::state::SharedState;
use crate::config::{AiConfig, Config, ReconConfig};
use crate::scope::Scope;

/// Config efektif = default/TOML di-overlay settings DB (recon + ai).
async fn effective_config(app: &SharedState) -> Config {
    let mut cfg = app.base_cfg.clone();
    if let Ok(Some(j)) = app.state.settings_get("recon").await {
        if let Ok(r) = serde_json::from_str::<ReconConfig>(&j) {
            cfg.recon = r;
        }
    }
    if let Ok(Some(j)) = app.state.settings_get("ai").await {
        if let Ok(a) = serde_json::from_str::<AiConfig>(&j) {
            cfg.ai = a;
        }
    }
    cfg
}

fn err500<E: std::fmt::Display>(e: E) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": e.to_string() })),
    )
        .into_response()
}

fn bad(msg: &str) -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({ "error": msg }))).into_response()
}

// ---- Config ----

pub async fn get_config(State(app): State<SharedState>) -> Response {
    let cfg = effective_config(&app).await;
    let key = cfg.ai.api_key.unwrap_or_default();
    let key_set = !key.trim().is_empty();
    let key_hint = if key_set {
        Some(format!("••••{}", &key[key.len().saturating_sub(4)..]))
    } else {
        None
    };
    Json(json!({
        "recon": cfg.recon,
        "ai": {
            "base_url": cfg.ai.base_url,
            "model": cfg.ai.model,
            "enabled": cfg.ai.enabled,
            "temperature": cfg.ai.temperature,
            "max_retries": cfg.ai.max_retries,
            "timeout_secs": cfg.ai.timeout_secs,
            "key_set": key_set,
            "key_hint": key_hint,
        }
    }))
    .into_response()
}

#[derive(Deserialize)]
pub struct ConfigUpdate {
    recon: ReconConfig,
    ai: AiUpdate,
}

#[derive(Deserialize)]
struct AiUpdate {
    base_url: String,
    model: String,
    enabled: bool,
    temperature: f32,
    max_retries: u32,
    timeout_secs: u64,
    /// Hanya diperbarui bila diisi; kosong/None → pertahankan key lama.
    api_key: Option<String>,
}

pub async fn put_config(State(app): State<SharedState>, Json(upd): Json<ConfigUpdate>) -> Response {
    let cur = effective_config(&app).await;
    let api_key = match upd.ai.api_key {
        Some(k) if !k.trim().is_empty() => Some(k),
        _ => cur.ai.api_key.clone(),
    };
    let ai = AiConfig {
        base_url: upd.ai.base_url,
        model: upd.ai.model,
        api_key,
        enabled: upd.ai.enabled,
        max_retries: upd.ai.max_retries,
        temperature: upd.ai.temperature,
        timeout_secs: upd.ai.timeout_secs,
    };
    let recon_json = match serde_json::to_string(&upd.recon) {
        Ok(s) => s,
        Err(e) => return err500(e),
    };
    let ai_json = match serde_json::to_string(&ai) {
        Ok(s) => s,
        Err(e) => return err500(e),
    };
    if let Err(e) = app.state.settings_set("recon", &recon_json).await {
        return err500(e);
    }
    if let Err(e) = app.state.settings_set("ai", &ai_json).await {
        return err500(e);
    }
    StatusCode::NO_CONTENT.into_response()
}

// ---- Kontrol scan ----

#[derive(Deserialize)]
pub struct StartReq {
    seed: String,
    scope: String,
    authorized: bool,
}

pub async fn start_scan(State(app): State<SharedState>, Json(req): Json<StartReq>) -> Response {
    if !req.authorized {
        return bad("Anda harus mencentang konfirmasi otorisasi");
    }
    let scope = Scope::parse(&req.scope);
    if scope.is_empty() {
        return bad("scope (allowlist) tidak boleh kosong");
    }
    let seed = match Url::parse(req.seed.trim()) {
        Ok(u) => u,
        Err(_) => return bad("seed URL tidak valid"),
    };
    if !scope.url_in_scope(&seed) {
        return bad("seed berada di luar scope — tambahkan host-nya ke allowlist");
    }
    let cfg = effective_config(&app).await;
    match app
        .scans
        .start(cfg, Arc::new(scope), seed, req.scope)
        .await
    {
        Ok(id) => Json(json!({ "scan_id": id })).into_response(),
        Err(e) => (
            StatusCode::CONFLICT,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

pub async fn stop_scan(State(app): State<SharedState>) -> StatusCode {
    app.scans.stop();
    StatusCode::NO_CONTENT
}

pub async fn scan_status(State(app): State<SharedState>) -> Response {
    Json(app.scans.status()).into_response()
}

// ---- Riwayat ----

pub async fn list_scans(State(app): State<SharedState>) -> Response {
    match app.state.list_scans().await {
        Ok(v) => Json(v).into_response(),
        Err(e) => err500(e),
    }
}

pub async fn scan_detail(State(app): State<SharedState>, Path(id): Path<i64>) -> Response {
    let scan = match app.state.get_scan(id).await {
        Ok(Some(s)) => s,
        Ok(None) => {
            return (StatusCode::NOT_FOUND, Json(json!({ "error": "scan tidak ditemukan" })))
                .into_response()
        }
        Err(e) => return err500(e),
    };
    let findings = match app.state.assets_by_scan(id).await {
        Ok(f) => f,
        Err(e) => return err500(e),
    };
    Json(json!({ "scan": scan, "findings": findings })).into_response()
}

// ---- Render attack graph (DOT → SVG via Graphviz) ----

pub async fn scan_graph_svg(State(app): State<SharedState>, Path(id): Path<i64>) -> Response {
    let dot = match app.state.get_scan(id).await {
        Ok(Some(s)) => match s.dot {
            Some(d) if !d.trim().is_empty() => d,
            _ => return (StatusCode::NOT_FOUND, "attack graph belum tersedia untuk scan ini")
                .into_response(),
        },
        Ok(None) => return (StatusCode::NOT_FOUND, "scan tidak ditemukan").into_response(),
        Err(e) => return err500(e),
    };

    // Render via `dot -Tsvg`. Tulis DOT ke stdin di task terpisah agar tidak deadlock
    // saat output besar mengisi buffer pipe.
    let mut child = match Command::new("dot")
        .arg("-Tsvg")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                "Graphviz 'dot' tidak terpasang di server (apt install graphviz)",
            )
                .into_response()
        }
    };

    if let Some(mut sin) = child.stdin.take() {
        tokio::spawn(async move {
            let _ = sin.write_all(dot.as_bytes()).await;
            // sin di-drop di sini → menutup stdin (EOF) agar dot mulai memproses.
        });
    }

    match tokio::time::timeout(Duration::from_secs(30), child.wait_with_output()).await {
        Ok(Ok(out)) if out.status.success() => (
            [(CONTENT_TYPE, "image/svg+xml; charset=utf-8")],
            out.stdout,
        )
            .into_response(),
        Ok(_) => (StatusCode::BAD_GATEWAY, "render graph gagal").into_response(),
        Err(_) => (StatusCode::GATEWAY_TIMEOUT, "render graph timeout").into_response(),
    }
}

/// Graf sebagai JSON {nodes, edges} untuk visualisasi D3 di dashboard.
pub async fn scan_graph_json(State(app): State<SharedState>, Path(id): Path<i64>) -> Response {
    match app.state.scan_graph_json(id).await {
        Ok(Some(j)) => ([(CONTENT_TYPE, "application/json")], j).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "graph belum tersedia untuk scan ini" })),
        )
            .into_response(),
        Err(e) => err500(e),
    }
}

// ---- SSE ----

pub async fn events(
    State(app): State<SharedState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = app.scans.subscribe();
    let stream = futures::stream::unfold(rx, |mut rx| async move {
        loop {
            match rx.recv().await {
                Ok(ev) => {
                    let data = serde_json::to_string(&ev).unwrap_or_default();
                    return Some((Ok(Event::default().data(data)), rx));
                }
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => return None,
            }
        }
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}
