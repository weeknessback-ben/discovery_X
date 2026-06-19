//! Konfigurasi runtime (dimuat dari TOML, dengan override env untuk API key).

use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Nama env var yang menimpa `ai.api_key` di file config.
pub const API_KEY_ENV: &str = "AGENT_AI_API_KEY";
/// Env var yang menimpa `ai.base_url` (mis. mengarahkan ke proxy LiteLLM).
pub const BASE_URL_ENV: &str = "AGENT_AI_BASE_URL";
/// Env var yang menimpa `ai.model` (alias model di LiteLLM / nama model provider).
pub const MODEL_ENV: &str = "AGENT_AI_MODEL";

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub ai: AiConfig,
    #[serde(default)]
    pub recon: ReconConfig,
    #[serde(default)]
    pub output: OutputConfig,
}

/// Konfigurasi web server + auth. Dimuat dari TOML/env saja (TIDAK bisa diubah
/// dari dashboard, demi keamanan). Tanpa kredensial default.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    /// Alamat bind. Default localhost demi keamanan (jangan ekspos ke jaringan).
    pub bind: String,
    /// Username admin.
    pub admin_user: String,
    /// Hash Argon2id dari password admin (buat via `discovery_x hash-password`).
    pub admin_password_hash: String,
    /// Masa berlaku sesi (detik).
    pub session_ttl_secs: u64,
    /// Maksimum percobaan login gagal sebelum lockout (per-IP).
    pub login_max_attempts: u32,
    /// Durasi lockout setelah melebihi batas (detik).
    pub login_lockout_secs: u64,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1:7373".to_string(),
            admin_user: "admin".to_string(),
            admin_password_hash: String::new(),
            session_ttl_secs: 8 * 3600,
            login_max_attempts: 5,
            login_lockout_secs: 300,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct AiConfig {
    /// Endpoint chat-completions OpenAI-compatible (mis. provider langsung, atau
    /// proxy LiteLLM untuk akses banyak provider). Override via env AGENT_AI_BASE_URL.
    pub base_url: String,
    /// Nama model (mis. "glm-5.2") atau alias model di LiteLLM. Override via env AGENT_AI_MODEL.
    pub model: String,
    /// API key. Bila kosong di sini, diambil dari env `AGENT_AI_API_KEY`.
    pub api_key: Option<String>,
    /// Apakah AI loop diaktifkan. Bila false, agen hanya melakukan recon.
    pub enabled: bool,
    /// Maksimum percobaan ulang saat output AI gagal divalidasi `serde`.
    pub max_retries: u32,
    pub temperature: f32,
    /// Timeout permintaan ke endpoint AI (detik).
    pub timeout_secs: u64,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            base_url: "https://opencode.ai/zen/go/v1/chat/completions".to_string(),
            model: "glm-5.2".to_string(),
            api_key: None,
            enabled: true,
            max_retries: 2,
            temperature: 0.1,
            timeout_secs: 60,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct ReconConfig {
    /// Batas konkurensi global (jumlah request HTTP serempak di seluruh target).
    pub global_concurrency: usize,
    /// Batas konkurensi per-domain (mencegah membanjiri satu target).
    pub per_domain_concurrency: usize,
    /// Timeout tiap request HTTP (detik).
    pub request_timeout_secs: u64,
    /// User-Agent yang dipakai untuk semua request.
    pub user_agent: String,
    /// Kedalaman crawl maksimum dari seed.
    pub max_depth: usize,
    /// Aktifkan enumerasi subdomain berbasis wordlist.
    pub enable_subdomain_enum: bool,
    /// Path wordlist subdomain (opsional). Bila kosong dipakai daftar bawaan kecil.
    pub subdomain_wordlist: Option<String>,
    /// Ambil & parse robots.txt + sitemap.xml dari seed.
    pub enable_feeds: bool,
    /// Fingerprint teknologi + dirbust terarah pada seed.
    pub enable_dirbust: bool,
    /// Batas probe endpoint dari satu file/halaman JS.
    pub max_js_probes: usize,
    /// Batas jumlah path dirbust yang dicoba.
    pub max_dirbust: usize,
    /// Render JS via headless Chrome untuk SPA (butuh Chrome terinstall).
    pub enable_render: bool,
    /// Verifikasi endpoint hasil temuan benar-benar "hidup" via headless browser
    /// (deteksi soft-404 SPA yang HTTP-nya 200 tapi sebenarnya halaman error).
    pub verify_live: bool,
    /// Maksimum render/tab serempak.
    pub render_max_concurrent: usize,
    /// Timeout render per halaman (detik).
    pub render_timeout_secs: u64,
    /// Jeda tunggu agar JS sempat me-render DOM (ms).
    pub render_wait_ms: u64,
}

impl Default for ReconConfig {
    fn default() -> Self {
        Self {
            global_concurrency: 50,
            per_domain_concurrency: 8,
            request_timeout_secs: 15,
            user_agent: "discovery_x/0.1 (+authorized-pentest)".to_string(),
            max_depth: 3,
            enable_subdomain_enum: false,
            subdomain_wordlist: None,
            enable_feeds: true,
            enable_dirbust: true,
            max_js_probes: 40,
            max_dirbust: 60,
            enable_render: false,
            verify_live: false,
            render_max_concurrent: 2,
            render_timeout_secs: 20,
            render_wait_ms: 800,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct OutputConfig {
    /// File database SQLite tempat menyimpan temuan + dedup (resume).
    pub db_path: String,
    /// File DOT (Graphviz) tempat mengekspor attack graph.
    pub graph_path: String,
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            db_path: "discovery.db".to_string(),
            graph_path: "attack-graph.dot".to_string(),
        }
    }
}

impl Config {
    /// Muat config dari path TOML. Bila path None, pakai default.
    pub fn load(path: Option<&Path>) -> Result<Self> {
        let mut cfg = match path {
            Some(p) => {
                let text = std::fs::read_to_string(p)
                    .with_context(|| format!("gagal membaca config {}", p.display()))?;
                toml::from_str(&text).with_context(|| format!("config TOML tidak valid: {}", p.display()))?
            }
            None => Config {
                server: ServerConfig::default(),
                ai: AiConfig::default(),
                recon: ReconConfig::default(),
                output: OutputConfig::default(),
            },
        };

        // Env var menimpa API key dari file (lebih aman daripada commit key ke disk).
        if let Ok(key) = std::env::var(API_KEY_ENV) {
            if !key.trim().is_empty() {
                cfg.ai.api_key = Some(key);
            }
        }
        // Endpoint & model AI bisa di-override env (memudahkan wiring ke LiteLLM di Docker).
        if let Ok(v) = std::env::var(BASE_URL_ENV) {
            if !v.trim().is_empty() {
                cfg.ai.base_url = v;
            }
        }
        if let Ok(v) = std::env::var(MODEL_ENV) {
            if !v.trim().is_empty() {
                cfg.ai.model = v;
            }
        }
        // Kredensial admin & bind boleh diset via env (lebih aman utk hash password).
        if let Ok(v) = std::env::var("DISCOVERY_ADMIN_USER") {
            if !v.trim().is_empty() {
                cfg.server.admin_user = v;
            }
        }
        if let Ok(v) = std::env::var("DISCOVERY_ADMIN_PASSWORD_HASH") {
            if !v.trim().is_empty() {
                cfg.server.admin_password_hash = v;
            }
        }
        if let Ok(v) = std::env::var("DISCOVERY_BIND") {
            if !v.trim().is_empty() {
                cfg.server.bind = v;
            }
        }
        Ok(cfg)
    }
}
