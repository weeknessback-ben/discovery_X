//! Allowlist scope — guardrail wajib. Tool menolak menyentuh host di luar daftar ini.

use std::collections::HashSet;
use std::path::Path;

use anyhow::{bail, Context, Result};
use url::Url;

/// Daftar host/IP yang diizinkan untuk dipindai.
///
/// Format file (satu entri per baris, `#` untuk komentar):
///   example.com          -> cocok persis "example.com"
///   *.example.com        -> cocok subdomain apa pun dari example.com (termasuk apex)
///   10.0.0.5             -> cocok IP persis
#[derive(Debug, Clone, Default)]
pub struct Scope {
    exact: HashSet<String>,
    wildcards: Vec<String>, // suffix domain, mis. "example.com" dari "*.example.com"
}

impl Scope {
    #[allow(dead_code)] // dipertahankan untuk pemakaian CLI/util mendatang
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("gagal membaca file scope {}", path.display()))?;
        let scope = Self::parse(&text);
        if scope.is_empty() {
            bail!(
                "file scope {} kosong — tool menolak berjalan tanpa target yang diizinkan",
                path.display()
            );
        }
        Ok(scope)
    }

    pub fn parse(text: &str) -> Self {
        let mut scope = Scope::default();
        for raw in text.lines() {
            let line = raw.split('#').next().unwrap_or("").trim().to_lowercase();
            if line.is_empty() {
                continue;
            }
            if let Some(suffix) = line.strip_prefix("*.") {
                scope.wildcards.push(suffix.to_string());
            } else {
                scope.exact.insert(line);
            }
        }
        scope
    }

    pub fn is_empty(&self) -> bool {
        self.exact.is_empty() && self.wildcards.is_empty()
    }

    /// Apakah host diizinkan?
    pub fn host_in_scope(&self, host: &str) -> bool {
        let host = host.trim().trim_end_matches('.').to_lowercase();
        if host.is_empty() {
            return false;
        }
        if self.exact.contains(&host) {
            return true;
        }
        for suffix in &self.wildcards {
            // *.example.com mencakup apex (example.com) dan subdomain (a.example.com).
            if host == *suffix || host.ends_with(&format!(".{suffix}")) {
                return true;
            }
        }
        false
    }

    /// Apakah URL ini boleh diakses?
    pub fn url_in_scope(&self, url: &Url) -> bool {
        match url.host_str() {
            Some(h) => self.host_in_scope(h),
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope() -> Scope {
        Scope::parse("# komentar\nexample.com\n*.target.io\n10.0.0.5\n")
    }

    #[test]
    fn exact_match() {
        assert!(scope().host_in_scope("example.com"));
        assert!(scope().host_in_scope("EXAMPLE.COM"));
    }

    #[test]
    fn wildcard_matches_apex_and_sub() {
        let s = scope();
        assert!(s.host_in_scope("target.io"));
        assert!(s.host_in_scope("api.target.io"));
        assert!(s.host_in_scope("a.b.target.io"));
    }

    #[test]
    fn out_of_scope_rejected() {
        let s = scope();
        assert!(!s.host_in_scope("evil.com"));
        assert!(!s.host_in_scope("nottarget.io"));
        assert!(!s.host_in_scope("target.io.evil.com"));
    }

    #[test]
    fn ip_exact() {
        assert!(scope().host_in_scope("10.0.0.5"));
        assert!(!scope().host_in_scope("10.0.0.6"));
    }

    #[test]
    fn empty_scope_is_empty() {
        assert!(Scope::parse("\n# only comments\n").is_empty());
    }
}
