//! Mengelola SATU scan aktif + jembatan event ke klien SSE.
//!
//! - `start` menolak bila ada scan berjalan (model satu-scan-aktif).
//! - Event dari engine (mpsc) diteruskan ke broadcast (banyak klien SSE) sekaligus
//!   memperbarui `LiveStatus` agar halaman yang baru dibuka langsung melihat keadaan.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{bail, Result};
use serde::Serialize;
use tokio::sync::{broadcast, mpsc, Notify};
use tracing::{info, warn};
use url::Url;

use crate::config::Config;
use crate::engine;
use crate::events::ScanEvent;
use crate::scope::Scope;
use crate::state::StateStore;

/// Ringkasan keadaan scan aktif (untuk endpoint status).
#[derive(Debug, Clone, Default, Serialize)]
pub struct LiveStatus {
    pub running: bool,
    pub scan_id: Option<i64>,
    pub seed: Option<String>,
    pub finished: bool,
    pub in_flight: i64,
    pub total: usize,
    pub pages: usize,
    pub js: usize,
    pub forms: usize,
    pub subdomains: usize,
    pub endpoints: usize,
    pub tech: usize,
    pub ai_proposals: usize,
    pub graph_nodes: usize,
    pub graph_edges: usize,
}

pub struct ScanManager {
    state: Arc<StateStore>,
    tx: broadcast::Sender<ScanEvent>,
    running: AtomicBool,
    live: Mutex<LiveStatus>,
    cancel: Mutex<Option<Arc<Notify>>>,
}

impl ScanManager {
    pub fn new(state: Arc<StateStore>) -> Self {
        let (tx, _) = broadcast::channel(512);
        Self {
            state,
            tx,
            running: AtomicBool::new(false),
            live: Mutex::new(LiveStatus::default()),
            cancel: Mutex::new(None),
        }
    }

    /// Berlangganan stream event untuk satu klien SSE.
    pub fn subscribe(&self) -> broadcast::Receiver<ScanEvent> {
        self.tx.subscribe()
    }

    pub fn status(&self) -> LiveStatus {
        self.live.lock().unwrap().clone()
    }

    /// Hentikan scan yang sedang berjalan (bila ada).
    pub fn stop(&self) {
        if let Some(c) = self.cancel.lock().unwrap().as_ref() {
            c.notify_one();
        }
    }

    fn apply_event(&self, ev: &ScanEvent) {
        use crate::types::AssetKind::*;
        let mut l = self.live.lock().unwrap();
        match ev {
            ScanEvent::Asset { asset } => {
                l.total += 1;
                match asset.kind {
                    Page => l.pages += 1,
                    JsFile => l.js += 1,
                    Form => l.forms += 1,
                    Subdomain => l.subdomains += 1,
                    Endpoint => l.endpoints += 1,
                    Tech => l.tech += 1,
                }
            }
            ScanEvent::InFlight { count } => l.in_flight = *count,
            ScanEvent::AiProposal { .. } => l.ai_proposals += 1,
            ScanEvent::Graph { nodes, edges } => {
                l.graph_nodes = *nodes;
                l.graph_edges = *edges;
            }
            ScanEvent::Log { .. } => {}
            ScanEvent::Finished => l.finished = true,
        }
    }

    /// Mulai scan baru. `cfg` = config efektif (recon+ai), `scope` sudah diparse,
    /// `seed` sudah divalidasi ∈ scope oleh pemanggil.
    pub async fn start(
        self: &Arc<Self>,
        cfg: Config,
        scope: Arc<Scope>,
        seed: Url,
        scope_text: String,
    ) -> Result<i64> {
        if self
            .running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            bail!("scan lain sedang berjalan");
        }

        let scan_id = match self.state.create_scan(seed.as_str(), &scope_text).await {
            Ok(id) => id,
            Err(e) => {
                self.running.store(false, Ordering::SeqCst);
                return Err(e);
            }
        };

        // Reset status live untuk scan baru.
        {
            let mut live = self.live.lock().unwrap();
            *live = LiveStatus {
                running: true,
                scan_id: Some(scan_id),
                seed: Some(seed.to_string()),
                ..Default::default()
            };
        }

        let cancel = Arc::new(Notify::new());
        *self.cancel.lock().unwrap() = Some(cancel.clone());

        let this = self.clone();
        tokio::spawn(async move {
            info!(scan_id, %seed, "scan dimulai");
            let (etx, mut erx) = mpsc::unbounded_channel::<ScanEvent>();

            // Forwarder: engine(mpsc) → live status + broadcast(SSE).
            let fwd_this = this.clone();
            let fwd = tokio::spawn(async move {
                while let Some(ev) = erx.recv().await {
                    fwd_this.apply_event(&ev);
                    let _ = fwd_this.tx.send(ev);
                }
            });

            let report =
                engine::run_scan(cfg, scope, seed, this.state.clone(), scan_id, etx, cancel).await;
            let _ = fwd.await;

            // Ambil isi DOT (file bisa ditimpa scan berikutnya → simpan ke DB).
            let dot = report
                .dot_path
                .as_deref()
                .and_then(|p| std::fs::read_to_string(p).ok());
            let assets = this.state.assets_written() as i64;
            if let Err(e) = this
                .state
                .finish_scan(
                    scan_id,
                    "finished",
                    assets,
                    report.graph_nodes as i64,
                    report.graph_edges as i64,
                    dot.as_deref(),
                    report.graph_json.as_deref(),
                    None,
                )
                .await
            {
                warn!(error = %e, "gagal menyimpan ringkasan scan");
            }

            {
                let mut l = this.live.lock().unwrap();
                l.running = false;
                l.finished = true;
            }
            *this.cancel.lock().unwrap() = None;
            this.running.store(false, Ordering::SeqCst);
            info!(scan_id, "scan selesai");
        });

        Ok(scan_id)
    }
}
