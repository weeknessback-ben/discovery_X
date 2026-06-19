import { useEffect, useState } from "react";
import { Routes, Route, NavLink, Navigate } from "react-router-dom";
import { api, setCsrf } from "./api";
import Login from "./pages/Login";
import Dashboard from "./pages/Dashboard";
import Config from "./pages/Config";
import History from "./pages/History";
import ScanDetail from "./pages/ScanDetail";
import Graph from "./pages/Graph";

export default function App() {
  const [authed, setAuthed] = useState<boolean | null>(null);

  useEffect(() => {
    api
      .csrf()
      .then((r) => r.json())
      .then((j) => {
        setCsrf(j.csrf);
        setAuthed(true);
      })
      .catch(() => setAuthed(false));
    const onUnauth = () => setAuthed(false);
    window.addEventListener("unauth", onUnauth);
    return () => window.removeEventListener("unauth", onUnauth);
  }, []);

  if (authed === null) return <div className="center muted">memuat…</div>;
  if (!authed) return <Login onLogin={() => setAuthed(true)} />;

  return (
    <div className="layout">
      <header className="topbar">
        <span className="brand">🛰 discovery_X</span>
        <nav>
          <NavLink to="/" end>Dashboard</NavLink>
          <NavLink to="/config">Config</NavLink>
          <NavLink to="/history">Riwayat</NavLink>
        </nav>
        <button
          className="link"
          onClick={async () => {
            await api.logout().catch(() => {});
            setAuthed(false);
          }}
        >
          Logout
        </button>
      </header>
      <main>
        <Routes>
          <Route path="/" element={<Dashboard />} />
          <Route path="/config" element={<Config />} />
          <Route path="/history" element={<History />} />
          <Route path="/scans/:id" element={<ScanDetail />} />
          <Route path="/scans/:id/graph" element={<Graph />} />
          <Route path="*" element={<Navigate to="/" />} />
        </Routes>
      </main>
    </div>
  );
}
