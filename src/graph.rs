//! Attack graph in-memory (petgraph): relasi antar aset yang ditemukan.
//!
//! Node = aset (halaman, file JS, form, subdomain, endpoint). Edge = relasi
//! (`links`, `references`, `contains`, `calls`, `resolves`, `hosts`).
//! Lihat ide di `discovery.md` (§E "Stateful Fuzzing Berbasis Graf").

use std::collections::HashMap;

use petgraph::dot::{Config as DotConfig, Dot};
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use petgraph::Direction;

use crate::types::{AssetKind, EdgeKind};

/// Node graf. `kind` bisa `None` bila node dibuat sebagai target edge
/// sebelum aset-nya sendiri sempat di-fetch/diklasifikasi.
#[derive(Debug, Clone)]
pub struct GraphNode {
    pub url: String,
    pub kind: Option<AssetKind>,
}

impl std::fmt::Display for GraphNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.kind {
            Some(k) => write!(f, "[{k:?}] {}", self.url),
            None => write!(f, "[?] {}", self.url),
        }
    }
}

#[derive(Default)]
pub struct AttackGraph {
    g: DiGraph<GraphNode, EdgeKind>,
    index: HashMap<String, NodeIndex>,
}

impl AttackGraph {
    pub fn new() -> Self {
        Self::default()
    }

    fn ensure_node(&mut self, url: &str) -> NodeIndex {
        if let Some(idx) = self.index.get(url) {
            return *idx;
        }
        let idx = self.g.add_node(GraphNode {
            url: url.to_string(),
            kind: None,
        });
        self.index.insert(url.to_string(), idx);
        idx
    }

    /// Tambahkan/aktualkan node aset dengan jenis yang diketahui.
    pub fn add_asset(&mut self, url: &str, kind: AssetKind) {
        let idx = self.ensure_node(url);
        self.g[idx].kind = Some(kind);
    }

    /// Tambahkan relasi `from -> to`. Edge ganda dengan jenis sama tidak diduplikasi.
    pub fn add_edge(&mut self, from: &str, to: &str, kind: EdgeKind) {
        let a = self.ensure_node(from);
        let b = self.ensure_node(to);
        // update_edge menambah bila belum ada, atau memperbarui bobotnya.
        self.g.update_edge(a, b, kind);
    }

    pub fn node_count(&self) -> usize {
        self.g.node_count()
    }

    pub fn edge_count(&self) -> usize {
        self.g.edge_count()
    }

    /// Endpoint hasil inferensi AI: target dari edge `Calls`.
    /// Inilah "hidden assets" paling menarik untuk diuji lebih lanjut.
    pub fn ai_discovered_endpoints(&self) -> Vec<String> {
        let mut out = Vec::new();
        for edge in self.g.edge_references() {
            if *edge.weight() == EdgeKind::Calls {
                out.push(self.g[edge.target()].url.clone());
            }
        }
        out.sort();
        out.dedup();
        out
    }

    /// Node "hub" (in-degree tertinggi) — aset paling banyak dirujuk.
    pub fn top_hubs(&self, n: usize) -> Vec<(String, usize)> {
        let mut hubs: Vec<(String, usize)> = self
            .g
            .node_indices()
            .map(|idx| {
                let indeg = self.g.edges_directed(idx, Direction::Incoming).count();
                (self.g[idx].url.clone(), indeg)
            })
            .filter(|(_, d)| *d > 0)
            .collect();
        hubs.sort_by(|a, b| b.1.cmp(&a.1));
        hubs.truncate(n);
        hubs
    }

    /// Ekspor graf sebagai JSON {nodes, edges} untuk visualisasi D3.
    /// `deg` = total derajat (in+out) → dipakai frontend untuk ukuran node.
    pub fn to_json(&self) -> String {
        let nodes: Vec<serde_json::Value> = self
            .g
            .node_indices()
            .map(|idx| {
                let n = &self.g[idx];
                let deg = self.g.edges_directed(idx, Direction::Incoming).count()
                    + self.g.edges_directed(idx, Direction::Outgoing).count();
                serde_json::json!({
                    "id": n.url,
                    "kind": n.kind.and_then(|k| serde_json::to_value(k).ok()),
                    "deg": deg,
                })
            })
            .collect();
        let edges: Vec<serde_json::Value> = self
            .g
            .edge_references()
            .map(|e| {
                serde_json::json!({
                    "source": self.g[e.source()].url,
                    "target": self.g[e.target()].url,
                    "kind": e.weight().as_str(),
                })
            })
            .collect();
        serde_json::json!({ "nodes": nodes, "edges": edges }).to_string()
    }

    /// Ekspor ke format Graphviz DOT (untuk dirender `dot -Tpng`).
    pub fn to_dot(&self) -> String {
        let dot = Dot::with_attr_getters(
            &self.g,
            &[DotConfig::NodeNoLabel, DotConfig::EdgeNoLabel],
            &|_, edge| format!("label=\"{}\"", edge.weight().as_str()),
            &|_, (_, node)| {
                let kind = node
                    .kind
                    .map(|k| format!("{k:?}"))
                    .unwrap_or_else(|| "?".to_string());
                format!("label=\"[{}] {}\"", kind, node.url)
            },
        );
        format!("{dot}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_and_queries_graph() {
        let mut g = AttackGraph::new();
        g.add_asset("https://t/", AssetKind::Page);
        g.add_asset("https://t/app.js", AssetKind::JsFile);
        g.add_edge("https://t/", "https://t/app.js", EdgeKind::References);
        // AI menemukan endpoint dari app.js
        g.add_edge("https://t/app.js", "https://t/api/admin", EdgeKind::Calls);
        g.add_asset("https://t/api/admin", AssetKind::Endpoint);

        assert_eq!(g.node_count(), 3);
        assert_eq!(g.edge_count(), 2);
        assert_eq!(g.ai_discovered_endpoints(), vec!["https://t/api/admin"]);
        // app.js & root punya in-degree; api/admin in-degree 1
        let hubs = g.top_hubs(5);
        assert!(hubs.iter().any(|(u, _)| u == "https://t/api/admin"));
        assert!(g.to_dot().contains("calls"));
    }

    #[test]
    fn dedups_edges_and_nodes() {
        let mut g = AttackGraph::new();
        g.add_edge("a", "b", EdgeKind::Links);
        g.add_edge("a", "b", EdgeKind::Links);
        assert_eq!(g.node_count(), 2);
        assert_eq!(g.edge_count(), 1);
    }
}
