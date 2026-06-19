//! Menyajikan aset frontend (React build) yang di-embed ke binary.

use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "frontend/dist"]
struct Assets;

/// Sajikan file statis; fallback ke `index.html` untuk client-side routing (SPA).
pub async fn static_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    if let Some(content) = Assets::get(path) {
        let mime = mime_guess::from_path(path).first_or_octet_stream();
        return ([(header::CONTENT_TYPE, mime.to_string())], content.data.to_vec()).into_response();
    }

    // Rute SPA (mis. /login, /config) → kirim index.html.
    match Assets::get("index.html") {
        Some(content) => (
            [(header::CONTENT_TYPE, "text/html; charset=utf-8".to_string())],
            content.data.to_vec(),
        )
            .into_response(),
        None => (StatusCode::NOT_FOUND, "frontend belum di-build").into_response(),
    }
}
