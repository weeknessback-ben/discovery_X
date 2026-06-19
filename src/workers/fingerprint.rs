//! Fingerprint teknologi/framework (+versi) dari respons HTTP, lalu pemetaan
//! ke daftar path khas framework untuk "brute force halus" yang terarah.

use once_cell::sync::Lazy;
use regex::Regex;
use reqwest::header::HeaderMap;

static GENERATOR_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?i)<meta[^>]+name=["']generator["'][^>]+content=["']([^"']+)["']"#).unwrap()
});

/// Teknologi terdeteksi.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tech {
    pub name: String,
    pub version: Option<String>,
}

impl Tech {
    fn new(name: &str, version: Option<String>) -> Self {
        Tech {
            name: name.to_string(),
            version,
        }
    }

    /// Label ringkas, mis. "WordPress 6.2" atau "Laravel".
    pub fn label(&self) -> String {
        match &self.version {
            Some(v) => format!("{} {}", self.name, v),
            None => self.name.clone(),
        }
    }
}

/// Deteksi teknologi dari header + body HTML.
pub fn detect(headers: &HeaderMap, body: &str) -> Vec<Tech> {
    let mut techs: Vec<Tech> = Vec::new();
    let mut push = |name: &str, ver: Option<String>| {
        if !techs.iter().any(|t| t.name == name) {
            techs.push(Tech::new(name, ver));
        }
    };

    let h = |k: &str| -> String {
        headers
            .get(k)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string()
    };
    let server = h("server");
    let powered = h("x-powered-by");
    let generator = GENERATOR_RE
        .captures(body)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
        .unwrap_or_default();
    let cookies: String = headers
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .collect::<Vec<_>>()
        .join("; ")
        .to_lowercase();
    let b = body.to_lowercase();
    let gen_l = generator.to_lowercase();

    // CMS / framework via body & generator
    if b.contains("/wp-content/") || b.contains("/wp-includes/") || gen_l.starts_with("wordpress") {
        push("WordPress", version_after(&generator, "WordPress"));
    }
    if gen_l.starts_with("drupal") || b.contains("/sites/default/files") {
        push("Drupal", version_after(&generator, "Drupal"));
    }
    if gen_l.starts_with("joomla") || b.contains("/media/jui/") {
        push("Joomla", version_after(&generator, "Joomla"));
    }
    // SPA / JS framework
    if powered.contains("Next.js") || b.contains("/_next/") || b.contains("__next_data__") {
        push("Next.js", None);
    }
    if b.contains("/_nuxt/") || b.contains("__nuxt__") {
        push("Nuxt", None);
    }
    if b.contains("data-reactroot") || b.contains("react-dom") {
        push("React", None);
    }
    if b.contains("ng-version") {
        push("Angular", None);
    }
    // Backend via cookies / powered
    if cookies.contains("laravel_session") || cookies.contains("xsrf-token") {
        push("Laravel", None);
    }
    if cookies.contains("csrftoken") || cookies.contains("django") {
        push("Django", None);
    }
    if powered.eq_ignore_ascii_case("Express") {
        push("Express", None);
    }
    if !powered.is_empty() && powered.to_lowercase().contains("php") {
        push("PHP", version_after(&powered, "PHP"));
    }
    // Web server
    if !server.is_empty() {
        let (name, ver) = split_server(&server);
        push(&name, ver);
    }

    techs
}

/// Path khas untuk teknologi tertentu (untuk dirbust terarah).
pub fn paths_for(techs: &[Tech]) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for t in techs {
        let extra: &[&str] = match t.name.as_str() {
            "WordPress" => &[
                "/wp-login.php",
                "/wp-admin/",
                "/wp-json/",
                "/wp-json/wp/v2/users",
                "/xmlrpc.php",
                "/wp-content/debug.log",
                "/wp-config.php.bak",
            ],
            "Drupal" => &["/user/login", "/CHANGELOG.txt", "/core/CHANGELOG.txt", "/admin"],
            "Joomla" => &["/administrator/", "/configuration.php-bak"],
            "Next.js" => &["/_next/static/", "/api/", "/_next/data/"],
            "Nuxt" => &["/_nuxt/", "/api/"],
            "Laravel" => &[
                "/telescope",
                "/horizon",
                "/.env",
                "/storage/logs/laravel.log",
                "/api",
            ],
            "Django" => &["/admin/", "/api/", "/static/admin/"],
            "Express" => &["/api", "/status", "/health", "/healthz"],
            _ => &[],
        };
        out.extend_from_slice(extra);
    }
    out
}

/// Path umum yang layak dicoba pada hampir semua target.
pub fn generic_paths() -> &'static [&'static str] {
    &[
        "/api",
        "/admin",
        "/login",
        "/.git/HEAD",
        "/.env",
        "/robots.txt",
        "/sitemap.xml",
        "/status",
        "/health",
        "/metrics",
        "/swagger.json",
        "/openapi.json",
        "/swagger-ui/",
        "/.well-known/security.txt",
        "/server-status",
        "/actuator",
        "/graphql",
    ]
}

/// Ambil versi setelah nama produk, mis. "WordPress 6.2" -> Some("6.2").
fn version_after(s: &str, product: &str) -> Option<String> {
    let lower = s.to_lowercase();
    let idx = lower.find(&product.to_lowercase())?;
    let rest = s[idx + product.len()..].trim_start_matches([' ', '/', ':']);
    let ver: String = rest
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    if ver.is_empty() {
        None
    } else {
        Some(ver)
    }
}

/// Pisahkan header Server "nginx/1.18.0" -> ("nginx", Some("1.18.0")).
fn split_server(server: &str) -> (String, Option<String>) {
    match server.split_once('/') {
        Some((name, rest)) => {
            let ver: String = rest
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect();
            (
                name.trim().to_string(),
                if ver.is_empty() { None } else { Some(ver) },
            )
        }
        None => (server.trim().to_string(), None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::HeaderMap;

    #[test]
    fn detects_wordpress_with_version() {
        let mut h = HeaderMap::new();
        h.insert("server", "Apache/2.4.41".parse().unwrap());
        let body = r#"<meta name="generator" content="WordPress 6.2.1"/><link href="/wp-content/themes/x"/>"#;
        let techs = detect(&h, body);
        let wp = techs.iter().find(|t| t.name == "WordPress").unwrap();
        assert_eq!(wp.version.as_deref(), Some("6.2.1"));
        assert!(techs.iter().any(|t| t.name == "Apache"));
        // path khas WordPress muncul
        assert!(paths_for(&techs).contains(&"/wp-json/"));
    }

    #[test]
    fn detects_next_js() {
        let mut h = HeaderMap::new();
        h.insert("x-powered-by", "Next.js".parse().unwrap());
        let techs = detect(&h, "<div id='__next'></div>");
        assert!(techs.iter().any(|t| t.name == "Next.js"));
        assert!(paths_for(&techs).contains(&"/_next/static/"));
    }
}
