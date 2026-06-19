//! Tipe data bersama yang dipakai lintas modul.

use serde::{Deserialize, Serialize};

/// Jenis discovery yang bisa direncanakan oleh AI brain.
///
/// `serde` akan menolak nilai di luar varian ini — inilah "kontrak ketat"
/// yang mencegah halusinasi AI (lihat `discovery.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryType {
    #[serde(alias = "JsAnalysis", alias = "js")]
    JsAnalysis,
    #[serde(alias = "ParamGuessing", alias = "param")]
    ParamGuessing,
    #[serde(alias = "DirBusting", alias = "dir", alias = "directory")]
    DirBusting,
}

/// Klasifikasi aset yang ditemukan selama discovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetKind {
    Page,
    JsFile,
    Endpoint,
    Form,
    Subdomain,
    /// Teknologi/framework yang terdeteksi (mis. "WordPress 6.2").
    Tech,
}

/// Satu aset yang ditemukan. Diserialisasi ke `findings.jsonl`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Asset {
    pub kind: AssetKind,
    pub url: String,
    /// Bagaimana aset ini ditemukan (provenance) — berguna untuk audit.
    pub origin: String,
    /// Metadata tambahan (status code, server, title, dll).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

impl Asset {
    pub fn new(kind: AssetKind, url: impl Into<String>, origin: impl Into<String>) -> Self {
        Self {
            kind,
            url: url.into(),
            origin: origin.into(),
            notes: Vec::new(),
        }
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    /// Kunci unik untuk dedup.
    pub fn dedup_key(&self) -> String {
        format!("{:?}|{}", self.kind, self.url)
    }
}

/// Jenis relasi (edge) dalam attack graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    /// Halaman menaut ke halaman lain (`<a href>`).
    Links,
    /// Halaman mereferensikan file JS (`<script src>`).
    References,
    /// Halaman memuat sub-aset (mis. `<form>`).
    Contains,
    /// File JS memanggil/menyiratkan sebuah endpoint (hasil analisis AI).
    Calls,
    /// Host root meng-resolve ke subdomain.
    Resolves,
    /// Host menyajikan sebuah halaman/aset.
    Hosts,
    /// Path hasil tebakan (dirbust/fingerprint) — bukan tautan eksplisit.
    Guessed,
}

impl EdgeKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            EdgeKind::Links => "links",
            EdgeKind::References => "references",
            EdgeKind::Contains => "contains",
            EdgeKind::Calls => "calls",
            EdgeKind::Resolves => "resolves",
            EdgeKind::Hosts => "hosts",
            EdgeKind::Guessed => "guessed",
        }
    }
}

impl std::fmt::Display for EdgeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Sebuah relasi terarah `from -> to` untuk attack graph (kunci = URL/host).
#[derive(Debug, Clone)]
pub struct GraphEdge {
    pub from: String,
    pub to: String,
    pub kind: EdgeKind,
}

impl GraphEdge {
    pub fn new(from: impl Into<String>, to: impl Into<String>, kind: EdgeKind) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
            kind,
        }
    }
}

/// Input yang dikirim ke AI brain untuk dianalisis.
#[derive(Debug, Clone)]
pub struct AiInput {
    /// URL sumber (mis. file JS) tempat kandidat diekstrak.
    pub source_url: String,
    /// Origin (skema://host[:port]) untuk me-resolve path relatif hasil AI.
    pub base_origin: String,
    /// String kandidat hasil ekstraksi regex dari file JS.
    pub candidates: Vec<String>,
    /// Teknologi/framework yang terdeteksi (fingerprint) agar AI bisa menyarankan
    /// path khas stack tersebut (mis. WordPress → /wp-json/...).
    pub tech: Vec<String>,
}
