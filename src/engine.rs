//! Menjalankan satu scan: rakit client/AI/renderer/orchestrator dari config efektif.
//! Diekstrak dari `main.rs` lama agar bisa dipanggil dari dashboard (ScanManager).

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::Notify;
use tracing::{info, warn};
use url::Url;

use crate::ai::openai_compat::OpenAiCompatBrain;
use crate::ai::AiBrain;
use crate::config::Config;
use crate::events::ScanEvent;
use crate::orchestrator::{Orchestrator, RunReport};
use crate::render::Renderer;
use crate::scope::Scope;
use crate::state::StateStore;

/// Jalankan discovery untuk satu `seed` memakai `cfg` efektif (recon+ai sudah dioverlay).
pub async fn run_scan(
    cfg: Config,
    scope: Arc<Scope>,
    seed: Url,
    state: Arc<StateStore>,
    scan_id: i64,
    event_tx: UnboundedSender<ScanEvent>,
    cancel: Arc<Notify>,
) -> RunReport {
    state.begin_scan(scan_id);

    let client = reqwest::Client::builder()
        .user_agent(&cfg.recon.user_agent)
        .timeout(Duration::from_secs(cfg.recon.request_timeout_secs))
        .cookie_store(true)
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    // AI brain (opsional).
    let ai: Option<Arc<dyn AiBrain>> = if cfg.ai.enabled {
        match cfg.ai.api_key.as_deref() {
            Some(k) if !k.trim().is_empty() => match OpenAiCompatBrain::new(
                cfg.ai.base_url.clone(),
                cfg.ai.model.clone(),
                cfg.ai.api_key.clone(),
                cfg.ai.temperature,
                cfg.ai.max_retries,
                cfg.ai.timeout_secs,
            ) {
                Ok(b) => {
                    info!(model = %cfg.ai.model, "AI brain aktif");
                    Some(Arc::new(b) as Arc<dyn AiBrain>)
                }
                Err(e) => {
                    warn!(error = %e, "AI gagal diinisialisasi; mode recon-only");
                    None
                }
            },
            _ => {
                info!("AI tanpa API key; mode recon-only");
                None
            }
        }
    } else {
        None
    };

    // Renderer headless (opsional) — dibutuhkan untuk render SPA ATAU verifikasi liveness.
    let renderer: Option<Arc<Renderer>> = if cfg.recon.enable_render || cfg.recon.verify_live {
        match Renderer::launch(
            cfg.recon.render_max_concurrent,
            cfg.recon.render_timeout_secs,
            cfg.recon.render_wait_ms,
        )
        .await
        {
            Ok(r) => Some(Arc::new(r)),
            Err(e) => {
                warn!(error = %e, "renderer gagal diluncurkan; lanjut tanpa render JS");
                None
            }
        }
    } else {
        None
    };

    let orch = Orchestrator::new(cfg, scope, state, client, ai, Some(event_tx), renderer);
    orch.run(seed, cancel).await
}
