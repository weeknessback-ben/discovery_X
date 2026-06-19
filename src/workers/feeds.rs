//! Parsing robots.txt dan sitemap.xml untuk sumber URL gratis.

use once_cell::sync::Lazy;
use regex::Regex;
use url::Url;

static LOC_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?s)<loc>\s*(.*?)\s*</loc>").unwrap());

/// Hasil parsing robots.txt.
#[derive(Debug, Default)]
pub struct Robots {
    /// Path dari Disallow/Allow (di-resolve ke URL absolut).
    pub paths: Vec<Url>,
    /// URL sitemap yang dideklarasikan.
    pub sitemaps: Vec<Url>,
}

/// Parse robots.txt relatif terhadap `base` (origin situs).
pub fn parse_robots(body: &str, base: &Url) -> Robots {
    let mut out = Robots::default();
    for raw in body.lines() {
        let line = raw.trim();
        let (key, val) = match line.split_once(':') {
            Some((k, v)) => (k.trim().to_lowercase(), v.trim()),
            None => continue,
        };
        match key.as_str() {
            "disallow" | "allow" => {
                // Buang wildcard/query agar bisa di-join sebagai path nyata.
                let p = val.split(['*', '?', '$']).next().unwrap_or("").trim();
                if p.is_empty() || p == "/" {
                    continue;
                }
                if let Ok(u) = base.join(p) {
                    if is_http(&u) {
                        out.paths.push(u);
                    }
                }
            }
            "sitemap" => {
                if let Ok(u) = Url::parse(val) {
                    if is_http(&u) {
                        out.sitemaps.push(u);
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Ekstrak URL `<loc>` dari sitemap.xml (mencakup sitemap index & urlset).
pub fn parse_sitemap(body: &str) -> Vec<Url> {
    let mut out = Vec::new();
    for cap in LOC_RE.captures_iter(body) {
        if let Some(m) = cap.get(1) {
            if let Ok(u) = Url::parse(m.as_str().trim()) {
                if is_http(&u) {
                    out.push(u);
                }
            }
        }
    }
    out
}

fn is_http(u: &Url) -> bool {
    matches!(u.scheme(), "http" | "https")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_robots() {
        let base = Url::parse("https://t.com/").unwrap();
        let body = "User-agent: *\nDisallow: /admin/\nDisallow: /private/*\nAllow: /public\nSitemap: https://t.com/sitemap.xml\n";
        let r = parse_robots(body, &base);
        assert!(r.paths.iter().any(|u| u.path() == "/admin/"));
        assert!(r.paths.iter().any(|u| u.path() == "/private/")); // wildcard dibuang
        assert!(r.paths.iter().any(|u| u.path() == "/public"));
        assert_eq!(r.sitemaps.len(), 1);
    }

    #[test]
    fn parses_sitemap_locs() {
        let xml = "<urlset><url><loc>https://t.com/a</loc></url><url><loc>https://t.com/b</loc></url></urlset>";
        let urls = parse_sitemap(xml);
        assert_eq!(urls.len(), 2);
        assert!(urls.iter().any(|u| u.path() == "/a"));
    }
}
