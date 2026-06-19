import { useEffect, useState } from "react";
import { useParams, Link } from "react-router-dom";
import { api } from "../api";
import { FindingRow, FindingsSummary } from "../findings";

export default function ScanDetail() {
  const { id } = useParams();
  const [d, setD] = useState<any>(null);

  useEffect(() => {
    if (id) api.scan(Number(id)).then(setD).catch(() => {});
  }, [id]);

  if (!d) return <div className="card muted">memuat…</div>;
  const scan = d.scan;

  const downloadDot = () => {
    const blob = new Blob([scan.dot || ""], { type: "text/vnd.graphviz" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `scan-${scan.id}.dot`;
    a.click();
    URL.revokeObjectURL(url);
  };

  return (
    <div className="card">
      <h2>Scan #{scan.id}</h2>
      <div className="muted">
        {scan.seed} — {scan.status} — {scan.assets_count} aset — graph {scan.graph_nodes}/{scan.graph_edges}
      </div>
      {scan.dot && (
        <div className="row" style={{ marginTop: 8 }}>
          <Link className="btnlink" to={`/scans/${scan.id}/graph`}>
            Lihat attack graph →
          </Link>
          <button className="ghost" onClick={downloadDot}>Unduh DOT</button>
        </div>
      )}
      <FindingsSummary findings={d.findings} />
      <h3>Temuan ({d.findings.length})</h3>
      <div className="findings">
        {d.findings.map((f: any, i: number) => (
          <FindingRow key={i} f={f} />
        ))}
      </div>
    </div>
  );
}
