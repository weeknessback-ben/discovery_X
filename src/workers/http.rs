//! HTTP worker: ambil URL, fingerprint, lalu parse HTML/JS + feeds + dirbust.

use std::collections::HashSet;

use anyhow::Result;
use reqwest::Client;
use tracing::{debug, warn};
use url::Url;

use super::crawl::CrawlExtract;
use super::{crawl, feeds, fingerprint, jsparse};
use crate::render::Renderer;
use crate::scope::Scope;
use crate::task::{DiscoveryResult, FetchOrigin, Task};
use crate::types::{AiInput, Asset, AssetKind, EdgeKind, GraphEdge};

/// Batas jumlah kandidat string JS per file/halaman.
const MAX_JS_CANDIDATES: usize = 80;

/// Opsi perilaku worker (diturunkan dari config).
#[derive(Clone, Copy)]
pub struct FetchOpts {
    pub max_depth: usize,
    pub enable_dirbust: bool,
    pub max_js_probes: usize,
    pub max_dirbust: usize,
    /// Verifikasi liveness endpoint via headless browser.
    pub verify_live: bool,
}

/// Eksekusi sebuah `Task::Fetch`.
pub async fn fetch(
    client: &Client,
    scope: &Scope,
    url: Url,
    origin: FetchOrigin,
    depth: usize,
    opts: FetchOpts,
    renderer: Option<&Renderer>,
) -> Result<DiscoveryResult> {
    let mut result = DiscoveryResult::default();

    // Guardrail ganda: jangan pernah menyentuh host di luar scope.
    if !scope.url_in_scope(&url) {
        warn!(%url, "fetch ditolak: di luar scope");
        return Ok(result);
    }

    let resp = match client.get(url.clone()).send().await {
        Ok(r) => r,
        Err(e) => {
            debug!(%url, error = %e, "request gagal");
            return Ok(result);
        }
    };

    let status = resp.status();
    let headers = resp.headers().clone();
    let content_type = headers
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_lowercase();
    let server = headers
        .get(reqwest::header::SERVER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let body = resp.text().await.unwrap_or_default();

    // Abaikan 404/410 — kurangi noise (penting untuk dirbust).
    if matches!(status.as_u16(), 404 | 410) {
        debug!(%url, status = status.as_u16(), "dilewati (not found)");
        return Ok(result);
    }

    let base_origin = origin_of(&url);
    let path = url.path();

    // --- robots.txt ---
    if path == "/robots.txt" {
        if let Ok(root) = Url::parse(&base_origin) {
            let robots = feeds::parse_robots(&body, &root);
            for p in robots.paths {
                push_fetch(&mut result, scope, p, FetchOrigin::Feed);
            }
            for sm in robots.sitemaps {
                push_fetch(&mut result, scope, sm, FetchOrigin::Feed);
            }
        }
        result
            .assets
            .insert(0, Asset::new(AssetKind::Page, url.as_str(), origin.as_str()));
        return Ok(result);
    }

    // --- sitemap.xml ---
    if path.ends_with(".xml") && (body.contains("<loc") || body.contains("<urlset") || body.contains("<sitemapindex")) {
        for u in feeds::parse_sitemap(&body) {
            push_fetch(&mut result, scope, u, FetchOrigin::Feed);
        }
        result
            .assets
            .insert(0, Asset::new(AssetKind::Page, url.as_str(), origin.as_str()));
        return Ok(result);
    }

    let is_js = content_type.contains("javascript")
        || content_type.contains("ecmascript")
        || path.ends_with(".js");
    let is_html = content_type.contains("text/html") || (!is_js && body.contains("<html"));

    let kind = if is_js { AssetKind::JsFile } else { AssetKind::Page };
    let mut asset =
        Asset::new(kind, url.as_str(), origin.as_str()).with_note(format!("status={}", status.as_u16()));
    if !server.is_empty() {
        asset = asset.with_note(format!("server={server}"));
    }

    // Subdomain → halaman yang disajikannya.
    if origin == FetchOrigin::Subdomain {
        if let Some(host) = url.host_str() {
            result
                .edges
                .push(GraphEdge::new(host, url.as_str(), EdgeKind::Hosts));
        }
    }

    if is_html {
        let extract = crawl::extract(&url, &body);
        if let Some(title) = &extract.title {
            asset = asset.with_note(format!("title={title}"));
        }
        harvest_html(&mut result, scope, url.as_str(), &base_origin, &extract, depth, opts);

        // Fingerprint + dirbust terarah, hanya pada seed (root run).
        if origin == FetchOrigin::Seed && opts.enable_dirbust {
            run_fingerprint(&mut result, scope, &url, &base_origin, &headers, &body, opts);
        }

        // Render SPA: bila halaman tampak butuh JS, render & panen DOM hasilnya.
        if let Some(r) = renderer {
            if looks_like_spa(&body, &extract) {
                if let Some(rendered) = r.render(url.as_str()).await {
                    let rex = crawl::extract(&url, &rendered);
                    harvest_html(&mut result, scope, url.as_str(), &base_origin, &rex, depth, opts);
                    asset = asset.with_note("rendered=spa");
                }
            }
        }
    } else if is_js {
        let candidates = jsparse::extract(&body, MAX_JS_CANDIDATES);
        debug!(%url, n = candidates.len(), "kandidat JS diekstrak");
        push_js_candidates(&mut result, scope, url.as_str(), &base_origin, candidates, opts);
    }

    // Verifikasi liveness endpoint hasil temuan via headless (deteksi soft-404 SPA).
    if opts.verify_live
        && matches!(
            origin,
            FetchOrigin::JsEndpoint | FetchOrigin::Dirbust | FetchOrigin::AiProbe
        )
    {
        if let Some(r) = renderer {
            if let Some((alive, title)) = verify_liveness(r, &url).await {
                asset = asset.with_note(format!("live={}", if alive { "yes" } else { "no" }));
                if let Some(t) = title {
                    if !t.is_empty() {
                        asset = asset.with_note(format!("rendered_title={t}"));
                    }
                }
            }
        }
    }

    result.assets.insert(0, asset);
    Ok(result)
}

/// Render endpoint di headless browser dan tentukan apakah "hidup" (bukan soft-404).
/// Mengembalikan `(alive, title)`; `None` bila render gagal.
async fn verify_liveness(renderer: &Renderer, url: &Url) -> Option<(bool, Option<String>)> {
    let html = renderer.render(url.as_str()).await?;
    let extract = crawl::extract(url, &html);
    let title = extract.title.clone();
    let title_l = title.as_deref().unwrap_or("").to_lowercase();
    let body_l = html.to_lowercase();
    const DEAD: [&str; 6] = [
        "404",
        "not found",
        "page not found",
        "tidak ditemukan",
        "does not exist",
        "halaman tidak",
    ];
    // Konservatif: hanya tandai mati bila judul, atau body kecil, memuat sinyal error.
    let looks_dead = DEAD.iter().any(|m| title_l.contains(m))
        || (body_l.len() < 3000 && DEAD.iter().any(|m| body_l.contains(m)));
    Some((!looks_dead, title))
}

/// Panen tautan/script/form/inline-JS dari sebuah HTML (statis maupun hasil render).
fn harvest_html(
    result: &mut DiscoveryResult,
    scope: &Scope,
    page_url: &str,
    base_origin: &str,
    extract: &CrawlExtract,
    depth: usize,
    opts: FetchOpts,
) {
    for form in &extract.forms {
        result
            .assets
            .push(Asset::new(AssetKind::Form, form.as_str(), "crawl"));
        result
            .edges
            .push(GraphEdge::new(page_url, form.as_str(), EdgeKind::Contains));
    }
    for link in &extract.links {
        result
            .edges
            .push(GraphEdge::new(page_url, link.as_str(), EdgeKind::Links));
    }
    for s in &extract.scripts {
        result
            .edges
            .push(GraphEdge::new(page_url, s.as_str(), EdgeKind::References));
    }
    if depth < opts.max_depth {
        for link in extract.links.iter().chain(extract.scripts.iter()) {
            if scope.url_in_scope(link) {
                result.follow_up.push(Task::Fetch {
                    url: link.clone(),
                    origin: FetchOrigin::Crawl,
                    depth: depth + 1,
                });
            }
        }
    }
    // Analisis <script> inline (endpoint sering tersembunyi di sini).
    let inline_js = extract.inline_scripts.join("\n");
    if !inline_js.trim().is_empty() {
        let candidates = jsparse::extract(&inline_js, MAX_JS_CANDIDATES);
        push_js_candidates(result, scope, page_url, base_origin, candidates, opts);
    }
}

/// Heuristik: halaman tampak SPA (perlu eksekusi JS) → layak dirender.
fn looks_like_spa(body: &str, extract: &CrawlExtract) -> bool {
    const MARKERS: [&str; 8] = [
        "id=\"root\"",
        "id=\"app\"",
        "__NEXT_DATA__",
        "__NUXT__",
        "__nuxt__",
        "ng-version",
        "data-reactroot",
        "window.__INITIAL_STATE__",
    ];
    let has_marker = MARKERS.iter().any(|m| body.contains(m));
    has_marker && extract.links.len() < 5
}

/// Ubah kandidat JS menjadi probe langsung (origin JsEndpoint) + edge Calls,
/// dan sediakan input untuk AI bila aktif. Bekerja dengan/atau tanpa AI.
fn push_js_candidates(
    result: &mut DiscoveryResult,
    scope: &Scope,
    source_url: &str,
    base_origin: &str,
    candidates: Vec<String>,
    opts: FetchOpts,
) {
    if candidates.is_empty() {
        return;
    }
    let mut n = 0;
    for cand in &candidates {
        if n >= opts.max_js_probes {
            break;
        }
        if let Some(u) = resolve_candidate(base_origin, cand) {
            if scope.url_in_scope(&u) {
                result
                    .edges
                    .push(GraphEdge::new(source_url, u.as_str(), EdgeKind::Calls));
                result.follow_up.push(Task::Fetch {
                    url: u,
                    origin: FetchOrigin::JsEndpoint,
                    depth: 0,
                });
                n += 1;
            }
        }
    }
    // Tetap sediakan kandidat untuk AI (penyaringan/penalaran tambahan).
    if result.ai_input.is_none() {
        result.ai_input = Some(AiInput {
            source_url: source_url.to_string(),
            base_origin: base_origin.to_string(),
            candidates,
            tech: Vec::new(), // diisi orchestrator dari fingerprint terkumpul
        });
    }
}

/// Deteksi teknologi lalu probe path khas framework + generic.
fn run_fingerprint(
    result: &mut DiscoveryResult,
    scope: &Scope,
    page: &Url,
    base_origin: &str,
    headers: &reqwest::header::HeaderMap,
    body: &str,
    opts: FetchOpts,
) {
    let techs = fingerprint::detect(headers, body);
    for t in &techs {
        let label = t.label();
        let tech_url = format!("{base_origin}#tech={}", label.replace(' ', "_"));
        result.assets.push(
            Asset::new(AssetKind::Tech, tech_url, "fingerprint").with_note(label),
        );
    }
    debug!(%page, techs = techs.len(), "fingerprint selesai");

    let mut paths: Vec<String> = fingerprint::paths_for(&techs)
        .iter()
        .map(|s| s.to_string())
        .collect();
    paths.extend(fingerprint::generic_paths().iter().map(|s| s.to_string()));

    let mut seen = HashSet::new();
    let mut n = 0;
    for p in paths {
        if n >= opts.max_dirbust {
            break;
        }
        if !seen.insert(p.clone()) {
            continue;
        }
        if let Some(u) = resolve_candidate(base_origin, &p) {
            if scope.url_in_scope(&u) {
                result
                    .edges
                    .push(GraphEdge::new(page.as_str(), u.as_str(), EdgeKind::Guessed));
                result.follow_up.push(Task::Fetch {
                    url: u,
                    origin: FetchOrigin::Dirbust,
                    depth: 0,
                });
                n += 1;
            }
        }
    }
}

/// Resolusi path/URL kandidat menjadi URL absolut http(s).
fn resolve_candidate(base_origin: &str, cand: &str) -> Option<Url> {
    let u = if cand.starts_with("http://") || cand.starts_with("https://") {
        Url::parse(cand).ok()?
    } else {
        Url::parse(base_origin).ok()?.join(cand).ok()?
    };
    matches!(u.scheme(), "http" | "https").then_some(u)
}

/// Tambah follow-up Fetch (scope-checked) dengan origin tertentu, depth 0.
fn push_fetch(result: &mut DiscoveryResult, scope: &Scope, url: Url, origin: FetchOrigin) {
    if scope.url_in_scope(&url) {
        result.follow_up.push(Task::Fetch {
            url,
            origin,
            depth: 0,
        });
    }
}

/// "scheme://host[:port]" dari sebuah URL, untuk me-resolve path relatif.
pub fn origin_of(url: &Url) -> String {
    let mut s = format!("{}://{}", url.scheme(), url.host_str().unwrap_or(""));
    if let Some(port) = url.port() {
        s.push_str(&format!(":{port}"));
    }
    s
}
