#![deny(clippy::float_arithmetic)]
//! AyeFire BLAKE3 Sovereign Edge Cache & LAN Pull-Through HTTP Proxy (`:8792`).
//!
//! Gives every machine on Hue's LAN a fast, content-addressed NVMe cache for packages
//! (`pacman`, `apt`, `crates.io`, `pip`, OCI layers, GitHub releases, PDFs, docs, media)
//! that continues serving cached objects even when the WAN link goes completely offline!
//!
//! Trust & Privacy Invariants (`ayefire_onboard.rs` Tier 1):
//! - **Zero OS Root CA / Zero TLS MITM**: `CONNECT` requests are passed through as raw
//!   encrypted TCP tunnels (`copy_bidirectional`) — never decrypted or inspected.
//! - **Zero private session caching**: Any HTTP request carrying `Authorization` or `Cookie`,
//!   or upstream responses marked `Cache-Control: private` / `no-store`, bypass disk cache.
//! - **Exact integer accounting**: Reduced `p/q` hit ratios, BLAKE3 integrity keys, C64-bounded
//!   disk budget with deterministic oldest-first eviction.

use crate::phoenix_gateway::{format_reduced_pq, PhoenixGateway};
use crate::sanctuary_dns;
use crate::tls_inspect::{is_privacy_sensitive_host, InspectEvent, TlsInspectState};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::RwLock;

pub const DEFAULT_PROXY_PORT: u16 = 8792;
pub const DEFAULT_EDGE_CACHE_DIR: &str = ".cache/ayefire/edge";
/// Default edge cache disk budget: 64 GiB (bounded deterministic eviction).
const DEFAULT_MAX_MIB: u64 = 65_536;
/// Max single cached object size via edge HTTP cache: 256 MiB.
const MAX_SINGLE_OBJECT_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CachedObjectMeta {
    pub blake3_key: String,
    pub url: String,
    pub host: String,
    pub content_type: String,
    #[serde(default)]
    pub content_encoding: Option<String>,
    pub content_blake3: String,
    pub size_bytes: u64,
    pub cached_unix_secs: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct CacheEventView {
    pub blake3_short: String,
    pub host: String,
    pub url_preview: String,
    pub content_type: String,
    pub size_bytes: u64,
    pub status: String,
    pub unix_secs: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct EdgeCacheStatus {
    pub proxy_port: u16,
    pub cache_dir: String,
    pub max_mib: u64,
    pub entries: usize,
    pub total_bytes: u64,
    pub total_mib: u64,
    pub hits: u64,
    pub misses: u64,
    pub offline_serves: u64,
    pub bypassed: u64,
    pub connect_tunnels: u64,
    pub hit_ratio_pq: String,
    pub wan_bytes_saved: u64,
    pub zero_root_ca_passthrough: bool,
    pub recent: Vec<CacheEventView>,
}

pub struct EdgeFetchResult {
    pub status_code: u16,
    pub content_type: String,
    pub cache_status: &'static str,
    pub blake3_key: String,
    pub content_blake3: String,
    pub body: Vec<u8>,
}

pub struct EdgeCache {
    pub cache_dir: PathBuf,
    pub proxy_port: u16,
    pub max_bytes: u64,
    pub client: reqwest::Client,
    pub hits: AtomicU64,
    pub misses: AtomicU64,
    pub offline_serves: AtomicU64,
    pub bypassed: AtomicU64,
    pub connect_tunnels: AtomicU64,
    pub wan_bytes_saved: AtomicU64,
    pub recent: RwLock<VecDeque<CacheEventView>>,
}

impl EdgeCache {
    pub fn load_from_env() -> Arc<Self> {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/home/hue".to_string());
        let cache_dir = std::env::var("AYEFIRE_CACHE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(&home).join(DEFAULT_EDGE_CACHE_DIR));
        let proxy_port = std::env::var("AYEFIRE_PROXY_PORT")
            .ok()
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(DEFAULT_PROXY_PORT);
        let max_mib = std::env::var("AYEFIRE_CACHE_MAX_MIB")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(DEFAULT_MAX_MIB)
            .max(64);
        let _ = fs::create_dir_all(&cache_dir);

        let client = reqwest::Client::builder()
            .user_agent("AyeFire-Sovereign-Cache/1.0")
            .timeout(std::time::Duration::from_secs(45))
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        let (entries, bytes) = scan_edge_dir(&cache_dir);
        println!(
            "[EDGE-CACHE] BLAKE3 Sovereign Cache ready | dir={} | entries={} | used={}MiB / {}MiB | proxy_port={}",
            cache_dir.display(),
            entries,
            bytes / (1024 * 1024),
            max_mib,
            proxy_port
        );

        Arc::new(Self {
            cache_dir,
            proxy_port,
            max_bytes: max_mib.saturating_mul(1024 * 1024),
            client,
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            offline_serves: AtomicU64::new(0),
            bypassed: AtomicU64::new(0),
            connect_tunnels: AtomicU64::new(0),
            wan_bytes_saved: AtomicU64::new(0),
            recent: RwLock::new(VecDeque::with_capacity(24)),
        })
    }

    pub fn key_for_url(url: &str) -> String {
        blake3::hash(url.trim().as_bytes()).to_hex().to_string()
    }

    fn bin_path(&self, key: &str) -> PathBuf {
        self.cache_dir.join(format!("{key}.bin"))
    }

    fn meta_path(&self, key: &str) -> PathBuf {
        self.cache_dir.join(format!("{key}.meta.json"))
    }

    pub fn load_cached(&self, key: &str) -> Option<(CachedObjectMeta, Vec<u8>)> {
        let meta_str = fs::read_to_string(self.meta_path(key)).ok()?;
        let meta: CachedObjectMeta = serde_json::from_str(&meta_str).ok()?;
        let data = fs::read(self.bin_path(key)).ok()?;
        if data.len() as u64 != meta.size_bytes {
            return None;
        }
        Some((meta, data))
    }

    pub fn store_cached(
        &self,
        key: &str,
        url: &str,
        content_type: &str,
        body: &[u8],
    ) -> anyhow::Result<CachedObjectMeta> {
        self.store_cached_with_encoding(key, url, content_type, None, body)
    }

    pub fn store_cached_with_encoding(
        &self,
        key: &str,
        url: &str,
        content_type: &str,
        content_encoding: Option<&str>,
        body: &[u8],
    ) -> anyhow::Result<CachedObjectMeta> {
        if body.len() as u64 > MAX_SINGLE_OBJECT_BYTES {
            anyhow::bail!("object exceeds 256 MiB single-object edge cache limit");
        }
        let content_blake3 = blake3::hash(body).to_hex().to_string();
        let host = extract_host(url);
        let meta = CachedObjectMeta {
            blake3_key: key.to_string(),
            url: url.to_string(),
            host,
            content_type: content_type.to_string(),
            content_encoding: content_encoding.map(|s| s.to_string()),
            content_blake3,
            size_bytes: body.len() as u64,
            cached_unix_secs: crate::pdf_forms::now_unix_secs(),
        };

        let tmp_bin = self
            .cache_dir
            .join(format!("{key}.{}.tmp", meta.cached_unix_secs));
        fs::write(&tmp_bin, body)?;
        fs::rename(&tmp_bin, self.bin_path(key))?;
        let meta_json = serde_json::to_string_pretty(&meta)?;
        fs::write(self.meta_path(key), meta_json)?;

        self.enforce_budget_if_needed();
        Ok(meta)
    }

    fn enforce_budget_if_needed(&self) {
        let (_, total) = scan_edge_dir(&self.cache_dir);
        if total <= self.max_bytes {
            return;
        }
        let Ok(rd) = fs::read_dir(&self.cache_dir) else {
            return;
        };
        let mut items: Vec<(u64, u64, String)> = Vec::new();
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) == Some("bin") {
                if let (Some(stem), Ok(md)) = (
                    p.file_stem().and_then(|s| s.to_str()),
                    e.metadata(),
                ) {
                    let mtime = md
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    items.push((mtime, md.len(), stem.to_string()));
                }
            }
        }
        items.sort_by_key(|(mtime, _, _)| *mtime);
        let mut current = total;
        for (_, sz, stem) in items {
            if current <= self.max_bytes {
                break;
            }
            let _ = fs::remove_file(self.bin_path(&stem));
            let _ = fs::remove_file(self.meta_path(&stem));
            current = current.saturating_sub(sz);
        }
    }

    async fn push_event(&self, ev: CacheEventView) {
        let mut q = self.recent.write().await;
        if q.len() >= 24 {
            q.pop_back();
        }
        q.push_front(ev);
    }

    /// Fetch an HTTP/HTTPS URL through the BLAKE3 edge cache.
    pub async fn fetch_or_serve(
        &self,
        raw_url: &str,
        allow_cache: bool,
        force_refresh: bool,
    ) -> anyhow::Result<EdgeFetchResult> {
        let url = crate::phoenix_gateway::validate_stream_url(raw_url)
            .map_err(|e| anyhow::anyhow!(e))?;
        let key = Self::key_for_url(&url);

        if allow_cache && !force_refresh {
            if let Some((meta, body)) = self.load_cached(&key) {
                self.hits.fetch_add(1, Ordering::Relaxed);
                self.wan_bytes_saved
                    .fetch_add(meta.size_bytes, Ordering::Relaxed);
                self.push_event(CacheEventView {
                    blake3_short: key[..12].to_string(),
                    host: meta.host.clone(),
                    url_preview: truncate_preview(&url, 84),
                    content_type: meta.content_type.clone(),
                    size_bytes: meta.size_bytes,
                    status: "HIT".to_string(),
                    unix_secs: crate::pdf_forms::now_unix_secs(),
                })
                .await;
                return Ok(EdgeFetchResult {
                    status_code: 200,
                    content_type: meta.content_type,
                    cache_status: "HIT",
                    blake3_key: key,
                    content_blake3: meta.content_blake3,
                    body,
                });
            }
        }

        let resp = match self.client.get(&url).send().await {
            Ok(r) => r,
            Err(err) => {
                // Offline fallback: even on force_refresh, serve stale cached object if present!
                if let Some((meta, body)) = self.load_cached(&key) {
                    self.offline_serves.fetch_add(1, Ordering::Relaxed);
                    self.wan_bytes_saved
                        .fetch_add(meta.size_bytes, Ordering::Relaxed);
                    return Ok(EdgeFetchResult {
                        status_code: 200,
                        content_type: meta.content_type,
                        cache_status: "OFFLINE-HIT",
                        blake3_key: key,
                        content_blake3: meta.content_blake3,
                        body,
                    });
                }
                return Err(anyhow::anyhow!("upstream fetch failed: {err}"));
            }
        };

        let status_code = resp.status().as_u16();
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("application/octet-stream")
            .to_string();
        let cc = resp
            .headers()
            .get(reqwest::header::CACHE_CONTROL)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_ascii_lowercase();
        let response_private = cc.contains("no-store") || cc.contains("private");
        let body = resp.bytes().await?.to_vec();
        let content_blake3 = blake3::hash(&body).to_hex().to_string();

        if allow_cache
            && status_code == 200
            && !response_private
            && (body.len() as u64) <= MAX_SINGLE_OBJECT_BYTES
        {
            self.misses.fetch_add(1, Ordering::Relaxed);
            let _ = self.store_cached(&key, &url, &content_type, &body);
            self.push_event(CacheEventView {
                blake3_short: key[..12].to_string(),
                host: extract_host(&url),
                url_preview: truncate_preview(&url, 84),
                content_type: content_type.clone(),
                size_bytes: body.len() as u64,
                status: "MISS->CACHED".to_string(),
                unix_secs: crate::pdf_forms::now_unix_secs(),
            })
            .await;
            Ok(EdgeFetchResult {
                status_code,
                content_type,
                cache_status: "MISS",
                blake3_key: key,
                content_blake3,
                body,
            })
        } else {
            self.bypassed.fetch_add(1, Ordering::Relaxed);
            Ok(EdgeFetchResult {
                status_code,
                content_type,
                cache_status: "BYPASS",
                blake3_key: key,
                content_blake3,
                body,
            })
        }
    }

    pub async fn status(&self) -> EdgeCacheStatus {
        let (entries, total_bytes) = scan_edge_dir(&self.cache_dir);
        let hits = self.hits.load(Ordering::Relaxed);
        let misses = self.misses.load(Ordering::Relaxed);
        let recent = self.recent.read().await.iter().cloned().collect();

        EdgeCacheStatus {
            proxy_port: self.proxy_port,
            cache_dir: self.cache_dir.display().to_string(),
            max_mib: self.max_bytes / (1024 * 1024),
            entries,
            total_bytes,
            total_mib: total_bytes / (1024 * 1024),
            hits,
            misses,
            offline_serves: self.offline_serves.load(Ordering::Relaxed),
            bypassed: self.bypassed.load(Ordering::Relaxed),
            connect_tunnels: self.connect_tunnels.load(Ordering::Relaxed),
            hit_ratio_pq: format_reduced_pq(hits, hits.saturating_add(misses)),
            wan_bytes_saved: self.wan_bytes_saved.load(Ordering::Relaxed),
            zero_root_ca_passthrough: true,
            recent,
        }
    }

    pub async fn list_cached_objects(&self, limit: usize) -> Vec<CachedObjectMeta> {
        let Ok(rd) = fs::read_dir(&self.cache_dir) else {
            return Vec::new();
        };
        let mut items = Vec::new();
        for e in rd.flatten() {
            let p = e.path();
            if p.file_name()
                .and_then(|s| s.to_str())
                .map(|s| s.ends_with(".meta.json"))
                .unwrap_or(false)
            {
                if let Ok(raw) = fs::read_to_string(&p) {
                    if let Ok(meta) = serde_json::from_str::<CachedObjectMeta>(&raw) {
                        items.push(meta);
                    }
                }
            }
        }
        items.sort_by_key(|b| std::cmp::Reverse(b.cached_unix_secs));
        items.truncate(limit);
        items
    }
}

/// Spawn the LAN HTTP Pull-Through Cache Proxy (`0.0.0.0:8792`).
///
/// - HTTP `GET http://...`: served from NVMe BLAKE3 cache when public & cacheable.
/// - HTTPS `CONNECT host:port`:
///   1. Telemetry/ad/UniFi sinkhole check (blocks trackers even without decryption).
///   2. Tier 1 Zero-MITM encrypted passthrough (default, or when privacy-bypass matches).
///   3. Tier 3 Opt-In Hardware-Local Inspect (when enabled by user: BLAKE3 HTTPS caching +
///      live on-the-fly zero-float Phoenix Audio restoration for `audio/*` streams!).
pub async fn spawn_http_cache_proxy(
    cache: Arc<EdgeCache>,
    inspect: Arc<TlsInspectState>,
    phoenix: Arc<PhoenixGateway>,
) -> anyhow::Result<()> {
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), cache.proxy_port);
    let listener = TcpListener::bind(addr).await?;
    println!(
        "[EDGE-PROXY] LAN Cache + Tier-1/Tier-3 HTTPS Proxy listening on http://0.0.0.0:{}",
        cache.proxy_port
    );

    loop {
        let (stream, _peer) = match listener.accept().await {
            Ok(v) => v,
            Err(_) => continue,
        };
        let c = Arc::clone(&cache);
        let i = Arc::clone(&inspect);
        let p = Arc::clone(&phoenix);
        tokio::spawn(async move {
            let _ = handle_proxy_conn(stream, c, i, p).await;
        });
    }
}

async fn handle_proxy_conn(
    stream: TcpStream,
    cache: Arc<EdgeCache>,
    inspect: Arc<TlsInspectState>,
    phoenix: Arc<PhoenixGateway>,
) -> anyhow::Result<()> {
    let mut reader = BufReader::new(stream);
    let mut req_line = String::new();
    let n = reader.read_line(&mut req_line).await?;
    if n == 0 {
        return Ok(());
    }
    let parts: Vec<&str> = req_line.split_whitespace().collect();
    if parts.len() < 3 {
        let mut s = reader.into_inner();
        let _ = s
            .write_all(b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n")
            .await;
        return Ok(());
    }
    let method = parts[0];
    let target = parts[1];

    // Read headers (bounded to 64 headers)
    let mut has_private_hdr = false;
    let mut header_count = 0usize;
    loop {
        let mut line = String::new();
        let r = reader.read_line(&mut line).await?;
        if r == 0 || line == "\r\n" || line == "\n" {
            break;
        }
        header_count += 1;
        if header_count > 64 {
            break;
        }
        let low = line.to_ascii_lowercase();
        if low.starts_with("authorization:") || low.starts_with("cookie:") {
            has_private_hdr = true;
        }
    }

    let mut client_stream = reader.into_inner();

    if method.eq_ignore_ascii_case("CONNECT") {
        let Some((host, port)) = parse_connect_target(target) else {
            let _ = client_stream
                .write_all(b"HTTP/1.1 403 Forbidden\r\nConnection: close\r\n\r\n")
                .await;
            return Ok(());
        };

        // 1. Telemetry / Ad / Ubiquiti phone-home sinkhole (Zero-MITM hostname block!)
        if inspect.sinkhole_connect_enabled.load(Ordering::Relaxed)
            && sanctuary_dns::is_telemetry_sinkholed(&host.to_ascii_lowercase())
        {
            inspect.connect_blocked.fetch_add(1, Ordering::Relaxed);
            inspect
                .record_event(InspectEvent {
                    host: host.clone(),
                    method: "CONNECT".to_string(),
                    path_preview: format!(":{port}"),
                    status: 403,
                    action: "SINKHOLE-BLOCKED".to_string(),
                    content_type: "telemetry/blocked".to_string(),
                    size_bytes: 0,
                    unix_secs: crate::pdf_forms::now_unix_secs(),
                })
                .await;
            let _ = client_stream
                .write_all(b"HTTP/1.1 403 Forbidden\r\nX-AyeFire-Block: Telemetry-Sinkhole\r\nConnection: close\r\n\r\n")
                .await;
            return Ok(());
        }

        // 2. Opt-In Tier 3 Hardware-Local HTTPS Inspection (when enabled by user)
        let do_inspect = inspect.inspect_enabled.load(Ordering::Relaxed) && port == 443;
        let privacy_bypass = inspect.privacy_bypass_enabled.load(Ordering::Relaxed)
            && is_privacy_sensitive_host(&host);

        if do_inspect && !privacy_bypass {
            client_stream
                .write_all(b"HTTP/1.1 200 Connection Established\r\nProxy-Agent: AyeFire-Tier3-Local-Inspect\r\n\r\n")
                .await?;
            let _ = crate::tls_inspect::serve_inspected_tls_tunnel(
                client_stream,
                host,
                port,
                inspect,
                cache,
                phoenix,
            )
            .await;
            return Ok(());
        }

        if do_inspect && privacy_bypass {
            inspect
                .https_privacy_bypassed
                .fetch_add(1, Ordering::Relaxed);
            inspect
                .record_event(InspectEvent {
                    host: host.clone(),
                    method: "CONNECT".to_string(),
                    path_preview: format!(":{port}"),
                    status: 200,
                    action: "PRIVACY-BYPASS".to_string(),
                    content_type: "tls/passthrough".to_string(),
                    size_bytes: 0,
                    unix_secs: crate::pdf_forms::now_unix_secs(),
                })
                .await;
        }

        // 3. Tier 1 Zero-MITM encrypted TCP passthrough
        let dest = format!("{host}:{port}");
        match TcpStream::connect(&dest).await {
            Ok(mut upstream) => {
                cache.connect_tunnels.fetch_add(1, Ordering::Relaxed);
                client_stream
                    .write_all(b"HTTP/1.1 200 Connection Established\r\nProxy-Agent: AyeFire-Zero-MITM\r\n\r\n")
                    .await?;
                let _ = tokio::io::copy_bidirectional(&mut client_stream, &mut upstream).await;
            }
            Err(_) => {
                let _ = client_stream
                    .write_all(b"HTTP/1.1 502 Bad Gateway\r\nConnection: close\r\n\r\n")
                    .await;
            }
        }
        return Ok(());
    }

    if !method.eq_ignore_ascii_case("GET") {
        let _ = client_stream
            .write_all(b"HTTP/1.1 405 Method Not Allowed\r\nConnection: close\r\n\r\n")
            .await;
        return Ok(());
    }

    match cache.fetch_or_serve(target, !has_private_hdr, false).await {
        Ok(res) => {
            let hdr = format!(
                "HTTP/1.1 {} OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nX-AyeFire-Cache: {}\r\nX-AyeFire-Blake3: {}\r\nConnection: close\r\n\r\n",
                res.status_code,
                res.content_type,
                res.body.len(),
                res.cache_status,
                res.content_blake3
            );
            client_stream.write_all(hdr.as_bytes()).await?;
            client_stream.write_all(&res.body).await?;
        }
        Err(e) => {
            let body = format!("AyeFire Edge Proxy error: {e}\n");
            let hdr = format!(
                "HTTP/1.1 502 Bad Gateway\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            client_stream.write_all(hdr.as_bytes()).await?;
            client_stream.write_all(body.as_bytes()).await?;
        }
    }
    Ok(())
}

pub fn parse_connect_target(target: &str) -> Option<(String, u16)> {
    let (host, port_s) = target.trim().rsplit_once(':')?;
    let port = port_s.parse::<u16>().ok()?;
    let h_low = host.to_ascii_lowercase();
    if h_low.is_empty()
        || h_low == "localhost"
        || h_low.starts_with("127.")
        || h_low == "::1"
        || h_low.starts_with("169.254.")
    {
        return None;
    }
    // Restrict CONNECT tunnels to standard web/git/TLS ports
    if !matches!(port, 443 | 80 | 8443 | 8080 | 9418) {
        return None;
    }
    Some((host.to_string(), port))
}

fn extract_host(url: &str) -> String {
    let after = url
        .split_once("://")
        .map(|(_, r)| r)
        .unwrap_or(url);
    after
        .split('/')
        .next()
        .unwrap_or("unknown")
        .to_string()
}

fn truncate_preview(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

fn scan_edge_dir(dir: &Path) -> (usize, u64) {
    let Ok(rd) = fs::read_dir(dir) else {
        return (0, 0);
    };
    let mut count = 0usize;
    let mut bytes = 0u64;
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().and_then(|s| s.to_str()) == Some("bin") {
            if let Ok(md) = e.metadata() {
                count += 1;
                bytes = bytes.saturating_add(md.len());
            }
        }
    }
    (count, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edge_cache_stores_verifies_blake3_and_connect_guard() {
        let tmp = std::env::temp_dir().join(format!(
            "ayefire-edge-test-{}",
            crate::pdf_forms::now_unix_secs()
        ));
        let _ = fs::create_dir_all(&tmp);
        let cache = EdgeCache {
            cache_dir: tmp.clone(),
            proxy_port: DEFAULT_PROXY_PORT,
            max_bytes: 10 * 1024 * 1024,
            client: reqwest::Client::new(),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            offline_serves: AtomicU64::new(0),
            bypassed: AtomicU64::new(0),
            connect_tunnels: AtomicU64::new(0),
            wan_bytes_saved: AtomicU64::new(0),
            recent: RwLock::new(VecDeque::new()),
        };
        let url = "http://mirror.example.org/arch/pkg.zst";
        let key = EdgeCache::key_for_url(url);
        let payload = b"sovereign-cached-package-bytes";
        let meta = cache
            .store_cached(&key, url, "application/zstd", payload)
            .unwrap();
        assert_eq!(meta.size_bytes, payload.len() as u64);
        let (loaded, bytes) = cache.load_cached(&key).unwrap();
        assert_eq!(bytes, payload);
        assert_eq!(loaded.content_blake3, meta.content_blake3);

        assert_eq!(
            parse_connect_target("github.com:443"),
            Some(("github.com".to_string(), 443))
        );
        assert_eq!(parse_connect_target("127.0.0.1:443"), None);
        assert_eq!(parse_connect_target("github.com:22"), None);
        let _ = fs::remove_dir_all(&tmp);
    }
}
