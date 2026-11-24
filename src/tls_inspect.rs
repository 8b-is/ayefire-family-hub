#![deny(clippy::float_arithmetic)]
//! AyeFire Tier 3 Opt-In Hardware-Local HTTPS Inspection, BLAKE3 HTTPS Edge Caching,
//! and On-the-Fly Zero-Float Phoenix Audio Stream Interception.
//!
//! Security & Sovereignty Invariants (`ayefire_onboard.rs` §3):
//! 1. **Hardware-Local Only**: Root CA key (`~/.config/ayefire/ca/ayefire-local-ca.key`,
//!    mode `0600`) lives strictly on this box, never backed by `i1.is`, never in Git,
//!    never synced to cloud storage.
//! 2. **Opt-In with Instant Incognito Bypass**: Runtime toggleable (`inspect_enabled`)
//!    without restart; OFF by default until explicitly enabled.
//! 3. **Zero Upstream TLS Weakening**: Upstream connections always validate real webpki
//!    certificates via `reqwest` (`rustls-tls`); bad/expired upstream certs are rejected.
//! 4. **Honest Privacy Boundaries**: Banking, tax, and password-manager domains bypass
//!    decryption automatically when `privacy_bypass_enabled` is ON; authenticated requests
//!    (`Authorization` / `Cookie`) are never cached to disk and never logged in full.
//! 5. **On-the-Fly Zero-Float Phoenix Audio Upgrade**: When `auto_phoenix_audio` is ON,
//!    public HTTPS `audio/*` streams (radio, podcasts, MP3/FLAC/OGG/AAC tracks) pass live
//!    through `PhoenixEnginePq` and cache on NVMe by BLAKE3 hash.

use crate::edge_cache::{CacheResponseHeaders, EdgeCache};
use crate::id1_transparency::Id1TransparencyBridge;
use crate::phoenix_gateway::{format_reduced_pq, PhoenixGateway, StreamOptions};
use crate::transparency_radar::{extract_referer_host, TrafficObservation, TransparencyRadar};
use futures::TryStreamExt;
use http_body_util::{BodyExt, Full, StreamBody};
use hyper::body::{Bytes, Frame, Incoming};
use hyper::header::{HeaderName, HeaderValue};
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, IsCa,
    Issuer, KeyPair, KeyUsagePurpose, SanType, PKCS_ECDSA_P256_SHA256,
};
use rustls::pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::net::IpAddr;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};
use tokio::net::TcpStream;
use tokio::sync::RwLock;
use tokio_rustls::TlsAcceptor;

/// Maximum cached leaf TLS `ServerConfig`s in RAM (C64 bounded).
const MAX_CACHED_HOST_CERTS: usize = 512;
/// Leaf certificate lifetime: 48 hours.
const LEAF_LIFETIME_SECS: u64 = 172_800;
/// Clock skew safety margin on leaf `not_before`.
const CLOCK_SKEW_SECS: u64 = 300;
/// Max single HTTPS response cached on NVMe (32 MiB).
const MAX_HTTPS_CACHE_BYTES: usize = 32 * 1024 * 1024;

/// Financial, government tax, and password-manager suffixes bypassed when `privacy_bypass_enabled` is ON.
const PRIVACY_BYPASS_SUFFIXES: &[&str] = &[
    "chase.com",
    "bankofamerica.com",
    "wellsfargo.com",
    "citibank.com",
    "citi.com",
    "capitalone.com",
    "americanexpress.com",
    "fidelity.com",
    "schwab.com",
    "vanguard.com",
    "paypal.com",
    "venmo.com",
    "stripe.com",
    "1password.com",
    "bitwarden.com",
    "lastpass.com",
    "dashlane.com",
    "irs.gov",
    "ssa.gov",
    "login.gov",
    "accounts.google.com",
    "appleid.apple.com",
    "login.microsoftonline.com",
    "login.live.com",
    "id1.is",
];

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PersistedInspectConfig {
    pub inspect_enabled: bool,
    pub auto_phoenix_audio: bool,
    pub privacy_bypass_enabled: bool,
    pub sinkhole_connect_enabled: bool,
    pub phoenix_preset: String,
    pub phoenix_rate: u32,
}

impl Default for PersistedInspectConfig {
    fn default() -> Self {
        Self {
            inspect_enabled: false,
            auto_phoenix_audio: true,
            privacy_bypass_enabled: true,
            sinkhole_connect_enabled: true,
            phoenix_preset: "natural".to_string(),
            phoenix_rate: 48_000,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct InspectEvent {
    pub host: String,
    pub method: String,
    pub path_preview: String,
    pub status: u16,
    pub action: String,
    pub content_type: String,
    pub size_bytes: u64,
    pub unix_secs: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct TlsInspectStatus {
    pub inspect_enabled: bool,
    pub auto_phoenix_audio: bool,
    pub privacy_bypass_enabled: bool,
    pub sinkhole_connect_enabled: bool,
    pub phoenix_preset: String,
    pub phoenix_rate: u32,
    pub ca_common_name: String,
    pub ca_cert_path: String,
    pub ca_blake3_fingerprint: String,
    pub ca_download_url: String,
    pub firefox_cert_installed: bool,
    pub leaf_certs_cached: usize,
    pub connect_blocked: u64,
    pub https_tunnels_inspected: u64,
    pub https_requests_inspected: u64,
    pub https_cache_hits: u64,
    pub https_cache_misses: u64,
    pub https_hit_ratio_pq: String,
    pub https_phoenix_restored: u64,
    pub https_privacy_bypassed: u64,
    pub https_wan_bytes_saved: u64,
    pub recent: Vec<InspectEvent>,
}

struct CachedHostConfig {
    config: Arc<rustls::ServerConfig>,
    expires_unix: u64,
}

pub struct TlsInspectState {
    pub ca_cert_path: PathBuf,
    pub config_path: PathBuf,
    pub ca_common_name: String,
    pub ca_cert_pem: String,
    pub ca_cert_der: Vec<u8>,
    pub ca_blake3_fingerprint: String,
    ca_key_pem: String,
    leaf_cache: RwLock<HashMap<String, CachedHostConfig>>,
    upstream_client: reqwest::Client,
    pub inspect_enabled: AtomicBool,
    pub auto_phoenix_audio: AtomicBool,
    pub privacy_bypass_enabled: AtomicBool,
    pub sinkhole_connect_enabled: AtomicBool,
    pub phoenix_preset: RwLock<String>,
    pub phoenix_rate: AtomicU64,
    pub connect_blocked: AtomicU64,
    pub https_tunnels_inspected: AtomicU64,
    pub https_requests_inspected: AtomicU64,
    pub https_cache_hits: AtomicU64,
    pub https_cache_misses: AtomicU64,
    pub https_phoenix_restored: AtomicU64,
    pub https_privacy_bypassed: AtomicU64,
    pub https_wan_bytes_saved: AtomicU64,
    pub recent: RwLock<VecDeque<InspectEvent>>,
}

impl TlsInspectState {
    pub fn load_or_create_from_env() -> anyhow::Result<Arc<Self>> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let home = std::env::var("HOME").unwrap_or_else(|_| "/home/hue".to_string());
        let cfg_root = std::env::var("AYEFIRE_CONFIG_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(&home).join(".config/ayefire"));
        let ca_dir = cfg_root.join("ca");
        fs::create_dir_all(&ca_dir)?;
        let _ = fs::set_permissions(&ca_dir, fs::Permissions::from_mode(0o700));

        let ca_key_path = ca_dir.join("ayefire-local-ca.key");
        let ca_cert_path = ca_dir.join("ayefire-local-ca.crt");
        let config_path = cfg_root.join("inspect-config.json");

        let host_label = fs::read_to_string("/etc/hostname")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "ayefire".to_string());
        let ca_common_name = format!("AyeFire Local Tier-3 Inspect CA ({host_label})");

        let (ca_key_pem, ca_cert_pem, ca_cert_der) =
            load_or_generate_local_ca(&ca_key_path, &ca_cert_path, &ca_common_name)?;
        let ca_blake3_fingerprint = blake3::hash(&ca_cert_der).to_hex().to_string();

        let persisted: PersistedInspectConfig = fs::read_to_string(&config_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

        // Upstream HTTP/HTTPS client: real verified TLS roots, NO auto-redirect, NO auto-decompress,
        // and connect/read timeouts (never a short total-body timeout that cuts off long video/SSE streams!).
        let upstream_client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(15))
            .read_timeout(Duration::from_secs(60))
            .build()?;

        println!(
            "[TIER3-INSPECT] Local CA ready ({}) | inspect_enabled={} | auto_phoenix={} | privacy_bypass={} | blake3={}..",
            ca_cert_path.display(),
            persisted.inspect_enabled,
            persisted.auto_phoenix_audio,
            persisted.privacy_bypass_enabled,
            &ca_blake3_fingerprint[..16]
        );

        Ok(Arc::new(Self {
            ca_cert_path,
            config_path,
            ca_common_name,
            ca_cert_pem,
            ca_cert_der,
            ca_blake3_fingerprint,
            ca_key_pem,
            leaf_cache: RwLock::new(HashMap::new()),
            upstream_client,
            inspect_enabled: AtomicBool::new(persisted.inspect_enabled),
            auto_phoenix_audio: AtomicBool::new(persisted.auto_phoenix_audio),
            privacy_bypass_enabled: AtomicBool::new(persisted.privacy_bypass_enabled),
            sinkhole_connect_enabled: AtomicBool::new(persisted.sinkhole_connect_enabled),
            phoenix_preset: RwLock::new(persisted.phoenix_preset),
            phoenix_rate: AtomicU64::new(persisted.phoenix_rate as u64),
            connect_blocked: AtomicU64::new(0),
            https_tunnels_inspected: AtomicU64::new(0),
            https_requests_inspected: AtomicU64::new(0),
            https_cache_hits: AtomicU64::new(0),
            https_cache_misses: AtomicU64::new(0),
            https_phoenix_restored: AtomicU64::new(0),
            https_privacy_bypassed: AtomicU64::new(0),
            https_wan_bytes_saved: AtomicU64::new(0),
            recent: RwLock::new(VecDeque::with_capacity(32)),
        }))
    }

    pub async fn update_policy(
        &self,
        inspect_enabled: Option<bool>,
        auto_phoenix_audio: Option<bool>,
        privacy_bypass_enabled: Option<bool>,
        sinkhole_connect_enabled: Option<bool>,
        phoenix_preset: Option<String>,
        phoenix_rate: Option<u32>,
    ) -> PersistedInspectConfig {
        if let Some(v) = inspect_enabled {
            self.inspect_enabled.store(v, Ordering::Relaxed);
        }
        if let Some(v) = auto_phoenix_audio {
            self.auto_phoenix_audio.store(v, Ordering::Relaxed);
        }
        if let Some(v) = privacy_bypass_enabled {
            self.privacy_bypass_enabled.store(v, Ordering::Relaxed);
        }
        if let Some(v) = sinkhole_connect_enabled {
            self.sinkhole_connect_enabled.store(v, Ordering::Relaxed);
        }
        if let Some(p) = phoenix_preset {
            let clean = p.trim().to_ascii_lowercase();
            if !clean.is_empty() && clean.len() <= 24 {
                *self.phoenix_preset.write().await = clean;
            }
        }
        if let Some(r) = phoenix_rate {
            let normalized = match r {
                96_000 => 96_000,
                192_000 => 192_000,
                384_000 => 384_000,
                _ => 48_000,
            };
            self.phoenix_rate.store(normalized as u64, Ordering::Relaxed);
        }

        let cfg = PersistedInspectConfig {
            inspect_enabled: self.inspect_enabled.load(Ordering::Relaxed),
            auto_phoenix_audio: self.auto_phoenix_audio.load(Ordering::Relaxed),
            privacy_bypass_enabled: self.privacy_bypass_enabled.load(Ordering::Relaxed),
            sinkhole_connect_enabled: self.sinkhole_connect_enabled.load(Ordering::Relaxed),
            phoenix_preset: self.phoenix_preset.read().await.clone(),
            phoenix_rate: self.phoenix_rate.load(Ordering::Relaxed) as u32,
        };
        if let Ok(json) = serde_json::to_string_pretty(&cfg) {
            let _ = fs::write(&self.config_path, json);
            let _ = fs::set_permissions(&self.config_path, fs::Permissions::from_mode(0o600));
        }
        cfg
    }

    pub async fn status(&self) -> TlsInspectStatus {
        let hits = self.https_cache_hits.load(Ordering::Relaxed);
        let misses = self.https_cache_misses.load(Ordering::Relaxed);
        let leaf_certs_cached = self.leaf_cache.read().await.len();
        let recent = self.recent.read().await.iter().cloned().collect();
        let firefox_cert_installed = check_firefox_ca_installed(&self.ca_common_name);

        TlsInspectStatus {
            inspect_enabled: self.inspect_enabled.load(Ordering::Relaxed),
            auto_phoenix_audio: self.auto_phoenix_audio.load(Ordering::Relaxed),
            privacy_bypass_enabled: self.privacy_bypass_enabled.load(Ordering::Relaxed),
            sinkhole_connect_enabled: self.sinkhole_connect_enabled.load(Ordering::Relaxed),
            phoenix_preset: self.phoenix_preset.read().await.clone(),
            phoenix_rate: self.phoenix_rate.load(Ordering::Relaxed) as u32,
            ca_common_name: self.ca_common_name.clone(),
            ca_cert_path: self.ca_cert_path.display().to_string(),
            ca_blake3_fingerprint: self.ca_blake3_fingerprint.clone(),
            ca_download_url: "/api/gateway/ca.crt".to_string(),
            firefox_cert_installed,
            leaf_certs_cached,
            connect_blocked: self.connect_blocked.load(Ordering::Relaxed),
            https_tunnels_inspected: self.https_tunnels_inspected.load(Ordering::Relaxed),
            https_requests_inspected: self.https_requests_inspected.load(Ordering::Relaxed),
            https_cache_hits: hits,
            https_cache_misses: misses,
            https_hit_ratio_pq: format_reduced_pq(hits, hits.saturating_add(misses)),
            https_phoenix_restored: self.https_phoenix_restored.load(Ordering::Relaxed),
            https_privacy_bypassed: self.https_privacy_bypassed.load(Ordering::Relaxed),
            https_wan_bytes_saved: self.https_wan_bytes_saved.load(Ordering::Relaxed),
            recent,
        }
    }

    pub async fn record_event(&self, ev: InspectEvent) {
        let mut q = self.recent.write().await;
        if q.len() >= 32 {
            q.pop_back();
        }
        q.push_front(ev);
    }

    /// Mint or reuse a short-lived (48h) ECDSA P-256 TLS `ServerConfig` for `host`.
    pub async fn server_config_for_host(&self, host: &str) -> anyhow::Result<Arc<rustls::ServerConfig>> {
        let key = host.trim().to_ascii_lowercase();
        let now = crate::pdf_forms::now_unix_secs();

        {
            let guard = self.leaf_cache.read().await;
            if let Some(entry) = guard.get(&key) {
                if entry.expires_unix > now.saturating_add(600) {
                    return Ok(Arc::clone(&entry.config));
                }
            }
        }

        let (cfg, expires_unix) = mint_host_server_config(
            &key,
            &self.ca_common_name,
            &self.ca_key_pem,
            &self.ca_cert_der,
            now,
        )?;
        let arc_cfg = Arc::new(cfg);

        {
            let mut guard = self.leaf_cache.write().await;
            if guard.len() >= MAX_CACHED_HOST_CERTS {
                guard.retain(|_, v| v.expires_unix > now);
                if guard.len() >= MAX_CACHED_HOST_CERTS {
                    guard.clear();
                }
            }
            guard.insert(
                key,
                CachedHostConfig {
                    config: Arc::clone(&arc_cfg),
                    expires_unix,
                },
            );
        }

        Ok(arc_cfg)
    }
}

pub fn is_privacy_sensitive_host(host: &str) -> bool {
    let low = host.trim().trim_end_matches('.').to_ascii_lowercase();
    for suffix in PRIVACY_BYPASS_SUFFIXES {
        if low == *suffix || low.ends_with(&format!(".{suffix}")) {
            return true;
        }
    }
    false
}

/// Only upgrade standalone direct audio streams (MP3/OGG/AAC/FLAC radio & podcasts).
/// NEVER intercept MSE/DASH/HLS fragmented `audio/mp4`, `audio/webm`, `video/*`, or
/// XHR/fetch chunks (`Sec-Fetch-Dest: empty`) used by Reddit (`v.redd.it`) & YouTube players!
pub fn is_phoenix_upgradable_audio(
    content_type: &str,
    host: &str,
    path_only: &str,
    sec_fetch_dest: &str,
) -> bool {
    if sec_fetch_dest.eq_ignore_ascii_case("empty") || sec_fetch_dest.eq_ignore_ascii_case("video") {
        return false;
    }
    let h = host.to_ascii_lowercase();
    if h.ends_with("v.redd.it")
        || h.ends_with("packaged-media.redd.it")
        || h.ends_with("googlevideo.com")
        || h.ends_with("ttvnw.net")
    {
        return false;
    }
    let p = path_only.to_ascii_lowercase();
    if p.contains("dash_")
        || p.contains("cmaf_")
        || p.ends_with(".m4s")
        || p.ends_with(".m4a")
        || p.ends_with(".mp4")
        || p.ends_with(".ts")
        || p.ends_with(".m3u8")
        || p.ends_with(".mpd")
    {
        return false;
    }
    let ct = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    matches!(
        ct.as_str(),
        "audio/mpeg" | "audio/mp3" | "audio/ogg" | "audio/aac" | "audio/aacp" | "audio/flac" | "audio/x-flac"
    )
}

fn is_streaming_media_type(content_type: &str) -> bool {
    let ct = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    ct.starts_with("video/")
        || ct == "audio/mp4"
        || ct == "audio/webm"
        || ct == "audio/mpegurl"
        || ct == "application/vnd.apple.mpegurl"
        || ct == "application/x-mpegurl"
        || ct == "application/dash+xml"
        || ct == "text/event-stream"
}

fn is_streaming_media_host_or_path(host: &str, path_only: &str) -> bool {
    let h = host.to_ascii_lowercase();
    if h.ends_with("v.redd.it")
        || h.ends_with("packaged-media.redd.it")
        || h.ends_with("googlevideo.com")
        || h.ends_with("ttvnw.net")
    {
        return true;
    }
    let p = path_only.to_ascii_lowercase();
    p.ends_with(".m3u8")
        || p.ends_with(".mpd")
        || p.ends_with(".m4s")
        || p.ends_with(".mp4")
        || p.ends_with(".ts")
        || p.ends_with(".webm")
}

/// Terminate client TLS with a hardware-local leaf cert and serve RFC-compliant HTTP/1.1
/// keep-alive requests with CORS-safe BLAKE3 HTTPS caching, zero-buffer media streaming,
/// `id1.is` preference signaling, & standalone Phoenix audio upgrade.
#[allow(clippy::too_many_arguments)]
pub async fn serve_inspected_tls_tunnel(
    client_stream: TcpStream,
    host: String,
    port: u16,
    inspect: Arc<TlsInspectState>,
    edge_cache: Arc<EdgeCache>,
    phoenix: Arc<PhoenixGateway>,
    radar: Arc<TransparencyRadar>,
    id1: Arc<Id1TransparencyBridge>,
) -> anyhow::Result<()> {
    let server_cfg = inspect.server_config_for_host(&host).await?;
    let acceptor = TlsAcceptor::from(server_cfg);
    let tls_stream = acceptor.accept(client_stream).await?;
    inspect
        .https_tunnels_inspected
        .fetch_add(1, Ordering::Relaxed);

    let io = TokioIo::new(tls_stream);
    let host_arc = Arc::new(host);

    let service = service_fn(move |req: Request<Incoming>| {
        let host_c = Arc::clone(&host_arc);
        let insp_c = Arc::clone(&inspect);
        let cache_c = Arc::clone(&edge_cache);
        let phx_c = Arc::clone(&phoenix);
        let rad_c = Arc::clone(&radar);
        let id1_c = Arc::clone(&id1);
        async move {
            Ok::<_, std::convert::Infallible>(
                handle_inspected_http_request(
                    req, &host_c, port, insp_c, cache_c, phx_c, rad_c, id1_c,
                )
                .await,
            )
        }
    });

    let _ = hyper::server::conn::http1::Builder::new()
        .preserve_header_case(true)
        .serve_connection(io, service)
        .await;

    Ok(())
}

type BoxedBody = http_body_util::combinators::BoxBody<Bytes, std::io::Error>;

fn full_body(data: Vec<u8>) -> BoxedBody {
    Full::new(Bytes::from(data))
        .map_err(|never| match never {})
        .boxed()
}

fn error_response(status: StatusCode, msg: String) -> Response<BoxedBody> {
    Response::builder()
        .status(status)
        .header("Content-Type", "text/plain; charset=utf-8")
        .header("X-AyeFire-Inspect", "TIER3-LOCAL")
        .body(full_body(msg.into_bytes()))
        .unwrap_or_else(|_| Response::new(full_body(Vec::new())))
}

#[allow(clippy::too_many_arguments)]
async fn handle_inspected_http_request(
    req: Request<Incoming>,
    host: &str,
    port: u16,
    inspect: Arc<TlsInspectState>,
    edge_cache: Arc<EdgeCache>,
    phoenix: Arc<PhoenixGateway>,
    radar: Arc<TransparencyRadar>,
    id1: Arc<Id1TransparencyBridge>,
) -> Response<BoxedBody> {
    inspect
        .https_requests_inspected
        .fetch_add(1, Ordering::Relaxed);

    let method = req.method().clone();
    let path_and_query = req
        .uri()
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or("/");
    let path_only = req.uri().path().to_string();
    let referer_host = req
        .headers()
        .get(hyper::header::REFERER)
        .or_else(|| req.headers().get(hyper::header::ORIGIN))
        .and_then(|v| v.to_str().ok())
        .and_then(extract_referer_host);
    let has_origin = req.headers().contains_key(hyper::header::ORIGIN);
    let sec_fetch_dest = req
        .headers()
        .get("sec-fetch-dest")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    // 0. Instant Beacon-Path Silencing (drops mouse-move / telemetry beacons locally with 204
    //    so the parent site keeps working 100% normally without leaking behavioral packets!)
    if radar
        .should_silence_path(host, method.as_str(), &path_only)
        .await
    {
        radar.record_silenced(host, &path_only).await;
        inspect
            .record_event(InspectEvent {
                host: host.to_string(),
                method: method.to_string(),
                path_preview: truncate_path(&path_only, 60),
                status: 204,
                action: "BEACON-SILENCED".to_string(),
                content_type: "telemetry/silenced".to_string(),
                size_bytes: 0,
                unix_secs: crate::pdf_forms::now_unix_secs(),
            })
            .await;
        return Response::builder()
            .status(StatusCode::NO_CONTENT)
            .header("X-AyeFire-Inspect", "TIER3-LOCAL")
            .header("X-AyeFire-Silenced", "BEACON-DROPPED")
            .body(full_body(Vec::new()))
            .unwrap_or_else(|_| Response::new(full_body(Vec::new())));
    }
    let full_url = if port == 443 {
        format!("https://{host}{path_and_query}")
    } else {
        format!("https://{host}:{port}{path_and_query}")
    };

    let has_auth_or_cookie = req.headers().contains_key(hyper::header::AUTHORIZATION)
        || req.headers().contains_key(hyper::header::COOKIE);
    let has_range = req.headers().contains_key(hyper::header::RANGE);
    let has_sensitive_query = url_has_sensitive_query(path_and_query);
    let is_streaming_req = is_streaming_media_host_or_path(host, &path_only);
    let cache_eligible_req = method == Method::GET
        && !has_auth_or_cookie
        && !has_range
        && !has_sensitive_query
        && !is_streaming_req;
    let https_cache_key = EdgeCache::key_for_url(&format!("https-v1|{full_url}"));

    // 1. Check CORS-safe BLAKE3 NVMe Edge Cache for public cacheable HTTPS GET
    if cache_eligible_req {
        if let Some((meta, body)) = edge_cache.load_cached(&https_cache_key) {
            // CORS safety: if caller sent Origin: and cached entry lacks Access-Control-Allow-Origin,
            // fall through to live fetch so cross-origin video/HLS/JS never fails CORS!
            let cors_ok = !has_origin || meta.access_control_allow_origin.is_some();
            if cors_ok && !is_streaming_media_type(&meta.content_type) {
                inspect.https_cache_hits.fetch_add(1, Ordering::Relaxed);
                inspect
                    .https_wan_bytes_saved
                    .fetch_add(meta.size_bytes, Ordering::Relaxed);
                edge_cache.hits.fetch_add(1, Ordering::Relaxed);
                edge_cache
                    .wan_bytes_saved
                    .fetch_add(meta.size_bytes, Ordering::Relaxed);

                inspect
                    .record_event(InspectEvent {
                        host: host.to_string(),
                        method: method.to_string(),
                        path_preview: truncate_path(&path_only, 60),
                        status: 200,
                        action: "HTTPS-CACHE-HIT".to_string(),
                        content_type: meta.content_type.clone(),
                        size_bytes: meta.size_bytes,
                        unix_secs: crate::pdf_forms::now_unix_secs(),
                    })
                    .await;

                let mut b = Response::builder()
                    .status(StatusCode::OK)
                    .header("Content-Type", meta.content_type)
                    .header("X-AyeFire-Inspect", "TIER3-LOCAL")
                    .header("X-AyeFire-Cache", "HTTPS-HIT")
                    .header("X-AyeFire-Blake3", meta.content_blake3);
                if let Some(enc) = meta.content_encoding {
                    b = b.header("Content-Encoding", enc);
                }
                if let Some(acao) = meta.access_control_allow_origin {
                    b = b.header("Access-Control-Allow-Origin", acao);
                }
                if let Some(aceh) = meta.access_control_expose_headers {
                    b = b.header("Access-Control-Expose-Headers", aceh);
                }
                return b
                    .body(full_body(body))
                    .unwrap_or_else(|_| error_response(StatusCode::INTERNAL_SERVER_ERROR, "build err".into()));
            }
        }
    }

    // 2. Forward to real upstream HTTPS server (verified TLS roots!)
    let (parts, incoming_body) = req.into_parts();
    let req_bytes = match incoming_body.collect().await {
        Ok(c) => c.to_bytes(),
        Err(e) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                format!("AyeFire Tier-3 request body error: {e}"),
            );
        }
    };

    let Ok(req_method) = reqwest::Method::from_bytes(method.as_str().as_bytes()) else {
        return error_response(StatusCode::BAD_REQUEST, "unsupported HTTP method".into());
    };
    let mut up_req = inspect.upstream_client.request(req_method, &full_url);
    for (name, value) in &parts.headers {
        let low = name.as_str();
        if matches!(
            low,
            "host"
                | "connection"
                | "proxy-connection"
                | "keep-alive"
                | "transfer-encoding"
                | "content-length"
        ) {
            continue;
        }
        up_req = up_req.header(name.as_str(), value.as_bytes());
    }

    // Inject id1.is Sovereign Transparency & Presentation signals (never breaks CORS preflights!)
    if method == Method::GET {
        let is_doc_or_sovereign = sec_fetch_dest.eq_ignore_ascii_case("document")
            || host.ends_with(".8b.is")
            || host == "8b.is"
            || host.ends_with(".i1.is")
            || host.ends_with(".8a.is")
            || host.ends_with(".pqtrn.com")
            || host == "pqtrn.com";
        let allow_id1p = is_doc_or_sovereign && !parts.headers.contains_key("x-id1-pref");
        let (send_gpc, opt_id1p) = id1.outbound_signal_headers(allow_id1p).await;
        if send_gpc && !parts.headers.contains_key("sec-gpc") {
            up_req = up_req.header("Sec-GPC", "1");
        }
        if let Some(id1p_hex) = opt_id1p {
            up_req = up_req.header("X-ID1-Pref", id1p_hex);
        }
    }

    let req_bytes_len = req_bytes.len() as u64;
    if !req_bytes.is_empty() {
        up_req = up_req.body(req_bytes.to_vec());
    }

    let up_resp = match up_req.send().await {
        Ok(r) => r,
        Err(e) => {
            // Offline resilience: serve stale BLAKE3 cache if upstream is unreachable!
            if cache_eligible_req {
                if let Some((meta, body)) = edge_cache.load_cached(&https_cache_key) {
                    edge_cache.offline_serves.fetch_add(1, Ordering::Relaxed);
                    let mut b = Response::builder()
                        .status(StatusCode::OK)
                        .header("Content-Type", meta.content_type)
                        .header("X-AyeFire-Inspect", "TIER3-LOCAL")
                        .header("X-AyeFire-Cache", "OFFLINE-HIT");
                    if let Some(enc) = meta.content_encoding {
                        b = b.header("Content-Encoding", enc);
                    }
                    if let Some(acao) = meta.access_control_allow_origin {
                        b = b.header("Access-Control-Allow-Origin", acao);
                    }
                    return b
                        .body(full_body(body))
                        .unwrap_or_else(|_| error_response(StatusCode::BAD_GATEWAY, "offline err".into()));
                }
            }
            return error_response(
                StatusCode::BAD_GATEWAY,
                format!("AyeFire Tier-3 upstream HTTPS failed: {e}"),
            );
        }
    };

    let status_u16 = up_resp.status().as_u16();
    let status = StatusCode::from_u16(status_u16).unwrap_or(StatusCode::OK);
    let content_type = up_resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();
    let content_encoding = up_resp
        .headers()
        .get(reqwest::header::CONTENT_ENCODING)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let acao_val = up_resp
        .headers()
        .get(reqwest::header::ACCESS_CONTROL_ALLOW_ORIGIN)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let aceh_val = up_resp
        .headers()
        .get(reqwest::header::ACCESS_CONTROL_EXPOSE_HEADERS)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let vary_origin = up_resp
        .headers()
        .get(reqwest::header::VARY)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_ascii_lowercase().contains("origin"))
        .unwrap_or(false);
    let cache_control = up_resp
        .headers()
        .get(reqwest::header::CACHE_CONTROL)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    let has_set_cookie = up_resp.headers().contains_key(reqwest::header::SET_COOKIE);
    let upstream_content_len = up_resp.content_length();

    if up_resp.headers().contains_key("x-id1-pref-ack") {
        id1.record_site_honored_id1(host).await;
    }

    // 3. On-the-Fly Zero-Float Phoenix Audio Upgrade (strictly standalone audio streams; NEVER MSE video!)
    if inspect.auto_phoenix_audio.load(Ordering::Relaxed)
        && method == Method::GET
        && status == StatusCode::OK
        && !has_range
        && !has_auth_or_cookie
        && is_phoenix_upgradable_audio(&content_type, host, &path_only, &sec_fetch_dest)
    {
        let preset_name = inspect.phoenix_preset.read().await.clone();
        let rate = inspect.phoenix_rate.load(Ordering::Relaxed) as u32;
        let opts = StreamOptions::parse(Some(&preset_name), None, Some(rate), Some(24), true);
        let phx_key = phoenix.cache_key(&format!("https-audio:{full_url}"), opts);

        if let Some((wav_path, meta)) = phoenix.lookup_cached(&phx_key) {
            if let Ok(wav_bytes) = tokio::fs::read(&wav_path).await {
                let sz = wav_bytes.len() as u64;
                phoenix
                    .record_cache_hit(&full_url, opts, sz, meta.as_ref())
                    .await;
                inspect.https_phoenix_restored.fetch_add(1, Ordering::Relaxed);
                inspect.https_wan_bytes_saved.fetch_add(sz, Ordering::Relaxed);
                inspect
                    .record_event(InspectEvent {
                        host: host.to_string(),
                        method: "GET".to_string(),
                        path_preview: truncate_path(&path_only, 60),
                        status: 200,
                        action: "PHOENIX-HIT".to_string(),
                        content_type: "audio/wav".to_string(),
                        size_bytes: sz,
                        unix_secs: crate::pdf_forms::now_unix_secs(),
                    })
                    .await;
                drop(up_resp);
                return Response::builder()
                    .status(StatusCode::OK)
                    .header("Content-Type", "audio/wav")
                    .header("X-AyeFire-Inspect", "TIER3-LOCAL")
                    .header("X-AyeFire-Phoenix", "HIT-RESTORED")
                    .header("X-AyeFire-Phoenix-Preset", opts.preset_label())
                    .header("X-AyeFire-Phoenix-Rate", opts.sample_rate.to_string())
                    .body(full_body(wav_bytes))
                    .unwrap_or_else(|_| error_response(StatusCode::INTERNAL_SERVER_ERROR, "wav err".into()));
            }
        }

        // Cache miss -> stream live through zero-float PhoenixEnginePq!
        drop(up_resp);
        if let Ok(rx) = phoenix.spawn_live_stream(
            full_url.clone(),
            format!("https://{host}{}", truncate_path(&path_only, 48)),
            opts,
            Some(phx_key),
        ) {
            inspect.https_phoenix_restored.fetch_add(1, Ordering::Relaxed);
            inspect
                .record_event(InspectEvent {
                    host: host.to_string(),
                    method: "GET".to_string(),
                    path_preview: truncate_path(&path_only, 60),
                    status: 200,
                    action: "PHOENIX-LIVE".to_string(),
                    content_type: "audio/wav".to_string(),
                    size_bytes: 0,
                    unix_secs: crate::pdf_forms::now_unix_secs(),
                })
                .await;
            let stream = futures::stream::unfold(rx, |mut r| async move {
                r.recv()
                    .await
                    .map(|chunk| (Ok::<Frame<Bytes>, std::io::Error>(Frame::data(Bytes::from(chunk))), r))
            });
            let body: BoxedBody = StreamBody::new(stream).boxed();
            return Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "audio/wav")
                .header("Cache-Control", "no-store")
                .header("X-AyeFire-Inspect", "TIER3-LOCAL")
                .header("X-AyeFire-Phoenix", "LIVE-DSP-RESTORED")
                .header("X-AyeFire-Phoenix-Preset", opts.preset_label())
                .header("X-AyeFire-Phoenix-Rate", opts.sample_rate.to_string())
                .body(body)
                .unwrap_or_else(|_| error_response(StatusCode::INTERNAL_SERVER_ERROR, "stream err".into()));
        }
        return error_response(StatusCode::BAD_GATEWAY, "Phoenix audio upgrade failed".into());
    }

    // 4. Decide Zero-Buffer Streaming vs Small Cacheable Asset Buffering!
    let resp_private = cache_control.contains("private") || cache_control.contains("no-store");
    let cors_cache_safe = match acao_val.as_deref() {
        Some("*") => true,
        Some(_) => false, // origin-specific ACAO must never be served to other origins
        None => !has_origin && !vary_origin,
    };
    let is_media_or_stream = has_range
        || is_streaming_req
        || status == StatusCode::PARTIAL_CONTENT
        || is_streaming_media_type(&content_type)
        || upstream_content_len.is_none_or(|len| len > 4 * 1024 * 1024);

    let can_cache_small = cache_eligible_req
        && status == StatusCode::OK
        && !resp_private
        && !has_set_cookie
        && cors_cache_safe
        && !is_media_or_stream;

    // 4a. Zero-Buffer Streaming Path (video/*, HLS/DASH, Range 206, SSE, >4MiB, POST, CORS-specific)
    if !can_cache_small {
        let mut resp_builder = Response::builder()
            .status(status)
            .header("X-AyeFire-Inspect", "TIER3-LOCAL")
            .header("X-AyeFire-Cache", "STREAM");

        for (name, val) in up_resp.headers() {
            let low = name.as_str();
            if matches!(
                low,
                "connection"
                    | "keep-alive"
                    | "transfer-encoding"
                    | "proxy-authenticate"
                    | "proxy-authorization"
                    | "te"
                    | "trailers"
                    | "upgrade"
                    | "alt-svc"
            ) {
                continue;
            }
            if let (Ok(hn), Ok(hv)) = (
                HeaderName::from_bytes(name.as_str().as_bytes()),
                HeaderValue::from_bytes(val.as_bytes()),
            ) {
                resp_builder = resp_builder.header(hn, hv);
            }
        }

        let est_bytes = upstream_content_len.unwrap_or(0);
        radar
            .observe_request(TrafficObservation {
                host: host.to_string(),
                referer_host,
                method: method.to_string(),
                path_only: path_only.clone(),
                status: status_u16,
                req_bytes: req_bytes_len,
                resp_bytes: est_bytes,
                content_type: content_type.clone(),
            })
            .await;

        inspect
            .record_event(InspectEvent {
                host: host.to_string(),
                method: method.to_string(),
                path_preview: truncate_path(&path_only, 60),
                status: status_u16,
                action: if is_streaming_media_type(&content_type) || status == StatusCode::PARTIAL_CONTENT {
                    "MEDIA-STREAM".to_string()
                } else {
                    "PASS".to_string()
                },
                content_type,
                size_bytes: est_bytes,
                unix_secs: crate::pdf_forms::now_unix_secs(),
            })
            .await;

        let stream = up_resp
            .bytes_stream()
            .map_ok(Frame::data)
            .map_err(|e| std::io::Error::other(e.to_string()));
        let body: BoxedBody = StreamBody::new(stream).boxed();
        return resp_builder
            .body(body)
            .unwrap_or_else(|_| error_response(StatusCode::INTERNAL_SERVER_ERROR, "stream build err".into()));
    }

    // 4b. Cacheable Public Small Asset Path (images, JS, CSS, fonts, JSON, PDFs <= 4 MiB)
    let mut resp_builder = Response::builder()
        .status(status)
        .header("X-AyeFire-Inspect", "TIER3-LOCAL");

    for (name, val) in up_resp.headers() {
        let low = name.as_str();
        if matches!(
            low,
            "connection"
                | "keep-alive"
                | "transfer-encoding"
                | "content-length"
                | "proxy-authenticate"
                | "proxy-authorization"
                | "te"
                | "trailers"
                | "upgrade"
                | "alt-svc"
        ) {
            continue;
        }
        if let (Ok(hn), Ok(hv)) = (
            HeaderName::from_bytes(name.as_str().as_bytes()),
            HeaderValue::from_bytes(val.as_bytes()),
        ) {
            resp_builder = resp_builder.header(hn, hv);
        }
    }

    let resp_bytes = match up_resp.bytes().await {
        Ok(b) => b.to_vec(),
        Err(e) => {
            return error_response(
                StatusCode::BAD_GATEWAY,
                format!("AyeFire Tier-3 response read error: {e}"),
            );
        }
    };

    let should_cache = !resp_bytes.is_empty() && resp_bytes.len() <= MAX_HTTPS_CACHE_BYTES;
    let action = if should_cache {
        inspect.https_cache_misses.fetch_add(1, Ordering::Relaxed);
        edge_cache.misses.fetch_add(1, Ordering::Relaxed);
        let _ = edge_cache.store_cached_with_headers(
            &https_cache_key,
            &full_url,
            &content_type,
            CacheResponseHeaders {
                content_encoding: content_encoding.as_deref(),
                access_control_allow_origin: acao_val.as_deref(),
                access_control_expose_headers: aceh_val.as_deref(),
            },
            &resp_bytes,
        );
        resp_builder = resp_builder.header("X-AyeFire-Cache", "HTTPS-MISS");
        "HTTPS-CACHED"
    } else {
        resp_builder = resp_builder.header("X-AyeFire-Cache", "PASS");
        "PASS"
    };

    radar
        .observe_request(TrafficObservation {
            host: host.to_string(),
            referer_host,
            method: method.to_string(),
            path_only: path_only.clone(),
            status: status_u16,
            req_bytes: req_bytes_len,
            resp_bytes: resp_bytes.len() as u64,
            content_type: content_type.clone(),
        })
        .await;

    inspect
        .record_event(InspectEvent {
            host: host.to_string(),
            method: method.to_string(),
            path_preview: truncate_path(&path_only, 60),
            status: status_u16,
            action: action.to_string(),
            content_type,
            size_bytes: resp_bytes.len() as u64,
            unix_secs: crate::pdf_forms::now_unix_secs(),
        })
        .await;

    resp_builder
        .body(full_body(resp_bytes))
        .unwrap_or_else(|_| error_response(StatusCode::INTERNAL_SERVER_ERROR, "resp build err".into()))
}

fn build_ca_params(common_name: &str) -> CertificateParams {
    let mut params = CertificateParams::default();
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, common_name);
    dn.push(DnType::OrganizationName, "AyeFire Sovereign Local Hardware");
    params.distinguished_name = dn;
    params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
    params.key_usages = vec![
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::CrlSign,
        KeyUsagePurpose::DigitalSignature,
    ];
    params
}

fn load_or_generate_local_ca(
    key_path: &Path,
    cert_path: &Path,
    common_name: &str,
) -> anyhow::Result<(String, String, Vec<u8>)> {
    if key_path.exists() && cert_path.exists() {
        let key_pem = fs::read_to_string(key_path)?;
        let cert_pem = fs::read_to_string(cert_path)?;
        let cert_der = CertificateDer::from_pem_slice(cert_pem.as_bytes())
            .map_err(|e| anyhow::anyhow!("invalid persisted CA cert PEM: {e}"))?
            .to_vec();
        let _ = KeyPair::from_pem(&key_pem)?;
        return Ok((key_pem, cert_pem, cert_der));
    }

    let key_pair = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256)?;
    let mut params = build_ca_params(common_name);
    let now = crate::pdf_forms::now_unix_secs();
    params.not_before = (UNIX_EPOCH + Duration::from_secs(now.saturating_sub(3600))).into();
    // 10-year hardware-local CA validity
    params.not_after = (UNIX_EPOCH + Duration::from_secs(now.saturating_add(315_360_000))).into();

    let cert = params.self_signed(&key_pair)?;
    let key_pem = key_pair.serialize_pem();
    let cert_pem = cert.pem();
    let cert_der = cert.der().to_vec();

    fs::write(key_path, &key_pem)?;
    fs::set_permissions(key_path, fs::Permissions::from_mode(0o600))?;
    fs::write(cert_path, &cert_pem)?;
    fs::set_permissions(cert_path, fs::Permissions::from_mode(0o644))?;

    Ok((key_pem, cert_pem, cert_der))
}

fn mint_host_server_config(
    host: &str,
    ca_common_name: &str,
    ca_key_pem: &str,
    ca_cert_der: &[u8],
    now_unix: u64,
) -> anyhow::Result<(rustls::ServerConfig, u64)> {
    let ca_kp = KeyPair::from_pem(ca_key_pem)?;
    let ca_params = build_ca_params(ca_common_name);
    let issuer = Issuer::from_params(&ca_params, &ca_kp);

    let leaf_kp = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256)?;
    let mut leaf_params = CertificateParams::default();
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, host);
    dn.push(DnType::OrganizationName, "AyeFire Local Tier-3 Leaf");
    leaf_params.distinguished_name = dn;
    leaf_params.is_ca = IsCa::NoCa;
    leaf_params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    leaf_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];

    if let Ok(ip) = host.parse::<IpAddr>() {
        leaf_params.subject_alt_names = vec![SanType::IpAddress(ip)];
    } else {
        let ia5 = host
            .to_string()
            .try_into()
            .map_err(|_| anyhow::anyhow!("invalid DNS SAN '{host}'"))?;
        leaf_params.subject_alt_names = vec![SanType::DnsName(ia5)];
    }

    let nb = now_unix.saturating_sub(CLOCK_SKEW_SECS);
    let na = now_unix.saturating_add(LEAF_LIFETIME_SECS);
    leaf_params.not_before = (UNIX_EPOCH + Duration::from_secs(nb)).into();
    leaf_params.not_after = (UNIX_EPOCH + Duration::from_secs(na)).into();

    let leaf_cert = leaf_params.signed_by(&leaf_kp, &issuer)?;
    let chain = vec![
        CertificateDer::from(leaf_cert.der().to_vec()),
        CertificateDer::from(ca_cert_der.to_vec()),
    ];
    let key_der = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(leaf_kp.serialize_der()));

    let mut cfg = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()?
    .with_no_client_auth()
    .with_single_cert(chain, key_der)?;
    cfg.alpn_protocols = vec![b"http/1.1".to_vec()];

    Ok((cfg, na))
}

fn url_has_sensitive_query(pq: &str) -> bool {
    let Some((_, q)) = pq.split_once('?') else {
        return false;
    };
    let low = q.to_ascii_lowercase();
    low.contains("token=")
        || low.contains("auth=")
        || low.contains("key=")
        || low.contains("sig=")
        || low.contains("password=")
        || low.contains("secret=")
}

fn truncate_path(path: &str, max: usize) -> String {
    if path.chars().count() <= max {
        path.to_string()
    } else {
        let mut s: String = path.chars().take(max.saturating_sub(1)).collect();
        s.push('…');
        s
    }
}

fn check_firefox_ca_installed(nickname: &str) -> bool {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/home/hue".to_string());
    let bases = [
        PathBuf::from(&home).join(".config/mozilla/firefox"),
        PathBuf::from(&home).join(".mozilla/firefox"),
    ];
    for base in bases {
        let Ok(rd) = fs::read_dir(&base) else {
            continue;
        };
        for entry in rd.flatten() {
            let db = entry.path().join("cert9.db");
            if db.exists() {
                if let Ok(out) = std::process::Command::new("certutil")
                    .args(["-L", "-d", &format!("sql:{}", entry.path().display())])
                    .output()
                {
                    if String::from_utf8_lossy(&out.stdout).contains(nickname) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_local_ca_and_mints_leaf_server_config() {
        let tmp = std::env::temp_dir().join(format!(
            "ayefire-ca-test-{}",
            crate::pdf_forms::now_unix_secs()
        ));
        let _ = fs::create_dir_all(&tmp);
        let key_p = tmp.join("ca.key");
        let crt_p = tmp.join("ca.crt");
        let (kp, cp, cder) =
            load_or_generate_local_ca(&key_p, &crt_p, "AyeFire Test Local CA").unwrap();
        assert!(kp.contains("BEGIN PRIVATE KEY"));
        assert!(cp.contains("BEGIN CERTIFICATE"));
        let now = crate::pdf_forms::now_unix_secs();
        let (_cfg, exp) =
            mint_host_server_config("example.org", "AyeFire Test Local CA", &kp, &cder, now)
                .unwrap();
        assert!(exp > now);
        assert!(is_privacy_sensitive_host("secure.chase.com"));
        assert!(is_privacy_sensitive_host("hue.id1.is"));
        assert!(!is_privacy_sensitive_host("news.ycombinator.com"));
        assert!(is_phoenix_upgradable_audio(
            "audio/mpeg; charset=binary",
            "stream.radio.org",
            "/live.mp3",
            "audio"
        ));
        // Never hijack Reddit DASH/MSE video audio segments or cache HLS/CMAF playlists!
        assert!(!is_phoenix_upgradable_audio(
            "audio/mp4",
            "v.redd.it",
            "/abc123/DASH_AUDIO_128.mp4",
            "empty"
        ));
        assert!(is_streaming_media_host_or_path(
            "v.redd.it",
            "/7i1urpm63f3i1/CMAF_480.m3u8"
        ));
        assert!(is_streaming_media_host_or_path(
            "cdn.example.org",
            "/video/chunk.m4s"
        ));
        assert!(!is_streaming_media_host_or_path(
            "styles.redditmedia.com",
            "/icon.webp"
        ));
        assert!(!is_phoenix_upgradable_audio(
            "text/html",
            "example.org",
            "/",
            "document"
        ));
        let _ = fs::remove_dir_all(&tmp);
    }
}
