import { useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { api } from "../api";

export default function History() {
  const [scans, setScans] = useState<any[]>([]);
  useEffect(() => {
    api.scans().then(setScans).catch(() => {});
  }, []);

  return (
    <div className="card">
      <h2>Riwayat scan</h2>
      <table>
        <thead>
          <tr>
            <th>ID</th>
            <th>Seed</th>
            <th>Status</th>
            <th>Aset</th>
            <th>Graph</th>
            <th>Mulai</th>
          </tr>
        </thead>
        <tbody>
          {scans.map((s) => (
            <tr key={s.id}>
              <td>
                <Link to={"/scans/" + s.id}>#{s.id}</Link>
              </td>
              <td className="trunc">{s.seed}</td>
              <td>{s.status}</td>
              <td>{s.assets_count}</td>
              <td>
                {s.graph_nodes}/{s.graph_edges}
              </td>
              <td>{s.started_at}</td>
            </tr>
          ))}
        </tbody>
      </table>
      {scans.length === 0 && <div className="muted">belum ada scan</div>}
    </div>
  );
}
