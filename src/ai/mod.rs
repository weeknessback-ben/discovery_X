//! AI brain: trait abstrak + implementasi OpenAI-compatible (GLM-5.2).

pub mod contract;
pub mod openai_compat;

use anyhow::Result;
use async_trait::async_trait;

use crate::types::AiInput;
use contract::AIActionPlan;

/// Antarmuka AI brain. Implementasi bertanggung jawab atas loop validasi/retry.
#[async_trait]
pub trait AiBrain: Send + Sync {
    /// Analisis input dan kembalikan rencana aksi tervalidasi.
    async fn analyze(&self, input: &AiInput) -> Result<AIActionPlan>;
}
