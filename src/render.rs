//! Headless rendering (chromiumoxide) untuk meng-crawl SPA yang butuh eksekusi JS.

use std::time::Duration;

use anyhow::{anyhow, Result};
use chromiumoxide::{Browser, BrowserConfig};
use futures::StreamExt;
use tokio::sync::Semaphore;
use tracing::{debug, warn};

/// Pengelola instance browser headless (dipakai bersama lintas worker).
pub struct Renderer {
    browser: Browser,
    /// Batasi jumlah tab/render serempak (rendering itu mahal).
    sem: Semaphore,
    timeout: Duration,
    wait: Duration,
}

impl Renderer {
    /// Luncurkan satu browser headless. Mengembalikan error bila Chrome tak tersedia.
    pub async fn launch(max_concurrent: usize, timeout_secs: u64, wait_ms: u64) -> Result<Self> {
        let config = BrowserConfig::builder()
            // Argumen wajib agar jalan sebagai root / di container (mis. Kali).
            .arg("--no-sandbox")
            .arg("--disable-gpu")
            .arg("--disable-dev-shm-usage")
            .build()
            .map_err(|e| anyhow!("konfigurasi browser gagal: {e}"))?;

        let (browser, mut handler) = Browser::launch(config)
            .await
            .map_err(|e| anyhow!("gagal meluncurkan Chrome headless: {e}"))?;

        // Handler harus terus dipoll (abaikan error transien) agar koneksi CDP hidup
        // sampai browser benar-benar ditutup (stream berakhir / None).
        tokio::spawn(async move {
            while handler.next().await.is_some() {}
        });

        Ok(Self {
            browser,
            sem: Semaphore::new(max_concurrent.max(1)),
            timeout: Duration::from_secs(timeout_secs.max(1)),
            wait: Duration::from_millis(wait_ms),
        })
    }

    /// Render `url` dan kembalikan HTML setelah JS berjalan. `None` bila gagal/timeout.
    pub async fn render(&self, url: &str) -> Option<String> {
        let _permit = self.sem.acquire().await.ok()?;
        let url = url.to_string();
        let wait = self.wait;
        let fut = async {
            let page = self
                .browser
                .new_page(url.as_str())
                .await
                .map_err(|e| anyhow!("new_page: {e}"))?;
            // Beri waktu framework SPA me-render DOM.
            tokio::time::sleep(wait).await;
            let html = page.content().await.map_err(|e| anyhow!("content: {e}"))?;
            let _ = page.close().await;
            Ok::<String, anyhow::Error>(html)
        };

        match tokio::time::timeout(self.timeout, fut).await {
            Ok(Ok(html)) => {
                debug!(%url, bytes = html.len(), "render selesai");
                Some(html)
            }
            Ok(Err(e)) => {
                warn!(%url, error = %e, "render gagal");
                None
            }
            Err(_) => {
                warn!(%url, "render timeout");
                None
            }
        }
    }
}
