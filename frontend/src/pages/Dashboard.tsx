import { useEffect, useRef, useState } from "react";
import { Link } from "react-router-dom";
import { api } from "../api";
import { FindingRow, FindingsSummary } from "../findings";

type Status = {
  running: boolean;
  scan_id?: number;
  seed?: string;
  finished: boolean;
  in_flight: number;
  total: number;
  pages: number;
  js: number;
  forms: number;
  subdomains: number;
  endpoints: number;
  tech: number;
  ai_proposals: number;
  graph_nodes: number;
  graph_edges: number;
};

const KIND_FIELD: Record<string, keyof Status> = {
  page: "pages",
  js_file: "js",
  form: "forms",
  subdomain: "subdomains",
  endpoint: "endpoints",
  tech: "tech",
};

export default function Dashboard() {
  const [st, setSt] = useState<Status | null>(null);
  const [findings, setFindings] = useState<any[]>([]);
  const [logs, setLogs] = useState<string[]>([]);
  const [seed, setSeed] = useState("");
  const [scope, setScope] = useState("");
  const [auth, setAuth] = useState(false);
  const [msg, setMsg] = useState("");
  const esRef = useRef<EventSource | null>(null);

  const refreshStatus = () => api.status().then(setSt).catch(() => {});

  useEffect(() => {
    refreshStatus();
    const es = new EventSource("/api/events");
    esRef.current = es;
    es.onmessage = (e) => {
      const ev = JSON.parse(e.data);
      switch (ev.type) {
        case "asset":
          setFindings((f) => [ev.asset, ...f].slice(0, 300));
          setSt((s) => {
            if (!s) return s;
            const cp: any = { ...s, total: s.total + 1 };
            const fld = KIND_FIELD[ev.asset.kind];
            if (fld) cp[fld] = (s as any)[fld] + 1;
            return cp;
          });
          break;
        case "in_flight":
          setSt((s) => (s ? { ...s, in_flight: ev.count } : s));
          break;
        case "graph":
          setSt((s) => (s ? { ...s, graph_nodes: ev.nodes, graph_edges: ev.edges } : s));
          break;
        case "ai_proposal":
          setLogs((l) => [`AI: ${ev.count} probe dari ${ev.source}`, ...l].slice(0, 200));
          break;
        case "log":
          setLogs((l) => [ev.line, ...l].slice(0, 200));
          break;
        case "finished":
          setLogs((l) => ["— scan selesai —", ...l].slice(0, 200));
          refreshStatus();
          break;
      }
    };
    return () => es.close();
  }, []);

  const start = async () => {
    setMsg("");
    try {
      const res = await api.startScan({ seed, scope, authorized: auth });
      if (res.ok) {
        const j = await res.json();
        setFindings([]);
        setLogs([]);
        setMsg("scan dimulai #" + j.scan_id);
        refreshStatus();
      } else {
        const j = await res.json().catch(() => ({}));
        setMsg("gagal: " + (j.error || res.status));
      }
    } catch {
      setMsg("gagal menghubungi server");
    }
  };

  const stop = async () => {
    await api.stopScan().catch(() => {});
    setMsg("menghentikan…");
  };

  const s = st;
  const statusLabel = s?.running ? "BERJALAN" : s?.finished ? "SELESAI" : "idle";

  return (
    <div className="grid2">
      <section className="card">
        <h2>Scan baru</h2>
        <label>
          Seed URL
          <input value={seed} onChange={(e) => setSeed(e.target.value)} placeholder="http://127.0.0.1:8080/" />
        </label>
        <label>
          Scope (allowlist — satu host/IP per baris)
          <textarea
            value={scope}
            onChange={(e) => setScope(e.target.value)}
            rows={4}
            placeholder={"127.0.0.1\n*.example.com"}
          />
        </label>
        <label className="row">
          <input type="checkbox" checked={auth} onChange={(e) => setAuth(e.target.checked)} />
          &nbsp;Saya berwenang memindai target ini
        </label>
        <div className="row">
          <button onClick={start} disabled={!!s?.running}>Mulai</button>
          <button className="ghost" onClick={stop} disabled={!s?.running}>Stop</button>
        </div>
        {msg && <div className="muted">{msg}</div>}

        {s?.scan_id && !s.running && s.graph_nodes > 0 && (
          <div className="row" style={{ marginTop: 8 }}>
            <Link className="btnlink" to={`/scans/${s.scan_id}/graph`}>
              Lihat attack graph →
            </Link>
          </div>
        )}

        <div className="stats">
          <Stat label="Status" v={statusLabel} hi={s?.running} />
          <Stat label="In-flight" v={s?.in_flight ?? 0} />
          <Stat label="Total aset" v={s?.total ?? 0} />
          <Stat label="Pages" v={s?.pages ?? 0} />
          <Stat label="JS" v={s?.js ?? 0} />
          <Stat label="Forms" v={s?.forms ?? 0} />
          <Stat label="Endpoints" v={s?.endpoints ?? 0} />
          <Stat label="Tech" v={s?.tech ?? 0} />
          <Stat label="AI proposals" v={s?.ai_proposals ?? 0} />
          <Stat label="Graph nodes" v={s?.graph_nodes ?? 0} />
          <Stat label="Graph edges" v={s?.graph_edges ?? 0} />
        </div>
      </section>

      <section className="card">
        <h2>Temuan ({findings.length})</h2>
        <FindingsSummary findings={findings} />
        <div className="findings">
          {findings.map((f, i) => (
            <FindingRow key={i} f={f} />
          ))}
          {findings.length === 0 && <div className="muted">belum ada temuan</div>}
        </div>
        <h2>Log</h2>
        <div className="logs">
          {logs.map((l, i) => (
            <div key={i}>{l}</div>
          ))}
        </div>
      </section>
    </div>
  );
}

function Stat({ label, v, hi }: { label: string; v: any; hi?: boolean }) {
  return (
    <div className={"stat" + (hi ? " hi" : "")}>
      <div className="k">{label}</div>
      <div className="val">{v}</div>
    </div>
  );
}
