//! Ekstraksi link/form/script dari HTML (fungsi murni — mudah diuji).

use scraper::{Html, Selector};
use url::Url;

/// Hasil parsing satu halaman HTML.
#[derive(Debug, Default)]
pub struct CrawlExtract {
    /// URL absolut hasil resolusi dari <a href>, dipakai untuk crawl lanjutan.
    pub links: Vec<Url>,
    /// URL absolut file JavaScript (<script src>).
    pub scripts: Vec<Url>,
    /// Action URL dari <form> (kandidat endpoint).
    pub forms: Vec<Url>,
    /// Isi <script> inline (tanpa src) untuk dianalisis seperti file JS.
    pub inline_scripts: Vec<String>,
    /// <title> halaman, bila ada.
    pub title: Option<String>,
}

/// Parse HTML relatif terhadap `base`, kembalikan link/script/form absolut.
pub fn extract(base: &Url, html: &str) -> CrawlExtract {
    let doc = Html::parse_document(html);
    let mut out = CrawlExtract::default();

    // Selector di-unwrap karena literalnya statis dan pasti valid.
    let a = Selector::parse("a[href]").unwrap();
    let script = Selector::parse("script[src]").unwrap();
    let inline = Selector::parse("script:not([src])").unwrap();
    let form = Selector::parse("form[action]").unwrap();
    let title = Selector::parse("title").unwrap();

    for el in doc.select(&a) {
        if let Some(href) = el.value().attr("href") {
            if let Some(u) = resolve(base, href) {
                out.links.push(u);
            }
        }
    }
    for el in doc.select(&script) {
        if let Some(src) = el.value().attr("src") {
            if let Some(u) = resolve(base, src) {
                out.scripts.push(u);
            }
        }
    }
    for el in doc.select(&form) {
        if let Some(action) = el.value().attr("action") {
            if let Some(u) = resolve(base, action) {
                out.forms.push(u);
            }
        }
    }
    for el in doc.select(&inline) {
        let code = el.text().collect::<String>();
        if code.trim().len() > 1 {
            out.inline_scripts.push(code);
        }
    }
    if let Some(t) = doc.select(&title).next() {
        let text = t.text().collect::<String>().trim().to_string();
        if !text.is_empty() {
            out.title = Some(text);
        }
    }
    out
}

/// Resolusi href relatif/absolut menjadi URL absolut http(s). Abaikan skema lain.
fn resolve(base: &Url, href: &str) -> Option<Url> {
    let href = href.trim();
    if href.is_empty() || href.starts_with('#') || href.starts_with("javascript:") || href.starts_with("mailto:") {
        return None;
    }
    let joined = base.join(href).ok()?;
    match joined.scheme() {
        "http" | "https" => Some(joined),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_links_scripts_forms() {
        let base = Url::parse("https://example.com/dir/page.html").unwrap();
        let html = r#"
            <html><head><title> Hello </title>
            <script src="/js/app.js"></script></head>
            <body>
              <a href="about.html">about</a>
              <a href="https://example.com/contact">contact</a>
              <a href="javascript:void(0)">x</a>
              <form action="/submit"></form>
            </body></html>
        "#;
        let r = extract(&base, html);
        assert_eq!(r.title.as_deref(), Some("Hello"));
        assert!(r.scripts.iter().any(|u| u.path() == "/js/app.js"));
        assert!(r.links.iter().any(|u| u.path() == "/dir/about.html"));
        assert!(r.links.iter().any(|u| u.path() == "/contact"));
        assert!(r.forms.iter().any(|u| u.path() == "/submit"));
        // javascript: diabaikan
        assert_eq!(r.links.len(), 2);
    }
}
