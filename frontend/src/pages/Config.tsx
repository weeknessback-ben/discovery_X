import { useEffect, useState } from "react";
import { api } from "../api";

const TOGGLES: [string, string][] = [
  ["enable_feeds", "robots.txt + sitemap.xml"],
  ["enable_dirbust", "fingerprint + dirbust"],
  ["enable_subdomain_enum", "enumerasi subdomain"],
  ["enable_render", "render JS (SPA, butuh Chrome)"],
  ["verify_live", "verifikasi endpoint hidup via headless (deteksi soft-404)"],
];

export default function Config() {
  const [cfg, setCfg] = useState<any>(null);
  const [key, setKey] = useState("");
  const [msg, setMsg] = useState("");

  const load = () => api.getConfig().then(setCfg).catch(() => setMsg("gagal memuat"));
  useEffect(() => {
    load();
  }, []);

  if (!cfg) return <div className="card muted">memuat…</div>;
  const r = cfg.recon;
  const a = cfg.ai;
  const setR = (k: string, v: any) => setCfg({ ...cfg, recon: { ...cfg.recon, [k]: v } });
  const setA = (k: string, v: any) => setCfg({ ...cfg, ai: { ...cfg.ai, [k]: v } });

  const save = async () => {
    setMsg("");
    const body = {
      recon: cfg.recon,
      ai: {
        base_url: a.base_url,
        model: a.model,
        enabled: a.enabled,
        temperature: a.temperature,
        max_retries: a.max_retries,
        timeout_secs: a.timeout_secs,
        api_key: key || undefined,
      },
    };
    const res = await api.putConfig(body);
    if (res.ok) {
      setMsg("tersimpan ✓");
      setKey("");
      load();
    } else {
      setMsg("gagal menyimpan");
    }
  };

  return (
    <div className="card config">
      <h2>Konfigurasi AI</h2>
      <label className="row">
        <input type="checkbox" checked={a.enabled} onChange={(e) => setA("enabled", e.target.checked)} />
        &nbsp;Aktifkan analisis AI
      </label>
      <label>
        Base URL
        <input value={a.base_url} onChange={(e) => setA("base_url", e.target.value)} />
      </label>
      <label>
        Model
        <input value={a.model} onChange={(e) => setA("model", e.target.value)} />
      </label>
      <label>
        API Key {a.key_set && <span className="muted">(tersimpan {a.key_hint})</span>}
        <input
          type="password"
          value={key}
          onChange={(e) => setKey(e.target.value)}
          placeholder={a.key_set ? "•••• (kosongkan untuk tetap)" : "masukkan API key"}
        />
      </label>
      <div className="row3">
        <label>
          Temperature
          <input type="number" step="0.1" value={a.temperature} onChange={(e) => setA("temperature", parseFloat(e.target.value))} />
        </label>
        <label>
          Max retries
          <input type="number" value={a.max_retries} onChange={(e) => setA("max_retries", parseInt(e.target.value))} />
        </label>
        <label>
          Timeout (s)
          <input type="number" value={a.timeout_secs} onChange={(e) => setA("timeout_secs", parseInt(e.target.value))} />
        </label>
      </div>

      <h2>Konfigurasi Recon</h2>
      <div className="row3">
        <label>
          Max depth
          <input type="number" value={r.max_depth} onChange={(e) => setR("max_depth", parseInt(e.target.value))} />
        </label>
        <label>
          Global concurrency
          <input type="number" value={r.global_concurrency} onChange={(e) => setR("global_concurrency", parseInt(e.target.value))} />
        </label>
        <label>
          Per-domain
          <input type="number" value={r.per_domain_concurrency} onChange={(e) => setR("per_domain_concurrency", parseInt(e.target.value))} />
        </label>
      </div>
      <div className="toggles">
        {TOGGLES.map(([k, lbl]) => (
          <label key={k} className="row">
            <input type="checkbox" checked={!!r[k]} onChange={(e) => setR(k, e.target.checked)} />
            &nbsp;{lbl}
          </label>
        ))}
      </div>

      <div className="row">
        <button onClick={save}>Simpan</button>
        {msg && <span className="muted">{msg}</span>}
      </div>
    </div>
  );
}
