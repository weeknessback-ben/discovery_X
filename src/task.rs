//! Definisi unit kerja (`Task`) dan hasilnya (`DiscoveryResult`).

use url::Url;

use crate::types::{AiInput, Asset, GraphEdge};

/// Asal-usul sebuah fetch — untuk provenance & kebijakan (mis. batas depth).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchOrigin {
    Seed,
    Crawl,
    Subdomain,
    AiProbe,
    /// Endpoint hasil ekstraksi dari file/inline JavaScript.
    JsEndpoint,
    /// Path hasil tebakan dirbust/fingerprint.
    Dirbust,
    /// URL dari robots.txt / sitemap.xml.
    Feed,
}

impl FetchOrigin {
    pub fn as_str(&self) -> &'static str {
        match self {
            FetchOrigin::Seed => "seed",
            FetchOrigin::Crawl => "crawl",
            FetchOrigin::Subdomain => "subdomain",
            FetchOrigin::AiProbe => "ai_probe",
            FetchOrigin::JsEndpoint => "js_endpoint",
            FetchOrigin::Dirbust => "dirbust",
            FetchOrigin::Feed => "feed",
        }
    }
}

/// Unit kerja yang mengalir lewat channel `task` ke worker.
#[derive(Debug, Clone)]
pub enum Task {
    /// Target awal. Orchestrator akan memecahnya menjadi `Fetch` + (opsional) `ResolveDns`.
    Seed(Url),
    /// Enumerasi subdomain untuk sebuah host root menggunakan wordlist.
    ResolveDns { host: String },
    /// Ambil sebuah URL via HTTP lalu fingerprint + parse isinya.
    Fetch {
        url: Url,
        origin: FetchOrigin,
        depth: usize,
    },
}

/// Hasil yang dikembalikan worker ke orchestrator melalui channel `result`.
#[derive(Debug, Default)]
pub struct DiscoveryResult {
    /// Aset yang ditemukan (akan disimpan & di-dedup).
    pub assets: Vec<Asset>,
    /// Task lanjutan yang ingin dijalankan (akan difilter scope + dedup).
    pub follow_up: Vec<Task>,
    /// Relasi antar-aset untuk attack graph.
    pub edges: Vec<GraphEdge>,
    /// Bila terisi, konten butuh dianalisis AI brain.
    pub ai_input: Option<AiInput>,
}

impl DiscoveryResult {
    pub fn requires_ai_analysis(&self) -> bool {
        self.ai_input.is_some()
    }
}
