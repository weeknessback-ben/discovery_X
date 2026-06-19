// Klien API kecil: menyisipkan CSRF token pada request mutasi & menangani 401.

let csrf = "";
export function setCsrf(t: string) {
  csrf = t;
}

async function req(path: string, opts: RequestInit = {}): Promise<Response> {
  const method = (opts.method || "GET").toUpperCase();
  const headers: Record<string, string> = { ...(opts.headers as any) };
  if (opts.body) headers["Content-Type"] = "application/json";
  if (["POST", "PUT", "DELETE", "PATCH"].includes(method)) {
    headers["X-CSRF-Token"] = csrf;
  }
  const res = await fetch("/api" + path, { ...opts, headers });
  if (res.status === 401) {
    window.dispatchEvent(new Event("unauth"));
    throw new Error("unauthenticated");
  }
  return res;
}

export const api = {
  // login tidak butuh CSRF (membuat sesi baru).
  login: (username: string, password: string) =>
    fetch("/api/login", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ username, password }),
    }),
  csrf: () => req("/csrf"),
  logout: () => req("/logout", { method: "POST" }),
  getConfig: () => req("/config").then((r) => r.json()),
  putConfig: (body: any) => req("/config", { method: "PUT", body: JSON.stringify(body) }),
  startScan: (body: any) => req("/scan", { method: "POST", body: JSON.stringify(body) }),
  stopScan: () => req("/scan", { method: "DELETE" }),
  status: () => req("/scan").then((r) => r.json()),
  scans: () => req("/scans").then((r) => r.json()),
  scan: (id: number) => req("/scans/" + id).then((r) => r.json()),
};
