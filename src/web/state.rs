//! State global web server + session store in-memory + rate-limiter login.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rand::RngCore;

use crate::config::{Config, ServerConfig};
use crate::scanmanager::ScanManager;
use crate::state::StateStore;

/// State yang dibagikan ke semua handler axum.
pub struct AppState {
    pub state: Arc<StateStore>,
    pub scans: Arc<ScanManager>,
    pub server: ServerConfig,
    /// Config dasar (default + TOML) untuk di-overlay dengan settings DB.
    pub base_cfg: Config,
    pub sessions: Sessions,
    pub login_guard: LoginGuard,
}

pub type SharedState = Arc<AppState>;

/// 32 byte acak (CSPRNG) → hex. Untuk token sesi & CSRF.
fn random_token() -> String {
    let mut buf = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut buf);
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

struct Session {
    csrf: String,
    expires: Instant,
}

/// Penyimpanan sesi in-memory (single-user, lokal). Hilang saat restart.
#[derive(Default)]
pub struct Sessions {
    inner: Mutex<HashMap<String, Session>>,
    ttl: Duration,
}

impl Sessions {
    pub fn new(ttl_secs: u64) -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            ttl: Duration::from_secs(ttl_secs.max(60)),
        }
    }

    /// Buat sesi baru → (token, csrf). Token baru tiap login (anti fixation).
    pub fn create(&self) -> (String, String) {
        let token = random_token();
        let csrf = random_token();
        self.inner.lock().unwrap().insert(
            token.clone(),
            Session {
                csrf: csrf.clone(),
                expires: Instant::now() + self.ttl,
            },
        );
        (token, csrf)
    }

    /// Kembalikan csrf token bila sesi valid & belum kedaluwarsa.
    pub fn csrf_for(&self, token: &str) -> Option<String> {
        let mut g = self.inner.lock().unwrap();
        match g.get(token) {
            Some(s) if s.expires > Instant::now() => Some(s.csrf.clone()),
            Some(_) => {
                g.remove(token);
                None
            }
            None => None,
        }
    }

    pub fn remove(&self, token: &str) {
        self.inner.lock().unwrap().remove(token);
    }
}

struct Attempts {
    count: u32,
    locked_until: Option<Instant>,
}

/// Rate-limit / lockout login per-IP (A07).
pub struct LoginGuard {
    inner: Mutex<HashMap<IpAddr, Attempts>>,
    max_attempts: u32,
    lockout: Duration,
}

impl LoginGuard {
    pub fn new(max_attempts: u32, lockout_secs: u64) -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            max_attempts: max_attempts.max(1),
            lockout: Duration::from_secs(lockout_secs.max(1)),
        }
    }

    /// `Err(sisa_detik)` bila IP sedang terkunci.
    pub fn check(&self, ip: IpAddr) -> Result<(), u64> {
        let g = self.inner.lock().unwrap();
        if let Some(a) = g.get(&ip) {
            if let Some(until) = a.locked_until {
                let now = Instant::now();
                if until > now {
                    return Err((until - now).as_secs() + 1);
                }
            }
        }
        Ok(())
    }

    pub fn record_fail(&self, ip: IpAddr) {
        let mut g = self.inner.lock().unwrap();
        let a = g.entry(ip).or_insert(Attempts {
            count: 0,
            locked_until: None,
        });
        a.count += 1;
        if a.count >= self.max_attempts {
            a.locked_until = Some(Instant::now() + self.lockout);
            a.count = 0;
        }
    }

    pub fn reset(&self, ip: IpAddr) {
        self.inner.lock().unwrap().remove(&ip);
    }
}
