# discovery_X — Dokumen Desain

> Catatan desain & latar belakang arsitektur untuk **discovery_X** (sebelumnya `discovery-agent`).
> Implementasi & cara pakai: lihat [`README.md`](./README.md).

---

Membuat *tools* *discovery* dan *pentesting* otonom menggunakan **Rust** adalah pilihan yang sangat brilian. Rust memberikan keunggulan yang tidak dimiliki Python atau Go dalam konteks ini: **Fearless Concurrency** (untuk *scanning* masif tanpa *thread exhaustion*), **Memory Safety** (saat mem-*parse* respons jaringan yang tidak terpercaya/malformed), dan **Single Binary Deployment** (mudah didistribusikan ke VPS atau *drop* ke mesin target tanpa instalasi *environment*).

Tantangan utama menggunakan Rust untuk AI adalah ekosistem *Machine Learning*-nya tidak sematang Python. Oleh karena itu, arsitekturnya harus memisahkan **Core Engine (Rust)** yang menangani I/O dan konkurensi, dengan **AI Brain** yang bisa berupa *local inference* (via bindings) atau *external API*.

Berikut adalah rancangan Arsitektur, Algoritma, dan Sisi Improvement khusus untuk *Rust-based Autonomous Discovery Agent*.

---

### 🏛️ 1. Arsitektur: "Rust-Native Asynchronous Agent"

Arsitektur ini menggunakan model *Actor-based* atau *Task-Queue* asinkron menggunakan `tokio`. AI tidak dipanggil untuk setiap *request* (itu akan lambat dan mahal), melainkan digunakan sebagai **Planner** dan **Analyzer** untuk data yang telah dikumpulkan oleh *workers* Rust.

```text
┌─────────────────────────────────────────────────────────────────────────┐
│                     RUST AUTONOMOUS PENTEST AGENT                       │
│  [ TUI Dashboard (Ratatui) ]  <---->  [ Orchestrator (Tokio Runtime) ]   │
└──────────────────────────────┬──────────────────────────────────────────┘
                               │
        ┌──────────────────────┼──────────────────────┐
        ▼                      ▼                      ▼
┌───────────────┐    ┌─────────────────┐    ┌──────────────────┐
│ 1. RECON WORKERS│    │ 2. DISCOVERY    │    │ 3. AI BRAIN      │
│ (Async I/O)   │    │ WORKERS (CPU)   │    │ (LLM Interface)  │
│ - Subdomain   │    │ - JS Parser     │    │ - Local (llama.cpp)│
│ - Port Scan   │    │ - HTML/DOM Scraper│   │ - API (OpenAI/Ollama)│
│ - Fingerprint │    │ - Param Fuzzer  │    │ - Structured JSON│
└───────┬───────┘    └────────┬────────┘    └────────┬─────────┘
        │ Raw Data            │ Parsed Assets        │ Action Plans
        ▼                     ▼                      ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                     STATE & GRAPH STORE                                 │
│  ├─ SQLite / Sled (KV Store untuk Antrian & Cache)                      │
│  └─ Neo4j / Petgraph (In-Memory Attack Graph untuk Relasi Aset)         │
└─────────────────────────────────────────────────────────────────────────┘
```

---

### ⚙️ 2. Algoritma: "AI-Guided Deep Discovery Loop"

Algoritma ini berfokus pada **Discovery** (menemukan *hidden assets*, API tersembunyi, dan parameter). AI digunakan untuk menganalisis konteks (misal: membaca file JavaScript atau HTML) dan menebak *endpoint* atau parameter yang tidak terlihat oleh *crawler* biasa.

#### Siklus Kerja (Pseudocode Rust)

```rust
use tokio::sync::mpsc;
use serde::{Deserialize, Serialize};

// 1. Definisi Kontrak AI yang Ketat (Mencegah Halusinasi)
#[derive(Deserialize, Debug)]
struct AIActionPlan {
    target_url: String,
    discovery_type: DiscoveryType, // Enum: JsAnalysis, ParamGuessing, DirBusting
    payloads: Vec<String>,
    reasoning: String,
}

#[tokio::main]
async fn main() {
    let (task_tx, mut task_rx) = mpsc::channel::<Task>(1000);
    let (result_tx, mut result_rx) = mpsc::channel::<DiscoveryResult>(1000);

    // Spawn Workers (Rust menangani ribuan koneksi konkuren dengan mudah)
    spawn_recon_workers(task_tx.clone()).await;
    spawn_crawl_workers(task_tx.clone()).await;

    // Seed Target Awal
    task_tx.send(Task::Seed("https://target.com")).await.unwrap();

    // Main Event Loop
    loop {
        tokio::select! {
            // A. Terima Hasil dari Workers
            Some(result) = result_rx.recv() => {
                // Simpan ke State/Graph
                state_store.insert(result.asset.clone());
                
                // B. Jika hasil berupa File JS atau HTML Kompleks, kirim ke AI
                if result.requires_ai_analysis() {
                    let plan = ai_brain.request_analysis(&result).await;
                    
                    // C. Validasi Output AI (Keunggulan Rust: Serde)
                    match plan {
                        Ok(action_plan) => {
                            // Ubah rencana AI menjadi Task baru untuk Workers
                            for payload in action_plan.payloads {
                                task_tx.send(Task::Probe(action_plan.target_url, payload)).await;
                            }
                        }
                        Err(e) => {
                            // AI berhalusinasi/JSON rusak. Rust menolaknya, minta AI retry.
                            ai_brain.retry_with_correction(&result, e).await;
                        }
                    }
                }
            }
            
            // D. Handle Task Baru untuk Workers
            Some(task) = task_rx.recv() => {
                // Distribusikan ke thread pool asinkron
                tokio::spawn(execute_task(task, result_tx.clone()));
            }
        }
    }
}
```

---

### 🚀 3. Sisi Improvement & "The Rust Advantage"

Membuat agen AI di Rust memberikan *improvement* teknis yang masif dibandingkan Python, terutama dalam menangani **Ketidakpastian AI** dan **Skala Jaringan**.

#### A. "Self-Healing" AI Contracts dengan `serde` (Anti-Halusinasi)
*   **Masalah di Python:** LLM sering mengembalikan JSON yang *malformed* atau key yang salah. Di Python, ini menyebabkan `KeyError` atau `NoneType` yang membuat *crash* agen atau menghasilkan *false positive*.
*   **Improvement Rust:** Gunakan `serde_json::from_str::<AIActionPlan>`. Jika LLM berhalusinasi dan membuat JSON yang tidak sesuai dengan *struct* Rust, **kompilator/runner akan langsung menolak (Error)**.
*   **Mekanisme:** Agen Rust secara otomatis menangkap error deserialisasi ini, lalu mengirimkan *prompt* koreksi ke LLM: *"JSON Anda tidak valid. Field 'payloads' harus berupa array string. Perbaiki dan kirim ulang."* Ini menciptakan *loop* yang sangat stabil.

#### B. Deep JS & DOM Analysis untuk Hidden Discovery
*   **Masalah Umum:** *Crawler* biasa hanya mengikuti tag `<a href>`. Mereka melewatkan API yang dipanggil via `fetch()` di dalam file JavaScript yang di-*minify*.
*   **Improvement Rust:** Gunakan *crate* seperti `swc_ecma_parser` atau `boa_engine` (JavaScript engine di Rust) untuk mem-*parse* file `.js` secara statis.
*   **Peran AI:** Ekstrak string URL, route, dan variabel dari AST (Abstract Syntax Tree) JS tersebut, lalu kirim *batch* string ini ke LLM. LLM bertugas menyaring mana yang merupakan *hidden API endpoint* (misal: `/api/v1/internal/admin_reset`) dan mana yang hanya *library path*.

#### C. Konkurensi Masif dengan `tokio` & `Semaphore`
*   **Masalah Umum:** Agen AI sering kali membanjiri target (DDoS tidak sengaja) atau kehabisan *file descriptors* karena membuka terlalu banyak koneksi HTTP saat *discovery*.
*   **Improvement Rust:** Gunakan `tokio::sync::Semaphore` untuk membatasi konkurensi secara global atau per-domain.
    ```rust
    let permit = semaphore.acquire().await.unwrap();
    let resp = reqwest::get(url).await?;
    drop(permit); // Otomatis melepas slot
    ```
    Ini memungkinkan agen melakukan *scanning* 10.000 port atau *fuzzing* parameter secara asinkron dengan penggunaan memori di bawah 50MB.

#### D. Local AI Inference (Zero-Latency & Privasi)
*   **Masalah Umum:** Mengirimkan *source code* atau respons HTTP sensitif ke API OpenAI/Anthropic berisiko membocorkan data klien (NDA violation) dan terkena *rate-limit*.
*   **Improvement Rust:** Integrasikan **`llama-cpp-rs`** atau **`candle`** (HuggingFace's Rust ML framework).
*   **Mekanisme:** *Load* model kecil yang dioptimalkan untuk *coding/reasoning* (seperti `Qwen2.5-Coder-7B-Instruct` atau `Llama-3-8B`) langsung ke dalam memori Rust. Agen dapat menganalisis file JS atau menebak parameter secara lokal dengan latensi milidetik, tanpa pernah mengirim data keluar dari mesin pentester.

#### E. Stateful Fuzzing Berbasis Graf (`petgraph`)
*   **Improvement:** Daripada hanya menyimpan URL di database, gunakan *crate* `petgraph` untuk membuat **In-Memory Attack Graph**.
*   **Mekanisme:**
    *   Node: `Page`, `API Endpoint`, `Parameter`, `JS File`.
    *   Edge: `Contains`, `Calls`, `RequiresAuth`.
    *   Saat AI merencanakan *discovery*, ia melakukan *query* pada Graf: *"Temukan semua Node `API Endpoint` yang terhubung dengan Node `JS File` yang memiliki kata 'admin', tetapi belum memiliki Edge `Tested`."* Ini membuat *discovery* sangat terarah dan tidak berulang.

---

### 🛠️ Rekomendasi Tech Stack (Crates Rust)

Untuk membangun *tools* ini, berikut adalah *crates* (library) standar industri yang wajib Anda gunakan:

| Kategori | Crate Rust | Fungsi |
| :--- | :--- | :--- |
| **Async Runtime** | `tokio` | Menangani ribuan *task* jaringan secara konkuren. |
| **HTTP Client** | `reqwest` (dengan `rustls`) | Melakukan *request* HTTP/HTTPS, menangani *cookies* & *redirects*. |
| **DNS Resolver** | `hickory-resolver` | *DNS lookup* asinkron yang sangat cepat untuk *subdomain discovery*. |
| **HTML Parsing** | `scraper` atau `html5ever` | Mem-*parse* DOM HTML untuk ekstraksi form dan link. |
| **JS Parsing** | `swc_ecma_parser` | Membedah file JavaScript untuk menemukan *hidden routes*. |
| **AI / LLM** | `async-openai`, `llama-cpp-rs` | Integrasi dengan API LLM atau *inference* model lokal. |
| **Data Store** | `sqlx` (SQLite) atau `sled` | Menyimpan antrian *task* dan hasil *discovery* secara lokal. |
| **Graph** | `petgraph` | Membangun *Attack Graph* di dalam memori (RAM). |
| **Terminal UI** | `ratatui` | Membuat dashboard *hacker* yang keren di terminal (TUI) untuk memonitor agen. |
| **Binary Parsing** | `nom` | Jika agen Anda juga melakukan *discovery* pada protokol biner/kustom. |

### Kesimpulan Strategi
Dengan menggunakan Rust, Anda tidak hanya membuat "skrip pentest yang diberi AI", tetapi Anda membangun **Sistem Operasi Discovery** yang tangguh. Keunggulan utamanya ada pada **Disiplin Tipe Data (Strict Typing)** yang memaksa AI untuk bekerja dalam batasan yang aman, dan **Performa Asinkron** yang memungkinkan agen menjelajah seluruh *attack surface* target dalam hitungan menit, bukan jam.
