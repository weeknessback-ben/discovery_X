//! DNS worker: enumerasi subdomain berbasis wordlist (async, via hickory).

use hickory_resolver::config::{ResolverConfig, ResolverOpts};
use hickory_resolver::TokioAsyncResolver;
use tracing::debug;
use url::Url;

use crate::scope::Scope;
use crate::task::{DiscoveryResult, FetchOrigin, Task};
use crate::types::{Asset, AssetKind, EdgeKind, GraphEdge};

/// Wordlist subdomain bawaan (kecil) bila user tidak menyediakan file.
const BUILTIN_WORDS: &[&str] = &[
    "www", "api", "dev", "staging", "test", "admin", "portal", "app", "mail", "vpn", "git",
    "jenkins", "internal", "beta", "dashboard", "auth", "static", "cdn", "assets", "gateway",
];

pub fn build_resolver() -> TokioAsyncResolver {
    TokioAsyncResolver::tokio(ResolverConfig::default(), ResolverOpts::default())
}

/// Muat wordlist dari file, atau pakai bawaan.
pub fn load_wordlist(path: Option<&str>) -> Vec<String> {
    if let Some(p) = path {
        if let Ok(text) = std::fs::read_to_string(p) {
            let words: Vec<String> = text
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .collect();
            if !words.is_empty() {
                return words;
            }
        }
    }
    BUILTIN_WORDS.iter().map(|s| s.to_string()).collect()
}

/// Enumerasi subdomain dari `root` (mis. "example.com").
///
/// Hanya subdomain yang lolos `scope` yang dipertimbangkan, lalu di-resolve.
/// Setiap subdomain hidup menghasilkan aset + task fetch http(s).
pub async fn enumerate(
    resolver: &TokioAsyncResolver,
    scope: &Scope,
    root: &str,
    words: &[String],
) -> DiscoveryResult {
    let mut result = DiscoveryResult::default();
    for w in words {
        let candidate = format!("{w}.{root}");
        // Hormati scope sebelum bahkan melakukan DNS lookup.
        if !scope.host_in_scope(&candidate) {
            continue;
        }
        match resolver.lookup_ip(candidate.as_str()).await {
            Ok(lookup) => {
                let ips: Vec<String> = lookup.iter().map(|ip| ip.to_string()).collect();
                if ips.is_empty() {
                    continue;
                }
                debug!(subdomain = %candidate, ips = ?ips, "subdomain hidup");
                result.assets.push(
                    Asset::new(AssetKind::Subdomain, candidate.clone(), "dns")
                        .with_note(format!("ips={}", ips.join(","))),
                );
                // Root host meng-resolve ke subdomain ini.
                result
                    .edges
                    .push(GraphEdge::new(root, candidate.clone(), EdgeKind::Resolves));
                for scheme in ["https", "http"] {
                    if let Ok(url) = Url::parse(&format!("{scheme}://{candidate}/")) {
                        result.follow_up.push(Task::Fetch {
                            url,
                            origin: FetchOrigin::Subdomain,
                            depth: 0,
                        });
                    }
                }
            }
            Err(_) => continue,
        }
    }
    result
}
