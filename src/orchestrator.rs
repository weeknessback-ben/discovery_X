//! Orchestrator: event loop asinkron yang menyatukan worker, state, dan AI brain.
//!
//! Pola persis seperti `discovery.md`: dua channel (`task`/`result`),
//! `tokio::select!`, dan konkurensi dibatasi `Semaphore` (global + per-domain).

use std::collections::HashMap;
use std::sync::Arc;

use reqwest::Client;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::{mpsc, Notify, Semaphore};
use tracing::{info, warn};
use url::Url;

use crate::ai::AiBrain;
use crate::config::Config;
use crate::graph::AttackGraph;
use crate::scope::Scope;
use crate::state::StateStore;
use crate::task::{FetchOrigin, Task};
use crate::types::{AssetKind, EdgeKind};
use crate::events::ScanEvent;
use crate::workers::{dns, http};

/// Pesan dari unit kerja yang dispawn kembali ke loop utama.
/// Tiap unit yang dispawn mengirim TEPAT satu `Msg` agar penghitung in-flight akurat.
enum Msg {
    /// Hasil dari worker fetch/dns.
    Result(crate::task::DiscoveryResult),
    /// Task fetch baru hasil rencana AI (kosong bila AI gagal), beserta URL sumber JS.
    AiPlanTasks { source: String, tasks: Vec<Task> },
}

/// Ringkasan hasil run untuk dicetak setelah selesai.
pub struct RunReport {
    pub graph_nodes: usize,
    pub graph_edges: usize,
    pub dot_path: Option<String>,
    /// Graf sebagai JSON {nodes, edges} untuk visualisasi D3 di dashboard.
    pub graph_json: Option<String>,
    /// Disurfacekan di detail scan (dashboard) nanti.
    #[allow(dead_code)]
    pub ai_endpoints: Vec<String>,
    #[allow(dead_code)]
    pub top_hubs: Vec<(String, usize)>,
}

pub struct Orchestrator {
    cfg: Config,
    scope: Arc<Scope>,
    state: Arc<StateStore>,
    client: Client,
    ai: Option<Arc<dyn AiBrain>>,
    resolver: hickory_resolver::TokioAsyncResolver,
    wordlist: Arc<Vec<String>>,
    global_sem: Arc<Semaphore>,
    domain_sems: HashMap<String, Arc<Semaphore>>,
    /// Channel opsional ke TUI dashboard.
    ui_tx: Option<UnboundedSender<ScanEvent>>,
    /// Renderer headless opsional (untuk SPA).
    renderer: Option<Arc<crate::render::Renderer>>,
}

impl Orchestrator {
    pub fn new(
        cfg: Config,
        scope: Arc<Scope>,
        state: Arc<StateStore>,
        client: Client,
        ai: Option<Arc<dyn AiBrain>>,
        ui_tx: Option<UnboundedSender<ScanEvent>>,
        renderer: Option<Arc<crate::render::Renderer>>,
    ) -> Self {
        let global_sem = Arc::new(Semaphore::new(cfg.recon.global_concurrency.max(1)));
        let wordlist = Arc::new(dns::load_wordlist(cfg.recon.subdomain_wordlist.as_deref()));
        Self {
            scope,
            state,
            client,
            ai,
            resolver: dns::build_resolver(),
            wordlist,
            global_sem,
            domain_sems: HashMap::new(),
            ui_tx,
            renderer,
            cfg,
        }
    }

    /// Kirim event ke dashboard bila aktif (no-op bila tidak).
    fn emit(&self, ev: ScanEvent) {
        if let Some(tx) = &self.ui_tx {
            let _ = tx.send(ev);
        }
    }

    fn domain_sem(&mut self, host: &str) -> Arc<Semaphore> {
        let limit = self.cfg.recon.per_domain_concurrency.max(1);
        self.domain_sems
            .entry(host.to_string())
            .or_insert_with(|| Arc::new(Semaphore::new(limit)))
            .clone()
    }

    /// Jalankan discovery dari satu seed URL sampai antrian habis,
    /// atau sampai `cancel` dipicu (mis. user keluar dari TUI).
    pub async fn run(mut self, seed: Url, cancel: Arc<Notify>) -> RunReport {
        let (task_tx, mut task_rx) = mpsc::unbounded_channel::<Task>();
        let (msg_tx, mut msg_rx) = mpsc::unbounded_channel::<Msg>();

        // Attack graph in-memory (dibangun di loop ini → tanpa lock).
        let mut graph = AttackGraph::new();

        // Stack teknologi yang terdeteksi (fingerprint) → dikirim ke AI sbg konteks.
        let mut detected_tech: Vec<String> = Vec::new();

        // Penghitung in-flight: hanya dimutasi di loop ini (tanpa race).
        let mut in_flight: i64 = 0;

        // Future pembatalan; di-pin agar waiter tetap teregistrasi antar iterasi.
        let cancel_fut = cancel.notified();
        tokio::pin!(cancel_fut);

        // Seed awal.
        enqueue(&mut in_flight, &task_tx, &self.scope, &self.state, Task::Seed(seed));

        loop {
            tokio::select! {
                // Pembatalan dari TUI.
                _ = &mut cancel_fut => {
                    break;
                }
                // A. Terima task baru → eksekusi/spawn worker.
                Some(task) = task_rx.recv() => {
                    match task {
                        Task::Seed(url) => {
                            // Pecah seed menjadi fetch awal (+ enumerasi subdomain bila aktif).
                            enqueue(&mut in_flight, &task_tx, &self.scope, &self.state,
                                Task::Fetch { url: url.clone(), origin: FetchOrigin::Seed, depth: 0 });
                            // robots.txt & sitemap.xml = sumber URL gratis.
                            if self.cfg.recon.enable_feeds {
                                let base = http::origin_of(&url);
                                for feed in ["/robots.txt", "/sitemap.xml"] {
                                    if let Ok(u) = Url::parse(&base).and_then(|b| b.join(feed)) {
                                        enqueue(&mut in_flight, &task_tx, &self.scope, &self.state,
                                            Task::Fetch { url: u, origin: FetchOrigin::Feed, depth: 0 });
                                    }
                                }
                            }
                            if self.cfg.recon.enable_subdomain_enum {
                                if let Some(host) = url.host_str() {
                                    enqueue(&mut in_flight, &task_tx, &self.scope, &self.state,
                                        Task::ResolveDns { host: host.to_string() });
                                }
                            }
                            in_flight -= 1; // unit Seed selesai
                        }
                        Task::Fetch { url, origin, depth } => {
                            let host = url.host_str().unwrap_or_default().to_string();
                            let dom = self.domain_sem(&host);
                            let global = self.global_sem.clone();
                            let client = self.client.clone();
                            let scope = self.scope.clone();
                            let opts = http::FetchOpts {
                                max_depth: self.cfg.recon.max_depth,
                                enable_dirbust: self.cfg.recon.enable_dirbust,
                                max_js_probes: self.cfg.recon.max_js_probes,
                                max_dirbust: self.cfg.recon.max_dirbust,
                                verify_live: self.cfg.recon.verify_live,
                            };
                            let out = msg_tx.clone();
                            let renderer = self.renderer.clone();
                            tokio::spawn(async move {
                                let _g = global.acquire().await;
                                let _d = dom.acquire().await;
                                let res = http::fetch(
                                    &client, &scope, url, origin, depth, opts, renderer.as_deref(),
                                )
                                .await
                                .unwrap_or_default();
                                let _ = out.send(Msg::Result(res));
                            });
                        }
                        Task::ResolveDns { host } => {
                            let global = self.global_sem.clone();
                            let resolver = self.resolver.clone();
                            let scope = self.scope.clone();
                            let words = self.wordlist.clone();
                            let out = msg_tx.clone();
                            tokio::spawn(async move {
                                let _g = global.acquire().await;
                                let res = dns::enumerate(&resolver, &scope, &host, &words).await;
                                let _ = out.send(Msg::Result(res));
                            });
                        }
                    }
                }

                // B. Terima hasil dari worker / AI.
                Some(msg) = msg_rx.recv() => {
                    match msg {
                        Msg::Result(res) => {
                            let needs_ai = res.requires_ai_analysis();
                            // Simpan aset + node graf.
                            for asset in &res.assets {
                                graph.add_asset(&asset.url, asset.kind);
                                // Kumpulkan stack terdeteksi untuk konteks AI.
                                if asset.kind == AssetKind::Tech {
                                    if let Some(label) = asset.notes.first() {
                                        if !detected_tech.contains(label) {
                                            detected_tech.push(label.clone());
                                        }
                                    }
                                }
                                match self.state.record_asset(asset).await {
                                    Ok(true) => self.emit(ScanEvent::Asset { asset: asset.clone() }),
                                    Ok(false) => {}
                                    Err(e) => warn!(error = %e, "gagal menulis temuan"),
                                }
                            }
                            // Edge relasi antar-aset.
                            for e in &res.edges {
                                graph.add_edge(&e.from, &e.to, e.kind);
                            }
                            // Enqueue follow-up (difilter scope + dedup di enqueue).
                            for t in res.follow_up {
                                enqueue(&mut in_flight, &task_tx, &self.scope, &self.state, t);
                            }
                            // Bila perlu analisis AI, spawn unit AI baru.
                            if let (true, Some(mut input), Some(ai)) =
                                (needs_ai, res.ai_input, self.ai.clone())
                            {
                                input.tech = detected_tech.clone();
                                in_flight += 1; // unit AI
                                let out = msg_tx.clone();
                                let ui = self.ui_tx.clone();
                                let source = input.source_url.clone();
                                tokio::spawn(async move {
                                    let tasks = match ai.analyze(&input).await {
                                        Ok(plan) => {
                                            info!(source = %input.source_url,
                                                  target_url = %plan.target_url,
                                                  n = plan.payloads.len(),
                                                  kind = ?plan.discovery_type,
                                                  reasoning = %plan.reasoning,
                                                  "AI mengusulkan probe");
                                            if let Some(ui) = &ui {
                                                let _ = ui.send(ScanEvent::AiProposal {
                                                    source: input.source_url.clone(),
                                                    count: plan.payloads.len(),
                                                });
                                            }
                                            plan_to_tasks(&input.base_origin, &plan.payloads)
                                        }
                                        Err(e) => {
                                            warn!(error = %e, "analisis AI gagal");
                                            if let Some(ui) = &ui {
                                                let _ = ui.send(ScanEvent::Log {
                                                    line: format!("AI gagal: {e}"),
                                                });
                                            }
                                            Vec::new()
                                        }
                                    };
                                    let _ = out.send(Msg::AiPlanTasks { source, tasks });
                                });
                            }
                            in_flight -= 1; // unit fetch/dns selesai
                        }
                        Msg::AiPlanTasks { source, tasks } => {
                            for t in tasks {
                                // Edge Calls: file JS sumber → endpoint hasil inferensi AI.
                                if let Task::Fetch { url, .. } = &t {
                                    graph.add_edge(&source, url.as_str(), EdgeKind::Calls);
                                }
                                enqueue(&mut in_flight, &task_tx, &self.scope, &self.state, t);
                            }
                            in_flight -= 1; // unit AI selesai
                        }
                    }
                }
            }

            self.emit(ScanEvent::InFlight { count: in_flight.max(0) });
            self.emit(ScanEvent::Graph {
                nodes: graph.node_count(),
                edges: graph.edge_count(),
            });

            if in_flight <= 0 {
                break;
            }
        }

        // Ekspor attack graph ke DOT (Graphviz).
        let dot_path = match std::fs::write(&self.cfg.output.graph_path, graph.to_dot()) {
            Ok(()) => {
                info!(path = %self.cfg.output.graph_path, "attack graph diekspor (DOT)");
                Some(self.cfg.output.graph_path.clone())
            }
            Err(e) => {
                warn!(error = %e, "gagal menulis attack graph");
                None
            }
        };

        self.emit(ScanEvent::Finished);

        RunReport {
            graph_nodes: graph.node_count(),
            graph_edges: graph.edge_count(),
            dot_path,
            graph_json: Some(graph.to_json()),
            ai_endpoints: graph.ai_discovered_endpoints(),
            top_hubs: graph.top_hubs(5),
        }
    }
}

/// Masukkan task ke antrian dengan filter scope + dedup (khusus Fetch).
fn enqueue(
    in_flight: &mut i64,
    tx: &mpsc::UnboundedSender<Task>,
    scope: &Scope,
    state: &StateStore,
    task: Task,
) {
    if let Task::Fetch { url, .. } = &task {
        if !scope.url_in_scope(url) {
            return;
        }
        // Dedup: hanya fetch URL yang belum pernah dikunjungi.
        if !state.mark_visited(&format!("fetch:{}", url.as_str())) {
            return;
        }
    }
    *in_flight += 1;
    if tx.send(task).is_err() {
        *in_flight -= 1;
    }
}

/// Ubah payload (path/URL) hasil AI menjadi `Task::Fetch` (origin AiProbe).
fn plan_to_tasks(base_origin: &str, payloads: &[String]) -> Vec<Task> {
    let mut out = Vec::new();
    for p in payloads {
        let resolved = if p.starts_with("http://") || p.starts_with("https://") {
            Url::parse(p).ok()
        } else {
            Url::parse(base_origin).ok().and_then(|b| b.join(p).ok())
        };
        if let Some(url) = resolved {
            out.push(Task::Fetch {
                url,
                origin: FetchOrigin::AiProbe,
                depth: 0,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_to_tasks_resolves_relative_and_absolute() {
        let tasks = plan_to_tasks(
            "https://target.io",
            &[
                "/api/admin".to_string(),
                "https://target.io/secret".to_string(),
            ],
        );
        let urls: Vec<String> = tasks
            .iter()
            .map(|t| match t {
                Task::Fetch { url, .. } => url.to_string(),
                _ => String::new(),
            })
            .collect();
        // path relatif di-resolve terhadap origin; URL absolut dipertahankan.
        assert!(urls.iter().any(|u| u == "https://target.io/api/admin"));
        assert!(urls.iter().any(|u| u == "https://target.io/secret"));
    }
}
