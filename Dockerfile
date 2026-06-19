# ── Stage 1: build frontend React/Vite (di-embed ke binary via rust-embed) ──
FROM node:22-slim AS frontend
WORKDIR /app/frontend
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci
COPY frontend/ ./
RUN npm run build      # → /app/frontend/dist

# ── Stage 2: build binary Rust (release) ───────────────────────────────────
FROM rust:1-bookworm AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
# rust-embed butuh frontend/dist ADA saat compile — ambil dari stage frontend.
COPY --from=frontend /app/frontend/dist ./frontend/dist
RUN cargo build --release

# ── Stage 3: runtime ramping (Chromium utk render SPA, Graphviz utk attack graph) ──
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
        chromium graphviz ca-certificates fonts-liberation tini \
    && rm -rf /var/lib/apt/lists/*

# chromiumoxide menemukan browser via env CHROME.
ENV CHROME=/usr/bin/chromium
# Di container, bind ke semua interface; AMANKAN dengan pemetaan port host
# 127.0.0.1 dan/atau reverse-proxy TLS (lihat docker-compose.yml & README).
ENV DISCOVERY_BIND=0.0.0.0:7373

# Jalankan sebagai user non-root.
RUN useradd --create-home --uid 10001 app && mkdir -p /data && chown app:app /data
COPY --from=builder /app/target/release/discovery_x /usr/local/bin/discovery_x

USER app
WORKDIR /data
EXPOSE 7373

# tini sebagai PID 1 → reap proses zombie Chromium.
ENTRYPOINT ["tini", "--", "discovery_x"]
