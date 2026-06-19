//! Penyimpanan state: SQLite (sqlx) untuk temuan per-scan, riwayat scan, & settings.
//!
//! - Temuan disimpan di tabel `assets` (PK gabungan `(scan_id, dedup_key)` → dedup
//!   per-scan via `INSERT OR IGNORE`).
//! - Dedup *fetch* (hot-path sinkron dari orchestrator) ditahan in-memory `HashSet`.
//! - `scans` menyimpan riwayat run; `settings` menyimpan config editable (recon/ai).

use std::collections::HashSet;
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
use std::sync::Mutex;

use anyhow::{Context, Result};
use serde::Serialize;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};

use crate::types::Asset;

pub struct StateStore {
    pool: SqlitePool,
    /// Dedup URL yang sudah di-fetch (in-memory, per-run).
    visited: Mutex<HashSet<String>>,
    /// Jumlah aset baru yang ditulis pada scan aktif.
    written: AtomicUsize,
    /// ID scan aktif (0 = belum ada). Aset baru ditandai dengan ini.
    current_scan: AtomicI64,
}

/// Baris riwayat scan (untuk API dashboard).
#[derive(Debug, Serialize)]
pub struct ScanRow {
    pub id: i64,
    pub seed: String,
    pub scope: String,
    pub status: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub assets_count: i64,
    pub graph_nodes: i64,
    pub graph_edges: i64,
    pub dot: Option<String>,
    pub error: Option<String>,
}

/// Baris temuan (untuk API dashboard).
#[derive(Debug, Serialize)]
pub struct AssetRow {
    pub kind: String,
    pub url: String,
    pub origin: String,
    pub notes: String,
    pub found_at: String,
}

impl StateStore {
    pub async fn open(db_path: &str) -> Result<Self> {
        let opts = SqliteConnectOptions::new()
            .filename(db_path)
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await
            .with_context(|| format!("gagal membuka SQLite {db_path}"))?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS assets (
                scan_id   INTEGER NOT NULL DEFAULT 0,
                dedup_key TEXT NOT NULL,
                kind      TEXT NOT NULL,
                url       TEXT NOT NULL,
                origin    TEXT NOT NULL,
                notes     TEXT NOT NULL DEFAULT '[]',
                found_at  TEXT NOT NULL DEFAULT (datetime('now')),
                PRIMARY KEY (scan_id, dedup_key)
            )",
        )
        .execute(&pool)
        .await
        .context("gagal membuat tabel assets")?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS scans (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                seed         TEXT NOT NULL,
                scope        TEXT NOT NULL,
                status       TEXT NOT NULL,
                started_at   TEXT NOT NULL DEFAULT (datetime('now')),
                finished_at  TEXT,
                assets_count INTEGER NOT NULL DEFAULT 0,
                graph_nodes  INTEGER NOT NULL DEFAULT 0,
                graph_edges  INTEGER NOT NULL DEFAULT 0,
                dot          TEXT,
                error        TEXT
            )",
        )
        .execute(&pool)
        .await
        .context("gagal membuat tabel scans")?;

        // Migrasi toleran: tambah kolom graph_json bila DB lama belum punya.
        let _ = sqlx::query("ALTER TABLE scans ADD COLUMN graph_json TEXT")
            .execute(&pool)
            .await;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS settings (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            )",
        )
        .execute(&pool)
        .await
        .context("gagal membuat tabel settings")?;

        // Batasi izin file DB (berisi API key + temuan) → hanya pemilik.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(meta) = std::fs::metadata(db_path) {
                let mut perm = meta.permissions();
                perm.set_mode(0o600);
                let _ = std::fs::set_permissions(db_path, perm);
            }
        }

        Ok(Self {
            pool,
            visited: Mutex::new(HashSet::new()),
            written: AtomicUsize::new(0),
            current_scan: AtomicI64::new(0),
        })
    }

    /// Tandai sebuah kunci (mis. `fetch:<url>`) sudah dikunjungi.
    pub fn mark_visited(&self, key: &str) -> bool {
        self.visited.lock().unwrap().insert(key.to_string())
    }

    /// Reset state per-scan baru: kosongkan dedup in-memory & set scan aktif.
    pub fn begin_scan(&self, scan_id: i64) {
        self.visited.lock().unwrap().clear();
        self.written.store(0, Ordering::Relaxed);
        self.current_scan.store(scan_id, Ordering::Relaxed);
    }

    /// Catat aset ke SQLite (ditandai scan aktif). `true` bila baris baru.
    pub async fn record_asset(&self, asset: &Asset) -> Result<bool> {
        let notes = serde_json::to_string(&asset.notes).unwrap_or_else(|_| "[]".to_string());
        let kind = format!("{:?}", asset.kind);
        let scan_id = self.current_scan.load(Ordering::Relaxed);
        let res = sqlx::query(
            "INSERT OR IGNORE INTO assets (scan_id, dedup_key, kind, url, origin, notes)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )
        .bind(scan_id)
        .bind(asset.dedup_key())
        .bind(kind)
        .bind(&asset.url)
        .bind(&asset.origin)
        .bind(notes)
        .execute(&self.pool)
        .await
        .context("gagal menulis asset ke SQLite")?;

        let inserted = res.rows_affected() == 1;
        if inserted {
            self.written.fetch_add(1, Ordering::Relaxed);
        }
        Ok(inserted)
    }

    pub fn assets_written(&self) -> usize {
        self.written.load(Ordering::Relaxed)
    }

    // ---- Riwayat scan ----

    /// Buat baris scan baru (status "running"), kembalikan id-nya.
    pub async fn create_scan(&self, seed: &str, scope: &str) -> Result<i64> {
        let res = sqlx::query(
            "INSERT INTO scans (seed, scope, status) VALUES (?1, ?2, 'running')",
        )
        .bind(seed)
        .bind(scope)
        .execute(&self.pool)
        .await
        .context("gagal membuat scan")?;
        Ok(res.last_insert_rowid())
    }

    /// Tutup scan dengan status akhir + statistik.
    #[allow(clippy::too_many_arguments)]
    pub async fn finish_scan(
        &self,
        id: i64,
        status: &str,
        assets_count: i64,
        graph_nodes: i64,
        graph_edges: i64,
        dot: Option<&str>,
        graph_json: Option<&str>,
        error: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE scans SET status=?1, finished_at=datetime('now'),
                assets_count=?2, graph_nodes=?3, graph_edges=?4, dot=?5,
                graph_json=?6, error=?7
             WHERE id=?8",
        )
        .bind(status)
        .bind(assets_count)
        .bind(graph_nodes)
        .bind(graph_edges)
        .bind(dot)
        .bind(graph_json)
        .bind(error)
        .bind(id)
        .execute(&self.pool)
        .await
        .context("gagal menutup scan")?;
        Ok(())
    }

    /// Ambil graf JSON (D3) untuk sebuah scan.
    pub async fn scan_graph_json(&self, id: i64) -> Result<Option<String>> {
        let row = sqlx::query("SELECT graph_json FROM scans WHERE id=?1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .context("gagal membaca graph_json")?;
        Ok(row.and_then(|r| r.get::<Option<String>, _>("graph_json")))
    }

    pub async fn list_scans(&self) -> Result<Vec<ScanRow>> {
        let rows = sqlx::query(
            "SELECT id, seed, scope, status, started_at, finished_at,
                    assets_count, graph_nodes, graph_edges, NULL as dot, error
             FROM scans ORDER BY id DESC LIMIT 100",
        )
        .fetch_all(&self.pool)
        .await
        .context("gagal membaca riwayat scan")?;
        Ok(rows.iter().map(map_scan).collect())
    }

    pub async fn get_scan(&self, id: i64) -> Result<Option<ScanRow>> {
        let row = sqlx::query(
            "SELECT id, seed, scope, status, started_at, finished_at,
                    assets_count, graph_nodes, graph_edges, dot, error
             FROM scans WHERE id=?1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .context("gagal membaca scan")?;
        Ok(row.as_ref().map(map_scan))
    }

    pub async fn assets_by_scan(&self, scan_id: i64) -> Result<Vec<AssetRow>> {
        let rows = sqlx::query(
            "SELECT kind, url, origin, notes, found_at FROM assets
             WHERE scan_id=?1 ORDER BY found_at",
        )
        .bind(scan_id)
        .fetch_all(&self.pool)
        .await
        .context("gagal membaca temuan scan")?;
        Ok(rows
            .iter()
            .map(|r| AssetRow {
                kind: r.get("kind"),
                url: r.get("url"),
                origin: r.get("origin"),
                notes: r.get("notes"),
                found_at: r.get("found_at"),
            })
            .collect())
    }

    // ---- Settings (config editable dari dashboard) ----

    pub async fn settings_get(&self, key: &str) -> Result<Option<String>> {
        let row = sqlx::query("SELECT value FROM settings WHERE key=?1")
            .bind(key)
            .fetch_optional(&self.pool)
            .await
            .context("gagal membaca settings")?;
        Ok(row.map(|r| r.get::<String, _>("value")))
    }

    pub async fn settings_set(&self, key: &str, value: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        )
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await
        .context("gagal menyimpan settings")?;
        Ok(())
    }
}

fn map_scan(r: &sqlx::sqlite::SqliteRow) -> ScanRow {
    ScanRow {
        id: r.get("id"),
        seed: r.get("seed"),
        scope: r.get("scope"),
        status: r.get("status"),
        started_at: r.get("started_at"),
        finished_at: r.get("finished_at"),
        assets_count: r.get("assets_count"),
        graph_nodes: r.get("graph_nodes"),
        graph_edges: r.get("graph_edges"),
        dot: r.get("dot"),
        error: r.get("error"),
    }
}
