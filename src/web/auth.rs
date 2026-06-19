//! Autentikasi: Argon2id, sesi cookie, CSRF, dan rate-limit login.

use std::net::SocketAddr;

use argon2::password_hash::{rand_core::OsRng, SaltString};
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use serde::Deserialize;
use serde_json::json;
use tracing::{info, warn};

use super::state::SharedState;

const SID: &str = "sid";

/// Hash password jadi Argon2id PHC string (dipakai subcommand `hash-password`).
pub fn hash_password(password: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("hash gagal: {e}"))?;
    Ok(hash.to_string())
}

/// Verifikasi password terhadap PHC hash (constant-time di dalam argon2).
fn verify_password(password: &str, phc: &str) -> bool {
    match PasswordHash::new(phc) {
        Ok(parsed) => Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

#[derive(Deserialize)]
pub struct LoginReq {
    username: String,
    password: String,
}

fn session_cookie(token: String) -> Cookie<'static> {
    Cookie::build((SID, token))
        .http_only(true)
        .same_site(SameSite::Strict)
        .path("/")
        // `Secure` sengaja tidak diset: default bind localhost-HTTP. Di balik TLS,
        // proxy harus menambahkan flag Secure.
        .build()
}

pub async fn login(
    State(app): State<SharedState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    jar: CookieJar,
    Json(req): Json<LoginReq>,
) -> Response {
    let ip = addr.ip();
    if let Err(secs) = app.login_guard.check(ip) {
        warn!(%ip, "login ditolak: terkunci");
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({ "error": format!("terlalu banyak percobaan; coba lagi dalam {secs}s") })),
        )
            .into_response();
    }

    let ok = req.username == app.server.admin_user
        && verify_password(&req.password, &app.server.admin_password_hash);
    if !ok {
        app.login_guard.record_fail(ip);
        warn!(%ip, user = %req.username, "login gagal");
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "username atau password salah" })),
        )
            .into_response();
    }

    app.login_guard.reset(ip);
    let (token, csrf) = app.sessions.create();
    info!(%ip, user = %req.username, "login sukses");
    (jar.add(session_cookie(token)), Json(json!({ "csrf": csrf }))).into_response()
}

pub async fn logout(State(app): State<SharedState>, jar: CookieJar) -> Response {
    if let Some(c) = jar.get(SID) {
        app.sessions.remove(c.value());
    }
    (jar.remove(Cookie::from(SID)), StatusCode::NO_CONTENT).into_response()
}

pub async fn csrf(State(app): State<SharedState>, jar: CookieJar) -> Response {
    match jar.get(SID).and_then(|c| app.sessions.csrf_for(c.value())) {
        Some(csrf) => Json(json!({ "csrf": csrf })).into_response(),
        None => StatusCode::UNAUTHORIZED.into_response(),
    }
}

/// Middleware: wajib sesi valid; untuk method mutasi wajib `X-CSRF-Token` cocok.
pub async fn require_auth(
    State(app): State<SharedState>,
    jar: CookieJar,
    req: Request,
    next: Next,
) -> Response {
    let csrf = jar
        .get(SID)
        .and_then(|c| app.sessions.csrf_for(c.value()));
    let Some(csrf) = csrf else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "tidak terautentikasi" })),
        )
            .into_response();
    };

    let mutating = matches!(
        *req.method(),
        Method::POST | Method::PUT | Method::DELETE | Method::PATCH
    );
    if mutating {
        let header = req
            .headers()
            .get("x-csrf-token")
            .and_then(|v| v.to_str().ok());
        if header != Some(csrf.as_str()) {
            return (
                StatusCode::FORBIDDEN,
                Json(json!({ "error": "CSRF token tidak valid" })),
            )
                .into_response();
        }
    }

    next.run(req).await
}
