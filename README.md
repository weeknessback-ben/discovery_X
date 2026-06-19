<h1 align="center">discovery_X</h1>

<p align="center">
  <img src="assets/banner.svg" alt="discovery_X — autonomous discovery/recon agent: Target → Crawl+JS → Fingerprint → AI brain → Attack graph" width="100%">
</p>

**Autonomous discovery/recon agent for authorized pentesting**, driven entirely from a web
dashboard. It maps a target's attack surface and uses AI to infer hidden endpoints — behind
strict scope guardrails.

**Core capabilities:**
- 🔎 **Find hidden endpoints** — crawl HTML + analyze JavaScript (external & inline) to surface
  unlinked paths/endpoints; works **with or without** AI.
- 🧬 **Fingerprint stack + version** (WordPress, Next.js, Laravel, nginx, …) then run **targeted
  dirbusting** against each technology's well-known paths.
- ✅ **Liveness verification** (*soft-404* detection) & **SPA rendering** via a headless browser.
- 🤖 **Multi-provider AI brain** — any OpenAI-compatible endpoint, or the **LiteLLM** proxy for
  OpenAI / Anthropic / Gemini / Ollama / etc.
- 🕸️ **Interactive attack graph** — map asset relationships; AI-inferred endpoints are highlighted.
- 🖥️ **Web dashboard (OWASP-hardened)** — every finding shows **stack + version**,
  **HTTP status code**, server, title, and live/dead status. Scan history is persisted.
- 🐳 **One-command deploy** with Docker (Chromium + Graphviz already included).

> ⚠️ **Only for targets you are authorized to test.** Every scan requires a scope allowlist
> plus an explicit authorization checkbox; seeds outside scope are rejected.
> Unauthorized use is illegal.

Design & architecture background: see [`discovery.md`](./discovery.md).

---

## Quick start with Docker (recommended)

No need to install Rust/Node/Chromium on the host — everything is inside the image.

```bash
# 1. Create the admin password hash (the server refuses to start without it).
docker compose run --rm discovery_x hash-password
#    → type a password, copy the "$argon2id$..." hash line

# 2. Save credentials in a .env file next to docker-compose.yml.
#    IMPORTANT: wrap the hash in SINGLE quotes — Compose interpolates '$'
#    when unquoted, which would corrupt the hash.
cat > .env <<'EOF'
DISCOVERY_ADMIN_USER=admin
DISCOVERY_ADMIN_PASSWORD_HASH='$argon2id$v=19$m=19456,t=2,p=1$...'   # from step 1
AGENT_AI_API_KEY=sk-...                                              # optional → AI brain
EOF

# 3. Build & run.
docker compose up -d --build

# 4. Open http://127.0.0.1:7373 → log in.
```

The port is mapped to `127.0.0.1` only (not exposed to the LAN). Findings + history are stored
in the `dxdata` volume. Chromium (SPA rendering) & Graphviz (attack graph) are bundled in the
image. For remote access, put it behind a **TLS reverse proxy**.

---

## Manual build (without Docker)

Requires: Rust (≥1.81), Node 20+, plus optional `chromium` (SPA rendering) & `graphviz`
(attack graph). The frontend (React/Vite) is embedded into the binary, so **build it first**:

```bash
cd frontend && npm install && npm run build && cd ..   # produces frontend/dist
cargo build --release                                   # binary: target/release/discovery_x
cargo test                                              # unit tests (scope, AI contract, parser)
```

Configure & run:

```bash
cp config.example.toml config.toml                      # then edit as needed
./target/release/discovery_x hash-password              # print an Argon2id admin hash
#   → put it in config.toml [server].admin_password_hash, or:
#   export DISCOVERY_ADMIN_PASSWORD_HASH='$argon2id$...'
./target/release/discovery_x --config config.toml
#   → open http://127.0.0.1:7373 → log in
```

---

## Using the dashboard

- **Config** — tune recon (depth, feeds, dirbust, render, verify-live, …) + the **AI API key**
  (stored in SQLite, shown masked; without a key → recon-only mode).
- **Dashboard** — enter a *seed* + *scope* (allowlist, one host/IP per line) and tick the
  authorization box → **Start**. Watch live stats/findings/logs (via SSE).
- **Finding details** — each asset gets badges:
  - detected **stack/technology** with its **version** (e.g. `WordPress 6.2`, `nginx 1.18`);
  - **HTTP status code** colored per class (2xx green, 3xx blue, 4xx amber, 5xx red);
  - **server** header, page **title**, an **SPA** marker, and **liveness** (`● live`/`○ dead?`).
  - Above the list there's a summary of **detected stacks** + **HTTP status distribution**.
- **Interactive attack graph (D3)** — the **"View attack graph →"** button opens a
  force-directed graph: nodes colored per asset type, hubs larger, `calls` edges (AI-inferred
  endpoints) highlighted; drag/zoom, click a node to open its URL. Export as **SVG** (Graphviz)
  & download **DOT** are also available.

Findings are stored in SQLite (`discovery.db`): tables `assets` (per-scan), `scans` (history),
`settings` (config). Query manually:
```bash
sqlite3 discovery.db 'SELECT kind, url, origin, notes FROM assets WHERE scan_id=1'
```

### Dashboard security (OWASP)
**Argon2id** passwords; CSPRNG session tokens (cookie `HttpOnly; SameSite=Strict`); **CSRF**
token on every mutation; per-IP login **rate-limit/lockout**; security headers (CSP `self`,
`X-Frame-Options: DENY`, `nosniff`, `no-referrer`); the API key is **never sent back**
(only its status + last 4 chars). Default **binds to localhost** — for remote access use a
TLS reverse proxy.

---

## AI brain & many providers (LiteLLM)

The AI brain speaks the **OpenAI-compatible chat-completions** format, so any provider that
talks it works out of the box — just set **Base URL + Model + API key** (from the dashboard
**Config** page, `config.toml`, or env `AGENT_AI_BASE_URL` / `AGENT_AI_MODEL` /
`AGENT_AI_API_KEY`). Default: GLM-5.2.

To reach **many providers at once** (OpenAI, Anthropic/Claude, Gemini, Ollama, …) without any
code changes, use the **LiteLLM** proxy already wired into `docker-compose.yml` (the `litellm`
profile):

```bash
# 1. Prepare the model/provider list.
cp litellm.config.example.yaml litellm.config.yaml      # edit model_list as needed

# 2. Fill .env (provider keys never go into config files — read from env):
cat >> .env <<'EOF'
LITELLM_MASTER_KEY=sk-your-proxy-secret
OPENAI_API_KEY=sk-...
ANTHROPIC_API_KEY=sk-ant-...
GEMINI_API_KEY=...
# Point discovery_X at the proxy + pick a model alias from litellm.config.yaml:
AGENT_AI_BASE_URL=http://litellm:4000/v1/chat/completions
AGENT_AI_MODEL=claude
AGENT_AI_API_KEY=sk-your-proxy-secret
EOF

# 3. Run discovery_X + LiteLLM together.
docker compose --profile litellm up -d --build
```

LiteLLM routes a single endpoint to many providers based on the `model_name` (alias) in
`litellm.config.yaml`. Switching models is just changing `AGENT_AI_MODEL` to another alias.
The proxy is only exposed on Docker's internal network (not to the host).

---

## Architecture

```
main.rs → web server (axum, binds 127.0.0.1:7373)
   ├─ /api/login,/logout,/csrf   auth (Argon2id, cookie session, CSRF, login rate-limit)
   ├─ /api/config                recon + API key (stored in SQLite, key masked)
   ├─ /api/scan, /api/scans      start/stop + history (one active scan)
   ├─ /api/events  (SSE)         live progress → React dashboard
   └─ static (rust-embed)        React/Vite frontend (embedded into the binary)
        │ ScanManager.start
        ▼
   engine::run_scan → Orchestrator (tokio, mpsc + select!)
        ├─ http worker (reqwest) → crawl + jsparse (swc) + fingerprint/dirbust + feeds + render SPA
        ├─ dns worker  (hickory) → subdomain enumeration
        └─ AI brain   (GLM-5.2)  → AIActionPlan (serde-validated, retry-on-error)
State: SQLite (sqlx) — `assets` (per-scan) + `scans` (history) + `settings` (config)
Graph: petgraph attack graph → export to DOT (Graphviz) / JSON (D3)
```

**Attack graph** (`petgraph`): each asset is a node, each relation an edge
(`links`, `references`, `contains`, `calls`, `resolves`, `hosts`, `guessed`). The `calls` edge
represents an endpoint the AI inferred from a JS file — the most interesting "hidden assets".
Concurrency is bounded by a global **and** per-domain `Semaphore` (so we never flood a target);
every request is checked against the scope allowlist before it's sent.

Discovery coverage improvements:
- **Inline `<script>` parsing** + JS candidates probed directly (works without AI).
- **robots.txt + sitemap.xml** harvested automatically from the seed (`enable_feeds`).
- **Tech fingerprint + targeted dirbust** (`enable_dirbust`) — detect WordPress/Next.js/Laravel/
  etc. (with version) then try their well-known paths (`/wp-json/`, `/_next/`, `/telescope`, …).
  The detected stack is also **sent to the AI** so it suggests stack-specific paths.
- **Liveness verification** (`verify_live`) — discovered endpoints are checked via a headless
  browser to detect **soft-404s** (HTTP 200 that are actually error pages).
- **JS SPA rendering** via headless Chrome (`--render` / `enable_render`) — degrades gracefully
  when Chrome is unavailable.

---

## Security & license

- Vulnerability reports & authorized-use policy: see [`SECURITY.md`](./SECURITY.md).
- License: [MIT](./LICENSE).
