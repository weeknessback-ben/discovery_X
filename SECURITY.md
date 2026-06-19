# Security Policy

## Penggunaan yang sah (PENTING)

**discovery_X adalah alat untuk pengujian keamanan yang TERAUTORISASI saja.**
Gunakan hanya terhadap sistem yang Anda miliki atau yang secara tegas memberi Anda
izin tertulis untuk diuji (mis. cakupan engagement pentest atau program bug bounty).

- Setiap scan mewajibkan **scope allowlist** + konfirmasi otorisasi; seed di luar scope ditolak.
- Pemindaian tanpa izin dapat melanggar hukum (mis. CFAA, UU ITE, dan peraturan setempat).
- Pemilik proyek dan kontributor **tidak bertanggung jawab** atas penyalahgunaan.

Memakai alat ini berarti Anda menyatakan bertanggung jawab penuh dan telah mendapat
otorisasi yang sah atas target Anda.

## Versi yang didukung

Proyek masih pra-1.0; perbaikan keamanan diterapkan pada `main` (rilis terbaru).

| Versi | Didukung |
| ----- | -------- |
| `main` (terbaru) | ✅ |
| < terbaru | ❌ |

## Melaporkan kerentanan

Mohon **jangan** membuka issue publik untuk kerentanan keamanan.

Cara melapor (lebih disukai → alternatif):
1. **GitHub Security Advisory** — tab *Security → Report a vulnerability* di repo ini
   (laporan privat ke maintainer).
2. **Email** — bennitampubolon0@gmail.com dengan subjek `SECURITY: discovery_X`.

Mohon sertakan:
- deskripsi masalah & dampaknya,
- langkah reproduksi / proof-of-concept,
- versi/commit yang terpengaruh,
- saran mitigasi bila ada.

### Yang bisa Anda harapkan
- Konfirmasi penerimaan dalam **72 jam**.
- Penilaian awal & rencana penanganan dalam **7 hari**.
- Disclosure terkoordinasi: kami akan menyepakati jadwal publikasi setelah perbaikan tersedia,
  dan dengan senang hati memberi kredit kepada pelapor (kecuali Anda meminta anonim).

## Catatan keamanan bawaan

discovery_X dirancang dengan beberapa guardrail (lihat README):
- dashboard ber-autentikasi (Argon2id, sesi cookie `HttpOnly`/`SameSite=Strict`, token CSRF,
  rate-limit/lockout login, security headers),
- **bind localhost** secara default — untuk akses jarak jauh, gunakan reverse-proxy TLS,
- API key tidak pernah dikirim balik ke klien (hanya status + 4 karakter terakhir),
- file rahasia (`config.toml`, `.env`, `scope.txt`, `discovery.db`) dikecualikan dari git.

Jika Anda menemukan kelemahan pada salah satu guardrail di atas, itu termasuk dalam cakupan laporan.
