#![deny(clippy::float_arithmetic)]
//! `id1.is` Sovereign Transparency Passport & 16-Byte `ID1P` Presentation Signature Bridge.
//!
//! Connects AyeFire's local Transparency Radar with the `id1.is` ecosystem:
//! 1. **Exact 16-byte `ID1P` Codec (CRC-16/CCITT-FALSE, bit-compatible with `pqtrn-core::id1_pref`)**:
//!    - Encodes theme, zero-purple guard (`FLAG_VETO_PURPLE`), minimum font size, reading width,
//!      language, and accessibility flags into 16 exact bytes (`32` hex chars).
//!    - Emits `X-ID1-Pref: <32-hex>` + `Sec-GPC: 1` (Global Privacy Control) on inspected HTTPS
//!      requests when opted in, and records `X-ID1-Pref-Ack` responses from honoring sites!
//! 2. **BLAKE3-Sealed Domain Transparency Receipts (`id1_transparency_receipt_v1`)**:
//!    - Exports strictly aggregate, zero-PII, zero-cookie site behavior reports attested by
//!      the local `id1.is` handle so transparency travels everywhere across `id1.is` / `8b.is` / `i1.is`.
//! 3. **Portable `id1.is` Policy Sync (`id1_transparency_policy_v1`)**:
//!    - Export & import blocked hosts, silenced beacon paths, auto-silence preference, and `ID1P`
//!      across every AyeFire box, workstation, and device owned by the same `id1.is` identity.

use crate::transparency_radar::{SiteTransparencyView, TransparencyRadar};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;

pub const ID1P_MAGIC: [u8; 4] = *b"ID1P";
pub const ID1P_VERSION: u8 = 1;
pub const ID1P_LEN: usize = 16;

pub const FLAG_STRICT_LANG_ONLY: u8 = 1 << 0;
pub const FLAG_REDUCED_MOTION: u8 = 1 << 1;
pub const FLAG_VETO_PURPLE: u8 = 1 << 2;
pub const FLAG_DYSLEXIA_SPACING: u8 = 1 << 3;
pub const FLAG_CAPTIONS_ALWAYS: u8 = 1 << 4;
pub const FLAG_HIGH_CONTRAST_BORDERS: u8 = 1 << 5;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Id1PassportConfig {
    pub handle: String,
    pub preset: String, // "hue" | "bob" | "warm_comfort"
    pub send_gpc_header: bool,
    pub send_id1p_header: bool,
    pub local_attestation_seed_hex: String,
}

impl Default for Id1PassportConfig {
    fn default() -> Self {
        let seed_input = format!(
            "ayefire-id1-local-seed|{}|{}",
            fs::read_to_string("/etc/hostname").unwrap_or_else(|_| "8bsov".into()),
            crate::pdf_forms::now_unix_secs()
        );
        let seed_hex = blake3::hash(seed_input.as_bytes()).to_hex().to_string();
        Self {
            handle: "hue.id1.is".to_string(),
            preset: "hue".to_string(),
            send_gpc_header: true,
            send_id1p_header: true,
            local_attestation_seed_hex: seed_hex,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Id1PassportStatus {
    pub handle: String,
    pub preset: String,
    pub id1p_hex: String,
    pub id1p_summary: String,
    pub send_gpc_header: bool,
    pub send_id1p_header: bool,
    pub passport_key_id: String,
    pub gpc_headers_sent: u64,
    pub id1p_headers_sent: u64,
    pub sites_honoring_id1_ack: Vec<String>,
    pub privacy_bypass_protected: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Id1TransparencyReceipt {
    pub schema: String,
    pub observer_handle: String,
    pub observer_key_id: String,
    pub id1p_hex: String,
    pub host: String,
    pub trust_of_20: u8,
    pub trust_pq: String,
    pub verdict: String,
    pub total_requests: u64,
    pub micro_beacons: u64,
    pub pixel_or_204_beacons: u64,
    pub third_party_hosts_count: usize,
    pub sample_beacon_paths: Vec<String>,
    pub known_tracker_note: Option<String>,
    pub observed_unix_secs: u64,
    pub blake3_attestation: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Id1PortablePolicy {
    pub schema: String,
    pub handle: String,
    pub key_id: String,
    pub preset: String,
    pub id1p_hex: String,
    pub send_gpc_header: bool,
    pub send_id1p_header: bool,
    pub auto_silence_beacons: bool,
    pub blocked_hosts: Vec<String>,
    pub silenced_paths: Vec<String>,
    pub exported_unix_secs: u64,
    pub blake3_seal: String,
}

pub struct Id1TransparencyBridge {
    config_path: PathBuf,
    cfg: RwLock<Id1PassportConfig>,
    pub gpc_headers_sent: AtomicU64,
    pub id1p_headers_sent: AtomicU64,
    honoring_sites: RwLock<BTreeSet<String>>,
}

impl Id1TransparencyBridge {
    pub fn load_from_env() -> Arc<Self> {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/home/hue".to_string());
        let cfg_root = std::env::var("AYEFIRE_CONFIG_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(&home).join(".config/ayefire"));
        let _ = fs::create_dir_all(&cfg_root);
        let config_path = cfg_root.join("id1-passport.json");

        let cfg: Id1PassportConfig = match fs::read_to_string(&config_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
        {
            Some(c) => c,
            None => {
                let fresh = Id1PassportConfig::default();
                if let Ok(json) = serde_json::to_string_pretty(&fresh) {
                    let _ = fs::write(&config_path, json);
                    let _ = fs::set_permissions(&config_path, fs::Permissions::from_mode(0o600));
                }
                fresh
            }
        };

        let id1p_hex = encode_id1p_preset_hex(&cfg.preset);
        println!(
            "[ID1-TRANSPARENCY] Sovereign passport '{}' ready | ID1P={} | Sec-GPC={} | X-ID1-Pref={}",
            cfg.handle, id1p_hex, cfg.send_gpc_header, cfg.send_id1p_header
        );

        Arc::new(Self {
            config_path,
            cfg: RwLock::new(cfg),
            gpc_headers_sent: AtomicU64::new(0),
            id1p_headers_sent: AtomicU64::new(0),
            honoring_sites: RwLock::new(BTreeSet::new()),
        })
    }

    pub async fn outbound_signal_headers(&self, allow_id1p: bool) -> (bool, Option<String>) {
        let g = self.cfg.read().await;
        let gpc = g.send_gpc_header;
        let id1p = if allow_id1p && g.send_id1p_header {
            Some(encode_id1p_preset_hex(&g.preset))
        } else {
            None
        };
        drop(g);
        if gpc {
            self.gpc_headers_sent.fetch_add(1, Ordering::Relaxed);
        }
        if id1p.is_some() {
            self.id1p_headers_sent.fetch_add(1, Ordering::Relaxed);
        }
        (gpc, id1p)
    }

    pub async fn record_site_honored_id1(&self, host: &str) {
        let clean = crate::transparency_radar::normalize_host(host);
        if !clean.is_empty() {
            let mut g = self.honoring_sites.write().await;
            if g.len() < 128 {
                g.insert(clean);
            }
        }
    }

    pub async fn update_config(
        &self,
        handle: Option<String>,
        preset: Option<String>,
        send_gpc_header: Option<bool>,
        send_id1p_header: Option<bool>,
    ) -> Id1PassportStatus {
        {
            let mut g = self.cfg.write().await;
            if let Some(h) = handle {
                let clean = sanitize_id1_handle(&h);
                if !clean.is_empty() {
                    g.handle = clean;
                }
            }
            if let Some(p) = preset {
                g.preset = normalize_preset_name(&p).to_string();
            }
            if let Some(v) = send_gpc_header {
                g.send_gpc_header = v;
            }
            if let Some(v) = send_id1p_header {
                g.send_id1p_header = v;
            }
            if let Ok(json) = serde_json::to_string_pretty(&*g) {
                let _ = fs::write(&self.config_path, json);
                let _ = fs::set_permissions(&self.config_path, fs::Permissions::from_mode(0o600));
            }
        }
        self.status().await
    }

    pub async fn status(&self) -> Id1PassportStatus {
        let g = self.cfg.read().await.clone();
        let id1p_hex = encode_id1p_preset_hex(&g.preset);
        let key_id = derive_public_key_id(&g.handle, &g.local_attestation_seed_hex);
        let sites_honoring_id1_ack: Vec<String> =
            self.honoring_sites.read().await.iter().cloned().collect();

        Id1PassportStatus {
            handle: g.handle,
            preset: g.preset.clone(),
            id1p_hex,
            id1p_summary: preset_summary(&g.preset).to_string(),
            send_gpc_header: g.send_gpc_header,
            send_id1p_header: g.send_id1p_header,
            passport_key_id: key_id,
            gpc_headers_sent: self.gpc_headers_sent.load(Ordering::Relaxed),
            id1p_headers_sent: self.id1p_headers_sent.load(Ordering::Relaxed),
            sites_honoring_id1_ack,
            privacy_bypass_protected: true,
        }
    }

    pub async fn attest_site(&self, site: &SiteTransparencyView) -> Id1TransparencyReceipt {
        let g = self.cfg.read().await.clone();
        let key_id = derive_public_key_id(&g.handle, &g.local_attestation_seed_hex);
        let id1p_hex = encode_id1p_preset_hex(&g.preset);
        let now = crate::pdf_forms::now_unix_secs();

        let sign_material = format!(
            "id1_transparency_receipt_v1|{}|{}|{}|{}|{}/20|{}|{}|{}|{}",
            g.handle,
            key_id,
            site.host,
            site.verdict,
            site.trust_of_20,
            site.post_beacons,
            site.pixel_or_204_beacons,
            site.third_party_hosts.len(),
            now
        );
        let keyed_input = format!("{}|{}", g.local_attestation_seed_hex, sign_material);
        let blake3_attestation = blake3::hash(keyed_input.as_bytes()).to_hex().to_string();

        Id1TransparencyReceipt {
            schema: "id1_transparency_receipt_v1".to_string(),
            observer_handle: g.handle,
            observer_key_id: key_id,
            id1p_hex,
            host: site.host.clone(),
            trust_of_20: site.trust_of_20,
            trust_pq: site.trust_pq.clone(),
            verdict: site.verdict.clone(),
            total_requests: site.total_requests,
            micro_beacons: site.post_beacons,
            pixel_or_204_beacons: site.pixel_or_204_beacons,
            third_party_hosts_count: site.third_party_hosts.len(),
            sample_beacon_paths: site.sample_beacon_paths.clone(),
            known_tracker_note: site.known_tracker_note.clone(),
            observed_unix_secs: now,
            blake3_attestation,
        }
    }

    pub async fn export_portable_policy(&self, radar: &TransparencyRadar) -> Id1PortablePolicy {
        let g = self.cfg.read().await.clone();
        let st = radar.status().await;
        let key_id = derive_public_key_id(&g.handle, &g.local_attestation_seed_hex);
        let id1p_hex = encode_id1p_preset_hex(&g.preset);
        let now = crate::pdf_forms::now_unix_secs();

        let seal_material = format!(
            "id1_transparency_policy_v1|{}|{}|{}|{}|{}|{}|{}|{}",
            g.handle,
            id1p_hex,
            g.send_gpc_header,
            g.send_id1p_header,
            st.auto_silence_beacons,
            st.blocked_hosts.join(","),
            st.silenced_paths.join(","),
            now
        );
        let blake3_seal = blake3::hash(seal_material.as_bytes()).to_hex().to_string();

        Id1PortablePolicy {
            schema: "id1_transparency_policy_v1".to_string(),
            handle: g.handle,
            key_id,
            preset: g.preset,
            id1p_hex,
            send_gpc_header: g.send_gpc_header,
            send_id1p_header: g.send_id1p_header,
            auto_silence_beacons: st.auto_silence_beacons,
            blocked_hosts: st.blocked_hosts,
            silenced_paths: st.silenced_paths,
            exported_unix_secs: now,
            blake3_seal,
        }
    }

    pub async fn import_portable_policy(
        &self,
        policy: &Id1PortablePolicy,
        radar: &TransparencyRadar,
    ) -> (usize, usize) {
        let _ = self
            .update_config(
                Some(policy.handle.clone()),
                Some(policy.preset.clone()),
                Some(policy.send_gpc_header),
                Some(policy.send_id1p_header),
            )
            .await;
        radar.set_auto_silence(policy.auto_silence_beacons).await;

        let mut added_hosts = 0usize;
        for h in &policy.blocked_hosts {
            if radar.block_host(h, true).await {
                added_hosts += 1;
            }
        }
        let mut added_paths = 0usize;
        for rule in &policy.silenced_paths {
            if let Some((h, p)) = rule.split_once('|') {
                if radar.silence_path(h, p, true).await {
                    added_paths += 1;
                }
            }
        }
        (added_hosts, added_paths)
    }
}

/// Exact CRC-16/CCITT-FALSE (`0x1021`, init `0xFFFF`) matching `pqtrn-core::id1_pref`.
pub const fn crc16_ccitt(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    let mut i = 0;
    while i < data.len() {
        crc ^= (data[i] as u16) << 8;
        let mut b = 0;
        while b < 8 {
            if (crc & 0x8000) != 0 {
                crc = (crc << 1) ^ 0x1021;
            } else {
                crc <<= 1;
            }
            b += 1;
        }
        i += 1;
    }
    crc
}

/// Encode a canonical 16-byte `ID1P` binary signature for preset `"hue" | "bob" | "warm_comfort"`.
pub fn encode_id1p_preset(preset: &str) -> [u8; ID1P_LEN] {
    let (accent_theme, min_font, line_h, flags, width_ch, wpm): (u8, u8, u8, u8, u8, u16) =
        match normalize_preset_name(preset) {
            "bob" => (
                0x01, // Cyan(0) << 4 | SunlitLight(1)
                22,
                170,
                FLAG_STRICT_LANG_ONLY | FLAG_VETO_PURPLE | FLAG_HIGH_CONTRAST_BORDERS,
                68,
                145,
            ),
            "warm_comfort" => (
                0x14, // Emerald(1) << 4 | WarmPaper(4)
                24,
                180,
                FLAG_REDUCED_MOTION
                    | FLAG_VETO_PURPLE
                    | FLAG_DYSLEXIA_SPACING
                    | FLAG_CAPTIONS_ALWAYS,
                64,
                135,
            ),
            _ => (
                0x00, // Cyan(0) << 4 | ObsidianDark(0)
                16,
                155,
                FLAG_VETO_PURPLE,
                80,
                175,
            ),
        };

    let wpm_le = wpm.to_le_bytes();
    let mut buf = [
        ID1P_MAGIC[0],
        ID1P_MAGIC[1],
        ID1P_MAGIC[2],
        ID1P_MAGIC[3],
        ID1P_VERSION,
        accent_theme,
        min_font,
        line_h,
        b'e',
        b'n',
        flags,
        width_ch,
        wpm_le[0],
        wpm_le[1],
        0,
        0,
    ];
    let crc = crc16_ccitt(&buf[..14]).to_le_bytes();
    buf[14] = crc[0];
    buf[15] = crc[1];
    buf
}

pub fn encode_id1p_preset_hex(preset: &str) -> String {
    let raw = encode_id1p_preset(preset);
    let mut out = String::with_capacity(ID1P_LEN * 2);
    for b in raw {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

fn normalize_preset_name(raw: &str) -> &'static str {
    match raw.trim().to_ascii_lowercase().as_str() {
        "bob" | "sunlit" => "bob",
        "warm_comfort" | "warm" | "comfort" => "warm_comfort",
        _ => "hue",
    }
}

fn preset_summary(preset: &str) -> &'static str {
    match normalize_preset_name(preset) {
        "bob" => "Sunlit Light · Cyan · >=22px · Strict English · Zero-Purple · High-Contrast",
        "warm_comfort" => "Warm Paper · Emerald · >=24px · Reduced Motion · Dyslexia Spacing · Captions",
        _ => "Obsidian Dark · Cyan · >=16px · English · Zero-Purple Guard",
    }
}

fn sanitize_id1_handle(raw: &str) -> String {
    let s = raw.trim().to_ascii_lowercase();
    if s.is_empty() || s.len() > 64 {
        return String::new();
    }
    if s.bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'-' | b'_'))
    {
        s
    } else {
        String::new()
    }
}

fn derive_public_key_id(handle: &str, seed_hex: &str) -> String {
    let mat = format!("id1-pub-key-id-v1|{handle}|{seed_hex}");
    let h = blake3::hash(mat.as_bytes()).to_hex().to_string();
    format!("id1_{}", &h[..16])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_exact_16_byte_id1p_with_valid_crc16() {
        let raw = encode_id1p_preset("hue");
        assert_eq!(&raw[0..4], b"ID1P");
        assert_eq!(raw[4], 1);
        assert_eq!(raw[6], 16);
        assert_eq!(raw[10], FLAG_VETO_PURPLE);
        let expected_crc = crc16_ccitt(&raw[..14]).to_le_bytes();
        assert_eq!(&raw[14..16], &expected_crc);
        assert_eq!(encode_id1p_preset_hex("hue").len(), 32);
    }
}
