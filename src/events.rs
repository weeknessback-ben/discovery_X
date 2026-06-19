//! Event yang dialirkan orchestrator → dashboard web (via SSE).
//!
//! Sebelumnya `UiEvent` di `ui.rs` (TUI). Kini di-serialize ke JSON untuk
//! dikirim ke browser lewat Server-Sent Events.

use serde::Serialize;

use crate::types::Asset;

/// Event progres scan. `type` membedakan varian saat di-serialize ke JSON.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ScanEvent {
    /// Aset baru tercatat.
    Asset { asset: Asset },
    /// Jumlah unit kerja in-flight saat ini.
    InFlight { count: i64 },
    /// AI mengusulkan sejumlah probe dari sebuah file sumber.
    AiProposal { source: String, count: usize },
    /// Statistik attack graph terkini.
    Graph { nodes: usize, edges: usize },
    /// Baris log untuk panel log.
    Log { line: String },
    /// Discovery selesai.
    Finished,
}
