//! Kontrak AI yang ketat (anti-halusinasi) + builder prompt.

use serde::Deserialize;

use crate::types::DiscoveryType;

/// Rencana aksi yang HARUS dipatuhi AI. `serde` menolak bila bentuknya salah.
#[derive(Debug, Clone, Deserialize)]
pub struct AIActionPlan {
    pub target_url: String,
    pub discovery_type: DiscoveryType,
    pub payloads: Vec<String>,
    #[serde(default)]
    pub reasoning: String,
}

/// System prompt: mendefinisikan peran & format output yang wajib JSON.
pub fn system_prompt() -> String {
    r#"You are a discovery assistant for an AUTHORIZED penetration test.
You are given candidate strings extracted from a JavaScript file.
Your job: pick the ones that are likely HIDDEN or INTERESTING HTTP endpoints/paths
(e.g. /api/v1/internal/admin_reset), and ignore static assets or third-party library paths.

If a detected technology stack is provided, ALSO propose paths/endpoints that are
characteristic of that stack and its version (e.g. WordPress -> /wp-json/wp/v2/users,
/wp-admin/; Laravel -> /telescope, /.env; Next.js -> /_next/data/...; Spring -> /actuator).
Prefer version-specific or sensitive paths known for the detected stack.

Respond with ONLY a single JSON object, no markdown, no prose, matching exactly:
{
  "target_url": "<the source file url you analyzed>",
  "discovery_type": "js_analysis" | "param_guessing" | "dir_busting",
  "payloads": ["/path/one", "/api/two", "..."],
  "reasoning": "<one short sentence>"
}
Rules:
- "payloads" MUST be an array of strings (paths or absolute URLs to probe).
- Return at most 25 payloads, the most promising ones.
- Output MUST be valid JSON parseable as-is. Do NOT wrap it in ```."#
        .to_string()
}

/// User prompt berisi konteks file, stack terdeteksi & kandidat.
pub fn user_prompt(source_url: &str, candidates: &[String], tech: &[String]) -> String {
    let list = candidates
        .iter()
        .map(|c| format!("- {c}"))
        .collect::<Vec<_>>()
        .join("\n");
    let stack = if tech.is_empty() {
        "Detected technology stack: (none detected)".to_string()
    } else {
        format!("Detected technology stack: {}", tech.join(", "))
    };
    format!(
        "Source JavaScript file: {source_url}\n{stack}\n\nCandidate strings:\n{list}\n\n\
         Suggest hidden endpoints — include paths typical for the detected stack. \
         Return the JSON object now."
    )
}

/// Pesan koreksi saat output sebelumnya gagal divalidasi `serde`.
pub fn correction_prompt(error: &str) -> String {
    format!(
        "Your previous response was not valid according to the required schema. \
         Error: {error}. Reply again with ONLY the corrected JSON object \
         (fields: target_url:string, discovery_type:one of js_analysis|param_guessing|dir_busting, \
         payloads:array of strings, reasoning:string). No markdown, no prose."
    )
}

/// Bersihkan respons LLM dari pagar markdown ```json ... ``` bila ada.
pub fn strip_code_fences(s: &str) -> &str {
    let t = s.trim();
    if let Some(rest) = t.strip_prefix("```") {
        // buang baris pertama (mis. "json") sampai newline, dan fence penutup
        let rest = rest.splitn(2, '\n').nth(1).unwrap_or(rest);
        return rest.trim().trim_end_matches("```").trim();
    }
    t
}

/// Parse + validasi output AI menjadi `AIActionPlan`.
pub fn parse_plan(raw: &str) -> Result<AIActionPlan, serde_json::Error> {
    serde_json::from_str(strip_code_fences(raw))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_plan() {
        let raw = r#"{"target_url":"https://t/app.js","discovery_type":"js_analysis",
            "payloads":["/api/admin","/api/internal"],"reasoning":"looks internal"}"#;
        let plan = parse_plan(raw).unwrap();
        assert_eq!(plan.discovery_type, DiscoveryType::JsAnalysis);
        assert_eq!(plan.payloads.len(), 2);
    }

    #[test]
    fn parses_with_code_fence() {
        let raw = "```json\n{\"target_url\":\"x\",\"discovery_type\":\"dir_busting\",\"payloads\":[\"/a\"]}\n```";
        let plan = parse_plan(raw).unwrap();
        assert_eq!(plan.discovery_type, DiscoveryType::DirBusting);
    }

    #[test]
    fn rejects_malformed() {
        // payloads bukan array string → harus error (kontrak ketat).
        let raw = r#"{"target_url":"x","discovery_type":"js_analysis","payloads":"oops"}"#;
        assert!(parse_plan(raw).is_err());
    }

    #[test]
    fn user_prompt_includes_detected_stack() {
        let p = user_prompt(
            "https://t/app.js",
            &["/api/x".to_string()],
            &["WordPress 6.2".to_string(), "nginx 1.18".to_string()],
        );
        assert!(p.contains("WordPress 6.2"));
        assert!(p.contains("nginx 1.18"));
        // tanpa stack → tetap valid (placeholder)
        let p2 = user_prompt("https://t/app.js", &["/api/x".to_string()], &[]);
        assert!(p2.contains("none detected"));
    }

    #[test]
    fn rejects_unknown_discovery_type() {
        let raw = r#"{"target_url":"x","discovery_type":"nuke_everything","payloads":[]}"#;
        assert!(parse_plan(raw).is_err());
    }
}
