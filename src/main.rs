//! discovery_X — bootstrap web dashboard.
//!
//! Tool ini HANYA untuk pentesting/recon yang TERAUTORISASI. Scan dijalankan dari
//! dashboard web berautentikasi; guardrail scope allowlist tetap wajib per-scan.

mod ai;
mod config;
mod engine;
mod events;
mod graph;
mod orchestrator;
mod render;
mod scanmanager;
mod scope;
mod state;
mod task;
mod types;
mod web;
mod workers;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};

use crate::config::Config;
use crate::scanmanager::ScanManager;
use crate::state::StateStore;
use crate::web::state::{AppState, LoginGuard, Sessions};

#[derive(Parser, Debug)]
#[command(
    name = "discovery_x",
    about = "discovery_X — AI-guided discovery agent (web dashboard) untuk pentesting TERAUTORISASI",
    version
)]
struct Args {
    /// Path file config TOML (opsional; pakai default bila tidak diberikan).
    #[arg(long)]
    config: Option<PathBuf>,

    /// Override alamat bind (mis. 127.0.0.1:7373).
    #[arg(long)]
    bind: Option<String>,

    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Hasilkan hash Argon2id dari sebuah password untuk dipakai sebagai kredensial admin.
    HashPassword,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Subcommand utilitas: cetak hash password lalu keluar (tanpa server).
    if let Some(Cmd::HashPassword) = args.cmd {
        use std::io::IsTerminal;
        let pw = if std::io::stdin().is_terminal() {
            let pw = rpassword::prompt_password("Password admin           : ")?;
            let pw2 = rpassword::prompt_password("Ulangi password          : ")?;
            if pw != pw2 {
                bail!("password tidak cocok");
            }
            pw
        } else {
            // Non-interaktif (pipe): baca satu baris dari stdin.
            use std::io::BufRead;
            let mut line = String::new();
            std::io::stdin().lock().read_line(&mut line)?;
            line
        };
        let pw = pw.trim().to_string();
        if pw.is_empty() {
            bail!("password kosong");
        }
        let hash = web::auth::hash_password(&pw)?;
        println!("\nHash Argon2id:\n{hash}\n");
        println!("Taruh di config.toml:\n  [server]\n  admin_user = \"admin\"\n  admin_password_hash = \"{hash}\"");
        println!("\natau lewat env:\n  export DISCOVERY_ADMIN_PASSWORD_HASH='{hash}'");
        return Ok(());
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let mut cfg = Config::load(args.config.as_deref())?;
    if let Some(b) = args.bind {
        cfg.server.bind = b;
    }

    // Tanpa kredensial default: tolak start bila hash admin belum diset (A05/A07).
    if cfg.server.admin_password_hash.trim().is_empty() {
        bail!(
            "DITOLAK: kredensial admin belum diset. Jalankan `discovery_x hash-password`, \
             lalu isi [server].admin_password_hash di config.toml atau env DISCOVERY_ADMIN_PASSWORD_HASH."
        );
    }

    let state = Arc::new(
        StateStore::open(&cfg.output.db_path)
            .await
            .context("gagal membuka state store")?,
    );
    let scans = Arc::new(ScanManager::new(state.clone()));

    let app = Arc::new(AppState {
        sessions: Sessions::new(cfg.server.session_ttl_secs),
        login_guard: LoginGuard::new(cfg.server.login_max_attempts, cfg.server.login_lockout_secs),
        server: cfg.server.clone(),
        base_cfg: cfg.clone(),
        state,
        scans,
    });

    web::serve(app).await
}
