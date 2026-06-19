//! Client chat-completions OpenAI-compatible (dipakai untuk GLM-5.2).
//!
//! `base_url` adalah URL lengkap endpoint chat-completions, mis.
//! `https://opencode.ai/zen/go/v1/chat/completions`.

use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use super::contract::{self, AIActionPlan};
use super::AiBrain;
use crate::types::AiInput;

pub struct OpenAiCompatBrain {
    client: reqwest::Client,
    base_url: String,
    model: String,
    api_key: Option<String>,
    temperature: f32,
    max_retries: u32,
}

impl OpenAiCompatBrain {
    pub fn new(
        base_url: String,
        model: String,
        api_key: Option<String>,
        temperature: f32,
        max_retries: u32,
        timeout_secs: u64,
    ) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .build()
            .context("gagal membangun HTTP client untuk AI")?;
        Ok(Self {
            client,
            base_url,
            model,
            api_key,
            temperature,
            max_retries,
        })
    }

    /// Satu panggilan chat-completions; kembalikan teks `choices[0].message.content`.
    async fn chat(&self, messages: &[ChatMessage]) -> Result<String> {
        let body = ChatRequest {
            model: &self.model,
            messages,
            temperature: self.temperature,
            stream: false,
        };
        let mut req = self.client.post(&self.base_url).json(&body);
        if let Some(key) = &self.api_key {
            req = req.bearer_auth(key);
        }
        let resp = req.send().await.context("permintaan ke endpoint AI gagal")?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(anyhow!("endpoint AI mengembalikan {status}: {text}"));
        }
        let parsed: ChatResponse =
            serde_json::from_str(&text).context("respons AI bukan JSON chat-completions yang valid")?;
        parsed
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .ok_or_else(|| anyhow!("respons AI tidak memuat choices"))
    }
}

#[async_trait]
impl AiBrain for OpenAiCompatBrain {
    async fn analyze(&self, input: &AiInput) -> Result<AIActionPlan> {
        let mut messages = vec![
            ChatMessage::system(contract::system_prompt()),
            ChatMessage::user(contract::user_prompt(
                &input.source_url,
                &input.candidates,
                &input.tech,
            )),
        ];

        // Loop validasi: bila JSON gagal di-parse, kirim prompt koreksi & ulangi.
        let mut attempt = 0u32;
        loop {
            let raw = self.chat(&messages).await?;
            match contract::parse_plan(&raw) {
                Ok(plan) => return Ok(plan),
                Err(e) => {
                    if attempt >= self.max_retries {
                        return Err(anyhow!(
                            "AI gagal menghasilkan AIActionPlan valid setelah {} percobaan: {e}",
                            attempt + 1
                        ));
                    }
                    warn!(error = %e, attempt, "output AI tidak valid, mengirim koreksi");
                    messages.push(ChatMessage::assistant(raw));
                    messages.push(ChatMessage::user(contract::correction_prompt(&e.to_string())));
                    attempt += 1;
                    debug!(attempt, "retry analisis AI");
                }
            }
        }
    }
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: &'a [ChatMessage],
    temperature: f32,
    stream: bool,
}

#[derive(Serialize, Clone)]
pub struct ChatMessage {
    role: &'static str,
    content: String,
}

impl ChatMessage {
    fn system(content: String) -> Self {
        Self { role: "system", content }
    }
    fn user(content: String) -> Self {
        Self { role: "user", content }
    }
    fn assistant(content: String) -> Self {
        Self { role: "assistant", content }
    }
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: RespMessage,
}

#[derive(Deserialize)]
struct RespMessage {
    #[serde(default)]
    content: String,
}
