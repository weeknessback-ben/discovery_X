import { useState } from "react";
import { api, setCsrf } from "../api";

export default function Login({ onLogin }: { onLogin: () => void }) {
  const [u, setU] = useState("admin");
  const [p, setP] = useState("");
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setErr("");
    setBusy(true);
    try {
      const res = await api.login(u, p);
      if (res.ok) {
        const j = await res.json();
        setCsrf(j.csrf);
        onLogin();
      } else {
        const j = await res.json().catch(() => ({}));
        setErr(j.error || "login gagal");
      }
    } catch {
      setErr("tidak bisa menghubungi server");
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="center">
      <form className="card login" onSubmit={submit}>
        <h1>🛰 discovery_X</h1>
        <p className="muted">Masuk untuk mengelola scan</p>
        <label>
          Username
          <input value={u} onChange={(e) => setU(e.target.value)} autoFocus />
        </label>
        <label>
          Password
          <input type="password" value={p} onChange={(e) => setP(e.target.value)} />
        </label>
        {err && <div className="error">{err}</div>}
        <button type="submit" disabled={busy}>
          {busy ? "…" : "Login"}
        </button>
      </form>
    </div>
  );
}
