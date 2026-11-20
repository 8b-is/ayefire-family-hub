#![deny(clippy::float_arithmetic)]
//! Local-First DeepSeek `p/q` (`deepseek-r1-pqm-a161330`) Attestation & Loopback Inference Client.
//!
//! Enforces literal loopback boundary (`127.0.0.1`, `::1`, `localhost`) so family meals,
//! medical/athletic forms, and household context NEVER silently egress to any third-party cloud.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const DEFAULT_BUNDLE_DIR: &str =
    "/home/hue/.local/share/ayefire/models/deepseek-r1-pqm-a161330/ayeos_bundle";
pub const DEFAULT_OLLAMA_ENDPOINT: &str = "http://127.0.0.1:11436";
pub const DEFAULT_MODEL_NAME: &str = "aye-deepseek-pqm:latest";

#[derive(Debug, Clone, Serialize)]
pub struct DeepSeekPqCapsuleStatus {
    pub bundle_dir: String,
    pub bundle_present: bool,
    pub schema: String,
    pub capsule_type: String,
    pub model_id: String,
    pub merkle_root_blake3: String,
    pub ternary_matrices_found: usize,
    pub ternary_matrices_expected: usize,
    pub m000_magic_valid: bool,
    pub embed_magic_valid: bool,
    pub total_bundle_mib: u64,
    pub inference_endpoint: String,
    pub inference_model: String,
    pub loopback_enforced: bool,
}

#[derive(Debug, Deserialize)]
struct AttestalProofHead {
    #[serde(default)]
    schema: String,
    #[serde(default)]
    capsule_type: String,
    #[serde(default)]
    model: String,
    #[serde(default)]
    num_ternary_matrices: usize,
    #[serde(default)]
    merkle_root_blake3: String,
}

pub fn is_literal_loopback_url(url: &str) -> bool {
    let rest = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
        .unwrap_or("");
    let host_port = rest.split('/').next().unwrap_or("");
    if host_port.starts_with("[::1]") {
        return true;
    }
    let host = host_port.split(':').next().unwrap_or("");
    matches!(host, "127.0.0.1" | "localhost")
}

pub fn inspect_local_pqm_bundle(bundle_dir: &Path, endpoint: &str, model: &str) -> DeepSeekPqCapsuleStatus {
    let proof_path = bundle_dir.join("attestal.proof.v1.json");
    let proof: Option<AttestalProofHead> = fs::read_to_string(&proof_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok());

    let mut matrices_found = 0usize;
    let mut total_bytes = 0u64;
    if let Ok(entries) = fs::read_dir(bundle_dir) {
        for ent in entries.flatten() {
            let p = ent.path();
            if let Ok(md) = ent.metadata() {
                total_bytes = total_bytes.saturating_add(md.len());
            }
            if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                if name.starts_with('m') && name.ends_with(".bin") {
                    matrices_found += 1;
                }
            }
        }
    }

    let m000_magic_valid = read_magic8(&bundle_dir.join("m000.bin")) == Some(*b"AYEM158\0");
    let embed_magic_valid =
        read_magic8(&bundle_dir.join("embed_tokens_q4_g64.bin")) == Some(*b"AYEEQ4G\0");

    let (schema, capsule_type, model_id, merkle_root_blake3, expected) = match proof {
        Some(p) => (
            p.schema,
            p.capsule_type,
            p.model,
            p.merkle_root_blake3,
            p.num_ternary_matrices,
        ),
        None => (
            "missing".to_string(),
            "missing".to_string(),
            "missing".to_string(),
            String::new(),
            196,
        ),
    };

    DeepSeekPqCapsuleStatus {
        bundle_dir: bundle_dir.display().to_string(),
        bundle_present: m000_magic_valid && embed_magic_valid && matrices_found >= expected,
        schema,
        capsule_type,
        model_id,
        merkle_root_blake3,
        ternary_matrices_found: matrices_found,
        ternary_matrices_expected: expected,
        m000_magic_valid,
        embed_magic_valid,
        total_bundle_mib: total_bytes / (1024 * 1024),
        inference_endpoint: endpoint.to_string(),
        inference_model: model.to_string(),
        loopback_enforced: is_literal_loopback_url(endpoint),
    }
}

fn read_magic8(path: &Path) -> Option<[u8; 8]> {
    let mut f = fs::File::open(path).ok()?;
    use std::io::Read;
    let mut buf = [0u8; 8];
    f.read_exact(&mut buf).ok()?;
    Some(buf)
}

pub async fn generate_local_reply(
    endpoint: &str,
    model: &str,
    system_prompt: &str,
    user_prompt: &str,
    max_tokens: u32,
) -> Result<String, String> {
    if !is_literal_loopback_url(endpoint) {
        return Err(
            "BLOCKED_NON_LOOPBACK: family inference endpoint must be literal loopback (127.0.0.1/localhost/::1)"
                .to_string(),
        );
    }

    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(25))
        .build()
        .map_err(|e| format!("http client init failed: {e}"))?;

    let url = format!("{}/api/generate", endpoint.trim_end_matches('/'));
    let payload = serde_json::json!({
        "model": model,
        "system": system_prompt,
        "prompt": user_prompt,
        "stream": false,
        "options": {
            "num_predict": max_tokens
        }
    });

    let resp = client
        .post(&url)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("local DeepSeek p/q unreachable at {url}: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("local DeepSeek p/q returned HTTP {}", resp.status().as_u16()));
    }

    let val: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("invalid json from DeepSeek p/q: {e}"))?;

    let raw = val
        .get("response")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();

    // Strip internal <think>...</think> blocks cleanly for family-friendly display
    let cleaned = strip_think_tags(&raw);
    if cleaned.is_empty() {
        Ok(raw)
    } else {
        Ok(cleaned)
    }
}

pub fn strip_think_tags(s: &str) -> String {
    if let Some(end_idx) = s.find("</think>") {
        s[end_idx + "</think>".len()..].trim().to_string()
    } else {
        s.trim().to_string()
    }
}

pub fn bundle_dir_from_env() -> PathBuf {
    std::env::var("AYEFIRE_PQM_BUNDLE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(DEFAULT_BUNDLE_DIR))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_loopback_guard_blocks_external_hosts() {
        assert!(is_literal_loopback_url("http://127.0.0.1:11436"));
        assert!(is_literal_loopback_url("http://localhost:11434"));
        assert!(is_literal_loopback_url("http://[::1]:11436"));
        assert!(!is_literal_loopback_url("https://api.openai.com"));
        assert!(!is_literal_loopback_url("http://127.0.0.1.evil.com"));
    }
}
