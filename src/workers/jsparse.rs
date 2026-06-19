//! Ekstraksi kandidat endpoint/route/path dari file JavaScript.
//!
//! Metode utama: **parsing AST** dengan `swc` — mengumpulkan string literal,
//! quasi template, dan argumen pemanggilan jaringan (`fetch`, `axios.get`, …).
//! Dibanding regex, AST tidak salah menangkap string di dalam komentar dan
//! memahami struktur kode. Bila parsing gagal (mis. JS rusak/ter-obfuscate berat),
//! kita jatuh ke ekstraksi regex sebagai cadangan.

use std::collections::HashSet;

use once_cell::sync::Lazy;
use regex::Regex;
use swc_core::common::sync::Lrc;
use swc_core::common::{FileName, SourceMap};
use swc_core::ecma::ast::*;
use swc_core::ecma::parser::{lexer::Lexer, Parser, StringInput, Syntax};
use swc_core::ecma::visit::{Visit, VisitWith};

/// Ekstrak kandidat string unik dari isi file JS, dibatasi `max` entri.
/// Hasil dengan prioritas: argumen panggilan jaringan dulu, lalu string lain.
pub fn extract(js: &str, max: usize) -> Vec<String> {
    match parse_ast(js) {
        Some(collector) => finalize(collector.calls, collector.strings, max),
        None => regex_fallback(js, max),
    }
}

/// Parse JS menjadi AST dan kumpulkan kandidat. `None` bila parsing gagal.
fn parse_ast(js: &str) -> Option<Collector> {
    let cm: Lrc<SourceMap> = Default::default();
    let fm = cm.new_source_file(FileName::Anon.into(), js.to_string());
    let lexer = Lexer::new(
        Syntax::Es(Default::default()),
        EsVersion::latest(),
        StringInput::from(&*fm),
        None,
    );
    let mut parser = Parser::new_from(lexer);
    let program = parser.parse_program().ok()?;
    let mut collector = Collector::default();
    program.visit_with(&mut collector);
    Some(collector)
}

#[derive(Default)]
struct Collector {
    /// Argumen string ke pemanggilan jaringan (prioritas tinggi).
    calls: Vec<String>,
    /// Semua string literal & quasi template lain.
    strings: Vec<String>,
}

impl Visit for Collector {
    fn visit_str(&mut self, n: &Str) {
        self.strings.push(str_value(n));
    }

    fn visit_tpl(&mut self, n: &Tpl) {
        for q in &n.quasis {
            let s = quasi_value(q);
            if !s.is_empty() {
                self.strings.push(s);
            }
        }
        n.visit_children_with(self);
    }

    fn visit_call_expr(&mut self, n: &CallExpr) {
        if let Callee::Expr(callee) = &n.callee {
            if is_network_callee(callee) {
                for arg in &n.args {
                    match &*arg.expr {
                        Expr::Lit(Lit::Str(s)) => self.calls.push(str_value(s)),
                        Expr::Tpl(t) => {
                            if let Some(q) = t.quasis.first() {
                                let s = quasi_value(q);
                                if !s.is_empty() {
                                    self.calls.push(s);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        n.visit_children_with(self);
    }
}

/// Nilai string literal (WTF-8 → String, lossy untuk surrogate tak berpasangan).
fn str_value(s: &Str) -> String {
    s.value.as_wtf8().to_string_lossy().into_owned()
}

/// Nilai sebuah quasi template (pakai `cooked` bila ada, jika tidak `raw`).
fn quasi_value(q: &TplElement) -> String {
    match &q.cooked {
        Some(c) => c.as_wtf8().to_string_lossy().into_owned(),
        None => q.raw.to_string(),
    }
}

/// Apakah callee menunjukkan permintaan jaringan (`fetch`, `axios.get`, `$.ajax`, …)?
fn is_network_callee(callee: &Expr) -> bool {
    match callee {
        // fetch("...")
        Expr::Ident(i) => i.sym.as_ref() == "fetch",
        // axios.get("..."), http.post("..."), $.ajax("..."), window.fetch("...")
        Expr::Member(m) => matches!(
            &*m.obj,
            Expr::Ident(i) if is_network_object(i.sym.as_ref())
        ),
        _ => false,
    }
}

fn is_network_object(name: &str) -> bool {
    matches!(
        name,
        "axios" | "http" | "https" | "api" | "client" | "$" | "window" | "self" | "xhr"
    )
}

/// Susun hasil akhir: calls dulu, lalu strings; filter + dedup + cap.
fn finalize(calls: Vec<String>, strings: Vec<String>, max: usize) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for s in calls.into_iter().chain(strings.into_iter()) {
        let s = s.trim().to_string();
        if is_interesting(&s) && seen.insert(s.clone()) {
            out.push(s);
            if out.len() >= max {
                break;
            }
        }
    }
    out
}

/// Saring kandidat yang jelas bukan endpoint (aset statis, library path, dll).
fn is_interesting(s: &str) -> bool {
    if s.len() < 2 {
        return false;
    }
    // Hanya path/URL yang menarik (mulai "/" atau skema http).
    if !(s.starts_with('/') || s.starts_with("http://") || s.starts_with("https://")) {
        return false;
    }
    let lower = s.to_lowercase();
    const BORING_EXT: [&str; 12] = [
        ".png", ".jpg", ".jpeg", ".gif", ".svg", ".css", ".woff", ".woff2", ".ttf", ".ico",
        ".map", ".webp",
    ];
    if BORING_EXT.iter().any(|e| lower.ends_with(e)) {
        return false;
    }
    if lower.contains("node_modules") {
        return false;
    }
    true
}

// ---- Fallback regex (dipakai hanya bila parsing AST gagal) ----

static PATTERNS: Lazy<Vec<Regex>> = Lazy::new(|| {
    vec![
        Regex::new(r#"["'`](/[A-Za-z0-9_\-./]{1,200})["'`]"#).unwrap(),
        Regex::new(r#"["'`](https?://[A-Za-z0-9_\-./:?=&%]{3,300})["'`]"#).unwrap(),
        Regex::new(r#"(?:fetch|axios(?:\.\w+)?)\(\s*["'`]([^"'`]{1,300})["'`]"#).unwrap(),
    ]
});

fn regex_fallback(js: &str, max: usize) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for re in PATTERNS.iter() {
        for cap in re.captures_iter(js) {
            if let Some(m) = cap.get(1) {
                let s = m.as_str().trim();
                if is_interesting(s) && seen.insert(s.to_string()) {
                    out.push(s.to_string());
                    if out.len() >= max {
                        return out;
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_api_paths_and_urls_via_ast() {
        let js = r#"
            const base = "/api/v1/users";
            fetch("/api/v1/internal/admin_reset");
            axios.get("https://api.target.io/secret");
            const logo = "/assets/logo.png";
            import x from "/node_modules/lib/index.js";
        "#;
        let found = extract(js, 50);
        assert!(found.iter().any(|s| s == "/api/v1/users"));
        assert!(found.iter().any(|s| s == "/api/v1/internal/admin_reset"));
        assert!(found.iter().any(|s| s.contains("api.target.io/secret")));
        // aset statis & node_modules disaring
        assert!(!found.iter().any(|s| s.ends_with("logo.png")));
        assert!(!found.iter().any(|s| s.contains("node_modules")));
    }

    #[test]
    fn ignores_comments_and_reads_templates() {
        // Keunggulan AST: URL di dalam komentar TIDAK ditangkap (regex akan salah tangkap).
        let js = r#"
            // legacy endpoint /api/old_deprecated must not be picked
            const u = "/api/new";
            const id = 1;
            fetch(`/api/tpl/${id}`);
        "#;
        let found = extract(js, 50);
        assert!(found.iter().any(|s| s == "/api/new"));
        assert!(found.iter().any(|s| s.starts_with("/api/tpl/")));
        assert!(!found.iter().any(|s| s.contains("old_deprecated")));
    }

    #[test]
    fn prioritizes_network_calls_first() {
        let js = r#"const z = "/zzz/last"; fetch("/api/first");"#;
        let found = extract(js, 50);
        // argumen fetch harus muncul sebelum string biasa
        assert_eq!(found.first().map(|s| s.as_str()), Some("/api/first"));
    }

    #[test]
    fn respects_max() {
        let js = r#"const a = ["/a/1", "/a/2", "/a/3", "/a/4"];"#;
        assert_eq!(extract(js, 2).len(), 2);
    }

    #[test]
    fn regex_fallback_on_unparseable() {
        // JS sengaja rusak → parse gagal → fallback regex tetap menemukan path.
        let js = r#"function( { "/api/broken" fetch("/api/frag" "#;
        let found = regex_fallback(js, 10);
        assert!(found.iter().any(|s| s == "/api/broken"));
    }
}
