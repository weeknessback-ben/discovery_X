import { useEffect, useRef, useState } from "react";
import { useParams, Link } from "react-router-dom";
import * as d3 from "d3";

const COLORS: Record<string, string> = {
  page: "#4ea1ff",
  js_file: "#ffd87a",
  form: "#d8a4ff",
  endpoint: "#2bd66a",
  subdomain: "#ffc08a",
  tech: "#ff6b6b",
  unknown: "#8593a3",
};

const LEGEND: [string, string][] = [
  ["page", "Page"],
  ["js_file", "JS"],
  ["form", "Form"],
  ["endpoint", "Endpoint"],
  ["subdomain", "Subdomain"],
  ["tech", "Tech"],
];

const NODE_CAP = 1500; // graf besar dipangkas ke node ter-hub agar tetap responsif

export default function Graph() {
  const { id } = useParams();
  const ref = useRef<SVGSVGElement | null>(null);
  const [info, setInfo] = useState("memuat…");
  const [hover, setHover] = useState<string>("");

  useEffect(() => {
    let sim: d3.Simulation<any, any> | null = null;

    fetch(`/api/scans/${id}/graph.json`)
      .then((r) => {
        if (!r.ok) throw new Error("nope");
        return r.json();
      })
      .then((data) => {
        let nodes = (data.nodes || []).map((n: any) => ({ ...n }));
        let links = (data.edges || []).map((e: any) => ({ ...e }));
        let capped = false;
        if (nodes.length > NODE_CAP) {
          capped = true;
          nodes.sort((a: any, b: any) => (b.deg || 0) - (a.deg || 0));
          nodes = nodes.slice(0, NODE_CAP);
          const keep = new Set(nodes.map((n: any) => n.id));
          links = links.filter((l: any) => keep.has(l.source) && keep.has(l.target));
        }
        setInfo(
          `${data.nodes.length} node, ${data.edges.length} edge` +
            (capped ? ` — menampilkan ${NODE_CAP} node ter-hub` : "")
        );
        draw(nodes, links);
      })
      .catch(() => setInfo("graph tidak tersedia untuk scan ini"));

    function draw(nodes: any[], links: any[]) {
      const el = ref.current!;
      const svg = d3.select(el);
      svg.selectAll("*").remove();
      const width = el.clientWidth || 900;
      const height = el.clientHeight || 600;

      const g = svg.append("g");
      const zoom = d3
        .zoom<SVGSVGElement, unknown>()
        .scaleExtent([0.05, 4])
        .on("zoom", (e) => g.attr("transform", e.transform.toString()));
      svg.call(zoom as any);

      const link = g
        .append("g")
        .selectAll("line")
        .data(links)
        .join("line")
        .attr("stroke", (d: any) => (d.kind === "calls" ? "#2bd66a" : "#3a4452"))
        .attr("stroke-opacity", (d: any) => (d.kind === "calls" ? 0.9 : 0.35))
        .attr("stroke-width", (d: any) => (d.kind === "calls" ? 1.8 : 0.8));

      const node = g
        .append("g")
        .selectAll("circle")
        .data(nodes)
        .join("circle")
        .attr("r", (d: any) => Math.min(3 + Math.sqrt(d.deg || 0) * 2, 20))
        .attr("fill", (d: any) => COLORS[d.kind || "unknown"] || COLORS.unknown)
        .attr("stroke", "#0f1419")
        .attr("stroke-width", 0.6)
        .style("cursor", "pointer")
        .on("mouseover", (_e: any, d: any) => setHover(`[${d.kind || "?"}] ${d.id}`))
        .on("mouseout", () => setHover(""))
        .on("click", (_e: any, d: any) => {
          if (/^https?:\/\//.test(d.id)) window.open(d.id, "_blank", "noreferrer");
        })
        .call(
          d3
            .drag<any, any>()
            .on("start", (e, d: any) => {
              if (!e.active) sim!.alphaTarget(0.3).restart();
              d.fx = d.x;
              d.fy = d.y;
            })
            .on("drag", (e, d: any) => {
              d.fx = e.x;
              d.fy = e.y;
            })
            .on("end", (e, d: any) => {
              if (!e.active) sim!.alphaTarget(0);
              d.fx = null;
              d.fy = null;
            }) as any
        );

      node.append("title").text((d: any) => `[${d.kind || "?"}] ${d.id}`);

      sim = d3
        .forceSimulation(nodes)
        .force("link", d3.forceLink(links).id((d: any) => d.id).distance(45).strength(0.35))
        .force("charge", d3.forceManyBody().strength(-70))
        .force("center", d3.forceCenter(width / 2, height / 2))
        .force("collide", d3.forceCollide().radius(11))
        .on("tick", () => {
          link
            .attr("x1", (d: any) => d.source.x)
            .attr("y1", (d: any) => d.source.y)
            .attr("x2", (d: any) => d.target.x)
            .attr("y2", (d: any) => d.target.y);
          node.attr("cx", (d: any) => d.x).attr("cy", (d: any) => d.y);
        });
    }

    return () => {
      if (sim) sim.stop();
    };
  }, [id]);

  return (
    <div className="card">
      <div className="row" style={{ justifyContent: "space-between" }}>
        <h2>Attack graph #{id}</h2>
        <div className="row">
          <span className="muted">{info}</span>
          <a className="btnlink" href={`/api/scans/${id}/graph.svg`} target="_blank" rel="noreferrer">
            SVG ↗
          </a>
          <Link className="ghostlink" to={`/scans/${id}`}>← detail</Link>
        </div>
      </div>
      <div className="legend">
        {LEGEND.map(([k, l]) => (
          <span key={k}>
            <i style={{ background: COLORS[k] }} />
            {l}
          </span>
        ))}
        <span>
          <i style={{ background: "#2bd66a", borderRadius: 0, width: 16, height: 3 }} />
          edge "calls" (endpoint inferensi AI)
        </span>
      </div>
      <div className="muted hint">seret node • scroll untuk zoom • klik node membuka URL</div>
      <svg ref={ref} className="graphsvg" />
      {hover && <div className="graphtip">{hover}</div>}
    </div>
  );
}
