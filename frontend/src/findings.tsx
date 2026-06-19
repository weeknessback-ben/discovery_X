// Parsing & rendering temuan dengan detail: stack/teknologi (+versi),
// HTTP status code, server, judul, dan status liveness.
//
// Catatan disimpan sebagai array string (mis. "status=200", "server=nginx/1.18",
// "title=…", "live=yes", "rendered=spa"). Aset jenis `tech` menyimpan label
// stack (mis. "WordPress 6.2") sebagai catatan pertama.

export type Finding = { kind: string; url: string; notes?: string[] | string; origin?: string };

export type Parsed = {
  kind: string; // dinormalisasi ke snake_case (page, js_file, tech, …)
  status?: number;
  server?: string;
  title?: string;
  live?: "yes" | "no";
  rendered?: boolean;
  stack?: { name: string; version?: string }; // hanya untuk aset `tech`
  extra: string[];
};

/** "JsFile" → "js_file", "tech" → "tech". Samakan format DB (Debug) & SSE (serde). */
export function normKind(kind: string): string {
  return kind
    .replace(/([a-z0-9])([A-Z])/g, "$1_$2")
    .toLowerCase();
}

function toArray(notes?: string[] | string): string[] {
  if (!notes) return [];
  if (Array.isArray(notes)) return notes;
  try {
    const v = JSON.parse(notes);
    return Array.isArray(v) ? v : [];
  } catch {
    return [];
  }
}

/** Pisah "WordPress 6.2" → {name:"WordPress", version:"6.2"}. */
function splitStack(label: string): { name: string; version?: string } {
  const m = label.match(/^(.*?)[\s/]+(\d[\w.\-]*)$/);
  return m ? { name: m[1], version: m[2] } : { name: label };
}

export function parseFinding(f: Finding): Parsed {
  const kind = normKind(f.kind);
  const notes = toArray(f.notes);
  const out: Parsed = { kind, extra: [] };

  for (const n of notes) {
    if (n.startsWith("status=")) out.status = Number(n.slice(7)) || undefined;
    else if (n.startsWith("server=")) out.server = n.slice(7);
    else if (n.startsWith("title=")) out.title = n.slice(6);
    else if (n.startsWith("live=")) out.live = n.slice(5) === "yes" ? "yes" : "no";
    else if (n === "rendered=spa") out.rendered = true;
    else if (n.startsWith("rendered_title=")) {
      if (!out.title) out.title = n.slice(15);
    } else if (kind === "tech" && !out.stack) out.stack = splitStack(n);
    else out.extra.push(n);
  }
  if (kind === "tech" && !out.stack) out.stack = splitStack(f.url.split("#tech=")[1]?.replace(/_/g, " ") || "?");
  return out;
}

/** Kelas warna untuk badge HTTP status (2xx hijau, 3xx biru, 4xx kuning, 5xx merah). */
function statusClass(s: number): string {
  if (s >= 500) return "s5";
  if (s >= 400) return "s4";
  if (s >= 300) return "s3";
  if (s >= 200) return "s2";
  return "s1";
}

/** Satu baris temuan dengan badge detail. */
export function FindingRow({ f }: { f: Finding }) {
  const p = parseFinding(f);
  return (
    <div className="finding">
      <span className={"tag " + p.kind}>{p.kind}</span>
      {p.status !== undefined && (
        <span className={"http " + statusClass(p.status)} title="HTTP status">{p.status}</span>
      )}
      {p.stack && (
        <span className="stack" title="Stack terdeteksi">
          {p.stack.name}
          {p.stack.version && <b> {p.stack.version}</b>}
        </span>
      )}
      {p.live && (
        <span className={"live " + (p.live === "yes" ? "ok" : "bad")}>
          {p.live === "yes" ? "● hidup" : "○ mati?"}
        </span>
      )}
      {p.rendered && <span className="meta">SPA</span>}
      <a href={f.url} target="_blank" rel="noreferrer">{f.url}</a>
      {p.server && <span className="meta" title="Server header">{p.server}</span>}
      {(p.title || p.extra.length > 0) && (
        <span className="notes">{[p.title, ...p.extra].filter(Boolean).join(" · ")}</span>
      )}
    </div>
  );
}

/** Ringkasan stack terdeteksi + distribusi HTTP status untuk seluruh temuan. */
export function FindingsSummary({ findings }: { findings: Finding[] }) {
  const parsed = findings.map(parseFinding);
  const stacks = new Map<string, { name: string; version?: string }>();
  const codes = new Map<number, number>();
  for (const p of parsed) {
    if (p.stack) {
      const key = p.stack.name + (p.stack.version ? " " + p.stack.version : "");
      stacks.set(key, p.stack);
    }
    if (p.status !== undefined) codes.set(p.status, (codes.get(p.status) || 0) + 1);
  }
  if (stacks.size === 0 && codes.size === 0) return null;
  const codeList = [...codes.entries()].sort((a, b) => a[0] - b[0]);
  return (
    <div className="summary">
      {stacks.size > 0 && (
        <div className="sumrow">
          <span className="k">Stack terdeteksi</span>
          <span className="chips">
            {[...stacks.values()].map((s, i) => (
              <span key={i} className="stack">
                {s.name}
                {s.version && <b> {s.version}</b>}
              </span>
            ))}
          </span>
        </div>
      )}
      {codeList.length > 0 && (
        <div className="sumrow">
          <span className="k">HTTP status</span>
          <span className="chips">
            {codeList.map(([code, n]) => (
              <span key={code} className={"http " + statusClass(code)} title={`${n} respons`}>
                {code} <i>×{n}</i>
              </span>
            ))}
          </span>
        </div>
      )}
    </div>
  );
}
